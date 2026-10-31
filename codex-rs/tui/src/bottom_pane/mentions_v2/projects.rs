//! Project selections are explicit attachments, distinct from ordinary directory references.

use codex_utils_absolute_path::AbsolutePathBuf;

use super::candidate::Candidate;
use super::candidate::MentionType;
use super::candidate::Selection;

pub(super) fn project_candidates(projects: &[AbsolutePathBuf]) -> Vec<Candidate> {
    let mut candidates: Vec<_> = projects
        .iter()
        .map(|path| {
            let path = path.to_string_lossy();
            let name = crate::project_mentions::project_name(&path);
            Candidate {
                display_name: path
                    .trim_end_matches(['/', '\\'])
                    .rsplit(['/', '\\'])
                    .next()
                    .filter(|name| !name.is_empty())
                    .unwrap_or(&path)
                    .to_string(),
                description: Some(format!("Attach for session: {path}")),
                search_terms: vec![path.to_string(), name.clone()],
                mention_type: MentionType::Project,
                selection: Selection::Tool {
                    insert_text: format!("@{name}"),
                    path: Some(crate::project_mentions::project_target(&path)),
                },
            }
        })
        .collect();
    candidates.push(Candidate {
        display_name: "Choose another directory…".to_string(),
        description: Some("Type a directory path".to_string()),
        search_terms: Vec::new(),
        mention_type: MentionType::Project,
        selection: Selection::BrowseProjects,
    });
    candidates
}
