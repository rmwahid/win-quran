//! Text shaping for the mushaf.
//!
//! Uthmani text cannot be drawn glyph by glyph. Letters take contextual forms,
//! lam followed by alef forms a ligature, and the harakat attach to their base
//! through GPOS, so the text has to pass through a real shaper before anything can
//! be positioned or drawn. This module runs that shaper over the bundled text,
//! which is how a character the font cannot draw is found before a single pixel
//! exists.
//!
//! The shaper is rustybuzz, a port of HarfBuzz, and so the same engine family
//! Chromium uses for the same text in the upstream web reader.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use rustybuzz::{Direction, Face, Feature, GlyphBuffer, GlyphInfo, GlyphPosition, UnicodeBuffer};

use crate::assets;

/// Why a font could not be prepared for shaping.
#[derive(Debug)]
pub enum FontError {
    /// The pinned WOFF2 could not be decompressed.
    Decompress(String),
    /// The decompressed bytes are not a font this shaper accepts.
    Unusable,
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decompress(reason) => write!(f, "a font could not be decompressed: {reason}"),
            Self::Unusable => write!(f, "a decompressed font is not one the shaper accepts"),
        }
    }
}

impl Error for FontError {}

/// A font, decompressed from its pinned WOFF2 form.
///
/// The decompressed bytes are kept for the lifetime of the process. They are
/// leaked on purpose: a font is decoded once and outlives everything that borrows
/// it, and a `'static` slice is what lets the shaper and the rasterizer share it
/// without a self-referential struct or reference counting.
pub struct Font {
    data: &'static [u8],
}

impl Font {
    /// Decompresses a WOFF2 asset.
    pub fn from_woff2(bytes: &[u8]) -> Result<Self, FontError> {
        let data = wuff::decompress_woff2(bytes)
            .map_err(|error| FontError::Decompress(error.to_string()))?;
        Ok(Self {
            data: Box::leak(data.into_boxed_slice()),
        })
    }

    /// The Amiri Quran font the ayah is shaped with.
    pub fn amiri_quran() -> Result<Self, FontError> {
        Self::from_woff2(assets::AMIRI_QURAN_WOFF2)
    }

    /// The decompressed OpenType bytes.
    pub fn data(&self) -> &'static [u8] {
        self.data
    }
}

/// A shaper bound to one face, reused across runs.
pub struct Shaper {
    face: Face<'static>,
}

impl Shaper {
    /// Prepares a face for shaping.
    pub fn new(font: &Font) -> Result<Self, FontError> {
        let face = Face::from_slice(font.data(), 0).ok_or(FontError::Unusable)?;
        Ok(Self { face })
    }

    /// Number of glyphs the font provides.
    pub fn glyph_count(&self) -> u16 {
        self.face.tables().maxp.number_of_glyphs.get()
    }

    /// Shapes one Arabic run, right to left, with the font's default features.
    ///
    /// No feature is requested explicitly, because the upstream reader requests
    /// none either, so the shaper's defaults are what a browser applies to the same
    /// text.
    pub fn shape(&self, text: &str) -> Shaped {
        self.shape_in(text, &[], Direction::RightToLeft)
    }

    /// Shapes one left to right run, for the card's chrome text.
    pub fn shape_ltr(&self, text: &str) -> Shaped {
        self.shape_in(text, &[], Direction::LeftToRight)
    }

    /// Shapes one run with extra OpenType features on top of the script defaults.
    pub fn shape_with_features(&self, text: &str, features: &[Feature]) -> Shaped {
        self.shape_in(text, features, Direction::RightToLeft)
    }

    fn shape_in(&self, text: &str, features: &[Feature], direction: Direction) -> Shaped {
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        // The direction is forced rather than left to the guess: the mushaf is
        // Arabic throughout and the chrome text is Latin throughout.
        buffer.set_direction(direction);
        let glyphs = rustybuzz::shape(&self.face, features, buffer);
        Shaped::new(text, glyphs)
    }

    /// Shapes one letter alone and the same letter inside a three letter run.
    ///
    /// This is the cheapest evidence that contextual substitution is engaged: an
    /// unshaped fallback would give one form for both.
    pub fn form_probe(&self, letter: char) -> FormProbe {
        let isolated = self.shape(&letter.to_string()).glyph_ids();
        let joined = self.shape(&letter.to_string().repeat(3)).glyph_ids();
        FormProbe {
            isolated: isolated.first().copied(),
            joined,
        }
    }
}

/// The glyph of a letter standing alone, and the glyphs of three of them joined.
#[derive(Debug, Clone)]
pub struct FormProbe {
    /// The glyph chosen for the letter with no neighbours, if it produced one.
    pub isolated: Option<u32>,
    /// The glyphs chosen for three joined copies, in visual order.
    pub joined: Vec<u32>,
}

impl FormProbe {
    /// True when the joined run uses at least two different forms and none of them
    /// is the isolated form.
    pub fn is_contextual(&self) -> bool {
        let distinct: BTreeSet<u32> = self.joined.iter().copied().collect();
        distinct.len() >= 2 && self.isolated.is_some_and(|glyph| !distinct.contains(&glyph))
    }
}

/// What the shaper produced for one run.
pub struct Shaped {
    infos: Vec<GlyphInfo>,
    positions: Vec<GlyphPosition>,
    codepoints: usize,
}

impl Shaped {
    fn new(text: &str, glyphs: GlyphBuffer) -> Self {
        Self {
            infos: glyphs.glyph_infos().to_vec(),
            positions: glyphs.glyph_positions().to_vec(),
            codepoints: text.chars().count(),
        }
    }

    /// Number of glyphs produced.
    pub fn len(&self) -> usize {
        self.infos.len()
    }

    /// True when the run produced no glyph at all.
    pub fn is_empty(&self) -> bool {
        self.infos.is_empty()
    }

    /// Number of codepoints in the run that was shaped.
    pub fn codepoints(&self) -> usize {
        self.codepoints
    }

    /// Glyph metadata, in visual order.
    pub fn infos(&self) -> &[GlyphInfo] {
        &self.infos
    }

    /// Placement of every glyph, in visual order.
    pub fn positions(&self) -> &[GlyphPosition] {
        &self.positions
    }

    /// Glyph ids in visual order.
    pub fn glyph_ids(&self) -> Vec<u32> {
        self.infos.iter().map(|info| info.glyph_id).collect()
    }

    /// How many glyphs the shaper asked the font for and did not find.
    ///
    /// A missing glyph is drawn as `.notdef`, so any count above zero means the
    /// font cannot render this run as given.
    pub fn missing(&self) -> usize {
        self.infos.iter().filter(|info| info.glyph_id == 0).count()
    }

    /// How many glyphs were placed away from the pen position.
    ///
    /// Marks are positioned by GPOS rather than by their own advance, so a non-zero
    /// count over the mushaf is evidence that mark attachment ran.
    pub fn attached(&self) -> usize {
        self.positions
            .iter()
            .filter(|position| position.x_offset != 0 || position.y_offset != 0)
            .count()
    }
}
