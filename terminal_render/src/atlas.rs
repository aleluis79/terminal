//! GPU texture atlas for glyph rendering.
//!
//! This module handles:
//! - Dynamic texture atlas allocation
//! - Glyph upload to GPU texture
//! - UV coordinate mapping
//! - Atlas growth strategy

use std::sync::Arc;
use wgpu::{
    Device, Extent3d, ImageCopyTexture, Origin3d, Queue, Texture, TextureAspect, TextureFormat,
    TextureUsages, TextureView,
};

/// A region in the texture atlas.
#[derive(Debug, Clone, Copy, Default)]
pub struct AtlasRegion {
    /// X offset in pixels.
    pub x: u32,
    /// Y offset in pixels.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl AtlasRegion {
    /// Normalized UV coordinates of this region, as
    /// `(left, bottom, right, top)`.
    ///
    /// Computed on demand rather than stored: growing the atlas changes the
    /// normalized coordinates of every region already handed out.
    #[expect(
        clippy::cast_precision_loss,
        reason = "Atlas coordinates are at most a few thousand pixels"
    )]
    pub fn uv(&self, atlas_size: (u32, u32)) -> (f32, f32, f32, f32) {
        let (atlas_w, atlas_h) = atlas_size;
        (
            self.x as f32 / atlas_w as f32,
            self.y as f32 / atlas_h as f32,
            (self.x + self.width) as f32 / atlas_w as f32,
            (self.y + self.height) as f32 / atlas_h as f32,
        )
    }
}

/// Packing cursor: where the next glyph goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Cursor {
    /// First free x position in the current row.
    x: u32,
    /// Top edge of the current row.
    y: u32,
    /// Height of the tallest glyph in the current row.
    row_height: u32,
}

impl Cursor {
    /// Place a `width`x`height` glyph in an atlas of `size`.
    ///
    /// Returns the glyph's top-left corner and the updated cursor, or `None`
    /// when the glyph does not fit at all and the atlas has to grow. Glyphs
    /// that do not fit in the current row start a new one below it.
    fn place(&self, width: u32, height: u32, size: (u32, u32)) -> Option<((u32, u32), Self)> {
        let (atlas_w, atlas_h) = size;
        if width > atlas_w || height > atlas_h {
            return None;
        }

        // Same row, if it is tall enough for this glyph.
        if self.x + width <= atlas_w && height <= self.row_height {
            let cursor = Self {
                x: self.x + width,
                y: self.y,
                row_height: self.row_height.max(height),
            };
            return Some(((self.x, self.y), cursor));
        }

        // Otherwise start a new row at the left edge.
        let y = self.y.max(1) + self.row_height;
        if y + height <= atlas_h {
            let cursor = Self {
                x: width,
                y,
                row_height: height,
            };
            return Some(((0, y), cursor));
        }

        None
    }
}

/// GPU texture atlas for storing glyph alpha masks.
///
/// Uses a simple row-based packing algorithm to efficiently
/// store multiple glyph bitmaps in a single single-channel texture.
pub struct GlyphAtlas {
    /// The GPU texture.
    texture: Texture,
    /// The texture view for sampling.
    texture_view: TextureView,
    /// Width of the atlas in pixels.
    width: u32,
    /// Height of the atlas in pixels.
    height: u32,
    /// Where the next glyph is packed.
    cursor: Cursor,
    /// Maximum atlas size before growing.
    max_size: u32,
    /// Device reference for texture creation.
    device: Arc<Device>,
    /// A 1x1 fully-opaque region, used to draw solid-color rectangles.
    white_region: AtlasRegion,
}

impl GlyphAtlas {
    /// Maximum atlas size (4096x4096).
    const MAX_SIZE: u32 = 4096;

    /// Initial atlas size.
    const INITIAL_SIZE: u32 = 1024;

    /// Create a new glyph atlas.
    ///
    /// Initializes a single-channel texture for glyph alpha masks and
    /// reserves a 1x1 opaque region for solid-color geometry.
    ///
    /// # Panics
    ///
    /// Panics if the freshly-created atlas cannot allocate its initial
    /// 1x1 white texel (which should never happen for the initial size).
    pub fn new(device: &Arc<Device>, queue: &Queue) -> Self {
        let size = Self::INITIAL_SIZE;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Initialize to fully transparent (0 = no coverage).
        let init_data = vec![0u8; (size * size) as usize];
        queue.write_texture(
            ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &init_data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(size),
                rows_per_image: Some(size),
            },
            Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );

        let mut atlas = Self {
            texture,
            texture_view,
            width: size,
            height: size,
            cursor: Cursor::default(),
            max_size: Self::MAX_SIZE,
            device: Arc::clone(device),
            white_region: AtlasRegion::default(),
        };

        // Reserve a 1x1 opaque texel for solid-color rectangles.
        let region = atlas
            .allocate(1, 1, queue)
            .expect("atlas has room for white texel");
        atlas.upload(&region, &[255], queue);
        atlas.white_region = region;

        atlas
    }

    /// Allocate a region in the atlas for a glyph.
    ///
    /// Glyphs are packed left to right, wrapping to a new row when the current
    /// one is full. Only when every row is full does the atlas grow.
    ///
    /// # Arguments
    ///
    /// * `width` - Glyph width in pixels.
    /// * `height` - Glyph height in pixels.
    /// * `queue` - GPU queue, used when the atlas has to grow.
    pub fn allocate(&mut self, width: u32, height: u32, queue: &Queue) -> Option<AtlasRegion> {
        let w = width.max(1);
        let h = height.max(1);

        if let Some(((x, y), cursor)) = self.cursor.place(w, h, (self.width, self.height)) {
            self.cursor = cursor;
            return Some(AtlasRegion {
                x,
                y,
                width: w,
                height: h,
            });
        }

        if self.width >= self.max_size {
            eprintln!(
                "Glyph atlas at maximum size ({:?}x{:?}), skipping glyph",
                self.width, self.height
            );
            return None;
        }

        self.grow(queue);

        // Retry allocation after growing
        self.allocate(w, h, queue)
    }

    /// Upload a glyph alpha mask to the atlas at the given region.
    ///
    /// The atlas is `R8Unorm`; `alpha` is one byte per pixel
    /// (`region.width * region.height`).
    ///
    /// # Arguments
    ///
    /// * `region` - The atlas region to write to.
    /// * `alpha` - Alpha mask data (1 byte per pixel).
    /// * `queue` - GPU queue for submitting the upload.
    pub fn upload(&self, region: &AtlasRegion, alpha: &[u8], queue: &Queue) {
        let width = region.width as usize;
        let height = region.height as usize;
        let expected = width * height;

        // Pad/shorten to the expected size so write_texture never reads OOB.
        let mut data = Vec::with_capacity(expected);
        data.extend_from_slice(&alpha[..alpha.len().min(expected)]);
        data.resize(expected, 0);

        queue.write_texture(
            ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: region.x,
                    y: region.y,
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            &data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(region.width),
                rows_per_image: Some(region.height),
            },
            wgpu::Extent3d {
                width: region.width,
                height: region.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Get the texture view for sampling glyphs.
    pub fn texture_view(&self) -> &TextureView {
        &self.texture_view
    }

    /// Get the atlas dimensions.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// A 1x1 fully-opaque region used for solid-color rectangles.
    pub fn white_region(&self) -> AtlasRegion {
        self.white_region
    }

    /// Grow the atlas by doubling its size.
    ///
    /// The packed glyphs are copied over so the regions already handed out keep
    /// pointing at their pixels, and the packing cursor is left untouched.
    fn grow(&mut self, queue: &Queue) {
        let new_size = (self.width * 2).min(self.max_size);
        eprintln!(
            "Growing glyph atlas from {:?}x{:?} to {:?}x{:?}",
            self.width, self.height, new_size, new_size
        );

        // Create new larger texture
        let new_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas_grown"),
            size: wgpu::Extent3d {
                width: new_size,
                height: new_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        // Copy the glyphs packed so far into the new texture.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("glyph_atlas_grow"),
            });
        encoder.copy_texture_to_texture(
            ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            ImageCopyTexture {
                texture: &new_texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        // Replace old texture
        self.texture = new_texture;
        self.texture_view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.width = new_size;
        self.height = new_size;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atlas_region_uv() {
        let region = AtlasRegion {
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        };
        assert_eq!(region.x, 0);
        assert_eq!(region.width, 64);
        // 64 pixels of a 1024-wide atlas.
        let (left, bottom, right, top) = region.uv((1024, 1024));
        assert!((left - 0.0).abs() < f32::EPSILON);
        assert!((bottom - 0.0).abs() < f32::EPSILON);
        assert!((right - 0.0625).abs() < f32::EPSILON);
        assert!((top - 0.0625).abs() < f32::EPSILON);
    }

    #[test]
    fn test_atlas_region_uv_follows_the_atlas_size() {
        // Growing the atlas must change the UVs of a region already handed out,
        // which is why they are computed on demand instead of cached.
        let region = AtlasRegion {
            x: 512,
            y: 0,
            width: 64,
            height: 64,
        };
        assert!((region.uv((1024, 1024)).0 - 0.5).abs() < f32::EPSILON);
        assert!((region.uv((2048, 2048)).0 - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn test_cursor_wraps_to_a_new_row() {
        // Regression: a full row used to grow the atlas — dropping every glyph
        // already packed — instead of wrapping to the next row.
        let size = (100, 100);
        let ((x, y), cursor) = Cursor::default().place(60, 10, size).expect("first glyph");
        assert_eq!((x, y), (0, 1));

        let ((x2, y2), cursor) = cursor.place(50, 10, size).expect("second glyph");
        assert_eq!((x2, y2), (0, 11), "a full row must wrap");
        assert_eq!(cursor.x, 50);

        // Glyphs that fit keep filling the same row.
        let ((x3, y3), _) = cursor.place(10, 10, size).expect("third glyph");
        assert_eq!((x3, y3), (50, 11));
    }

    #[test]
    fn test_cursor_reports_a_full_atlas() {
        // Only a genuinely full atlas makes `place` give up.
        let size = (64, 62);
        let ((_, _), cursor) = Cursor::default().place(64, 60, size).expect("first glyph");
        assert!(cursor.place(10, 10, size).is_none());
    }

    #[test]
    fn test_atlas_max_size() {
        assert_eq!(GlyphAtlas::MAX_SIZE, 4096);
    }
}
