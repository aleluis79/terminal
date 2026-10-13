//! Tab bar rendering and interaction.
//!
//! This module handles:
//! - Rendering tabs in the top bar
//! - Click handling to switch tabs
//! - Hover effects (visual feedback)
//!
//! # Architecture
//!
//! ```text
//! TabBar (UI) ←→ TabManager (state)
//! ```

use winit::window::Window;

/// A single tab button in the tab bar.
#[derive(Debug, Clone)]
pub struct TabButton {
    /// Tab title to display.
    pub title: String,
    /// Tab index in the manager.
    pub index: usize,
    /// Whether this tab is currently active.
    pub is_active: bool,
    /// Whether the mouse is hovering over this tab.
    pub is_hovered: bool,
}

impl TabButton {
    /// Create a new tab button.
    pub fn new(title: String, index: usize, is_active: bool) -> Self {
        Self {
            title,
            index,
            is_active,
            is_hovered: false,
        }
    }
}

/// Tab bar UI component.
///
/// Renders a horizontal bar of tab buttons at the top of the window.
/// Handles mouse clicks to switch tabs.
#[derive(Debug)]
pub struct TabBar {
    /// Height of the tab bar in pixels.
    height: u32,
    /// Padding around tab content.
    #[expect(dead_code)]
    padding: u32,
    /// Spacing between tabs.
    #[expect(dead_code)]
    spacing: u32,
    /// Font size for tab titles.
    font_size: u32,
    /// Estimated button width (used for hit testing).
    #[expect(dead_code)]
    estimated_button_width: u32,
}

impl Default for TabBar {
    fn default() -> Self {
        Self::new()
    }
}

impl TabBar {
    /// Default tab bar height.
    const DEFAULT_HEIGHT: u32 = 32;
    /// Default padding.
    const DEFAULT_PADDING: u32 = 8;
    /// Default spacing between tabs.
    const DEFAULT_SPACING: u32 = 4;
    /// Default font size.
    const DEFAULT_FONT_SIZE: u32 = 13;
    /// Estimated minimum button width.
    const ESTIMATED_BUTTON_WIDTH: u32 = 100;

    /// Create a new tab bar with default dimensions.
    pub fn new() -> Self {
        Self {
            height: Self::DEFAULT_HEIGHT,
            padding: Self::DEFAULT_PADDING,
            spacing: Self::DEFAULT_SPACING,
            font_size: Self::DEFAULT_FONT_SIZE,
            estimated_button_width: Self::ESTIMATED_BUTTON_WIDTH,
        }
    }

    /// Get the tab bar height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Convert a window position to a tab index.
    ///
    /// Used for hit testing when the user clicks on the tab bar.
    ///
    /// # Arguments
    ///
    /// * `x` - X coordinate in window pixels.
    /// * `tab_count` - Number of tabs.
    /// * `_active_idx` - Index of the active tab (reserved for future use).
    ///
    /// # Returns
    ///
    /// `Some(index)` if the click is on a tab, `None` otherwise.
    #[expect(clippy::cast_sign_loss)]
    pub fn hit_test(x: u32, tab_count: usize, _active_idx: usize) -> Option<usize> {
        if tab_count == 0 {
            return None;
        }

        // Calculate total width needed for all tabs
        let total_width = tab_count as u32 * Self::ESTIMATED_BUTTON_WIDTH
            + (tab_count.saturating_sub(1) as u32 * Self::DEFAULT_SPACING);

        // Check if click is within tab bar bounds
        if x >= total_width {
            return None;
        }

        // Determine which tab was clicked
        let x_per_tab = f64::from(total_width) / f64::from(tab_count as u32);
        let clicked_idx = (f64::from(x) / x_per_tab) as usize;

        if clicked_idx < tab_count {
            Some(clicked_idx)
        } else {
            None
        }
    }

    /// Build tab buttons from a tab manager.
    ///
    /// This is called each frame to sync the UI state with the
    /// tab manager state.
    ///
    /// # Arguments
    ///
    /// * `tab_count` - Number of tabs.
    /// * `active_idx` - Index of the active tab.
    pub fn build_buttons(&self, tab_count: usize, active_idx: usize) -> Vec<TabButton> {
        (0..tab_count)
            .map(|i| TabButton::new(format!("Tab {i}"), i, i == active_idx))
            .collect()
    }

    /// Render the tab bar to a buffer.
    ///
    /// This is a stub implementation. In production, this would
    /// render the tab bar using the GPU renderer.
    ///
    /// # Arguments
    ///
    /// * `window` - The winit window (for title).
    /// * `buttons` - The tab buttons to render.
    pub fn render(&self, _window: &Window, _buttons: &[TabButton]) {
        // Stub: In production, render tabs to the GPU surface
    }

    /// Set the tab bar height.
    pub fn set_height(&mut self, height: u32) {
        self.height = height.max(16);
    }

    /// Set the font size.
    pub fn set_font_size(&mut self, font_size: u32) {
        self.font_size = font_size.max(8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_bar_default() {
        let bar = TabBar::default();
        assert_eq!(bar.height(), TabBar::DEFAULT_HEIGHT);
    }

    #[test]
    fn test_tab_bar_hit_test_no_tabs() {
        assert!(TabBar::hit_test(0, 0, 0).is_none());
    }

    #[test]
    fn test_tab_bar_hit_test_single_tab() {
        assert_eq!(TabBar::hit_test(50, 1, 0), Some(0));
        assert!(TabBar::hit_test(500, 1, 0).is_none());
    }

    #[test]
    fn test_tab_bar_hit_test_multiple_tabs() {
        // 3 tabs, each ~104px wide (100 + 4 spacing)
        assert_eq!(TabBar::hit_test(50, 3, 1), Some(0));
        assert_eq!(TabBar::hit_test(150, 3, 1), Some(1));
        assert_eq!(TabBar::hit_test(250, 3, 1), Some(2));
        assert!(TabBar::hit_test(500, 3, 1).is_none());
    }

    #[test]
    fn test_tab_button_creation() {
        let btn = TabButton::new("Test".to_string(), 0, true);
        assert_eq!(btn.title, "Test");
        assert_eq!(btn.index, 0);
        assert!(btn.is_active);
        assert!(!btn.is_hovered);
    }

    #[test]
    fn test_tab_bar_build_buttons() {
        let bar = TabBar::new();
        let buttons = bar.build_buttons(3, 1);
        assert_eq!(buttons.len(), 3);
        assert!(!buttons[0].is_active);
        assert!(buttons[1].is_active);
        assert!(!buttons[2].is_active);
    }

    #[test]
    fn test_tab_bar_set_height() {
        let mut bar = TabBar::new();
        bar.set_height(48);
        assert_eq!(bar.height(), 48);
        bar.set_height(8); // Should clamp to 16
        assert_eq!(bar.height(), 16);
    }

    #[test]
    fn test_tab_bar_set_font_size() {
        let mut bar = TabBar::new();
        bar.set_font_size(16);
        assert_eq!(bar.font_size, 16);
        bar.set_font_size(4); // Should clamp to 8
        assert_eq!(bar.font_size, 8);
    }
}
