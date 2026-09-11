use crate::chatwidget::tests::helpers::make_chatwidget_manual_with_sender;
use crate::chatwidget::tests::helpers::render_bottom_popup;

#[tokio::test]
async fn composer_popup_does_not_block_rewrite_shortcut() {
    let (mut chat, _app_event_tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.bottom_pane
        .set_composer_text("/review".to_string(), Vec::new(), Vec::new());

    assert!(!chat.no_modal_or_popup_active());
    assert!(chat.can_start_prompt_rewrite());
}

#[tokio::test]
async fn successful_rewrite_footer_snapshot() {
    let (mut chat, _app_event_tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.bottom_pane.set_composer_text(
        "Please make this clearer".to_string(),
        Vec::new(),
        Vec::new(),
    );
    chat.show_prompt_rewrite_flash(
        "Prompt rewritten — undo restores the original.",
        /*is_error*/ false,
    );

    insta::assert_snapshot!(
        "prompt_rewrite_success_footer",
        render_bottom_popup(&chat, /*width*/ 72)
    );
}
