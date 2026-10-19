// The widget is a window, not a console program: double clicking it must not open a
// black console window. The command line tools still print, because `main` attaches
// to the console of whatever launched it before anything is written.
#![windows_subsystem = "windows"]

//! Diagnostic entry point.
//!
//! With no arguments this starts the widget, which is what someone double clicking
//! the executable wants. The other commands exist for development and for checking
//! the bundled data.

use std::collections::BTreeSet;
use std::ops::RangeInclusive;
use std::process::ExitCode;

use rustybuzz::{Feature, ttf_parser::Tag};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use win_quran::card::{self, Card};
use win_quran::fonts::Fonts;
use win_quran::raster;
use win_quran::shaping::{Font, Shaper};
use win_quran::{SURA_COUNT, arabic, assets, basmala, metadata, quran, verify};

/// Number of suras Tanzil prefixes the basmala to, that is every sura but 1 and 9.
const SURAS_WITH_BASMALA: u16 = SURA_COUNT - 2;

/// A dual joining letter, so its forms differ by position.
const PROBE_LETTER: char = '\u{062C}';

/// LAM followed by ALEF forms the mandatory lam-alef ligature.
const LIGATURE_RUN: [char; 2] = ['\u{0644}', '\u{0627}'];

/// Codepoint ranges that attach to a base letter rather than sitting on their own.
const MARK_RANGES: [RangeInclusive<char>; 4] = [
    '\u{0610}'..='\u{061A}',
    '\u{064B}'..='\u{065F}',
    '\u{0670}'..='\u{0670}',
    '\u{06D6}'..='\u{06ED}',
];

/// The ayah end marker, which the reader generates for display.
const END_OF_AYAH: char = '\u{06DD}';

fn is_mark(character: char) -> bool {
    MARK_RANGES.iter().any(|range| range.contains(&character))
}

fn main() -> ExitCode {
    // A windowed program starts with no console. When it was launched from a
    // terminal there is one to borrow, and borrowing it before anything is printed
    // is what keeps the command line tools usable from a shell. A double click has
    // no parent console and nothing to attach to, which is exactly right.
    // SAFETY: attaching this process to its parent's console, a plain Win32 call.
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }

    let mut args = std::env::args().skip(1);
    let command = args.next();
    let rest: Vec<String> = args.collect();
    match command.as_deref() {
        // No arguments means "be the widget", because that is what double clicking
        // the executable does.
        None | Some("run") => win_quran::platform::run(),
        Some("info") => info(),
        Some("verify") => checks(),
        Some("shaping") => shaping_report(),
        Some("marker") => marker_report(),
        Some("dump") => dump(&rest),
        Some("icon") => icon(&rest),
        Some("autostart") => autostart(rest.first().map(String::as_str)),
        Some("help" | "-h" | "--help") => {
            usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("win-quran: unknown command `{other}`");
            usage();
            ExitCode::from(2)
        }
    }
}

fn usage() {
    println!(
        "win-quran {}\n\n  run                           show the widget and stay in it\n  info                          summarise the bundled mushaf (default)\n  verify                        run every data integrity check\n  shaping                       shape the whole mushaf and report what the font produced\n  dump [--surah N] [--aya N] [--light] [--out PATH]\n                                render the card to a PNG\n  autostart on | off            add or remove the per-user startup entry\n  help                          show this text\n\nIn the widget: left and right arrows (or j and k) move, t toggles always on top,\na right click does the same, the top bar drags the window, and x or Escape closes it.",
        env!("CARGO_PKG_VERSION")
    );
}

/// Reads the value that follows a flag.
fn value(args: &[String], flag: &str) -> Option<String> {
    let index = args.iter().position(|arg| arg == flag)?;
    args.get(index + 1).cloned()
}

/// Turns the widget's entry in the per-user Run key on or off.
///
/// The entry is written for the current user only, and it names this executable
/// with the `run` argument, so uninstalling is a matter of running the same command
/// with `off` or deleting the value by hand.
fn autostart(action: Option<&str>) -> ExitCode {
    match action.unwrap_or("status") {
        "on" | "off" => {
            let enabled = action == Some("on");
            match win_quran::platform::set_autostart(enabled) {
                Ok(()) => {
                    let exe = std::env::current_exe()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|_| "this executable".to_string());
                    if enabled {
                        println!("autostart on: `\"{exe}\" run` will start with Windows");
                    } else {
                        println!("autostart off: the Run entry was removed");
                    }
                    ExitCode::SUCCESS
                }
                Err(problem) => {
                    eprintln!("win-quran: {problem}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            println!("usage: win-quran autostart on | off");
            ExitCode::SUCCESS
        }
    }
}

/// Draws the application icon and writes it as a PNG and as an ICO.
fn icon(args: &[String]) -> ExitCode {
    let out = value(args, "--out").unwrap_or_else(|| "icon.png".to_string());
    let fonts = match Fonts::load() {
        Ok(fonts) => fonts,
        Err(error) => {
            eprintln!("win-quran: {error}");
            return ExitCode::FAILURE;
        }
    };

    let canvas = win_quran::icon::canvas(&fonts);
    let png = raster::encode_png(canvas.width, canvas.height, &canvas.straight_rgba());
    let ico = win_quran::icon::ico(&png);

    let ico_path = std::path::Path::new(&out).with_extension("ico");
    if let Err(error) = std::fs::write(&out, &png) {
        eprintln!("win-quran: could not write {out}: {error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = std::fs::write(&ico_path, &ico) {
        eprintln!("win-quran: could not write {}: {error}", ico_path.display());
        return ExitCode::FAILURE;
    }

    println!(
        "icon        {}x{} px, written to {out} and {}",
        canvas.width,
        canvas.height,
        ico_path.display()
    );
    ExitCode::SUCCESS
}

/// Reports how the end-of-ayah marker shapes under several OpenType feature sets.
///
/// The card draws U+06DD followed by Arabic-Indic digits and the digits land beside
/// the ornament rather than inside it. This asks the font directly whether any
/// feature fuses the pair, instead of guessing.
fn marker_report() -> ExitCode {
    let font = match Font::amiri_quran() {
        Ok(font) => font,
        Err(error) => {
            eprintln!("win-quran: {error}");
            return ExitCode::FAILURE;
        }
    };
    let shaper = match Shaper::new(&font) {
        Ok(shaper) => shaper,
        Err(error) => {
            eprintln!("win-quran: {error}");
            return ExitCode::FAILURE;
        }
    };

    let feature = |tag: &[u8; 4]| Feature::new(Tag::from_bytes(tag), 1, ..);
    let run = format!("\u{06DD}{}", arabic::digits(286));
    let sets: [(&str, Vec<Feature>); 8] = [
        ("default", vec![]),
        ("ccmp", vec![feature(b"ccmp")]),
        ("rlig", vec![feature(b"rlig")]),
        ("liga", vec![feature(b"liga")]),
        ("calt", vec![feature(b"calt")]),
        ("mark", vec![feature(b"mark")]),
        ("mkmk", vec![feature(b"mkmk")]),
        (
            "all",
            vec![
                feature(b"ccmp"),
                feature(b"rlig"),
                feature(b"liga"),
                feature(b"calt"),
                feature(b"mark"),
                feature(b"mkmk"),
            ],
        ),
    ];

    println!("marker       U+06DD followed by the digits of 286");
    for (label, features) in sets {
        let shaped = shaper.shape_with_features(&run, &features);
        println!(
            "{label:8}     glyphs={} ids={:?}",
            shaped.len(),
            shaped.glyph_ids()
        );
    }
    let ornament = shaper.shape("\u{06DD}");
    let digits = shaper.shape(&arabic::digits(286));
    println!(
        "ornament     glyphs={} ids={:?}",
        ornament.len(),
        ornament.glyph_ids()
    );
    println!(
        "digits       glyphs={} ids={:?}",
        digits.len(),
        digits.glyph_ids()
    );

    // Positions and ink boxes are what decide whether the font intends the digits
    // inside the ornament: an enclosing layout needs their ink to overlap it.
    let mut pen = 0.0f32;
    let shaped = shaper.shape(&run);
    let outlines = rustybuzz::ttf_parser::Face::parse(font.data(), 0).ok();
    println!();
    println!("glyph   advance   x_offset   pen_x   ink range (font units)");
    for (info, position) in shaped.infos().iter().zip(shaped.positions()) {
        let x = pen + position.x_offset as f32;
        let ink = outlines
            .as_ref()
            .and_then(|face| {
                face.glyph_bounding_box(rustybuzz::ttf_parser::GlyphId(info.glyph_id as u16))
            })
            .map(|rect| {
                format!(
                    "{:.0} to {:.0}",
                    x + f32::from(rect.x_min),
                    x + f32::from(rect.x_max)
                )
            })
            .unwrap_or_else(|| "no outline".to_string());
        println!(
            "{:5}   {:7}   {:8}   {:5.0}   {ink}",
            info.glyph_id, position.x_advance, position.x_offset, pen
        );
        pen += position.x_advance as f32;
    }
    println!("total advance {pen:.0} font units");
    ExitCode::SUCCESS
}

/// Renders the card to a PNG.
///
/// This is how the drawing is inspected without a window: the same layout and the
/// same rasterizer the window uses, written to a file instead of to a layered
/// surface.
fn dump(args: &[String]) -> ExitCode {
    let surah: u16 = value(args, "--surah")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(1);
    let aya: u16 = value(args, "--aya")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(1);
    let light = args.iter().any(|arg| arg == "--light");
    let scroll: usize = value(args, "--scroll")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(0);
    let out = value(args, "--out").unwrap_or_else(|| "card.png".to_string());

    let mushaf = match quran() {
        Ok(mushaf) => mushaf,
        Err(error) => {
            eprintln!("win-quran: the bundled text does not parse: {error}");
            return ExitCode::FAILURE;
        }
    };
    let Some(text) = mushaf.verse(surah, aya) else {
        eprintln!("win-quran: {surah}:{aya} is outside the mushaf");
        return ExitCode::FAILURE;
    };
    let Some(sura) = win_quran::sura(surah) else {
        eprintln!("win-quran: sura {surah} is outside the mushaf");
        return ExitCode::FAILURE;
    };

    let fonts = match Fonts::load() {
        Ok(fonts) => fonts,
        Err(error) => {
            eprintln!("win-quran: {error}");
            return ExitCode::FAILURE;
        }
    };

    let palette = if light { &card::LIGHT } else { &card::DARK };
    let counter = format!("{} {aya}", sura.transliteration);
    let reference = format!("{} \u{b7} {surah}:{aya}", sura.transliteration.to_uppercase());
    let screen = if args.iter().any(|arg| arg == "--list") {
        card::Screen::SurahList {
            first: usize::from(surah).saturating_sub(1).min(114 - card::LIST_ROWS),
        }
    } else {
        card::Screen::Ayah
    };
    let (ops, card_layout) = card::layout(
        &Card {
            surah,
            basmala: mushaf.verse(1, 1).unwrap_or_default(),
            ayah: text,
            aya,
            reference: &reference,
            pinned: true,
            counter: &counter,
        },
        &screen,
        scroll,
        palette,
        &fonts,
    );
    let (canvas, report) = raster::render(&ops, &fonts);
    let png = raster::encode_png(canvas.width, canvas.height, &canvas.straight_rgba());

    if let Err(error) = std::fs::write(&out, &png) {
        eprintln!("win-quran: could not write {out}: {error}");
        return ExitCode::FAILURE;
    }

    println!(
        "card        {}x{} px, {} palette",
        canvas.width,
        canvas.height,
        if light { "light" } else { "dark" }
    );
    println!(
        "screen      {}",
        match screen {
            card::Screen::Ayah => format!("ayah, {} line(s)", card_layout.ayah_lines),
            card::Screen::SurahList { first } => {
                format!("sura list from row {first}")
            }
        }
    );
    println!(
        "reading     {surah}:{aya}, {} glyphs drawn, {} without an outline",
        report.glyphs, report.missing
    );
    if card_layout.overflowing {
        println!("note        the ayah is taller than the stage, which the card scrolls for");
    }
    if let Some((x0, y0, x1, y1)) = report.ink {
        println!("ink         x {x0:.1} to {x1:.1}, y {y0:.1} to {y1:.1}");
    }
    println!(
        "written     {out} ({} bytes)",
        png.len()
    );
    ExitCode::SUCCESS
}

fn info() -> ExitCode {
    let mushaf = match quran() {
        Ok(mushaf) => mushaf,
        Err(error) => {
            eprintln!("FAIL  the bundled text does not parse: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(mismatch) = verify::against_metadata(&mushaf) {
        eprintln!("FAIL  the bundled text and the metadata table disagree: {mismatch}");
        return ExitCode::FAILURE;
    }

    println!("win-quran {}", env!("CARGO_PKG_VERSION"));
    println!("mushaf:    {SURA_COUNT} suras, {} ayat", mushaf.total_ayat());
    println!("integrity: the text and the metadata table agree");
    if let Some(first) = metadata::SURAS.first() {
        println!(
            "first:     {} ({}, {} ayat)",
            first.transliteration, first.english_name, first.ayah_count
        );
    }
    if let Some(last) = metadata::SURAS.last() {
        println!(
            "last:      {} ({}, {} ayat)",
            last.transliteration, last.english_name, last.ayah_count
        );
    }
    ExitCode::SUCCESS
}

/// Collects check results in the same shape the data tooling prints.
struct Report {
    failures: usize,
}

impl Report {
    fn ok(&self, label: &str) {
        println!("ok    {label}");
    }

    fn fail(&mut self, label: &str, detail: &str) {
        self.failures += 1;
        println!("FAIL  {label}: {detail}");
    }

    fn finish(&self) -> ExitCode {
        if self.failures > 0 {
            println!("\n{} check(s) failed", self.failures);
            return ExitCode::FAILURE;
        }
        println!("\nall checks passed");
        ExitCode::SUCCESS
    }
}

fn checks() -> ExitCode {
    let mut report = Report { failures: 0 };

    let mushaf = match quran() {
        Ok(mushaf) => {
            report.ok("the bundled text parses as a complete mushaf");
            mushaf
        }
        Err(error) => {
            report.fail(
                "the bundled text parses as a complete mushaf",
                &error.to_string(),
            );
            return report.finish();
        }
    };

    match verify::against_metadata(&mushaf) {
        Ok(()) => report.ok("every sura's ayah count agrees with the metadata table"),
        Err(mismatch) => report.fail("the text and the metadata table agree", &mismatch.to_string()),
    }

    if mushaf.total_ayat() == metadata::AYAH_COUNT as usize {
        report.ok(&format!(
            "the text holds {} ayat, as Tanzil counts them",
            metadata::AYAH_COUNT
        ));
    } else {
        report.fail(
            "the text holds the pinned number of ayat",
            &format!(
                "found {}, expected {}",
                mushaf.total_ayat(),
                metadata::AYAH_COUNT
            ),
        );
    }

    if assets::QURAN_TEXT.contains('\r') {
        report.fail(
            "the text has LF line endings",
            "a carriage return was found, so the file was checked out with CRLF endings",
        );
    } else {
        report.ok("the text has LF line endings");
    }

    let fatiha = mushaf.verse(1, 1).unwrap_or_default();
    let mut splits: u16 = 0;
    let mut lossless = true;
    for number in 1..=SURA_COUNT {
        let Some(text) = mushaf.verse(number, 1) else {
            lossless = false;
            continue;
        };
        if let Some((basmala, rest)) = basmala::split(number, 1, text, fatiha) {
            splits += 1;
            if basmala.len() + 1 + rest.len() != text.len() {
                lossless = false;
            }
        }
    }
    if !lossless {
        report.fail(
            "every basmala split rebuilds its ayah",
            "at least one split did not round trip byte for byte",
        );
    } else if splits != SURAS_WITH_BASMALA {
        report.fail(
            "the basmala prefix is recognised on every sura that carries it",
            &format!("recognised on {splits} suras, expected {SURAS_WITH_BASMALA}"),
        );
    } else {
        report.ok(&format!(
            "the basmala prefix splits losslessly on {splits} suras"
        ));
    }

    let marker = format!(
        "{END_OF_AYAH}{}",
        arabic::digits(u32::from(mushaf.aya_count(2).unwrap_or(0)))
    );
    report.ok(&format!(
        "the ayah end marker renders as {} codepoints",
        marker.chars().count()
    ));

    report.finish()
}

/// Shapes the whole mushaf and reports what the font and the shaper produced.
///
/// This is the report that decides whether Uthmani text can be drawn natively at
/// all: a missing glyph means the font cannot render some character, and a letter
/// that takes one form everywhere would mean contextual substitution is not
/// happening.
fn shaping_report() -> ExitCode {
    let mut report = Report { failures: 0 };

    let mushaf = match quran() {
        Ok(mushaf) => mushaf,
        Err(error) => {
            report.fail("the bundled text parses", &error.to_string());
            return report.finish();
        }
    };

    let font = match Font::amiri_quran() {
        Ok(font) => font,
        Err(error) => {
            report.fail("the pinned font decompresses", &error.to_string());
            return report.finish();
        }
    };

    let shaper = match Shaper::new(&font) {
        Ok(shaper) => shaper,
        Err(error) => {
            report.fail("the pinned font is usable by the shaper", &error.to_string());
            return report.finish();
        }
    };

    let mut ayat = 0usize;
    let mut codepoints = 0usize;
    let mut glyphs = 0usize;
    let mut missing = 0usize;
    let mut attached = 0usize;
    let mut chained_marks = 0usize;
    let mut end_markers = 0usize;
    let mut alphabet: BTreeSet<char> = BTreeSet::new();
    let mut longest = (0u16, 0u16, 0usize);

    for (sura, aya, text) in mushaf.iter() {
        let count = text.chars().count();
        if count > longest.2 {
            longest = (sura, aya, count);
        }
        alphabet.extend(text.chars());
        if text.contains(END_OF_AYAH) {
            end_markers += 1;
        }
        let mut previous_was_mark = false;
        for character in text.chars() {
            let mark = is_mark(character);
            if mark && previous_was_mark {
                chained_marks += 1;
            }
            previous_was_mark = mark;
        }

        let shaped = shaper.shape(text);
        if shaped.is_empty() {
            report.fail(
                &format!("ayah {sura}:{aya} shapes"),
                "the shaper produced no glyph at all",
            );
        }
        missing += shaped.missing();
        attached += shaped.attached();
        glyphs += shaped.len();
        codepoints += shaped.codepoints();
        ayat += 1;
    }

    let mut probe = shaper.form_probe(PROBE_LETTER);
    probe.joined.sort_unstable();
    probe.joined.dedup();
    let run: String = LIGATURE_RUN.iter().collect();
    let ligature = shaper.shape(&run);
    // Ask for the mandatory ligature feature explicitly. If the pair still does not
    // merge, the font draws it as two positioned glyphs and the feature was never
    // the reason.
    let required = Feature::new(Tag::from_bytes(b"rlig"), 1, ..);
    let forced = shaper.shape_with_features(&run, &[required]);

    // A joined pair must not fall back to the isolated form of either letter, since
    // that is what would visibly break the connection. Whether the pair merges into
    // one glyph or stays two positioned glyphs is the font's choice: Amiri Quran
    // keeps two, even with the mandatory ligature feature forced on, so glyph count
    // is not the thing to check.
    let isolated_forms: Vec<u32> = LIGATURE_RUN
        .iter()
        .filter_map(|letter| {
            shaper
                .shape(&letter.to_string())
                .glyph_ids()
                .first()
                .copied()
        })
        .collect();
    let joined_forms_only = ligature
        .glyph_ids()
        .iter()
        .all(|glyph| !isolated_forms.contains(glyph));

    // How much of the text merges at all: marks are separate glyphs, so a ratio
    // below one codepoint per glyph can only come from ligatures and substitutions.
    let merged_ayat = mushaf
        .iter()
        .filter(|(_, _, text)| shaper.shape(text).len() < text.chars().count())
        .count();

    println!("win-quran {} shaping report", env!("CARGO_PKG_VERSION"));
    println!();
    println!(
        "font         Amiri Quran, {} bytes decompressed, {} glyphs available",
        font.data().len(),
        shaper.glyph_count()
    );
    println!(
        "text         {ayat} ayat, {codepoints} codepoints, {} distinct codepoints",
        alphabet.len()
    );
    println!(
        "shaped       {glyphs} glyphs, {:.3} per codepoint",
        glyphs as f64 / codepoints as f64
    );
    println!(
        "longest      {}:{} at {} codepoints",
        longest.0, longest.1, longest.2
    );
    println!("mark pairs   {chained_marks} places where two marks sit on one base");
    println!(
        "end marker   {end_markers} ayat contain U+06DD, which is generated for display rather than shipped"
    );
    println!(
        "letter probe isolated glyph {:?}, joined glyphs {:?}",
        probe.isolated, probe.joined
    );
    println!(
        "pair probe   lam-alef: {} codepoints became {} glyphs, {} with rlig forced, joined forms only: {joined_forms_only}",
        ligature.codepoints(),
        ligature.len(),
        forced.len()
    );
    println!(
        "merging      {merged_ayat} of {ayat} ayat shape to fewer glyphs than codepoints"
    );
    println!();

    if ayat == metadata::AYAH_COUNT as usize {
        report.ok(&format!("all {ayat} ayat were shaped"));
    } else {
        report.fail("every ayah was shaped", &format!("shaped {ayat}"));
    }

    if missing == 0 {
        report.ok("no ayah needs a glyph the font does not provide");
    } else {
        report.fail(
            "every codepoint has a glyph in the font",
            &format!("{missing} glyphs came back missing"),
        );
    }

    if probe.is_contextual() {
        report.ok("a letter takes different forms alone and joined");
    } else {
        report.fail(
            "contextual letter forms are applied",
            "the joined run did not use a different form than the isolated letter",
        );
    }

    if ligature.missing() == 0 && joined_forms_only {
        report.ok("a joined pair uses joined forms rather than the isolated forms");
    } else {
        report.fail(
            "a joined pair uses joined forms",
            &format!(
                "lam-alef produced {:?}, isolated forms are {isolated_forms:?}",
                ligature.glyph_ids()
            ),
        );
    }

    if merged_ayat * 2 > ayat {
        report.ok(&format!(
            "{merged_ayat} of {ayat} ayat merge codepoints through substitutions"
        ));
    } else {
        report.fail(
            "substitutions merge codepoints across the text",
            &format!("only {merged_ayat} of {ayat} ayat did"),
        );
    }

    if attached > 0 {
        report.ok(&format!(
            "{attached} glyphs were placed by attachment rather than by advance"
        ));
    } else {
        report.fail(
            "marks are attached to their base",
            "no glyph carried a placement offset",
        );
    }

    report.finish()
}
