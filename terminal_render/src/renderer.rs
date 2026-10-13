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
use crate::color::{unpack_rgba, Color, TerminalPalette};
use crate::font::{FontConfig, FontHandle};
use crate::glyph::GlyphRenderer;

/// Number of terminal cells the vertex/index buffers are sized for.
///
/// Each cell can produce up to two quads (background + glyph), plus a handful
/// for the cursor and selection.
const MAX_CELLS: usize = 20_000;

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
    /// Foreground color index.
    pub fg: u8,
    /// Background color index.
    pub bg: u8,
}

/// Renderable terminal content.
pub struct RenderContent<'a> {
    /// The terminal grid cells.
    pub cells: &'a [CellContent],
    /// Cursor row position.
    pub cursor_row: usize,
    /// Cursor column position.
    pub cursor_col: usize,
    /// Cursor shape.
    pub cursor_shape: CursorShape,
    /// Cursor visible.
    pub cursor_visible: bool,
    /// Current selection.
    pub selection: Selection,
    /// Background color (RGBA-ordered).
    pub background_color: Color,
    /// Dimensions (rows, cols).
    pub rows: usize,
    /// Number of columns.
    pub cols: usize,
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
    /// Glyph cache: character → atlas entry (or `None` if unrenderable).
    glyph_cache: HashMap<char, Option<CachedGlyph>>,
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
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
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

        // Build all geometry for this frame (in pixel coordinates).
        let (mut vertices, indices) = self.build_geometry(content);

        // Convert pixel coordinates to normalized device coordinates.
        let viewport_w = (config.width.max(1)) as f32;
        let viewport_h = (config.height.max(1)) as f32;
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
                            a: 1.0,
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

            if self.index_count > 0 {
                pass.draw_indexed(0..self.index_count, 0, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Rasterize and upload glyphs for every unique character on screen.
    fn upload_glyphs(&mut self, content: &RenderContent) {
        for cell in content.cells {
            if cell.ch == ' ' || cell.ch == '\0' || self.glyph_cache.contains_key(&cell.ch) {
                continue;
            }

            let cached = match self.glyph_renderer.rasterize(&self.font, cell.ch) {
                Some(glyph) if glyph.width > 0 && glyph.height > 0 => self
                    .atlas
                    .allocate(glyph.width, glyph.height)
                    .map(|region| {
                        self.atlas.upload(&region, &glyph.data, &self.queue);
                        CachedGlyph {
                            region,
                            width: glyph.width as f32,
                            height: glyph.height as f32,
                            left: glyph.left as f32,
                            top: glyph.top as f32,
                        }
                    }),
                _ => None,
            };

            self.glyph_cache.insert(cell.ch, cached);
        }
    }

    /// Build the vertex and index geometry for the whole frame.
    fn build_geometry(&self, content: &RenderContent) -> (Vec<QuadVertex>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        let cell_w = self.cell_width as f32;
        let cell_h = self.cell_height as f32;
        let baseline = self.ascent;

        let white = self.atlas.white_region();
        let white_uv = (
            (white.uv_left + white.uv_right) * 0.5,
            (white.uv_bottom + white.uv_top) * 0.5,
        );

        let cols = content.cols;
        for row in 0..content.rows {
            for col in 0..cols {
                let idx = row * cols + col;
                let Some(cell) = content.cells.get(idx) else {
                    continue;
                };

                let cell_x = col as f32 * cell_w;
                let cell_y = row as f32 * cell_h;

                // Per-cell background (index 0 means "terminal background").
                if cell.bg != 0 {
                    self.push_quad(
                        &mut vertices,
                        &mut indices,
                        cell_x,
                        cell_y,
                        cell_w,
                        cell_h,
                        (white_uv.0, white_uv.1, white_uv.0, white_uv.1),
                        self.color(cell.bg),
                    );
                }

                // Glyph.
                if cell.ch != ' ' && cell.ch != '\0' {
                    if let Some(Some(glyph)) = self.glyph_cache.get(&cell.ch) {
                        let x = cell_x + glyph.left;
                        let y = cell_y + baseline - glyph.top;
                        self.push_quad(
                            &mut vertices,
                            &mut indices,
                            x,
                            y,
                            glyph.width,
                            glyph.height,
                            (
                                glyph.region.uv_left,
                                glyph.region.uv_bottom,
                                glyph.region.uv_right,
                                glyph.region.uv_top,
                            ),
                            self.color(cell.fg),
                        );
                    }
                }
            }
        }

        self.push_selection(
            content,
            &mut vertices,
            &mut indices,
            cell_w,
            cell_h,
            white_uv,
        );
        self.push_cursor(
            content,
            &mut vertices,
            &mut indices,
            cell_w,
            cell_h,
            white_uv,
        );

        (vertices, indices)
    }

    /// Push selection-highlight quads.
    fn push_selection(
        &self,
        content: &RenderContent,
        vertices: &mut Vec<QuadVertex>,
        indices: &mut Vec<u32>,
        cell_w: f32,
        cell_h: f32,
        white_uv: (f32, f32),
    ) {
        let Some((r1, c1, r2, c2)) = content.selection.bounds() else {
            return;
        };
        let color = [0.25, 0.45, 0.85, 0.35];

        for row in r1..=r2.min(content.rows.saturating_sub(1)) {
            let x = c1 as f32 * cell_w;
            let width = (c2 - c1 + 1) as f32 * cell_w;
            let y = row as f32 * cell_h;
            self.push_quad(
                vertices,
                indices,
                x,
                y,
                width,
                cell_h,
                (white_uv.0, white_uv.1, white_uv.0, white_uv.1),
                color,
            );
        }
    }

    /// Push the cursor quad (shape depends on [`CursorShape`]).
    fn push_cursor(
        &self,
        content: &RenderContent,
        vertices: &mut Vec<QuadVertex>,
        indices: &mut Vec<u32>,
        cell_w: f32,
        cell_h: f32,
        white_uv: (f32, f32),
    ) {
        if !content.cursor_visible {
            return;
        }

        let x = content.cursor_col as f32 * cell_w;
        let y = content.cursor_row as f32 * cell_h;
        let uv = (white_uv.0, white_uv.1, white_uv.0, white_uv.1);
        let color = [0.75, 0.75, 0.95, 0.6];

        match content.cursor_shape {
            CursorShape::Block => {
                self.push_quad(vertices, indices, x, y, cell_w, cell_h, uv, color);
            }
            CursorShape::Underline => {
                let h = (cell_h * 0.12).max(2.0);
                self.push_quad(vertices, indices, x, y + cell_h - h, cell_w, h, uv, color);
            }
            CursorShape::Beam => {
                let w = (cell_w * 0.15).max(2.0);
                self.push_quad(vertices, indices, x, y, w, cell_h, uv, color);
            }
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
}
