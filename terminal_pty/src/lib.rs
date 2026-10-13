//! PTY abstraction for Unix and Windows.
//!
//! This crate provides a cross-platform abstraction for pseudo-terminal
//! management:
//! - Unix: fork + `posix_openpt`
//! - Windows: `ConPTY` API
//!
//! # Architecture
//!
//! ```text
//! TerminalSession → PTY trait → platform-specific impl
//! ```

#![warn(missing_docs)]

/// PTY trait definition.
///
/// This trait abstracts over pseudo-terminal management on different
/// platforms. Implementations exist for Unix (fork + pts) and Windows (`ConPTY`).
pub mod pty;

/// Re-export the PTY trait and error type for convenience.
pub use pty::{Pty, PtyError};

/// Unix implementation (Linux/macOS).
#[cfg(unix)]
pub mod unix;

/// Windows implementation (ConPTY).
#[cfg(windows)]
pub mod win32;
