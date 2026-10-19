//! Shaping verification over the whole mushaf.
//!
//! The decisive question for a native reader is whether the bundled font can
//! render every character of the Uthmani text through a real shaper. A glyph the
//! font does not provide is drawn as `.notdef`, so counting those across all 6,236
//! ayat answers that question exactly. The three smaller checks prove the shaper
//! is doing real work rather than falling back to isolated forms.

use win_quran::metadata;
use win_quran::quran;
use win_quran::shaping::{Font, Shaper};

/// A dual joining letter, so its forms differ by position.
const JOINING_LETTER: char = '\u{062C}';

/// LAM followed by ALEF forms the mandatory lam-alef ligature.
const LIGATURE_RUN: [char; 2] = ['\u{0644}', '\u{0627}'];

/// How many ayat a spot check walks when the whole mushaf is not the point.
const SPOT_CHECK_AYAT: usize = 100;

#[test]
fn the_pinned_font_decompresses_and_is_usable() {
    let font = Font::amiri_quran().expect("the pinned WOFF2 must decompress");
    assert!(!font.data().is_empty(), "the decompressed font must have bytes");

    let shaper = Shaper::new(&font).expect("the decompressed font must be usable");
    assert!(
        shaper.glyph_count() > 1000,
        "Amiri Quran carries thousands of glyphs, got {}",
        shaper.glyph_count()
    );
}

#[test]
fn every_ayah_shapes_without_a_missing_glyph() {
    let font = Font::amiri_quran().expect("the pinned WOFF2 must decompress");
    let shaper = Shaper::new(&font).expect("the decompressed font must be usable");
    let mushaf = quran().expect("the bundled text must parse");

    let mut shaped_ayat = 0usize;
    let mut glyphs = 0usize;
    for (sura, aya, text) in mushaf.iter() {
        let shaped = shaper.shape(text);
        assert!(
            !shaped.is_empty(),
            "ayah {sura}:{aya} produced no glyph at all"
        );
        assert_eq!(
            shaped.missing(),
            0,
            "ayah {sura}:{aya} needs a glyph the font does not provide"
        );
        glyphs += shaped.len();
        shaped_ayat += 1;
    }

    assert_eq!(shaped_ayat, metadata::AYAH_COUNT as usize);
    assert!(
        glyphs >= shaped_ayat,
        "shaping must produce at least one glyph per ayah"
    );
}

#[test]
fn contextual_letter_forms_differ_by_position() {
    let font = Font::amiri_quran().expect("the pinned WOFF2 must decompress");
    let shaper = Shaper::new(&font).expect("the decompressed font must be usable");

    let probe = shaper.form_probe(JOINING_LETTER);
    assert!(
        probe.is_contextual(),
        "a letter must not take the same glyph alone and joined, got {probe:?}"
    );
}

#[test]
fn a_joined_pair_never_falls_back_to_the_isolated_forms() {
    let font = Font::amiri_quran().expect("the pinned WOFF2 must decompress");
    let shaper = Shaper::new(&font).expect("the decompressed font must be usable");

    let isolated: Vec<u32> = LIGATURE_RUN
        .iter()
        .filter_map(|letter| {
            shaper
                .shape(&letter.to_string())
                .glyph_ids()
                .first()
                .copied()
        })
        .collect();
    assert_eq!(isolated.len(), LIGATURE_RUN.len());

    // Amiri Quran keeps lam-alef as two positioned glyphs rather than merging them
    // into one, so glyph count is not what proves this pair is joined. What must
    // hold is that neither letter falls back to the form it takes standing alone,
    // because that is what would visibly break the connection.
    let run: String = LIGATURE_RUN.iter().collect();
    let shaped = shaper.shape(&run);
    assert_eq!(shaped.codepoints(), LIGATURE_RUN.len());
    assert_eq!(shaped.missing(), 0);
    for glyph in shaped.glyph_ids() {
        assert!(
            !isolated.contains(&glyph),
            "a joined letter was drawn in its isolated form, glyph {glyph}"
        );
    }
}

#[test]
fn substitutions_merge_codepoints_across_the_mushaf() {
    // Harakat are placed as their own glyphs, so a run that ends up with fewer
    // glyphs than codepoints can only get there through ligatures or substitutions.
    // Finding that in most ayat is evidence the machinery runs on the real text and
    // not only on a two letter probe.
    let font = Font::amiri_quran().expect("the pinned WOFF2 must decompress");
    let shaper = Shaper::new(&font).expect("the decompressed font must be usable");
    let mushaf = quran().expect("the bundled text must parse");

    let mut merging = 0usize;
    let mut ayat = 0usize;
    for (_, _, text) in mushaf.iter() {
        if shaper.shape(text).len() < text.chars().count() {
            merging += 1;
        }
        ayat += 1;
    }

    assert!(
        merging * 2 > ayat,
        "substitutions must merge codepoints in most ayat, but only {merging} of {ayat} did"
    );
}

#[test]
fn harakat_are_attached_rather_than_left_on_the_advance() {
    let font = Font::amiri_quran().expect("the pinned WOFF2 must decompress");
    let shaper = Shaper::new(&font).expect("the decompressed font must be usable");
    let mushaf = quran().expect("the bundled text must parse");

    let attached: usize = mushaf
        .iter()
        .take(SPOT_CHECK_AYAT)
        .map(|(_, _, text)| shaper.shape(text).attached())
        .sum();

    assert!(
        attached > 0,
        "marks must be placed by attachment rather than by their own advance"
    );
}
