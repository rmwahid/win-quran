//! Cross-checks between the two bundled data sources.
//!
//! The text file and the metadata table are separate Tanzil artefacts, so
//! agreement between them is evidence that both survived the trip intact. The
//! reader refuses to draw anything when they disagree.

use std::error::Error;
use std::fmt;

use crate::metadata::{self, AYAH_COUNT};
use crate::tanzil::{Quran, SURA_COUNT};

/// A disagreement between the text and the metadata table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mismatch {
    /// The metadata table does not describe 114 suras.
    SuraCount { found: usize },
    /// One sura's ayah count differs between the text and the metadata.
    AyahCount {
        sura: u16,
        text: u16,
        metadata: u16,
    },
    /// The text holds a different number of ayat than Tanzil counts.
    Total { found: usize, expected: u32 },
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SuraCount { found } => {
                write!(f, "the metadata table holds {found} suras, expected {SURA_COUNT}")
            }
            Self::AyahCount {
                sura,
                text,
                metadata,
            } => write!(
                f,
                "sura {sura} has {text} ayat in the text but {metadata} in the metadata table"
            ),
            Self::Total { found, expected } => {
                write!(f, "the text holds {found} ayat, expected {expected}")
            }
        }
    }
}

impl Error for Mismatch {}

/// Checks a parsed mushaf against the metadata table and the pinned totals.
pub fn against_metadata(quran: &Quran<'_>) -> Result<(), Mismatch> {
    if metadata::SURAS.len() != usize::from(SURA_COUNT) {
        return Err(Mismatch::SuraCount {
            found: metadata::SURAS.len(),
        });
    }

    for sura in metadata::SURAS {
        let text = quran.aya_count(sura.number).unwrap_or(0);
        if text != sura.ayah_count {
            return Err(Mismatch::AyahCount {
                sura: sura.number,
                text,
                metadata: sura.ayah_count,
            });
        }
    }

    let found = quran.total_ayat();
    if found != AYAH_COUNT as usize {
        return Err(Mismatch::Total {
            found,
            expected: AYAH_COUNT,
        });
    }

    Ok(())
}
