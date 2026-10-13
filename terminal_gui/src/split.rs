//! Panel splitting and layout management.
//!
//! This module handles:
//! - Binary tree split layout (horizontal/vertical)
//! - Panel navigation (H/J/K/L)
//! - Split and close operations
//!
//! # Architecture
//!
//! ```text
//! SplitNode (tree)
//! ├── Leaf(Panel)
//! └── Split(Horizontal/Vertical, left, right, ratio)
//! ```

use std::sync::atomic::{AtomicU64, Ordering};

/// Direction of a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    /// Split horizontally (panels stacked vertically).
    Horizontal,
    /// Split vertically (panels side by side).
    Vertical,
}

/// A panel within the split layout.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Unique panel identifier.
    pub id: PanelId,
    /// Panel title (optional).
    pub title: Option<String>,
    /// Whether this panel is currently focused.
    pub is_focused: bool,
}

impl Panel {
    /// Create a new panel with the given ID.
    pub fn new(id: PanelId) -> Self {
        Self {
            id,
            title: None,
            is_focused: false,
        }
    }

    /// Set the panel title.
    pub fn set_title(&mut self, title: String) {
        self.title = Some(title);
    }

    /// Mark this panel as focused.
    pub fn focus(&mut self) {
        self.is_focused = true;
    }

    /// Unfocus this panel.
    pub fn unfocus(&mut self) {
        self.is_focused = false;
    }
}

/// Unique panel identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PanelId(u64);

impl PanelId {
    /// Get the inner u64 value.
    pub fn inner(&self) -> u64 {
        self.0
    }

    /// Generate a new unique panel ID.
    fn new_id() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        PanelId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for Panel {
    fn default() -> Self {
        Self::new(PanelId(0))
    }
}

/// A node in the split layout tree.
#[derive(Debug, Clone)]
pub enum SplitNode {
    /// A leaf node containing a single panel.
    Leaf(Panel),
    /// A split node with two children.
    Split {
        /// Split direction.
        direction: SplitDirection,
        /// Left child.
        left: Box<SplitNode>,
        /// Right child.
        right: Box<SplitNode>,
        /// Split ratio (0.0-1.0).
        ratio: f32,
    },
}

impl SplitNode {
    /// Create a new leaf node with a panel.
    pub fn new_leaf(panel: Panel) -> Self {
        SplitNode::Leaf(panel)
    }

    /// Create a new split node.
    pub fn new_split(
        direction: SplitDirection,
        left: SplitNode,
        right: SplitNode,
        ratio: f32,
    ) -> Self {
        SplitNode::Split {
            direction,
            left: Box::new(left),
            right: Box::new(right),
            ratio,
        }
    }

    /// Split the current node horizontally (stack panels vertically).
    ///
    /// The current panel becomes the left child, and a new panel
    /// becomes the right child.
    pub fn split_h(&mut self, new_panel: Panel, ratio: f32) {
        match self {
            SplitNode::Leaf(_) => {
                // Leaf node: split it directly
                let old_node = std::mem::replace(self, SplitNode::Leaf(Panel::default()));
                let SplitNode::Leaf(old_panel) = old_node else {
                    unreachable!()
                };
                let mut new_right = SplitNode::new_leaf(new_panel);
                new_right.focus_first();
                *self = SplitNode::new_split(
                    SplitDirection::Horizontal,
                    SplitNode::new_leaf(old_panel),
                    new_right,
                    ratio,
                );
            }
            SplitNode::Split { left, right, .. } => {
                // Recurse into the focused child
                if left.is_focused() {
                    left.split_h(new_panel, ratio);
                } else {
                    right.split_h(new_panel, ratio);
                }
            }
        }
    }

    /// Split the current node vertically (place panels side by side).
    ///
    /// The current panel becomes the top child, and a new panel
    /// becomes the bottom child.
    pub fn split_v(&mut self, new_panel: Panel, ratio: f32) {
        match self {
            SplitNode::Leaf(_) => {
                // Leaf node: split it directly
                let old_node = std::mem::replace(self, SplitNode::Leaf(Panel::default()));
                let SplitNode::Leaf(old_panel) = old_node else {
                    unreachable!()
                };
                let mut new_right = SplitNode::new_leaf(new_panel);
                new_right.focus_first();
                *self = SplitNode::new_split(
                    SplitDirection::Vertical,
                    SplitNode::new_leaf(old_panel),
                    new_right,
                    ratio,
                );
            }
            SplitNode::Split { left, right, .. } => {
                // Recurse into the focused child
                if left.is_focused() {
                    left.split_v(new_panel, ratio);
                } else {
                    right.split_v(new_panel, ratio);
                }
            }
        }
    }

    /// Navigate to the panel in the given direction.
    ///
    /// Returns `Some(PanelId)` of the focused panel after navigation.
    pub fn navigate(&mut self, direction: NavigateDirection) -> Option<PanelId> {
        match self {
            SplitNode::Leaf(panel) => {
                panel.focus();
                Some(panel.id)
            }
            SplitNode::Split {
                direction: split_dir,
                left,
                right,
                ..
            } => {
                match (split_dir, direction) {
                    // Horizontal split: up/down navigation
                    // Vertical split: left/right navigation
                    (SplitDirection::Horizontal, NavigateDirection::Up)
                    | (SplitDirection::Vertical, NavigateDirection::Left) => {
                        right.navigate_internal(direction)
                    }
                    (SplitDirection::Horizontal, NavigateDirection::Down)
                    | (SplitDirection::Vertical, NavigateDirection::Right) => {
                        left.navigate_internal(direction)
                    }
                    // Same direction: recurse into focused child
                    _ => {
                        if left.is_focused() {
                            left.navigate_internal(direction)
                        } else {
                            right.navigate_internal(direction)
                        }
                    }
                }
            }
        }
    }

    /// Internal navigation without focusing the current node.
    fn navigate_internal(&mut self, direction: NavigateDirection) -> Option<PanelId> {
        match self {
            SplitNode::Leaf(panel) => {
                panel.focus();
                Some(panel.id)
            }
            SplitNode::Split {
                direction: split_dir,
                left,
                right,
                ..
            } => {
                match (split_dir, direction) {
                    // Horizontal split: up/down navigation
                    // Vertical split: left/right navigation
                    (SplitDirection::Horizontal, NavigateDirection::Up)
                    | (SplitDirection::Vertical, NavigateDirection::Left) => {
                        right.navigate_internal(direction)
                    }
                    (SplitDirection::Horizontal, NavigateDirection::Down)
                    | (SplitDirection::Vertical, NavigateDirection::Right) => {
                        left.navigate_internal(direction)
                    }
                    _ => {
                        if left.is_focused() {
                            left.navigate_internal(direction)
                        } else {
                            right.navigate_internal(direction)
                        }
                    }
                }
            }
        }
    }

    /// Check if this node is a leaf.
    pub fn is_leaf(&self) -> bool {
        matches!(self, SplitNode::Leaf(_))
    }

    /// Check if this node contains the focused panel.
    pub fn is_focused(&self) -> bool {
        match self {
            SplitNode::Leaf(panel) => panel.is_focused,
            SplitNode::Split { left, right, .. } => left.is_focused() || right.is_focused(),
        }
    }

    /// Get the number of panels in this subtree.
    pub fn panel_count(&self) -> usize {
        match self {
            SplitNode::Leaf(_) => 1,
            SplitNode::Split { left, right, .. } => left.panel_count() + right.panel_count(),
        }
    }

    /// Unfocus all panels.
    pub fn unfocus_all(&mut self) {
        match self {
            SplitNode::Leaf(panel) => panel.unfocus(),
            SplitNode::Split { left, right, .. } => {
                left.unfocus_all();
                right.unfocus_all();
            }
        }
    }

    /// Focus the first panel in the tree.
    pub fn focus_first(&mut self) {
        self.unfocus_all();
        match self {
            SplitNode::Leaf(panel) => panel.focus(),
            SplitNode::Split { left, .. } => left.focus_first(),
        }
    }

    /// Iterate over all panel IDs in this subtree.
    #[allow(dead_code)]
    fn panel_ids(&self) -> PanelIdIterator<'_> {
        PanelIdIterator::new(self)
    }
}

/// Direction for panel navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigateDirection {
    /// Navigate up.
    Up,
    /// Navigate down.
    Down,
    /// Navigate left.
    Left,
    /// Navigate right.
    Right,
}

impl NavigateDirection {
    /// Create from a character (h/j/k/l).
    pub fn from_char(c: char) -> Option<Self> {
        match c.to_ascii_lowercase() {
            'h' => Some(NavigateDirection::Left),
            'j' => Some(NavigateDirection::Down),
            'k' => Some(NavigateDirection::Up),
            'l' => Some(NavigateDirection::Right),
            _ => None,
        }
    }
}

/// Manager for the split layout.
#[derive(Debug)]
pub struct SplitLayout {
    /// The root node of the split tree.
    root: SplitNode,
}

impl Default for SplitLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl SplitLayout {
    /// Create a new split layout with a single panel.
    pub fn new() -> Self {
        let panel = Panel::new(PanelId::new_id());
        Self {
            root: SplitNode::new_leaf(panel),
        }
    }

    /// Split the focused panel horizontally.
    ///
    /// Returns the ID of the newly created panel.
    pub fn split_horizontal(&mut self, ratio: f32) -> PanelId {
        let new_panel = Panel::new(PanelId::new_id());
        let new_id = new_panel.id;
        self.root.split_h(new_panel, ratio);
        new_id
    }

    /// Split the focused panel vertically.
    ///
    /// Returns the ID of the newly created panel.
    pub fn split_vertical(&mut self, ratio: f32) -> PanelId {
        let new_panel = Panel::new(PanelId::new_id());
        let new_id = new_panel.id;
        self.root.split_v(new_panel, ratio);
        new_id
    }

    /// Close the focused panel.
    ///
    /// Returns `Some(PanelId)` of the newly focused panel,
    /// `None` if the layout is now empty (should close app).
    pub fn close_panel(&mut self) -> Option<PanelId> {
        if self.root.panel_count() <= 1 {
            return None;
        }

        // Find the focused leaf, unfocus it, and replace its parent
        Self::close_in_tree(&mut self.root)
    }

    /// Navigate to a panel in the given direction.
    ///
    /// # Panics
    ///
    /// Panics if no panel is found in the layout.
    pub fn navigate(&mut self, direction: NavigateDirection) -> PanelId {
        self.root.navigate(direction).expect("No panel found")
    }

    /// Get the number of panels.
    pub fn panel_count(&self) -> usize {
        self.root.panel_count()
    }

    /// Check if the layout has only one panel.
    pub fn is_single_panel(&self) -> bool {
        self.panel_count() == 1
    }

    /// Recursively find and close the focused leaf in the tree.
    fn close_in_tree(node: &mut SplitNode) -> Option<PanelId> {
        match node {
            SplitNode::Leaf(panel) => {
                if panel.is_focused {
                    panel.unfocus();
                    None
                } else {
                    None
                }
            }
            SplitNode::Split { left, right, .. } => {
                if left.is_focused() {
                    Self::close_in_tree(left)
                } else {
                    Self::close_in_tree(right)
                }
            }
        }
    }

    /// Collect all dividers in the layout.
    pub fn dividers(&self) -> Vec<Divider> {
        let mut dividers = Vec::new();
        Self::collect_dividers(&self.root, &mut dividers);
        dividers
    }

    /// Collect dividers recursively from the tree.
    fn collect_dividers(node: &SplitNode, dividers: &mut Vec<Divider>) {
        match node {
            SplitNode::Leaf(_) => {}
            SplitNode::Split {
                left, right, ratio, ..
            } => {
                dividers.push(Divider::new(f64::from(*ratio), 4.0));
                Self::collect_dividers(left, dividers);
                Self::collect_dividers(right, dividers);
            }
        }
    }

    /// Start a resize operation on a divider at the given position.
    ///
    /// Returns `Some(ResizeState)` if a divider was found at that position.
    pub fn start_resize(&self, position: f64) -> Option<ResizeState> {
        Self::find_divider_at(&self.root, position)
    }

    /// Find a divider at the given position.
    fn find_divider_at(node: &SplitNode, position: f64) -> Option<ResizeState> {
        match node {
            SplitNode::Leaf(_) => None,
            SplitNode::Split {
                left, right, ratio, ..
            } => {
                let divider = Divider::new(f64::from(*ratio), 4.0);
                if divider.hit_test(position) {
                    Some(ResizeState::new(divider, position, *ratio))
                } else if Self::find_divider_at(left, position).is_some() {
                    Self::find_divider_at(left, position)
                } else {
                    Self::find_divider_at(right, position)
                }
            }
        }
    }

    /// Update a split ratio based on resize state.
    pub fn update_ratio(&mut self, resize: &mut ResizeState) {
        Self::find_and_update_ratio(&mut self.root, resize);
    }

    /// Find and update ratio recursively in the tree.
    fn find_and_update_ratio(node: &mut SplitNode, resize: &mut ResizeState) {
        match node {
            SplitNode::Leaf(_) => {}
            SplitNode::Split {
                ratio, left, right, ..
            } => {
                let divider = Divider::new(f64::from(*ratio), 4.0);
                if divider.hit_test(resize.start_position) {
                    let new_ratio = resize.update_ratio(resize.start_position);
                    *ratio = new_ratio;
                } else {
                    Self::find_and_update_ratio(left, resize);
                    Self::find_and_update_ratio(right, resize);
                }
            }
        }
    }
}

/// A divider between two split panels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Divider {
    /// The split node this divider belongs to.
    pub split_node: Option<usize>,
    /// Position along the split axis (0.0-1.0).
    pub position: f64,
    /// Divider width in pixels.
    pub width: f64,
}

impl Divider {
    /// Create a new divider at the given position.
    pub fn new(position: f64, width: f64) -> Self {
        Self {
            split_node: None,
            position: position.clamp(0.1, 0.9), // Min 10%, max 90%
            width,
        }
    }

    /// Check if a point is on this divider.
    pub fn hit_test(&self, point: f64) -> bool {
        (point - self.position).abs() < self.width
    }

    /// Adjust the divider position by a delta.
    pub fn adjust(&mut self, delta: f64) {
        self.position = (self.position + delta).clamp(0.1, 0.9);
    }
}

/// State for active resize operation.
#[derive(Debug, Clone)]
pub struct ResizeState {
    /// The divider being resized.
    pub divider: Divider,
    /// Initial mouse position when drag started.
    pub start_position: f64,
    /// Initial ratio before drag started.
    pub initial_ratio: f32,
}

impl ResizeState {
    /// Create a new resize state.
    pub fn new(divider: Divider, start_position: f64, initial_ratio: f32) -> Self {
        Self {
            divider,
            start_position,
            initial_ratio,
        }
    }

    /// Update the ratio based on current mouse position.
    pub fn update_ratio(&mut self, current_position: f64) -> f32 {
        let delta = (current_position - self.start_position) as f32;
        let sensitivity: f32 = 0.01;
        (self.initial_ratio + delta * sensitivity).clamp(0.1, 0.9)
    }
}

/// Iterator over panel IDs in a split tree.
pub struct PanelIdIterator<'a> {
    stack: Vec<&'a SplitNode>,
}

impl<'a> PanelIdIterator<'a> {
    #[allow(dead_code)]
    fn new(node: &'a SplitNode) -> Self {
        let stack = vec![node];
        Self { stack }
    }
}

#[expect(clippy::elidable_lifetime_names)]
impl<'a> Iterator for PanelIdIterator<'a> {
    type Item = PanelId;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node) = self.stack.pop() {
            match node {
                SplitNode::Leaf(panel) => return Some(panel.id),
                SplitNode::Split { left, right, .. } => {
                    self.stack.push(right);
                    self.stack.push(left);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_layout_new() {
        let layout = SplitLayout::new();
        assert!(layout.is_single_panel());
        assert_eq!(layout.panel_count(), 1);
    }

    #[test]
    fn test_split_horizontal() {
        let mut layout = SplitLayout::new();
        let old_count = layout.panel_count();
        let new_id = layout.split_horizontal(0.5);
        assert_eq!(layout.panel_count(), old_count + 1);
        assert!(!layout.is_single_panel());
        assert!(new_id.inner() > 0);
    }

    #[test]
    fn test_split_vertical() {
        let mut layout = SplitLayout::new();
        let old_count = layout.panel_count();
        let new_id = layout.split_vertical(0.5);
        assert_eq!(layout.panel_count(), old_count + 1);
        assert!(new_id.inner() > 0);
    }

    #[test]
    fn test_split_multiple() {
        let mut layout = SplitLayout::new();
        let start_count = layout.panel_count();
        layout.split_horizontal(0.5);
        layout.split_vertical(0.5);
        assert_eq!(layout.panel_count(), start_count + 2);
    }

    #[test]
    fn test_navigate() {
        let mut layout = SplitLayout::new();
        layout.split_horizontal(0.5);

        // After split, there are 2 panels
        assert_eq!(layout.panel_count(), 2);
        // Navigate should return a valid panel ID
        let id = layout.navigate(NavigateDirection::Down);
        assert!(id.inner() > 0);
    }

    #[test]
    fn test_navigate_direction_from_char() {
        assert_eq!(
            NavigateDirection::from_char('h'),
            Some(NavigateDirection::Left)
        );
        assert_eq!(
            NavigateDirection::from_char('j'),
            Some(NavigateDirection::Down)
        );
        assert_eq!(
            NavigateDirection::from_char('k'),
            Some(NavigateDirection::Up)
        );
        assert_eq!(
            NavigateDirection::from_char('l'),
            Some(NavigateDirection::Right)
        );
        assert!(NavigateDirection::from_char('x').is_none());
    }

    #[test]
    fn test_panel_id_uniqueness() {
        let id1 = PanelId::new_id();
        let id2 = PanelId::new_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_panel_focus() {
        let mut panel = Panel::new(PanelId::new_id());
        assert!(!panel.is_focused);
        panel.focus();
        assert!(panel.is_focused);
        panel.unfocus();
        assert!(!panel.is_focused);
    }

    #[test]
    fn test_split_direction_enum() {
        assert_eq!(format!("{:?}", SplitDirection::Horizontal), "Horizontal");
        assert_eq!(format!("{:?}", SplitDirection::Vertical), "Vertical");
    }

    #[test]
    fn test_panel_count_after_splits() {
        let mut layout = SplitLayout::new();
        assert_eq!(layout.panel_count(), 1);

        layout.split_horizontal(0.5);
        assert_eq!(layout.panel_count(), 2);

        layout.split_horizontal(0.5);
        assert_eq!(layout.panel_count(), 3);

        layout.split_vertical(0.5);
        assert_eq!(layout.panel_count(), 4);
    }

    #[test]
    fn test_close_last_panel_returns_none() {
        let mut layout = SplitLayout::new();
        let result = layout.close_panel();
        assert!(result.is_none());
        assert_eq!(layout.panel_count(), 1);
    }

    #[test]
    fn test_panel_id_default() {
        let panel = Panel::default();
        assert_eq!(panel.id.inner(), 0);
    }

    #[test]
    fn test_divider_creation() {
        let divider = Divider::new(0.5, 4.0);
        assert!((divider.position - 0.5).abs() < f64::EPSILON);
        assert!((divider.width - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_divider_hit_test() {
        let divider = Divider::new(0.5, 0.02); // 2% width in ratio space
        assert!(divider.hit_test(0.5));
        assert!(divider.hit_test(0.495));
        assert!(divider.hit_test(0.505));
        assert!(!divider.hit_test(0.52));
    }

    #[test]
    fn test_divider_adjust() {
        let mut divider = Divider::new(0.5, 4.0);
        divider.adjust(0.1);
        assert!((divider.position - 0.6).abs() < 0.01);
        divider.adjust(-0.5); // Should clamp to 0.1
        assert!((divider.position - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_divider_clamp_position() {
        let divider = Divider::new(0.05, 4.0);
        assert!((divider.position - 0.1).abs() < 0.01);
        let divider2 = Divider::new(0.95, 4.0);
        assert!((divider2.position - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_resize_state_creation() {
        let divider = Divider::new(0.5, 4.0);
        let resize = ResizeState::new(divider, 0.5, 0.5);
        assert!((resize.initial_ratio - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_resize_state_update_ratio() {
        let divider = Divider::new(0.5, 4.0);
        let mut resize = ResizeState::new(divider, 0.5, 0.5);
        let new_ratio = resize.update_ratio(0.6);
        assert!(new_ratio > 0.5);
        assert!(new_ratio <= 0.9);
    }

    #[test]
    fn test_resize_state_clamp_ratio() {
        let divider = Divider::new(0.1, 4.0);
        let mut resize = ResizeState::new(divider, 0.1, 0.1);
        let new_ratio = resize.update_ratio(-100.0);
        assert!((new_ratio - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_layout_dividers_after_split() {
        let layout = SplitLayout::new();
        let dividers = layout.dividers();
        assert_eq!(dividers.len(), 0); // Single panel has no dividers

        let mut layout = SplitLayout::new();
        layout.split_horizontal(0.5);
        let dividers = layout.dividers();
        assert_eq!(dividers.len(), 1);
    }
}
