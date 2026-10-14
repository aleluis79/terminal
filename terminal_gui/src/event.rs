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

use terminal_core::grid::Grid;
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

    /// Extract the selected text from `grid`.
    ///
    /// The selection is linear: it runs from its anchor to its current cell in
    /// reading order, taking the whole of every row in between. Trailing blanks
    /// are trimmed from each line, so copying a line never drags the empty rest
    /// of the screen along. Returns `None` when no selection is active.
    pub fn extract_text(&self, grid: &Grid) -> Option<String> {
        let start = self.start?;
        let current = self.current?;

        // Order the two ends in reading order: the earlier cell starts the
        // selection, the later one ends it.
        let ((r1, c1), (r2, c2)) = if start <= current {
            (start, current)
        } else {
            (current, start)
        };

        let last_col = grid.cols.saturating_sub(1);
        let last_row = r2.min(grid.rows.saturating_sub(1));

        let mut lines = Vec::with_capacity(last_row.saturating_sub(r1) + 1);
        for row in r1..=last_row {
            let first = if row == r1 { c1 } else { 0 };
            let last = (if row == r2 { c2 } else { last_col }).min(last_col);

            let mut line = String::new();
            for col in first..=last {
                line.push(grid.get(row, col).c);
            }
            lines.push(line.trim_end().to_owned());
        }

        Some(lines.join("\n"))
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

/// What a mouse-wheel event should do on the pane under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelAction {
    /// Scroll the scrollback view by this many lines (positive = back in time).
    Scroll(i32),
    /// Send cursor-key presses to the program, which is what pagers such as
    /// `less` (and therefore `glow -p`) expect on the alternate screen (RF-56).
    Arrows {
        /// Arrow direction to send.
        direction: ArrowDirection,
        /// How many times to repeat the key.
        count: usize,
    },
}

/// Decide what a wheel delta should do, given whether the pane is on the
/// alternate screen (RF-56, CL-16).
///
/// On the main screen the wheel moves the scrollback view (RF-53); on the
/// alternate screen there is no history, so the delta becomes cursor keys for
/// the program (the classic *alternate scroll*).
pub fn wheel_action(alternate_screen: bool, lines: i32) -> WheelAction {
    if lines == 0 {
        return WheelAction::Scroll(0);
    }
    if alternate_screen {
        WheelAction::Arrows {
            direction: if lines > 0 {
                ArrowDirection::Up
            } else {
                ArrowDirection::Down
            },
            count: lines.unsigned_abs() as usize,
        }
    } else {
        WheelAction::Scroll(lines)
    }
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
    /// Whether the left mouse button is currently held down.
    dragging: bool,
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
            dragging: false,
        }
    }

    /// Get the current selection state.
    pub fn selection(&self) -> SelectionState {
        self.selection
    }

    /// Whether a selection drag is in progress (left button held).
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// Reset the selection state.
    pub fn reset_selection(&mut self) {
        self.selection = SelectionState::default();
        self.dragging = false;
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

    /// Like [`pixel_to_cell`](Self::pixel_to_cell) but clamped to a grid.
    fn pixel_to_cell_clamped(
        x: f64,
        y: f64,
        cell_width: f64,
        cell_height: f64,
        rows: usize,
        cols: usize,
    ) -> (usize, usize) {
        let (row, col) = Self::pixel_to_cell(x, y, cell_width, cell_height);
        (
            row.min(rows.saturating_sub(1)),
            col.min(cols.saturating_sub(1)),
        )
    }

    /// Process a mouse event for selection.
    ///
    /// A selection is anchored on left button press and only becomes visible
    /// once the mouse moves (drag). Releasing without dragging clears it.
    ///
    /// # Arguments
    ///
    /// * `button` - The mouse button pressed/released.
    /// * `state` - The event state.
    /// * `position` - Mouse position in pixels.
    /// * `cell_width` - Width of a single cell in pixels.
    /// * `cell_height` - Height of a single cell in pixels.
    /// * `rows` - Number of rows in the grid (for clamping).
    /// * `cols` - Number of columns in the grid (for clamping).
    ///
    /// # Returns
    ///
    /// A `MouseEvent` with selection state.
    #[expect(
        clippy::too_many_arguments,
        reason = "Mouse event needs position, cell size and grid size"
    )]
    pub fn process_mouse_event(
        &mut self,
        button: MouseButton,
        state: ElementState,
        position: (f64, f64),
        cell_width: f64,
        cell_height: f64,
        rows: usize,
        cols: usize,
    ) -> MouseEvent {
        let (row, col) = Self::pixel_to_cell_clamped(
            position.0,
            position.1,
            cell_width,
            cell_height,
            rows,
            cols,
        );

        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => {
                // Anchor a new selection at the pressed cell.
                self.selection.start = Some((row, col));
                self.selection.current = Some((row, col));
                self.dragging = true;
                MouseEvent {
                    button,
                    state,
                    position,
                    selection_active: true,
                }
            }
            (MouseButton::Left, ElementState::Released) => {
                self.dragging = false;
                // A plain click (no drag) clears the selection instead of
                // leaving a one-cell highlight stuck on screen.
                if self.selection.start == self.selection.current {
                    self.selection = SelectionState::default();
                }
                MouseEvent {
                    button,
                    state,
                    position,
                    selection_active: self.selection.is_active(),
                }
            }
            (_, ElementState::Released) => {
                self.dragging = false;
                MouseEvent {
                    button,
                    state,
                    position,
                    selection_active: self.selection.is_active(),
                }
            }
            _ => MouseEvent {
                button,
                state,
                position,
                selection_active: self.selection.is_active(),
            },
        }
    }

    /// Update selection during mouse drag.
    ///
    /// Only extends the selection while the left button is held, so simply
    /// moving the mouse never selects text.
    ///
    /// # Arguments
    ///
    /// * `position` - Mouse position in pixels.
    /// * `cell_width` - Width of a single cell in pixels.
    /// * `cell_height` - Height of a single cell in pixels.
    /// * `rows` - Number of rows in the grid (for clamping).
    /// * `cols` - Number of columns in the grid (for clamping).
    pub fn update_selection(
        &mut self,
        position: (f64, f64),
        cell_width: f64,
        cell_height: f64,
        rows: usize,
        cols: usize,
    ) {
        if !self.dragging {
            return;
        }
        let (row, col) = Self::pixel_to_cell_clamped(
            position.0,
            position.1,
            cell_width,
            cell_height,
            rows,
            cols,
        );
        self.selection.current = Some((row, col));
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
        self.process_key(&event.logical_key, event.state)
    }

    /// Resolve a key press/release into a GUI action or terminal input.
    ///
    /// Only key *presses* produce results, so a shortcut fires once per press
    /// instead of also on release.
    ///
    /// Split out from [`process_key_event`](Self::process_key_event) so it can
    /// be exercised without constructing a winit `KeyEvent`.
    pub fn process_key(&self, key: &Key, state: ElementState) -> Option<EventResult> {
        if state != ElementState::Pressed {
            return None;
        }

        // Check the keymap first for GUI actions.
        let gui_action = self.keymap.resolve(key, &self.modifiers);
        if gui_action != GuiAction::None {
            return Some(EventResult::GuiAction(gui_action));
        }

        let terminal_event = Self::key_to_terminal_event(key, self.modifiers)?;
        let bytes = terminal_core::input::key_event_to_bytes(&terminal_event);

        Some(EventResult::TerminalInput(TerminalInput {
            bytes,
            event: terminal_event,
        }))
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
                NamedKey::Space => Some(TerminalKeyEvent::Char {
                    c: ' ',
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                }),
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
    fn test_key_to_terminal_space() {
        // winit reports the space bar as `NamedKey::Space`, not as a character.
        let modifiers = ModifiersState::default();
        let result = EventProcessor::key_to_terminal_event(&Key::Named(NamedKey::Space), modifiers);
        assert!(result.is_some());
        let event = result.unwrap();
        let bytes = terminal_core::input::key_event_to_bytes(&event);
        assert_eq!(bytes, vec![b' ']);
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
    fn test_ctrl_d_after_modifiers_change() {
        // Regression: the GUI must feed `WindowEvent::ModifiersChanged` into the
        // processor, otherwise Ctrl combos are seen as plain characters.
        let mut processor = EventProcessor::new();
        processor.update_modifiers(ModifiersState::CONTROL);

        let result = processor.process_key(&Key::Character("d".into()), ElementState::Pressed);
        match result {
            Some(EventResult::TerminalInput(input)) => assert_eq!(input.bytes, vec![0x04]),
            other => panic!("expected Ctrl+D to send EOT, got {other:?}"),
        }
    }

    #[test]
    fn test_plain_d_without_modifiers() {
        let processor = EventProcessor::new();
        let result = processor.process_key(&Key::Character("d".into()), ElementState::Pressed);
        match result {
            Some(EventResult::TerminalInput(input)) => assert_eq!(input.bytes, vec![b'd']),
            other => panic!("expected plain 'd', got {other:?}"),
        }
    }

    #[test]
    fn test_shortcut_after_modifiers_change() {
        let mut processor = EventProcessor::new();
        processor.update_modifiers(ModifiersState::CONTROL | ModifiersState::SHIFT);
        assert!(matches!(
            processor.process_key(&Key::Character("n".into()), ElementState::Pressed),
            Some(EventResult::GuiAction(GuiAction::NewTab))
        ));
    }

    #[test]
    fn test_release_does_not_trigger_action() {
        // Regression: a shortcut must fire once per press, not also on release
        // (which opened two tabs / closed two).
        let mut processor = EventProcessor::new();
        processor.update_modifiers(ModifiersState::CONTROL | ModifiersState::SHIFT);

        assert!(processor
            .process_key(&Key::Character("n".into()), ElementState::Released)
            .is_none());
        assert!(processor
            .process_key(&Key::Character("w".into()), ElementState::Released)
            .is_none());
    }

    #[test]
    fn test_release_sends_no_terminal_input() {
        let processor = EventProcessor::new();
        assert!(processor
            .process_key(&Key::Character("a".into()), ElementState::Released)
            .is_none());
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

    /// Write `text` into `row`, starting at column 0.
    fn fill_row(grid: &mut Grid, row: usize, text: &str) {
        for (col, c) in text.chars().enumerate() {
            grid.set(
                row,
                col,
                terminal_core::cell::Cell {
                    c,
                    style: terminal_core::cell::CellStyle::default(),
                    width: 1,
                },
            );
        }
    }

    #[test]
    fn test_extract_text_without_selection() {
        let grid = Grid::new(3, 10);
        assert_eq!(SelectionState::default().extract_text(&grid), None);
    }

    #[test]
    fn test_extract_text_single_row() {
        let mut grid = Grid::new(3, 10);
        fill_row(&mut grid, 0, "hello");

        let state = SelectionState {
            start: Some((0, 0)),
            current: Some((0, 4)),
        };
        assert_eq!(state.extract_text(&grid).as_deref(), Some("hello"));
    }

    #[test]
    fn test_extract_text_trims_trailing_blanks() {
        let mut grid = Grid::new(3, 10);
        fill_row(&mut grid, 0, "hello");

        // Selecting past the end of the line must not copy the blank rest.
        let state = SelectionState {
            start: Some((0, 0)),
            current: Some((0, 9)),
        };
        assert_eq!(state.extract_text(&grid).as_deref(), Some("hello"));
    }

    #[test]
    fn test_extract_text_multiline_is_linear() {
        let mut grid = Grid::new(3, 10);
        fill_row(&mut grid, 0, "abcdef");
        fill_row(&mut grid, 1, "ghijkl");
        fill_row(&mut grid, 2, "mnopqr");

        // From column 2 of the first row to column 1 of the last one: the
        // middle row is taken whole.
        let state = SelectionState {
            start: Some((0, 2)),
            current: Some((2, 1)),
        };
        assert_eq!(
            state.extract_text(&grid).as_deref(),
            Some("cdef\nghijkl\nmn")
        );
    }

    #[test]
    fn test_extract_text_reversed_selection_matches() {
        let mut grid = Grid::new(3, 10);
        fill_row(&mut grid, 0, "abcdef");
        fill_row(&mut grid, 1, "ghijkl");
        fill_row(&mut grid, 2, "mnopqr");

        let state = SelectionState {
            start: Some((2, 1)),
            current: Some((0, 2)),
        };
        assert_eq!(
            state.extract_text(&grid).as_deref(),
            Some("cdef\nghijkl\nmn")
        );
    }

    #[test]
    fn test_extract_text_clamps_to_grid() {
        let mut grid = Grid::new(2, 4);
        fill_row(&mut grid, 0, "ab");

        let state = SelectionState {
            start: Some((0, 0)),
            current: Some((9, 9)),
        };
        // Rows past the grid are ignored instead of panicking; the blank second
        // row is part of the selection, so it becomes a trailing newline.
        assert_eq!(state.extract_text(&grid).as_deref(), Some("ab\n"));
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
            24,
            80,
        );
        assert!(event.selection_active);
        assert!(processor.is_dragging());
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
            24,
            80,
        );
        // Drag to new position
        processor.update_selection((50.0, 80.0), 10.0, 20.0, 24, 80);
        assert!(processor.selection().is_active());
        assert_eq!(processor.selection().bounds(), Some((1, 1, 4, 5)));
    }

    #[test]
    fn test_drag_selection_survives_release() {
        let mut processor = EventProcessor::new();
        processor.process_mouse_event(
            MouseButton::Left,
            ElementState::Pressed,
            (10.0, 20.0),
            10.0,
            20.0,
            24,
            80,
        );
        processor.update_selection((50.0, 80.0), 10.0, 20.0, 24, 80);
        let event = processor.process_mouse_event(
            MouseButton::Left,
            ElementState::Released,
            (50.0, 80.0),
            10.0,
            20.0,
            24,
            80,
        );
        assert!(event.selection_active);
        assert!(!processor.is_dragging());
        assert_eq!(processor.selection().bounds(), Some((1, 1, 4, 5)));
    }

    #[test]
    fn test_click_clears_selection() {
        // A click without dragging must not leave a stuck highlight.
        let mut processor = EventProcessor::new();
        processor.process_mouse_event(
            MouseButton::Left,
            ElementState::Pressed,
            (10.0, 20.0),
            10.0,
            20.0,
            24,
            80,
        );
        let event = processor.process_mouse_event(
            MouseButton::Left,
            ElementState::Released,
            (10.0, 20.0),
            10.0,
            20.0,
            24,
            80,
        );
        assert!(!event.selection_active);
        assert!(!processor.selection().is_active());
        assert!(!processor.is_dragging());
    }

    #[test]
    fn test_motion_without_drag_does_not_select() {
        let mut processor = EventProcessor::new();
        processor.update_selection((100.0, 200.0), 10.0, 20.0, 24, 80);
        assert!(!processor.selection().is_active());
    }

    #[test]
    fn test_selection_is_clamped_to_grid() {
        let mut processor = EventProcessor::new();
        // Press at the origin of a 2x3 grid and drag far past its end.
        processor.process_mouse_event(
            MouseButton::Left,
            ElementState::Pressed,
            (0.0, 0.0),
            10.0,
            20.0,
            2,
            3,
        );
        processor.update_selection((9999.0, 9999.0), 10.0, 20.0, 2, 3);
        assert_eq!(processor.selection().bounds(), Some((0, 0, 1, 2)));
    }

    #[test]
    fn test_wheel_scrolls_scrollback_on_main_screen() {
        // RF-53: without the alternate screen the wheel moves the view.
        assert_eq!(wheel_action(false, 3), WheelAction::Scroll(3));
        assert_eq!(wheel_action(false, -2), WheelAction::Scroll(-2));
        assert_eq!(wheel_action(false, 0), WheelAction::Scroll(0));
    }

    #[test]
    fn test_wheel_sends_arrows_on_alternate_screen() {
        // RF-56 / CL-16: `less`/`glow -p` live on the alternate screen and
        // expect cursor keys, so the wheel must not touch the scrollback.
        assert_eq!(
            wheel_action(true, 2),
            WheelAction::Arrows {
                direction: ArrowDirection::Up,
                count: 2
            }
        );
        assert_eq!(
            wheel_action(true, -1),
            WheelAction::Arrows {
                direction: ArrowDirection::Down,
                count: 1
            }
        );
        assert_eq!(wheel_action(true, 0), WheelAction::Scroll(0));
    }
}
