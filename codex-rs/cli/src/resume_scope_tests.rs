use crate::MultitoolCli;
use crate::tests::finalize_resume_from_args;
use clap::Parser;
use codex_tui::LastSessionScope;
use pretty_assertions::assert_eq;

#[test]
fn resume_help_describes_directory_scopes() {
    let help = MultitoolCli::try_parse_from(["codex", "resume", "--help"])
        .expect_err("help should exit before launching the TUI");
    assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
    let help = help
        .to_string()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(help);
}

#[test]
fn latest_session_flags_select_scope_and_accept_a_prompt() {
    for (flag, scope) in [
        ("--last", LastSessionScope::Cwd),
        ("--last-for-repo", LastSessionScope::Repo),
    ] {
        for prompt in [None, Some("continue here")] {
            let mut args = vec!["codex", "resume", flag, "--include-non-interactive"];
            args.extend(prompt);
            let cli = finalize_resume_from_args(&args);
            assert_eq!(
                (
                    cli.resume_picker,
                    cli.resume_last,
                    cli.resume_session_id,
                    cli.prompt.as_deref(),
                    cli.resume_show_all,
                    cli.resume_include_non_interactive,
                ),
                (false, Some(scope), None, prompt, false, true),
            );
        }
    }
}

#[test]
fn last_for_repo_rejects_conflicting_selection_options() {
    for extra in [vec!["--last"], vec!["--all"], vec!["session-id", "prompt"]] {
        let mut args = vec!["codex", "resume", "--last-for-repo"];
        args.extend(extra);
        let error = MultitoolCli::try_parse_from(args).expect_err("conflicting scope");
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
}

#[test]
fn last_all_retains_global_lookup() {
    let cli = finalize_resume_from_args(&["codex", "resume", "--last", "--all"]);
    assert_eq!(
        (cli.resume_last, cli.resume_show_all),
        (Some(LastSessionScope::Cwd), true),
    );
}
