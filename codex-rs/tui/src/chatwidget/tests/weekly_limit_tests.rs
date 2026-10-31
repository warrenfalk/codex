use super::*;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn set_weekly_limit(chat: &mut ChatWidget, used_percent: f64, time_remaining_percent: Option<f64>) {
    chat.rate_limit_snapshots_by_limit_id.insert(
        "codex".to_string(),
        RateLimitSnapshotDisplay {
            limit_name: "codex".to_string(),
            normal_model_slug: None,
            captured_at: chrono::Local::now(),
            primary: Some(RateLimitWindowDisplay {
                used_percent,
                time_remaining: None,
                time_remaining_percent,
                resets_at: None,
                window_minutes: Some(7 * 24 * 60),
            }),
            secondary: Some(RateLimitWindowDisplay {
                used_percent: 10.0,
                time_remaining: None,
                time_remaining_percent: Some(20.0),
                resets_at: None,
                window_minutes: Some(5 * 60),
            }),
            credits: None,
            individual_limit: None,
        },
    );
}

#[tokio::test]
async fn weekly_limit_items_share_window_selection_and_handle_missing_time() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.local_settings.tui.status_line = Some(vec![
        "weekly-limit".to_string(),
        "weekly-limit-bar".to_string(),
    ]);

    for (used, time, expected) in [
        (
            6.0,
            Some(97.0),
            "limit 94%, time 97% · [██████████████████│░]",
        ),
        (
            45.0,
            Some(48.0),
            "limit 55%, time 48% · [█████████│█░░░░░░░░░]",
        ),
        (
            80.0,
            Some(70.0),
            "limit 20%, time 70% · [████░░░░░░░░░│░░░░░░]",
        ),
        (50.0, None, "limit 50% · [██████████░░░░░░░░░░]"),
        (
            -5.0,
            Some(110.0),
            "limit 100%, time 100% · [███████████████████│]",
        ),
        (
            110.0,
            Some(-5.0),
            "limit 0%, time 0% · [│░░░░░░░░░░░░░░░░░░░]",
        ),
    ] {
        set_weekly_limit(&mut chat, used, time);
        chat.refresh_status_line();
        assert_eq!(status_line_text(&chat).as_deref(), Some(expected));
    }
    assert!(drain_insert_history(&mut rx).is_empty());

    chat.rate_limit_snapshots_by_limit_id.clear();
    chat.refresh_status_line();
    assert_eq!(status_line_text(&chat), None);

    set_weekly_limit(
        &mut chat, /*used_percent*/ 50.0, /*time_remaining_percent*/ None,
    );
    let snapshot = chat
        .rate_limit_snapshots_by_limit_id
        .get_mut("codex")
        .unwrap();
    snapshot.primary = snapshot.secondary.take();
    chat.refresh_status_line();
    assert_eq!(status_line_text(&chat), None);
    let preview = chat.status_surface_preview_data();
    assert_eq!(
        preview.status_line_for_items(
            [StatusLineItem::WeeklyLimit, StatusLineItem::WeeklyLimitBar],
            /*use_theme_colors*/ true,
        ),
        None,
    );
}

#[tokio::test]
async fn weekly_limit_bar_footer_snapshots() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.show_welcome_banner = false;
    set_weekly_limit(
        &mut chat,
        /*used_percent*/ 45.0,
        /*time_remaining_percent*/ Some(48.0),
    );

    let mut snapshots = Vec::new();
    for (name, items, width) in [
        ("weekly_limit_bar_footer", vec!["weekly-limit-bar"], 80),
        (
            "weekly_limit_bar_footer_narrow",
            vec!["weekly-limit-bar"],
            24,
        ),
        (
            "weekly_limit_percentages_and_bar_footer",
            vec!["weekly-limit", "weekly-limit-bar"],
            80,
        ),
    ] {
        chat.local_settings.tui.status_line = Some(items.into_iter().map(str::to_string).collect());
        chat.refresh_status_line();
        let height = chat.desired_height(width);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("create terminal");
        terminal
            .draw(|frame| chat.render(frame.area(), frame.buffer_mut()))
            .expect("draw weekly limit footer");
        snapshots.push(format!(
            "{name}:\n{}",
            normalized_backend_snapshot(terminal.backend()),
        ));
    }
    assert_chatwidget_snapshot!("weekly_limit_bar_footers", snapshots.join("\n\n"));
}
