#![cfg(unix)]
use codex_core::spawn::StdioPolicy;
use codex_protocol::protocol::SandboxPolicy;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::collections::HashMap;
use std::env;
use std::future::Future;
use std::io;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitStatus;
use tokio::fs::create_dir_all;
use tokio::process::Child;

#[cfg(target_os = "macos")]
async fn spawn_command_under_sandbox(
    command: Vec<String>,
    command_cwd: PathBuf,
    sandbox_policy: &SandboxPolicy,
    sandbox_cwd: &Path,
    stdio_policy: StdioPolicy,
    env: HashMap<String, String>,
) -> std::io::Result<Child> {
    use codex_core::seatbelt::spawn_command_under_seatbelt;
    spawn_command_under_seatbelt(
        command,
        command_cwd,
        sandbox_policy,
        sandbox_cwd,
        stdio_policy,
        None,
        env,
    )
    .await
}

#[cfg(target_os = "linux")]
async fn spawn_command_under_sandbox(
    command: Vec<String>,
    command_cwd: PathBuf,
    sandbox_policy: &SandboxPolicy,
    sandbox_cwd: &Path,
    stdio_policy: StdioPolicy,
    env: HashMap<String, String>,
) -> std::io::Result<Child> {
    use codex_core::landlock::spawn_command_under_linux_sandbox;
    let codex_linux_sandbox_exe = codex_utils_cargo_bin::cargo_bin("codex-exec")
        .map_err(|err| io::Error::new(io::ErrorKind::NotFound, err))?;
    spawn_command_under_linux_sandbox(
        codex_linux_sandbox_exe,
        command,
        command_cwd,
        sandbox_policy,
        sandbox_cwd,
        false,
        stdio_policy,
        None,
        env,
    )
    .await
}

fn find_executable_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(target_os = "linux")]
/// Determines whether Linux sandbox tests can run on this host.
///
/// These tests require an enforceable filesystem sandbox. We run a tiny command
/// under the production Landlock path and skip when enforcement is unavailable
/// (for example on kernels or container profiles where Landlock is not
/// enforced).
async fn linux_sandbox_test_env() -> Option<HashMap<String, String>> {
    let command_cwd = std::env::current_dir().ok()?;
    let sandbox_cwd = command_cwd.clone();
    let policy = SandboxPolicy::new_read_only_policy();

    if can_apply_linux_sandbox_policy(&policy, &command_cwd, sandbox_cwd.as_path(), HashMap::new())
        .await
    {
        return Some(HashMap::new());
    }

    eprintln!("Skipping test: Landlock is not enforceable on this host.");
    None
}

#[cfg(target_os = "linux")]
/// Returns whether a minimal command can run successfully with the requested
/// Linux sandbox policy applied.
///
/// This is used as a capability probe so sandbox behavior tests only run when
/// Landlock enforcement is actually active.
async fn can_apply_linux_sandbox_policy(
    policy: &SandboxPolicy,
    command_cwd: &Path,
    sandbox_cwd: &Path,
    env: HashMap<String, String>,
) -> bool {
    let spawn_result = spawn_command_under_sandbox(
        vec!["/usr/bin/true".to_string()],
        command_cwd.to_path_buf(),
        policy,
        sandbox_cwd,
        StdioPolicy::RedirectForShellTool,
        env,
    )
    .await;
    let Ok(mut child) = spawn_result else {
        return false;
    };
    child
        .wait()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn python_multiprocessing_lock_works_under_sandbox() {
    core_test_support::skip_if_sandbox!();
    let Some(python3) = find_executable_on_path("python3") else {
        eprintln!("python3 not found in PATH, skipping test.");
        return;
    };
    let python3 = python3.to_string_lossy().to_string();
    #[cfg(target_os = "linux")]
    let sandbox_env = match linux_sandbox_test_env().await {
        Some(env) => env,
        None => return,
    };
    #[cfg(not(target_os = "linux"))]
    let sandbox_env = HashMap::new();
    #[cfg(target_os = "macos")]
    let writable_roots = Vec::<AbsolutePathBuf>::new();

    // From https://man7.org/linux/man-pages/man7/sem_overview.7.html
    //
    // > On Linux, named semaphores are created in a virtual filesystem,
    // > normally mounted under /dev/shm.
    #[cfg(target_os = "linux")]
    let writable_roots: Vec<AbsolutePathBuf> = vec!["/dev/shm".try_into().unwrap()];

    let policy = SandboxPolicy::WorkspaceWrite {
        writable_roots,
        read_only_access: Default::default(),
        network_access: false,
        exclude_tmpdir_env_var: false,
        exclude_slash_tmp: false,
    };

    let python_code = r#"import multiprocessing
from multiprocessing import Lock, Process

def f(lock):
    with lock:
        print("Lock acquired in child process")

if __name__ == '__main__':
    lock = Lock()
    p = Process(target=f, args=(lock,))
    p.start()
    p.join()
"#;

    let command_cwd = std::env::current_dir().expect("should be able to get current dir");
    let sandbox_cwd = command_cwd.clone();
    let mut child = spawn_command_under_sandbox(
        vec![python3, "-c".to_string(), python_code.to_string()],
        command_cwd,
        &policy,
        sandbox_cwd.as_path(),
        StdioPolicy::Inherit,
        sandbox_env,
    )
    .await
    .expect("should be able to spawn python under sandbox");

    let status = child.wait().await.expect("should wait for child process");
    assert!(status.success(), "python exited with {status:?}");
}

#[tokio::test]
async fn python_getpwuid_works_under_sandbox() {
    core_test_support::skip_if_sandbox!();

    let Some(python3) = find_executable_on_path("python3") else {
        eprintln!("python3 not found in PATH, skipping test.");
        return;
    };
    let python3 = python3.to_string_lossy().to_string();

    #[cfg(target_os = "linux")]
    let sandbox_env = match linux_sandbox_test_env().await {
        Some(env) => env,
        None => return,
    };
    #[cfg(not(target_os = "linux"))]
    let sandbox_env = HashMap::new();
    let policy = SandboxPolicy::new_read_only_policy();
    let command_cwd = std::env::current_dir().expect("should be able to get current dir");
    let sandbox_cwd = command_cwd.clone();

    let mut child = spawn_command_under_sandbox(
        vec![
            python3,
            "-c".to_string(),
            "import pwd, os; print(pwd.getpwuid(os.getuid()))".to_string(),
        ],
        command_cwd,
        &policy,
        sandbox_cwd.as_path(),
        StdioPolicy::RedirectForShellTool,
        sandbox_env,
    )
    .await
    .expect("should be able to spawn python under sandbox");

    let status = child
        .wait()
        .await
        .expect("should be able to wait for child process");
    assert!(status.success(), "python exited with {status:?}");
}

#[tokio::test]
async fn sandbox_distinguishes_command_and_policy_cwds() {
    core_test_support::skip_if_sandbox!();
    let Some(touch) = find_executable_on_path("touch") else {
        eprintln!("touch not found in PATH, skipping test.");
        return;
    };
    let touch = touch.to_string_lossy().to_string();

    #[cfg(target_os = "linux")]
    let sandbox_env = match linux_sandbox_test_env().await {
        Some(env) => env,
        None => return,
    };
    #[cfg(not(target_os = "linux"))]
    let sandbox_env = HashMap::new();
    let temp = tempfile::tempdir().expect("should be able to create temp dir");
    let sandbox_root = temp.path().join("sandbox");
    let command_root = temp.path().join("command");
    create_dir_all(&sandbox_root).await.expect("mkdir");
    create_dir_all(&command_root).await.expect("mkdir");
    let canonical_sandbox_root = tokio::fs::canonicalize(&sandbox_root)
        .await
        .expect("canonicalize sandbox root");
    let canonical_allowed_path = canonical_sandbox_root.join("allowed.txt");

    let disallowed_path = command_root.join("forbidden.txt");

    // Note writable_roots is empty: verify that `canonical_allowed_path` is
    // writable only because it is under the sandbox policy cwd, not because it
    // is under a writable root.
    let policy = SandboxPolicy::WorkspaceWrite {
        writable_roots: vec![],
        read_only_access: Default::default(),
        network_access: false,
        exclude_tmpdir_env_var: true,
        exclude_slash_tmp: true,
    };

    // Attempt to write inside the command cwd, which is outside of the sandbox policy cwd.
    let mut child = spawn_command_under_sandbox(
        vec![
            "bash".to_string(),
            "-lc".to_string(),
            "echo forbidden > forbidden.txt".to_string(),
        ],
        command_root.clone(),
        &policy,
        canonical_sandbox_root.as_path(),
        StdioPolicy::Inherit,
        sandbox_env.clone(),
    )
    .await
    .expect("should spawn command writing to forbidden path");

    let status = child
        .wait()
        .await
        .expect("should wait for forbidden command");
    assert!(
        !status.success(),
        "sandbox unexpectedly allowed writing to command cwd: {status:?}"
    );
    let forbidden_exists = tokio::fs::try_exists(&disallowed_path)
        .await
        .expect("try_exists failed");
    assert!(
        !forbidden_exists,
        "forbidden path should not have been created"
    );

    // Writing to the sandbox policy cwd after changing directories into it should succeed.
    let mut child = spawn_command_under_sandbox(
        vec![touch, canonical_allowed_path.to_string_lossy().into_owned()],
        command_root,
        &policy,
        canonical_sandbox_root.as_path(),
        StdioPolicy::Inherit,
        sandbox_env,
    )
    .await
    .expect("should spawn command writing to sandbox root");

    let status = child.wait().await.expect("should wait for allowed command");
    assert!(
        status.success(),
        "sandbox blocked allowed write: {status:?}"
    );
    let allowed_exists = tokio::fs::try_exists(&canonical_allowed_path)
        .await
        .expect("try_exists allowed failed");
    assert!(allowed_exists, "allowed path should exist");
}

fn unix_sock_body() {
    unsafe {
        let mut fds = [0i32; 2];
        let r = libc::socketpair(libc::AF_UNIX, libc::SOCK_DGRAM, 0, fds.as_mut_ptr());
        assert_eq!(
            r,
            0,
            "socketpair(AF_UNIX, SOCK_DGRAM) failed: {}",
            io::Error::last_os_error()
        );

        let msg = b"hello_unix";
        // write() from one end (generic write is allowed)
        let sent = libc::write(fds[0], msg.as_ptr() as *const libc::c_void, msg.len());
        assert!(sent >= 0, "write() failed: {}", io::Error::last_os_error());

        // recvfrom() on the other end. We don’t need the address for socketpair,
        // so we pass null pointers for src address.
        let mut buf = [0u8; 64];
        let recvd = libc::recvfrom(
            fds[1],
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        assert!(
            recvd >= 0,
            "recvfrom() failed: {}",
            io::Error::last_os_error()
        );

        let recvd_slice = &buf[..(recvd as usize)];
        assert_eq!(
            recvd_slice,
            &msg[..],
            "payload mismatch: sent {} bytes, got {} bytes",
            msg.len(),
            recvd
        );

        // Also exercise AF_UNIX stream socketpair quickly to ensure AF_UNIX in general works.
        let mut sfds = [0i32; 2];
        let sr = libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, sfds.as_mut_ptr());
        assert_eq!(
            sr,
            0,
            "socketpair(AF_UNIX, SOCK_STREAM) failed: {}",
            io::Error::last_os_error()
        );
        let snt2 = libc::write(sfds[0], msg.as_ptr() as *const libc::c_void, msg.len());
        assert!(
            snt2 >= 0,
            "write(stream) failed: {}",
            io::Error::last_os_error()
        );
        let mut b2 = [0u8; 64];
        let rcv2 = libc::recv(sfds[1], b2.as_mut_ptr() as *mut libc::c_void, b2.len(), 0);
        assert!(
            rcv2 >= 0,
            "recv(stream) failed: {}",
            io::Error::last_os_error()
        );

        // Clean up
        let _ = libc::close(sfds[0]);
        let _ = libc::close(sfds[1]);
        let _ = libc::close(fds[0]);
        let _ = libc::close(fds[1]);
    }
}

#[cfg(target_os = "linux")]
fn sockaddr_un_for_path(path: &Path) -> (libc::sockaddr_un, libc::socklen_t) {
    let path_bytes = path.as_os_str().as_bytes();

    let mut addr = unsafe { std::mem::zeroed::<libc::sockaddr_un>() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    assert!(
        path_bytes.len() < addr.sun_path.len(),
        "unix socket path too long: {}",
        path.display()
    );

    unsafe {
        std::ptr::copy_nonoverlapping(
            path_bytes.as_ptr().cast::<libc::c_char>(),
            addr.sun_path.as_mut_ptr(),
            path_bytes.len(),
        );
    }

    let len = (std::mem::size_of::<libc::sa_family_t>() + path_bytes.len() + 1)
        .try_into()
        .expect("sockaddr_un length should fit into socklen_t");
    (addr, len)
}

#[cfg(target_os = "linux")]
fn af_inet_socket_denied_body() {
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        assert_eq!(fd, -1, "AF_INET socket should be denied");
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::EPERM),
            "AF_INET socket should fail with EPERM"
        );
    }
}

#[cfg(target_os = "linux")]
fn unix_socket_path_calls_work_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stream_path = dir.path().join("stream.sock");
    let dgram_path = dir.path().join("dgram.sock");
    let payload = b"hello_unix_path";

    unsafe {
        let stream_fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
        assert!(
            stream_fd >= 0,
            "AF_UNIX stream socket failed: {}",
            io::Error::last_os_error()
        );
        let (stream_addr, stream_addr_len) = sockaddr_un_for_path(&stream_path);
        let bind_stream = libc::bind(
            stream_fd,
            (&stream_addr as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
            stream_addr_len,
        );
        assert_eq!(
            bind_stream,
            0,
            "bind(AF_UNIX stream) failed: {}",
            io::Error::last_os_error()
        );
        let listen_result = libc::listen(stream_fd, 1);
        assert_eq!(
            listen_result,
            0,
            "listen(AF_UNIX stream) failed: {}",
            io::Error::last_os_error()
        );
        let _ = libc::close(stream_fd);

        let server_fd = libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM, 0);
        assert!(
            server_fd >= 0,
            "AF_UNIX dgram server socket failed: {}",
            io::Error::last_os_error()
        );
        let (server_addr, server_addr_len) = sockaddr_un_for_path(&dgram_path);
        let bind_server = libc::bind(
            server_fd,
            (&server_addr as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
            server_addr_len,
        );
        assert_eq!(
            bind_server,
            0,
            "bind(AF_UNIX dgram) failed: {}",
            io::Error::last_os_error()
        );

        let client_fd = libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM, 0);
        assert!(
            client_fd >= 0,
            "AF_UNIX dgram client socket failed: {}",
            io::Error::last_os_error()
        );
        let connect_result = libc::connect(
            client_fd,
            (&server_addr as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
            server_addr_len,
        );
        assert_eq!(
            connect_result,
            0,
            "connect(AF_UNIX dgram) failed: {}",
            io::Error::last_os_error()
        );

        let sent = libc::write(
            client_fd,
            payload.as_ptr().cast::<libc::c_void>(),
            payload.len(),
        );
        assert!(
            sent >= 0,
            "write(AF_UNIX dgram) failed: {}",
            io::Error::last_os_error()
        );

        let mut recv_buf = [0u8; 64];
        let recvd = libc::recv(
            server_fd,
            recv_buf.as_mut_ptr().cast::<libc::c_void>(),
            recv_buf.len(),
            0,
        );
        assert!(
            recvd >= 0,
            "recv(AF_UNIX dgram) failed: {}",
            io::Error::last_os_error()
        );
        assert_eq!(&recv_buf[..(recvd as usize)], payload);

        let _ = libc::close(client_fd);
        let _ = libc::close(server_fd);
    }

    let _ = std::fs::remove_file(stream_path);
    let _ = std::fs::remove_file(dgram_path);
}

#[cfg(target_os = "linux")]
fn unix_socket_getsockopt_so_error_works_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dgram_path = dir.path().join("getsockopt.sock");

    unsafe {
        let server_fd = libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM, 0);
        assert!(
            server_fd >= 0,
            "AF_UNIX dgram server socket failed: {}",
            io::Error::last_os_error()
        );
        let (server_addr, server_addr_len) = sockaddr_un_for_path(&dgram_path);
        let bind_server = libc::bind(
            server_fd,
            (&server_addr as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
            server_addr_len,
        );
        assert_eq!(
            bind_server,
            0,
            "bind(AF_UNIX dgram) failed: {}",
            io::Error::last_os_error()
        );

        let client_fd = libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM, 0);
        assert!(
            client_fd >= 0,
            "AF_UNIX dgram client socket failed: {}",
            io::Error::last_os_error()
        );

        let connect_result = libc::connect(
            client_fd,
            (&server_addr as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
            server_addr_len,
        );
        assert_eq!(
            connect_result,
            0,
            "connect(AF_UNIX dgram) failed: {}",
            io::Error::last_os_error()
        );

        let mut so_error: libc::c_int = -1;
        let mut optlen: libc::socklen_t = std::mem::size_of::<libc::c_int>()
            .try_into()
            .expect("size_of::<c_int>() should fit into socklen_t");
        let getsockopt_result = libc::getsockopt(
            client_fd,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            (&mut so_error as *mut libc::c_int).cast::<libc::c_void>(),
            &mut optlen,
        );
        assert_eq!(
            getsockopt_result,
            0,
            "getsockopt(SO_ERROR) failed: {}",
            io::Error::last_os_error()
        );
        assert_eq!(so_error, 0, "SO_ERROR should be zero");

        let _ = libc::close(client_fd);
        let _ = libc::close(server_fd);
    }

    let _ = std::fs::remove_file(dgram_path);
}

#[tokio::test]
async fn allow_unix_socketpair_recvfrom() {
    run_code_under_sandbox(
        "allow_unix_socketpair_recvfrom",
        &SandboxPolicy::new_read_only_policy(),
        || async { unix_sock_body() },
    )
    .await
    .expect("should be able to reexec");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn deny_af_inet_socket_in_workspace_write() {
    let status = run_code_under_sandbox(
        "deny_af_inet_socket_in_workspace_write",
        &SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![],
            read_only_access: Default::default(),
            network_access: false,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        },
        || async { af_inet_socket_denied_body() },
    )
    .await
    .expect("should be able to reexec");

    if let Some(status) = status {
        assert!(status.success(), "child exited with {status:?}");
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn allow_unix_socket_path_calls_in_workspace_write() {
    let status = run_code_under_sandbox(
        "allow_unix_socket_path_calls_in_workspace_write",
        &SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![],
            read_only_access: Default::default(),
            network_access: false,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        },
        || async { unix_socket_path_calls_work_body() },
    )
    .await
    .expect("should be able to reexec");

    if let Some(status) = status {
        assert!(status.success(), "child exited with {status:?}");
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn allow_unix_socket_getsockopt_so_error_in_workspace_write() {
    let status = run_code_under_sandbox(
        "allow_unix_socket_getsockopt_so_error_in_workspace_write",
        &SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![],
            read_only_access: Default::default(),
            network_access: false,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        },
        || async { unix_socket_getsockopt_so_error_works_body() },
    )
    .await
    .expect("should be able to reexec");

    if let Some(status) = status {
        assert!(status.success(), "child exited with {status:?}");
    }
}

const IN_SANDBOX_ENV_VAR: &str = "IN_SANDBOX";

#[expect(clippy::expect_used)]
pub async fn run_code_under_sandbox<F, Fut>(
    test_selector: &str,
    policy: &SandboxPolicy,
    child_body: F,
) -> io::Result<Option<ExitStatus>>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    if std::env::var(IN_SANDBOX_ENV_VAR).is_err() {
        let exe = std::env::current_exe()?;
        let mut cmds = vec![exe.to_string_lossy().into_owned(), "--exact".into()];
        let mut stdio_policy = StdioPolicy::RedirectForShellTool;
        // Allow for us to pass forward --nocapture / use the right stdio policy.
        if std::env::args().any(|a| a == "--nocapture") {
            cmds.push("--nocapture".into());
            stdio_policy = StdioPolicy::Inherit;
        }
        cmds.push(test_selector.into());

        // Your existing launcher:
        let command_cwd = std::env::current_dir().expect("should be able to get current dir");
        let sandbox_cwd = command_cwd.clone();
        let mut child = spawn_command_under_sandbox(
            cmds,
            command_cwd,
            policy,
            sandbox_cwd.as_path(),
            stdio_policy,
            HashMap::from([("IN_SANDBOX".into(), "1".into())]),
        )
        .await?;

        let status = child.wait().await?;
        Ok(Some(status))
    } else {
        // Child branch: run the provided body.
        child_body().await;
        Ok(None)
    }
}
