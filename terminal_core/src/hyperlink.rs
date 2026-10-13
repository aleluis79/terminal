//! Hyperlink tracking (OSC-8).
//!
//! OSC-8 hyperlinks associate a URL with a range of cells in the grid.
//! Format: `]8;id;url^\\` to start, `]8;;^\\` to end.

/// A hyperlink entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hyperlink {
    /// Unique identifier for this hyperlink instance.
    pub id: String,
    /// The URL this hyperlink points to.
    pub url: String,
}

impl Hyperlink {
    /// Create a new hyperlink.
    pub fn new(id: String, url: String) -> Self {
        Self { id, url }
    }
}

/// Hyperlink manager: tracks active hyperlinks during parsing.
pub struct HyperlinkManager {
    /// Currently active hyperlink (if any).
    active: Option<Hyperlink>,
    /// Hyperlink ID counter for auto-generated IDs.
    id_counter: u64,
}

impl Default for HyperlinkManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HyperlinkManager {
    /// Create a new hyperlink manager.
    pub fn new() -> Self {
        Self {
            active: None,
            id_counter: 0,
        }
    }

    /// Start a new hyperlink.
    pub fn start(&mut self, url: String) {
        self.id_counter += 1;
        let id = format!("t{}", self.id_counter);
        self.active = Some(Hyperlink::new(id, url));
    }

    /// End the current hyperlink.
    pub fn end(&mut self) {
        self.active = None;
    }

    /// Get the currently active hyperlink.
    pub fn active(&self) -> Option<&Hyperlink> {
        self.active.as_ref()
    }

    /// Get a mutable reference to the currently active hyperlink.
    pub fn active_mut(&mut self) -> Option<&mut Hyperlink> {
        self.active.as_mut()
    }

    /// Reset the manager.
    pub fn reset(&mut self) {
        self.active = None;
        self.id_counter = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_and_end() {
        let mut manager = HyperlinkManager::new();
        assert!(manager.active().is_none());
        manager.start("https://example.com".to_string());
        assert!(manager.active().is_some());
        assert_eq!(manager.active().unwrap().url, "https://example.com");
        manager.end();
        assert!(manager.active().is_none());
    }

    #[test]
    fn test_id_generation() {
        let mut manager = HyperlinkManager::new();
        manager.start("https://a.com".to_string());
        assert_eq!(manager.active().unwrap().id, "t1");
        manager.end();
        manager.start("https://b.com".to_string());
        assert_eq!(manager.active().unwrap().id, "t2");
    }

    #[test]
    fn test_reset() {
        let mut manager = HyperlinkManager::new();
        manager.start("https://example.com".to_string());
        manager.reset();
        assert!(manager.active().is_none());
    }
}
