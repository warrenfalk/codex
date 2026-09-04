use codex_exec_server::ExecutorFileSystem;
use codex_exec_server::GetMetadataOptions;
use codex_exec_server::ReadFileOptions;
use codex_file_system::FindUpErrorPolicy;
use codex_file_system::find_nearest_native_ancestor_with_markers;
use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::FileSystemAccessMode;
use codex_protocol::permissions::FileSystemPath;
use codex_protocol::permissions::FileSystemSandboxEntry;
use codex_protocol::permissions::FileSystemSandboxKind;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use std::ffi::OsStr;

const MAX_GIT_POINTER_BYTES: u64 = 4096;
const WRITE_PROBE_NAME: &str = ".codex-git-state-write-probe";

const COMMON_READ_ONLY_PATHS: &[&str] = &[
    "config",
    "config.lock",
    "hooks",
    "branches",
    "remotes",
    "info",
    "objects/info/alternates",
    "objects/info/http-alternates",
    "modules",
    "worktrees",
];

const PRIVATE_READ_ONLY_PATHS: &[&str] = &["config.worktree", "config.worktree.lock"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitMetadataLayout {
    dot_git_pointer: Option<AbsolutePathBuf>,
    common_git_dir: AbsolutePathBuf,
    private_git_dir: AbsolutePathBuf,
    linked_worktree: bool,
}

pub(crate) async fn with_git_metadata_write_access(
    fs: &dyn ExecutorFileSystem,
    cwd: &AbsolutePathBuf,
    permission_profile: PermissionProfile,
) -> PermissionProfile {
    let mut file_system_policy = permission_profile.file_system_sandbox_policy();
    if file_system_policy.kind != FileSystemSandboxKind::Restricted {
        return permission_profile;
    }

    let cwd_uri = PathUri::from_abs_path(cwd);
    let base = match fs
        .get_metadata(
            &cwd_uri,
            GetMetadataOptions::default(),
            /*sandbox*/ None,
        )
        .await
    {
        Ok(metadata) if metadata.is_directory => cwd.clone(),
        _ => {
            let Some(parent) = cwd.parent() else {
                return permission_profile;
            };
            parent
        }
    };
    let Some(repo_root) = find_nearest_native_ancestor_with_markers(
        fs,
        &base,
        vec![".git".to_string()],
        FindUpErrorPolicy::Ignore,
        /*sandbox*/ None,
    )
    .await
    .ok()
    .flatten() else {
        return permission_profile;
    };
    if !file_system_policy
        .can_write_path_with_cwd(repo_root.join(WRITE_PROBE_NAME).as_path(), cwd.as_path())
    {
        return permission_profile;
    }

    let Some(layout) = resolve_git_metadata_layout(fs, &repo_root).await else {
        return permission_profile;
    };

    append_path_entry(
        &mut file_system_policy.entries,
        layout.common_git_dir.clone(),
        FileSystemAccessMode::Write,
    );
    append_path_entry(
        &mut file_system_policy.entries,
        layout.private_git_dir.clone(),
        FileSystemAccessMode::Write,
    );

    if let Some(dot_git_pointer) = layout.dot_git_pointer {
        append_path_entry(
            &mut file_system_policy.entries,
            dot_git_pointer,
            FileSystemAccessMode::Read,
        );
    }

    for relative_path in COMMON_READ_ONLY_PATHS {
        append_path_entry(
            &mut file_system_policy.entries,
            layout.common_git_dir.join(relative_path),
            FileSystemAccessMode::Read,
        );
    }
    for relative_path in PRIVATE_READ_ONLY_PATHS {
        append_path_entry(
            &mut file_system_policy.entries,
            layout.private_git_dir.join(relative_path),
            FileSystemAccessMode::Read,
        );
    }
    if layout.linked_worktree {
        for relative_path in ["commondir", "gitdir"] {
            append_path_entry(
                &mut file_system_policy.entries,
                layout.private_git_dir.join(relative_path),
                FileSystemAccessMode::Read,
            );
        }
    }

    PermissionProfile::from_runtime_permissions_with_enforcement(
        permission_profile.enforcement(),
        &file_system_policy,
        permission_profile.network_sandbox_policy(),
    )
}

async fn resolve_git_metadata_layout(
    fs: &dyn ExecutorFileSystem,
    repo_root: &AbsolutePathBuf,
) -> Option<GitMetadataLayout> {
    let dot_git = repo_root.join(".git");
    let metadata = fs
        .get_metadata(
            &PathUri::from_abs_path(&dot_git),
            GetMetadataOptions::default(),
            /*sandbox*/ None,
        )
        .await
        .ok()?;
    if metadata.is_symlink {
        return None;
    }

    if metadata.is_directory {
        let git_dir = canonicalize_directory(fs, &dot_git).await?;
        if !has_standard_git_directory_layout(fs, &git_dir).await {
            return None;
        }
        return Some(GitMetadataLayout {
            dot_git_pointer: None,
            common_git_dir: git_dir.clone(),
            private_git_dir: git_dir,
            linked_worktree: false,
        });
    }
    if !metadata.is_file || metadata.size > MAX_GIT_POINTER_BYTES {
        return None;
    }

    let pointer = read_single_line(fs, &dot_git).await?;
    let private_path = pointer.strip_prefix("gitdir:")?.trim();
    if private_path.is_empty() {
        return None;
    }
    let private_git_dir = canonicalize_directory(
        fs,
        &AbsolutePathBuf::resolve_path_against_base(private_path, repo_root.as_path()),
    )
    .await?;
    let worktrees_dir = private_git_dir.parent()?;
    if worktrees_dir.as_path().file_name()? != OsStr::new("worktrees") {
        return None;
    }
    let common_git_dir = worktrees_dir.parent()?;
    if common_git_dir.as_path().file_name()? != OsStr::new(".git") {
        return None;
    }
    let common_git_dir = canonicalize_directory(fs, &common_git_dir).await?;
    if !has_standard_git_directory_layout(fs, &common_git_dir).await {
        return None;
    }

    let commondir_path = private_git_dir.join("commondir");
    let commondir = read_single_line(fs, &commondir_path).await?;
    let resolved_commondir = canonicalize_directory(
        fs,
        &AbsolutePathBuf::resolve_path_against_base(commondir.trim(), private_git_dir.as_path()),
    )
    .await?;
    if resolved_commondir != common_git_dir {
        return None;
    }

    let reverse_pointer = read_single_line(fs, &private_git_dir.join("gitdir")).await?;
    let resolved_reverse_pointer = canonicalize(
        fs,
        &AbsolutePathBuf::resolve_path_against_base(
            reverse_pointer.trim(),
            private_git_dir.as_path(),
        ),
    )
    .await?;
    if resolved_reverse_pointer != canonicalize(fs, &dot_git).await? {
        return None;
    }

    Some(GitMetadataLayout {
        dot_git_pointer: Some(dot_git),
        common_git_dir,
        private_git_dir,
        linked_worktree: true,
    })
}

async fn has_standard_git_directory_layout(
    fs: &dyn ExecutorFileSystem,
    git_dir: &AbsolutePathBuf,
) -> bool {
    read_single_line(fs, &git_dir.join("HEAD")).await.is_some()
        && canonicalize_directory(fs, &git_dir.join("objects"))
            .await
            .is_some()
        && canonicalize_directory(fs, &git_dir.join("refs"))
            .await
            .is_some()
}

async fn canonicalize(
    fs: &dyn ExecutorFileSystem,
    path: &AbsolutePathBuf,
) -> Option<AbsolutePathBuf> {
    fs.canonicalize(&PathUri::from_abs_path(path), /*sandbox*/ None)
        .await
        .ok()?
        .to_abs_path()
        .ok()
}

async fn canonicalize_directory(
    fs: &dyn ExecutorFileSystem,
    path: &AbsolutePathBuf,
) -> Option<AbsolutePathBuf> {
    let metadata = fs
        .get_metadata(
            &PathUri::from_abs_path(path),
            GetMetadataOptions::default(),
            /*sandbox*/ None,
        )
        .await
        .ok()?;
    if !metadata.is_directory || metadata.is_symlink {
        return None;
    }
    canonicalize(fs, path).await
}

async fn read_single_line(fs: &dyn ExecutorFileSystem, path: &AbsolutePathBuf) -> Option<String> {
    let path_uri = PathUri::from_abs_path(path);
    let metadata = fs
        .get_metadata(
            &path_uri,
            GetMetadataOptions::default(),
            /*sandbox*/ None,
        )
        .await
        .ok()?;
    if !metadata.is_file || metadata.is_symlink || metadata.size > MAX_GIT_POINTER_BYTES {
        return None;
    }
    let contents = fs
        .read_file_text(&path_uri, ReadFileOptions::default(), /*sandbox*/ None)
        .await
        .ok()?;
    let value = contents.trim();
    if value.is_empty() || value.lines().count() != 1 {
        return None;
    }
    Some(value.to_string())
}

fn append_path_entry(
    entries: &mut Vec<FileSystemSandboxEntry>,
    path: AbsolutePathBuf,
    access: FileSystemAccessMode,
) {
    let entry = FileSystemSandboxEntry {
        path: FileSystemPath::Path { path: path.into() },
        access,
        missing_path_behavior: None,
    };
    if !entries.iter().any(|existing| existing == &entry) {
        entries.push(entry);
    }
}

#[cfg(test)]
#[path = "git_metadata_permissions_tests.rs"]
mod tests;
