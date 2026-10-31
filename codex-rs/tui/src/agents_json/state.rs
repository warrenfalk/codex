//! JSON connection adapter for the shared agents model.

use crate::agents_model::AgentsModel;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::EventFirehoseResponse;
use codex_app_server_protocol::RequestId;
use std::future::Future;
use std::io;
use std::time::Duration;
use uuid::Uuid;

pub(super) struct Observer<'a> {
    pub(super) client: AppServerClient,
    pub(super) model: &'a mut AgentsModel,
}

impl Observer<'_> {
    pub(super) async fn receive(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.client.next_event().await;
            if self.observe(event)? {
                return Ok(());
            }
        }
    }

    fn observe(&mut self, event: Option<AppServerEvent>) -> anyhow::Result<bool> {
        match event {
            Some(AppServerEvent::ServerNotification(notification)) => {
                Ok(self.model.observe(&notification))
            }
            Some(AppServerEvent::ServerRequest(_)) => {
                anyhow::bail!("passive observer received an interactive request")
            }
            Some(AppServerEvent::Lagged { .. }) | None => Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "agents event stream lost synchronization",
            )
            .into()),
            Some(AppServerEvent::Disconnected { message }) => {
                Err(io::Error::new(io::ErrorKind::ConnectionReset, message).into())
            }
        }
    }

    async fn observe_while<T>(
        &mut self,
        response: impl Future<Output = anyhow::Result<T>>,
    ) -> anyhow::Result<T> {
        tokio::pin!(response);
        loop {
            tokio::select! {
                biased;
                event = self.client.next_event() => { self.observe(event)?; },
                result = &mut response => return result,
            }
        }
    }

    pub(super) async fn subscribe(&mut self) -> anyhow::Result<()> {
        let handle = self.client.request_handle();
        self.observe_while(async {
            let _: EventFirehoseResponse = tokio::time::timeout(
                Duration::from_secs(/*secs*/ 30),
                handle.request_typed(ClientRequest::EventFirehose {
                    request_id: RequestId::String(Uuid::new_v4().to_string()),
                    params: None,
                }),
            )
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "agents subscription timed out")
            })??;
            Ok(())
        })
        .await
    }

    pub(super) async fn synchronize(&mut self) -> anyhow::Result<()> {
        while let Some(request) = self.model.begin_refresh() {
            let id = request.id;
            let result = self
                .observe_while(request.fetch(self.client.request_handle()))
                .await;
            self.model.finish_refresh(id, result)?;
        }
        Ok(())
    }
}
