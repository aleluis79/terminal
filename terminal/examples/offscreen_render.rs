//! Offscreen render smoke test.
//!
//! Renders a small grid to a headless texture (no window needed) and reads the
//! pixels back, reporting how many differ from the background. Useful for
//! verifying the render pipeline on a machine with a GPU:
//!
//! ```sh
//! cargo run -p terminal --example offscreen_render
//! ```
use std::sync::Arc;

use terminal_render::color::pack_rgba;
use terminal_render::font::FontConfig;
use terminal_render::renderer::{
    CellContent, CursorShape, GridPane, PaneRect, RenderContent, Selection, TerminalRenderer,
};

#[expect(
    clippy::too_many_lines,
    reason = "Linear offscreen setup + render + readback"
)]
fn main() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("no adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
        },
        None,
    ))
    .expect("no device");
    let device = Arc::new(device);
    let queue = Arc::new(queue);

    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = TerminalRenderer::new(&device, &queue, &FontConfig::default(), format);

    let width = 256u32;
    let height = 120u32;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let cols = 8;
    let rows = 2;
    let plain = terminal_core::cell::CellStyle::default();
    let mut cells = vec![
        CellContent {
            ch: ' ',
            style: plain,
            width: 1,
        };
        cols * rows
    ];
    let mut bright = plain;
    bright.fg = terminal_core::cell::Color::Named(15);
    for (i, ch) in "Hi!".chars().enumerate() {
        cells[i].ch = ch;
        cells[i].style = bright;
    }
    cells[5].ch = 'X';
    cells[5].style.bg = terminal_core::cell::Color::Named(1); // red background
    cells[6].ch = 'B';
    cells[6].style.bold = true; // synthetic bold
    cells[7].ch = 'U';
    cells[7].style.underline = true;

    let panes = [GridPane {
        cells: &cells,
        rows,
        cols,
        rect: PaneRect {
            x: 0,
            y: 0,
            width,
            height,
        },
        cursor_row: 0,
        cursor_col: 3,
        cursor_shape: CursorShape::Block,
        cursor_visible: true,
        selection: Selection::default(),
    }];

    let content = RenderContent {
        panes: &panes,
        background_color: pack_rgba(0x10, 0x10, 0x20, 0xFF),
        tab_bar: None,
        dividers: &[],
        divider_color: pack_rgba(0x60, 0x60, 0x70, 0xFF),
    };
    let config = wgpu::SurfaceConfiguration {
        width,
        height,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: vec![],
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        present_mode: wgpu::PresentMode::AutoVsync,
        desired_maximum_frame_latency: 2,
    };
    renderer.render(&view, &config, &content);

    let bytes_per_row = width * 4; // 512, multiple of 256
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(bytes_per_row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range();

    let bg = [0x10u8, 0x10, 0x20];
    let mut non_bg = 0u32;
    let mut has_red = false;
    let mut has_light = false;
    for px in data.as_chunks::<4>().0 {
        if px[0].abs_diff(bg[0]) > 12 || px[1].abs_diff(bg[1]) > 12 || px[2].abs_diff(bg[2]) > 12 {
            non_bg += 1;
        }
        if px[0] > 100 && px[1] < 80 && px[2] < 80 {
            has_red = true;
        }
        if px[0] > 150 && px[1] > 150 && px[2] > 150 {
            has_light = true;
        }
    }
    println!("non-background pixels = {non_bg} / {}", width * height);
    println!("has_red_background = {has_red}");
    println!("has_light_glyph_pixels = {has_light}");
}
