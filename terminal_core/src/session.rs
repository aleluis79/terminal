//! Terminal session: PTY + parser + buffer integration.
//!
//! A session represents a single terminal instance connected to a PTY.
//! It reads from the PTY, feeds data through the parser, and updates
//! the buffer.
//!
//! # Architecture
//!
//! ```text
//! PTY read → engine.input() → grid update
//! GUI write → pty.write() → shell input
//! ```

#[cfg(unix)]
pub use terminal_pty::unix::UnixPty as PlatformPty;
#[cfg(windows)]
pub use terminal_pty::win32::WinPty as PlatformPty;
use terminal_pty::Pty;

use crate::engine::TerminalEngine;
use crate::parser::Action;
use crate::state::TerminalState;

/// Events emitted by a terminal session.
///
/// These events are used to notify the GUI about session-level changes
/// such as process exit, title changes, etc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// The child process has exited.
    ///
    /// Contains the exit status:
    /// - `0..=255`: normal exit code
    /// - Negative: killed by signal (e.g., `-2` for SIGINT)
    SessionExited {
        /// The exit code or negative signal number.
        exit_code: i32,
    },
}

/// Result of polling a PTY for data.
///
/// Returned by [`TerminalSession::poll_pty`](Self::poll_pty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollResult {
    /// No data available right now (EAGAIN/EWOULDBLOCK).
    WouldBlock,
    /// Data available, ready to be fed into the engine.
    Data(Vec<u8>),
    /// The child process has exited.
    Exited {
        /// The exit code or negative signal number.
        exit_code: i32,
    },
}

/// A terminal session: PTY process + rendering state.
///
/// On Unix, this wraps a [`UnixPty`] that manages a pseudo-terminal
/// connected to a shell process.
pub struct TerminalSession {
    /// The terminal engine (parser + buffer + state).
    engine: TerminalEngine,
    /// The PTY for I/O with the shell process.
    pty: Option<PlatformPty>,
    /// Whether the session is still running.
    running: bool,
    /// The exit code, if the session has exited.
    exit_code: Option<i32>,
    /// GUI actions produced by the parser and not yet answered (osc queries,
    /// cursor position requests, …) (RF-51).
    gui_actions: Vec<Action>,
}

impl TerminalSession {
    /// Create a new session with the given dimensions.
    ///
    /// The session is created without a PTY. Use [`spawn_shell`](Self::spawn_shell)
    /// to create a PTY and start a shell.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            engine: TerminalEngine::new(rows, cols),
            pty: None,
            running: true,
            exit_code: None,
            gui_actions: Vec::new(),
        }
    }

    /// Spawn a shell process on a new PTY.
    ///
    /// This creates a pseudo-terminal, forks a child process that execs
    /// the specified shell, and connects it to this session.
    ///
    /// # Errors
    ///
    /// Returns an error if the PTY cannot be created or the shell cannot be spawned.
    pub fn spawn_shell(&mut self, shell: &str) -> Result<(), terminal_pty::PtyError> {
        self.spawn_command(shell, &[])
    }

    /// Spawn a program with explicit arguments on a new PTY (RF-62).
    ///
    /// The arguments are passed as separate entries, so they are never
    /// re-interpreted by an intermediate shell (CL-25).
    ///
    /// # Errors
    ///
    /// Returns an error if the PTY cannot be created or the program cannot be spawned.
    pub fn spawn_command(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<(), terminal_pty::PtyError> {
        let pty = PlatformPty::spawn(program, args)?;
        self.pty = Some(pty);
        // Tell the shell the real terminal size before it draws anything.
        let (rows, cols) = self.engine.dimensions();
        self.resize(rows, cols);
        Ok(())
    }

    /// Resize the terminal screen.
    ///
    /// On Unix the child process is also notified of the new window size, so
    /// programs can reflow their output.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.engine.resize(rows, cols);

        let (rows, cols) = self.engine.dimensions();
        let rows = u16::try_from(rows).unwrap_or(u16::MAX);
        let cols = u16::try_from(cols).unwrap_or(u16::MAX);
        if let Some(pty) = self.pty.as_mut() {
            let _ = pty.resize(rows, cols);
        }
    }

    /// Feed bytes from the PTY into the session.
    ///
    /// This processes incoming data from the shell through the terminal
    /// parser, updating the grid and state accordingly. Any action that needs
    /// the GUI (for example answering a cursor-position or colour query) is
    /// queued and can be drained with [`take_gui_actions`](Self::take_gui_actions).
    pub fn feed(&mut self, bytes: &[u8]) {
        if self.running {
            let actions = self.engine.input(bytes);
            self.gui_actions.extend(actions);
        }
    }

    /// Take the queued GUI actions, leaving the queue empty (RF-51).
    pub fn take_gui_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.gui_actions)
    }

    /// Read data from the PTY and feed it into the session.
    ///
    /// This is the main loop body: reads from the PTY master fd,
    /// feeds the data into the engine, and checks if the child
    /// process has exited.
    ///
    /// Returns:
    /// - `Ok(PollResult::WouldBlock)` if no data is available right now (EAGAIN)
    /// - `Ok(PollResult::Data(buf))` with bytes to feed into the engine
    /// - `Ok(PollResult::Exited { exit_code })` when the child process has exited
    ///
    /// # Errors
    ///
    /// Returns an error if a read error occurs (other than EAGAIN/EWOULDBLOCK).
    pub fn poll_pty(&mut self) -> Result<PollResult, terminal_pty::PtyError> {
        let Some(pty) = self.pty.as_mut() else {
            return Ok(PollResult::WouldBlock);
        };

        let mut buf = [0u8; 4096];
        match pty.read(&mut buf)? {
            Some(0) => {
                // No data available right now
                Ok(PollResult::WouldBlock)
            }
            Some(n) => {
                // Feed data into the engine
                self.feed(&buf[..n]);
                Ok(PollResult::Data(buf[..n].to_vec()))
            }
            None => {
                // EOF: child has exited
                // The exit code was already stored by check_child_exit() in read()
                let code = pty.exit_code().unwrap_or(-1);
                self.running = false;
                self.exit_code = Some(code);
                Ok(PollResult::Exited { exit_code: code })
            }
        }
    }

    /// Write data to the PTY (sends input to the shell).
    ///
    /// This sends bytes to the shell's stdin through the PTY.
    ///
    /// # Errors
    ///
    /// Returns an error if the write to the PTY fails.
    pub fn write_to_shell(&mut self, buf: &[u8]) -> Result<usize, terminal_pty::PtyError> {
        match self.pty.as_mut() {
            Some(pty) => pty.write(buf),
            None => Ok(0),
        }
    }

    /// Send SIGINT (Ctrl+C) to the shell process.
    pub fn send_interrupt(&self) {
        if let Some(pty) = &self.pty {
            pty.send_interrupt();
        }
    }

    /// Send EOF (Ctrl+D) to the shell process.
    pub fn send_eof(&mut self) {
        if let Some(pty) = self.pty.as_mut() {
            pty.send_eof();
        }
    }

    /// Check if the child process has exited.
    ///
    /// This calls `waitpid` with `WNOHANG` to check without blocking.
    /// Updates the internal `exit_code` if the process has exited.
    ///
    /// # Errors
    ///
    /// Returns an error if `waitpid` fails with an unexpected error.
    pub fn check_exit(&mut self) -> Result<Option<i32>, terminal_pty::PtyError> {
        match self.pty.as_mut() {
            Some(pty) => {
                let status = pty.wait()?;
                if status.is_some() {
                    self.running = false;
                    self.exit_code = status;
                }
                Ok(status)
            }
            None => Ok(None),
        }
    }

    /// Wait for the child process to exit (blocking).
    ///
    /// Updates the internal `exit_code` when the process exits.
    ///
    /// # Errors
    ///
    /// Returns an error if `waitpid` fails with an unexpected error.
    pub fn wait(&mut self) -> Result<Option<i32>, terminal_pty::PtyError> {
        match self.pty.as_mut() {
            Some(pty) => {
                let status = pty.wait()?;
                if status.is_some() {
                    self.running = false;
                    self.exit_code = status;
                }
                Ok(status)
            }
            None => Ok(None),
        }
    }

    /// Get the PID of the shell process.
    pub fn pid(&self) -> Option<u32> {
        self.pty.as_ref().and_then(terminal_pty::Pty::pid)
    }

    /// Get a reference to the engine.
    pub fn engine(&self) -> &TerminalEngine {
        &self.engine
    }

    /// Get a mutable reference to the engine.
    pub fn engine_mut(&mut self) -> &mut TerminalEngine {
        &mut self.engine
    }

    /// Check if the session is running.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Mark the session as stopped (PTY exited).
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Get the exit code of the child process.
    ///
    /// Returns `Some(code)` if the session has exited, `None` otherwise.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Check if this session has emitted an exit event.
    ///
    /// Returns `Some(SessionEvent::SessionExited { exit_code })` if the
    /// child process has exited and the event has not yet been consumed.
    ///
    /// Note: this is a query method. The actual event is produced by
    /// [`poll_pty`](Self::poll_pty) when it returns `PollResult::Exited`.
    pub fn session_event(&self) -> Option<SessionEvent> {
        self.exit_code
            .map(|code| SessionEvent::SessionExited { exit_code: code })
    }

    /// Get the current state.
    pub fn state(&self) -> &TerminalState {
        self.engine.state()
    }

    /// Get the current buffer.
    pub fn buffer(&self) -> &crate::buffer::GridBuffer {
        self.engine.buffer()
    }

    /// Get screen dimensions.
    pub fn dimensions(&self) -> (usize, usize) {
        self.engine.dimensions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Interactive shell used by the PTY tests on this platform.
    #[cfg(unix)]
    const TEST_SHELL: &str = "/bin/bash";
    #[cfg(windows)]
    const TEST_SHELL: &str = "cmd.exe";

    /// Program and arguments that exit immediately with the given code (RF-62).
    #[cfg(unix)]
    fn exit_command(code: i32) -> (&'static str, Vec<String>) {
        let program = if code == 0 { "/bin/true" } else { "/bin/false" };
        (program, Vec::new())
    }

    /// Program and arguments that exit immediately with the given code (RF-62).
    #[cfg(windows)]
    fn exit_command(code: i32) -> (&'static str, Vec<String>) {
        (
            "cmd.exe",
            vec!["/c".to_string(), "exit".to_string(), code.to_string()],
        )
    }

    /// Program and arguments that print something and exit (RF-62).
    #[cfg(unix)]
    fn echo_command() -> (&'static str, Vec<String>) {
        ("/bin/echo", Vec::new())
    }

    /// Program and arguments that print something and exit (RF-62).
    #[cfg(windows)]
    fn echo_command() -> (&'static str, Vec<String>) {
        ("cmd.exe", vec!["/c".to_string(), "echo".to_string()])
    }

    /// Line terminator the shell treats as "Enter".
    #[cfg(unix)]
    const TEST_ENTER: &str = "\n";
    #[cfg(windows)]
    const TEST_ENTER: &str = "\r";

    /// Poll `session` until the child exit surfaces, draining pending output.
    ///
    /// `ConPTY` emits an initial terminal frame when a client connects, so on
    /// Windows a single `poll_pty` after the child exits may still report the
    /// buffered `Data` before the EOF that carries the exit code.
    fn poll_until_exit(session: &mut TerminalSession) -> PollResult {
        for _ in 0..100 {
            match session.poll_pty().expect("poll failed") {
                PollResult::Exited { exit_code } => return PollResult::Exited { exit_code },
                PollResult::Data(_) | PollResult::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
        panic!("child did not exit in time");
    }

    #[test]
    fn test_session_queues_gui_query_actions() {
        // RF-51: queries parsed out of the PTY stream must surface to the GUI
        // so it can answer them.
        let mut session = TerminalSession::new(24, 80);
        session.feed(b"abc\x1b[6n");
        let actions = session.take_gui_actions();
        assert!(actions.contains(&Action::QueryCursorPosition));
        // Draining leaves the queue empty.
        assert_eq!(session.take_gui_actions(), Vec::<Action>::new());
    }

    #[test]
    fn test_session_creation() {
        let session = TerminalSession::new(24, 80);
        assert!(session.is_running());
        assert_eq!(session.dimensions(), (24, 80));
    }

    #[test]
    fn test_session_resize_updates_dimensions() {
        let mut session = TerminalSession::new(24, 80);
        session.resize(10, 40);
        assert_eq!(session.dimensions(), (10, 40));
    }

    #[test]
    fn test_session_feed() {
        let mut session = TerminalSession::new(24, 80);
        session.feed(b"Hello");
        assert_eq!(session.buffer().active().grid.get(0, 0).c, 'H');
    }

    #[test]
    fn test_session_stop() {
        let mut session = TerminalSession::new(24, 80);
        session.stop();
        assert!(!session.is_running());
        // Feeding after stop should be a no-op
        session.feed(b"World");
        assert_eq!(session.buffer().active().grid.get(0, 0).c, ' '); // still default
    }

    #[test]
    fn test_session_state() {
        let session = TerminalSession::new(24, 80);
        let state = session.state();
        assert_eq!(state.cursor.row, 0);
        assert_eq!(state.cursor.col, 0);
    }

    #[test]
    fn test_session_spawn_shell() {
        let mut session = TerminalSession::new(24, 80);
        assert!(session.spawn_shell(TEST_SHELL).is_ok());
        assert!(session.pid().is_some());
        assert!(session.is_running());
    }

    #[test]
    fn test_session_spawn_command_with_arguments() {
        // RF-62: a program plus explicit arguments can be spawned.
        let mut session = TerminalSession::new(24, 80);
        let (program, args) = exit_command(0);
        assert!(session.spawn_command(program, &args).is_ok());
        assert!(session.is_running());

        std::thread::sleep(std::time::Duration::from_millis(500));
        assert_eq!(
            poll_until_exit(&mut session),
            PollResult::Exited { exit_code: 0 }
        );
    }

    #[test]
    fn test_session_write_to_shell() {
        let mut session = TerminalSession::new(24, 80);
        session.spawn_shell(TEST_SHELL).expect("spawn failed");

        // Give the shell time to start
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Write a command
        let command = format!("echo hello{TEST_ENTER}");
        let written = session
            .write_to_shell(command.as_bytes())
            .expect("write failed");
        assert!(written > 0);

        // Poll to read the output
        let mut total = 0;
        let mut buf = [0u8; 4096];
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if let Some(pty) = session.pty.as_mut() {
                match pty.read(&mut buf[total..]) {
                    Ok(Some(n)) => {
                        if n > 0 {
                            total += n;
                            if String::from_utf8_lossy(&buf[..total]).contains("hello") {
                                break;
                            }
                        }
                    }
                    Ok(None) | Err(_) => break,
                }
            }
        }

        assert!(total > 0, "should have received output");
        let output = String::from_utf8_lossy(&buf[..total]);
        assert!(output.contains("hello"));
    }

    #[test]
    fn test_session_interrupt() {
        let mut session = TerminalSession::new(24, 80);
        session.spawn_shell(TEST_SHELL).expect("spawn failed");

        // Send interrupt
        session.send_interrupt();

        // Process should still be alive (bash handles SIGINT)
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(session.is_running());
    }

    #[test]
    fn test_session_wait_exited() {
        let mut session = TerminalSession::new(24, 80);
        let (program, args) = exit_command(0);
        session.spawn_command(program, &args).expect("spawn failed");

        // The command exits immediately
        std::thread::sleep(std::time::Duration::from_millis(500));

        let status = session.wait().expect("wait failed");
        assert!(status.is_some());
        assert_eq!(status.unwrap(), 0);
        assert!(!session.is_running());
    }

    #[test]
    fn test_session_poll_pty() {
        let mut session = TerminalSession::new(24, 80);
        session.spawn_shell(TEST_SHELL).expect("spawn failed");

        // Give the shell time to start
        std::thread::sleep(std::time::Duration::from_millis(300));

        // Poll should not error; bash may or may not have output
        let result = session.poll_pty().expect("poll failed");
        // poll_pty returns PollResult; we just verify it doesn't error
        // and the session state is consistent
        match result {
            PollResult::WouldBlock => {
                // No data available, session should still be running
                assert!(session.is_running());
            }
            PollResult::Data(_) => {
                // Data available, session should still be running
                assert!(session.is_running());
            }
            PollResult::Exited { .. } => {
                // Session exited
                assert!(!session.is_running());
            }
        }
    }

    #[test]
    fn test_session_write_without_pty() {
        let mut session = TerminalSession::new(24, 80);
        // No PTY spawned
        let written = session.write_to_shell(b"test").expect("write failed");
        assert_eq!(written, 0); // No PTY, returns 0
    }

    #[test]
    fn test_session_poll_exited_process() {
        // Test that poll_pty returns PollResult::Exited when the child process exits
        let mut session = TerminalSession::new(24, 80);
        let (program, args) = exit_command(0);
        session.spawn_command(program, &args).expect("spawn failed");

        // The command exits immediately, wait for it
        std::thread::sleep(std::time::Duration::from_millis(500));

        // poll_pty should detect the exit
        let result = poll_until_exit(&mut session);
        match result {
            PollResult::Exited { exit_code } => {
                assert_eq!(exit_code, 0);
                assert!(!session.is_running());
                assert_eq!(session.exit_code(), Some(0));
                assert!(matches!(
                    session.session_event(),
                    Some(SessionEvent::SessionExited { exit_code: 0 })
                ));
            }
            other => {
                panic!("Expected PollResult::Exited, got {other:?}");
            }
        }
    }

    #[test]
    fn test_session_poll_exited_with_error_code() {
        // Test that poll_pty returns the correct exit code when child exits with error
        let mut session = TerminalSession::new(24, 80);
        let (program, args) = exit_command(1);
        session.spawn_command(program, &args).expect("spawn failed");

        // The command exits immediately with code 1
        std::thread::sleep(std::time::Duration::from_millis(500));

        let result = poll_until_exit(&mut session);
        match result {
            PollResult::Exited { exit_code } => {
                assert_eq!(exit_code, 1);
                assert!(!session.is_running());
                assert_eq!(session.exit_code(), Some(1));
            }
            other => {
                panic!("Expected PollResult::Exited, got {other:?}");
            }
        }
    }

    #[test]
    fn test_session_exit_surfaces_after_output_drained() {
        // Regression: once the child's last output has been read, the session
        // must still report the exit instead of getting stuck on WouldBlock.
        let mut session = TerminalSession::new(24, 80);
        let (program, args) = echo_command();
        session.spawn_command(program, &args).expect("spawn failed");
        std::thread::sleep(std::time::Duration::from_millis(500));

        let mut exit_code = None;
        for _ in 0..50 {
            match session.poll_pty().expect("poll failed") {
                PollResult::Exited { exit_code: code } => {
                    exit_code = Some(code);
                    break;
                }
                PollResult::Data(_) | PollResult::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }

        assert_eq!(exit_code, Some(0));
        assert!(!session.is_running());
    }

    #[test]
    fn test_session_exit_code_before_exit() {
        // Test that exit_code() returns None before the process exits
        let mut session = TerminalSession::new(24, 80);
        session.spawn_shell(TEST_SHELL).expect("spawn failed");

        assert!(session.exit_code().is_none());
        assert!(session.session_event().is_none());
    }

    #[test]
    fn test_session_check_exit_updates_code() {
        // Test that check_exit() updates the internal exit_code
        let mut session = TerminalSession::new(24, 80);
        let (program, args) = exit_command(0);
        session.spawn_command(program, &args).expect("spawn failed");

        std::thread::sleep(std::time::Duration::from_millis(500));

        let status = session.check_exit().expect("check_exit failed");
        assert!(status.is_some());
        assert_eq!(status.unwrap(), 0);
        assert_eq!(session.exit_code(), Some(0));
        assert!(!session.is_running());
    }

    #[test]
    fn test_session_poll_no_pty() {
        // Test that poll_pty returns WouldBlock when no PTY is spawned
        let mut session = TerminalSession::new(24, 80);
        let result = session.poll_pty().expect("poll failed");
        assert!(matches!(result, PollResult::WouldBlock));
        assert!(session.is_running());
        assert!(session.exit_code().is_none());
    }
}
