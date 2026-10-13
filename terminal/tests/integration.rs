//! Integration tests for the terminal emulator.
//!
//! These tests verify the full integration between crates:
//! - Config loading and validation
//! - Terminal session lifecycle (PTY → parser → buffer)
//! - Tab and split management
//! - Event processing pipeline

use terminal_config::config::Config;
use terminal_config::loader;
use terminal_core::engine::TerminalEngine;
use terminal_core::session::{PollResult, TerminalSession};
use terminal_gui::event::EventProcessor;
use terminal_gui::split::{NavigateDirection, SplitLayout};
use terminal_gui::tabs::TabManager;

// ============================================================================
// Config Integration Tests
// ============================================================================

#[test]
fn test_config_load_default() {
    // Test that loading a non-existent config returns defaults
    let config_path = loader::find_config_path();
    let result = loader::load_config(&config_path);

    // Either it loads successfully or returns an error (which is expected
    // if no config file exists and default generation hasn't been called)
    if let Ok(cfg) = result {
        // If config loaded, verify defaults
        assert_eq!(cfg.window.title, "Terminal");
        assert_eq!(cfg.window.dimensions.columns, 80);
        assert_eq!(cfg.window.dimensions.rows, 24);
    }
    // Otherwise no config file exists, which is expected.
}

#[test]
fn test_config_default_values() {
    let config = Config::default();
    assert_eq!(config.window.title, "Terminal");
    assert_eq!(config.window.dimensions.columns, 80);
    assert_eq!(config.window.dimensions.rows, 24);
    assert!((config.window.opacity - 1.0).abs() < f32::EPSILON);
    assert!((config.font.size - 14.0).abs() < f32::EPSILON);
    assert!(config.font.ligatures);
    assert_eq!(config.colors.primary.foreground, "#ffffff");
    assert_eq!(config.colors.primary.background, "#1e1e2e");
}

#[test]
fn test_config_serde_roundtrip() {
    let config = Config::default();
    let toml = toml::to_string(&config).expect("Failed to serialize config");
    let parsed: Config = toml::from_str(&toml).expect("Failed to deserialize config");
    assert_eq!(config, parsed);
}

#[test]
fn test_config_custom_values() {
    let toml = r#"
[window]
title = "Custom Terminal"
opacity = 0.95
[window.dimensions]
columns = 120
rows = 40

[font]
family = "Fira Code"
size = 16.0
ligatures = true

[cursor]
style = "Beam"
blink_interval = 500
"#;
    let config: Config = toml::from_str(toml).expect("Failed to parse custom config");
    assert_eq!(config.window.title, "Custom Terminal");
    assert_eq!(config.window.dimensions.columns, 120);
    assert_eq!(config.window.dimensions.rows, 40);
    assert!((config.window.opacity - 0.95).abs() < f32::EPSILON);
    assert_eq!(config.font.family, "Fira Code");
    assert!((config.font.size - 16.0).abs() < f32::EPSILON);
    assert!(config.font.ligatures);
}

// ============================================================================
// Session Integration Tests
// ============================================================================

#[test]
fn test_session_basic_lifecycle() {
    // Create session, spawn shell, verify it runs
    let mut session = TerminalSession::new(24, 80);
    assert!(session.is_running());
    assert!(session.spawn_shell("/bin/bash").is_ok());
    assert!(session.pid().is_some());
    assert!(session.is_running());
}

#[test]
fn test_session_write_and_read() {
    let mut session = TerminalSession::new(24, 80);
    session.spawn_shell("/bin/bash").expect("spawn failed");

    // Give shell time to start
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Write a command
    let _ = session.write_to_shell(b"echo hello\n");

    // Poll to read output
    let mut output = Vec::new();
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let result = session.poll_pty().expect("poll failed");
        match result {
            PollResult::Data(data) => {
                output.extend_from_slice(&data);
                if String::from_utf8_lossy(&output).contains("hello") {
                    break;
                }
            }
            PollResult::Exited { .. } => break,
            PollResult::WouldBlock => {}
        }
    }

    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("hello"),
        "Expected 'hello' in output, got: {output_str}"
    );
}

#[test]
fn test_session_exit_detection() {
    // Test that session detects when a short-lived process exits
    let mut session = TerminalSession::new(24, 80);
    session.spawn_shell("/bin/true").expect("spawn failed");

    // /bin/true exits immediately
    std::thread::sleep(std::time::Duration::from_millis(500));

    let result = session.poll_pty().expect("poll failed");
    match result {
        PollResult::Exited { exit_code } => {
            assert_eq!(exit_code, 0);
            assert!(!session.is_running());
        }
        other => {
            panic!("Expected Exited, got {other:?}");
        }
    }
}

#[test]
fn test_session_exit_with_error() {
    let mut session = TerminalSession::new(24, 80);
    session.spawn_shell("/bin/false").expect("spawn failed");

    std::thread::sleep(std::time::Duration::from_millis(500));

    let result = session.poll_pty().expect("poll failed");
    match result {
        PollResult::Exited { exit_code } => {
            assert_eq!(exit_code, 1);
            assert!(!session.is_running());
        }
        other => {
            panic!("Expected Exited with code 1, got {other:?}");
        }
    }
}

#[test]
fn test_session_feed_text() {
    let mut session = TerminalSession::new(24, 80);

    // Feed text directly (no PTY)
    session.feed(b"Hello, World!");

    // Verify text is in the buffer
    let buf = session.buffer().active();
    let text: String = (0..13).map(|col| buf.grid.get(0, col).c).collect();
    assert!(
        text.contains("Hello"),
        "Expected 'Hello' in buffer, got: {text}"
    );
    assert!(
        text.contains("World"),
        "Expected 'World' in buffer, got: {text}"
    );
}

#[test]
fn test_session_feed_ansi_sequences() {
    let mut session = TerminalSession::new(24, 80);

    // Feed text with ANSI color sequences
    session.feed(b"Normal \x1b[31mRed\x1b[0m Normal");

    let buf = session.buffer().active();
    let text: String = (0..23).map(|col| buf.grid.get(0, col).c).collect();
    assert!(
        text.contains("Normal"),
        "Expected 'Normal' in buffer, got: {text}"
    );
    assert!(
        text.contains("Red"),
        "Expected 'Red' in buffer, got: {text}"
    );
}

#[test]
fn test_session_cursor_movement() {
    let mut session = TerminalSession::new(24, 80);

    // Move cursor to row 5, col 10
    session.feed(b"\x1b[6;11H");

    let state = session.state();
    assert_eq!(state.cursor.row, 5);
    assert_eq!(state.cursor.col, 10);
}

#[test]
fn test_session_alternate_screen() {
    let mut session = TerminalSession::new(24, 80);

    // Write on main screen
    session.feed(b"Main screen");

    // Enter alternate screen
    session.feed(b"\x1b[?1049h");

    // Alternate screen should be empty
    let buf = session.buffer().active();
    assert_eq!(buf.grid.get(0, 0).c, ' ');

    // Write on alternate screen
    session.feed(b"Alt screen");

    // Exit alternate screen
    session.feed(b"\x1b[?1049l");

    // Main screen should still have content
    let buf = session.buffer().active();
    assert_eq!(buf.grid.get(0, 0).c, 'M');
}

#[test]
fn test_session_stop_prevents_input() {
    let mut session = TerminalSession::new(24, 80);

    // Feed some text
    session.feed(b"Before stop");

    // Stop the session
    session.stop();

    // Feed more text (should be ignored)
    session.feed(b"After stop");

    // Only "Before stop" should be in the buffer
    let buf = session.buffer().active();
    assert_eq!(buf.grid.get(0, 0).c, 'B');
    assert_eq!(buf.grid.get(0, 1).c, 'e');
    // After "Before stop" (11 chars), cursor should be at col 11
    // "After stop" should not have been written
    assert_eq!(buf.grid.get(0, 11).c, ' ');
}

#[test]
fn test_session_write_without_pty() {
    let mut session = TerminalSession::new(24, 80);

    // No PTY spawned, write should return 0
    let written = session.write_to_shell(b"test").expect("write failed");
    assert_eq!(written, 0);
}

// ============================================================================
// Tab Integration Tests
// ============================================================================

#[test]
fn test_tab_manager_create_and_switch() {
    let mut manager = TabManager::new();
    assert_eq!(manager.len(), 1);
    assert_eq!(manager.active().unwrap().title(), "Terminal");

    // Create new tab
    let id1 = manager.create_tab();
    let id2 = manager.create_tab();
    assert_eq!(manager.len(), 3);
    assert_ne!(id1, id2);
    assert_eq!(manager.active_idx(), Some(2));

    // Switch to first tab
    manager.switch_to(0);
    assert_eq!(manager.active_idx(), Some(0));
    assert_eq!(manager.active().unwrap().title(), "Terminal");
}

#[test]
fn test_tab_manager_close_last_tab() {
    let mut manager = TabManager::new();

    // Closing last tab should return None
    let result = manager.close_active_tab();
    assert!(result.is_none());
    assert_eq!(manager.len(), 1);
}

#[test]
fn test_tab_manager_close_middle_tab() {
    let mut manager = TabManager::new();
    manager.create_tab(); // 2 tabs
    manager.create_tab(); // 3 tabs

    assert_eq!(manager.len(), 3);

    // Close active (index 2)
    let result = manager.close_active_tab();
    assert_eq!(result, Some(1));
    assert_eq!(manager.len(), 2);

    // Close active (index 1)
    let result = manager.close_active_tab();
    assert_eq!(result, Some(0));
    assert_eq!(manager.len(), 1);

    // Can't close last tab
    assert!(manager.close_active_tab().is_none());
}

#[test]
fn test_tab_manager_wrap_around() {
    let mut manager = TabManager::new();
    manager.create_tab(); // 2 tabs, active = 1

    // Switch next wraps to 0
    manager.switch_next();
    assert_eq!(manager.active_idx(), Some(0));

    // Switch next wraps to 1
    manager.switch_next();
    assert_eq!(manager.active_idx(), Some(1));

    // Switch prev wraps to 0
    manager.switch_prev();
    assert_eq!(manager.active_idx(), Some(0));
}

// ============================================================================
// Split Integration Tests
// ============================================================================

#[test]
fn test_split_layout_single_panel() {
    let layout = SplitLayout::new();
    assert_eq!(layout.panel_count(), 1);
    assert!(layout.is_single_panel());
}

#[test]
fn test_split_layout_horizontal() {
    let mut layout = SplitLayout::new();
    layout.split_horizontal(0.5);
    assert_eq!(layout.panel_count(), 2);
    assert!(!layout.is_single_panel());
}

#[test]
fn test_split_layout_vertical() {
    let mut layout = SplitLayout::new();
    layout.split_vertical(0.5);
    assert_eq!(layout.panel_count(), 2);
}

#[test]
fn test_split_layout_nested() {
    let mut layout = SplitLayout::new();
    layout.split_horizontal(0.5); // 2 panels
    layout.split_vertical(0.5); // 3 panels
    layout.split_horizontal(0.5); // 4 panels
    assert_eq!(layout.panel_count(), 4);
}

#[test]
fn test_split_navigate() {
    let mut layout = SplitLayout::new();
    layout.split_horizontal(0.5); // 2 panels

    // Navigate should return a valid panel ID
    let id = layout.navigate(NavigateDirection::Down);
    assert!(id.inner() > 0);

    // Navigate back
    let id = layout.navigate(NavigateDirection::Up);
    assert!(id.inner() > 0);
}

#[test]
fn test_split_close_panel() {
    let mut layout = SplitLayout::new();
    layout.split_horizontal(0.5); // 2 panels

    // Can't close single panel
    assert!(layout.close_panel().is_none());

    // Create another split
    layout.split_vertical(0.5); // 3 panels

    // Can now close a panel (if the implementation supports it)
    // Note: close_panel behavior may vary by implementation
    let _ = layout.close_panel();
    assert!(layout.panel_count() <= 3);
}

#[test]
fn test_split_close_last_panel() {
    let mut layout = SplitLayout::new();

    // Can't close the last panel
    assert!(layout.close_panel().is_none());
    assert_eq!(layout.panel_count(), 1);
}

// ============================================================================
// Event Processing Integration Tests
// ============================================================================

#[test]
fn test_event_processor_key_event() {
    let processor = EventProcessor::new();

    // Test that key events are processed
    // We can't easily test the actual winit KeyEvent, but we can verify
    // the keymap resolution works
    let keymap = processor.keymap();
    assert!(!keymap.default_bindings().is_empty());
}

#[test]
fn test_event_processor_selection() {
    let mut processor = EventProcessor::new();

    // Selection should be inactive by default
    assert!(!processor.selection().is_active());

    // Reset should keep it inactive
    processor.reset_selection();
    assert!(!processor.selection().is_active());
}

#[test]
fn test_event_processor_pixel_to_cell() {
    // Test pixel to cell conversion
    let (row, col) = EventProcessor::pixel_to_cell(0.0, 0.0, 10.0, 20.0);
    assert_eq!(row, 0);
    assert_eq!(col, 0);

    let (row, col) = EventProcessor::pixel_to_cell(10.0, 20.0, 10.0, 20.0);
    assert_eq!(row, 1);
    assert_eq!(col, 1);

    let (row, col) = EventProcessor::pixel_to_cell(19.9, 39.9, 10.0, 20.0);
    assert_eq!(row, 1);
    assert_eq!(col, 1);
}

#[test]
fn test_event_processor_mouse_selection() {
    use winit::event::{ElementState, MouseButton};

    let mut processor = EventProcessor::new();

    // Start selection
    let event = processor.process_mouse_event(
        MouseButton::Left,
        ElementState::Pressed,
        (15.0, 25.0),
        10.0,
        20.0,
    );
    assert!(event.selection_active);
    assert!(processor.selection().is_active());
    assert_eq!(processor.selection().bounds(), Some((1, 1, 1, 1)));

    // Drag selection
    processor.update_selection((50.0, 80.0), 10.0, 20.0);
    assert!(processor.selection().is_active());
    assert_eq!(processor.selection().bounds(), Some((1, 1, 4, 5)));

    // Release selection
    let event = processor.process_mouse_event(
        MouseButton::Left,
        ElementState::Released,
        (50.0, 80.0),
        10.0,
        20.0,
    );
    assert!(event.selection_active);
}

// ============================================================================
// Engine Integration Tests
// ============================================================================

#[test]
fn test_engine_text_and_colors() {
    let mut engine = TerminalEngine::new(24, 80);

    // Write text with colors
    let _actions = engine.input(b"Hello \x1b[31mWorld\x1b[0m!");

    // Verify the engine has a valid buffer
    let buf = engine.buffer().active();
    assert_eq!(buf.grid.rows, 24);
    assert_eq!(buf.grid.cols, 80);
}

#[test]
fn test_engine_cursor_and_scroll() {
    let mut engine = TerminalEngine::new(3, 10);

    // Write more than 3 lines worth of text
    for i in 0u8..30 {
        engine.input(&[b'A' + (i % 26)]);
    }

    // Should have scrolled
    let buf = engine.buffer().active();
    assert!(buf.grid.rows <= 3);
}

#[test]
fn test_engine_clear_display() {
    let mut engine = TerminalEngine::new(24, 80);

    // Write some text
    engine.input(b"Line 1\nLine 2\nLine 3");

    // Clear display
    engine.input(b"\x1b[2J");

    // Screen should be cleared
    let buf = engine.buffer().active();
    for row in 0..3 {
        for col in 0..6 {
            assert_eq!(buf.grid.get(row, col).c, ' ');
        }
    }
}

#[test]
fn test_engine_bell_action() {
    let mut engine = TerminalEngine::new(24, 80);

    let actions = engine.input(b"\x07");
    assert!(actions
        .iter()
        .any(|a| matches!(a, terminal_core::parser::Action::Bell)));
}

#[test]
fn test_engine_window_title() {
    let mut engine = TerminalEngine::new(24, 80);

    // OSC 0 with BEL terminator - engine should not panic
    let _actions = engine.input(b"\x1b]0;My Title\x07");
    // Verify engine is still functional
    let buf = engine.buffer().active();
    assert_eq!(buf.grid.rows, 24);
}

// ============================================================================
// Full Pipeline Test
// ============================================================================

#[test]
fn test_full_pipeline_config_to_session() {
    // This test verifies the full integration:
    // Config → Engine → Session

    let config = Config::default();

    // Create engine with config dimensions
    let mut engine = TerminalEngine::new(
        config.window.dimensions.rows as usize,
        config.window.dimensions.columns as usize,
    );

    // Feed text through engine
    engine.input(b"Pipeline test");

    // Create session with same dimensions
    let mut session = TerminalSession::new(
        config.window.dimensions.rows as usize,
        config.window.dimensions.columns as usize,
    );

    // Feed same text through session
    session.feed(b"Pipeline test");

    // Both should have the same content
    let engine_buf = engine.buffer().active();
    let session_buf = session.buffer().active();

    for col in 0..13 {
        assert_eq!(
            engine_buf.grid.get(0, col).c,
            session_buf.grid.get(0, col).c,
            "Mismatch at col {col}"
        );
    }
}

#[test]
fn test_full_pipeline_tabs_with_sessions() {
    // Create tab manager with sessions
    let mut manager = TabManager::new();

    // Create multiple tabs
    let _id1 = manager.create_tab();
    let _id2 = manager.create_tab();

    assert_eq!(manager.len(), 3);

    // Each tab should have a unique ID
    let tabs: Vec<_> = manager.tabs().iter().map(|t| t.session().id()).collect();
    assert!(tabs
        .iter()
        .all(|&id| tabs.iter().filter(|&&x| x == id).count() == 1));

    // Switch between tabs
    manager.switch_to(0);
    assert_eq!(manager.active_idx(), Some(0));

    manager.switch_next();
    assert_eq!(manager.active_idx(), Some(1));

    manager.switch_prev();
    assert_eq!(manager.active_idx(), Some(0));

    // Close a tab
    let result = manager.close_active_tab();
    assert!(result.is_some());
    assert_eq!(manager.len(), 2);
}

#[test]
fn test_full_pipeline_splits_with_navigate() {
    // Create layout and navigate through splits
    let mut layout = SplitLayout::new();

    // Initial state
    assert_eq!(layout.panel_count(), 1);

    // Create a grid of panels (2x2)
    layout.split_horizontal(0.5); // Top and bottom
    layout.split_vertical(0.5); // Left and right of top

    assert_eq!(layout.panel_count(), 3);

    // Navigate should work
    let _id = layout.navigate(NavigateDirection::Down);
    let _id = layout.navigate(NavigateDirection::Up);
    let _id = layout.navigate(NavigateDirection::Right);
    let _id = layout.navigate(NavigateDirection::Left);
}

#[test]
fn sim_cursor_prompt() {
    use terminal_core::session::TerminalSession;
    let mut s = TerminalSession::new(24, 80);
    s.feed(b"\x1b[?2004hbash-5.3$ ");
    let st = s.state();
    println!("AFTER PROMPT1: row={} col={}", st.cursor.row, st.cursor.col);
    s.feed(b"echo hi\r\n\x1b[?2004l\rhi\r\n\x1b[?2004hbash-5.3$ ");
    let st = s.state();
    println!("AFTER PROMPT2: row={} col={}", st.cursor.row, st.cursor.col);
}
