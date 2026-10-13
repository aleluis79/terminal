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
    /// UV coordinate (left).
    pub uv_left: f32,
    /// UV coordinate (bottom).
    pub uv_bottom: f32,
    /// UV coordinate (right).
    pub uv_right: f32,
    /// UV coordinate (top).
    pub uv_top: f32,
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
    /// Current free x position (scans left to right).
    free_x: u32,
    /// Current free y position.
    free_y: u32,
    /// Height of the current row.
    row_height: u32,
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
            free_x: 0,
            free_y: 0,
            row_height: 0,
            max_size: Self::MAX_SIZE,
            device: Arc::clone(device),
            white_region: AtlasRegion::default(),
        };

        // Reserve a 1x1 opaque texel for solid-color rectangles.
        let region = atlas
            .allocate(1, 1)
            .expect("atlas has room for white texel");
        atlas.upload(&region, &[255], queue);
        atlas.white_region = region;

        atlas
    }

    /// Allocate a region in the atlas for a glyph.
    ///
    /// Returns the UV coordinates and pixel offset for the glyph.
    /// If the atlas is full, it grows by doubling in size.
    ///
    /// # Arguments
    ///
    /// * `width` - Glyph width in pixels.
    /// * `height` - Glyph height in pixels.
    pub fn allocate(&mut self, width: u32, height: u32) -> Option<AtlasRegion> {
        let w = width.max(1);
        let h = height.max(1);

        // Try to fit in current row
        if self.free_x + w <= self.width && h <= self.row_height {
            return Some(self.allocate_at(self.free_x, self.free_y, w, h));
        }

        // Try to start a new row
        if self.free_x + w <= self.width && self.free_y + h <= self.height {
            self.free_y = self.free_y.max(1) + self.row_height;
            return Some(self.allocate_at(self.free_x, self.free_y, w, h));
        }

        // Atlas needs to grow
        if self.width >= self.max_size {
            eprintln!(
                "Glyph atlas at maximum size ({:?}x{:?}), skipping glyph",
                self.width, self.height
            );
            return None;
        }

        self.grow();

        // Retry allocation after growing
        self.allocate(w, h)
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

    /// Allocate at a specific position.
    #[expect(clippy::cast_precision_loss)]
    fn allocate_at(&mut self, x: u32, y: u32, w: u32, h: u32) -> AtlasRegion {
        let uv_left = x as f32 / self.width as f32;
        let uv_right = (x + w) as f32 / self.width as f32;
        let uv_bottom = y as f32 / self.height as f32;
        let uv_top = (y + h) as f32 / self.height as f32;

        let region = AtlasRegion {
            x,
            y,
            width: w,
            height: h,
            uv_left,
            uv_bottom,
            uv_right,
            uv_top,
        };

        // Update cursor
        self.free_x += w;
        if h > self.row_height {
            self.row_height = h;
        }

        region
    }

    /// Grow the atlas by doubling its size.
    fn grow(&mut self) {
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

        // Replace old texture
        self.texture = new_texture;
        self.texture_view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.width = new_size;
        self.height = new_size;
        self.free_x = 0;
        self.free_y = 0;
        self.row_height = 0;
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
            uv_left: 0.0,
            uv_bottom: 0.0,
            uv_right: 0.0625,
            uv_top: 0.0625,
        };
        assert_eq!(region.x, 0);
        assert_eq!(region.width, 64);
        assert!((region.uv_right - 0.0625).abs() < f32::EPSILON);
    }

    #[test]
    fn test_atlas_max_size() {
        assert_eq!(GlyphAtlas::MAX_SIZE, 4096);
    }
}
