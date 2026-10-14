//! Keyboard bindings / keymap for the terminal GUI.
//!
//! This module handles:
//! - Default keyboard bindings (Ctrl+Shift+N, Ctrl+Shift+W, etc.)
//! - Custom bindings from config
//! - Key event → action resolution
//!
//! # Architecture
//!
//! ```text
//! Winit KeyEvent → Keymap → GuiAction (NewTab, CloseTab, SwitchTab, ...)
//! ```

use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey};

/// Actions that can be triggered by keyboard shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiAction {
    /// Create a new tab.
    NewTab,
    /// Close the active tab.
    CloseTab,
    /// Switch to the next tab.
    NextTab,
    /// Switch to the previous tab.
    PrevTab,
    /// Split panel horizontally (up/down).
    SplitHorizontal,
    /// Split panel vertically (left/right).
    SplitVertical,
    /// Close the active panel.
    ClosePanel,
    /// Navigate to the panel above.
    NavigateUp,
    /// Navigate to the panel below.
    NavigateDown,
    /// Navigate to the panel to the left.
    NavigateLeft,
    /// Navigate to the panel to the right.
    NavigateRight,
    /// Copy selected text to clipboard.
    Copy,
    /// Paste clipboard content to active panel.
    Paste,
    /// Scroll the view up one line.
    ScrollLineUp,
    /// Scroll the view down one line.
    ScrollLineDown,
    /// Scroll the view up one page.
    ScrollPageUp,
    /// Scroll the view down one page.
    ScrollPageDown,
    /// Jump the view to the oldest history line.
    ScrollTop,
    /// Jump the view to the newest output.
    ScrollBottom,
    /// No action (key not bound).
    None,
}

/// A single keyboard binding.
#[derive(Debug, Clone)]
pub struct KeyBinding {
    /// The key combination.
    pub key: KeyCombination,
    /// The action to perform.
    pub action: GuiAction,
}

/// A key combination (key + modifiers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCombination {
    /// The main key (as string for comparison).
    pub key: String,
    /// Required modifier state (bitmask).
    pub modifiers: u64,
}

impl KeyCombination {
    /// Create a new key combination.
    pub fn new(key: &str, modifiers: ModifiersState) -> Self {
        Self {
            key: key.to_lowercase(),
            modifiers: u64::from(modifiers.bits()),
        }
    }

    /// Check if this combination matches the given key event.
    pub fn matches(&self, key: &Key, modifiers: &ModifiersState) -> bool {
        // Match modifiers
        if self.modifiers != u64::from(modifiers.bits()) {
            return false;
        }

        // Match key
        let key_str = key_to_string(key);
        key_str.eq_ignore_ascii_case(&self.key)
    }
}

/// Keyboard binding manager.
///
/// Resolves key events to GUI actions using default bindings
/// or custom bindings from configuration.
#[derive(Debug)]
pub struct Keymap {
    /// Custom bindings (from config).
    custom_bindings: Vec<KeyBinding>,
    /// Default bindings.
    default_bindings: Vec<KeyBinding>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new()
    }
}

impl Keymap {
    /// Create a new keymap with default bindings.
    pub fn new() -> Self {
        let default_bindings = Self::build_default_bindings();
        Self {
            custom_bindings: Vec::new(),
            default_bindings,
        }
    }

    /// Add a custom binding.
    pub fn add_binding(&mut self, binding: KeyBinding) {
        self.custom_bindings.push(binding);
    }

    /// Resolve a key event to a GUI action.
    ///
    /// Custom bindings take precedence over default bindings.
    pub fn resolve(&self, key: &Key, modifiers: &ModifiersState) -> GuiAction {
        // Check custom bindings first
        for binding in &self.custom_bindings {
            if binding.key.matches(key, modifiers) {
                return binding.action;
            }
        }

        // Check default bindings
        for binding in &self.default_bindings {
            if binding.key.matches(key, modifiers) {
                return binding.action;
            }
        }

        GuiAction::None
    }

    /// Get the default bindings.
    pub fn default_bindings(&self) -> &[KeyBinding] {
        &self.default_bindings
    }

    /// Get the custom bindings.
    pub fn custom_bindings(&self) -> &[KeyBinding] {
        &self.custom_bindings
    }

    /// Clear all custom bindings.
    pub fn clear_custom_bindings(&mut self) {
        self.custom_bindings.clear();
    }

    /// Load bindings from a TOML config (stub).
    ///
    /// In production, this would parse TOML and create `KeyBinding`s.
    pub fn from_config(_bindings: &[[String; 2]]) {
        // Stub: Parse TOML bindings array
        // Example: [["Ctrl+Shift+N", "NewTab"], ["Ctrl+Shift+W", "CloseTab"]]
    }

    /// Build the default key bindings.
    fn build_default_bindings() -> Vec<KeyBinding> {
        let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;
        vec![
            // Tab management (RF-2, RF-3)
            KeyBinding {
                key: KeyCombination::new("n", ctrl_shift),
                action: GuiAction::NewTab,
            },
            KeyBinding {
                key: KeyCombination::new("w", ctrl_shift),
                action: GuiAction::CloseTab,
            },
            KeyBinding {
                key: KeyCombination::new("Tab", ctrl_shift),
                action: GuiAction::PrevTab,
            },
            KeyBinding {
                key: KeyCombination::new("Tab", ModifiersState::CONTROL),
                action: GuiAction::NextTab,
            },
            // Panel management (RF-6, RF-7, RF-8, RF-10)
            KeyBinding {
                key: KeyCombination::down(ctrl_shift),
                action: GuiAction::SplitHorizontal,
            },
            KeyBinding {
                key: KeyCombination::right(ctrl_shift),
                action: GuiAction::SplitVertical,
            },
            KeyBinding {
                key: KeyCombination::new("x", ctrl_shift),
                action: GuiAction::ClosePanel,
            },
            KeyBinding {
                key: KeyCombination::new("h", ctrl_shift),
                action: GuiAction::NavigateLeft,
            },
            KeyBinding {
                key: KeyCombination::new("j", ctrl_shift),
                action: GuiAction::NavigateDown,
            },
            KeyBinding {
                key: KeyCombination::new("k", ctrl_shift),
                action: GuiAction::NavigateUp,
            },
            KeyBinding {
                key: KeyCombination::new("l", ctrl_shift),
                action: GuiAction::NavigateRight,
            },
            // Clipboard (RF-29, RF-30)
            KeyBinding {
                key: KeyCombination::new("c", ctrl_shift),
                action: GuiAction::Copy,
            },
            KeyBinding {
                key: KeyCombination::new("v", ctrl_shift),
                action: GuiAction::Paste,
            },
            // Vertical scroll (RF-53)
            KeyBinding {
                key: KeyCombination::new("pageup", ModifiersState::SHIFT),
                action: GuiAction::ScrollPageUp,
            },
            KeyBinding {
                key: KeyCombination::new("pagedown", ModifiersState::SHIFT),
                action: GuiAction::ScrollPageDown,
            },
            KeyBinding {
                key: KeyCombination::new("home", ModifiersState::SHIFT),
                action: GuiAction::ScrollTop,
            },
            KeyBinding {
                key: KeyCombination::new("end", ModifiersState::SHIFT),
                action: GuiAction::ScrollBottom,
            },
        ]
    }
}

impl KeyCombination {
    /// Create a key combination for the down arrow.
    fn down(modifiers: ModifiersState) -> Self {
        Self {
            key: "down".to_string(),
            modifiers: u64::from(modifiers.bits()),
        }
    }

    /// Create a key combination for the right arrow.
    fn right(modifiers: ModifiersState) -> Self {
        Self {
            key: "right".to_string(),
            modifiers: u64::from(modifiers.bits()),
        }
    }
}

/// Convert a winit Key to a string for comparison.
fn key_to_string(key: &Key) -> String {
    match key {
        Key::Character(s) => s.to_string(),
        Key::Named(NamedKey::Tab) => "Tab".to_string(),
        Key::Named(NamedKey::ArrowDown) => "down".to_string(),
        Key::Named(NamedKey::ArrowRight) => "right".to_string(),
        Key::Named(NamedKey::ArrowUp) => "up".to_string(),
        Key::Named(NamedKey::ArrowLeft) => "left".to_string(),
        Key::Named(NamedKey::PageUp) => "pageup".to_string(),
        Key::Named(NamedKey::PageDown) => "pagedown".to_string(),
        Key::Named(NamedKey::Home) => "home".to_string(),
        Key::Named(NamedKey::End) => "end".to_string(),
        _ => String::new(),
    }
}

/// Convert a winit `KeyCode` to a Key string.
pub fn key_code_to_key(code: KeyCode) -> String {
    match code {
        KeyCode::KeyN => "n".to_string(),
        KeyCode::KeyW => "w".to_string(),
        KeyCode::KeyX => "x".to_string(),
        KeyCode::KeyC => "c".to_string(),
        KeyCode::KeyV => "v".to_string(),
        KeyCode::KeyH => "h".to_string(),
        KeyCode::KeyJ => "j".to_string(),
        KeyCode::KeyK => "k".to_string(),
        KeyCode::KeyL => "l".to_string(),
        KeyCode::ArrowDown => "down".to_string(),
        KeyCode::ArrowRight => "right".to_string(),
        KeyCode::ArrowUp => "up".to_string(),
        KeyCode::ArrowLeft => "left".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keymap_default() {
        let keymap = Keymap::default();
        assert!(!keymap.default_bindings().is_empty());
        assert!(keymap.custom_bindings().is_empty());
    }

    #[test]
    fn test_key_combination_match() {
        let combo = KeyCombination::new("n", ModifiersState::CONTROL | ModifiersState::SHIFT);

        // Should match
        assert!(combo.matches(
            &Key::Character("n".into()),
            &(ModifiersState::CONTROL | ModifiersState::SHIFT)
        ));

        // Should not match (wrong key)
        assert!(!combo.matches(
            &Key::Character("m".into()),
            &(ModifiersState::CONTROL | ModifiersState::SHIFT)
        ));

        // Should not match (wrong modifiers)
        assert!(!combo.matches(&Key::Character("n".into()), &ModifiersState::CONTROL));
    }

    #[test]
    fn test_key_combination_case_insensitive() {
        let combo = KeyCombination::new("n", ModifiersState::CONTROL | ModifiersState::SHIFT);

        assert!(combo.matches(
            &Key::Character("N".into()),
            &(ModifiersState::CONTROL | ModifiersState::SHIFT)
        ));
    }

    #[test]
    fn test_keymap_resolve_new_tab() {
        let keymap = Keymap::default();
        let action = keymap.resolve(
            &Key::Character("n".into()),
            &(ModifiersState::CONTROL | ModifiersState::SHIFT),
        );
        assert_eq!(action, GuiAction::NewTab);
    }

    #[test]
    fn test_keymap_resolve_close_tab() {
        let keymap = Keymap::default();
        let action = keymap.resolve(
            &Key::Character("w".into()),
            &(ModifiersState::CONTROL | ModifiersState::SHIFT),
        );
        assert_eq!(action, GuiAction::CloseTab);
    }

    #[test]
    fn test_keymap_resolve_no_match() {
        let keymap = Keymap::default();
        let action = keymap.resolve(&Key::Character("z".into()), &ModifiersState::empty());
        assert_eq!(action, GuiAction::None);
    }

    #[test]
    fn test_keymap_tab_shortcuts() {
        let keymap = Keymap::default();
        let ctrl = ModifiersState::CONTROL;
        let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;

        // New tab is its own shortcut, not shadowed by "next tab".
        assert_eq!(
            keymap.resolve(&Key::Character("n".into()), &ctrl_shift),
            GuiAction::NewTab
        );
        assert_eq!(
            keymap.resolve(&Key::Named(NamedKey::Tab), &ctrl),
            GuiAction::NextTab
        );
        assert_eq!(
            keymap.resolve(&Key::Named(NamedKey::Tab), &ctrl_shift),
            GuiAction::PrevTab
        );
    }

    #[test]
    fn test_keymap_custom_binding() {
        let mut keymap = Keymap::default();
        keymap.add_binding(KeyBinding {
            key: KeyCombination::new("q", ModifiersState::CONTROL | ModifiersState::SHIFT),
            action: GuiAction::CloseTab,
        });

        let action = keymap.resolve(
            &Key::Character("q".into()),
            &(ModifiersState::CONTROL | ModifiersState::SHIFT),
        );
        assert_eq!(action, GuiAction::CloseTab);
    }

    #[test]
    fn test_keymap_custom_overrides_default() {
        let mut keymap = Keymap::default();
        // Override Ctrl+Shift+N to close tab instead of new tab
        keymap.add_binding(KeyBinding {
            key: KeyCombination::new("n", ModifiersState::CONTROL | ModifiersState::SHIFT),
            action: GuiAction::CloseTab,
        });

        let action = keymap.resolve(
            &Key::Character("n".into()),
            &(ModifiersState::CONTROL | ModifiersState::SHIFT),
        );
        assert_eq!(action, GuiAction::CloseTab); // Custom wins
    }

    #[test]
    fn test_key_code_to_key() {
        assert_eq!(key_code_to_key(KeyCode::KeyN), "n");
        assert_eq!(key_code_to_key(KeyCode::KeyW), "w");
        assert_eq!(key_code_to_key(KeyCode::ArrowDown), "down");
        assert_eq!(key_code_to_key(KeyCode::ArrowUp), "up");
        assert_eq!(key_code_to_key(KeyCode::ArrowLeft), "left");
        assert_eq!(key_code_to_key(KeyCode::Tab), "Tab");
    }

    #[test]
    fn test_keymap_resolve_split_horizontal() {
        let keymap = Keymap::default();
        let action = keymap.resolve(
            &Key::Named(NamedKey::ArrowDown),
            &(ModifiersState::CONTROL | ModifiersState::SHIFT),
        );
        assert_eq!(action, GuiAction::SplitHorizontal);
    }

    #[test]
    fn test_keymap_resolve_split_vertical() {
        let keymap = Keymap::default();
        let action = keymap.resolve(
            &Key::Named(NamedKey::ArrowRight),
            &(ModifiersState::CONTROL | ModifiersState::SHIFT),
        );
        assert_eq!(action, GuiAction::SplitVertical);
    }

    #[test]
    fn test_keymap_resolve_close_panel() {
        let keymap = Keymap::default();
        let action = keymap.resolve(
            &Key::Character("x".into()),
            &(ModifiersState::CONTROL | ModifiersState::SHIFT),
        );
        assert_eq!(action, GuiAction::ClosePanel);
    }

    #[test]
    fn test_keymap_resolve_navigate_hjkl() {
        let keymap = Keymap::default();
        let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;

        let action = keymap.resolve(&Key::Character("h".into()), &ctrl_shift);
        assert_eq!(action, GuiAction::NavigateLeft);

        let action = keymap.resolve(&Key::Character("j".into()), &ctrl_shift);
        assert_eq!(action, GuiAction::NavigateDown);

        let action = keymap.resolve(&Key::Character("k".into()), &ctrl_shift);
        assert_eq!(action, GuiAction::NavigateUp);

        let action = keymap.resolve(&Key::Character("l".into()), &ctrl_shift);
        assert_eq!(action, GuiAction::NavigateRight);
    }

    #[test]
    fn test_keymap_resolve_scroll_bindings() {
        let keymap = Keymap::default();
        let shift = ModifiersState::SHIFT;

        assert_eq!(
            keymap.resolve(&Key::Named(NamedKey::PageUp), &shift),
            GuiAction::ScrollPageUp
        );
        assert_eq!(
            keymap.resolve(&Key::Named(NamedKey::PageDown), &shift),
            GuiAction::ScrollPageDown
        );
        assert_eq!(
            keymap.resolve(&Key::Named(NamedKey::Home), &shift),
            GuiAction::ScrollTop
        );
        assert_eq!(
            keymap.resolve(&Key::Named(NamedKey::End), &shift),
            GuiAction::ScrollBottom
        );
    }

    #[test]
    fn test_all_default_actions_present() {
        let keymap = Keymap::default();
        let actions: Vec<GuiAction> = keymap.default_bindings().iter().map(|b| b.action).collect();

        assert!(actions.contains(&GuiAction::NewTab));
        assert!(actions.contains(&GuiAction::CloseTab));
        assert!(actions.contains(&GuiAction::NextTab));
        assert!(actions.contains(&GuiAction::PrevTab));
        assert!(actions.contains(&GuiAction::SplitHorizontal));
        assert!(actions.contains(&GuiAction::SplitVertical));
        assert!(actions.contains(&GuiAction::ClosePanel));
        assert!(actions.contains(&GuiAction::NavigateUp));
        assert!(actions.contains(&GuiAction::NavigateDown));
        assert!(actions.contains(&GuiAction::NavigateLeft));
        assert!(actions.contains(&GuiAction::NavigateRight));
        assert!(actions.contains(&GuiAction::Copy));
        assert!(actions.contains(&GuiAction::Paste));
        assert!(actions.contains(&GuiAction::ScrollPageUp));
        assert!(actions.contains(&GuiAction::ScrollPageDown));
        assert!(actions.contains(&GuiAction::ScrollTop));
        assert!(actions.contains(&GuiAction::ScrollBottom));
    }
}
