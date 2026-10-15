//! Terminal emulator — main application.
//!
//! This module integrates all crates into a single application:
//! - Config loading and CLI parsing
//! - Terminal core engine + session (PTY)
//! - GPU rendering pipeline
//! - GUI with tabs, splits, keymap, clipboard
//! - Full event loop connecting everything

#![warn(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

// Brings `WindowAttributes::with_name` into scope on Linux, which sets the
// Wayland `app_id` / X11 `WM_CLASS` so the desktop shell can name and icon the
// window (it matches them against `terminal.desktop`).
#[cfg(target_os = "linux")]
use winit::platform::wayland::WindowAttributesExtWayland as _;

use terminal_config::cli::{merge_cli, parse as parse_cli};
use terminal_config::config::{ColorScheme, Config};
use terminal_config::loader;
use terminal_config::shell::ShellCommand;

use terminal_core::engine::TerminalEngine;
use terminal_core::input::KeyEvent as TerminalKeyEvent;
use terminal_core::parser::Action;
use terminal_core::session::{PollResult, TerminalSession};
use terminal_core::state::CursorShape as EngineCursorShape;

use terminal_render::color::{pack_rgba, unpack_rgba, Color as RenderColor, TerminalPalette};
use terminal_render::font::FontConfig as RenderFontConfig;
use terminal_render::renderer::{
    CursorShape, GridPane, PaneRect, RenderContent, Selection, TabBarContent, TabLabel,
    TerminalRenderer,
};

use terminal_gui::clipboard::Clipboard;
use terminal_gui::event::{wheel_action, EventProcessor, EventResult, WheelAction};
use terminal_gui::keymap::GuiAction;
use terminal_gui::split::{
    NavigateDirection, PanelId, Rect as SplitRect, SplitDirection, SplitLayout,
};
use terminal_gui::tab_bar::TabBar;
use terminal_gui::tabs::{Tab, TabId, TabManager};
use terminal_gui::window::{GpuContext, GpuContextBase};

/// Pixel gap left between split panes.
const SPLIT_GAP: u32 = 2;

/// Application id / `WM_CLASS`, matched against `terminal.desktop` by the
/// desktop shell so the window gets a name and icon (RF-60).
#[cfg(target_os = "linux")]
const APP_ID: &str = "terminal";
#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
const APP_ID: &str = "terminal";

/// Maximum time a synchronized-output window may hold the frame before it is
/// presented anyway, so a program that never sends `ESC [ ? 2026 l` cannot
/// freeze the window (RF-47, CL-12).
const SYNC_TIMEOUT_MS: u64 = 100;

/// Build the bytes that answer the terminal queries a program made (RF-51).
///
/// `cursor` is the 0-indexed position; DSR reports it 1-indexed. Colour queries
/// answer with the configured theme colour as `#rrggbb`.
fn query_responses(actions: &[Action], cursor: (usize, usize), fg: &str, bg: &str) -> Vec<u8> {
    use std::fmt::Write as _;

    let mut out = String::new();
    for action in actions {
        match action {
            Action::QueryCursorPosition => {
                let _ = write!(out, "\x1b[{};{}R", cursor.0 + 1, cursor.1 + 1);
            }
            Action::QueryDeviceAttributes => {
                // DA1: VT220 with ANSI colour.
                out.push_str("\x1b[?62;22c");
            }
            Action::QueryForegroundColor => {
                let _ = write!(out, "\x1b]10;{fg}\x07");
            }
            Action::QueryBackgroundColor => {
                let _ = write!(out, "\x1b]11;{bg}\x07");
            }
            Action::QueryTextColor => {
                let _ = write!(out, "\x1b]12;{fg}\x07");
            }
            _ => {}
        }
    }
    out.into_bytes()
}

/// Shared application state for the event loop.
pub struct AppState {
    /// The winit window.
    pub window: Arc<winit::window::Window>,
    /// GPU context.
    pub gpu: Arc<GpuContext>,
    /// Surface configuration (separate mutex to avoid borrow conflicts).
    pub surface_config: std::sync::Mutex<wgpu::SurfaceConfiguration>,
}

/// One pane: its shell session plus the shell that is actually running in it.
struct PaneState {
    /// The session driving the pane.
    session: TerminalSession,
    /// Shell really running in the pane: the requested one, or the system
    /// shell after a failed spawn (RF-67, CL-31).
    command: ShellCommand,
}

/// Terminal state for one tab: a split layout plus a session per pane.
struct TabState {
    /// Split tree of panes.
    layout: SplitLayout,
    /// One shell session per pane.
    panes: HashMap<PanelId, PaneState>,
}

impl TabState {
    /// Create a tab with a single pane and its shell session.
    fn new(
        rows: usize,
        cols: usize,
        cursor_style: EngineCursorShape,
        command: &ShellCommand,
    ) -> Self {
        let layout = SplitLayout::new();
        let mut panes = HashMap::new();
        panes.insert(
            layout.focused_panel(),
            AppInner::spawn_session(rows, cols, cursor_style, command),
        );
        Self { layout, panes }
    }

    /// Shell for the pane created by splitting the focused one (RF-70).
    ///
    /// It is the shell that is really running in the pane being split, so the
    /// chosen profile is kept instead of going back to the default shell. The
    /// `default` is only used when the focused pane has no session.
    fn split_command(&self, default: &ShellCommand) -> ShellCommand {
        self.panes
            .get(&self.layout.focused_panel())
            .map_or_else(|| default.clone(), |pane| pane.command.clone())
    }
}

/// Per-pane data gathered for one frame of rendering.
struct PaneSpec {
    /// Pixel rectangle of the pane.
    rect: SplitRect,
    /// Cursor shape.
    cursor_shape: CursorShape,
    /// Cursor row within the pane.
    cursor_row: usize,
    /// Cursor column within the pane.
    cursor_col: usize,
    /// Whether to draw the cursor.
    cursor_visible: bool,
    /// Number of columns in the pane.
    cols: usize,
    /// Selection to highlight, if this pane owns it.
    selection: Selection,
}

/// Application state shared across the event loop.
struct AppInner {
    /// Shared app state (window, gpu, `surface_config`).
    #[allow(dead_code)]
    app_state: Option<Arc<std::sync::Mutex<AppState>>>,
    /// Loaded configuration.
    config: Config,
    /// WGPU instance (created before event loop).
    #[allow(dead_code)]
    wgpu_instance: std::sync::Arc<wgpu::Instance>,
    /// GPU base context (device/queue, created before event loop).
    gpu_base: GpuContextBase,
    /// Tab manager with all tabs.
    tab_manager: TabManager,
    /// Clipboard for copy/paste.
    clipboard: Clipboard,
    /// Event processor for keyboard/mouse.
    event_processor: EventProcessor,
    /// Per-tab terminal state (split layout + one session per pane).
    tabs: HashMap<TabId, TabState>,
    /// Pane that owns the current mouse selection, if any.
    selection_pane: Option<PanelId>,
    /// Terminal renderer.
    #[allow(dead_code)]
    renderer: Option<TerminalRenderer>,
    /// Tab bar UI.
    tab_bar: TabBar,
    /// Current grid size in rows and columns.
    grid_rows: usize,
    /// Current grid size in columns.
    grid_cols: usize,
    /// Window title.
    window_title: String,
    /// Configured cursor shape applied to new sessions.
    cursor_style: EngineCursorShape,
    /// Shell used by new sessions and splits (RF-62).
    default_shell: ShellCommand,
    /// Shells pickable at runtime (RF-64, RF-66).
    shell_profiles: Vec<ShellCommand>,
    /// Cursor blink interval in milliseconds (0 = steady cursor).
    cursor_blink_interval: u64,
    /// Window opacity in `0.0..=1.0`.
    window_opacity: f32,
    /// Cursor visibility toggle (for blink).
    cursor_visible: bool,
    /// Cursor blink timer (ms).
    cursor_blink_timer: u64,
    /// Whether to request redraw next frame.
    needs_redraw: bool,
    /// Terminal palette for color conversion.
    #[allow(dead_code)]
    palette: TerminalPalette,
    /// Last known cursor position (for mouse button events).
    last_mouse_position: (f64, f64),
    /// Deadline for the current synchronized-output window (RF-47, CL-12).
    sync_deadline: Option<std::time::Instant>,
}

impl AppInner {
    /// Create a new application state.
    fn new(
        config: &Config,
        wgpu_instance: std::sync::Arc<wgpu::Instance>,
        gpu_base: GpuContextBase,
    ) -> Self {
        let tab_manager = TabManager::new();

        let rows = config.window.dimensions.rows as usize;
        let cols = config.window.dimensions.columns as usize;

        // One pane session per tab; spawn the shell for the initial tab.
        let cursor_style = cursor_shape_from_config(config.cursor.style);
        let default_shell = config.default_shell_command();
        let shell_profiles = config.shell_commands();
        let mut tabs = HashMap::new();
        if let Some(tab) = tab_manager.active() {
            tabs.insert(
                tab.id(),
                TabState::new(rows, cols, cursor_style, &default_shell),
            );
        }

        Self {
            config: config.clone(),
            wgpu_instance,
            gpu_base,
            tab_manager,
            clipboard: Clipboard::new(),
            event_processor: EventProcessor::new(),
            tabs,
            selection_pane: None,
            renderer: None,
            tab_bar: TabBar::new(),
            grid_rows: rows,
            grid_cols: cols,
            window_title: config.window.title.clone(),
            cursor_style,
            default_shell,
            shell_profiles,
            cursor_blink_interval: config.cursor.blink_interval,
            window_opacity: config.window.opacity.clamp(0.0, 1.0),
            cursor_visible: true,
            cursor_blink_timer: 0,
            needs_redraw: true,
            palette: TerminalPalette::default(),
            last_mouse_position: (0.0, 0.0),
            sync_deadline: None,
            app_state: None,
        }
    }

    /// Spawn a session sized for the given screen, running `command`.
    ///
    /// If the requested shell cannot be started the error is written into the
    /// pane and the shell of the system is started instead, so the tab is never
    /// left empty (RF-67, CL-24). The returned [`PaneState`] remembers the shell
    /// that really runs, which is what splits inherit (RF-70, CL-31).
    fn spawn_session(
        rows: usize,
        cols: usize,
        cursor_style: EngineCursorShape,
        command: &ShellCommand,
    ) -> PaneState {
        let mut session = TerminalSession::new(rows, cols);
        let mut running = command.clone();
        if let Err(error) = session.spawn_command(&command.program, &command.args) {
            let fallback = ShellCommand::platform_default();
            tracing::error!(
                "Could not start shell {:?}: {error}; falling back to {:?}",
                command.program,
                fallback.program
            );
            let notice = format!(
                "\r\n\x1b[31mterminal:\x1b[0m could not start shell \"{}\": {error}\r\n\
                 \x1b[31mterminal:\x1b[0m falling back to \"{}\"\r\n\r\n",
                command.program, fallback.program
            );
            session.feed(notice.as_bytes());
            let _ = session.spawn_command(&fallback.program, &fallback.args);
            running = fallback;
        }
        // Apply the configured cursor style; OSC 22 can still override it.
        session.engine_mut().state_mut().cursor.shape = cursor_style;
        PaneState {
            session,
            command: running,
        }
    }

    /// ID of the active tab, if any.
    fn active_tab_id(&self) -> Option<TabId> {
        self.tab_manager.active().map(Tab::id)
    }

    /// State of the active tab.
    fn active_tab_state(&self) -> Option<&TabState> {
        self.active_tab_id().and_then(|id| self.tabs.get(&id))
    }

    /// State of the active tab (mutable).
    fn active_tab_state_mut(&mut self) -> Option<&mut TabState> {
        let id = self.active_tab_id()?;
        self.tabs.get_mut(&id)
    }

    /// Get the active (focused) session.
    fn active_session(&self) -> Option<&TerminalSession> {
        let state = self.active_tab_state()?;
        state
            .panes
            .get(&state.layout.focused_panel())
            .map(|pane| &pane.session)
    }

    /// Get the active (focused) session (mutable).
    fn active_session_mut(&mut self) -> Option<&mut TerminalSession> {
        let state = self.active_tab_state_mut()?;
        let focused = state.layout.focused_panel();
        state.panes.get_mut(&focused).map(|pane| &mut pane.session)
    }

    /// Get the engine from the active session.
    fn engine(&self) -> Option<&TerminalEngine> {
        self.active_session().map(TerminalSession::engine)
    }

    /// Whether the focused program is inside a synchronized update (RF-47).
    fn synchronized_output_active(&self) -> bool {
        self.engine()
            .is_some_and(|engine| engine.state().modes.synchronized_output)
    }

    /// Terminal area in pixels (below the tab bar).
    fn terminal_area(&self) -> SplitRect {
        let (cell_w, cell_h) = self.cell_size();
        let bar_h = self.tab_bar.height();
        SplitRect {
            x: 0,
            y: bar_h,
            width: (cell_w * self.grid_cols as u32).max(1),
            height: (cell_h * self.grid_rows as u32).max(1),
        }
    }

    /// Resize every pane's session to fit its rectangle.
    fn resize_sessions(&mut self) {
        let (cell_w, cell_h) = self.cell_size();
        let (cell_w, cell_h) = (cell_w.max(1), cell_h.max(1));
        let area = self.terminal_area();

        for state in self.tabs.values_mut() {
            for (id, rect) in state.layout.rects(area, SPLIT_GAP) {
                if let Some(pane) = state.panes.get_mut(&id) {
                    let rows = (rect.height / cell_h).max(1) as usize;
                    let cols = (rect.width / cell_w).max(1) as usize;
                    pane.session.resize(rows, cols);
                }
            }
        }
    }

    /// Close the active tab and its panes.
    ///
    /// Returns `false` when no tabs remain (the app should exit).
    fn close_active_tab(&mut self) -> bool {
        let closing = self.active_tab_id();
        let Some(new_idx) = self.tab_manager.close_active_tab() else {
            return false;
        };
        if let Some(id) = closing {
            // Dropping the sessions closes their PTYs.
            self.tabs.remove(&id);
        }
        self.clear_selection();
        if let Some(tab) = self.tab_manager.get_tab(new_idx) {
            tracing::info!("Switched to tab: {}", tab.title());
        }
        true
    }

    /// Remove a pane and its session, closing the tab if it was the last pane.
    fn close_pane(&mut self, tab_id: TabId, panel: PanelId) {
        let last_pane = {
            let Some(state) = self.tabs.get_mut(&tab_id) else {
                return;
            };
            state.panes.remove(&panel);
            if state.panes.is_empty() {
                true
            } else {
                state.layout.remove_panel(panel);
                false
            }
        };

        if last_pane {
            self.tabs.remove(&tab_id);
            self.tab_manager.close_tab(tab_id);
            self.clear_selection();
        }
    }

    /// Drop the current selection.
    fn clear_selection(&mut self) {
        self.selection_pane = None;
        self.event_processor.reset_selection();
    }

    /// Split the focused pane, spawning a shell for the new one.
    ///
    /// The new pane inherits the shell that is running in the pane being split
    /// (RF-70), not the default shell.
    fn split_active(&mut self, direction: SplitDirection) {
        let (rows, cols) = (self.grid_rows, self.grid_cols);
        let cursor_style = self.cursor_style;
        let default = self.default_shell.clone();
        let Some(state) = self.active_tab_state_mut() else {
            return;
        };

        let command = state.split_command(&default);
        let panel = match direction {
            SplitDirection::Horizontal => state.layout.split_horizontal(0.5),
            SplitDirection::Vertical => state.layout.split_vertical(0.5),
        };
        state.panes.insert(
            panel,
            Self::spawn_session(rows, cols, cursor_style, &command),
        );
        tracing::info!("Split {direction:?}: new panel {panel:?}");

        self.resize_sessions();
        self.needs_redraw = true;
    }

    /// Focus the neighbouring pane in the given direction.
    fn navigate_panel(&mut self, direction: NavigateDirection) {
        if let Some(state) = self.active_tab_state_mut() {
            let panel = state.layout.navigate(direction);
            tracing::info!("Focused panel {panel:?}");
        }
        self.needs_redraw = true;
    }

    /// Convert a `terminal_core` Color to a render Color (u32).
    #[expect(dead_code, reason = "Used for future color conversion")]
    fn color_to_render(&self, color: terminal_core::cell::Color) -> RenderColor {
        let palette = self.palette.as_array();
        let rgba = color.to_rgba(&palette);
        // Convert from BGRA (terminal_core) to RGBA (render)
        // terminal_core uses: (r << 16) | (g << 8) | b
        // render uses: (a << 24) | (r << 16) | (g << 8) | b
        (0xFF << 24) | rgba
    }

    /// Process incoming PTY data.
    ///
    /// Returns the shell's exit code once the session has ended, so the caller
    /// can close the window.
    /// Poll every pane's session, closing panes and tabs whose shell exited.
    ///
    /// Returns `true` when no tabs remain (the app should exit).
    fn poll_sessions(&mut self) -> bool {
        let mut exited: Vec<(TabId, PanelId)> = Vec::new();
        let fg = self.config.colors.primary.foreground.clone();
        let bg = self.config.colors.primary.background.clone();

        for (tab_id, state) in &mut self.tabs {
            for (panel_id, pane) in &mut state.panes {
                let session = &mut pane.session;
                loop {
                    match session.poll_pty() {
                        Ok(PollResult::WouldBlock) => break,
                        Ok(PollResult::Data(_)) => {}
                        Ok(PollResult::Exited { exit_code }) => {
                            tracing::info!("Pane {panel_id:?} exited with code {exit_code}");
                            exited.push((*tab_id, *panel_id));
                            break;
                        }
                        Err(e) => {
                            tracing::error!("PTY error: {e}");
                            break;
                        }
                    }
                }

                // Answer any query the program sent (RF-51). The response is
                // written back to the PTY, which doubles as this terminal's
                // "stdin" for the child.
                let actions = session.take_gui_actions();
                if !actions.is_empty() {
                    let cursor = session.engine().state().cursor;
                    let response = query_responses(&actions, (cursor.row, cursor.col), &fg, &bg);
                    if !response.is_empty() {
                        let _ = session.write_to_shell(&response);
                    }
                }
            }
        }

        self.needs_redraw = true;

        let mut closed_any = false;
        for (tab_id, panel_id) in exited {
            closed_any = true;
            self.close_pane(tab_id, panel_id);
        }

        // Surviving panes must reclaim the space freed by the closed ones.
        if closed_any {
            self.resize_sessions();
        }

        self.tabs.is_empty()
    }

    /// Process a key event.
    fn process_key_event(&mut self, event: &winit::event::KeyEvent) {
        if let Some(result) = self.event_processor.process_key_event(event) {
            match result {
                EventResult::GuiAction(action) => {
                    self.handle_gui_action(action);
                }
                EventResult::TerminalInput(input) => {
                    // Send terminal input to the active session
                    if let Some(session) = self.active_session_mut() {
                        let _ = session.write_to_shell(&input.bytes);
                    }
                    self.needs_redraw = true;
                }
                EventResult::Mouse(_) => {
                    // Mouse events handled separately
                }
            }
        }
    }

    /// Cell size in pixels, taken from the renderer (fallback if not built yet).
    fn cell_size(&self) -> (u32, u32) {
        self.renderer
            .as_ref()
            .map_or((8, 16), TerminalRenderer::cell_size)
    }

    /// Grid dimensions (rows, cols) of the active session.
    fn grid_size(&self) -> (usize, usize) {
        self.engine().map_or((24, 80), TerminalEngine::dimensions)
    }

    /// Process a mouse button event.
    fn process_mouse_button(
        &mut self,
        button: winit::event::MouseButton,
        state: winit::event::ElementState,
        position: (f64, f64),
    ) {
        // Clicks on the tab bar switch tabs (or close them) instead of starting
        // a selection.
        let bar_h = f64::from(self.tab_bar.height());
        if position.1 < bar_h {
            if button == winit::event::MouseButton::Left
                && state == winit::event::ElementState::Pressed
                && !self.tab_bar_click(position.0)
            {
                tracing::info!("Last tab closed, exiting");
                std::process::exit(0);
            }
            return;
        }

        let Some((panel, rect)) = self.pane_at(position) else {
            return;
        };

        if button == winit::event::MouseButton::Left && state == winit::event::ElementState::Pressed
        {
            // Focus the clicked pane and start a fresh selection in it.
            if let Some(tab_state) = self.active_tab_state_mut() {
                tab_state.layout.focus_panel(panel);
            }
            self.selection_pane = Some(panel);
            self.event_processor.reset_selection();
        }

        let (cell_w, cell_h) = self.cell_size();
        let (rows, cols) = self.grid_size();
        let local = (
            position.0 - f64::from(rect.x),
            position.1 - f64::from(rect.y),
        );
        let mouse_event = self.event_processor.process_mouse_event(
            button,
            state,
            local,
            f64::from(cell_w),
            f64::from(cell_h),
            rows,
            cols,
        );
        if mouse_event.selection_active {
            self.needs_redraw = true;
        }
    }

    /// Find the pane (and its rectangle) under a pixel position.
    fn pane_at(&self, position: (f64, f64)) -> Option<(PanelId, SplitRect)> {
        let state = self.active_tab_state()?;
        let area = self.terminal_area();
        state
            .layout
            .rects(area, SPLIT_GAP)
            .into_iter()
            .find(|(_, rect)| {
                position.0 >= f64::from(rect.x)
                    && position.0 < f64::from(rect.x + rect.width)
                    && position.1 >= f64::from(rect.y)
                    && position.1 < f64::from(rect.y + rect.height)
            })
    }

    /// Handle a click on the tab bar (RF-59).
    ///
    /// The close button (`×`) closes the clicked tab; anywhere else switches to
    /// it. Returns `false` when closing the last tab should exit the app.
    #[expect(
        clippy::cast_sign_loss,
        reason = "Pixel coordinates are clamped to be non-negative"
    )]
    fn tab_bar_click(&mut self, x: f64) -> bool {
        let (cell_w, _cell_h) = self.cell_size();
        let titles: Vec<&str> = self.tab_manager.tabs().iter().map(Tab::title).collect();
        let max_width = cell_w * self.grid_size().1 as u32;
        let rects = self.tab_bar.layout(&titles, cell_w, max_width);

        let x = x.max(0.0) as u32;
        let Some(rect) = rects.iter().find(|r| r.contains(x)) else {
            return true;
        };

        if rect.close_hit(x) {
            self.needs_redraw = true;
            return self.close_tab_at(rect.index);
        }

        self.tab_manager.switch_to(rect.index);
        self.clear_selection();
        tracing::info!("Switched to tab {}", rect.index);
        self.needs_redraw = true;
        true
    }

    /// Close the tab at `idx`, its panes and sessions (RF-59).
    ///
    /// Returns `false` when no tabs remain (the app should exit, CL-7).
    fn close_tab_at(&mut self, idx: usize) -> bool {
        if idx == self.tab_manager.active_idx().unwrap_or(usize::MAX) {
            return self.close_active_tab();
        }
        let Some(id) = self.tab_manager.get_tab(idx).map(Tab::id) else {
            return true;
        };
        // Dropping the sessions closes their PTYs.
        self.tabs.remove(&id);
        self.tab_manager.close_tab(id);
        self.clear_selection();
        tracing::info!("Closed tab {idx}");
        true
    }

    /// Process a mouse cursor move (for drag selection).
    fn process_mouse_motion(&mut self, position: (f64, f64)) {
        self.last_mouse_position = position;
        if !self.event_processor.is_dragging() {
            return;
        }
        let Some(panel) = self.selection_pane else {
            return;
        };

        // Map the pointer into the pane that owns the selection.
        let rect = {
            let Some(state) = self.active_tab_state() else {
                return;
            };
            let area = self.terminal_area();
            state
                .layout
                .rects(area, SPLIT_GAP)
                .into_iter()
                .find(|(id, _)| *id == panel)
                .map(|(_, rect)| rect)
        };
        let Some(rect) = rect else {
            return;
        };

        let (cell_w, cell_h) = self.cell_size();
        let (rows, cols) = self.grid_size();
        self.event_processor.update_selection(
            (
                position.0 - f64::from(rect.x),
                position.1 - f64::from(rect.y),
            ),
            f64::from(cell_w),
            f64::from(cell_h),
            rows,
            cols,
        );
        self.needs_redraw = true;
    }

    /// Handle a GUI action from the keymap.
    fn handle_gui_action(&mut self, action: GuiAction) {
        match action {
            GuiAction::NewTab => {
                let tab_id = self.tab_manager.create_tab();
                tracing::info!("New tab created: {tab_id:?}");

                let (rows, cols) = (self.grid_rows, self.grid_cols);
                let command = self.default_shell.clone();
                self.tabs.insert(
                    tab_id,
                    TabState::new(rows, cols, self.cursor_style, &command),
                );
                self.clear_selection();
                self.needs_redraw = true;
            }
            GuiAction::NewTabWithProfile(index) => {
                let Some(command) = self.shell_profiles.get(index).cloned() else {
                    // CL-27: no profile behind the shortcut, so no tab.
                    tracing::warn!("No shell profile #{index}; shortcut ignored");
                    return;
                };

                let tab_id = self.tab_manager.create_tab();
                tracing::info!(
                    "New tab created: {tab_id:?} with profile {} ({})",
                    command.name,
                    command.program
                );

                let (rows, cols) = (self.grid_rows, self.grid_cols);
                self.tabs.insert(
                    tab_id,
                    TabState::new(rows, cols, self.cursor_style, &command),
                );
                self.clear_selection();
                self.needs_redraw = true;
            }
            GuiAction::CloseTab => {
                if !self.close_active_tab() {
                    tracing::info!("Last tab closed, exiting");
                    std::process::exit(0);
                }
                self.needs_redraw = true;
            }
            GuiAction::NextTab => {
                self.tab_manager.switch_next();
                self.clear_selection();
                if let Some(tab) = self.tab_manager.active() {
                    tracing::info!("Switched to tab: {}", tab.title());
                }
                self.needs_redraw = true;
            }
            GuiAction::PrevTab => {
                self.tab_manager.switch_prev();
                self.clear_selection();
                if let Some(tab) = self.tab_manager.active() {
                    tracing::info!("Switched to tab: {}", tab.title());
                }
                self.needs_redraw = true;
            }
            GuiAction::SplitHorizontal => self.split_active(SplitDirection::Horizontal),
            GuiAction::SplitVertical => self.split_active(SplitDirection::Vertical),
            GuiAction::ClosePanel => {
                let removed = self.active_tab_state().map(|s| s.layout.focused_panel());
                if let (Some(panel), Some(state)) = (removed, self.active_tab_state_mut()) {
                    if state.layout.close_panel().is_some() {
                        state.panes.remove(&panel);
                        tracing::info!("Closed panel {panel:?}");
                    } else {
                        tracing::info!("Cannot close the last panel");
                    }
                }
                self.resize_sessions();
                self.needs_redraw = true;
            }
            GuiAction::NavigateUp => self.navigate_panel(NavigateDirection::Up),
            GuiAction::NavigateDown => self.navigate_panel(NavigateDirection::Down),
            GuiAction::NavigateLeft => self.navigate_panel(NavigateDirection::Left),
            GuiAction::NavigateRight => self.navigate_panel(NavigateDirection::Right),
            GuiAction::Copy => self.copy_selection(),
            GuiAction::Paste => {
                // Paste clipboard content into the active session.
                let text = self.clipboard.get_text();
                self.paste_text(&text);
            }
            GuiAction::ScrollLineUp => self.scroll_view(1),
            GuiAction::ScrollLineDown => self.scroll_view(-1),
            GuiAction::ScrollPageUp => {
                let page = (self.grid_rows.saturating_sub(1)).max(1);
                self.scroll_view(i32::try_from(page).unwrap_or(i32::MAX));
            }
            GuiAction::ScrollPageDown => {
                let page = (self.grid_rows.saturating_sub(1)).max(1);
                self.scroll_view(-i32::try_from(page).unwrap_or(i32::MAX));
            }
            GuiAction::ScrollTop => self.scroll_view_to_edge(true),
            GuiAction::ScrollBottom => self.scroll_view_to_edge(false),
            GuiAction::None => {}
        }
    }

    /// Handle a mouse-wheel delta on the pane under the pointer (RF-53, RF-56).
    ///
    /// On the main screen it moves the scrollback view; on the alternate screen
    /// (where a pager such as `less`/`glow -p` runs) it forwards cursor keys to
    /// the program instead.
    fn wheel_scroll(&mut self, lines: i32) {
        if lines == 0 {
            return;
        }
        let Some(session) = self.scroll_target_session() else {
            return;
        };
        match wheel_action(session.buffer().is_alternate_screen_active(), lines) {
            WheelAction::Scroll(n) => {
                let buffer = session.engine_mut().buffer_mut();
                if n > 0 {
                    buffer.scroll_back(n.unsigned_abs() as usize);
                } else {
                    buffer.scroll_forward(n.unsigned_abs() as usize);
                }
            }
            WheelAction::Arrows { direction, count } => {
                let arrow = TerminalKeyEvent::Arrow {
                    direction,
                    ctrl: false,
                    alt: false,
                };
                let bytes = terminal_core::input::key_event_to_bytes(&arrow);
                let mut seq = Vec::with_capacity(bytes.len() * count);
                for _ in 0..count {
                    seq.extend_from_slice(&bytes);
                }
                if let Err(err) = session.write_to_shell(&seq) {
                    tracing::warn!("Could not send wheel scroll to the shell: {err}");
                }
            }
        }
        self.needs_redraw = true;
    }

    /// Scroll the view of the pane under the cursor (or the focused pane).
    ///
    /// Positive `lines` scrolls back into the history, negative returns towards
    /// the newest output (RF-53).
    fn scroll_view(&mut self, lines: i32) {
        if lines == 0 {
            return;
        }
        let Some(session) = self.scroll_target_session() else {
            return;
        };
        let buffer = session.engine_mut().buffer_mut();
        if lines > 0 {
            buffer.scroll_back(lines.unsigned_abs() as usize);
        } else {
            buffer.scroll_forward(lines.unsigned_abs() as usize);
        }
        self.needs_redraw = true;
    }

    /// Jump the view of the pane under the cursor to the top or bottom.
    fn scroll_view_to_edge(&mut self, top: bool) {
        let Some(session) = self.scroll_target_session() else {
            return;
        };
        let buffer = session.engine_mut().buffer_mut();
        if top {
            buffer.scroll_to_top();
        } else {
            buffer.scroll_to_bottom();
        }
        self.needs_redraw = true;
    }

    /// The session to scroll: the pane under the mouse, else the focused pane.
    fn scroll_target_session(&mut self) -> Option<&mut TerminalSession> {
        let panel = self
            .pane_at(self.last_mouse_position)
            .map(|(id, _)| id)
            .or_else(|| self.active_tab_state().map(|s| s.layout.focused_panel()))?;
        self.active_tab_state_mut()?
            .panes
            .get_mut(&panel)
            .map(|pane| &mut pane.session)
    }

    /// Text currently selected in the active pane, if any.
    fn selected_text(&self) -> Option<String> {
        let engine = self.engine()?;
        let buffer = engine.buffer().active();
        self.event_processor.selection().extract_text(&buffer.grid)
    }

    /// Copy the current selection to the system clipboard.
    fn copy_selection(&mut self) {
        let Some(text) = self.selected_text() else {
            tracing::debug!("Copy requested with no selection");
            return;
        };
        if text.is_empty() {
            return;
        }

        if let Err(err) = self.clipboard.set_text(&text) {
            tracing::warn!("Could not copy to the system clipboard: {err}");
        }
        tracing::debug!("Copied {} bytes to the clipboard", text.len());
    }

    /// Send `text` to the active session, honouring bracketed paste mode.
    fn paste_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let bracketed = self
            .engine()
            .is_some_and(|engine| engine.state().modes.bracketed_paste);
        let bytes = terminal_core::input::paste_bytes(text, bracketed);

        if let Some(session) = self.active_session_mut() {
            if let Err(err) = session.write_to_shell(&bytes) {
                tracing::warn!("Could not paste into the shell: {err}");
            }
        }
        tracing::debug!("Pasted {} bytes", bytes.len());
        self.needs_redraw = true;
    }

    /// Update cursor visibility (blink effect).
    fn update_cursor(&mut self, delta_ms: u64) {
        // A zero interval, or a program that disabled blinking (DECSCUSR),
        // means a steady cursor (RF-48, RF-49).
        let blinking = self
            .engine()
            .is_none_or(|engine| engine.state().cursor.blink);
        if self.cursor_blink_interval == 0 || !blinking {
            self.cursor_visible = true;
            return;
        }

        self.cursor_blink_timer += delta_ms;
        if self.cursor_blink_timer >= self.cursor_blink_interval {
            self.cursor_visible = !self.cursor_visible;
            self.cursor_blink_timer = 0;
            self.needs_redraw = true;
        }
    }

    /// Render the current frame.
    #[expect(
        clippy::too_many_lines,
        clippy::cast_sign_loss,
        reason = "Builds the tab bar and every pane's geometry; the opacity is clamped to 0..=1"
    )]
    fn render(&mut self) {
        let Some(app_state) = self.app_state.as_ref() else {
            return;
        };
        let app_state = app_state.lock().unwrap();

        if self.renderer.is_none() {
            return;
        }

        // RF-47 / CL-12: while the focused program is inside a synchronized
        // update (`ESC [ ? 2026 h` … `l`), hold the frame so no intermediate
        // state is shown. The hold is bounded by `SYNC_TIMEOUT_MS` so a program
        // that dies without sending the closing `l` cannot freeze the window.
        if self.synchronized_output_active() {
            let deadline = *self.sync_deadline.get_or_insert_with(|| {
                std::time::Instant::now() + std::time::Duration::from_millis(SYNC_TIMEOUT_MS)
            });
            if std::time::Instant::now() < deadline {
                return;
            }
        } else {
            self.sync_deadline = None;
        }

        // Background color from config (RGBA-ordered for the renderer).
        let mut bg_render = parse_palette_color(&self.config.colors.primary.background)
            .unwrap_or_else(|| pack_rgba(0x1e, 0x1e, 0x2e, 0xFF));
        // Apply the window opacity to the clear alpha (RGBA packing: low byte).
        if self.window_opacity < 1.0 {
            let alpha = (self.window_opacity.clamp(0.0, 1.0) * 255.0) as u32;
            bg_render = (bg_render & 0xFFFF_FF00) | alpha;
        }

        // ---- Tab bar (layout shared with mouse hit-testing) ----
        let bar_height = self.tab_bar.height();
        let (cell_w, _cell_h) = self.cell_size();
        let active_idx = self.tab_manager.active_idx().unwrap_or(0);
        let titles: Vec<String> = self
            .tab_manager
            .tabs()
            .iter()
            .map(|t| t.title().to_string())
            .collect();
        let title_refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        let tab_rects = self
            .tab_bar
            .layout(&title_refs, cell_w, cell_w * self.grid_cols as u32);
        let labels: Vec<TabLabel> = tab_rects
            .iter()
            .filter_map(|rect| {
                title_refs.get(rect.index).map(|&title| TabLabel {
                    title,
                    x: rect.x,
                    width: rect.width,
                    active: rect.index == active_idx,
                    close_x: rect.close_x,
                })
            })
            .collect();
        let tab_bar = if labels.is_empty() {
            None
        } else {
            Some(TabBarContent {
                tabs: &labels,
                height: bar_height,
                cell_width: cell_w,
            })
        };

        // ---- Panes of the active tab ----
        let area = self.terminal_area();
        let selection = self.event_processor.selection();
        let selection_pane = self.selection_pane;

        // Cell buffers are kept alive here; the panes below borrow them.
        let mut pane_cells: Vec<Vec<terminal_render::renderer::CellContent>> = Vec::new();
        let mut pane_specs: Vec<PaneSpec> = Vec::new();
        // Divider gaps between the panes of the active tab (RF-52).
        let mut divider_rects: Vec<PaneRect> = Vec::new();

        if let Some(tab_state) = self.active_tab_state() {
            let focused = tab_state.layout.focused_panel();
            divider_rects = tab_state
                .layout
                .divider_rects(area, SPLIT_GAP)
                .into_iter()
                .map(|r| PaneRect {
                    x: r.x,
                    y: r.y,
                    width: r.width,
                    height: r.height,
                })
                .collect();
            for (id, rect) in tab_state.layout.rects(area, SPLIT_GAP) {
                let Some(session) = tab_state.panes.get(&id).map(|pane| &pane.session) else {
                    continue;
                };
                let engine = session.engine();
                let grid_buffer = engine.buffer();
                let st = engine.state();
                let cols = grid_buffer.active().grid.cols;

                // Rows as seen through the scroll offset: history lines first,
                // then the active grid (RF-53).
                let cells: Vec<terminal_render::renderer::CellContent> = grid_buffer
                    .visible_rows()
                    .iter()
                    .flat_map(|row| {
                        row.iter()
                            .map(|cell| terminal_render::renderer::CellContent {
                                ch: cell.c,
                                style: cell.style,
                                width: cell.width,
                            })
                    })
                    .collect();

                let cursor_shape = match st.cursor.shape {
                    terminal_core::state::CursorShape::Block => CursorShape::Block,
                    terminal_core::state::CursorShape::Underline => CursorShape::Underline,
                    terminal_core::state::CursorShape::Beam => CursorShape::Beam,
                };

                // The cursor is hidden while the view is scrolled back (RF-54).
                let cursor_visible = id == focused
                    && self.cursor_visible
                    && st.cursor.visible
                    && grid_buffer.scroll_offset() == 0;

                pane_cells.push(cells);
                pane_specs.push(PaneSpec {
                    rect,
                    cursor_shape,
                    cursor_row: st.cursor.row,
                    cursor_col: st.cursor.col,
                    cursor_visible,
                    cols,
                    selection: if selection_pane == Some(id) {
                        Selection {
                            start: selection.start,
                            end: selection.current,
                        }
                    } else {
                        Selection::default()
                    },
                });
            }
        }

        let panes: Vec<GridPane> = pane_cells
            .iter()
            .zip(&pane_specs)
            .map(|(cells, spec)| GridPane {
                cells,
                rows: cells.len().checked_div(spec.cols).unwrap_or(0),
                cols: spec.cols,
                rect: PaneRect {
                    x: spec.rect.x,
                    y: spec.rect.y,
                    width: spec.rect.width,
                    height: spec.rect.height,
                },
                cursor_row: spec.cursor_row,
                cursor_col: spec.cursor_col,
                cursor_shape: spec.cursor_shape,
                cursor_visible: spec.cursor_visible,
                selection: spec.selection,
            })
            .collect();

        // Divider accent colour: the theme's cursor colour, kept subtle with a
        // little transparency, falling back to a neutral grey (RF-52).
        let divider_color = parse_palette_color(&self.config.colors.primary.cursor).map_or_else(
            || pack_rgba(0x60, 0x60, 0x70, 0xFF),
            |c| (c & 0xFFFF_FF00) | 0x88,
        );

        let content = RenderContent {
            panes: &panes,
            background_color: bg_render,
            tab_bar,
            dividers: &divider_rects,
            divider_color,
        };

        // Get the surface texture view and render
        if let Ok(frame) = app_state.gpu.surface.get_current_texture() {
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            // Use the renderer to render the terminal content
            let surface_config = app_state.surface_config.lock().unwrap();
            self.renderer
                .as_mut()
                .unwrap()
                .render(&view, &surface_config, &content);

            // Present the frame
            frame.present();
        }
    }

    /// Request a redraw.
    #[expect(dead_code, reason = "Used for future redraw management")]
    fn request_redraw(&self) {
        if let Some(app_state) = &self.app_state {
            let app_state = app_state.lock().unwrap();
            app_state.window.request_redraw();
        }
    }
}

/// Application handler for winit event loop.
struct App {
    state: std::sync::Mutex<AppInner>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Clone needed data before creating window (avoids holding lock during window creation)
        let (window_title, gpu_base, transparent, maximized) = {
            let state = self.state.lock().unwrap();
            (
                state.window_title.clone(),
                state.gpu_base.clone(),
                state.window_opacity < 1.0,
                state.config.window.maximized,
            )
        };

        // Create window (transparent when the configured opacity is below 1.0)
        let window_attrs = winit::window::WindowAttributes::default()
            .with_title(&window_title)
            .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(320.0, 240.0))
            .with_visible(true)
            .with_active(true)
            .with_transparent(transparent)
            .with_maximized(maximized);

        // Name the window as an application, not just with a title: the Wayland
        // `app_id` / X11 `WM_CLASS` is what the desktop shell shows in the task
        // bar and uses to look up `terminal.desktop` for the icon.
        #[cfg(target_os = "linux")]
        let window_attrs = window_attrs.with_name(APP_ID, APP_ID);

        let window = match event_loop.create_window(window_attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("Failed to create window: {}", e);
                return;
            }
        };

        // Create full GPU context from base (surface creation is synchronous)
        let mut gpu = match GpuContext::from_base(&window, &gpu_base) {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("Failed to create GPU context: {}", e);
                return;
            }
        };
        // Pick an sRGB surface format so it matches the render pipeline target.
        let _ = gpu.update_preferred_format();
        let target_format = gpu.preferred_format;
        let gpu = Arc::new(gpu);

        // Create renderer
        let config = {
            let state = self.state.lock().unwrap();
            state.config.clone()
        };
        let font_config = RenderFontConfig {
            family: config.font.family.clone(),
            size: config.font.size,
            load_system_fonts: true,
        };
        let mut renderer = TerminalRenderer::new(
            &Arc::clone(&gpu.device),
            &Arc::clone(&gpu.queue),
            &font_config,
            target_format,
        );
        // Apply the configured colors.
        renderer.set_palette(palette_from_config(&config.colors));
        renderer.set_cursor_color(cursor_color_from_config(&config.colors));

        // Fit the window to the terminal grid plus the tab bar.
        let (cell_w, cell_h) = renderer.cell_size();
        let bar_h = self.state.lock().unwrap().tab_bar.height();
        let grid_width = (cell_w * u32::from(config.window.dimensions.columns)).max(1);
        let grid_height = (cell_h * u32::from(config.window.dimensions.rows) + bar_h).max(1);

        // When starting maximized the compositor picks the size, so only the
        // initial surface configuration uses the grid size.
        if !maximized {
            let _ =
                window.request_inner_size(winit::dpi::PhysicalSize::new(grid_width, grid_height));
        }

        // Configure surface - defer to avoid blocking on Wayland
        let mut surface_config = wgpu::SurfaceConfiguration {
            width: grid_width,
            height: grid_height,
            format: target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: vec![],
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
        };
        // Configure the surface so we can render to it
        gpu.configure_surface(&mut surface_config);

        // Create shared app state
        let app_state = Arc::new(std::sync::Mutex::new(AppState {
            window,
            gpu,
            surface_config: std::sync::Mutex::new(surface_config),
        }));

        // Store in state
        let mut s = self.state.lock().unwrap();
        s.renderer = Some(renderer);
        s.app_state = Some(app_state);
        s.needs_redraw = true; // Force initial redraw
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match &event {
            WindowEvent::CloseRequested => {
                std::process::exit(0);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let mut state = self.state.lock().unwrap();
                state.event_processor.update_modifiers(modifiers.state());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let mut state = self.state.lock().unwrap();
                state.process_key_event(event);
            }
            WindowEvent::MouseInput {
                state: btn_state,
                button,
                ..
            } => {
                let mut state = self.state.lock().unwrap();
                let pos = state.last_mouse_position;
                state.process_mouse_button(*button, *btn_state, pos);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let mut state = self.state.lock().unwrap();
                state.process_mouse_motion((position.x, position.y));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let mut state = self.state.lock().unwrap();
                let (_cell_w, cell_h) = state.cell_size();
                let lines: f64 = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y / f64::from(cell_h.max(1)),
                };
                // Wheel up (positive) scrolls back into the history (RF-53);
                // on the alternate screen it becomes cursor keys (RF-56).
                state.wheel_scroll(lines.round() as i32);
            }
            WindowEvent::Resized(size) => {
                let mut state = self.state.lock().unwrap();
                let width = size.width.max(1);
                let height = size.height.max(1);

                // Reconfigure the GPU surface to the new size.
                if let Some(app_state_mutex) = state.app_state.clone() {
                    let app_state = app_state_mutex.lock().unwrap();
                    let mut config = app_state.surface_config.lock().unwrap();
                    config.width = width;
                    config.height = height;
                    app_state.gpu.configure_surface(&mut config);
                }

                // Reflow the terminal grid below the tab bar.
                let (cell_w, cell_h) = state.cell_size();
                let bar_h = state.tab_bar.height();
                let cols = (width / cell_w.max(1)).max(1) as usize;
                let rows = (height.saturating_sub(bar_h) / cell_h.max(1)).max(1) as usize;
                state.grid_rows = rows;
                state.grid_cols = cols;
                state.resize_sessions();
                state.needs_redraw = true;
            }
            WindowEvent::RedrawRequested => {
                let should_exit = {
                    let mut state = self.state.lock().unwrap();
                    let exit = state.poll_sessions();
                    if !exit {
                        state.update_cursor(16); // ~60fps
                        state.render();
                        // Always request next frame for continuous rendering
                        state.needs_redraw = true;
                    }
                    exit
                };

                if should_exit {
                    tracing::info!("No sessions left, closing window");
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let should_redraw = {
            let state = self.state.lock().unwrap();
            state.needs_redraw && state.app_state.is_some()
        };

        if should_redraw {
            // Reset needs_redraw
            {
                let mut state = self.state.lock().unwrap();
                state.needs_redraw = false;
            }

            // Request redraw
            let app_state = {
                let state = self.state.lock().unwrap();
                state.app_state.clone()
            };
            if let Some(app_state) = app_state {
                let app_state = app_state.lock().unwrap();
                app_state.window.request_redraw();
            }
        }
    }
}

/// Parse a `#rrggbb` config color into an RGBA-ordered palette entry.
///
/// [`TerminalPalette`] entries are RGBA-ordered (`r << 24 | g << 16 | b << 8 | a`),
/// which is why [`parse_hex_color`](terminal_render::color::parse_hex_color) is
/// not used here (it returns ARGB).
fn parse_palette_color(hex: &str) -> Option<RenderColor> {
    let cleaned = hex.trim().trim_start_matches('#');
    if cleaned.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&cleaned[0..2], 16).ok()?;
    let g = u8::from_str_radix(&cleaned[2..4], 16).ok()?;
    let b = u8::from_str_radix(&cleaned[4..6], 16).ok()?;
    Some(pack_rgba(r, g, b, 0xFF))
}

/// Build the renderer palette from the configured color scheme.
fn palette_from_config(colors: &ColorScheme) -> TerminalPalette {
    let mut palette = TerminalPalette::default();

    let standard = [
        &colors.ansi.black,
        &colors.ansi.red,
        &colors.ansi.green,
        &colors.ansi.yellow,
        &colors.ansi.blue,
        &colors.ansi.magenta,
        &colors.ansi.cyan,
        &colors.ansi.white,
    ];
    for (slot, hex) in palette.standard.iter_mut().zip(standard) {
        if let Some(color) = parse_palette_color(hex) {
            *slot = color;
        }
    }

    let bright = [
        &colors.ansi.bright_black,
        &colors.ansi.bright_red,
        &colors.ansi.bright_green,
        &colors.ansi.bright_yellow,
        &colors.ansi.bright_blue,
        &colors.ansi.bright_magenta,
        &colors.ansi.bright_cyan,
        &colors.ansi.bright_white,
    ];
    for (slot, hex) in palette.bright.iter_mut().zip(bright) {
        if let Some(color) = parse_palette_color(hex) {
            *slot = color;
        }
    }

    if let Some(color) = parse_palette_color(&colors.primary.foreground) {
        palette.foreground = color;
    }
    if let Some(color) = parse_palette_color(&colors.primary.background) {
        palette.background = color;
    }

    palette
}

/// Cursor color from the configured color scheme (kept translucent).
fn cursor_color_from_config(colors: &ColorScheme) -> [f32; 4] {
    match parse_palette_color(&colors.primary.cursor) {
        Some(color) => {
            let [r, g, b, _] = unpack_rgba(color);
            [r, g, b, 0.6]
        }
        None => [0.75, 0.75, 0.95, 0.6],
    }
}

/// Map the configured cursor style to the engine's cursor shape.
fn cursor_shape_from_config(style: terminal_config::config::CursorStyle) -> EngineCursorShape {
    match style {
        terminal_config::config::CursorStyle::Block => EngineCursorShape::Block,
        terminal_config::config::CursorStyle::Underline => EngineCursorShape::Underline,
        terminal_config::config::CursorStyle::Beam => EngineCursorShape::Beam,
    }
}

fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "terminal=debug,info".into()),
        )
        .init();

    tracing::info!("Terminal emulator starting");

    // CLI overrides the config file (RF-33, RF-63).
    let cli = parse_cli();

    // Load configuration
    let config = match loader::load_config(&loader::find_config_path()) {
        Ok(cfg) => {
            tracing::info!("Configuration loaded successfully");
            cfg
        }
        Err(e) => {
            tracing::error!("Failed to load configuration: {}", e);
            Config::default()
        }
    };
    let config = merge_cli(config, &cli);

    tracing::info!(
        "Terminal engine created: {}x{}",
        config.window.dimensions.rows,
        config.window.dimensions.columns
    );

    // Initialize wgpu + GPU base before event loop (critical for Wayland)
    let wgpu_instance =
        std::sync::Arc::new(wgpu::Instance::new(wgpu::InstanceDescriptor::default()));
    let gpu_base = pollster::block_on(GpuContextBase::new(Arc::clone(&wgpu_instance)))
        .expect("Failed to create GPU base");

    // Create and run the application
    let mut app = App {
        state: std::sync::Mutex::new(AppInner::new(&config, wgpu_instance, gpu_base)),
    };

    let event_loop = winit::event_loop::EventLoop::new().expect("Failed to create event loop");

    event_loop.run_app(&mut app).expect("Event loop error");
    // app is used inside the event loop, no need to use it after
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program name that cannot exist, so spawning it always fails.
    const MISSING_PROGRAM: &str = "terminal-definitely-missing-shell-xyz";

    /// Read the visible screen as text, one line per row.
    fn screen_text(session: &TerminalSession) -> String {
        let buf = session.buffer().active();
        let (rows, cols) = (buf.grid.rows, buf.grid.cols);
        let mut text = String::new();
        for row in 0..rows {
            for col in 0..cols {
                text.push(buf.grid.get(row, col).c);
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn test_spawn_session_uses_the_requested_shell() {
        // RF-62: the session runs the command it was given.
        let command = Config::default().default_shell_command();
        let pane = AppInner::spawn_session(24, 80, EngineCursorShape::Block, &command);
        let session = &pane.session;
        assert!(session.is_running());
        assert!(session.pid().is_some());
    }

    #[test]
    fn test_spawn_session_falls_back_to_the_default_shell() {
        // RF-67, CL-24: a shell that cannot be started leaves the pane usable
        // by running the shell of the system instead.
        let command = ShellCommand::new("broken", MISSING_PROGRAM, Vec::new());
        let pane = AppInner::spawn_session(24, 80, EngineCursorShape::Block, &command);
        let session = &pane.session;

        assert!(session.is_running());
        assert!(session.pid().is_some());
    }

    #[test]
    fn test_spawn_session_reports_the_error_visibly() {
        // RF-67: the failure must be visible, not silent.
        let command = ShellCommand::new("broken", MISSING_PROGRAM, Vec::new());
        let pane = AppInner::spawn_session(24, 80, EngineCursorShape::Block, &command);

        let text = screen_text(&pane.session);
        assert!(
            text.contains(MISSING_PROGRAM),
            "expected an error notice, got: {text:?}"
        );
        assert!(
            text.contains("falling back"),
            "expected a fallback notice, got: {text:?}"
        );
    }

    #[test]
    fn test_split_inherits_the_focused_pane_shell() {
        // RF-70: splitting keeps the shell that is already running instead of
        // going back to the default shell.
        let running = Config::default().default_shell_command();
        let tab = TabState::new(24, 80, EngineCursorShape::Block, &running);
        let other = ShellCommand::new("other", MISSING_PROGRAM, Vec::new());

        let inherited = tab.split_command(&other);

        assert_eq!(inherited, running);
        assert_ne!(inherited, other);
    }

    #[test]
    fn test_split_inherits_the_shell_that_actually_runs() {
        // CL-31: when the chosen shell failed and the system shell took over
        // (RF-67), the new pane inherits the shell that runs, not the broken one.
        let broken = ShellCommand::new("broken", MISSING_PROGRAM, Vec::new());
        let tab = TabState::new(24, 80, EngineCursorShape::Block, &broken);

        let inherited = tab.split_command(&broken);

        assert_eq!(inherited, ShellCommand::platform_default());
        assert_ne!(inherited.program, MISSING_PROGRAM);
    }
}
