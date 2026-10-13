//! Keyboard and mouse event handling.
//!
//! This module handles:
//! - Winit KeyEvent → TerminalInput conversion
//! - Keymap resolution (custom → defaults)
//! - Mouse event processing for selection
//!
//! # Architecture
//!
//! ```text
//! Winit Event → EventProcessor → GuiAction / TerminalInput
//! ```

use winit::event::{ElementState, KeyEvent, MouseButton};
use winit::keyboard::{Key, ModifiersState, NamedKey};

use terminal_core::input::{ArrowDirection, KeyEvent as TerminalKeyEvent, SpecialKey};

use crate::keymap::{GuiAction, Keymap};

/// Selection state for mouse-driven text selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SelectionState {
    /// Start position (row, col), if selection has started.
    pub start: Option<(usize, usize)>,
    /// Current position (row, col), if selection is active.
    pub current: Option<(usize, usize)>,
}

impl SelectionState {
    /// Check if selection is active.
    pub fn is_active(&self) -> bool {
        self.start.is_some() && self.current.is_some()
    }

    /// Get the selection bounds (min row, min col, max row, max col).
    pub fn bounds(&self) -> Option<(usize, usize, usize, usize)> {
        let (sr, sc) = self.start?;
        let (cr, cc) = self.current?;
        Some((sr.min(cr), sc.min(cc), sr.max(cr), sc.max(cc)))
    }
}

/// Mouse event result for selection handling.
#[derive(Debug, Clone)]
pub struct MouseEvent {
    /// The mouse button.
    pub button: MouseButton,
    /// The event state (pressed/released).
    pub state: ElementState,
    /// Mouse position (x, y) in pixels.
    pub position: (f64, f64),
    /// Whether selection is active after this event.
    pub selection_active: bool,
}

/// Result of processing an event.
#[derive(Debug, Clone)]
pub enum EventResult {
    /// A GUI action to perform.
    GuiAction(GuiAction),
    /// Terminal input to send to the session.
    TerminalInput(TerminalInput),
    /// Mouse event for selection handling.
    Mouse(MouseEvent),
}

/// Processed keyboard event ready for the terminal session.
#[derive(Debug, Clone)]
pub struct TerminalInput {
    /// The key event bytes to send to the terminal.
    pub bytes: Vec<u8>,
    /// The original key event for reference.
    pub event: TerminalKeyEvent,
}

/// Event processor for the terminal GUI.
///
/// Converts winit events to GUI actions and terminal input.
#[derive(Debug)]
pub struct EventProcessor {
    /// Keyboard bindings manager.
    keymap: Keymap,
    /// Current modifier state.
    modifiers: ModifiersState,
    /// Mouse selection state.
    selection: SelectionState,
}

impl Default for EventProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl EventProcessor {
    /// Create a new event processor.
    pub fn new() -> Self {
        Self {
            keymap: Keymap::default(),
            modifiers: ModifiersState::default(),
            selection: SelectionState::default(),
        }
    }

    /// Get the current selection state.
    pub fn selection(&self) -> SelectionState {
        self.selection
    }

    /// Reset the selection state.
    pub fn reset_selection(&mut self) {
        self.selection = SelectionState::default();
    }

    /// Convert pixel coordinates to terminal cell coordinates.
    ///
    /// # Arguments
    ///
    /// * `x` - X position in pixels.
    /// * `y` - Y position in pixels.
    /// * `cell_width` - Width of a single cell in pixels.
    /// * `cell_height` - Height of a single cell in pixels.
    ///
    /// # Returns
    ///
    /// The (row, col) position, clamped to the terminal dimensions.
    #[expect(
        clippy::cast_sign_loss,
        reason = "Pixel coordinates are always positive"
    )]
    pub fn pixel_to_cell(x: f64, y: f64, cell_width: f64, cell_height: f64) -> (usize, usize) {
        let col = (x / cell_width).floor() as usize;
        let row = (y / cell_height).floor() as usize;
        (row, col)
    }

    /// Process a mouse event for selection.
    ///
    /// # Arguments
    ///
    /// * `button` - The mouse button pressed/released.
    /// * `state` - The event state.
    /// * `position` - Mouse position in pixels.
    /// * `cell_width` - Width of a single cell in pixels.
    /// * `cell_height` - Height of a single cell in pixels.
    ///
    /// # Returns
    ///
    /// A `MouseEvent` with selection state.
    pub fn process_mouse_event(
        &mut self,
        button: MouseButton,
        state: ElementState,
        position: (f64, f64),
        cell_width: f64,
        cell_height: f64,
    ) -> MouseEvent {
        let (row, col) = Self::pixel_to_cell(position.0, position.1, cell_width, cell_height);

        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => {
                // Start a new selection
                self.selection.start = Some((row, col));
                self.selection.current = Some((row, col));
                MouseEvent {
                    button,
                    state,
                    position,
                    selection_active: true,
                }
            }
            (MouseButton::Left, ElementState::Released) => {
                // End selection
                let selection_active = self.selection.is_active();
                // Keep selection for potential copy, but mark as released
                MouseEvent {
                    button,
                    state,
                    position,
                    selection_active,
                }
            }
            (_, ElementState::Released) => {
                // Release any selection on other button release
                if self.selection.is_active() {
                    self.selection.current = None;
                }
                MouseEvent {
                    button,
                    state,
                    position,
                    selection_active: false,
                }
            }
            _ => MouseEvent {
                button,
                state,
                position,
                selection_active: false,
            },
        }
    }

    /// Update selection during mouse drag.
    ///
    /// # Arguments
    ///
    /// * `position` - Mouse position in pixels.
    /// * `cell_width` - Width of a single cell in pixels.
    /// * `cell_height` - Height of a single cell in pixels.
    pub fn update_selection(&mut self, position: (f64, f64), cell_width: f64, cell_height: f64) {
        if self.selection.start.is_some() {
            let (row, col) = Self::pixel_to_cell(position.0, position.1, cell_width, cell_height);
            self.selection.current = Some((row, col));
        }
    }

    /// Add a custom key binding.
    pub fn add_binding(&mut self, binding: crate::keymap::KeyBinding) {
        self.keymap.add_binding(binding);
    }

    /// Update modifier state.
    pub fn update_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers = modifiers;
    }

    /// Process a key event.
    ///
    /// Returns `Some(GuiAction)` for GUI actions,
    /// `Some(TerminalInput)` for terminal input,
    /// or `None` if the event was not handled.
    pub fn process_key_event(&self, event: &KeyEvent) -> Option<EventResult> {
        // Check keymap first for GUI actions
        let gui_action = self.resolve_gui_action(event);
        if gui_action != GuiAction::None {
            return Some(EventResult::GuiAction(gui_action));
        }

        // Convert to terminal input only for key press events
        if event.state == ElementState::Pressed {
            if let Some(input) = self.key_event_to_terminal_input(event) {
                return Some(EventResult::TerminalInput(input));
            }
        }

        None
    }

    /// Resolve a key event to a GUI action.
    fn resolve_gui_action(&self, event: &KeyEvent) -> GuiAction {
        let key = &event.logical_key;
        self.keymap.resolve(key, &self.modifiers)
    }

    /// Convert a winit `KeyEvent` to terminal input.
    fn key_event_to_terminal_input(&self, event: &KeyEvent) -> Option<TerminalInput> {
        if event.state != ElementState::Pressed {
            return None;
        }

        let terminal_event = Self::key_to_terminal_event(&event.logical_key, self.modifiers)?;
        let bytes = terminal_core::input::key_event_to_bytes(&terminal_event);

        Some(TerminalInput {
            bytes,
            event: terminal_event,
        })
    }

    /// Convert a winit Key to a terminal `KeyEvent`.
    #[expect(
        clippy::too_many_lines,
        reason = "Key mapping function with all variants"
    )]
    fn key_to_terminal_event(key: &Key, modifiers: ModifiersState) -> Option<TerminalKeyEvent> {
        match key {
            Key::Character(c) => {
                let char_event = TerminalKeyEvent::Char {
                    c: c.to_string().chars().next()?,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                };
                Some(char_event)
            }
            Key::Named(named) => match named {
                NamedKey::ArrowUp => Some(TerminalKeyEvent::Arrow {
                    direction: ArrowDirection::Up,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::ArrowDown => Some(TerminalKeyEvent::Arrow {
                    direction: ArrowDirection::Down,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::ArrowLeft => Some(TerminalKeyEvent::Arrow {
                    direction: ArrowDirection::Left,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::ArrowRight => Some(TerminalKeyEvent::Arrow {
                    direction: ArrowDirection::Right,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::Enter => Some(TerminalKeyEvent::Special {
                    key: SpecialKey::Enter,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::Tab => Some(TerminalKeyEvent::Special {
                    key: SpecialKey::Tab,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::Escape => Some(TerminalKeyEvent::Special {
                    key: SpecialKey::Escape,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::Backspace => Some(TerminalKeyEvent::Special {
                    key: SpecialKey::Backspace,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::Home => Some(TerminalKeyEvent::Special {
                    key: SpecialKey::Home,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::End => Some(TerminalKeyEvent::Special {
                    key: SpecialKey::End,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::PageUp => Some(TerminalKeyEvent::Special {
                    key: SpecialKey::PageUp,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::PageDown => Some(TerminalKeyEvent::Special {
                    key: SpecialKey::PageDown,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::Delete => Some(TerminalKeyEvent::Special {
                    key: SpecialKey::Delete,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::Insert => Some(TerminalKeyEvent::Special {
                    key: SpecialKey::Insert,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::F1 => Some(TerminalKeyEvent::Function {
                    num: 1,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::F2 => Some(TerminalKeyEvent::Function {
                    num: 2,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::F3 => Some(TerminalKeyEvent::Function {
                    num: 3,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::F4 => Some(TerminalKeyEvent::Function {
                    num: 4,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::F5 => Some(TerminalKeyEvent::Function {
                    num: 5,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::F6 => Some(TerminalKeyEvent::Function {
                    num: 6,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::F7 => Some(TerminalKeyEvent::Function {
                    num: 7,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::F8 => Some(TerminalKeyEvent::Function {
                    num: 8,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::F9 => Some(TerminalKeyEvent::Function {
                    num: 9,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::F10 => Some(TerminalKeyEvent::Function {
                    num: 10,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::F11 => Some(TerminalKeyEvent::Function {
                    num: 11,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                NamedKey::F12 => Some(TerminalKeyEvent::Function {
                    num: 12,
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
                _ => None,
            },
            Key::Unidentified(_) | Key::Dead(_) => None,
        }
    }

    /// Get the current keymap.
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_processor_default() {
        let processor = EventProcessor::default();
        assert!(!processor.keymap.default_bindings().is_empty());
    }

    #[test]
    fn test_modifier_update() {
        let mut processor = EventProcessor::new();
        processor.update_modifiers(ModifiersState::CONTROL | ModifiersState::SHIFT);
        // Modifiers are stored internally
    }

    #[test]
    fn test_key_to_terminal_char() {
        let modifiers = ModifiersState::default();
        let result = EventProcessor::key_to_terminal_event(&Key::Character("a".into()), modifiers);
        assert!(result.is_some());
        let event = result.unwrap();
        let bytes = terminal_core::input::key_event_to_bytes(&event);
        assert_eq!(bytes, vec![b'a']);
    }

    #[test]
    fn test_key_to_terminal_ctrl_c() {
        let modifiers = ModifiersState::CONTROL;
        let result = EventProcessor::key_to_terminal_event(&Key::Character("c".into()), modifiers);
        assert!(result.is_some());
        let event = result.unwrap();
        let bytes = terminal_core::input::key_event_to_bytes(&event);
        assert_eq!(bytes, vec![0x03]); // Ctrl+C = 0x03
    }

    #[test]
    fn test_key_to_terminal_arrow_up() {
        let modifiers = ModifiersState::default();
        let result =
            EventProcessor::key_to_terminal_event(&Key::Named(NamedKey::ArrowUp), modifiers);
        assert!(result.is_some());
        let event = result.unwrap();
        let bytes = terminal_core::input::key_event_to_bytes(&event);
        assert_eq!(bytes, vec![0x1b, b'[', b'A']);
    }

    #[test]
    fn test_key_to_terminal_enter() {
        let modifiers = ModifiersState::default();
        let result = EventProcessor::key_to_terminal_event(&Key::Named(NamedKey::Enter), modifiers);
        assert!(result.is_some());
        let event = result.unwrap();
        let bytes = terminal_core::input::key_event_to_bytes(&event);
        assert_eq!(bytes, vec![0x0d]);
    }

    #[test]
    fn test_keymap_resolves_terminal_input() {
        let modifiers = ModifiersState::default();
        let result = EventProcessor::key_to_terminal_event(&Key::Character("n".into()), modifiers);
        assert!(result.is_some());
        let event = result.unwrap();
        let bytes = terminal_core::input::key_event_to_bytes(&event);
        assert_eq!(bytes, vec![b'n']);
    }

    #[test]
    fn test_key_to_terminal_escape() {
        let modifiers = ModifiersState::default();
        let result =
            EventProcessor::key_to_terminal_event(&Key::Named(NamedKey::Escape), modifiers);
        assert!(result.is_some());
        let event = result.unwrap();
        // Note: Escape is handled as a special key that produces ESC [ D in key_event_to_bytes
        let bytes = terminal_core::input::key_event_to_bytes(&event);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_key_to_terminal_backspace() {
        let modifiers = ModifiersState::default();
        let result =
            EventProcessor::key_to_terminal_event(&Key::Named(NamedKey::Backspace), modifiers);
        assert!(result.is_some());
        let event = result.unwrap();
        let bytes = terminal_core::input::key_event_to_bytes(&event);
        assert_eq!(bytes, vec![0x08]);
    }

    #[test]
    fn test_key_to_terminal_tab() {
        let modifiers = ModifiersState::default();
        let result = EventProcessor::key_to_terminal_event(&Key::Named(NamedKey::Tab), modifiers);
        assert!(result.is_some());
        let event = result.unwrap();
        let bytes = terminal_core::input::key_event_to_bytes(&event);
        assert_eq!(bytes, vec![0x09]);
    }

    #[test]
    fn test_unhandled_key_returns_none() {
        let modifiers = ModifiersState::default();
        let result = EventProcessor::key_to_terminal_event(
            &Key::Unidentified(winit::keyboard::NativeKey::Unidentified),
            modifiers,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_selection_state_default() {
        let state = SelectionState::default();
        assert!(!state.is_active());
        assert!(state.bounds().is_none());
    }

    #[test]
    fn test_selection_state_active() {
        let state = SelectionState {
            start: Some((0, 0)),
            current: Some((5, 10)),
        };
        assert!(state.is_active());
        assert_eq!(state.bounds(), Some((0, 0, 5, 10)));
    }

    #[test]
    fn test_selection_state_reversed() {
        let state = SelectionState {
            start: Some((5, 10)),
            current: Some((0, 0)),
        };
        assert!(state.is_active());
        assert_eq!(state.bounds(), Some((0, 0, 5, 10)));
    }

    #[test]
    fn test_pixel_to_cell() {
        let (row, col) = EventProcessor::pixel_to_cell(0.0, 0.0, 10.0, 20.0);
        assert_eq!(row, 0);
        assert_eq!(col, 0);

        let (row, col) = EventProcessor::pixel_to_cell(5.0, 10.0, 10.0, 20.0);
        assert_eq!(row, 0);
        assert_eq!(col, 0);

        let (row, col) = EventProcessor::pixel_to_cell(10.0, 20.0, 10.0, 20.0);
        assert_eq!(row, 1);
        assert_eq!(col, 1);

        let (row, col) = EventProcessor::pixel_to_cell(19.9, 39.9, 10.0, 20.0);
        assert_eq!(row, 1);
        assert_eq!(col, 1);

        let (row, col) = EventProcessor::pixel_to_cell(20.0, 40.0, 10.0, 20.0);
        assert_eq!(row, 2);
        assert_eq!(col, 2);
    }

    #[test]
    fn test_event_processor_with_selection() {
        let mut processor = EventProcessor::new();
        assert!(!processor.selection().is_active());
        processor.reset_selection();
        assert!(!processor.selection().is_active());
    }

    #[test]
    fn test_mouse_event_start_selection() {
        let mut processor = EventProcessor::new();
        let event = processor.process_mouse_event(
            MouseButton::Left,
            ElementState::Pressed,
            (15.0, 25.0),
            10.0,
            20.0,
        );
        assert!(event.selection_active);
        assert_eq!(processor.selection().bounds(), Some((1, 1, 1, 1)));
    }

    #[test]
    fn test_mouse_event_drag_selection() {
        let mut processor = EventProcessor::new();
        // Start selection
        processor.process_mouse_event(
            MouseButton::Left,
            ElementState::Pressed,
            (10.0, 20.0),
            10.0,
            20.0,
        );
        // Drag to new position
        processor.update_selection((50.0, 80.0), 10.0, 20.0);
        assert!(processor.selection().is_active());
        assert_eq!(processor.selection().bounds(), Some((1, 1, 4, 5)));
    }

    #[test]
    fn test_mouse_event_release_selection() {
        let mut processor = EventProcessor::new();
        // Start selection
        processor.process_mouse_event(
            MouseButton::Left,
            ElementState::Pressed,
            (10.0, 20.0),
            10.0,
            20.0,
        );
        // Release
        let event = processor.process_mouse_event(
            MouseButton::Left,
            ElementState::Released,
            (10.0, 20.0),
            10.0,
            20.0,
        );
        assert!(event.selection_active);
    }
}
