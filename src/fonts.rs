//! The fonts the card draws with, decoded once from the pinned WOFF2 assets.
//!
//! Every font carries its own shaper and the metrics the layout needs, so a caller
//! asks for a role and gets everything required to measure and place text.

use rustybuzz::ttf_parser::Face as OutlineFace;

use crate::assets;
use crate::shaping::{Font, FontError, Shaped, Shaper};

/// Which of the card's fonts a piece of text is drawn in.
///
/// These are the three faces the upstream stylesheet names: Amiri Quran for the
/// ayah, Manrope for the card's chrome, and DM Mono for references and counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Arabic,
    Sans,
    Mono,
}

/// A font with its shaper and the metrics text layout needs.
pub struct Typeface {
    font: Font,
    shaper: Shaper,
    /// Font units per em.
    pub upem: f32,
    /// Ascender in font units, y up.
    pub ascender: f32,
    /// Descender in font units, y up, so this is negative.
    pub descender: f32,
}

impl Typeface {
    fn load(woff2: &[u8]) -> Result<Self, FontError> {
        let font = Font::from_woff2(woff2)?;
        let shaper = Shaper::new(&font)?;
        let outlines = OutlineFace::parse(font.data(), 0).map_err(|_| FontError::Unusable)?;
        Ok(Self {
            upem: f32::from(outlines.units_per_em()),
            ascender: f32::from(outlines.ascender()),
            descender: f32::from(outlines.descender()),
            font,
            shaper,
        })
    }

    /// The font, for rasterizing its outlines.
    pub fn font(&self) -> &Font {
        &self.font
    }

    /// The shaper, for turning text into placed glyphs.
    pub fn shaper(&self) -> &Shaper {
        &self.shaper
    }

    /// Width of a shaped run at `size`, including any letter spacing.
    pub fn advance(&self, shaped: &Shaped, size: f32, letter_spacing: f32) -> f32 {
        let scale = size / self.upem;
        let advances: f32 = shaped
            .positions()
            .iter()
            .map(|position| position.x_advance as f32)
            .sum();
        let gaps = shaped.len().saturating_sub(1) as f32 * letter_spacing;
        advances * scale + gaps
    }

    /// Width of `text` at `size`, shaped left to right.
    pub fn width(&self, text: &str, size: f32, letter_spacing: f32) -> f32 {
        self.advance(&self.shaper.shape_ltr(text), size, letter_spacing)
    }

    /// Where the baseline sits inside a line box, using the half-leading model a
    /// browser uses: centre the font's own ascent and descent in the line box.
    pub fn baseline_in(&self, box_top: f32, box_height: f32, size: f32) -> f32 {
        let scale = size / self.upem;
        let ascent = self.ascender * scale;
        let descent = self.descender * scale;
        let content = ascent - descent;
        box_top + (box_height - content) / 2.0 + ascent
    }
}

/// The card's three fonts.
pub struct Fonts {
    arabic: Typeface,
    sans: Typeface,
    mono: Typeface,
}

impl Fonts {
    /// Decodes all three fonts from the assets this binary carries.
    pub fn load() -> Result<Self, FontError> {
        Ok(Self {
            arabic: Typeface::load(assets::AMIRI_QURAN_WOFF2)?,
            sans: Typeface::load(assets::MANROPE_WOFF2)?,
            mono: Typeface::load(assets::DM_MONO_WOFF2)?,
        })
    }

    /// The typeface for a role.
    pub fn get(&self, role: Role) -> &Typeface {
        match role {
            Role::Arabic => &self.arabic,
            Role::Sans => &self.sans,
            Role::Mono => &self.mono,
        }
    }
}
