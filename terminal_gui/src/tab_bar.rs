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

/// A laid-out tab button, in pixels from the left edge of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabRect {
    /// Index of the tab in the manager.
    pub index: usize,
    /// Left edge in pixels.
    pub x: u32,
    /// Width in pixels.
    pub width: u32,
}

impl TabRect {
    /// Whether the pixel `x` falls inside this tab.
    pub fn contains(&self, x: u32) -> bool {
        x >= self.x && x < self.x + self.width
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
    /// Horizontal padding inside each tab button.
    padding: u32,
    /// Spacing between tab buttons.
    spacing: u32,
    /// Font size for tab titles.
    font_size: u32,
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

    /// Create a new tab bar with default dimensions.
    pub fn new() -> Self {
        Self {
            height: Self::DEFAULT_HEIGHT,
            padding: Self::DEFAULT_PADDING,
            spacing: Self::DEFAULT_SPACING,
            font_size: Self::DEFAULT_FONT_SIZE,
        }
    }

    /// Get the tab bar height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Compute the pixel layout of the tab buttons.
    ///
    /// Each tab is as wide as its title (`cell_width` per character) plus
    /// padding. Tabs that do not fit within `max_width` are dropped. Rendering
    /// and hit-testing must both use this layout so they stay in sync.
    pub fn layout(&self, titles: &[&str], cell_width: u32, max_width: u32) -> Vec<TabRect> {
        let mut rects = Vec::new();
        let mut x = 0u32;

        for (index, title) in titles.iter().enumerate() {
            let chars = u32::try_from(title.chars().count()).unwrap_or(u32::MAX);
            let width = self.padding * 2 + chars * cell_width.max(1);
            if x + width > max_width {
                break;
            }
            rects.push(TabRect { index, x, width });
            x += width + self.spacing;
        }

        rects
    }

    /// Find the tab at pixel `x` within a layout from [`layout`](Self::layout).
    pub fn hit_test(&self, x: u32, rects: &[TabRect]) -> Option<usize> {
        rects.iter().find(|r| r.contains(x)).map(|r| r.index)
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
    fn test_tab_bar_layout_and_hit_test() {
        let bar = TabBar::new();
        let titles = ["Terminal", "Terminal 2", "Terminal 3"];

        // "Terminal" = 8 chars: padding*2 (16) + 8 * 10px = 96.
        let rects = bar.layout(&titles, 10, 1000);
        assert_eq!(rects.len(), 3);
        assert_eq!(rects[0].x, 0);
        assert_eq!(rects[0].width, 96);
        // 96 + spacing (4) = 100.
        assert_eq!(rects[1].x, 100);
        assert_eq!(rects[1].width, 16 + 10 * 10);

        assert_eq!(bar.hit_test(50, &rects), Some(0));
        assert_eq!(bar.hit_test(120, &rects), Some(1));
        assert_eq!(bar.hit_test(300, &rects), Some(2));
        assert!(bar.hit_test(9999, &rects).is_none());
    }

    #[test]
    fn test_tab_bar_layout_drops_overflowing_tabs() {
        let bar = TabBar::new();
        let titles = ["Terminal", "Terminal 2"];
        // Only room for the first tab (96px wide).
        let rects = bar.layout(&titles, 10, 100);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].index, 0);
    }

    #[test]
    fn test_tab_rect_contains() {
        let rect = TabRect {
            index: 0,
            x: 10,
            width: 20,
        };
        assert!(!rect.contains(9));
        assert!(rect.contains(10));
        assert!(rect.contains(29));
        assert!(!rect.contains(30));
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
