//! Terminal emulator — main application.
//!
//! This module integrates all crates into a single application:
//! - Config loading and CLI parsing
//! - Terminal core engine + session (PTY)
//! - GPU rendering pipeline
//! - GUI with tabs, splits, keymap, clipboard
//! - Full event loop connecting everything

#![warn(missing_docs)]

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use terminal_config::config::Config;
use terminal_config::loader;

use terminal_core::engine::TerminalEngine;
use terminal_core::session::{PollResult, TerminalSession};

use terminal_render::color::{pack_rgba, Color as RenderColor, TerminalPalette};
use terminal_render::font::FontConfig as RenderFontConfig;
use terminal_render::renderer::{CursorShape, RenderContent, TerminalRenderer};

use terminal_gui::clipboard::Clipboard;
use terminal_gui::event::{EventProcessor, EventResult};
use terminal_gui::keymap::GuiAction;
use terminal_gui::split::{NavigateDirection, SplitLayout};
use terminal_gui::tab_bar::TabBar;
use terminal_gui::tabs::TabManager;
use terminal_gui::window::{GpuContext, GpuContextBase};

/// Shared application state for the event loop.
pub struct AppState {
    /// The winit window.
    pub window: Arc<winit::window::Window>,
    /// GPU context.
    pub gpu: Arc<GpuContext>,
    /// Surface configuration (separate mutex to avoid borrow conflicts).
    pub surface_config: std::sync::Mutex<wgpu::SurfaceConfiguration>,
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
    /// Split layout for the active tab's panel.
    split_layout: SplitLayout,
    /// Clipboard for copy/paste.
    clipboard: Clipboard,
    /// Event processor for keyboard/mouse.
    event_processor: EventProcessor,
    /// Active terminal session.
    session: Option<TerminalSession>,
    /// Terminal renderer.
    #[allow(dead_code)]
    renderer: Option<TerminalRenderer>,
    /// Tab bar UI.
    #[allow(dead_code)]
    tab_bar: TabBar,
    /// Window title.
    window_title: String,
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
}

impl AppInner {
    /// Create a new application state.
    fn new(
        config: &Config,
        wgpu_instance: std::sync::Arc<wgpu::Instance>,
        gpu_base: GpuContextBase,
    ) -> Self {
        let mut tab_manager = TabManager::new();
        let _tab_id = tab_manager.create_tab();

        let split_layout = SplitLayout::new();

        // Create initial session for the active tab
        let mut session = TerminalSession::new(
            config.window.dimensions.rows as usize,
            config.window.dimensions.columns as usize,
        );
        let _ = session.spawn_shell(&default_shell());

        Self {
            config: config.clone(),
            wgpu_instance,
            gpu_base,
            tab_manager,
            split_layout,
            clipboard: Clipboard::new(),
            event_processor: EventProcessor::new(),
            session: Some(session),
            renderer: None,
            tab_bar: TabBar::new(),
            window_title: config.window.title.clone(),
            cursor_visible: true,
            cursor_blink_timer: 0,
            needs_redraw: true,
            palette: TerminalPalette::default(),
            last_mouse_position: (0.0, 0.0),
            app_state: None,
        }
    }

    /// Get the active session.
    #[expect(dead_code, reason = "Used for future session management")]
    fn session_mut(&mut self) -> Option<&mut TerminalSession> {
        self.session.as_mut()
    }

    /// Get the active session (immutable).
    #[expect(dead_code, reason = "Used for future session management")]
    fn session(&self) -> Option<&TerminalSession> {
        self.session.as_ref()
    }

    /// Get the engine from the active session.
    fn engine(&self) -> Option<&TerminalEngine> {
        self.session.as_ref().map(TerminalSession::engine)
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
    fn poll_session(&mut self) {
        if let Some(session) = self.session.as_mut() {
            loop {
                match session.poll_pty() {
                    Ok(PollResult::WouldBlock) => break,
                    Ok(PollResult::Data(_)) => {
                        // Data was already fed into the engine by poll_pty
                        self.needs_redraw = true;
                    }
                    Ok(PollResult::Exited { exit_code }) => {
                        tracing::info!("Session exited with code: {}", exit_code);
                        self.needs_redraw = true;
                        break;
                    }
                    Err(e) => {
                        tracing::error!("PTY error: {}", e);
                        break;
                    }
                }
            }
        }
    }

    /// Process a key event.
    fn process_key_event(&mut self, event: &winit::event::KeyEvent) {
        if let Some(result) = self.event_processor.process_key_event(event) {
            match result {
                EventResult::GuiAction(action) => {
                    self.handle_gui_action(action);
                }
                EventResult::TerminalInput(input) => {
                    // Send terminal input to the session
                    if let Some(session) = self.session.as_mut() {
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

    /// Process a mouse button event.
    fn process_mouse_button(
        &mut self,
        button: winit::event::MouseButton,
        state: winit::event::ElementState,
        position: (f64, f64),
    ) {
        let mouse_event = self.event_processor.process_mouse_event(
            button, state, position, 10.0, // approximate cell width
            20.0, // approximate cell height
        );
        if mouse_event.selection_active {
            self.needs_redraw = true;
        }
    }

    /// Process a mouse cursor move (for drag selection).
    fn process_mouse_motion(&mut self, position: (f64, f64)) {
        self.last_mouse_position = position;
        if self.event_processor.selection().is_active() {
            self.event_processor.update_selection(
                position, 10.0, // approximate cell width
                20.0, // approximate cell height
            );
            self.needs_redraw = true;
        }
    }

    /// Handle a GUI action from the keymap.
    #[expect(
        clippy::too_many_lines,
        reason = "Central dispatch for all keymap actions"
    )]
    fn handle_gui_action(&mut self, action: GuiAction) {
        match action {
            GuiAction::NewTab => {
                let tab_id = self.tab_manager.create_tab();
                tracing::info!("New tab created: {:?}", tab_id);

                // Create a new session for the tab
                let mut session = TerminalSession::new(
                    self.config.window.dimensions.rows as usize,
                    self.config.window.dimensions.columns as usize,
                );
                let _ = session.spawn_shell(&default_shell());
                self.session = Some(session);

                // Reset split layout for new tab
                self.split_layout = SplitLayout::new();

                self.needs_redraw = true;
            }
            GuiAction::CloseTab => {
                if let Some(new_idx) = self.tab_manager.close_active_tab() {
                    // Switch to the new active tab
                    if let Some(tab) = self.tab_manager.get_tab(new_idx) {
                        tracing::info!("Switched to tab: {}", tab.title());
                        // Create session for new tab
                        let mut session = TerminalSession::new(
                            self.config.window.dimensions.rows as usize,
                            self.config.window.dimensions.columns as usize,
                        );
                        let _ = session.spawn_shell(&default_shell());
                        self.session = Some(session);
                        self.split_layout = SplitLayout::new();
                    }
                } else {
                    // No tabs left, close app
                    tracing::info!("Last tab closed, exiting");
                    std::process::exit(0);
                }
                self.needs_redraw = true;
            }
            GuiAction::NextTab => {
                self.tab_manager.switch_next();
                if let Some(tab) = self.tab_manager.active() {
                    tracing::info!("Switched to tab: {}", tab.title());
                    if let Some(session) = self.session.as_mut() {
                        session.stop();
                    }
                    let mut new_session = TerminalSession::new(
                        self.config.window.dimensions.rows as usize,
                        self.config.window.dimensions.columns as usize,
                    );
                    let _ = new_session.spawn_shell(&default_shell());
                    self.session = Some(new_session);
                    self.split_layout = SplitLayout::new();
                }
                self.needs_redraw = true;
            }
            GuiAction::PrevTab => {
                self.tab_manager.switch_prev();
                if let Some(tab) = self.tab_manager.active() {
                    tracing::info!("Switched to tab: {}", tab.title());
                    if let Some(session) = self.session.as_mut() {
                        session.stop();
                    }
                    let mut new_session = TerminalSession::new(
                        self.config.window.dimensions.rows as usize,
                        self.config.window.dimensions.columns as usize,
                    );
                    let _ = new_session.spawn_shell(&default_shell());
                    self.session = Some(new_session);
                    self.split_layout = SplitLayout::new();
                }
                self.needs_redraw = true;
            }
            GuiAction::SplitHorizontal => {
                let _panel_id = self.split_layout.split_horizontal(0.5);
                tracing::info!("Split horizontal");
                self.needs_redraw = true;
            }
            GuiAction::SplitVertical => {
                let _panel_id = self.split_layout.split_vertical(0.5);
                tracing::info!("Split vertical");
                self.needs_redraw = true;
            }
            GuiAction::ClosePanel => {
                if let Some(panel_id) = self.split_layout.close_panel() {
                    tracing::info!("Closed panel: {:?}", panel_id);
                }
                self.needs_redraw = true;
            }
            GuiAction::NavigateUp => {
                let _panel_id = self.split_layout.navigate(NavigateDirection::Up);
                self.needs_redraw = true;
            }
            GuiAction::NavigateDown => {
                let _panel_id = self.split_layout.navigate(NavigateDirection::Down);
                self.needs_redraw = true;
            }
            GuiAction::NavigateLeft => {
                let _panel_id = self.split_layout.navigate(NavigateDirection::Left);
                self.needs_redraw = true;
            }
            GuiAction::NavigateRight => {
                let _panel_id = self.split_layout.navigate(NavigateDirection::Right);
                self.needs_redraw = true;
            }
            GuiAction::Copy => {
                // Copy selection to clipboard
                if let Some(engine) = self.engine() {
                    let buf = engine.buffer().active();
                    let selection = self.event_processor.selection();
                    if let Some((r1, c1, r2, c2)) = selection.bounds() {
                        let mut text = String::new();
                        for row in r1..=r2.min(buf.grid.rows - 1) {
                            if row > r1 {
                                text.push('\n');
                            }
                            for col in c1..=c2.min(buf.grid.cols - 1) {
                                let cell = buf.grid.get(row, col);
                                text.push(cell.c);
                            }
                        }
                        self.clipboard.set_text(text);
                        tracing::debug!(
                            "Copied to clipboard: {} bytes",
                            self.clipboard.text().len()
                        );
                    }
                }
            }
            GuiAction::Paste => {
                // Paste clipboard content to session
                if let Some(session) = self.session.as_mut() {
                    let text = self.clipboard.text();
                    if !text.is_empty() {
                        let _ = session.write_to_shell(text.as_bytes());
                        tracing::debug!("Pasted {} bytes", text.len());
                    }
                }
                self.needs_redraw = true;
            }
            GuiAction::None => {}
        }
    }

    /// Update cursor visibility (blink effect).
    fn update_cursor(&mut self, delta_ms: u64) {
        self.cursor_blink_timer += delta_ms;
        if self.cursor_blink_timer >= 500 {
            self.cursor_visible = !self.cursor_visible;
            self.cursor_blink_timer = 0;
            self.needs_redraw = true;
        }
    }

    /// Render the current frame.
    fn render(&mut self) {
        let Some(app_state) = self.app_state.as_ref() else {
            return;
        };
        let app_state = app_state.lock().unwrap();

        if self.renderer.is_none() {
            return;
        }
        let Some(engine) = self.engine() else {
            return;
        };

        let buf = engine.buffer().active();
        let state = engine.state();

        // Build cursor shape
        let cursor_shape = match state.cursor.shape {
            terminal_core::state::CursorShape::Block => CursorShape::Block,
            terminal_core::state::CursorShape::Underline => CursorShape::Underline,
            terminal_core::state::CursorShape::Beam => CursorShape::Beam,
        };

        // Build cells array with proper color conversion
        let cells: Vec<terminal_render::renderer::CellContent> = (0..buf.grid.rows)
            .flat_map(|row| {
                (0..buf.grid.cols).map(move |col| {
                    let cell = buf.grid.get(row, col);
                    terminal_render::renderer::CellContent {
                        ch: cell.c,
                        fg: cell.style.fg.as_u8_or_ansi(),
                        bg: cell.style.bg.as_u8_or_ansi(),
                    }
                })
            })
            .collect();

        // Parse background color from config (RGBA-ordered for the renderer)
        let bg_hex = self
            .config
            .colors
            .primary
            .background
            .trim_start_matches('#');
        let bg_color = u32::from_str_radix(bg_hex, 16).unwrap_or(0x001e_1e2e);
        let bg_render = pack_rgba(
            ((bg_color >> 16) & 0xFF) as u8,
            ((bg_color >> 8) & 0xFF) as u8,
            (bg_color & 0xFF) as u8,
            0xFF,
        );

        // Selection highlight from the GUI event processor.
        let selection = match self.event_processor.selection().bounds() {
            Some((r1, c1, r2, c2)) => terminal_render::renderer::Selection {
                start: Some((r1, c1)),
                end: Some((r2, c2)),
            },
            None => terminal_render::renderer::Selection::default(),
        };

        let content = RenderContent {
            cells: &cells,
            cursor_row: state.cursor.row,
            cursor_col: state.cursor.col,
            cursor_shape,
            cursor_visible: self.cursor_visible,
            selection,
            background_color: bg_render,
            rows: buf.grid.rows,
            cols: buf.grid.cols,
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
        let (window_title, gpu_base) = {
            let state = self.state.lock().unwrap();
            (state.window_title.clone(), state.gpu_base.clone())
        };

        // Create window
        let window_attrs = winit::window::WindowAttributes::default()
            .with_title(&window_title)
            .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(320.0, 240.0))
            .with_visible(true)
            .with_active(true);

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
        let renderer = TerminalRenderer::new(
            &Arc::clone(&gpu.device),
            &Arc::clone(&gpu.queue),
            &font_config,
            target_format,
        );

        // Size the window to fit the terminal grid (cols x cell_width).
        let (cell_w, cell_h) = renderer.cell_size();
        let grid_width = (cell_w * u32::from(config.window.dimensions.columns)).max(1);
        let grid_height = (cell_h * u32::from(config.window.dimensions.rows)).max(1);
        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(grid_width, grid_height));

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
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match &event {
            WindowEvent::CloseRequested => {
                std::process::exit(0);
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
            WindowEvent::Resized(size) => {
                {
                    if let Some(app_state_mutex) = &self.state.lock().unwrap().app_state {
                        let app_state = app_state_mutex.lock().unwrap();
                        let width = size.width.max(1);
                        let height = size.height.max(1);
                        {
                            let mut config = app_state.surface_config.lock().unwrap();
                            config.width = width;
                            config.height = height;
                            app_state.gpu.configure_surface(&mut config);
                        }
                    }
                }
                self.state.lock().unwrap().needs_redraw = true;
            }
            WindowEvent::RedrawRequested => {
                let mut state = self.state.lock().unwrap();
                state.poll_session();
                state.update_cursor(16); // ~60fps
                state.render();
                // Always request next frame for continuous rendering
                state.needs_redraw = true;
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

/// Return the shell to spawn on Unix: `$SHELL`, falling back to `/bin/bash`.
#[cfg(unix)]
fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
}

/// Return the shell to spawn on Windows: `%COMSPEC%`, falling back to `cmd.exe`.
#[cfg(windows)]
fn default_shell() -> String {
    std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
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
