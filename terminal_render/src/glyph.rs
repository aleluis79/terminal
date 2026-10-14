//! Glyph shaping and rendering with swash.
//!
//! This module handles:
//! - Glyph rasterization to alpha masks (via swash)
//! - Ligature detection helpers
//! - Emoji detection helpers

use crate::font::FontHandle;

/// Emboldening applied to synthesise bold text, as a fraction of the font size.
const SYNTHETIC_BOLD_STRENGTH: f32 = 0.035;

/// Slant applied to synthesise italic text, in degrees.
const SYNTHETIC_ITALIC_DEGREES: f32 = 12.0;

/// A rasterized glyph (8-bit alpha mask) ready for atlas upload.
#[derive(Debug, Clone)]
pub struct RasterGlyph {
    /// Glyph ID in the font.
    pub glyph_id: u16,
    /// Bitmap width in pixels.
    pub width: u32,
    /// Bitmap height in pixels.
    pub height: u32,
    /// Horizontal offset from the pen position to the left edge.
    pub left: i32,
    /// Vertical offset from the baseline (positive up) to the top edge.
    pub top: i32,
    /// Alpha mask, one byte per pixel (`width * height`).
    pub data: Vec<u8>,
}

/// Glyph renderer using swash.
pub struct GlyphRenderer {
    /// Font size in pixels for rendering.
    font_size: f32,
    /// Reusable scaling context (holds glyph caches).
    context: swash::scale::ScaleContext,
}

impl GlyphRenderer {
    /// Create a new glyph renderer.
    ///
    /// # Arguments
    ///
    /// * `font_size` - The font size in pixels.
    pub fn new(font_size: f32) -> Self {
        Self {
            font_size,
            context: swash::scale::ScaleContext::new(),
        }
    }

    /// Rasterize a single character into an 8-bit alpha mask.
    ///
    /// A real bold or italic face is used when the family provides one.
    /// Otherwise the style is synthesised from the regular outline —
    /// emboldened or slanted — which is enough for text but fattens fine
    /// details such as the acute accent of `í`.
    ///
    /// Returns `None` if the character has no renderable outline in the font
    /// (for example a space, or a glyph missing from the face).
    ///
    /// # Arguments
    ///
    /// * `font` - The font handle to rasterize with.
    /// * `ch` - The character to rasterize.
    /// * `bold` - Whether to draw the glyph bold.
    /// * `italic` - Whether to draw the glyph italic.
    pub fn rasterize(
        &mut self,
        font: &FontHandle,
        ch: char,
        bold: bool,
        italic: bool,
    ) -> Option<RasterGlyph> {
        // Per-glyph fallback (CL-5): use whichever face actually has the glyph.
        // A styled face that does not cover it (a symbol from another family,
        // say) falls back to the regular face with the style synthesised.
        let styled = font
            .styled_face(bold, italic)
            .filter(|id| font.face_has_glyph(*id, ch));
        let face = styled.or_else(|| font.face_for_char(ch))?;
        let synthesise = styled.is_none();
        let size = self.font_size;
        let context = &mut self.context;

        font.with_face_data_for(face, |data, index| {
            let font_ref = swash::FontRef::from_index(data, index as usize)?;
            let glyph_id = font_ref.charmap().map(ch);
            if glyph_id == 0 {
                return None;
            }

            let mut scaler = context.builder(font_ref).size(size).hint(true).build();
            let mut render = swash::scale::Render::new(&[swash::scale::Source::Outline]);
            render
                .format(swash::zeno::Format::Alpha)
                .offset(swash::zeno::Vector::new(0.0, 0.0));
            if bold && synthesise {
                render.embolden(size * SYNTHETIC_BOLD_STRENGTH);
            }
            if italic && synthesise {
                // Shear the outline: the top of the glyph leans to the right.
                render.transform(Some(swash::zeno::Transform::skew(
                    swash::zeno::Angle::from_degrees(SYNTHETIC_ITALIC_DEGREES),
                    swash::zeno::Angle::ZERO,
                )));
            }
            let image = render.render(&mut scaler, glyph_id)?;

            let placement = image.placement;
            let mut data = image.data;
            let len = (placement.width * placement.height) as usize;
            data.resize(len, 0);

            Some(RasterGlyph {
                glyph_id,
                width: placement.width,
                height: placement.height,
                left: placement.left,
                top: placement.top,
                data,
            })
        })
        .flatten()
    }

    /// Compute character-level shift for ligature support.
    ///
    /// Ligature pairs that are commonly supported:
    /// - "fi", "fl", "ffi", "ff"
    /// - "ft", "st"
    ///
    /// # Arguments
    ///
    /// * `text` - The full text being shaped.
    /// * `index` - The byte index of the current character.
    /// * `ch` - The current character.
    ///
    /// # Returns
    ///
    /// `Some(CharacterLevelShift)` if this character is part of a ligature,
    /// `None` otherwise.
    pub fn compute_cls(
        text: &str,
        index: usize,
        ch: char,
    ) -> Option<crate::font::CharacterLevelShift> {
        // Get the next character
        let next_char = text[index + ch.len_utf8()..].chars().next();

        // Common ligature pairs
        let ligature_pair: String = format!("{}{}", ch, next_char.unwrap_or('\0'));

        match ligature_pair.as_str() {
            // Standard ligatures
            "fi" | "fl" | "ffi" | "ff" | "ft" | "st" => {
                // This character is the start of a ligature
                // Return shift for the first character in a 2-char cluster
                Some(crate::font::CharacterLevelShift::new(0, 2, 0.0))
            }
            _ => None,
        }
    }

    /// Check if a character is an emoji.
    ///
    /// Emojis are in the Unicode ranges:
    /// - U+1F600..=U+1F64F (Emoticons)
    /// - U+1F300..=U+1F5FF (Misc Symbols and Pictographs)
    /// - U+1F680..=U+1F6FF (Transport and Map)
    /// - U+1F900..=U+1F9FF (Supplemental Symbols)
    /// - U+1FA00..=U+1FA6F (Chess Symbols)
    /// - U+1FA70..=U+1FAFF (Symbols and Pictographs)
    /// - U+2600..=U+26FF (Misc Symbols)
    /// - U+2700..=U+27BF (Dingbats)
    /// - U+FE00..=U+FE0F (Variation Selectors)
    /// - U+200D (ZWJ - Zero Width Joiner)
    /// - U+1F3FB..=U+1F3FF (Skin tone modifiers)
    pub fn is_emoji(ch: char) -> bool {
        let cp = ch as u32;
        matches!(cp,
            0x1F600..=0x1F64F |
            0x1F300..=0x1F5FF |
            0x1F680..=0x1F6FF |
            0x1F900..=0x1F9FF |
            0x1FA00..=0x1FA6F |
            0x1FA70..=0x1FAFF |
            0x2600..=0x26FF |
            0x2700..=0x27BF |
            0xFE00..=0xFE0F |
            0x200D
        ) || Self::is_skin_tone_modifier(ch)
    }

    /// Check if a character is a ZWJ (Zero Width Joiner).
    pub fn is_zwj(ch: char) -> bool {
        ch == '\u{200D}'
    }

    /// Check if a character is a skin tone modifier.
    pub fn is_skin_tone_modifier(ch: char) -> bool {
        matches!(ch as u32, 0x1F3FB..=0x1F3FF)
    }
}

impl Default for GlyphRenderer {
    fn default() -> Self {
        Self::new(14.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontConfig;

    #[test]
    fn test_ligature_detection_fi() {
        let text = "fi";
        let cls = GlyphRenderer::compute_cls(text, 0, 'f');
        assert!(cls.is_some());
        let cls = cls.unwrap();
        assert_eq!(cls.position, 0);
        assert_eq!(cls.total, 2);
    }

    #[test]
    fn test_ligature_detection_fl() {
        let text = "fl";
        let cls = GlyphRenderer::compute_cls(text, 0, 'f');
        assert!(cls.is_some());
        let cls = cls.unwrap();
        assert_eq!(cls.position, 0);
        assert_eq!(cls.total, 2);
    }

    #[test]
    fn test_ligature_detection_ff() {
        let text = "ff";
        let cls = GlyphRenderer::compute_cls(text, 0, 'f');
        assert!(cls.is_some());
        let cls = cls.unwrap();
        assert_eq!(cls.position, 0);
        assert_eq!(cls.total, 2);
    }

    #[test]
    fn test_ligature_detection_no_ligature() {
        let text = "fa";
        let cls = GlyphRenderer::compute_cls(text, 0, 'f');
        assert!(cls.is_none());
    }

    #[test]
    fn test_ligature_detection_normal_char() {
        let text = "abc";
        let cls = GlyphRenderer::compute_cls(text, 0, 'a');
        assert!(cls.is_none());
    }

    #[test]
    fn test_rasterize_ascii_glyph() {
        let font = FontHandle::new(&FontConfig::default());
        let mut renderer = GlyphRenderer::new(font.pixel_size());
        let glyph = renderer.rasterize(&font, 'A', false, false);
        let glyph = glyph.expect("'A' should rasterize to a bitmap");
        assert!(glyph.width > 0);
        assert!(glyph.height > 0);
        assert_eq!(glyph.data.len(), (glyph.width * glyph.height) as usize);
        // The mask must contain some coverage, not be entirely empty.
        assert!(glyph.data.iter().any(|&px| px > 0));
    }

    #[test]
    fn test_rasterize_bold_is_heavier() {
        let font = FontHandle::new(&FontConfig::default());
        let mut renderer = GlyphRenderer::new(font.pixel_size());
        let regular = renderer
            .rasterize(&font, 'H', false, false)
            .expect("regular 'H'");
        let bold = renderer
            .rasterize(&font, 'H', true, false)
            .expect("bold 'H'");

        let ink = |g: &RasterGlyph| g.data.iter().map(|&p| u32::from(p)).sum::<u32>();
        assert!(
            ink(&bold) > ink(&regular),
            "bold text must cover more pixels ({} vs {})",
            ink(&bold),
            ink(&regular)
        );
    }

    #[test]
    fn test_bold_falls_back_to_a_synthesised_outline() {
        // A family with no bold face of its own must still render bold text, by
        // fattening the regular outline.
        let font = FontHandle::new(&FontConfig::default());
        let no_styles = FontHandle {
            bold_face_id: None,
            italic_face_id: None,
            bold_italic_face_id: None,
            ..font.clone()
        };
        assert_eq!(no_styles.styled_face(true, false), None);

        let mut renderer = GlyphRenderer::new(no_styles.pixel_size());
        let regular = renderer
            .rasterize(&no_styles, 'H', false, false)
            .expect("regular 'H'");
        let bold = renderer
            .rasterize(&no_styles, 'H', true, false)
            .expect("bold 'H'");

        let ink = |g: &RasterGlyph| g.data.iter().map(|&p| u32::from(p)).sum::<u32>();
        assert!(
            ink(&bold) > ink(&regular),
            "synthesised bold must still be heavier ({} vs {})",
            ink(&bold),
            ink(&regular)
        );
    }

    #[test]
    fn test_rasterize_italic_leans_right() {
        let font = FontHandle::new(&FontConfig::default());
        let mut renderer = GlyphRenderer::new(font.pixel_size());
        let regular = renderer
            .rasterize(&font, 'H', false, false)
            .expect("regular 'H'");
        let italic = renderer
            .rasterize(&font, 'H', false, true)
            .expect("italic 'H'");

        // Slanting pushes the top of the glyph to the right, so the ink's
        // centre of mass moves that way.
        let (regular_mass, italic_mass) = (centre_of_mass(&regular), centre_of_mass(&italic));
        assert!(
            italic_mass > regular_mass,
            "italic text must lean right ({italic_mass} vs {regular_mass})"
        );
    }

    /// Horizontal centre of mass of a glyph's ink, in pixels.
    #[expect(
        clippy::cast_precision_loss,
        reason = "Glyph masks are a few dozen pixels wide"
    )]
    fn centre_of_mass(glyph: &RasterGlyph) -> f32 {
        if glyph.width == 0 {
            return 0.0;
        }
        let mut moment = 0.0;
        let mut weight = 0.0;
        for (i, &px) in glyph.data.iter().enumerate() {
            let x = (i % glyph.width as usize) as f32;
            moment += x * f32::from(px);
            weight += f32::from(px);
        }
        if weight == 0.0 {
            0.0
        } else {
            moment / weight
        }
    }

    #[test]
    fn test_rasterize_latin1_glyph() {
        // Regression: a non-ASCII letter must rasterize like any other glyph.
        let font = FontHandle::new(&FontConfig::default());
        let mut renderer = GlyphRenderer::new(font.pixel_size());
        let glyph = renderer
            .rasterize(&font, 'ñ', false, false)
            .expect("'ñ' should rasterize to a bitmap");
        assert!(glyph.width > 0);
        assert!(glyph.height > 0);
        assert!(glyph.data.iter().any(|&px| px > 0));
    }

    #[test]
    fn test_rasterize_space_is_empty() {
        let font = FontHandle::new(&FontConfig::default());
        let mut renderer = GlyphRenderer::new(font.pixel_size());
        // A space has no outline; swash returns no image (or an empty one).
        if let Some(glyph) = renderer.rasterize(&font, ' ', false, false) {
            assert!(glyph.data.iter().all(|&px| px == 0));
        }
    }

    #[test]
    fn test_emoji_crate() {
        assert!(GlyphRenderer::is_emoji('🦀'));
    }

    #[test]
    fn test_emoji_smile() {
        assert!(GlyphRenderer::is_emoji('😀'));
    }

    #[test]
    fn test_emoji_zwj() {
        assert!(GlyphRenderer::is_zwj('\u{200D}'));
    }

    #[test]
    fn test_skin_tone_modifier() {
        assert!(GlyphRenderer::is_skin_tone_modifier('🏻'));
        assert!(GlyphRenderer::is_skin_tone_modifier('🏼'));
        assert!(GlyphRenderer::is_skin_tone_modifier('🏽'));
        assert!(GlyphRenderer::is_skin_tone_modifier('🏾'));
        assert!(GlyphRenderer::is_skin_tone_modifier('🏿'));
    }

    #[test]
    fn test_not_emoji() {
        assert!(!GlyphRenderer::is_emoji('a'));
        assert!(!GlyphRenderer::is_emoji('1'));
        assert!(!GlyphRenderer::is_emoji(' '));
    }
}
