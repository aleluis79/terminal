#![allow(missing_docs)]
//! Minimal test: winit + wgpu without blocking

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowAttributes;

struct MinimalApp {
    window: Option<std::sync::Arc<winit::window::Window>>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    surface: Option<wgpu::Surface<'static>>,
    frame_count: u32,
}

impl MinimalApp {
    fn new() -> Self {
        MinimalApp {
            window: None,
            device: None,
            queue: None,
            surface: None,
            frame_count: 0,
        }
    }
}

impl ApplicationHandler for MinimalApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        println!("[MINIMAL] Resumed event");

        let window = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("Minimal Test")
                    .with_inner_size(winit::dpi::LogicalSize::new(400.0, 300.0))
                    .with_visible(true)
                    .with_active(true),
            )
            .expect("Failed to create window");

        self.window = Some(std::sync::Arc::new(window));
        println!("[MINIMAL] Window created");
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match &event {
            WindowEvent::RedrawRequested => {
                self.frame_count += 1;
                println!("[MINIMAL] RedrawRequested #{}", self.frame_count);

                if let (Some(_window), Some(device), Some(queue), Some(surface)) =
                    (&self.window, &self.device, &self.queue, &self.surface)
                {
                    if let Ok(frame) = surface.get_current_texture() {
                        let view = frame
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());
                        let mut encoder =
                            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("frame"),
                            });

                        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });

                        drop(pass);
                        queue.submit(std::iter::once(encoder.finish()));
                        frame.present();

                        if self.frame_count >= 5 {
                            println!("[MINIMAL] Presented 5 frames, exiting");
                            event_loop.exit();
                        }
                    }
                }
            }
            WindowEvent::CloseRequested => {
                println!("[MINIMAL] Close requested");
                std::process::exit(0);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.frame_count == 0 {
            println!("[MINIMAL] about_to_wait, requesting first redraw");
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

fn main() {
    println!("[MINIMAL] Starting...");

    // Initialize wgpu first (outside event loop)
    let _instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    println!("[MINIMAL] WGPU instance created");

    let event_loop = winit::event_loop::EventLoop::new().expect("Failed to create event loop");
    println!("[MINIMAL] Event loop created");

    let mut app = MinimalApp::new();

    // Create window and initialize wgpu in resumed event
    event_loop.run_app(&mut app).expect("Event loop error");

    println!("[MINIMAL] Event loop exited");
}
