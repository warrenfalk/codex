use std::num::NonZero;

use pretty_assertions::assert_eq;

use super::*;
use crate::FileSearchOptions;
use crate::run;

#[test]
fn default_search_does_not_globally_crawl_gitignored_files() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    fs::create_dir_all(repo.path().join("ignored-dir")).unwrap();
    fs::write(repo.path().join(".gitignore"), "ignored-dir/\n").unwrap();
    fs::write(
        repo.path().join("ignored-dir").join("generated.txt"),
        "generated",
    )
    .unwrap();

    let results = run(
        "generated",
        vec![repo.path().to_path_buf()],
        FileSearchOptions {
            limit: NonZero::new(20).unwrap(),
            threads: NonZero::new(2).unwrap(),
            compute_indices: false,
            ..Default::default()
        },
        /*cancel_flag*/ None,
    )
    .expect("run ok");

    assert_eq!(results.matches, Vec::new());
}

#[test]
fn path_local_matches_include_ignored_entries_without_descendants() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    fs::create_dir_all(repo.path().join("ignored-dir").join("nested")).unwrap();
    fs::write(repo.path().join(".gitignore"), "ignored-dir/\n").unwrap();
    fs::write(
        repo.path().join("ignored-dir").join("generated.txt"),
        "generated",
    )
    .unwrap();
    fs::write(
        repo.path()
            .join("ignored-dir")
            .join("nested")
            .join("deep.txt"),
        "deep",
    )
    .unwrap();

    let options = FileSearchOptions {
        limit: NonZero::new(20).unwrap(),
        threads: NonZero::new(2).unwrap(),
        compute_indices: false,
        ..Default::default()
    };
    let ignored_dir_results = run(
        "ignored-dir",
        vec![repo.path().to_path_buf()],
        options.clone(),
        /*cancel_flag*/ None,
    )
    .expect("run ok");

    assert_eq!(
        ignored_dir_results.matches,
        vec![FileMatch {
            score: PATH_LOCAL_MATCH_SCORE,
            path: PathBuf::from("ignored-dir"),
            match_type: MatchType::Directory,
            root: repo.path().to_path_buf(),
            indices: None,
        }]
    );

    let child_results = run(
        "ignored-dir/",
        vec![repo.path().to_path_buf()],
        options,
        /*cancel_flag*/ None,
    )
    .expect("run ok");

    assert_eq!(
        child_results.matches,
        vec![
            FileMatch {
                score: PATH_LOCAL_MATCH_SCORE,
                path: Path::new("ignored-dir").join("generated.txt"),
                match_type: MatchType::File,
                root: repo.path().to_path_buf(),
                indices: None,
            },
            FileMatch {
                score: PATH_LOCAL_MATCH_SCORE,
                path: Path::new("ignored-dir").join("nested"),
                match_type: MatchType::Directory,
                root: repo.path().to_path_buf(),
                indices: None,
            },
        ]
    );
}

#[test]
fn path_local_matches_do_not_replace_corpus_matches() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    fs::write(repo.path().join("abexy"), "content").unwrap();

    let results = run(
        "abe",
        vec![repo.path().to_path_buf()],
        FileSearchOptions {
            limit: NonZero::new(20).unwrap(),
            threads: NonZero::new(2).unwrap(),
            compute_indices: true,
            ..Default::default()
        },
        /*cancel_flag*/ None,
    )
    .expect("run ok");

    assert_eq!(results.matches.len(), 1);
    let file_match = results.matches.first().expect("match");
    assert_eq!(file_match.path, PathBuf::from("abexy"));
    assert_eq!(file_match.indices, Some(vec![0, 1, 2]));
    assert_ne!(file_match.score, PATH_LOCAL_MATCH_SCORE);
}
