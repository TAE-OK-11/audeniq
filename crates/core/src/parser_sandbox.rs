//! Mandatory Linux sandbox for every process that parses user-controlled files.
//! Landlock protects files; seccomp denies networking and cross-process access.
use std::{io, path::Path, process::Command};

/// Deployment check: exercise the actual enforced policy inside its container.
pub fn self_test() -> io::Result<()> {
    let mut command = Command::new("python3");
    command.args(["-c", "import os,socket\nassert 'DATABASE_URL' not in os.environ\nfor path in ['/proc/self/environ', '/etc/shadow']:\n try: open(path).read()\n except PermissionError: pass\n else: raise Exception('filesystem isolation failed')\ntry: socket.socket()\nexcept PermissionError: pass\nelse: raise Exception('network isolation failed')\ntry: os.pidfd_open(os.getppid())\nexcept PermissionError: pass\nelse: raise Exception('cross-process isolation failed')"]);
    restrict(&mut command, &[], &[])?;
    if !command.status()?.success() {
        return Err(io::Error::from_raw_os_error(libc::EPERM));
    }
    Ok(())
}

pub fn restrict(command: &mut Command, inputs: &[&Path], outputs: &[&Path]) -> io::Result<()> {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        linux::restrict(command, inputs, outputs)
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        let _ = (command, inputs, outputs);
        Err(io::Error::from_raw_os_error(libc::ENOTSUP))
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod linux {
    use super::*;
    use std::os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{fs::OpenOptionsExt, process::CommandExt},
    };
    const EXEC: u64 = 1;
    const WRITE: u64 = 1 << 1;
    const READ: u64 = 1 << 2;
    const READ_DIR: u64 = 1 << 3;
    const TRUNCATE: u64 = 1 << 14;
    const ALL: u64 = (1 << 15) - 1;
    #[repr(C, packed)]
    struct Beneath {
        access: u64,
        fd: i32,
    }

    fn add(fd: &OwnedFd, path: &Path, access: u64) -> io::Result<()> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_CLOEXEC)
            .open(path)?;
        let rule = Beneath {
            access,
            fd: file.as_raw_fd(),
        };
        // SAFETY: the kernel copies this fixed ABI structure; both descriptors are live.
        if unsafe { libc::syscall(libc::SYS_landlock_add_rule, fd.as_raw_fd(), 1, &rule, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn restrict(
        command: &mut Command,
        inputs: &[&Path],
        outputs: &[&Path],
    ) -> io::Result<()> {
        // ABI 3 is required: older ABIs cannot restrict file truncation.
        // SAFETY: the version query has no pointers to user memory.
        if unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<u8>(),
                0,
                1,
            )
        } < 3
        {
            return Err(io::Error::from_raw_os_error(libc::ENOTSUP));
        }
        let rights = ALL;
        // SAFETY: the ABI 1-compatible prefix contains only handled_access_fs.
        let raw = unsafe { libc::syscall(libc::SYS_landlock_create_ruleset, &rights, 8, 0) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: raw is a newly owned descriptor returned by the kernel.
        let rules = unsafe { OwnedFd::from_raw_fd(raw as i32) };
        for path in ["/usr", "/bin", "/lib", "/lib64"] {
            if Path::new(path).exists() {
                add(&rules, Path::new(path), EXEC | READ | READ_DIR)?;
            }
        }
        for path in [
            "/etc/ld.so.cache",
            "/etc/fonts",
            "/dev/urandom",
            "/dev/random",
        ] {
            let p = Path::new(path);
            if p.exists() {
                add(&rules, p, READ | if p.is_dir() { READ_DIR } else { 0 })?;
            }
        }
        add(&rules, Path::new("/dev/null"), READ | WRITE | TRUNCATE)?;
        for path in inputs {
            add(
                &rules,
                path,
                READ | if path.is_dir() { READ_DIR } else { 0 },
            )?;
        }
        for path in outputs {
            if !path.exists() {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)?;
            }
            let meta = std::fs::symlink_metadata(path)?;
            if meta.file_type().is_symlink() {
                return Err(io::Error::from_raw_os_error(libc::ELOOP));
            }
            let access = if meta.is_dir() {
                // Only the unique, private processing directory may be writable.
                READ | READ_DIR | WRITE | TRUNCATE | (1 << 4) | (1 << 5) | (1 << 7) | (1 << 8)
            } else {
                READ | WRITE | TRUNCATE
            };
            add(&rules, path, access)?;
        }
        let program = command.get_program();
        let program_path = Path::new(program);
        if program_path.is_absolute() && program_path.exists() {
            add(&rules, program_path, EXEC | READ)?;
        }
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .env("HOME", "/nonexistent")
            .env("TMPDIR", "/nonexistent")
            .env("OMP_THREAD_LIMIT", "1")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .current_dir("/");
        let denied = [
            libc::SYS_socket,
            libc::SYS_socketpair,
            libc::SYS_connect,
            libc::SYS_bind,
            libc::SYS_listen,
            libc::SYS_accept,
            libc::SYS_accept4,
            libc::SYS_sendto,
            libc::SYS_sendmsg,
            libc::SYS_sendmmsg,
            libc::SYS_recvfrom,
            libc::SYS_recvmsg,
            libc::SYS_recvmmsg,
            libc::SYS_shutdown,
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_kill,
            libc::SYS_tkill,
            libc::SYS_tgkill,
            libc::SYS_rt_sigqueueinfo,
            libc::SYS_rt_tgsigqueueinfo,
            libc::SYS_pidfd_open,
            libc::SYS_pidfd_getfd,
            libc::SYS_pidfd_send_signal,
            libc::SYS_process_madvise,
            libc::SYS_process_mrelease,
            libc::SYS_mount,
            libc::SYS_umount2,
            libc::SYS_pivot_root,
            libc::SYS_chroot,
            libc::SYS_unshare,
            libc::SYS_setns,
            libc::SYS_bpf,
            libc::SYS_perf_event_open,
            libc::SYS_keyctl,
            libc::SYS_add_key,
            libc::SYS_request_key,
            libc::SYS_io_uring_setup,
            libc::SYS_open_by_handle_at,
            libc::SYS_memfd_create,
            libc::SYS_execveat,
        ];
        fn ins(code: u16, jt: u8, jf: u8, k: u32) -> libc::sock_filter {
            libc::sock_filter { code, jt, jf, k }
        }
        #[cfg(target_arch = "x86_64")]
        let arch = 0xc000003e;
        #[cfg(target_arch = "aarch64")]
        let arch = 0xc00000b7;
        let mut filter = vec![
            ins(0x20, 0, 0, 4),
            ins(0x15, 1, 0, arch),
            ins(0x06, 0, 0, 0x80000000),
            ins(0x20, 0, 0, 0),
            // Reject x32/other alternate syscall tables rather than bypassing the filter.
            ins(0x45, 0, 1, 0x40000000),
            ins(0x06, 0, 0, 0x80000000),
        ];
        for syscall in denied {
            filter.push(ins(0x15, 0, 1, syscall as u32));
            filter.push(ins(0x06, 0, 0, 0x00050000 | libc::EPERM as u32));
        }
        filter.push(ins(0x06, 0, 0, 0x7fff0000));
        // SAFETY: all allocations, file opens and policy construction happen before
        // fork. This hook invokes only syscalls, with stable captured buffers.
        unsafe {
            command.pre_exec(move || {
                for (resource, max) in [
                    (libc::RLIMIT_CORE, 0),
                    (libc::RLIMIT_AS, 1024 * 1024 * 1024),
                    (libc::RLIMIT_FSIZE, 536870912),
                    (libc::RLIMIT_CPU, 600),
                    (libc::RLIMIT_NOFILE, 128),
                ] {
                    let limit = libc::rlimit {
                        rlim_cur: max,
                        rlim_max: max,
                    };
                    if libc::setrlimit(resource, &limit) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                    || libc::syscall(libc::SYS_landlock_restrict_self, rules.as_raw_fd(), 0) != 0
                {
                    return Err(io::Error::last_os_error());
                }
                let policy = libc::sock_fprog {
                    len: filter.len() as u16,
                    filter: filter.as_ptr() as *mut _,
                };
                if libc::prctl(libc::PR_SET_SECCOMP, 2, &policy) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(())
    }
}
