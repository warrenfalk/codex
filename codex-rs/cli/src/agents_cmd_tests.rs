use super::*;
use pretty_assertions::assert_eq;

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
