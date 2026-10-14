//! Clipboard integration.
//!
//! This module handles:
//! - Copy selected text to clipboard (Ctrl+Shift+C)
//! - Paste clipboard content to terminal (Ctrl+Shift+V)
//! - System clipboard abstraction for cross-platform support
//!
//! # Architecture
//!
//! ```text
//! GuiAction::Copy  → Clipboard::set_text(selected_text)
//! GuiAction::Paste → Clipboard::get_text() → terminal input
//! ```
//!
//! The OS clipboard is reached through `arboard`. When no system clipboard can
//! be opened — a headless test runner, for instance — the clipboard degrades to
//! an in-memory buffer so copy/paste still round-trips inside the application
//! instead of failing outright.

/// Errors reported by the system clipboard.
#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    /// The operating system clipboard reported a failure.
    #[error("system clipboard error: {0}")]
    System(#[from] arboard::Error),
}

/// Clipboard content.
///
/// Wraps the OS clipboard and keeps a copy of the text in memory, so paste
/// keeps working even when the OS clipboard is unavailable.
pub struct Clipboard {
    /// Handle to the OS clipboard, if one could be opened.
    system: Option<arboard::Clipboard>,
    /// Text kept while no OS clipboard is available (headless fallback).
    fallback: String,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard {
    /// Open the OS clipboard.
    ///
    /// Falls back to an in-memory buffer when the system clipboard is
    /// unavailable, for example on a headless machine.
    pub fn new() -> Self {
        let mut clipboard = Self::in_memory();
        clipboard.system = match arboard::Clipboard::new() {
            Ok(system) => Some(system),
            Err(err) => {
                tracing::warn!("System clipboard unavailable, falling back to memory: {err}");
                None
            }
        };
        clipboard
    }

    /// Create an in-memory clipboard with no OS access.
    ///
    /// Copy and paste still round-trip within the process. Used by tests and by
    /// headless environments.
    pub fn in_memory() -> Self {
        Self {
            system: None,
            fallback: String::new(),
        }
    }

    /// Whether an OS clipboard is backing this instance.
    pub fn has_system_clipboard(&self) -> bool {
        self.system.is_some()
    }

    /// Copy `text` to the OS clipboard.
    ///
    /// The text is always kept in memory too, so pasting still works inside the
    /// application when the OS clipboard rejects the write.
    ///
    /// # Errors
    ///
    /// Returns [`ClipboardError`] when an OS clipboard exists but refuses the
    /// write.
    pub fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        text.clone_into(&mut self.fallback);
        if let Some(system) = &mut self.system {
            system.set_text(text).map_err(ClipboardError::from)?;
        }
        Ok(())
    }

    /// Read the OS clipboard, or the in-memory fallback when there is none.
    pub fn get_text(&mut self) -> String {
        if let Some(system) = &mut self.system {
            match system.get_text() {
                Ok(text) => return text,
                Err(err) => tracing::debug!("Could not read the system clipboard: {err}"),
            }
        }
        self.fallback.clone()
    }

    /// Check if the in-memory fallback is empty.
    pub fn is_empty(&self) -> bool {
        self.fallback.is_empty()
    }

    /// Forget the text kept in the in-memory fallback.
    pub fn clear(&mut self) {
        self.fallback.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_new_is_empty() {
        let clipboard = Clipboard::in_memory();
        assert!(clipboard.is_empty());
        assert!(!clipboard.has_system_clipboard());
    }

    #[test]
    fn test_clipboard_set_text() {
        let mut clipboard = Clipboard::in_memory();
        clipboard.set_text("Hello, world!").unwrap();
        assert!(!clipboard.is_empty());
        assert_eq!(clipboard.get_text(), "Hello, world!");
    }

    #[test]
    fn test_clipboard_get_on_empty_returns_empty() {
        let mut clipboard = Clipboard::in_memory();
        assert_eq!(clipboard.get_text(), "");
    }

    #[test]
    fn test_clipboard_overwrites_previous_text() {
        let mut clipboard = Clipboard::in_memory();
        clipboard.set_text("first").unwrap();
        clipboard.set_text("second").unwrap();
        assert_eq!(clipboard.get_text(), "second");
    }

    #[test]
    fn test_clipboard_clear() {
        let mut clipboard = Clipboard::in_memory();
        clipboard.set_text("Hello").unwrap();
        clipboard.clear();
        assert!(clipboard.is_empty());
        assert_eq!(clipboard.get_text(), "");
    }

    #[test]
    fn test_clipboard_default() {
        let clipboard = Clipboard::default();
        assert!(clipboard.is_empty());
    }

    #[test]
    fn test_clipboard_unicode() {
        let mut clipboard = Clipboard::in_memory();
        clipboard.set_text("Héllo 世界").unwrap();
        assert_eq!(clipboard.get_text(), "Héllo 世界");
    }
}
