use super::*;
use pretty_assertions::assert_eq;

#[test]
fn agents_help_describes_noninteractive_modes() {
    use clap::CommandFactory;
    let mut cli = MultitoolCli::command();
    let agents = cli.find_subcommand_mut("agents").unwrap();
    let help = agents
        .render_long_help()
        .to_string()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(help);
}

#[test]
fn json_flags_preserve_interactive_default_and_require_json_for_watch() {
    for (args, expected) in [
        (vec!["codex", "agents"], (false, false)),
        (vec!["codex", "agents", "--json"], (true, false)),
        (vec!["codex", "agents", "--json", "--watch"], (true, true)),
    ] {
        let cli = MultitoolCli::try_parse_from(args).unwrap();
        let Some(Subcommand::Agents(options)) = cli.subcommand else {
            panic!("expected agents");
        };
        assert_eq!((options.json, options.watch), expected);
    }
    assert_eq!(
        MultitoolCli::try_parse_from(["codex", "agents", "--watch"])
            .unwrap_err()
            .kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
}

#[test]
fn json_accepts_existing_endpoint_positions() {
    for flag in ["--remote", "--local"] {
        for args in [
            vec!["codex", flag, "ws://localhost:5000", "agents", "--json"],
            vec!["codex", "agents", "--json", flag, "ws://localhost:5000"],
        ] {
            let cli = MultitoolCli::try_parse_from(args).unwrap();
            let Some(Subcommand::Agents(options)) = cli.subcommand else {
                panic!("expected agents");
            };
            assert_eq!(
                options
                    .remote
                    .local
                    .or(options.remote.remote)
                    .or(cli.remote.local)
                    .or(cli.remote.remote),
                Some("ws://localhost:5000".into())
            );
        }
    }
}

#[test]
fn focus_accepts_endpoint_positions_and_conflicts_with_json_and_watch() {
    for flag in ["--remote", "--local"] {
        for args in [
            vec![
                "codex",
                flag,
                "ws://localhost:5000",
                "agents",
                "--focus",
                "session",
            ],
            vec![
                "codex",
                "agents",
                "--focus",
                "session",
                flag,
                "ws://localhost:5000",
            ],
        ] {
            let cli = MultitoolCli::try_parse_from(args).unwrap();
            let Some(Subcommand::Agents(options)) = cli.subcommand else {
                panic!("expected agents")
            };
            assert_eq!(
                (options.focus.as_deref(), options.json, options.watch),
                (Some("session"), false, false)
            );
        }
    }
    for flag in ["--json", "--watch"] {
        assert_eq!(
            MultitoolCli::try_parse_from(["codex", "agents", "--focus", "session", flag])
                .unwrap_err()
                .kind(),
            clap::error::ErrorKind::ArgumentConflict
        );
    }
    assert!(MultitoolCli::try_parse_from(["codex", "agents", "--focus"]).is_err());
}
