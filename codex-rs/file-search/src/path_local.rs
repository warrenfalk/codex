use std::collections::HashSet;
use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::FileMatch;
use crate::MatchType;

const PATH_LOCAL_MATCH_SCORE: u32 = u32::MAX;

pub(crate) fn augment_matches(
    query: &str,
    search_directories: &[PathBuf],
    limit: usize,
    corpus_match_count: usize,
    matches: &mut Vec<FileMatch>,
) -> usize {
    let mut path_local_matches = collect_path_local_matches(query, search_directories);
    if path_local_matches.is_empty() {
        return corpus_match_count;
    }

    path_local_matches.sort_by(|a, b| a.path.cmp(&b.path));
    let path_local_match_count = path_local_matches.len();
    let corpus_matches = std::mem::take(matches);
    let corpus_match_keys = corpus_matches
        .iter()
        .map(|file_match| (file_match.root.clone(), file_match.path.clone()))
        .collect::<HashSet<_>>();
    let mut seen = HashSet::<(PathBuf, PathBuf)>::new();
    let mut merged = Vec::with_capacity(limit.min(path_local_match_count + corpus_matches.len()));

    for file_match in path_local_matches
        .into_iter()
        .filter(|file_match| {
            !corpus_match_keys.contains(&(file_match.root.clone(), file_match.path.clone()))
        })
        .chain(corpus_matches)
    {
        if seen.insert((file_match.root.clone(), file_match.path.clone())) {
            merged.push(file_match);
            if merged.len() == limit {
                break;
            }
        }
    }

    let total_match_count = corpus_match_count
        .max(path_local_match_count)
        .max(merged.len());
    *matches = merged;
    total_match_count
}

fn collect_path_local_matches(query: &str, search_directories: &[PathBuf]) -> Vec<FileMatch> {
    let Some((dir_query, name_prefix)) = query_path_parts(query) else {
        return Vec::new();
    };
    let Some(relative_dir) = normalize_relative_dir(dir_query) else {
        return Vec::new();
    };
    let name_prefix = name_prefix.to_lowercase();

    let mut matches = Vec::new();
    for root in search_directories {
        let directory = root.join(&relative_dir);
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };

        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name_text = file_name.to_string_lossy();
            if !name_prefix.is_empty() && !file_name_text.to_lowercase().starts_with(&name_prefix) {
                continue;
            }

            let path = relative_dir.join(file_name);
            let full_path = root.join(&path);
            let match_type = if full_path.is_dir() {
                MatchType::Directory
            } else {
                MatchType::File
            };
            matches.push(FileMatch {
                score: PATH_LOCAL_MATCH_SCORE,
                path,
                match_type,
                root: root.clone(),
                indices: None,
            });
        }
    }

    matches
}

fn query_path_parts(query: &str) -> Option<(&str, &str)> {
    if query.is_empty()
        || query.chars().next().is_some_and(is_path_separator)
        || Path::new(query).is_absolute()
    {
        return None;
    }

    match query.char_indices().rfind(|(_, ch)| is_path_separator(*ch)) {
        Some((idx, separator)) => Some((&query[..idx], &query[idx + separator.len_utf8()..])),
        None => Some(("", query)),
    }
}

fn is_path_separator(ch: char) -> bool {
    ch == '/' || ch == '\\'
}

fn normalize_relative_dir(path: &str) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
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
}
