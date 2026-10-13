//! GPU rendering pipeline for the terminal.
//!
//! This crate handles:
//! - Font loading and fallback (fontdb)
//! - Glyph shaping and rendering (swash)
//! - GPU texture atlas
//! - Main render pipeline (wgpu)
//! - Image rendering (Kitty graphics protocol)

#![warn(missing_docs)]

/// Font loading and management.
pub mod font;

/// Glyph shaping and rendering.
pub mod glyph;

/// GPU texture atlas.
pub mod atlas;

/// Main renderer.
pub mod renderer;

/// Color handling.
pub mod color;

/// Image rendering (Kitty graphics protocol).
pub mod image;
