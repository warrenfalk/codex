//! Isolate process-wide test state before installing executable helper aliases.
//! Helper invocations retain the explicit Codex home provided by their caller.

use codex_apply_patch::CODEX_CORE_APPLY_PATCH_ARG1;
#[cfg(unix)]
use codex_exec_server::CODEX_ARG0_EXEC_HELPER_ARG1;
use codex_exec_server::CODEX_FS_HELPER_ARG1;
use codex_sandboxing::landlock::CODEX_LINUX_SANDBOX_ARG0;
use codex_test_binary_support::TestBinaryDispatchGuard;
use codex_test_binary_support::TestBinaryDispatchMode;
use codex_test_binary_support::configure_test_binary_dispatch;
use ctor::ctor;
use std::sync::OnceLock;

/// Mark a re-executed fixture as a helper that retains its caller's isolated state.
/// Set this to `helper` only for subprocesses that must use an explicit Codex home
/// or run inside a sandbox that cannot allocate a new one.
pub const TEST_BINARY_ROLE_ENV: &str = "CODEX_TEST_BINARY_ROLE";

pub(super) static TEST_BINARY_DISPATCH_GUARD: OnceLock<Option<TestBinaryDispatchGuard>> =
    OnceLock::new();

#[ctor]
fn configure_test_process() {
    TEST_BINARY_DISPATCH_GUARD.get_or_init(|| {
        configure_test_binary_dispatch("codex-core-tests", |exe_name, argv1| {
            if argv1 == Some(CODEX_CORE_APPLY_PATCH_ARG1) {
                return TestBinaryDispatchMode::DispatchArg0Only;
            }
            #[cfg(unix)]
            if argv1 == Some(CODEX_ARG0_EXEC_HELPER_ARG1) {
                return TestBinaryDispatchMode::DispatchArg0Only;
            }
            if argv1 == Some(CODEX_FS_HELPER_ARG1) {
                return TestBinaryDispatchMode::DispatchArg0Only;
            }
            if argv1 == Some(codex_sandboxing::CODEX_WINDOWS_MXC_ARG1) {
                return TestBinaryDispatchMode::DispatchArg0Only;
            }
            if exe_name == CODEX_LINUX_SANDBOX_ARG0 {
                return TestBinaryDispatchMode::DispatchArg0Only;
            }
            if std::env::var(TEST_BINARY_ROLE_ENV).as_deref() == Ok("helper") {
                return TestBinaryDispatchMode::Skip;
            }
            TestBinaryDispatchMode::IsolateTestProcess
        })
    });
}
