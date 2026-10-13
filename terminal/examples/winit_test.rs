#![allow(missing_docs)]
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowAttributes;

struct SimpleApp;

impl ApplicationHandler for SimpleApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("Test Window")
                    .with_inner_size(winit::dpi::LogicalSize::new(400.0, 300.0))
                    .with_visible(true)
                    .with_active(true),
            )
            .expect("Failed to create window");

        println!("Window created: {:?}", window.id());

        // Keep window alive for 5 seconds
        std::thread::sleep(std::time::Duration::from_secs(5));

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
    println!("Starting winit test...");
    let event_loop = winit::event_loop::EventLoop::new().expect("Failed to create event loop");
    println!("Event loop created");
    event_loop
        .run_app(&mut SimpleApp)
        .expect("Event loop error");
    println!("Event loop exited");
}
