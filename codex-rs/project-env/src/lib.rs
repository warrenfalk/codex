use codex_utils_absolute_path::AbsolutePathBuf;
use futures::FutureExt;
use futures::future::BoxFuture;
use futures::future::Shared;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::capture::abs_path;
use crate::capture::cap_message;
use crate::capture::capture_env;
use crate::capture::direnv_identity;
use crate::capture::discover_envrc;
use crate::capture::fingerprint_watched_inputs;
use crate::capture::parse_direnv_json;
use crate::capture::parse_watched_paths;
use crate::capture::run_direnv;

pub use codex_protocol::protocol::ProjectEnvMode;

const PROJECT_ENV_PROCESS_ID_PREFIX: &str = "project-env:";

mod capture;

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
