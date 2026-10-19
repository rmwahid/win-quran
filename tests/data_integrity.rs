//! Integrity of the bundled data.
//!
//! The text file and the metadata table are separate Tanzil artefacts, so
//! agreement between them is real evidence that both survived the trip. Every
//! check here runs against the bytes embedded in the binary, which are the bytes
//! the widget draws. Byte level pins (SHA-256) are enforced by
//! `scripts/convert-quran-data.mjs --check`, because that tool already has a
//! digest implementation and this crate takes no dependencies.

use win_quran::tanzil::{Quran, SURA_COUNT};
use win_quran::{assets, basmala, metadata, quran, sura, verify};

/// Number of suras Tanzil prefixes the basmala to, that is every sura but 1 and 9.
const SURAS_WITH_BASMALA: u16 = SURA_COUNT - 2;

fn mushaf() -> Quran<'static> {
    quran().expect("the bundled Tanzil text must parse")
}

#[test]
fn the_text_parses_as_a_complete_mushaf() {
    let mushaf = mushaf();
    assert_eq!(mushaf.total_ayat(), metadata::AYAH_COUNT as usize);
    assert_eq!(mushaf.aya_count(SURA_COUNT), Some(6), "An-Naas has six ayat");
}

#[test]
fn the_text_has_lf_line_endings() {
    assert!(
        !assets::QURAN_TEXT.contains('\r'),
        "the bundled text contains a carriage return, so it was checked out or copied with CRLF endings"
    );
}

#[test]
fn the_metadata_describes_every_sura_once() {
    assert_eq!(metadata::SURAS.len(), usize::from(SURA_COUNT));
    let mut total = 0u32;
    for (index, sura) in metadata::SURAS.iter().enumerate() {
        assert_eq!(sura.number, index as u16 + 1, "the table must be in mushaf order");
        assert!(sura.ayah_count > 0, "sura {} has no ayat", sura.number);
        assert!(!sura.arabic_name.is_empty());
        assert!(!sura.transliteration.is_empty());
        assert!(!sura.english_name.is_empty());
        total += u32::from(sura.ayah_count);
    }
    assert_eq!(total, metadata::AYAH_COUNT);
}

#[test]
fn the_text_and_the_metadata_agree() {
    verify::against_metadata(&mushaf()).expect("the two data sources must agree");
}

#[test]
fn the_text_never_contains_the_generated_end_marker() {
    // The reader draws the ayah number as U+06DD plus Arabic-Indic digits. That
    // marker is presentation, so it must never appear in the Tanzil text itself.
    let mushaf = mushaf();
    let ayat = mushaf
        .iter()
        .filter(|(_, _, text)| text.contains('\u{06DD}'))
        .count();
    assert_eq!(ayat, 0, "U+06DD is generated for display, never shipped");
}

#[test]
fn sura_lookup_is_one_based_and_bounded() {
    let first = sura(1).expect("sura 1 exists");
    assert_eq!(first.number, 1);
    assert_eq!(first.ayah_count, 7);
    assert_eq!(sura(SURA_COUNT).map(|sura| sura.number), Some(SURA_COUNT));
    assert!(sura(0).is_none());
    assert!(sura(SURA_COUNT + 1).is_none());
}

#[test]
fn every_ayah_is_reachable_and_non_empty() {
    let mushaf = mushaf();
    let mut visited = 0usize;
    for (sura, aya, text) in mushaf.iter() {
        assert!(!text.is_empty(), "ayah {sura}:{aya} is empty");
        assert_eq!(
            mushaf.verse(sura, aya),
            Some(text),
            "ayah {sura}:{aya} is not reachable by reference"
        );
        visited += 1;
    }
    assert_eq!(visited, metadata::AYAH_COUNT as usize);
}

#[test]
fn the_basmala_prefix_splits_losslessly_on_every_sura() {
    let mushaf = mushaf();
    let fatiha = mushaf.verse(1, 1).expect("1:1 exists");

    let mut splits = 0u16;
    for number in 1..=SURA_COUNT {
        let text = mushaf.verse(number, 1).expect("ayah 1 of every sura exists");

        if number == 1 || number == 9 {
            assert!(
                basmala::split(number, 1, text, fatiha).is_none(),
                "sura {number} carries no basmala prefix"
            );
            continue;
        }

        let (prefix, rest) = basmala::split(number, 1, text, fatiha)
            .unwrap_or_else(|| panic!("sura {number} carries a basmala prefix"));
        assert_eq!(
            format!("{prefix} {rest}"),
            text,
            "the split of {number}:1 must rebuild the ayah byte for byte"
        );
        assert_eq!(
            prefix.len() + 1 + rest.len(),
            text.len(),
            "the split of {number}:1 must account for every byte"
        );
        splits += 1;
    }

    assert_eq!(
        splits, SURAS_WITH_BASMALA,
        "Tanzil prefixes the basmala to every sura except 1 and 9"
    );
}
