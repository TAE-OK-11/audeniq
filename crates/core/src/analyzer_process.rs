//! Engine-independent lifetime and IO bounds for audio analyzer processes.
use std::{
    io::{self, Read},
    ops::{Deref, DerefMut},
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, Command},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(crate) struct AnalyzerProcess {
    child: Child,
    cancelled: Arc<AtomicBool>,
    readers: Vec<JoinHandle<()>>,
    deadline: Instant,
}

impl AnalyzerProcess {
    pub(crate) fn spawn(command: &mut Command, deadline: Instant) -> io::Result<Self> {
        command.process_group(0);
        let parent = unsafe { libc::getpid() };
        // SAFETY: only async-signal-safe syscalls run after fork. Check the
        // parent again to close the death-before-prctl race.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    return Err(io::Error::from_raw_os_error(libc::ESRCH));
                }
                Ok(())
            });
        }
        Ok(Self {
            child: command.spawn()?,
            cancelled: Arc::new(AtomicBool::new(false)),
            readers: Vec::with_capacity(3),
            deadline,
        })
    }

    pub(crate) fn reader<R: Read + AsRawFd>(&self, inner: R) -> io::Result<AnalyzerReader<R>> {
        let fd = inner.as_raw_fd();
        // SAFETY: fd is live and owned by inner; only its read end changes.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags == -1 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(AnalyzerReader {
            inner,
            cancelled: self.cancelled.clone(),
            deadline: self.deadline,
        })
    }

    pub(crate) fn spawn_reader(
        &mut self,
        name: &str,
        read: impl FnOnce() + Send + 'static,
    ) -> io::Result<()> {
        let thread = std::thread::Builder::new()
            .name(name.into())
            .stack_size(256 * 1024)
            .spawn(read)?;
        self.readers.push(thread);
        Ok(())
    }
}

impl Deref for AnalyzerProcess {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}
impl DerefMut for AnalyzerProcess {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}
impl Drop for AnalyzerProcess {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        // Each analyzer owns a new process group. Terminate inherited pipe
        // writers as well as the direct child, then reap and join readers.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

pub(crate) struct AnalyzerReader<R> {
    inner: R,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl<R: Read + AsRawFd> Read for AnalyzerReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.cancelled.load(Ordering::Relaxed) || Instant::now() >= self.deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            match self.inner.read(buf) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    let left = self.deadline.saturating_duration_since(Instant::now());
                    let millis = left.min(Duration::from_millis(50)).as_millis().max(1) as i32;
                    let mut fd = libc::pollfd {
                        fd: self.inner.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    // SAFETY: poll borrows one live descriptor and fixed buffer.
                    if unsafe { libc::poll(&mut fd, 1, millis) } < 0 {
                        let error = io::Error::last_os_error();
                        if error.kind() != io::ErrorKind::Interrupted {
                            return Err(error);
                        }
                    }
                }
                result => return result,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn dropping_analyzer_cancels_and_joins_a_reader() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30"]).stdout(Stdio::piped());
        let mut process =
            AnalyzerProcess::spawn(&mut command, Instant::now() + Duration::from_secs(60)).unwrap();
        let stdout = process.stdout.take().unwrap();
        let mut reader = process.reader(stdout).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        process
            .spawn_reader("test-reader", move || {
                let _ = reader.read(&mut [0; 1]);
                let _ = tx.send(());
            })
            .unwrap();
        let start = Instant::now();
        drop(process);
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn inherited_stdout_does_not_keep_a_reader_alive_after_deadline() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 30 & exit 0"])
            .stdout(Stdio::piped());
        let mut process =
            AnalyzerProcess::spawn(&mut command, Instant::now() + Duration::from_millis(200))
                .unwrap();
        let stdout = process.stdout.take().unwrap();
        let mut reader = process.reader(stdout).unwrap();
        let start = Instant::now();
        assert_eq!(
            reader.read(&mut [0; 1]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
