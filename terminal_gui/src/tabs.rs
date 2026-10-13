//! Tab management: tabs, panels, and session lifecycle.
//!
//! This module handles:
//! - Tab creation, closure, and switching
//! - Panel management within tabs
//! - Session lifecycle (spawn shell, read/write)
//! - Tab title management

use std::sync::atomic::{AtomicU64, Ordering};

/// Unique tab identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(u64);

impl TabId {
    /// Generate a new unique tab ID.
    fn new_id() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        TabId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Terminal session running inside a tab.
///
/// Manages the PTY, parser, and buffer for a single shell session.
#[derive(Debug)]
pub struct TerminalSession {
    /// Unique session identifier.
    id: TabId,
    /// Shell title (from OSC 0/1/2 or default).
    title: String,
    /// Whether the session is still running.
    is_running: bool,
}

impl TerminalSession {
    /// Create a new terminal session.
    pub fn new(title: String) -> Self {
        Self {
            id: TabId::new_id(),
            title,
            is_running: true,
        }
    }

    /// Get the session ID.
    pub fn id(&self) -> TabId {
        self.id
    }

    /// Get the session title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Update the session title.
    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    /// Check if the session is still running.
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Mark the session as exited.
    pub fn exit(&mut self) {
        self.is_running = false;
    }
}

/// A single terminal tab.
///
/// Contains one or more panels (split views), each with its own
/// terminal session.
#[derive(Debug)]
pub struct Tab {
    /// Unique tab identifier.
    id: TabId,
    /// Display title for the tab bar.
    title: String,
    /// Active session in this tab.
    session: TerminalSession,
    /// Whether this tab is visible.
    is_visible: bool,
}

impl Tab {
    /// Create a new tab with the given title.
    pub fn new(title: String) -> Self {
        Self {
            id: TabId::new_id(),
            title,
            session: TerminalSession::new("Shell".to_string()),
            is_visible: true,
        }
    }

    /// Get the tab ID.
    pub fn id(&self) -> TabId {
        self.id
    }

    /// Get the tab title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Update the tab title.
    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    /// Get the session.
    pub fn session(&self) -> &TerminalSession {
        &self.session
    }

    /// Get the session (mutable).
    pub fn session_mut(&mut self) -> &mut TerminalSession {
        &mut self.session
    }

    /// Check if the tab is visible.
    pub fn is_visible(&self) -> bool {
        self.is_visible
    }

    /// Set the tab visibility.
    pub fn set_visible(&mut self, visible: bool) {
        self.is_visible = visible;
    }

    /// Check if the tab's session is running.
    pub fn is_running(&self) -> bool {
        self.session.is_running()
    }
}

/// Manager for all tabs in the application.
///
/// Handles tab creation, closure, switching, and enforces
/// the constraint that closing the last tab closes the app (CL-7).
#[derive(Debug)]
pub struct TabManager {
    /// All active tabs.
    tabs: Vec<Tab>,
    /// Index of the currently focused tab.
    active_idx: usize,
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TabManager {
    /// Create a new tab manager with one default tab.
    pub fn new() -> Self {
        Self {
            tabs: vec![Tab::new("Terminal".to_string())],
            active_idx: 0,
        }
    }

    /// Get the number of tabs.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Check if there are no tabs.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Get the currently active tab (immutable).
    pub fn active(&self) -> Option<&Tab> {
        self.tabs.get(self.active_idx)
    }

    /// Get the currently active tab (mutable).
    pub fn active_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active_idx)
    }

    /// Get the index of the active tab.
    pub fn active_idx(&self) -> Option<usize> {
        if self.active_idx < self.tabs.len() {
            Some(self.active_idx)
        } else {
            None
        }
    }

    /// Create a new tab and make it active.
    ///
    /// The new tab is appended to the end of the tab list.
    /// Returns the ID of the newly created tab.
    pub fn create_tab(&mut self) -> TabId {
        let title = format!("Terminal {}", self.tabs.len() + 1);
        let tab = Tab::new(title);
        let id = tab.id();
        self.tabs.push(tab);
        self.active_idx = self.tabs.len() - 1;
        id
    }

    /// Close the active tab.
    ///
    /// # Returns
    ///
    /// `Some(new_active_idx)` if tabs remain, `None` if the app should close (CL-7).
    pub fn close_active_tab(&mut self) -> Option<usize> {
        if self.tabs.len() <= 1 {
            // CL-7: Last tab closed → app should close
            return None;
        }

        let idx = self.active_idx;
        self.tabs.remove(idx);

        // Adjust active index
        if idx >= self.tabs.len() {
            self.active_idx = self.tabs.len() - 1;
        } else {
            self.active_idx = idx;
        }

        Some(self.active_idx)
    }

    /// Switch to the tab at the given index.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    pub fn switch_to(&mut self, idx: usize) {
        assert!(
            idx < self.tabs.len(),
            "Tab index {} out of bounds ({} tabs)",
            idx,
            self.tabs.len()
        );
        self.active_idx = idx;
    }

    /// Close the tab with the given ID.
    ///
    /// Returns the new active index, or `None` if no tabs remain.
    pub fn close_tab(&mut self, id: TabId) -> Option<usize> {
        let Some(idx) = self.tabs.iter().position(|tab| tab.id() == id) else {
            return self.active_idx();
        };

        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            self.active_idx = 0;
            return None;
        }

        if self.active_idx >= self.tabs.len() {
            self.active_idx = self.tabs.len() - 1;
        } else if idx < self.active_idx {
            self.active_idx -= 1;
        }
        Some(self.active_idx)
    }

    /// Switch to the next tab (wrap around).
    pub fn switch_next(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.active_idx = (self.active_idx + 1) % self.tabs.len();
    }

    /// Switch to the previous tab (wrap around).
    pub fn switch_prev(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        if self.active_idx == 0 {
            self.active_idx = self.tabs.len() - 1;
        } else {
            self.active_idx -= 1;
        }
    }

    /// Get a reference to all tabs.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Get a mutable reference to all tabs.
    pub fn tabs_mut(&mut self) -> &mut [Tab] {
        &mut self.tabs
    }

    /// Get a tab by index.
    pub fn get_tab(&self, idx: usize) -> Option<&Tab> {
        self.tabs.get(idx)
    }

    /// Get a mutable reference to a tab by index.
    pub fn get_tab_mut(&mut self, idx: usize) -> Option<&mut Tab> {
        self.tabs.get_mut(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_creation() {
        let tab = Tab::new("My Tab".to_string());
        assert_eq!(tab.title(), "My Tab");
        assert!(tab.is_visible());
        assert!(tab.is_running());
    }

    #[test]
    fn test_tab_title_update() {
        let mut tab = Tab::new("Initial".to_string());
        tab.set_title("Updated".to_string());
        assert_eq!(tab.title(), "Updated");
    }

    #[test]
    fn test_tab_manager_default() {
        let manager = TabManager::default();
        assert_eq!(manager.len(), 1);
        assert!(!manager.is_empty());
        assert!(manager.active().is_some());
        assert_eq!(manager.active().unwrap().title(), "Terminal");
    }

    #[test]
    fn test_tab_manager_create_tab() {
        let mut manager = TabManager::new();
        let id = manager.create_tab();
        assert_eq!(manager.len(), 2);
        assert!(id.0 > 0);
        assert_eq!(manager.active_idx(), Some(1));
        assert_eq!(manager.active().unwrap().title(), "Terminal 2");
    }

    #[test]
    fn test_tab_manager_close_last_tab() {
        let mut manager = TabManager::new();
        // Should not be able to close the last tab
        let result = manager.close_active_tab();
        assert!(result.is_none());
        assert_eq!(manager.len(), 1);
    }

    #[test]
    fn test_tab_manager_close_active_with_multiple() {
        let mut manager = TabManager::new();
        manager.create_tab(); // Now have 2 tabs, active = 1
        manager.create_tab(); // Now have 3 tabs, active = 2
        assert_eq!(manager.len(), 3);
        assert_eq!(manager.active_idx(), Some(2));

        // Close the active tab (index 2)
        let result = manager.close_active_tab();
        assert_eq!(result, Some(1)); // After removing index 2, active_idx clamped to 1
        assert_eq!(manager.len(), 2);

        // Close again (index 1)
        let result = manager.close_active_tab();
        assert_eq!(result, Some(0)); // After removing index 1, active_idx = 0
        assert_eq!(manager.len(), 1);

        // Can't close the last one
        assert!(manager.close_active_tab().is_none());
    }

    #[test]
    fn test_tab_manager_close_middle_tab() {
        let mut manager = TabManager::new();
        manager.create_tab(); // Tab 0, Tab 1 (active)
        manager.switch_to(0); // Tab 0 (active)

        assert_eq!(manager.active_idx(), Some(0));
        let result = manager.close_active_tab();
        assert_eq!(result, Some(0));
        assert_eq!(manager.len(), 1);
    }

    #[test]
    fn test_tab_manager_switch_next() {
        let mut manager = TabManager::new();
        manager.create_tab(); // 2 tabs, active = 1 (create_tab sets active to last)
                              // Already at index 1, switch_next wraps to 0
        manager.switch_next();
        assert_eq!(manager.active_idx(), Some(0));
        manager.switch_next();
        assert_eq!(manager.active_idx(), Some(1)); // Wrap around
    }

    #[test]
    fn test_tab_manager_switch_prev() {
        let mut manager = TabManager::new();
        manager.create_tab(); // 2 tabs, active = 1
                              // Already at index 1, switch_prev goes to 0
        manager.switch_prev();
        assert_eq!(manager.active_idx(), Some(0));
        manager.switch_prev();
        assert_eq!(manager.active_idx(), Some(1)); // Wrap around
    }

    #[test]
    fn test_tab_manager_switch_to() {
        let mut manager = TabManager::new();
        manager.create_tab();
        manager.create_tab();
        manager.switch_to(0);
        assert_eq!(manager.active_idx(), Some(0));
        manager.switch_to(2);
        assert_eq!(manager.active_idx(), Some(2));
    }

    #[test]
    #[should_panic(expected = "Tab index 5 out of bounds")]
    fn test_tab_manager_switch_to_invalid() {
        let mut manager = TabManager::new();
        manager.switch_to(5); // Should panic
    }

    #[test]
    fn test_tab_session() {
        let tab = Tab::new("Test".to_string());
        let session = tab.session();
        assert!(session.is_running());
        assert_eq!(session.title(), "Shell");
    }

    #[test]
    fn test_tab_session_mut() {
        let mut tab = Tab::new("Test".to_string());
        tab.session_mut().set_title("My Shell".to_string());
        assert_eq!(tab.session().title(), "My Shell");
    }

    #[test]
    fn test_rapid_tab_creation() {
        // CL-6: Create many tabs rapidly
        let mut manager = TabManager::new();
        for i in 0..10 {
            manager.create_tab();
            assert_eq!(manager.len(), i + 2);
        }
        assert_eq!(manager.active_idx(), Some(10));
    }
}
