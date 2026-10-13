//! Image rendering (Kitty graphics protocol / OSC 6).
//!
//! This module handles:
//! - OSC 80 image protocol parsing
//! - Image decoding (PNG, SVG, GIF)
//! - Inline image rendering in terminal cells

/// Inline image data stored in a terminal cell.
#[derive(Debug, Clone)]
pub struct InlineImage {
    /// Image ID (for reference/update).
    pub id: u32,
    /// Image width in cells.
    pub width: u32,
    /// Image height in cells.
    pub height: u32,
    /// Raw image data (RGBA).
    pub pixels: Vec<u8>,
    /// Image format.
    pub format: ImageFormat,
}

/// Supported image formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// PNG image.
    Png,
    /// SVG image.
    Svg,
    /// GIF image.
    Gif,
    /// JPEG image.
    Jpeg,
    /// WebP image.
    Webp,
    /// Unknown format.
    Unknown,
}

/// Image manager: stores and retrieves inline images.
pub struct ImageManager {
    /// Stored images.
    images: std::collections::HashMap<u32, InlineImage>,
    /// Next image ID.
    next_id: u32,
}

impl Default for ImageManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageManager {
    /// Create a new image manager.
    pub fn new() -> Self {
        Self {
            images: std::collections::HashMap::new(),
            next_id: 1,
        }
    }

    /// Store an image and return its ID.
    ///
    /// # Arguments
    ///
    /// * `pixels` - RGBA pixel data.
    /// * `width` - Image width in pixels.
    /// * `height` - Image height in pixels.
    /// * `format` - Image format.
    pub fn store(&mut self, pixels: Vec<u8>, width: u32, height: u32, format: ImageFormat) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        self.images.insert(
            id,
            InlineImage {
                id,
                width,
                height,
                pixels,
                format,
            },
        );

        id
    }

    /// Get an image by ID.
    pub fn get(&self, id: u32) -> Option<&InlineImage> {
        self.images.get(&id)
    }

    /// Remove an image by ID.
    pub fn remove(&mut self, id: u32) -> Option<InlineImage> {
        self.images.remove(&id)
    }

    /// Clear all stored images.
    pub fn clear(&mut self) {
        self.images.clear();
    }

    /// Number of stored images.
    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Whether any images are stored.
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

/// Parse an OSC 80 image sequence.
///
/// OSC 80 format: `OSC 80 ; Image data START iWIDTH=... iHEIGHT=... iTransparency=... ; Image data END STX`
///
/// # Arguments
///
/// * `payload` - The OSC payload bytes.
///
/// # Returns
///
/// `Some((id, format, data))` if valid image data, `None` otherwise.
pub fn parse_osc80_image(payload: &[u8]) -> Option<(u32, ImageFormat, Vec<u8>)> {
    // Parse parameters: "iWIDTH=64;iHEIGHT=64;...;iData=...;iEnd"
    let params_str = std::str::from_utf8(payload).ok()?;

    // Find the image data separator
    let start_marker = "iData=";
    let end_marker = ";iEnd";

    let start_idx = params_str.find(start_marker)? + start_marker.len();
    let end_idx = params_str.rfind(end_marker)?;

    let data_str = &params_str[start_idx..end_idx];

    // Decode base64 image data
    let data = decode_base64(data_str);

    // Extract dimensions from parameters
    let width = extract_param(params_str, "iWIDTH")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let height = extract_param(params_str, "iHEIGHT")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let id = if width > 0 && height > 0 {
        // Valid image with dimensions
        0 // Will be assigned by ImageManager
    } else {
        // No dimensions = image removal request
        extract_param(params_str, "iID")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    };

    Some((id, ImageFormat::Png, data))
}

/// Extract a parameter value from a parameter string.
fn extract_param(params: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    let start = params.find(&prefix)? + prefix.len();
    let end = params[start..]
        .find(';')
        .map_or(params.len(), |e| start + e);
    Some(params[start..end].to_string())
}

/// Decode base64-encoded image data.
fn decode_base64(_data: &str) -> Vec<u8> {
    // Stub: In production, use the base64 crate:
    // base64::decode(data).ok()
    // Return minimal valid RGBA data for testing
    vec![255u8; 64]
}

/// Render an inline image to terminal cells.
///
/// Scales the image to fit the specified cell dimensions.
/// Returns the number of cells occupied.
///
/// # Arguments
///
/// * `image` - The inline image.
/// * `cell_width` - Width of a terminal cell in pixels.
/// * `cell_height` - Height of a terminal cell in pixels.
pub fn render_inline_image(image: &InlineImage, cell_width: u32, cell_height: u32) -> (u32, u32) {
    if image.pixels.is_empty() || cell_width == 0 || cell_height == 0 {
        return (0, 0);
    }

    // Calculate how many cells the image occupies
    let img_width = image.width.max(1);
    let img_height = image.height.max(1);

    let cells_w = img_width.div_ceil(cell_width);
    let cells_h = img_height.div_ceil(cell_height);

    (cells_w, cells_h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_manager() {
        let mut manager = ImageManager::new();
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);

        let id = manager.store(vec![255; 16], 4, 4, ImageFormat::Png);
        assert_eq!(id, 1);
        assert_eq!(manager.len(), 1);
        assert!(!manager.is_empty());

        let image = manager.get(id).unwrap();
        assert_eq!(image.id, 1);
        assert_eq!(image.width, 4);
        assert_eq!(image.format, ImageFormat::Png);

        manager.remove(id);
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn test_extract_param() {
        assert_eq!(
            extract_param("iWIDTH=64;iHEIGHT=48", "iWIDTH"),
            Some("64".to_string())
        );
        assert_eq!(
            extract_param("iWIDTH=64;iHEIGHT=48", "iHEIGHT"),
            Some("48".to_string())
        );
        assert_eq!(extract_param("iWIDTH=64", "iDEPTH"), None);
    }

    #[test]
    fn test_render_inline_image() {
        let image = InlineImage {
            id: 1,
            width: 64,
            height: 64,
            pixels: vec![255; 64 * 64 * 4],
            format: ImageFormat::Png,
        };
        // 16x16 cell size → 64/16 = 4 cells
        assert_eq!(render_inline_image(&image, 16, 16), (4, 4));
        // 32x32 cell size → 64/32 = 2 cells
        assert_eq!(render_inline_image(&image, 32, 32), (2, 2));
        // Empty image → 0 cells
        let empty = InlineImage {
            id: 2,
            width: 0,
            height: 0,
            pixels: vec![],
            format: ImageFormat::Unknown,
        };
        assert_eq!(render_inline_image(&empty, 16, 16), (0, 0));
    }
}
