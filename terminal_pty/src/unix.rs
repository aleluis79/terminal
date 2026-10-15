//! Unix PTY implementation (Linux/macOS).
//!
//! Creates a pseudo-terminal using `openpty(3)`, forks a child process
//! that execs the requested shell, and provides non-blocking I/O on the
//! master fd.
//!
//! # Lifecycle
//!
//! ```text
//! openpty() → fork() → child: setsid + exec(shell)
//!                         parent: close(slave) + set_nonblock(master)
//! ```

use std::os::fd::{AsRawFd, IntoRawFd, OwnedFd};
use std::path::PathBuf;

use nix::fcntl::{self, FcntlArg, OFlag};
use nix::pty::openpty;
use nix::sys::{
    signal,
    wait::{waitpid, WaitPidFlag, WaitStatus},
};
use nix::unistd::{self, close, dup2, fork, setsid, ForkResult, Pid};

use super::pty::{Pty, PtyError};

/// Size of the read buffer for PTY output.
#[allow(dead_code)]
const READ_BUF_SIZE: usize = 4096;

/// Unix PTY implementation.
///
/// Holds the master file descriptor and the child PID.
pub struct UnixPty {
    /// Master file descriptor (non-blocking).
    master: OwnedFd,
    /// Child PID, if the process has been spawned.
    pid: Option<Pid>,
    /// Program that was executed.
    #[allow(dead_code)]
    program: String,
    /// Exit code, if the child process has exited.
    exit_code: Option<i32>,
}

impl UnixPty {
    /// Create a new Unix PTY and spawn the given program.
    ///
    /// Steps:
    /// 1. `openpty()` creates master/slave pair
    /// 2. `fork()` creates child process
    /// 3. Child: `setsid()`, dups slave to stdin/stdout/stderr, then `exec()`s the program
    /// 4. Parent: closes slave fd, sets master to non-blocking, returns
    ///
    /// # Errors
    ///
    /// Returns an error if `openpty`, `fork`, or setting non-blocking mode fails.
    pub fn spawn(program: &str, args: &[String]) -> Result<Self, PtyError> {
        // Fail fast when the program cannot be found: `exec` runs in the child,
        // so without this check a missing shell would only show up as a child
        // that exits immediately and the caller could not fall back (RF-67).
        let program_path = resolve_program(program)
            .ok_or_else(|| PtyError::Spawn(format!("program not found: {program}")))?;

        // Open a new PTY (no special terminal settings needed)
        let pty =
            openpty(None, None).map_err(|e| PtyError::Spawn(format!("Failed to open PTY: {e}")))?;

        let master = pty.master;
        let slave = pty.slave;

        // Fork the child process
        let child_pid =
            unsafe { fork() }.map_err(|e| PtyError::Spawn(format!("Fork failed: {e}")))?;

        match child_pid {
            ForkResult::Child => {
                // Child process

                // Create a new session
                if setsid().is_err() {
                    std::process::exit(1);
                }

                let slave_fd = slave.into_raw_fd();

                // Make the slave the controlling terminal for this session.
                // SAFETY: `slave_fd` is a valid open fd and TIOCSCTTY takes no
                // pointer argument.
                unsafe {
                    nix::libc::ioctl(slave_fd, nix::libc::TIOCSCTTY, 0);
                }

                // Wire the slave to stdin/stdout/stderr. `dup2` targets the
                // exact fds; `dup` would fill the lowest *free* fd instead,
                // leaving the shell attached to the parent's stdio.
                for fd in 0..=2 {
                    let _ = dup2(slave_fd, fd);
                }
                if slave_fd > 2 {
                    close(slave_fd).ok();
                }

                // Execute the program with its arguments as real argv entries,
                // so they are never re-interpreted (CL-25). With no configured
                // arguments keep the historical `sh -` invocation, which makes
                // shells such as bash read commands from the PTY.
                let program_cstring = std::ffi::CString::new(program).unwrap_or_default();
                let path_cstring = std::ffi::CString::new(program_path.to_string_lossy().as_ref())
                    .unwrap_or_default();
                let argument_cstrings: Vec<std::ffi::CString> = args
                    .iter()
                    .map(|arg| std::ffi::CString::new(arg.as_str()).unwrap_or_default())
                    .collect();
                let mut exec_argv: Vec<&std::ffi::CStr> = Vec::with_capacity(args.len() + 2);
                exec_argv.push(program_cstring.as_c_str());
                exec_argv.extend(argument_cstrings.iter().map(std::ffi::CString::as_c_str));
                if args.is_empty() {
                    exec_argv.push(c"-");
                }
                let _ = unistd::execv(path_cstring.as_c_str(), &exec_argv);

                // execv failed
                std::process::exit(1);
            }
            ForkResult::Parent { child } => {
                // Parent process

                // Close the slave fd in the parent by converting to raw fd and closing
                // We use into_raw_fd() so OwnedFd won't close it again on drop
                let slave_fd = slave.into_raw_fd();
                close(slave_fd)
                    .map_err(|e| PtyError::Spawn(format!("Failed to close slave fd: {e}")))?;

                // Set master to non-blocking
                let master_fd = master.as_raw_fd();
                let flags = fcntl::fcntl(master_fd, FcntlArg::F_GETFL)
                    .map_err(|e| PtyError::Spawn(format!("Failed to get flags: {e}")))?;
                fcntl::fcntl(
                    master_fd,
                    FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags | OFlag::O_NONBLOCK.bits())),
                )
                .map_err(|e| PtyError::Spawn(format!("Failed to set non-blocking: {e}")))?;

                Ok(Self {
                    master,
                    pid: Some(child),
                    program: program.to_string(),
                    exit_code: None,
                })
            }
        }
    }

    /// Create a PTY without spawning a process (for testing).
    ///
    /// # Errors
    ///
    /// Returns an error if `openpty` or setting non-blocking mode fails.
    pub fn new_empty() -> Result<Self, PtyError> {
        let pty =
            openpty(None, None).map_err(|e| PtyError::Spawn(format!("Failed to open PTY: {e}")))?;

        let master_fd = pty.master.as_raw_fd();

        // Set master to non-blocking
        let flags = fcntl::fcntl(master_fd, FcntlArg::F_GETFL)
            .map_err(|e| PtyError::Spawn(format!("Failed to get flags: {e}")))?;
        fcntl::fcntl(
            master_fd,
            FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags | OFlag::O_NONBLOCK.bits())),
        )
        .map_err(|e| PtyError::Spawn(format!("Failed to set non-blocking: {e}")))?;

        // Close slave so we can detect EOF
        let slave_fd = pty.slave.into_raw_fd();
        close(slave_fd).ok();

        Ok(Self {
            master: pty.master,
            pid: None,
            program: String::new(),
            exit_code: None,
        })
    }

    /// Perform a single non-blocking read from the master fd.
    fn do_read(&self, buf: &mut [u8]) -> Result<Option<usize>, PtyError> {
        match unistd::read(self.master.as_raw_fd(), buf) {
            Ok(n) => Ok(Some(n)),
            Err(nix::errno::Errno::EAGAIN) => Ok(Some(0)),
            Err(nix::errno::Errno::EIO) => {
                // EOF: slave side closed
                Ok(Some(0))
            }
            Err(e) => Err(PtyError::Read(format!("Read from PTY failed: {e}"))),
        }
    }
}

/// Resolve `program` the way `execvp(3)` would: a name containing a slash is
/// used as an explicit path, a bare name is looked up in `PATH`.
fn resolve_program(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let path = PathBuf::from(program);
        return path.is_file().then_some(path);
    }

    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

impl Pty for UnixPty {
    /// Spawn a new process on this PTY.
    fn spawn(program: &str, args: &[String]) -> Result<Self, PtyError>
    where
        Self: Sized,
    {
        Self::spawn(program, args)
    }

    /// Read data from the PTY master fd.
    ///
    /// Returns:
    /// - `Ok(Some(n))` with `n > 0` bytes read
    /// - `Ok(Some(0))` if no data available right now (EAGAIN/EWOULDBLOCK)
    /// - `Ok(None)` if EOF detected (child exited and master closed)
    ///
    /// # Errors
    ///
    /// Returns an error if a read error occurs that is not EAGAIN or EIO.
    fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, PtyError> {
        // Reap the child if it has exited. Once its PID is gone the terminal
        // has been drained, so reading with no data left means end of stream
        // (this is what surfaces `PollResult::Exited` to the session).
        let _ = self.check_child_exit()?;
        let child_gone = self.pid.is_none();

        match self.do_read(buf) {
            // Nothing left and the child is gone: EOF.
            Ok(Some(0)) if child_gone => Ok(None),
            // Nothing to read right now, but the child is still alive.
            Ok(Some(0)) => Ok(Some(0)),
            Ok(n) => Ok(n),
            Err(e) => Err(e),
        }
    }

    /// Write data to the PTY master fd.
    ///
    /// This sends bytes to the slave side, which the shell reads as input.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    fn write(&mut self, buf: &[u8]) -> Result<usize, PtyError> {
        let n = unistd::write(&self.master, buf)
            .map_err(|e| PtyError::Write(format!("Write to PTY failed: {e}")))?;
        Ok(n)
    }

    /// Get the PID of the child process.
    fn pid(&self) -> Option<u32> {
        self.pid.map(|p| p.as_raw().cast_unsigned())
    }

    /// Check if the child process is still running.
    fn is_alive(&self) -> bool {
        self.pid.is_some()
    }

    /// Get the exit code of the child process, if it has exited.
    fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Send SIGINT (Ctrl+C) to the child process group.
    ///
    /// SIGINT is sent to the process group of the child, which is the
    /// standard way terminals implement Ctrl+C.
    fn send_interrupt(&self) {
        if let Some(pid) = self.pid {
            // Send to process group (negative PID)
            let result = signal::killpg(Pid::from_raw(-pid.as_raw()), signal::Signal::SIGINT);
            if result.is_err() {
                // Process may have already exited
                eprintln!("send_interrupt: failed to send SIGINT to pid {pid}");
            }
        }
    }

    /// Send EOF (Ctrl+D) to the child process.
    ///
    /// In a terminal, EOF is sent by writing the ASCII EOT character (0x04)
    /// to the PTY master. The shell interprets this as EOF when it's at the
    /// beginning of a line.
    fn send_eof(&mut self) {
        let _ = unistd::write(&self.master, &[0x04]);
    }

    /// Resize the pseudo-terminal window (`TIOCSWINSZ`).
    ///
    /// # Errors
    ///
    /// Returns an error if the `TIOCSWINSZ` ioctl fails.
    fn resize(&mut self, rows: u16, cols: u16) -> Result<(), PtyError> {
        let ws = nix::libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: `master` is an open fd and `ws` outlives the call.
        let ret = unsafe { nix::libc::ioctl(self.master.as_raw_fd(), nix::libc::TIOCSWINSZ, &ws) };
        if ret == -1 {
            return Err(PtyError::Resize(format!(
                "TIOCSWINSZ failed: {}",
                nix::errno::Errno::last()
            )));
        }
        Ok(())
    }

    /// Wait for the child process to exit.
    ///
    /// Returns the exit status:
    /// - `Ok(Some(0))` for normal exit with code 0
    /// - `Ok(Some(n))` for normal exit with non-zero code n
    /// - `Ok(Some(-n))` for termination by signal n
    /// - `Ok(None)` if the process is still running
    ///
    /// # Errors
    ///
    /// Returns an error if `waitpid` fails with an unexpected error.
    fn wait(&mut self) -> Result<Option<i32>, PtyError> {
        let Some(pid) = self.pid else {
            return Ok(None);
        };

        match waitpid(pid, None) {
            Ok(WaitStatus::Exited(_, code)) => {
                self.pid = None;
                Ok(Some(code))
            }
            Ok(WaitStatus::Signaled(_, sig, _)) => {
                self.pid = None;
                Ok(Some(-(sig as i32)))
            }
            Ok(
                WaitStatus::PtraceEvent(_, _, _)
                | WaitStatus::PtraceSyscall(_)
                | WaitStatus::StillAlive
                | WaitStatus::Continued(_)
                | WaitStatus::Stopped(_, _),
            ) => Ok(None),
            Err(nix::errno::Errno::ECHILD) => {
                self.pid = None;
                Ok(None)
            }
            Err(e) => Err(PtyError::Read(format!("waitpid failed: {e}"))),
        }
    }
}

impl UnixPty {
    /// Check if a child process has exited (non-blocking).
    fn check_child_exit(&mut self) -> Result<Option<i32>, PtyError> {
        let Some(pid) = self.pid else {
            return Ok(None);
        };

        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, code)) => {
                self.pid = None;
                self.exit_code = Some(code);
                Ok(Some(code))
            }
            Ok(WaitStatus::Signaled(_, sig, _)) => {
                self.pid = None;
                let code = -(sig as i32);
                self.exit_code = Some(code);
                Ok(Some(code))
            }
            Ok(
                WaitStatus::PtraceEvent(_, _, _)
                | WaitStatus::PtraceSyscall(_)
                | WaitStatus::StillAlive
                | WaitStatus::Continued(_)
                | WaitStatus::Stopped(_, _),
            ) => Ok(None),
            Err(nix::errno::Errno::ECHILD) => {
                // No child process
                self.pid = None;
                Ok(None)
            }
            Err(nix::errno::Errno::EINTR) => {
                // Interrupted by signal, try again later
                Ok(None)
            }
            Err(e) => Err(PtyError::Read(format!("waitpid failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_program_finds_absolute_path() {
        assert_eq!(
            resolve_program("/bin/bash"),
            Some(PathBuf::from("/bin/bash"))
        );
    }

    #[test]
    fn test_resolve_program_uses_path_for_bare_names() {
        // `sh` is a bare name, so it must be resolved through PATH.
        let resolved = resolve_program("sh");
        assert!(resolved.is_some(), "sh should be found in PATH");
    }

    #[test]
    fn test_resolve_program_rejects_missing_program() {
        assert_eq!(
            resolve_program("terminal-definitely-missing-shell-xyz"),
            None
        );
    }

    #[test]
    fn test_spawn_missing_program_errors() {
        // RF-67: a missing shell must be reported so the caller can fall back
        // to the shell of the system instead of leaving an empty pane.
        let result = UnixPty::spawn("terminal-definitely-missing-shell-xyz", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_pty_create_empty() {
        let pty = UnixPty::new_empty();
        assert!(pty.is_ok());
        let pty = pty.unwrap();
        // No child, so not alive
        assert!(!pty.is_alive());
        assert!(pty.pid().is_none());
    }

    #[test]
    fn test_pty_spawn() {
        let pty = UnixPty::spawn("/bin/bash", &[]);
        assert!(pty.is_ok());
        let pty = pty.unwrap();
        assert!(pty.is_alive());
        assert!(pty.pid().is_some());
    }

    #[test]
    fn test_pty_spawn_with_arguments() {
        // RF-62 / CL-25: arguments are passed as real argv entries, so a
        // multi-word argument must arrive as a single argument.
        let mut pty = UnixPty::spawn(
            "/bin/sh",
            &["-c".to_string(), "printf '<%s>\\n' \"a b\"".to_string()],
        )
        .expect("spawn failed");

        std::thread::sleep(std::time::Duration::from_millis(200));

        let mut buf = [0u8; 256];
        let mut output = String::new();
        for _ in 0..50 {
            match pty.read(&mut buf) {
                Ok(Some(0)) => {}
                Ok(Some(n)) => output.push_str(&String::from_utf8_lossy(&buf[..n])),
                Ok(None) => break,
                Err(e) => panic!("read error: {e}"),
            }
            if output.contains('>') {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        // `-c` was honoured (the command ran) and "a b" stayed one argument.
        assert!(output.contains("<a b>"), "unexpected output: {output:?}");
    }

    #[test]
    fn test_pty_read_write() {
        let mut pty = UnixPty::spawn("/bin/bash", &[]).expect("spawn failed");

        // Give the shell time to start
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Write a command
        let written = pty.write(b"echo hello\n").expect("write failed");
        assert!(written > 0);

        // Read the output
        let mut buf = [0u8; READ_BUF_SIZE];
        // Poll a few times for output to appear
        let mut total = 0;
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            match pty.read(&mut buf[total..]) {
                Ok(Some(n)) => {
                    if n > 0 {
                        total += n;
                        if total > 0 && String::from_utf8_lossy(&buf[..total]).contains("hello") {
                            break;
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => panic!("read error: {e}"),
            }
        }

        assert!(
            total > 0,
            "should have received output, got: {:?}",
            &buf[..total]
        );
        let output = String::from_utf8_lossy(&buf[..total]);
        assert!(output.contains("hello"));
    }

    #[test]
    fn test_pty_send_interrupt() {
        let pty = UnixPty::spawn("/bin/bash", &[]).expect("spawn failed");
        assert!(pty.is_alive());

        // Send SIGINT
        pty.send_interrupt();

        // Process should still be alive (bash handles SIGINT)
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(pty.is_alive());
    }

    #[test]
    fn test_pty_wait_exited() {
        let mut pty = UnixPty::spawn("/bin/true", &[]).expect("spawn failed");
        assert!(pty.is_alive());

        // /bin/true exits immediately, wait for it
        std::thread::sleep(std::time::Duration::from_millis(500));

        let status = pty.wait().expect("wait failed");
        assert!(status.is_some());
        assert_eq!(status.unwrap(), 0);
        assert!(!pty.is_alive());
    }

    #[test]
    fn test_pty_pid_consistency() {
        let pty = UnixPty::spawn("/bin/bash", &[]).expect("spawn failed");
        let pid = pty.pid();
        assert!(pid.is_some());
        let pid = pid.unwrap();

        // PID should be a valid process ID
        assert!(pid > 0);

        // Verify process exists
        let result = signal::kill(Pid::from_raw(pid.cast_signed()), signal::Signal::SIGINT);
        // ESRCH means process doesn't exist, EPERM means it exists but we can't send signal
        assert!(
            result.is_ok() || result == Err(nix::errno::Errno::ESRCH),
            "process with pid {pid} should exist, got: {result:?}"
        );
    }

    #[test]
    fn test_pty_read_empty_buf() {
        let mut pty = UnixPty::new_empty().expect("new_empty failed");

        // Slave is closed, so read should return 0 (no data)
        let mut buf = [0u8; 100];
        let result = pty.read(&mut buf);
        assert!(result.is_ok());
        // With slave closed, we get EIO which is treated as EOF
        let n = result.unwrap();
        assert!(n.is_none() || n.unwrap() == 0);
    }

    #[test]
    fn test_pty_resize_succeeds() {
        let mut pty = UnixPty::new_empty().expect("new_empty failed");
        assert!(pty.resize(40, 120).is_ok());
    }

    #[test]
    fn test_pty_reads_eof_after_child_exits() {
        // Regression: after the child's output is drained, reads must report EOF
        // so the session can surface the exit.
        let mut pty = UnixPty::spawn("/bin/echo", &[]).expect("spawn failed");
        std::thread::sleep(std::time::Duration::from_millis(300));

        let mut buf = [0u8; 256];
        let mut eof = false;
        for _ in 0..50 {
            match pty.read(&mut buf) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    eof = true;
                    break;
                }
                Err(e) => panic!("read error: {e}"),
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(eof, "read should report EOF after the child exits");
    }

    #[test]
    fn test_pty_child_exits_on_eof() {
        // Use `cat`, which reliably exits on EOF. (An interactive `bash` may
        // ignore Ctrl+D depending on the user's `IGNOREEOF` setting, which
        // would make this test hang on `wait()`.)
        let mut pty = UnixPty::spawn("/bin/cat", &[]).expect("spawn failed");

        // Give the child time to start
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Send EOF (Ctrl+D)
        pty.send_eof();

        // Wait for the child to exit
        std::thread::sleep(std::time::Duration::from_millis(200));

        let status = pty.wait();
        assert!(status.is_ok());
        assert!(!pty.is_alive());
    }
}
