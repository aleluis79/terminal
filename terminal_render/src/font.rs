//! Font loading and management with fontdb.
//!
//! This module handles:
//! - Font database initialization (fontdb)
//! - Font family loading and fallback
//! - Font size configuration
//! - Access to raw face data for swash (metrics, glyph rasterization)

use fontdb::Database;

/// Configuration for font loading.
#[derive(Debug, Clone)]
pub struct FontConfig {
    /// Font family name (e.g., `JetBrains Mono`, `Fira Code`).
    pub family: String,
    /// Font size in points.
    pub size: f32,
    /// Whether to load system fonts.
    pub load_system_fonts: bool,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "JetBrains Mono".to_string(),
            size: 14.0,
            load_system_fonts: true,
        }
    }
}

/// Handle to a loaded font face.
#[derive(Debug, Clone)]
pub struct FontHandle {
    /// The underlying fontdb database.
    pub db: Database,
    /// The face ID of the loaded font.
    pub face_id: fontdb::ID,
    /// Bold face of the same family, when it has one.
    pub bold_face_id: Option<fontdb::ID>,
    /// Italic face of the same family, when it has one.
    pub italic_face_id: Option<fontdb::ID>,
    /// Bold italic face of the same family, when it has one.
    pub bold_italic_face_id: Option<fontdb::ID>,
    /// The configured font size in points.
    pub size: f32,
}

impl FontHandle {
    /// Create a new font handle from configuration.
    ///
    /// Loads the specified font family from system fonts or the
    /// provided font bytes. Falls back to a monospace font if
    /// the requested family is not found.
    ///
    /// # Panics
    ///
    /// Panics if no fonts are available in the system.
    pub fn new(config: &FontConfig) -> Self {
        let mut db = Database::new();

        // Load system fonts if configured
        if config.load_system_fonts {
            db.load_system_fonts();
        }

        // Query for the requested font family
        let face_id = db
            .query(&fontdb::Query {
                families: &[fontdb::Family::Name(&config.family)],
                weight: fontdb::Weight::NORMAL,
                stretch: fontdb::Stretch::Normal,
                style: fontdb::Style::Normal,
            })
            .or_else(|| {
                // Fallback: use an explicitly monospaced face. Note that
                // `Family::Monospace` resolves by configured family name, not by
                // the `monospaced` flag, so it is unreliable here.
                eprintln!(
                    "Font '{}' not found, falling back to default monospace",
                    config.family
                );
                db.faces().find(|f| f.monospaced).map(|f| f.id)
            })
            .or_else(|| {
                // Last resort: use any available font
                db.faces().next().map(|f| f.id)
            })
            .expect("No fonts available");

        // Styled faces of the same family, so bold and italic text uses the
        // real designs instead of a synthesised outline. A face identical to
        // the regular one is discarded: `query` always returns its closest
        // match, which would otherwise make every style "found".
        let family = db
            .face(face_id)
            .and_then(|face| face.families.first().map(|(name, _)| name.clone()));
        let styled = |weight, style| {
            family.as_ref().and_then(|family| {
                db.query(&fontdb::Query {
                    families: &[fontdb::Family::Name(family)],
                    weight,
                    stretch: fontdb::Stretch::Normal,
                    style,
                })
            })
        };
        let styled = |weight, style| styled(weight, style).filter(|id| *id != face_id);

        Self {
            bold_face_id: styled(fontdb::Weight::BOLD, fontdb::Style::Normal),
            italic_face_id: styled(fontdb::Weight::NORMAL, fontdb::Style::Italic),
            bold_italic_face_id: styled(fontdb::Weight::BOLD, fontdb::Style::Italic),
            db,
            face_id,
            size: config.size,
        }
    }

    /// Face to use for the given style, when the family provides a real one.
    ///
    /// Returns `None` when the style has to be synthesised from the regular
    /// outline.
    pub fn styled_face(&self, bold: bool, italic: bool) -> Option<fontdb::ID> {
        match (bold, italic) {
            (false, false) => Some(self.face_id),
            (true, false) => self.bold_face_id,
            (false, true) => self.italic_face_id,
            (true, true) => self.bold_italic_face_id,
        }
    }

    /// Get the font size in pixels for the given DPI.
    pub fn size_in_pixels(&self, dpi: f32) -> f32 {
        // Standard DPI is 72 DPI for font points
        self.size * dpi / 72.0
    }

    /// Font size in pixels assuming a 96 DPI display.
    pub fn pixel_size(&self) -> f32 {
        self.size_in_pixels(96.0)
    }

    /// Run a closure with the raw face data and its face index.
    ///
    /// Returns `None` if the face data is unavailable.
    pub fn with_face_data<R>(&self, f: impl FnOnce(&[u8], u32) -> R) -> Option<R> {
        self.db.with_face_data(self.face_id, f)
    }

    /// Run a closure with the raw face data of a specific face.
    ///
    /// Returns `None` if the face data is unavailable.
    pub fn with_face_data_for<R>(
        &self,
        id: fontdb::ID,
        f: impl FnOnce(&[u8], u32) -> R,
    ) -> Option<R> {
        self.db.with_face_data(id, f)
    }

    /// Whether the face `id` maps `ch` to a real glyph.
    pub fn face_has_glyph(&self, id: fontdb::ID, ch: char) -> bool {
        self.with_face_data_for(id, |data, index| {
            swash::FontRef::from_index(data, index as usize)
                .is_some_and(|font| font.charmap().map(ch) != 0)
        })
        .unwrap_or(false)
    }

    /// Faces to try for a glyph, in priority order (RF-46): the configured
    /// family first, then the other monospaced faces, then every remaining
    /// face (symbol and emoji fonts live here).
    pub fn fallback_chain(&self) -> Vec<fontdb::ID> {
        let mut chain = Vec::new();
        chain.push(self.face_id);
        for face in self.db.faces() {
            if face.id != self.face_id && face.monospaced {
                chain.push(face.id);
            }
        }
        for face in self.db.faces() {
            if face.id != self.face_id && !face.monospaced {
                chain.push(face.id);
            }
        }
        chain
    }

    /// Find a face able to render `ch`, preferring the configured face.
    ///
    /// Implements per-glyph fallback (CL-5, RF-46): when the primary face does
    /// not cover a character (emoji, Nerd Font symbols, …), fonts are searched
    /// in [`Self::fallback_chain`] order. Returns `None` only when no installed
    /// face has the glyph, so the caller draws `.notdef` as a last resort
    /// (CL-14) instead of a face that would render the wrong thing.
    pub fn face_for_char(&self, ch: char) -> Option<fontdb::ID> {
        self.fallback_chain()
            .into_iter()
            .find(|&id| self.face_has_glyph(id, ch))
    }

    /// Map a character to a glyph id in this font (0 if unmapped).
    pub fn glyph_id(&self, ch: char) -> u16 {
        self.with_face_data(|data, index| {
            swash::FontRef::from_index(data, index as usize)
                .map_or(0, |font| font.charmap().map(ch))
        })
        .unwrap_or(0)
    }

    /// Get scaled font metrics for the configured size.
    pub fn metrics(&self) -> FontMetrics {
        let ppem = self.pixel_size();
        let advance = self.advance_width(ppem);

        let scaled = self
            .with_face_data(|data, index| {
                swash::FontRef::from_index(data, index as usize)
                    .map(|font| font.metrics(&[]).scale(ppem))
            })
            .flatten();

        match scaled {
            Some(m) => FontMetrics {
                ascent: m.ascent,
                descent: m.descent,
                line_gap: m.leading,
                advance_width: advance.unwrap_or_else(|| m.average_width.max(1.0)),
            },
            None => FontMetrics {
                ascent: ppem * 0.8,
                descent: ppem * 0.2,
                line_gap: 0.0,
                advance_width: advance.unwrap_or(ppem * 0.6),
            },
        }
    }

    /// Advance width (in pixels) of a representative glyph, scaled to `ppem`.
    fn advance_width(&self, ppem: f32) -> Option<f32> {
        self.with_face_data(|data, index| {
            let font = swash::FontRef::from_index(data, index as usize)?;
            let units_per_em = f32::from(font.metrics(&[]).units_per_em);
            if units_per_em == 0.0 {
                return None;
            }
            let glyph_id = font.charmap().map('M');
            Some(font.glyph_metrics(&[]).advance_width(glyph_id) * ppem / units_per_em)
        })
        .flatten()
        .filter(|advance| *advance > 0.0)
    }
}

/// Font metrics for shaping and layout.
#[derive(Debug, Clone, Copy)]
pub struct FontMetrics {
    /// Distance from baseline to ascent.
    pub ascent: f32,
    /// Distance from baseline to descent.
    pub descent: f32,
    /// Gap between lines.
    pub line_gap: f32,
    /// Width of a standard character.
    pub advance_width: f32,
}

/// Character Level Shift for ligature support.
///
/// Some ligatures (like "fi", "fl") combine multiple characters
/// into a single glyph. CLS tracks the offset for each character
/// in a ligature cluster.
#[derive(Debug, Clone, Copy)]
pub struct CharacterLevelShift {
    /// The character position within the cluster.
    pub position: usize,
    /// Total number of characters in the cluster.
    pub total: usize,
    /// The glyph advance width for this character.
    pub advance: f32,
}

impl CharacterLevelShift {
    /// Create a new CLS entry.
    pub fn new(position: usize, total: usize, advance: f32) -> Self {
        Self {
            position,
            total,
            advance,
        }
    }

    /// Get the horizontal offset for this character.
    #[expect(clippy::cast_precision_loss)]
    pub fn offset(&self) -> f32 {
        // Distribute the total advance evenly across cluster members
        let total_advance = self.advance * self.total as f32;
        (self.position as f32 * total_advance) / self.total as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_config_default() {
        let config = FontConfig::default();
        assert_eq!(config.family, "JetBrains Mono");
        assert!((config.size - 14.0).abs() < f32::EPSILON);
        assert!(config.load_system_fonts);
    }

    #[test]
    fn test_font_handle_creation() {
        let config = FontConfig::default();
        let font = FontHandle::new(&config);
        // Font should have a valid face ID (display is non-zero)
        let id_str = format!("{}", font.face_id);
        assert!(!id_str.is_empty());
        assert!((font.size - 14.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_font_handle_metrics() {
        let config = FontConfig::default();
        let font = FontHandle::new(&config);
        let metrics = font.metrics();
        // Metrics should be positive
        assert!(metrics.ascent > 0.0);
        assert!(metrics.advance_width > 0.0);
    }

    #[test]
    fn test_glyph_id_maps_ascii() {
        let config = FontConfig::default();
        let font = FontHandle::new(&config);
        // 'A' must map to a non-zero glyph id in any usable font.
        assert!(font.glyph_id('A') > 0);
    }

    #[test]
    fn test_face_for_char_prefers_primary() {
        let font = FontHandle::new(&FontConfig::default());
        // Characters the configured face covers must resolve to that face.
        assert_eq!(font.face_for_char('A'), Some(font.face_id));
        assert_eq!(font.face_for_char(' '), Some(font.face_id));
    }

    #[test]
    fn test_fallback_chain_starts_with_the_configured_face() {
        let font = FontHandle::new(&FontConfig::default());
        let chain = font.fallback_chain();
        assert_eq!(chain.first(), Some(&font.face_id));
        // No face is offered twice.
        let mut sorted = chain.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), chain.len());
    }

    #[test]
    fn test_face_for_char_never_returns_a_face_without_the_glyph() {
        // RF-46 / CL-14: fallback either finds a face that really has the glyph
        // or reports `None` (the caller may then draw `.notdef`).
        let font = FontHandle::new(&FontConfig::default());
        for ch in ['A', '─', '🦀', '\u{1F3FB}', '\u{10FFFD}'] {
            if let Some(id) = font.face_for_char(ch) {
                assert!(
                    font.face_has_glyph(id, ch),
                    "face_for_char({ch:?}) returned a face without the glyph"
                );
            }
        }
    }

    #[test]
    fn test_styled_faces_share_the_family() {
        let font = FontHandle::new(&FontConfig::default());

        // The regular style always resolves to the configured face.
        assert_eq!(font.styled_face(false, false), Some(font.face_id));

        // Any styled face reported must be a *different* face of the same
        // family: the renderer uses it instead of fattening the regular
        // outline, which mangles details such as the acute accent of `í`.
        let family = |id| {
            font.db
                .face(id)
                .and_then(|face| face.families.first().map(|(name, _)| name.clone()))
        };
        for (bold, italic) in [(true, false), (false, true), (true, true)] {
            if let Some(id) = font.styled_face(bold, italic) {
                assert_ne!(
                    id, font.face_id,
                    "a styled face must not be the regular one"
                );
                assert_eq!(
                    family(id),
                    family(font.face_id),
                    "a styled face must belong to the same family"
                );
            }
        }
    }

    #[test]
    fn test_character_level_shift() {
        let cls = CharacterLevelShift::new(0, 2, 16.0);
        assert_eq!(cls.position, 0);
        assert_eq!(cls.total, 2);
        assert!((cls.advance - 16.0).abs() < f32::EPSILON);
        assert!((cls.offset() - 0.0).abs() < f32::EPSILON);

        let cls2 = CharacterLevelShift::new(1, 2, 16.0);
        assert!((cls2.offset() - 16.0).abs() < f32::EPSILON);
    }
}
