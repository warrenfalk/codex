use super::*;
use tokio::time::Instant;

impl ThreadControlService {
    pub(crate) async fn request(
        self: &Arc<Self>,
        controller_id: &str,
        params: ThreadControlRequestParams,
    ) -> Result<ThreadControlState, JSONRPCErrorError> {
        validate_label(&params.request_id)?;
        if params.reason.len() > 1024 || params.reason.chars().any(|c| c.is_control() && c != '\n')
        {
            return Err(invalid_request(
                "reason must be at most 1024 bytes without terminal control characters",
            ));
        }
        let (id, mut entry) = self.entry(&params.thread_id).await?;
        if let Some(request) = &entry.request
            && request.id == params.request_id
        {
            if request.controller_id != controller_id || request.reason != params.reason {
                return Err(invalid_request(
                    "request ID already used with different parameters",
                ));
            }
            return Ok(self.snapshot(id, &entry).await);
        }
        if let Some(grant) = &entry.grant {
            if grant.controller_id == controller_id {
                return Ok(self.snapshot(id, &entry).await);
            }
            return Err(invalid_request("thread is controlled by another agent"));
        }
        if entry
            .request
            .as_ref()
            .is_some_and(|r| r.status == ThreadControlRequestStatus::Pending)
        {
            return Err(invalid_request("a control request is already pending"));
        }
        self.persist_thread(id).await?;
        let current = self.snapshot(id, &entry).await;
        let now = chrono::Utc::now().timestamp();
        let immediate = !entry.locked && !current.interactive_attached && !current.turn_running;
        entry.request = Some(ThreadControlRequest {
            id: params.request_id.clone(),
            controller_id: controller_id.to_owned(),
            reason: params.reason,
            status: ThreadControlRequestStatus::Pending,
            requested_at: now,
            auto_grant_at: (!entry.locked && !immediate).then_some(now + 60),
        });
        if immediate {
            entry.grant(controller_id.to_owned());
        } else if !entry.locked {
            let deadline = Instant::now() + AUTO_GRANT_DELAY;
            let service = Arc::downgrade(self);
            tokio::spawn(async move {
                tokio::time::sleep_until(deadline).await;
                let Some(service) = service.upgrade() else {
                    return;
                };
                if let Err(error) = service.auto_grant(id, &params.request_id).await {
                    tracing::error!(?error, "failed to grant thread control after timeout");
                }
            });
        }
        Ok(self.publish(id, &entry).await)
    }

    async fn auto_grant(&self, id: ThreadId, request_id: &str) -> Result<(), JSONRPCErrorError> {
        let (_, mut entry) = self.entry(&id.to_string()).await?;
        if !entry.locked
            && entry.grant.is_none()
            && let Some(request) = &entry.request
            && request.id == request_id
            && request.status == ThreadControlRequestStatus::Pending
            && request.auto_grant_at.is_some()
        {
            let controller_id = request.controller_id.clone();
            entry.grant(controller_id);
            self.publish(id, &entry).await;
        }
        Ok(())
    }

    pub(crate) async fn update(
        &self,
        params: ThreadControlUpdateParams,
    ) -> Result<ThreadControlState, JSONRPCErrorError> {
        let (id, mut entry) = self.entry(&params.thread_id).await?;
        let approve = matches!(params.action, ThreadControlAction::Approve { .. });
        match params.action {
            ThreadControlAction::Approve { request_id }
            | ThreadControlAction::Deny { request_id } => {
                let request = entry
                    .request
                    .as_ref()
                    .filter(|request| {
                        request.id == request_id
                            && request.status == ThreadControlRequestStatus::Pending
                    })
                    .ok_or_else(|| invalid_request("request is no longer pending"))?;
                let controller_id = request.controller_id.clone();
                if approve {
                    entry.grant(controller_id);
                } else {
                    entry.revoke();
                }
            }
            ThreadControlAction::Grant { controller_id } => {
                validate_label(&controller_id)?;
                if entry
                    .grant
                    .as_ref()
                    .is_some_and(|g| g.controller_id != controller_id)
                {
                    return Err(invalid_request(
                        "revoke the current controller before granting another",
                    ));
                }
                self.persist_thread(id).await?;
                entry.revoke();
                entry.request = None;
                entry.grant(controller_id);
            }
            ThreadControlAction::Revoke => {
                entry.revoke();
            }
            ThreadControlAction::Lock => {
                self.persist_thread(id).await?;
                // Fail without changing state if the durable lock cannot be written.
                tokio::fs::create_dir_all(&self.lock_directory)
                    .await
                    .map_err(|e| internal_error(format!("cannot persist thread lock: {e}")))?;
                tokio::fs::write(self.lock_directory.join(id.to_string()), b"")
                    .await
                    .map_err(|e| internal_error(format!("cannot persist thread lock: {e}")))?;
                entry.locked = true;
                entry.revoke();
            }
            ThreadControlAction::Unlock => {
                match tokio::fs::remove_file(self.lock_directory.join(id.to_string())).await {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(internal_error(format!("cannot remove thread lock: {e}")));
                    }
                }
                entry.locked = false;
                // Existing locked requests stay explicit-only; a fresh request gets a timeout.
            }
        }
        Ok(self.publish(id, &entry).await)
    }

    pub(crate) async fn release(
        &self,
        controller_id: &str,
        params: ThreadControlReleaseParams,
    ) -> Result<ThreadControlState, JSONRPCErrorError> {
        let (id, mut entry) = self.entry(&params.thread_id).await?;
        validate_grant(&entry, controller_id, &params.grant_id)?;
        entry.revoke();
        Ok(self.publish(id, &entry).await)
    }
}

pub(super) fn validate_grant(
    entry: &ControlEntry,
    controller_id: &str,
    grant_id: &str,
) -> Result<(), JSONRPCErrorError> {
    if !entry
        .grant
        .as_ref()
        .is_some_and(|grant| grant.id == grant_id && grant.controller_id == controller_id)
    {
        return Err(invalid_request(
            "thread control grant is missing, revoked, or belongs to another controller",
        ));
    }
    Ok(())
}
