use codex_utils_absolute_path::AbsolutePathBuf;
use serde_json::Value;
use sha2::Digest as _;
use sha2::Sha256;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::DirenvIdentity;
use super::ProjectEnvError;

const STATUS_MESSAGE_MAX_BYTES: usize = 2048;
const HASH_FILE_MAX_BYTES: u64 = 1024 * 1024;
const FINGERPRINT_SCHEMA_VERSION: u8 = 1;
const STOPPED_MESSAGE: &str = "project environment loading was stopped before the command could run. Retry the command, or run with project_env: \"bypass\" to inspect or repair the project environment.";

pub(super) fn capture_env(
    process_env: &HashMap<String, String>,
    envrc_dir: &Path,
) -> Result<HashMap<String, String>, ProjectEnvError> {
    let xdg_root = envrc_dir
        .join(".direnv")
        .join("codex")
        .join("project-env")
        .join("xdg");
    let config = xdg_root.join("config");
    let data = xdg_root.join("data");
    let cache = xdg_root.join("cache");
    std::fs::create_dir_all(&config).map_err(|err| {
        ProjectEnvError::Failed(format!(
            "failed to create private direnv config dir {}: {err}",
            config.display()
        ))
    })?;
    std::fs::create_dir_all(&data).map_err(|err| {
        ProjectEnvError::Failed(format!(
            "failed to create private direnv data dir {}: {err}",
            data.display()
        ))
    })?;
    std::fs::create_dir_all(&cache).map_err(|err| {
        ProjectEnvError::Failed(format!(
            "failed to create private direnv cache dir {}: {err}",
            cache.display()
        ))
    })?;

    let mut env = process_env.clone();
    env.insert(
        "XDG_CONFIG_HOME".to_string(),
        config.to_string_lossy().to_string(),
    );
    env.insert(
        "XDG_DATA_HOME".to_string(),
        data.to_string_lossy().to_string(),
    );
    env.insert(
        "XDG_CACHE_HOME".to_string(),
        cache.to_string_lossy().to_string(),
    );
    env.remove("DIRENV_CONFIG");
    Ok(env)
}

pub(super) async fn direnv_identity(
    process_env: &HashMap<String, String>,
) -> Result<DirenvIdentity, ProjectEnvError> {
    let path = process_env
        .get("PATH")
        .and_then(|path| find_on_path(path, "direnv"));
    let output = run_program(
        "direnv",
        &["version"],
        Path::new("."),
        process_env,
        &CancellationToken::new(),
    )
    .await?;
    Ok(DirenvIdentity {
        path,
        version: output.trim().to_string(),
    })
}

pub(super) async fn run_direnv(
    args: &[&str],
    cwd: &Path,
    env: &HashMap<String, String>,
    cancellation: &CancellationToken,
) -> Result<String, ProjectEnvError> {
    run_program("direnv", args, cwd, env, cancellation).await
}

async fn run_program(
    program: &str,
    args: &[&str],
    cwd: &Path,
    env: &HashMap<String, String>,
    cancellation: &CancellationToken,
) -> Result<String, ProjectEnvError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .kill_on_drop(true);
    let output = tokio::select! {
        output = command.output() => output,
        _ = cancellation.cancelled() => {
            return Err(ProjectEnvError::Stopped(STOPPED_MESSAGE.to_string()));
        }
    }
    .map_err(|err| {
        ProjectEnvError::Failed(format!(
            "failed to run {} {}: {err}",
            program,
            args.join(" ")
        ))
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = cap_message(&format!("{stderr}{stdout}"));
        return Err(ProjectEnvError::Failed(format!(
            "{} {} failed with status {}{}{}",
            program,
            args.join(" "),
            output.status,
            if detail.is_empty() { "" } else { ": " },
            detail
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub(super) fn parse_direnv_json(output: &str) -> Result<HashMap<String, String>, ProjectEnvError> {
    let value: Value = serde_json::from_str(output).map_err(|err| {
        ProjectEnvError::Failed(format!("direnv export json produced invalid JSON: {err}"))
    })?;
    let object = value.as_object().ok_or_else(|| {
        ProjectEnvError::Failed("direnv export json did not return a JSON object".to_string())
    })?;
    Ok(object
        .iter()
        .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
        .collect())
}

pub(super) fn parse_watched_paths(output: &str, cwd: &Path, envrc_path: &Path) -> Vec<PathBuf> {
    let mut paths = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        })
        .collect::<Vec<_>>();
    paths.push(envrc_path.to_path_buf());
    paths.sort();
    paths.dedup();
    paths
}

pub(super) fn discover_envrc(cwd: &Path) -> Option<PathBuf> {
    let mut current = cwd;
    loop {
        let envrc = current.join(".envrc");
        if envrc.is_file() {
            return Some(envrc);
        }
        current = current.parent()?;
    }
}

pub(super) fn fingerprint_watched_inputs(
    watched_paths: &[PathBuf],
    envrc_path: &AbsolutePathBuf,
    direnv_identity: &DirenvIdentity,
) -> Result<String, ProjectEnvError> {
    let mut hasher = Sha256::new();
    hash_line(&mut hasher, &format!("schema:{FINGERPRINT_SCHEMA_VERSION}"));
    hash_line(
        &mut hasher,
        &format!("envrc:{}", envrc_path.as_path().display()),
    );
    hash_line(
        &mut hasher,
        &format!(
            "direnv_path:{}",
            direnv_identity
                .path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<unknown>".to_string())
        ),
    );
    hash_line(
        &mut hasher,
        &format!("direnv_version:{}", direnv_identity.version),
    );
    let mut paths = watched_paths.to_vec();
    paths.sort();
    paths.dedup();
    for path in paths {
        hash_path(&mut hasher, &path)?;
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_path(hasher: &mut Sha256, path: &Path) -> Result<(), ProjectEnvError> {
    hash_line(hasher, &format!("path:{}", path.display()));
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            let file_type = metadata.file_type();
            let kind = if file_type.is_file() {
                "file"
            } else if file_type.is_dir() {
                "dir"
            } else if file_type.is_symlink() {
                "symlink"
            } else {
                "other"
            };
            hash_line(hasher, &format!("kind:{kind}"));
            hash_line(hasher, &format!("len:{}", metadata.len()));
            if let Ok(modified) = metadata.modified()
                && let Ok(duration) = modified.duration_since(UNIX_EPOCH)
            {
                hash_line(
                    hasher,
                    &format!("mtime:{}:{}", duration.as_secs(), duration.subsec_nanos()),
                );
            }
            if should_content_hash(path, &metadata) {
                let bytes = std::fs::read(path).map_err(|err| {
                    ProjectEnvError::Failed(format!(
                        "failed to read watched input {}: {err}",
                        path.display()
                    ))
                })?;
                hash_line(hasher, &format!("content:{:x}", Sha256::digest(bytes)));
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            hash_line(hasher, "kind:missing");
        }
        Err(err) => {
            return Err(ProjectEnvError::Failed(format!(
                "failed to inspect watched input {}: {err}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn should_content_hash(path: &Path, metadata: &std::fs::Metadata) -> bool {
    if !metadata.is_file() || metadata.len() > HASH_FILE_MAX_BYTES {
        return false;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches!(name, ".envrc" | "flake.nix" | "flake.lock" | ".env.local")
}

fn hash_line(hasher: &mut Sha256, line: &str) {
    hasher.update(line.as_bytes());
    hasher.update(b"\0");
}

fn find_on_path(path: &str, exe: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(exe))
        .find(|candidate| candidate.is_file())
}

pub(super) fn abs_path(path: PathBuf) -> Result<AbsolutePathBuf, ProjectEnvError> {
    AbsolutePathBuf::try_from(path).map_err(|err| {
        ProjectEnvError::Failed(format!("failed to resolve absolute path for .envrc: {err}"))
    })
}

pub(super) fn cap_message(message: &str) -> String {
    let mut end = message.len().min(STATUS_MESSAGE_MAX_BYTES);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message[..end].trim().to_string()
}
