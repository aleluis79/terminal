//! Render a captured byte stream to a PPM image for visual inspection.
//!
//! Usage: cargo run -p terminal --example `render_capture` -- <capture.bin> <out.ppm> [rows cols]
//!
//! Feeds the bytes through `TerminalEngine` and draws the resulting grid with
//! the real `TerminalRenderer` into a headless texture, so the GPU output of a
//! TUI session can be inspected without a display (see plan.md, RF-44 to RF-51).

use std::io::Write;
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
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .expect("usage: render_capture <in> <out.ppm> [rows cols]");
    let out_path = args
        .get(2)
        .expect("usage: render_capture <in> <out.ppm> [rows cols]");
    let rows: usize = args.get(3).map_or(40, |v| v.parse().unwrap());
    let cols: usize = args.get(4).map_or(120, |v| v.parse().unwrap());

    let bytes = std::fs::read(path).expect("read capture");
    let mut engine = terminal_core::engine::TerminalEngine::new(rows, cols);
    engine.input(&bytes);

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
    let (cell_w, cell_h) = renderer.cell_size();
    let width = cell_w * cols as u32;
    let height = cell_h * rows as u32;

    let grid = &engine.buffer().active().grid;
    let mut cells = Vec::with_capacity(rows * cols);
    for row in 0..rows {
        for col in 0..cols {
            let cell = grid.get(row, col);
            cells.push(CellContent {
                ch: cell.c,
                style: cell.style,
                width: cell.width,
            });
        }
    }

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

    let (cursor_row, cursor_col) = engine.buffer().active().cursor();
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
        cursor_row,
        cursor_col,
        cursor_shape: CursorShape::Block,
        cursor_visible: false,
        selection: Selection::default(),
    }];

    let content = RenderContent {
        panes: &panes,
        background_color: pack_rgba(0x0A, 0x0A, 0x0A, 0xFF),
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

    let unpadded = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let bytes_per_row = unpadded.div_ceil(align) * align;
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

    let mut ppm = Vec::with_capacity((width * height * 3) as usize + 32);
    write!(ppm, "P6\n{width} {height}\n255\n").unwrap();
    for row in data.chunks_exact(bytes_per_row as usize) {
        for px in row[..unpadded as usize].chunks_exact(4) {
            ppm.extend_from_slice(&px[..3]);
        }
    }
    std::fs::write(out_path, &ppm).expect("write ppm");
    println!("wrote {out_path} ({width}x{height}, cell {cell_w}x{cell_h})");
}
