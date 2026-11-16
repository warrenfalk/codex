use super::*;
use codex_config::types::UriBasedFileOpener;
use pretty_assertions::assert_eq;

#[test]
fn parked_reply_uses_configured_file_opener() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("src/lib.rs");
    std::fs::create_dir_all(file.parent().expect("source directory"))?;
    std::fs::write(&file, "fn example() {}\n")?;
    let cwd = AbsolutePathBuf::try_from(directory.path().to_path_buf())?;
    let mut pane = SideCompanion::empty(cwd, UriBasedFileOpener::VsCode);
    pane.write_answer(/*index*/ 0, "See `src/lib.rs:7`.".into(), &[]);

    let lines = pane.cells[0].display_hyperlink_lines(/*width*/ 80);
    let targets = lines
        .iter()
        .flat_map(|line| &line.hyperlinks)
        .map(|link| {
            link.destination
                .strip_prefix("vscode://file/")
                .expect("configured VS Code opener")
                .rsplit('/')
                .next()
                .expect("file target")
        })
        .collect::<Vec<_>>();
    assert_eq!(targets, vec!["lib.rs:7"]);
    let visible = lines
        .iter()
        .map(|line| line.line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(visible);
    Ok(())
}
