//! Native Qur'an reader data layer.
//!
//! Two independent Tanzil artefacts are bundled and cross-checked: the Uthmani
//! text (`assets/quran-uthmani.txt`) and the metadata table generated from
//! `quran-data.js`. Agreement between them, plus the SHA-256 pins verified by
//! `scripts/convert-quran-data.mjs --check`, is what allows the reader to draw an
//! ayah at all.
//!
//! Nothing in this crate rewrites the text. Tanzil's licence forbids it, and the
//! parser refuses rather than guesses when a record does not look like a Tanzil
//! record.

pub mod arabic;
pub mod assets;
pub mod basmala;
pub mod card;
pub mod fonts;
pub mod icon;
#[path = "metadata_generated.rs"]
pub mod metadata;
pub mod platform;
pub mod raster;
pub mod shaping;
pub mod state;
pub mod tanzil;
pub mod verify;

pub use metadata::{AYAH_COUNT, Sura};
pub use tanzil::{ParseError, Quran, SURA_COUNT};

/// Looks up a sura by its 1-based number.
pub fn sura(number: u16) -> Option<&'static Sura> {
    metadata::SURAS.get(usize::from(number).checked_sub(1)?)
}

/// Parses the mushaf that is embedded in this binary.
pub fn quran() -> Result<Quran<'static>, ParseError> {
    Quran::parse(assets::QURAN_TEXT)
}
