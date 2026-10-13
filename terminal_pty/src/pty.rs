//! PTY trait definition.

/// A pseudo-terminal abstraction.
///
/// Implementations exist for Unix (fork + pts) and Windows (`ConPTY`).
pub trait Pty: Send + 'static {
    /// Spawn a new shell process on this PTY.
    ///
    /// # Errors
    ///
    /// Returns an error if the PTY cannot be created or the process cannot be spawned.
    fn spawn(shell: &str) -> Result<Self, PtyError>
    where
        Self: Sized;

    /// Read data from the PTY.
    ///
    /// Returns `Ok(Some(n))` with `n` bytes read, `Ok(Some(0))` if no data is
    /// available, or `Ok(None)` when the child process has exited.
    ///
    /// When `Ok(None)` is returned, call [`exit_code`](Self::exit_code) to get
    /// the process exit status.
    ///
    /// # Errors
    ///
    /// Returns an error if a read error occurs that is not EAGAIN or EIO.
    fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, PtyError>;

    /// Write data to the PTY.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    fn write(&mut self, buf: &[u8]) -> Result<usize, PtyError>;

    /// Get the PID of the child process.
    fn pid(&self) -> Option<u32>;

    /// Check if the child process is still running.
    fn is_alive(&self) -> bool;

    /// Get the exit code of the child process, if it has exited.
    ///
    /// This returns the exit code after [`read`](Self::read) has returned `Ok(None)`.
    fn exit_code(&self) -> Option<i32>;

    /// Send SIGINT to the child process.
    fn send_interrupt(&self);

    /// Send EOF to the child process.
    fn send_eof(&mut self);

    /// Resize the pseudo-terminal window.
    ///
    /// This informs the child process of the new terminal size so programs can
    /// reflow their output. Platforms without window-size support keep the
    /// previous size.
    ///
    /// # Errors
    ///
    /// Returns an error if the new size cannot be applied.
    fn resize(&mut self, rows: u16, cols: u16) -> Result<(), PtyError> {
        let _ = (rows, cols);
        Ok(())
    }

    /// Wait for the child process to exit.
    ///
    /// # Errors
    ///
    /// Returns an error if `waitpid` fails with an unexpected error.
    fn wait(&mut self) -> Result<Option<i32>, PtyError>;
}

/// PTY errors.
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Spawn error.
    #[error("Failed to spawn process: {0}")]
    Spawn(String),

    /// Read error.
    #[error("Failed to read from PTY: {0}")]
    Read(String),

    /// Write error.
    #[error("Failed to write to PTY: {0}")]
    Write(String),

    /// Resize error.
    #[error("Failed to resize PTY: {0}")]
    Resize(String),
}
