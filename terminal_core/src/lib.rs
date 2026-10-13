//! Core terminal engine: grid, parser, state, buffer.
//!
//! This crate has **no GUI dependencies**. It handles:
//! - 2D character grid with attributes
//! - ANSI/ECMA-48 escape sequence parsing
//! - Terminal state (cursor, modes, attributes)
//! - Scrollback buffer management
//!
//! # Architecture
//!
//! ```text
//! bytes → parser → state → grid → (output to renderer)
//! ```

#![warn(missing_docs)]

/// Individual cell in the terminal grid.
pub mod cell;

/// 2D grid data structure.
pub mod grid;

/// Terminal state: cursor, modes, attributes.
pub mod state;

/// Scrollback buffer management.
pub mod buffer;

/// ANSI/ECMA-48 escape sequence parser.
pub mod parser;

/// Terminal engine: ties parser, state, and grid together.
pub mod engine;

/// Hyperlink tracking (OSC-8).
pub mod hyperlink;

/// Terminal session: PTY + parser + buffer integration.
pub mod session;

/// Keyboard input handling.
pub mod input;
