//! GUI layer: windows, tabs, splits, events, clipboard.
//!
//! This crate handles:
//! - Window creation and management (winit)
//! - Tab bar and tab management
//! - Panel splitting and navigation
//! - Keyboard and mouse event handling
//! - Clipboard integration

#![warn(missing_docs)]

/// Window creation and management.
pub mod window;

/// Tab bar rendering.
pub mod tab_bar;

/// Tab management.
pub mod tabs;

/// Panel splitting and layout.
pub mod split;

/// Event handling (keyboard, mouse).
pub mod event;

/// Clipboard integration.
pub mod clipboard;

/// Keyboard bindings / keymap.
pub mod keymap;

/// Platform-specific code.
pub mod platform;
