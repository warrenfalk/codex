use codex_utils_absolute_path::AbsolutePathBuf;
use futures::FutureExt;
use futures::future::BoxFuture;
use futures::future::Shared;
use serde_json::Value;
use sha2::Digest as _;
use sha2::Sha256;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

pub use codex_protocol::protocol::ProjectEnvMode;

const STATUS_MESSAGE_MAX_BYTES: usize = 2048;
const HASH_FILE_MAX_BYTES: u64 = 1024 * 1024;
const FINGERPRINT_SCHEMA_VERSION: u8 = 1;
const PROJECT_ENV_PROCESS_ID_PREFIX: &str = "project-env:";
const STOPPED_MESSAGE: &str = "project environment loading was stopped before the command could run. Retry the command, or run with project_env: \"bypass\" to inspect or repair the project environment.";

#[derive(Clone, Debug)]
pub struct ProjectEnvConfig {
    pub disabled: bool,
    pub process_env: HashMap<String, String>,
}

impl ProjectEnvConfig {
    pub fn from_current_process(disabled: bool) -> Self {
        Self {
            disabled,
            process_env: std::env::vars().collect(),
        }
    }

    pub fn disabled_for_tests() -> Self {
        Self {
            disabled: true,
            process_env: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectEnvOverlay {
    pub env: HashMap<String, String>,
    pub envrc_path: AbsolutePathBuf,
    pub watched_input_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectEnvState {
    Disabled,
    None,
    Building,
    Ready,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectEnvStatus {
    pub state: ProjectEnvState,
    pub cwd: Option<AbsolutePathBuf>,
    pub envrc_path: Option<AbsolutePathBuf>,
    pub message: Option<String>,
    pub updated_at: i64,
    pub watched_input_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectEnvBuildInfo {
    pub item_id: String,
    pub process_id: String,
    pub command: String,
    pub cwd: AbsolutePathBuf,
}

#[derive(Clone, Debug)]
struct DirenvIdentity {
    path: Option<PathBuf>,
    version: String,
}

#[derive(Clone, Debug)]
struct ReadyEnv {
    overlay: Arc<ProjectEnvOverlay>,
    watched_paths: Vec<PathBuf>,
    fingerprint: String,
    direnv_identity: DirenvIdentity,
}

#[derive(Clone)]
struct BuildTask {
    id: u64,
    info: ProjectEnvBuildInfo,
    cancellation: CancellationToken,
    result: Shared<BoxFuture<'static, BuildOutcome>>,
}

type BuildOutcome = Result<Arc<ProjectEnvOverlay>, ProjectEnvError>;

#[derive(Clone)]
enum EntryState {
    Idle,
    Building(BuildTask),
    Ready(ReadyEnv),
    Failed {
        message: String,
        envrc_path: AbsolutePathBuf,
        watched_input_count: usize,
        updated_at: i64,
    },
}

impl EntryState {
    fn status_for_envrc(
        &self,
        cwd: &AbsolutePathBuf,
        envrc_path: &AbsolutePathBuf,
    ) -> ProjectEnvStatus {
        match self {
            EntryState::Idle => ProjectEnvStatus {
                state: ProjectEnvState::None,
                cwd: Some(cwd.clone()),
                envrc_path: Some(envrc_path.clone()),
                message: None,
                updated_at: now_unix_secs(),
                watched_input_count: 0,
            },
            EntryState::Building(_) => ProjectEnvStatus {
                state: ProjectEnvState::Building,
                cwd: Some(cwd.clone()),
                envrc_path: Some(envrc_path.clone()),
                message: Some("project environment loading is running".to_string()),
                updated_at: now_unix_secs(),
                watched_input_count: 0,
            },
            EntryState::Ready(ready) => ProjectEnvStatus {
                state: ProjectEnvState::Ready,
                cwd: Some(cwd.clone()),
                envrc_path: Some(ready.overlay.envrc_path.clone()),
                message: None,
                updated_at: now_unix_secs(),
                watched_input_count: ready.overlay.watched_input_count,
            },
            EntryState::Failed {
                message,
                envrc_path,
                watched_input_count,
                updated_at,
            } => ProjectEnvStatus {
                state: ProjectEnvState::Failed,
                cwd: Some(cwd.clone()),
                envrc_path: Some(envrc_path.clone()),
                message: Some(message.clone()),
                updated_at: *updated_at,
                watched_input_count: *watched_input_count,
            },
        }
    }
}

struct EnvrcEntry {
    state: Mutex<EntryState>,
}

impl EnvrcEntry {
    fn new() -> Self {
        Self {
            state: Mutex::new(EntryState::Idle),
        }
    }
}

struct Inner {
    config: ProjectEnvConfig,
    entries: Mutex<HashMap<AbsolutePathBuf, Arc<EnvrcEntry>>>,
    cached_envs: StdMutex<HashMap<String, ReadyEnv>>,
    status_tx: broadcast::Sender<ProjectEnvStatus>,
    next_build_id: AtomicU64,
}

#[derive(Clone)]
pub struct ProjectEnvManager {
    inner: Arc<Inner>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ProjectEnvError {
    #[error("{0}")]
    Failed(String),
    #[error("{0}")]
    Stopped(String),
    #[error("project environment loading is disabled")]
    Disabled,
}

impl ProjectEnvError {
    pub fn model_message(&self) -> String {
        match self {
            ProjectEnvError::Failed(message) => {
                format!(
                    "{message}\n\nRetry with project_env: \"bypass\" to inspect or repair the project environment."
                )
            }
            ProjectEnvError::Stopped(message) => message.clone(),
            ProjectEnvError::Disabled => "project environment loading is disabled".to_string(),
        }
    }
}

impl ProjectEnvManager {
    pub fn new(config: ProjectEnvConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                entries: Mutex::new(HashMap::new()),
                cached_envs: StdMutex::new(HashMap::new()),
                status_tx: broadcast::channel(/*capacity*/ 32).0,
                next_build_id: AtomicU64::new(1),
            }),
        }
    }

    pub fn disabled_for_tests() -> Self {
        Self::new(ProjectEnvConfig::disabled_for_tests())
    }

    pub fn is_disabled(&self) -> bool {
        self.inner.config.disabled
    }

    pub fn subscribe_status(&self) -> broadcast::Receiver<ProjectEnvStatus> {
        self.inner.status_tx.subscribe()
    }

    pub fn prewarm(&self, cwd: AbsolutePathBuf) {
        if self.inner.config.disabled {
            return;
        }
        let manager = self.clone();
        tokio::spawn(async move {
            let _ = manager
                .environment_for_command(&cwd, ProjectEnvMode::Auto, CancellationToken::new())
                .await;
        });
    }

    pub async fn environment_for_command(
        &self,
        cwd: &AbsolutePathBuf,
        mode: ProjectEnvMode,
        cancellation: CancellationToken,
    ) -> Result<Option<Arc<ProjectEnvOverlay>>, ProjectEnvError> {
        if mode == ProjectEnvMode::Bypass {
            return Ok(None);
        }
        if self.inner.config.disabled {
            return Ok(None);
        }
        let Some(envrc_path) = discover_envrc(cwd.as_path()) else {
            return Ok(None);
        };
        let envrc_path = abs_path(envrc_path)?;
        let entry = self.entry_for(envrc_path.clone()).await;

        let wait_task = {
            let mut state = entry.state.lock().await;
            match &*state {
                EntryState::Ready(ready) => {
                    if let Ok(fingerprint) = fingerprint_watched_inputs(
                        &ready.watched_paths,
                        &ready.overlay.envrc_path,
                        &ready.direnv_identity,
                    ) {
                        if fingerprint == ready.fingerprint {
                            return Ok(Some(Arc::clone(&ready.overlay)));
                        }
                        let cached_ready = self
                            .inner
                            .cached_envs
                            .lock()
                            .ok()
                            .and_then(|cached_envs| cached_envs.get(&fingerprint).cloned());
                        if let Some(cached_ready) = cached_ready {
                            let overlay = Arc::clone(&cached_ready.overlay);
                            *state = EntryState::Ready(cached_ready);
                            self.notify(ProjectEnvStatus {
                                state: ProjectEnvState::Ready,
                                cwd: Some(cwd.clone()),
                                envrc_path: Some(overlay.envrc_path.clone()),
                                message: None,
                                updated_at: now_unix_secs(),
                                watched_input_count: overlay.watched_input_count,
                            });
                            return Ok(Some(overlay));
                        }
                    }
                    self.start_build_locked(&mut state, cwd.clone(), envrc_path.clone())
                }
                EntryState::Failed { .. } | EntryState::Idle => {
                    self.start_build_locked(&mut state, cwd.clone(), envrc_path.clone())
                }
                EntryState::Building(build) => build.clone(),
            }
        };

        match wait_for_build(wait_task.result.clone(), &cancellation).await {
            Ok(result) => result.map(Some),
            Err(err) => Err(err),
        }
    }

    pub async fn status_for_cwd(&self, cwd: Option<&AbsolutePathBuf>) -> ProjectEnvStatus {
        if self.inner.config.disabled {
            return ProjectEnvStatus {
                state: ProjectEnvState::Disabled,
                cwd: cwd.cloned(),
                envrc_path: None,
                message: Some("project environment loading is disabled".to_string()),
                updated_at: now_unix_secs(),
                watched_input_count: 0,
            };
        }
        let Some(cwd) = cwd else {
            return ProjectEnvStatus {
                state: ProjectEnvState::None,
                cwd: None,
                envrc_path: None,
                message: Some("no local thread cwd is selected".to_string()),
                updated_at: now_unix_secs(),
                watched_input_count: 0,
            };
        };
        let Some(envrc_path) = discover_envrc(cwd.as_path()) else {
            return ProjectEnvStatus {
                state: ProjectEnvState::None,
                cwd: Some(cwd.clone()),
                envrc_path: None,
                message: None,
                updated_at: now_unix_secs(),
                watched_input_count: 0,
            };
        };
        let Ok(envrc_path) = abs_path(envrc_path) else {
            return ProjectEnvStatus {
                state: ProjectEnvState::Failed,
                cwd: Some(cwd.clone()),
                envrc_path: None,
                message: Some("failed to resolve .envrc path".to_string()),
                updated_at: now_unix_secs(),
                watched_input_count: 0,
            };
        };
        let entry = self.entry_for(envrc_path.clone()).await;
        entry.state.lock().await.status_for_envrc(cwd, &envrc_path)
    }

    pub async fn list_builds(&self) -> Vec<ProjectEnvBuildInfo> {
        let entries = {
            let entries = self.inner.entries.lock().await;
            entries.values().cloned().collect::<Vec<_>>()
        };

        let mut builds = Vec::new();
        for entry in entries {
            if let EntryState::Building(build) = &*entry.state.lock().await {
                builds.push(build.info.clone());
            }
        }
        builds.sort_by(|a, b| a.process_id.cmp(&b.process_id));
        builds
    }

    pub async fn cancel_all(&self) {
        let entries = {
            let entries = self.inner.entries.lock().await;
            entries.values().cloned().collect::<Vec<_>>()
        };
        for entry in entries {
            if let EntryState::Building(build) = &*entry.state.lock().await {
                build.cancellation.cancel();
            }
        }
    }

    pub async fn cancel_build(&self, process_id: &str) -> bool {
        if !process_id.starts_with(PROJECT_ENV_PROCESS_ID_PREFIX) {
            return false;
        }
        let entries = {
            let entries = self.inner.entries.lock().await;
            entries.values().cloned().collect::<Vec<_>>()
        };
        for entry in entries {
            if let EntryState::Building(build) = &*entry.state.lock().await
                && build.info.process_id == process_id
            {
                build.cancellation.cancel();
                return true;
            }
        }
        false
    }

    async fn entry_for(&self, envrc_path: AbsolutePathBuf) -> Arc<EnvrcEntry> {
        let mut entries = self.inner.entries.lock().await;
        entries
            .entry(envrc_path)
            .or_insert_with(|| Arc::new(EnvrcEntry::new()))
            .clone()
    }

    fn start_build_locked(
        &self,
        state: &mut EntryState,
        cwd: AbsolutePathBuf,
        envrc_path: AbsolutePathBuf,
    ) -> BuildTask {
        let build_id = self.inner.next_build_id.fetch_add(1, Ordering::Relaxed);
        let cancellation = CancellationToken::new();
        let manager = self.clone();
        let build_cancellation = cancellation.clone();
        let build_cwd = cwd.clone();
        let build_envrc_path = envrc_path.clone();
        let future = async move {
            let result = manager
                .build_env(
                    build_cwd.clone(),
                    build_envrc_path.clone(),
                    build_cancellation,
                )
                .await;
            manager
                .finish_build(build_id, build_envrc_path.clone(), result.clone())
                .await;
            result
        }
        .boxed()
        .shared();
        let process_id = format!("{PROJECT_ENV_PROCESS_ID_PREFIX}{build_id}");
        let task = BuildTask {
            id: build_id,
            info: ProjectEnvBuildInfo {
                item_id: process_id.clone(),
                process_id,
                command: "direnv export json".to_string(),
                cwd,
            },
            cancellation,
            result: future,
        };
        *state = EntryState::Building(task.clone());
        self.notify(ProjectEnvStatus {
            state: ProjectEnvState::Building,
            cwd: Some(task.info.cwd.clone()),
            envrc_path: Some(envrc_path),
            message: Some("project environment loading is running".to_string()),
            updated_at: now_unix_secs(),
            watched_input_count: 0,
        });
        task
    }

    async fn finish_build(&self, build_id: u64, envrc_path: AbsolutePathBuf, result: BuildOutcome) {
        let entry = self.entry_for(envrc_path.clone()).await;
        let mut state = entry.state.lock().await;
        let EntryState::Building(build) = &*state else {
            return;
        };
        if build.id != build_id {
            return;
        }
        match result {
            Ok(overlay) => {
                let ready = ReadyEnv {
                    watched_paths: Vec::new(),
                    fingerprint: String::new(),
                    direnv_identity: DirenvIdentity {
                        path: None,
                        version: String::new(),
                    },
                    overlay,
                };
                // The build path installs the populated ready state before returning.
                if matches!(&*state, EntryState::Building(_)) {
                    *state = EntryState::Ready(ready);
                }
            }
            Err(err) => {
                let cwd = build.info.cwd.clone();
                let message = cap_message(&err.model_message());
                *state = EntryState::Failed {
                    message: message.clone(),
                    envrc_path: envrc_path.clone(),
                    watched_input_count: 0,
                    updated_at: now_unix_secs(),
                };
                self.notify(ProjectEnvStatus {
                    state: ProjectEnvState::Failed,
                    cwd: Some(cwd),
                    envrc_path: Some(envrc_path),
                    message: Some(message),
                    updated_at: now_unix_secs(),
                    watched_input_count: 0,
                });
            }
        }
    }

    async fn build_env(
        &self,
        cwd: AbsolutePathBuf,
        envrc_path: AbsolutePathBuf,
        cancellation: CancellationToken,
    ) -> BuildOutcome {
        let direnv_identity = direnv_identity(&self.inner.config.process_env).await?;
        let envrc_dir = envrc_path
            .parent()
            .ok_or_else(|| ProjectEnvError::Failed("invalid .envrc path".to_string()))?
            .to_path_buf();
        let capture_env = capture_env(&self.inner.config.process_env, &envrc_dir)?;

        let envrc_arg = envrc_path.as_path().to_string_lossy().to_string();
        run_direnv(
            &["allow", envrc_arg.as_str()],
            &envrc_dir,
            &capture_env,
            &cancellation,
        )
        .await?;
        let export_output = run_direnv(
            &["export", "json"],
            cwd.as_path(),
            &capture_env,
            &cancellation,
        )
        .await?;
        let env = parse_direnv_json(&export_output)?;
        let watch_output =
            run_direnv(&["watch-print"], cwd.as_path(), &capture_env, &cancellation).await?;
        let watched_paths = parse_watched_paths(&watch_output, cwd.as_path(), envrc_path.as_path());
        let fingerprint =
            fingerprint_watched_inputs(&watched_paths, &envrc_path, &direnv_identity)?;
        let overlay = Arc::new(ProjectEnvOverlay {
            env,
            envrc_path: envrc_path.clone(),
            watched_input_count: watched_paths.len(),
        });
        let ready = ReadyEnv {
            overlay: Arc::clone(&overlay),
            watched_paths,
            fingerprint: fingerprint.clone(),
            direnv_identity,
        };
        if let Ok(mut cached_envs) = self.inner.cached_envs.lock() {
            cached_envs.insert(fingerprint, ready.clone());
        }
        let entry = self.entry_for(envrc_path).await;
        *entry.state.lock().await = EntryState::Ready(ready);
        self.notify(ProjectEnvStatus {
            state: ProjectEnvState::Ready,
            cwd: Some(cwd),
            envrc_path: Some(overlay.envrc_path.clone()),
            message: None,
            updated_at: now_unix_secs(),
            watched_input_count: overlay.watched_input_count,
        });
        Ok(overlay)
    }

    fn notify(&self, status: ProjectEnvStatus) {
        let _ = self.inner.status_tx.send(status);
    }
}

async fn wait_for_build(
    result: Shared<BoxFuture<'static, BuildOutcome>>,
    cancellation: &CancellationToken,
) -> Result<BuildOutcome, ProjectEnvError> {
    tokio::select! {
        result = result => Ok(result),
        _ = cancellation.cancelled() => Err(ProjectEnvError::Stopped("project environment wait was cancelled before the command could run. Retry the command, or run with project_env: \"bypass\" to inspect or repair the project environment.".to_string())),
    }
}

fn capture_env(
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

async fn direnv_identity(
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

async fn run_direnv(
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

fn parse_direnv_json(output: &str) -> Result<HashMap<String, String>, ProjectEnvError> {
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

fn parse_watched_paths(output: &str, cwd: &Path, envrc_path: &Path) -> Vec<PathBuf> {
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

fn discover_envrc(cwd: &Path) -> Option<PathBuf> {
    let mut current = cwd;
    loop {
        let envrc = current.join(".envrc");
        if envrc.is_file() {
            return Some(envrc);
        }
        current = current.parent()?;
    }
}

fn fingerprint_watched_inputs(
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

fn abs_path(path: PathBuf) -> Result<AbsolutePathBuf, ProjectEnvError> {
    AbsolutePathBuf::try_from(path).map_err(|err| {
        ProjectEnvError::Failed(format!("failed to resolve absolute path for .envrc: {err}"))
    })
}

fn cap_message(message: &str) -> String {
    let mut end = message.len().min(STATUS_MESSAGE_MAX_BYTES);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message[..end].trim().to_string()
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

pub fn apply_overlay(
    env: &mut HashMap<String, String>,
    overlay: &ProjectEnvOverlay,
    shell_environment_set: &HashMap<String, String>,
    thread_id: Option<String>,
) {
    env.extend(overlay.env.clone());
    env.extend(shell_environment_set.clone());
    if let Some(thread_id) = thread_id {
        env.insert("CODEX_THREAD_ID".to_string(), thread_id);
    }
}

#[cfg(test)]
#[path = "project_env_tests.rs"]
mod tests;
