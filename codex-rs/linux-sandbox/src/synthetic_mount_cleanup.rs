use crate::linux_run_main::SyntheticMountTargetRegistration;
use crate::linux_run_main::cleanup_synthetic_mount_targets;
use crate::linux_run_main::process_is_active;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::Duration;

const CLEANUP_WORKER_READY: u8 = 1;
const CLEANUP_WORKER_DISARM: u8 = 2;

/// Keeps synthetic mount cleanup alive if the sandbox supervisor is hard-killed.
pub(crate) struct SyntheticMountCleanupGuard {
    control: UnixStream,
    worker_pid: libc::pid_t,
}

impl SyntheticMountCleanupGuard {
    pub(crate) fn spawn(
        bwrap_pid: libc::pid_t,
        registrations: &[SyntheticMountTargetRegistration],
        inherited_fds_to_close: &[libc::c_int],
    ) -> Option<Self> {
        if registrations.is_empty() {
            return None;
        }

        let registrations = registrations.to_vec();
        let inherited_fds_to_close = inherited_fds_to_close.to_vec();
        let (mut supervisor_control, worker_control) = UnixStream::pair()
            .unwrap_or_else(|err| panic!("failed to create synthetic mount cleanup socket: {err}"));
        let worker_pid = unsafe { libc::fork() };
        if worker_pid < 0 {
            let err = io::Error::last_os_error();
            panic!("failed to fork synthetic mount cleanup worker: {err}");
        }

        if worker_pid == 0 {
            drop(supervisor_control);
            let exit_code = run_cleanup_worker(
                worker_control,
                bwrap_pid,
                &registrations,
                &inherited_fds_to_close,
            );
            unsafe { libc::_exit(exit_code) };
        }

        drop(worker_control);
        let mut ready = [0_u8; 1];
        supervisor_control
            .read_exact(&mut ready)
            .unwrap_or_else(|err| panic!("synthetic mount cleanup worker failed to start: {err}"));
        if ready[0] != CLEANUP_WORKER_READY {
            panic!("synthetic mount cleanup worker sent an invalid readiness value");
        }

        Some(Self {
            control: supervisor_control,
            worker_pid,
        })
    }

    pub(crate) fn disarm(mut self) {
        let _ = self.control.write_all(&[CLEANUP_WORKER_DISARM]);
        drop(self.control);
        wait_for_worker(self.worker_pid);
    }
}

fn run_cleanup_worker(
    mut control: UnixStream,
    bwrap_pid: libc::pid_t,
    registrations: &[SyntheticMountTargetRegistration],
    inherited_fds_to_close: &[libc::c_int],
) -> libc::c_int {
    if unsafe { libc::setsid() } < 0 {
        return 1;
    }
    for fd in inherited_fds_to_close {
        if *fd >= 0 {
            unsafe {
                libc::close(*fd);
            }
        }
    }
    for fd in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        unsafe {
            libc::close(fd);
        }
    }

    let pidfd = open_pidfd(bwrap_pid);
    if control.write_all(&[CLEANUP_WORKER_READY]).is_err() {
        return 1;
    }

    let mut command = [0_u8; 1];
    loop {
        match control.read(&mut command) {
            Ok(0) => break,
            Ok(1) if command[0] == CLEANUP_WORKER_DISARM => return 0,
            Ok(1) => return 1,
            Ok(_) => unreachable!("single-byte cleanup control read returned more than one byte"),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return 1,
        }
    }

    wait_for_bwrap_exit(bwrap_pid, pidfd);
    cleanup_synthetic_mount_targets(registrations);
    0
}

fn open_pidfd(pid: libc::pid_t) -> Option<libc::c_int> {
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    libc::c_int::try_from(fd).ok().filter(|fd| *fd >= 0)
}

fn wait_for_bwrap_exit(pid: libc::pid_t, pidfd: Option<libc::c_int>) {
    if let Some(pidfd) = pidfd {
        let mut pollfd = libc::pollfd {
            fd: pidfd,
            events: libc::POLLIN,
            revents: 0,
        };
        let process_exited = loop {
            let result = unsafe {
                libc::poll(&mut pollfd, /*nfds*/ 1, /*timeout*/ -1)
            };
            if result > 0 {
                break true;
            }
            if result < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break false;
        };
        unsafe {
            libc::close(pidfd);
        }
        if process_exited {
            return;
        }
    }

    while process_is_active(pid) {
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_worker(worker_pid: libc::pid_t) {
    loop {
        let result = unsafe {
            libc::waitpid(worker_pid, std::ptr::null_mut(), /*options*/ 0)
        };
        if result >= 0 {
            return;
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return;
        }
    }
}
