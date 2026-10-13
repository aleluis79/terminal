#![allow(missing_docs)]
//! Test: winit window with wgpu initialization

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowAttributes;

struct GpuTestApp;

impl ApplicationHandler for GpuTestApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        println!("Creating window...");

        let window = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("WGPU Test Window")
                    .with_inner_size(winit::dpi::LogicalSize::new(400.0, 300.0))
                    .with_visible(true)
                    .with_active(true),
            )
            .expect("Failed to create window");

        println!("Window created: {:?}", window.id());

        // Try to initialize wgpu (like the main app does)
        println!("Initializing wgpu...");
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(&window)
            .expect("Failed to create surface");
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("Failed to request adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
                .expect("Failed to request device");

        println!("WGPU initialized");

        // Configure surface
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: 400,
            height: 300,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        println!("Surface configured");

        // Render a frame
        if let Ok(frame) = surface.get_current_texture() {
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("test_frame"),
            });

            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("test_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLUE),
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

            println!("Frame rendered and presented");
        }

        println!("Window should be visible with blue background");
        println!("Waiting 10 seconds...");
        std::thread::sleep(std::time::Duration::from_secs(10));

        event_loop.exit();
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if let WindowEvent::CloseRequested = event {
            println!("Close requested");
            std::process::exit(0);
        }
    }
}

fn main() {
    println!("Starting WGPU test...");
    let event_loop = winit::event_loop::EventLoop::new().expect("Failed to create event loop");
    println!("Event loop created");
    event_loop
        .run_app(&mut GpuTestApp)
        .expect("Event loop error");
    println!("Event loop exited");
}
