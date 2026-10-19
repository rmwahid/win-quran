//! Byte-exact build inputs.
//!
//! Everything here is embedded at compile time, so the widget has no data file to
//! locate at runtime and cannot show a text other than the one it was built with.
//! Each blob is pinned by SHA-256; `scripts/convert-quran-data.mjs --check`
//! verifies the same pins on disk before anything is committed.

/// The Tanzil Uthmani v1.1 text, verbatim.
///
/// The file must keep LF line endings. A CRLF checkout changes its SHA-256 and
/// leaves a trailing byte on every line, which is why `.gitattributes` pins
/// `text eol=lf` for it and why a test asserts there is no `\r` in this string.
pub const QURAN_TEXT: &str = include_str!("../assets/quran-uthmani.txt");

/// SHA-256 of `assets/quran-uthmani.txt`.
///
/// Pinned by Tanzil and identical to the value the upstream `quran-turn` plugin
/// ships in its `data/SHA256SUMS`. Enforced on disk by
/// `scripts/convert-quran-data.mjs --check`.
pub const QURAN_TEXT_SHA256: &str = "6933e133dd56db778c801bf738848454e43648105a151e8d84d86a7cae39ec5f";

/// SHA-256 of `assets/fonts/AmiriQuran.woff2`, the font the text is shaped with.
/// The rest of the card (chrome text) uses Manrope and DM Mono, pinned in the
/// conversion script under this project's own recorded hashes.
pub const AMIRI_QURAN_SHA256: &str = "2c01d1ccdc75bf003a6fd1cae6ea8603ee5e49dea1e4fb6780ab75b30af27128";

/// The Amiri Quran font exactly as it is shipped and pinned, in WOFF2 form.
///
/// `shaping::Font` decompresses it once at startup. Keeping the pinned bytes
/// rather than a converted copy means the font that shapes the text is the font
/// whose SHA-256 is recorded above.
pub const AMIRI_QURAN_WOFF2: &[u8] = include_bytes!("../assets/fonts/AmiriQuran.woff2");

/// Manrope, the card's chrome font, in its pinned WOFF2 form.
pub const MANROPE_WOFF2: &[u8] = include_bytes!("../assets/fonts/Manrope.woff2");

/// DM Mono, the card's reference and counter font, in its pinned WOFF2 form.
pub const DM_MONO_WOFF2: &[u8] = include_bytes!("../assets/fonts/DMMono-Regular.woff2");
