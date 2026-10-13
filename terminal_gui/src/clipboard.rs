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
//! GuiAction::Copy → Clipboard::copy(selected_text)
//! GuiAction::Paste → Clipboard::paste() → TerminalInput
//! ```

/// Clipboard content.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Clipboard {
    /// The current clipboard text.
    text: String,
}

impl Clipboard {
    /// Create a new empty clipboard.
    pub fn new() -> Self {
        Self {
            text: String::new(),
        }
    }

    /// Get the current clipboard text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Set the clipboard text.
    pub fn set_text(&mut self, text: String) {
        self.text = text;
    }

    /// Clear the clipboard.
    pub fn clear(&mut self) {
        self.text.clear();
    }

    /// Check if the clipboard is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Get the clipboard text as bytes, ready to send to the terminal.
    pub fn as_bytes(&self) -> Vec<u8> {
        self.text.as_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_new_is_empty() {
        let clipboard = Clipboard::new();
        assert!(clipboard.is_empty());
        assert_eq!(clipboard.text(), "");
    }

    #[test]
    fn test_clipboard_set_text() {
        let mut clipboard = Clipboard::new();
        clipboard.set_text("Hello, world!".to_string());
        assert!(!clipboard.is_empty());
        assert_eq!(clipboard.text(), "Hello, world!");
    }

    #[test]
    fn test_clipboard_clear() {
        let mut clipboard = Clipboard::new();
        clipboard.set_text("Hello".to_string());
        clipboard.clear();
        assert!(clipboard.is_empty());
        assert_eq!(clipboard.text(), "");
    }

    #[test]
    fn test_clipboard_as_bytes() {
        let mut clipboard = Clipboard::new();
        clipboard.set_text("Hello".to_string());
        assert_eq!(clipboard.as_bytes(), b"Hello");
    }

    #[test]
    fn test_clipboard_default() {
        let clipboard = Clipboard::default();
        assert!(clipboard.is_empty());
    }

    #[test]
    fn test_clipboard_unicode() {
        let mut clipboard = Clipboard::new();
        clipboard.set_text("Héllo 世界".to_string());
        assert_eq!(clipboard.text(), "Héllo 世界");
        assert_eq!(clipboard.as_bytes(), "Héllo 世界".as_bytes());
    }
}
