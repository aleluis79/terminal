//! Main renderer: grid → glyph atlas → GPU draw calls.
//!
//! This module handles:
//! - Rendering the terminal grid to the GPU (colored text, per-cell backgrounds)
//! - Cursor rendering (block, underline, beam)
//! - Selection highlight
//! - Resize handling
//!
//! Colors passed as [`Color`] values are RGBA-ordered
//! (`r << 24 | g << 16 | b << 8 | a`), matching [`TerminalPalette`] entries.

#![expect(
    clippy::cast_precision_loss,
    reason = "Render math operates on f32 screen coordinates"
)]
#![expect(
    clippy::cast_sign_loss,
    reason = "Font metrics and layout values are non-negative"
)]

use std::collections::HashMap;
use std::sync::Arc;
use wgpu::{
    Buffer, BufferAddress, BufferUsages, Device, Queue, SurfaceConfiguration, TextureView,
    VertexBufferLayout, VertexFormat,
};

use crate::atlas::{AtlasRegion, GlyphAtlas};
use crate::color::{pack_rgba, unpack_rgba, Color, TerminalPalette};
use crate::font::{FontConfig, FontHandle};
use crate::glyph::{GlyphRenderer, RasterGlyph};
use terminal_core::cell::{CellStyle, Color as TerminalColor};

/// Number of terminal cells the vertex/index buffers are sized for.
///
/// Each cell can produce up to two quads (background + glyph), plus a handful
/// for the cursor and selection.
const MAX_CELLS: usize = 20_000;

/// Fraction of a pixel by which a tiling glyph's ink may fall short of a cell
/// edge before it is snapped onto it (see [`fit_tiling_glyph`]).
const TILING_TOLERANCE: f32 = 1.0;

/// Selection state for mouse-driven text selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    /// Start position (row, col).
    pub start: Option<(usize, usize)>,
    /// End position (row, col).
    pub end: Option<(usize, usize)>,
}

impl Selection {
    /// Check if there is an active selection.
    pub fn is_active(&self) -> bool {
        self.start.is_some() && self.end.is_some()
    }

    /// Get the selection bounds (min row, min col, max row, max col).
    pub fn bounds(&self) -> Option<(usize, usize, usize, usize)> {
        let (sr, sc) = self.start?;
        let (er, ec) = self.end?;
        Some((sr.min(er), sc.min(ec), sr.max(er), sc.max(ec)))
    }
}

/// Cursor shape for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// Block cursor (replaces the character at cursor position).
    #[default]
    Block,
    /// Underline cursor (draws a line below the character).
    Underline,
    /// Beam cursor (vertical line at cursor position).
    Beam,
}

/// Cell content from the terminal grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellContent {
    /// Character to render.
    pub ch: char,
    /// Colours and text attributes for the cell.
    pub style: CellStyle,
}

/// A tab label drawn in the tab bar.
#[derive(Debug, Clone, Copy)]
pub struct TabLabel<'a> {
    /// Title text.
    pub title: &'a str,
    /// Left edge in pixels.
    pub x: u32,
    /// Width in pixels.
    pub width: u32,
    /// Whether this tab is the active one.
    pub active: bool,
}

/// Tab bar drawn above the terminal grid.
#[derive(Debug, Clone, Copy)]
pub struct TabBarContent<'a> {
    /// Tabs to draw, left to right.
    pub tabs: &'a [TabLabel<'a>],
    /// Bar height in pixels.
    pub height: u32,
    /// Cell width in pixels (advance used for title glyphs).
    pub cell_width: u32,
}

/// Pixel rectangle of a pane within the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaneRect {
    /// Left edge in pixels.
    pub x: u32,
    /// Top edge in pixels.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// A terminal grid pane to draw.
pub struct GridPane<'a> {
    /// The pane's cells, row-major.
    pub cells: &'a [CellContent],
    /// Number of rows.
    pub rows: usize,
    /// Number of columns.
    pub cols: usize,
    /// Pixel rectangle of the pane.
    pub rect: PaneRect,
    /// Cursor row position within the pane.
    pub cursor_row: usize,
    /// Cursor column position within the pane.
    pub cursor_col: usize,
    /// Cursor shape.
    pub cursor_shape: CursorShape,
    /// Whether to draw the cursor (only the focused pane does).
    pub cursor_visible: bool,
    /// Current selection within the pane.
    pub selection: Selection,
}

/// Renderable terminal content.
pub struct RenderContent<'a> {
    /// Panes to draw, back to front.
    pub panes: &'a [GridPane<'a>],
    /// Background color (RGBA-ordered).
    pub background_color: Color,
    /// Tab bar to draw above the panes, if any.
    pub tab_bar: Option<TabBarContent<'a>>,
}

/// A contiguous range of indices to draw, optionally clipped to a rectangle.
#[derive(Debug, Clone, Copy)]
struct DrawBatch {
    /// First index in the index buffer.
    first_index: u32,
    /// Number of indices in the batch.
    index_count: u32,
    /// Scissor rectangle in pixels, if the batch must be clipped.
    scissor: Option<PaneRect>,
}

/// Vertex for a quad in the screen-space render.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct QuadVertex {
    /// X position in pixels.
    x: f32,
    /// Y position in pixels.
    y: f32,
    /// U texture coordinate.
    u: f32,
    /// V texture coordinate.
    v: f32,
    /// Red color component.
    r: f32,
    /// Green color component.
    g: f32,
    /// Blue color component.
    b: f32,
    /// Alpha color component.
    a: f32,
}

impl QuadVertex {
    /// Describes the layout of vertices for the GPU.
    fn descriptor<'a>() -> VertexBufferLayout<'a> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<QuadVertex>() as BufferAddress,
            attributes: &[
                wgpu::VertexAttribute {
                    format: VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: VertexFormat::Float32x2,
                    offset: 8,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: 16,
                    shader_location: 2,
                },
            ],
            step_mode: wgpu::VertexStepMode::Vertex,
        }
    }
}

/// Shader source for the terminal renderer.
const SHADER_SOURCE: &str = r"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@group(0) @binding(0) var my_sampler: sampler;
@group(0) @binding(1) var my_texture: texture_2d<f32>;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // R channel holds glyph coverage; the vertex color provides the tint.
    let mask = textureSample(my_texture, my_sampler, in.uv).r;
    return vec4<f32>(in.color.rgb, in.color.a * mask);
}
";

/// A glyph that has been uploaded to the atlas.
#[derive(Debug, Clone, Copy)]
struct CachedGlyph {
    /// Atlas region holding the glyph mask.
    region: AtlasRegion,
    /// Bitmap width in pixels.
    width: f32,
    /// Bitmap height in pixels.
    height: f32,
    /// Horizontal offset from the pen position to the left edge.
    left: f32,
    /// Vertical offset from the baseline (positive up) to the top edge.
    top: f32,
}

/// Identity of a rasterized glyph: a character plus the styles that change its
/// outline, so bold and italic get their own bitmaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    /// The character.
    ch: char,
    /// Whether the glyph is drawn emboldened.
    bold: bool,
    /// Whether the glyph is drawn slanted.
    italic: bool,
}

impl GlyphKey {
    /// Build the key for a cell.
    fn of(ch: char, style: &CellStyle) -> Self {
        Self {
            ch,
            bold: style.bold,
            italic: style.italic,
        }
    }
}

/// Whether `ch` is a line-drawing or block-element character.
///
/// These are meant to tile the monospace grid: the ink of a `─` has to reach
/// the right edge of its cell and that of a `│` the bottom edge, so that
/// neighbouring cells join into an unbroken line.
fn is_tiling_char(ch: char) -> bool {
    matches!(ch, '\u{2500}'..='\u{259F}')
}

/// Snap a tiling glyph's ink onto the cell edges it is about to meet.
///
/// Line-drawing and block characters are meant to tile the monospace grid: the
/// ink of a `─` has to reach the right edge of its cell and that of a `│` the
/// bottom edge, so that neighbouring cells join into an unbroken line. Fonts
/// round the ink box a fraction of a pixel short of the cell, which leaves a
/// one-pixel seam wherever two cells meet — visible as a dotted border.
///
/// Each edge that already sits within [`TILING_TOLERANCE`] of a cell border is
/// nudged onto it, which stretches the glyph by at most a pixel. Edges further
/// away are left alone: they belong to a stroke that genuinely stops there (a
/// corner's stub, a half block, the thin side of a `─`).
///
/// Returns `(width, height, left, top)`, where `top` is measured from the
/// baseline in the coordinate system of [`CachedGlyph`].
fn fit_tiling_glyph(
    ch: char,
    glyph: &RasterGlyph,
    cell_width: u32,
    cell_height: u32,
    ascent: f32,
) -> (f32, f32, f32, f32) {
    // Ink box in cell coordinates: x from the left border, y from the top one.
    let mut left = glyph.left as f32;
    let mut top = ascent - glyph.top as f32;
    let mut right = left + glyph.width as f32;
    let mut bottom = top + glyph.height as f32;

    if is_tiling_char(ch) {
        let cell_w = cell_width as f32;
        let cell_h = cell_height as f32;

        if 0.0 < top && top <= TILING_TOLERANCE {
            top = 0.0;
        }
        if 0.0 < cell_h - bottom && cell_h - bottom <= TILING_TOLERANCE {
            bottom = cell_h;
        }
        if 0.0 < left && left <= TILING_TOLERANCE {
            left = 0.0;
        }
        if 0.0 < cell_w - right && cell_w - right <= TILING_TOLERANCE {
            right = cell_w;
        }
    }

    (right - left, bottom - top, left, ascent - top)
}

/// Main GPU renderer for the terminal.
pub struct TerminalRenderer {
    /// GPU device (wrapped in Arc for sharing).
    device: Arc<Device>,
    /// GPU queue (wrapped in Arc for sharing).
    queue: Arc<Queue>,
    /// Font handle.
    font: FontHandle,
    /// Glyph renderer.
    glyph_renderer: GlyphRenderer,
    /// Glyph texture atlas.
    atlas: GlyphAtlas,
    /// Terminal color palette.
    palette: TerminalPalette,
    /// Palette expanded to a 256-entry lookup table.
    palette_array: [Color; 256],
    /// WGPU shader module (used during pipeline creation).
    #[expect(dead_code)]
    shader: Arc<wgpu::ShaderModule>,
    /// WGPU sampler (linear filtering for smooth text).
    #[expect(dead_code)]
    sampler: Arc<wgpu::Sampler>,
    /// Render pipeline.
    pipeline: Arc<wgpu::RenderPipeline>,
    /// Vertex buffer (reused across frames).
    vertex_buffer: Buffer,
    /// Index buffer (reused across frames).
    index_buffer: Buffer,
    /// Number of vertices in the vertex buffer.
    vertex_count: u32,
    /// Number of indices in the index buffer.
    index_count: u32,
    /// Maximum vertices the vertex buffer can hold.
    vertex_capacity: usize,
    /// Maximum indices the index buffer can hold.
    index_capacity: usize,
    /// Binding group for the pipeline.
    bind_group: Arc<wgpu::BindGroup>,
    /// Cell dimensions (computed from font metrics).
    cell_width: u32,
    cell_height: u32,
    /// Distance from the top of a cell to the text baseline.
    ascent: f32,
    /// Cursor color as normalized RGBA.
    cursor_color: [f32; 4],
    /// Glyph cache: character → atlas entry (or `None` if unrenderable).
    /// Rasterized glyphs, keyed by character and style.
    glyph_cache: HashMap<GlyphKey, Option<CachedGlyph>>,
}

impl TerminalRenderer {
    /// Create a new terminal renderer.
    ///
    /// # Arguments
    ///
    /// * `device` - WGPU device (wrapped in Arc).
    /// * `queue` - WGPU queue (wrapped in Arc).
    /// * `font_config` - Font configuration.
    /// * `target_format` - Surface texture format the pipeline renders to.
    #[expect(
        clippy::too_many_lines,
        reason = "Linear sequence of wgpu resource creation"
    )]
    pub fn new(
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        font_config: &FontConfig,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let font = FontHandle::new(font_config);
        let font_size = font.pixel_size();
        let glyph_renderer = GlyphRenderer::new(font_size);

        // Compute cell dimensions from font metrics
        let metrics = font.metrics();
        let ascent = metrics.ascent;
        let cell_width = (metrics.advance_width.ceil() as u32).max(1);
        let cell_height =
            ((metrics.ascent + metrics.descent + metrics.line_gap).ceil() as u32).max(1);

        let atlas = GlyphAtlas::new(device, queue);

        // Create shader module
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terminal_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SOURCE.into()),
        });

        // Create sampler (linear filtering for smooth text)
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("terminal_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terminal_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        // Create bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terminal_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(atlas.texture_view()),
                },
            ],
        });

        // Create render pipeline
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terminal_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terminal_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[QuadVertex::descriptor()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            // Preserve the cleared alpha so the window keeps a
                            // uniform opacity no matter what is drawn over it.
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        // Pre-allocate vertex and index buffers.
        let vertex_capacity = MAX_CELLS * 2 * 4; // 2 quads/cell, 4 vertices each
        let index_capacity = MAX_CELLS * 2 * 6; // 2 quads/cell, 6 indices each

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terminal_vertex_buffer"),
            size: (vertex_capacity * std::mem::size_of::<QuadVertex>()) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terminal_index_buffer"),
            size: (index_capacity * std::mem::size_of::<u32>()) as u64,
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let palette = TerminalPalette::default();
        let palette_array = palette.as_array();

        Self {
            device: Arc::clone(device),
            queue: Arc::clone(queue),
            font,
            glyph_renderer,
            atlas,
            palette,
            palette_array,
            shader: Arc::new(shader),
            sampler: Arc::new(sampler),
            pipeline: Arc::new(pipeline),
            vertex_buffer,
            index_buffer,
            vertex_count: 0,
            index_count: 0,
            vertex_capacity,
            index_capacity,
            bind_group: Arc::new(bind_group),
            cell_width,
            cell_height,
            ascent,
            cursor_color: [0.75, 0.75, 0.95, 0.6],
            glyph_cache: HashMap::new(),
        }
    }

    /// Render the terminal grid to a texture view.
    ///
    /// Renders, in order: background (cleared to the configured color), each
    /// cell's background and character, the selection highlight, and the cursor.
    ///
    /// # Arguments
    ///
    /// * `view` - The texture view to render to.
    /// * `config` - Surface configuration (used for the viewport size).
    /// * `content` - The terminal content to render.
    pub fn render(
        &mut self,
        view: &TextureView,
        config: &SurfaceConfiguration,
        content: &RenderContent,
    ) {
        // Rasterize and upload any glyphs not yet in the atlas.
        self.upload_glyphs(content);

        let viewport = (config.width.max(1), config.height.max(1));

        // Build all geometry for this frame (in pixel coordinates).
        let (mut vertices, indices, batches) = self.build_geometry(content, viewport);

        // Convert pixel coordinates to normalized device coordinates.
        let viewport_w = viewport.0 as f32;
        let viewport_h = viewport.1 as f32;
        for vertex in &mut vertices {
            vertex.x = vertex.x / viewport_w * 2.0 - 1.0;
            vertex.y = 1.0 - vertex.y / viewport_h * 2.0;
        }

        self.vertex_count = vertices.len() as u32;
        self.index_count = indices.len() as u32;

        if !vertices.is_empty() {
            self.queue
                .write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        }
        if !indices.is_empty() {
            self.queue
                .write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(&indices));
        }

        let bg = unpack_rgba(content.background_color);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("terminal_render"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terminal_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(bg[0]),
                            g: f64::from(bg[1]),
                            b: f64::from(bg[2]),
                            // Alpha carries the window opacity.
                            a: f64::from(bg[3]),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &*self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);

            for batch in &batches {
                if batch.index_count == 0 {
                    continue;
                }

                let (x, y, w, h) = match batch.scissor {
                    Some(rect) => (rect.x, rect.y, rect.width.max(1), rect.height.max(1)),
                    None => (0, 0, viewport.0, viewport.1),
                };

                // Keep the scissor inside the render target.
                let x = x.min(viewport.0);
                let y = y.min(viewport.1);
                let w = w.min(viewport.0 - x);
                let h = h.min(viewport.1 - y);
                if w == 0 || h == 0 {
                    continue;
                }

                pass.set_scissor_rect(x, y, w, h);
                pass.draw_indexed(
                    batch.first_index..batch.first_index + batch.index_count,
                    0,
                    0..1,
                );
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Rasterize and upload glyphs for every unique character on screen.
    fn upload_glyphs(&mut self, content: &RenderContent) {
        for pane in content.panes {
            for cell in pane.cells {
                self.upload_glyph(GlyphKey::of(cell.ch, &cell.style));
            }
        }

        if let Some(bar) = &content.tab_bar {
            for tab in bar.tabs {
                for ch in tab.title.chars() {
                    self.upload_glyph(GlyphKey {
                        ch,
                        bold: false,
                        italic: false,
                    });
                }
            }
        }
    }

    /// Rasterize and upload a single glyph if it is not cached yet.
    fn upload_glyph(&mut self, key: GlyphKey) {
        if key.ch == ' ' || key.ch == '\0' || self.glyph_cache.contains_key(&key) {
            return;
        }

        let cached = match self
            .glyph_renderer
            .rasterize(&self.font, key.ch, key.bold, key.italic)
        {
            Some(glyph) if glyph.width > 0 && glyph.height > 0 => {
                let (width, height, left, top) = fit_tiling_glyph(
                    key.ch,
                    &glyph,
                    self.cell_width,
                    self.cell_height,
                    self.ascent,
                );
                self.atlas
                    .allocate(glyph.width, glyph.height, &self.queue)
                    .map(|region| {
                        self.atlas.upload(&region, &glyph.data, &self.queue);
                        CachedGlyph {
                            region,
                            width,
                            height,
                            left,
                            top,
                        }
                    })
            }
            _ => None,
        };

        self.glyph_cache.insert(key, cached);
    }

    /// Build the vertex and index geometry for the whole frame.
    fn build_geometry(
        &self,
        content: &RenderContent,
        viewport: (u32, u32),
    ) -> (Vec<QuadVertex>, Vec<u32>, Vec<DrawBatch>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut batches = Vec::new();

        for pane in content.panes {
            let first_index = indices.len() as u32;
            self.push_pane(pane, content.background_color, &mut vertices, &mut indices);
            batches.push(DrawBatch {
                first_index,
                index_count: indices.len() as u32 - first_index,
                scissor: Some(pane.rect),
            });
        }

        if let Some(bar) = &content.tab_bar {
            let bar_width = content
                .panes
                .iter()
                .map(|p| p.rect.x + p.rect.width)
                .max()
                .unwrap_or(viewport.0);
            let first_index = indices.len() as u32;
            self.push_tab_bar(bar, bar_width, &mut vertices, &mut indices);
            batches.push(DrawBatch {
                first_index,
                index_count: indices.len() as u32 - first_index,
                scissor: None,
            });
        }

        (vertices, indices, batches)
    }

    /// Push one pane: backgrounds, glyphs, attributes, selection and cursor.
    ///
    /// `background` is the terminal background colour, which cells ask for with
    /// the "default background" attribute and which inverse video swaps into.
    fn push_pane(
        &self,
        pane: &GridPane,
        background: Color,
        vertices: &mut Vec<QuadVertex>,
        indices: &mut Vec<u32>,
    ) {
        let cell_w = self.cell_width as f32;
        let cell_h = self.cell_height as f32;
        let origin_x = pane.rect.x as f32;
        let origin_y = pane.rect.y as f32;

        let white = self.atlas.white_region();
        let (left, bottom, right, top) = white.uv(self.atlas.dimensions());
        let white_uv = ((left + right) * 0.5, (bottom + top) * 0.5);
        let solid = (white_uv.0, white_uv.1, white_uv.0, white_uv.1);

        let cols = pane.cols;
        for row in 0..pane.rows {
            for col in 0..cols {
                let idx = row * cols + col;
                let Some(cell) = pane.cells.get(idx) else {
                    continue;
                };
                self.push_cell(
                    cell,
                    (
                        origin_x + col as f32 * cell_w,
                        origin_y + row as f32 * cell_h,
                    ),
                    background,
                    solid,
                    vertices,
                    indices,
                );
            }
        }

        // Selection highlight.
        self.push_selection_highlight(pane, solid, vertices, indices);

        // Cursor (shape depends on [`CursorShape`]).
        if pane.cursor_visible {
            let x = origin_x + pane.cursor_col as f32 * cell_w;
            let y = origin_y + pane.cursor_row as f32 * cell_h;
            let color = self.cursor_color;
            match pane.cursor_shape {
                CursorShape::Block => {
                    self.push_quad(vertices, indices, x, y, cell_w, cell_h, solid, color);
                }
                CursorShape::Underline => {
                    let h = (cell_h * 0.12).max(2.0);
                    self.push_quad(
                        vertices,
                        indices,
                        x,
                        y + cell_h - h,
                        cell_w,
                        h,
                        solid,
                        color,
                    );
                }
                CursorShape::Beam => {
                    let w = (cell_w * 0.15).max(2.0);
                    self.push_quad(vertices, indices, x, y, w, cell_h, solid, color);
                }
            }
        }
    }

    /// Push the quads of a single cell: background, glyph, underline and
    /// strikethrough.
    ///
    /// `origin` is the top-left corner of the cell in pixels and `background`
    /// the terminal background colour, which cells request with the "default
    /// background" attribute and which inverse video swaps into.
    fn push_cell(
        &self,
        cell: &CellContent,
        origin: (f32, f32),
        background: Color,
        solid: (f32, f32, f32, f32),
        vertices: &mut Vec<QuadVertex>,
        indices: &mut Vec<u32>,
    ) {
        let (cell_x, cell_y) = origin;
        let cell_w = self.cell_width as f32;
        let cell_h = self.cell_height as f32;
        let style = cell.style;

        // "Default background" (ANSI 0) means the terminal background.
        let default_bg = style.bg == TerminalColor::Named(0);
        let mut fg = self.cell_color(style.fg);
        let mut bg = if default_bg {
            background
        } else {
            self.cell_color(style.bg)
        };
        if style.inverse {
            std::mem::swap(&mut fg, &mut bg);
        }

        // Per-cell background. Inverse video always paints one, since it turns
        // the foreground colour into the background.
        if style.inverse || !default_bg {
            self.push_quad(
                vertices,
                indices,
                cell_x,
                cell_y,
                cell_w,
                cell_h,
                solid,
                unpack_rgba(bg),
            );
        }

        // Glyph. Hidden text is not drawn at all.
        if !style.hidden && cell.ch != ' ' && cell.ch != '\0' {
            if let Some(Some(glyph)) = self.glyph_cache.get(&GlyphKey::of(cell.ch, &style)) {
                let (uv_left, uv_bottom, uv_right, uv_top) =
                    glyph.region.uv(self.atlas.dimensions());
                self.push_quad(
                    vertices,
                    indices,
                    cell_x + glyph.left,
                    cell_y + self.ascent - glyph.top,
                    glyph.width,
                    glyph.height,
                    (uv_left, uv_bottom, uv_right, uv_top),
                    unpack_rgba(fg),
                );
            }
        }

        // Underline and strikethrough are quads under the glyph, so they work
        // with any font.
        let rule = (cell_h * 0.08).max(1.0);
        if style.underline {
            self.push_quad(
                vertices,
                indices,
                cell_x,
                cell_y + cell_h - rule,
                cell_w,
                rule,
                solid,
                unpack_rgba(fg),
            );
        }
        if style.strikethrough {
            self.push_quad(
                vertices,
                indices,
                cell_x,
                cell_y + cell_h * 0.55,
                cell_w,
                rule,
                solid,
                unpack_rgba(fg),
            );
        }
    }

    /// Push the tab bar (background, tab buttons and titles) above the grid.
    fn push_tab_bar(
        &self,
        bar: &TabBarContent,
        bar_width: u32,
        vertices: &mut Vec<QuadVertex>,
        indices: &mut Vec<u32>,
    ) {
        let bar_h = bar.height as f32;
        let total_w = bar_width as f32;

        let white = self.atlas.white_region();
        let uv = white.uv(self.atlas.dimensions());
        let solid = (
            (uv.0 + uv.2) * 0.5,
            (uv.1 + uv.3) * 0.5,
            (uv.0 + uv.2) * 0.5,
            (uv.1 + uv.3) * 0.5,
        );

        // Bar background.
        self.push_quad(
            vertices,
            indices,
            0.0,
            0.0,
            total_w,
            bar_h,
            solid,
            self.color(0),
        );

        // Vertically centre the title text within the bar.
        let baseline = (bar_h - self.cell_height as f32) * 0.5 + self.ascent;
        let title_w = bar.cell_width as f32;

        for tab in bar.tabs {
            let x = tab.x as f32;
            let w = tab.width as f32;

            // Highlight the active tab.
            let (bg, fg) = if tab.active {
                (self.color(8), self.color(15))
            } else {
                (self.color(0), self.color(7))
            };
            self.push_quad(vertices, indices, x, 0.0, w, bar_h, solid, bg);

            // Centre the title inside the tab.
            let text_w = tab.title.chars().count() as f32 * title_w;
            let mut pen = x + (w - text_w) * 0.5;
            for ch in tab.title.chars() {
                let key = GlyphKey {
                    ch,
                    bold: false,
                    italic: false,
                };
                if let Some(Some(glyph)) = self.glyph_cache.get(&key) {
                    let (uv_left, uv_bottom, uv_right, uv_top) =
                        glyph.region.uv(self.atlas.dimensions());
                    self.push_quad(
                        vertices,
                        indices,
                        pen + glyph.left,
                        baseline - glyph.top,
                        glyph.width,
                        glyph.height,
                        (uv_left, uv_bottom, uv_right, uv_top),
                        fg,
                    );
                }
                pen += title_w;
            }
        }
    }

    /// Push the selection highlight quads for one pane.
    ///
    /// The highlight is linear, matching the text that Ctrl+Shift+C copies:
    /// only the first and last row are partial, the rows in between are
    /// highlighted from edge to edge.
    fn push_selection_highlight(
        &self,
        pane: &GridPane,
        solid: (f32, f32, f32, f32),
        vertices: &mut Vec<QuadVertex>,
        indices: &mut Vec<u32>,
    ) {
        let (Some(start), Some(end)) = (pane.selection.start, pane.selection.end) else {
            return;
        };

        // Order the ends in reading order, so dragging upwards highlights the
        // same cells as dragging downwards.
        let ((r1, c1), (r2, c2)) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };

        let cell_w = self.cell_width as f32;
        let cell_h = self.cell_height as f32;
        let origin_x = pane.rect.x as f32;
        let origin_y = pane.rect.y as f32;
        let last_col = pane.cols.saturating_sub(1);
        let color = [0.25, 0.45, 0.85, 0.35];

        for row in r1..=r2.min(pane.rows.saturating_sub(1)) {
            let first = if row == r1 { c1 } else { 0 };
            let last = (if row == r2 { c2 } else { last_col }).min(last_col);
            if last < first {
                continue;
            }
            self.push_quad(
                vertices,
                indices,
                origin_x + first as f32 * cell_w,
                origin_y + row as f32 * cell_h,
                (last - first + 1) as f32 * cell_w,
                cell_h,
                solid,
                color,
            );
        }
    }

    /// Append a single textured, colored quad.
    #[expect(
        clippy::too_many_arguments,
        reason = "Geometry helper: position, size, UV and color"
    )]
    fn push_quad(
        &self,
        vertices: &mut Vec<QuadVertex>,
        indices: &mut Vec<u32>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        uv: (f32, f32, f32, f32),
        color: [f32; 4],
    ) {
        // Guard against overflowing the pre-allocated buffers.
        if vertices.len() + 4 > self.vertex_capacity || indices.len() + 6 > self.index_capacity {
            return;
        }

        let base = vertices.len() as u32;
        let (left, top, right, bottom) = uv;

        vertices.push(QuadVertex {
            x,
            y,
            u: left,
            v: top,
            r: color[0],
            g: color[1],
            b: color[2],
            a: color[3],
        });
        vertices.push(QuadVertex {
            x: x + w,
            y,
            u: right,
            v: top,
            r: color[0],
            g: color[1],
            b: color[2],
            a: color[3],
        });
        vertices.push(QuadVertex {
            x,
            y: y + h,
            u: left,
            v: bottom,
            r: color[0],
            g: color[1],
            b: color[2],
            a: color[3],
        });
        vertices.push(QuadVertex {
            x: x + w,
            y: y + h,
            u: right,
            v: bottom,
            r: color[0],
            g: color[1],
            b: color[2],
            a: color[3],
        });

        indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
    }

    /// Resolve a cell colour to a packed RGBA value.
    ///
    /// Palette indices (both the 16 ANSI ones and the 256-colour table) are
    /// looked up in the palette; 24-bit RGB is used as-is.
    fn cell_color(&self, color: TerminalColor) -> Color {
        match color {
            TerminalColor::Named(index) | TerminalColor::Palette(index) => {
                self.palette_array[index as usize]
            }
            TerminalColor::Rgb { r, g, b } => pack_rgba(r, g, b, 0xFF),
        }
    }

    /// Look up a palette color as normalized RGBA.
    fn color(&self, index: u8) -> [f32; 4] {
        unpack_rgba(self.palette_array[index as usize])
    }

    /// Update the surface configuration (called on resize).
    pub fn resize(&mut self, width: u32, height: u32) {
        // The atlas grows automatically and geometry is rebuilt each frame.
        let _ = (width, height);
    }

    /// Get the font handle.
    pub fn font(&self) -> &FontHandle {
        &self.font
    }

    /// Get the cell size in pixels (`width`, `height`).
    pub fn cell_size(&self) -> (u32, u32) {
        (self.cell_width, self.cell_height)
    }

    /// Get the glyph atlas.
    pub fn atlas(&self) -> &GlyphAtlas {
        &self.atlas
    }

    /// Set the color palette.
    pub fn set_palette(&mut self, palette: TerminalPalette) {
        self.palette = palette;
        self.palette_array = palette.as_array();
    }

    /// Get the current palette.
    pub fn palette(&self) -> &TerminalPalette {
        &self.palette
    }

    /// Set the cursor color (normalized RGBA).
    pub fn set_cursor_color(&mut self, color: [f32; 4]) {
        self.cursor_color = color;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_default() {
        let sel = Selection::default();
        assert!(!sel.is_active());
        assert!(sel.bounds().is_none());
    }

    #[test]
    fn test_selection_active() {
        let sel = Selection {
            start: Some((0, 0)),
            end: Some((5, 10)),
        };
        assert!(sel.is_active());
        assert_eq!(sel.bounds(), Some((0, 0, 5, 10)));
    }

    #[test]
    fn test_selection_reversed() {
        let sel = Selection {
            start: Some((5, 10)),
            end: Some((0, 0)),
        };
        assert!(sel.is_active());
        assert_eq!(sel.bounds(), Some((0, 0, 5, 10)));
    }

    #[test]
    fn test_cursor_shape_default() {
        let shape = CursorShape::default();
        assert_eq!(shape, CursorShape::Block);
    }

    /// Build a raster glyph with the given ink box (no bitmap data needed).
    fn glyph(width: u32, height: u32, left: i32, top: i32) -> RasterGlyph {
        RasterGlyph {
            glyph_id: 1,
            width,
            height,
            left,
            top,
            data: Vec::new(),
        }
    }

    /// Assert a fitted glyph box equals the expected one (within a pixel).
    fn assert_box(actual: (f32, f32, f32, f32), expected: (f32, f32, f32, f32)) {
        let actual = [actual.0, actual.1, actual.2, actual.3];
        let expected = [expected.0, expected.1, expected.2, expected.3];
        let labels = ["width", "height", "left", "top"];
        for (i, label) in labels.iter().enumerate() {
            assert!(
                (actual[i] - expected[i]).abs() < 0.01,
                "{label}: expected {}, got {}",
                expected[i],
                actual[i]
            );
        }
    }

    #[test]
    fn test_fit_tiling_glyph_vertical_line_fills_cell_height() {
        // Regression: Hack Nerd Font draws `│` 21px tall in a 22px cell, which
        // left a one-pixel seam between rows (box borders looked dotted).
        // Real ink box: 5x21, left=3, top=17, cell 12x22, ascent 17.3.
        assert_box(
            fit_tiling_glyph('│', &glyph(5, 21, 3, 17), 12, 22, 17.3),
            (5.0, 22.0, 3.0, 17.3),
        );
    }

    #[test]
    fn test_fit_tiling_glyph_corner_reaches_next_row() {
        // `┌`'s stub stops a fraction of a pixel above the cell bottom; it must
        // reach it to join the `│` in the row below.
        assert_box(
            fit_tiling_glyph('┌', &glyph(10, 13, 3, 9), 12, 22, 17.3),
            (10.0, 13.7, 3.0, 9.0),
        );
        // `┴`'s stub stops just below the cell top; it must reach it.
        assert_box(
            fit_tiling_glyph('┴', &glyph(15, 13, -2, 17), 12, 22, 17.3),
            (15.0, 13.3, -2.0, 17.3),
        );
    }

    #[test]
    fn test_fit_tiling_glyph_half_blocks_reach_their_cell_edge() {
        // The upper half block starts at the cell top, the lower one ends at
        // its bottom, so stacked halves leave no seam.
        assert_box(
            fit_tiling_glyph('▀', &glyph(14, 11, -1, 17), 12, 22, 17.3),
            (14.0, 11.3, -1.0, 17.3),
        );
        assert_box(
            fit_tiling_glyph('▄', &glyph(14, 11, -1, 7), 12, 22, 17.3),
            (14.0, 11.7, -1.0, 7.0),
        );
    }

    #[test]
    fn test_fit_tiling_glyph_horizontal_line_keeps_thickness() {
        // `─` already overlaps its neighbours horizontally (15px in a 12px
        // cell); stretching it would only make the line thicker.
        assert_box(
            fit_tiling_glyph('─', &glyph(15, 3, -2, 8), 12, 22, 17.3),
            (15.0, 3.0, -2.0, 8.0),
        );
    }

    #[test]
    fn test_fit_tiling_glyph_leaves_far_edges_alone() {
        // A stroke that stops well short of an edge is a design choice (a
        // shorter line) and must keep its size.
        assert_box(
            fit_tiling_glyph('│', &glyph(5, 20, 3, 17), 12, 22, 17.3),
            (5.0, 20.3, 3.0, 17.3),
        );
        assert_box(
            fit_tiling_glyph('┐', &glyph(10, 13, -2, 9), 12, 22, 17.3),
            (10.0, 13.7, -2.0, 9.0),
        );
    }

    #[test]
    fn test_fit_tiling_glyph_ignores_ordinary_characters() {
        assert_box(
            fit_tiling_glyph('M', &glyph(13, 14, -1, 14), 12, 22, 17.3),
            (13.0, 14.0, -1.0, 14.0),
        );
    }

    #[test]
    fn test_fit_tiling_glyph_widens_a_short_horizontal_line() {
        // A font whose `─` is one pixel narrower than the cell would leave
        // horizontal seams; it gets stretched to the cell width.
        assert_box(
            fit_tiling_glyph('─', &glyph(11, 3, 0, 8), 12, 22, 17.3),
            (12.0, 3.0, 0.0, 8.0),
        );
    }
}
