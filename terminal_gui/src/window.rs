//! Window creation and management with winit + wgpu.
//!
//! This module handles:
//! - Window creation via `winit`
//! - GPU device initialization via `wgpu`
//! - Surface creation for presentation
//! - Window resize events
//!
//! # Architecture
//!
//! ```text
//! winit EventLoop → WindowEvent → wgpu Device/Surface → present()
//! ```

use std::sync::Arc;

use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::EventLoop,
    window::{Window, WindowAttributes},
};

use wgpu::{
    Adapter, Device, Instance, PowerPreference, Queue, RequestAdapterOptions, Surface,
    SurfaceConfiguration,
};

/// Configuration for the window.
#[derive(Debug, Clone)]
pub struct WindowConfig {
    /// Window title.
    pub title: String,
    /// Initial width in pixels.
    pub width: u32,
    /// Initial height in u32.
    pub height: u32,
    /// Minimum width in pixels.
    pub min_width: u32,
    /// Minimum height in u32.
    pub min_height: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Terminal".to_string(),
            width: 1024,
            height: 768,
            min_width: 320,
            min_height: 240,
        }
    }
}

/// Base GPU context: wgpu instance, adapter, device, queue (no surface).
/// Can be created before the event loop starts.
#[derive(Clone)]
pub struct GpuContextBase {
    /// WGPU instance.
    pub instance: std::sync::Arc<Instance>,
    /// WGPU adapter (GPU device), wrapped in Arc for sharing.
    pub adapter: Arc<Adapter>,
    /// WGPU device (logical GPU device), wrapped in Arc for sharing.
    pub device: Arc<Device>,
    /// WGPU queue (for submitting commands), wrapped in Arc for sharing.
    pub queue: Arc<Queue>,
}

/// GPU context: wgpu instance, adapter, device, queue, and surface.
///
/// Uses `Arc` internally so it can be shared across the event loop.
pub struct GpuContext {
    /// WGPU instance.
    pub instance: std::sync::Arc<Instance>,
    /// WGPU adapter (GPU device), wrapped in Arc for sharing.
    pub adapter: Arc<Adapter>,
    /// WGPU device (logical GPU device), wrapped in Arc for sharing.
    pub device: Arc<Device>,
    /// WGPU queue (for submitting commands), wrapped in Arc for sharing.
    pub queue: Arc<Queue>,
    /// WGPU surface (for presenting to the screen).
    pub surface: Surface<'static>,
    /// Preferred texture format.
    pub preferred_format: wgpu::TextureFormat,
}

impl GpuContextBase {
    /// Create a base GPU context (instance, adapter, device, queue) WITHOUT surface.
    /// Can be called before the event loop starts.
    ///
    /// # Errors
    ///
    /// Returns an error if no suitable GPU adapter is found or the logical
    /// device cannot be created.
    pub async fn new(instance: std::sync::Arc<Instance>) -> Result<Self, String> {
        // Request an adapter (GPU) - no surface needed
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .ok_or_else(|| "No suitable GPU adapter found".to_string())?;

        // Request a logical device
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("terminal_device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None, // tracing
            )
            .await
            .map_err(|e| format!("Failed to create device: {e}"))?;

        Ok(Self {
            instance,
            adapter: Arc::new(adapter),
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    }
}

impl GpuContext {
    /// Create a full GPU context with surface for the given window.
    /// Uses existing base context to avoid re-initializing device/queue.
    /// Note: `preferred_format` is set to `Bgra8Unorm` by default and will be
    /// updated when `configure_surface` is called.
    ///
    /// # Errors
    ///
    /// Returns an error if the window handle cannot be retrieved or the
    /// surface cannot be created.
    pub fn from_base(window: &Window, base: &GpuContextBase) -> Result<Self, String> {
        // Create the surface from the window
        let surface = unsafe {
            let target = wgpu::SurfaceTargetUnsafe::from_window(window)
                .map_err(|e| format!("Failed to get window handle: {e}"))?;
            base.instance
                .create_surface_unsafe(target)
                .map_err(|e| format!("Failed to create surface: {e}"))?
        };

        // Use a default format - will be updated when configure_surface is called
        let preferred_format = wgpu::TextureFormat::Bgra8Unorm;

        Ok(Self {
            instance: Arc::clone(&base.instance),
            adapter: Arc::clone(&base.adapter),
            device: Arc::clone(&base.device),
            queue: Arc::clone(&base.queue),
            surface,
            preferred_format,
        })
    }

    /// Update the preferred format by querying surface capabilities.
    /// This may block on some platforms (e.g., Wayland) so call it after
    /// the event loop has started processing events.
    ///
    /// # Errors
    ///
    /// Currently never fails; the `Result` is kept for future surface errors.
    pub fn update_preferred_format(&mut self) -> Result<(), String> {
        let caps = self.surface.get_capabilities(&self.adapter);
        self.preferred_format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(caps.formats[0]);
        Ok(())
    }

    /// Configure the surface for the given dimensions.
    pub fn configure_surface(&self, config: &mut SurfaceConfiguration) {
        config.width = config.width.max(1);
        config.height = config.height.max(1);
        config.format = self.preferred_format;
        config.alpha_mode = wgpu::CompositeAlphaMode::Auto;
        config.view_formats = vec![];
        config.present_mode = wgpu::PresentMode::AutoVsync;
        self.surface.configure(&self.device, config);
    }
}

/// Options for the WGPU instance.
#[allow(dead_code)]
type InstanceOptions = wgpu::InstanceDescriptor;

/// Shared application state passed to the frame callback.
pub struct AppHandle {
    /// The winit window.
    pub window: Arc<Window>,
    /// GPU context.
    pub gpu: Arc<GpuContext>,
    /// Surface configuration.
    pub surface_config: SurfaceConfiguration,
}

impl AppHandle {
    /// Present the current frame to the screen.
    ///
    /// # Errors
    ///
    /// Returns an error if the surface cannot acquire a frame.
    pub fn present(&mut self) -> Result<(), wgpu::SurfaceError> {
        let frame = self.gpu.surface.get_current_texture()?;
        let _view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame_encoder"),
            });

        // Clear the texture (clear entire image)
        encoder.clear_texture(&frame.texture, &wgpu::ImageSubresourceRange::default());

        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        Ok(())
    }

    /// Resize the surface to the given dimensions.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.gpu.configure_surface(&mut self.surface_config);
        }
    }
}

/// The main window controller.
///
/// Manages the winit event loop, window creation, and GPU context.
pub struct TerminalWindow {
    /// Window configuration.
    config: WindowConfig,
}

impl TerminalWindow {
    /// Create a new terminal window with the given configuration.
    pub fn new(config: WindowConfig) -> Self {
        Self { config }
    }

    /// Run the application event loop.
    ///
    /// This creates the window, initializes the GPU, and runs the
    /// event loop until the window is closed.
    ///
    /// # Errors
    ///
    /// Returns an error if the window or GPU cannot be created.
    pub fn run<F>(self, on_frame: F) -> Result<(), String>
    where
        F: Fn(&mut AppHandle) + Send + 'static,
    {
        // Create the event loop
        let event_loop =
            EventLoop::new().map_err(|e| format!("Failed to create event loop: {e}"))?;

        // Initialize GPU base before event loop (critical for Wayland)
        let wgpu_instance =
            std::sync::Arc::new(wgpu::Instance::new(wgpu::InstanceDescriptor::default()));
        let gpu_base = pollster::block_on(GpuContextBase::new(Arc::clone(&wgpu_instance)))
            .map_err(|e| format!("Failed to create GPU base: {e}"))?;

        // Shared state across event loop callbacks
        let state = std::sync::Mutex::new(AppInner {
            on_frame: Box::new(on_frame),
            window: None,
            gpu: None,
            gpu_base,
            surface_config: SurfaceConfiguration {
                width: self.config.width,
                height: self.config.height,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: vec![],
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: 2,
            },
        });

        // Run the event loop
        event_loop
            .run_app(&mut App { state })
            .map_err(|e| format!("Event loop error: {e}"))?;

        Ok(())
    }

    /// Get the window configuration.
    pub fn config(&self) -> &WindowConfig {
        &self.config
    }
}

/// Internal state for the application.
struct AppInner {
    /// Frame callback.
    on_frame: Box<dyn Fn(&mut AppHandle) + Send>,
    /// The winit window.
    window: Option<Arc<Window>>,
    /// GPU context.
    gpu: Option<Arc<GpuContext>>,
    /// GPU base context (device/queue, created before event loop).
    gpu_base: GpuContextBase,
    /// Surface configuration.
    surface_config: SurfaceConfiguration,
}

impl AppInner {
    /// Configure the surface for the current GPU.
    fn configure(&mut self) {
        if let Some(gpu) = self.gpu.as_ref() {
            gpu.configure_surface(&mut self.surface_config);
        }
    }

    /// Handle a resize event.
    fn on_resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            if let Some(gpu) = self.gpu.as_ref() {
                self.surface_config.width = width;
                self.surface_config.height = height;
                gpu.configure_surface(&mut self.surface_config);
            }
        }
    }

    /// Build an `AppHandle` from current state.
    fn build_handle(&self) -> Option<AppHandle> {
        let (window, gpu) = (self.window.as_ref()?, self.gpu.as_ref()?);
        Some(AppHandle {
            window: Arc::clone(window),
            gpu: Arc::clone(gpu),
            surface_config: self.surface_config.clone(),
        })
    }
}

/// Internal app state for the event loop.
struct App {
    state: std::sync::Mutex<AppInner>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let mut state = self.state.lock().unwrap();

        // Create the window
        let window_attrs = WindowAttributes::default()
            .with_title("Terminal")
            .with_inner_size(LogicalSize::new(
                f64::from(state.surface_config.width),
                f64::from(state.surface_config.height),
            ))
            .with_min_inner_size(LogicalSize::new(
                f64::from(state.surface_config.width.min(320)),
                f64::from(state.surface_config.height.min(240)),
            ));
        let window = event_loop
            .create_window(window_attrs)
            .expect("Failed to create window");
        state.window = Some(Arc::new(window));

        // Create full GPU context from base (surface creation is synchronous)
        if let Some(window) = state.window.as_ref() {
            match GpuContext::from_base(window, &state.gpu_base) {
                Ok(gpu) => {
                    state.gpu = Some(Arc::new(gpu));
                    state.configure();
                }
                Err(e) => {
                    tracing::error!("Failed to create GPU context: {}", e);
                }
            }
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                // Exit the event loop
                std::process::exit(0);
            }
            WindowEvent::Resized(size) => {
                let mut state = self.state.lock().unwrap();
                state.on_resize(size.width, size.height);
            }
            WindowEvent::Destroyed => {
                let mut state = self.state.lock().unwrap();
                state.window = None;
            }
            WindowEvent::RedrawRequested => {
                // Lock state, build handle, call frame callback, present
                let window = {
                    let mut state = self.state.lock().unwrap();
                    let Some(mut handle) = state.build_handle() else {
                        return;
                    };

                    // Call frame callback
                    (state.on_frame)(&mut handle);

                    // Present before moving surface_config
                    if let Err(e) = handle.present() {
                        tracing::debug!("Present error (surface possibly resized): {}", e);
                    }

                    // Update surface config back into state
                    state.surface_config = handle.surface_config;

                    // Return window for request_redraw
                    Arc::clone(state.window.as_ref().unwrap())
                }; // state lock released

                // Request next frame
                window.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        // Request redraw for next frame to keep the render loop going
        let window = {
            let state = self.state.lock().unwrap();
            state.window.as_ref().map(Arc::clone)
        };
        if let Some(w) = window {
            w.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_config_default() {
        let config = WindowConfig::default();
        assert_eq!(config.title, "Terminal");
        assert_eq!(config.width, 1024);
        assert_eq!(config.height, 768);
        assert_eq!(config.min_width, 320);
        assert_eq!(config.min_height, 240);
    }

    #[test]
    fn test_window_config_custom() {
        let config = WindowConfig {
            title: "My Terminal".to_string(),
            width: 800,
            height: 600,
            min_width: 400,
            min_height: 300,
        };
        assert_eq!(config.title, "My Terminal");
        assert_eq!(config.width, 800);
        assert_eq!(config.height, 600);
    }

    #[test]
    fn test_terminal_window_creation() {
        let config = WindowConfig::default();
        let window = TerminalWindow::new(config);
        assert_eq!(window.config().width, 1024);
        assert_eq!(window.config().height, 768);
    }
}
