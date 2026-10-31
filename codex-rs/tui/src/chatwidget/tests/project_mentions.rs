use super::*;
use crate::legacy_core::config::ConfigOverrides;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn selected_project_is_structured_and_plain_text_is_not_an_attachment() {
    for selected in [false, true] {
        let (mut chat, _events, mut ops) = make_chatwidget_manual(/*model_override*/ None).await;
        chat.thread_id = Some(ThreadId::new());
        let directory = tempfile::tempdir().unwrap();
        let path = crate::project_mentions::project_target(&directory.path().to_string_lossy());
        let name = "project-other";
        let text = format!("Compare @{name}");
        let bindings = if selected {
            vec![MentionBinding {
                sigil: '@',
                mention: name.to_string(),
                path: path.clone(),
            }]
        } else {
            Vec::new()
        };
        chat.submit_user_message(UserMessage {
            text: text.clone(),
            local_images: Vec::new(),
            remote_image_urls: Vec::new(),
            text_elements: Vec::new(),
            mention_bindings: bindings.clone(),
        });
        let Op::UserTurn { items, .. } = next_submit_op(&mut ops) else {
            panic!("expected turn");
        };
        let mut expected = vec![UserInput::Text {
            text: text.clone(),
            text_elements: Vec::new(),
        }];
        if selected {
            expected.push(UserInput::Mention {
                name: name.to_string(),
                path,
            });
        }
        assert_eq!(items, expected);
        assert_eq!(mention_bindings_from_user_inputs(&items, &text), bindings);
    }
}

#[tokio::test]
async fn new_project_attachment_waits_for_the_running_turn() {
    let (mut chat, _events, mut ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.turn_lifecycle.agent_turn_running = true;
    let directory = tempfile::tempdir().unwrap();
    let user_message = UserMessage {
        text: "Use @project-other".to_string(),
        local_images: Vec::new(),
        remote_image_urls: Vec::new(),
        text_elements: Vec::new(),
        mention_bindings: vec![MentionBinding {
            sigil: '@',
            mention: "project-other".to_string(),
            path: crate::project_mentions::project_target(&directory.path().to_string_lossy()),
        }],
    };
    chat.submit_user_message(user_message.clone());
    assert_eq!(
        chat.input_queue
            .queued_user_messages
            .front()
            .unwrap()
            .user_message,
        user_message
    );
    assert!(ops.try_recv().is_err());
}

#[tokio::test]
async fn project_attachment_feedback_snapshot() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.add_info_message(
        "Attached project directories for this session: /work/other-project".to_string(),
        Some("The current permission profile and denied paths still apply.".to_string()),
    );
    let rendered = drain_insert_history(&mut events)
        .into_iter()
        .flatten()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert_chatwidget_snapshot!("project_attachment_feedback", rendered);
}

#[tokio::test]
async fn project_roots_preserve_named_profile_denials_and_network_policy() {
    let (mut chat, _events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    let home = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let root = other.path().canonicalize().unwrap().abs();
    let denied = root.join("private");
    let denied_key = serde_json::to_string(&denied.to_string_lossy()).unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            r#"
default_permissions = "projects"
[permissions.projects]
extends = ":workspace"
[permissions.projects.filesystem]
{denied_key} = "deny"
"#
        ),
    )
    .unwrap();
    chat.config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .harness_overrides(ConfigOverrides {
            cwd: Some(home.path().to_path_buf()),
            ..Default::default()
        })
        .build()
        .await
        .unwrap();
    let profile_before = chat.config.permissions.permission_profile().clone();
    let selection_before = chat.config.permissions.active_permission_profile();
    let mut roots = chat
        .config
        .permissions
        .user_visible_workspace_roots()
        .to_vec();
    roots.push(root.clone());
    chat.on_projects_attached(roots.clone(), std::slice::from_ref(&root));
    assert_eq!(
        chat.config.permissions.permission_profile(),
        &profile_before
    );
    assert_eq!(
        chat.config.permissions.active_permission_profile(),
        selection_before
    );
    assert_eq!(
        chat.config.permissions.user_visible_workspace_roots(),
        roots
    );
    let profile_after = chat.config.permissions.effective_permission_profile();
    assert_eq!(
        profile_after.network_sandbox_policy(),
        profile_before.network_sandbox_policy()
    );
    let filesystem = profile_after.file_system_sandbox_policy();
    assert!(filesystem.can_write_local_path_with_cwd(root.as_path(), chat.config.cwd.as_path()));
    assert!(!filesystem.can_read_local_path_with_cwd(denied.as_path(), chat.config.cwd.as_path()));
}
