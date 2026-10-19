//! Parser for the Tanzil `txt-2` Qur'an text format.
//!
//! One `sura|aya|text` record per line, with an optional trailing `#` comment
//! block. Ayah strings are never normalized, trimmed or rewritten: the bytes
//! Tanzil ships are the bytes that reach the screen.
//!
//! Parsing is strict on purpose. A record that is malformed, duplicated, empty or
//! out of order aborts with an error instead of producing a mushaf that is quietly
//! missing an ayah.

use std::error::Error;
use std::fmt;

/// Number of suras in the mushaf.
pub const SURA_COUNT: u16 = 114;

/// How much of an offending line an error message keeps.
const ERROR_CONTEXT_CHARS: usize = 40;

/// `boundaries[sura]` is the index of the first ayah of that sura, so
/// `boundaries[sura + 1] - boundaries[sura]` is its ayah count. Entry 0 is
/// unused, and the row after the last sura holds the total ayah count. This
/// mirrors Tanzil's own sentinel row, which is what makes "start of the next
/// sura" work uniformly for the last sura.
const TOTAL_ROW: usize = SURA_COUNT as usize + 1;

/// A parsed mushaf, borrowing its ayah strings from the source text.
#[derive(Debug)]
pub struct Quran<'a> {
    verses: Vec<&'a str>,
    boundaries: [usize; TOTAL_ROW + 1],
}

/// Why a text file is not a usable Tanzil data set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// A line is neither blank, a comment, nor a `sura|aya|text` record.
    MalformedLine { line: usize, content: String },
    /// A field that must be a number is not one.
    NotANumber {
        line: usize,
        field: &'static str,
        content: String,
    },
    /// The sura or aya number is outside the mushaf.
    OutOfRange { line: usize, sura: u32, aya: u32 },
    /// The ayah text is empty.
    EmptyAyah { sura: u16, aya: u16 },
    /// The same ayah appears twice.
    DuplicateAyah { sura: u16, aya: u16 },
    /// Ayat of one sura are not in ascending order with no gaps.
    OutOfOrder { sura: u16, aya: u16, expected: u16 },
    /// A sura is missing, repeated, or the suras are out of order.
    SuraOutOfOrder { sura: u16, expected: u16 },
    /// Fewer than 114 suras were present when the text ended.
    Incomplete { suras_found: u16 },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedLine { line, content } => {
                write!(f, "line {line} is not a sura|aya|text record: {content:?}")
            }
            Self::NotANumber {
                line,
                field,
                content,
            } => write!(f, "line {line} has a non-numeric {field}: {content:?}"),
            Self::OutOfRange { line, sura, aya } => {
                write!(f, "line {line} points outside the mushaf: {sura}:{aya}")
            }
            Self::EmptyAyah { sura, aya } => write!(f, "ayah {sura}:{aya} has no text"),
            Self::DuplicateAyah { sura, aya } => write!(f, "ayah {sura}:{aya} appears twice"),
            Self::OutOfOrder {
                sura,
                aya,
                expected,
            } => write!(
                f,
                "ayah {sura}:{aya} arrived where {sura}:{expected} was expected"
            ),
            Self::SuraOutOfOrder { sura, expected } => {
                write!(f, "sura {sura} arrived where sura {expected} was expected")
            }
            Self::Incomplete { suras_found } => {
                write!(
                    f,
                    "the text holds {suras_found} suras, expected {SURA_COUNT}"
                )
            }
        }
    }
}

impl Error for ParseError {}

impl<'a> Quran<'a> {
    /// Parses a whole Tanzil `txt-2` file.
    pub fn parse(raw: &'a str) -> Result<Self, ParseError> {
        let mut verses: Vec<&'a str> = Vec::new();
        let mut boundaries = [0usize; TOTAL_ROW + 1];
        let mut sura: u16 = 0;
        let mut aya: u16 = 0;

        for (index, line) in raw.split('\n').enumerate() {
            let line_number = index + 1;
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (record, text) = split_record(line_number, line)?;
            let (record_sura, record_aya) = parse_numbers(line_number, record, line)?;

            if record_sura != sura {
                let expected = sura + 1;
                if record_sura != expected {
                    return Err(ParseError::SuraOutOfOrder {
                        sura: record_sura,
                        expected,
                    });
                }
                sura = record_sura;
                aya = 0;
                boundaries[usize::from(sura)] = verses.len();
            }

            if record_aya <= aya {
                return Err(ParseError::DuplicateAyah {
                    sura,
                    aya: record_aya,
                });
            }
            let expected = aya + 1;
            if record_aya != expected {
                return Err(ParseError::OutOfOrder {
                    sura,
                    aya: record_aya,
                    expected,
                });
            }
            if text.is_empty() {
                return Err(ParseError::EmptyAyah {
                    sura,
                    aya: record_aya,
                });
            }

            aya = record_aya;
            verses.push(text);
        }

        if sura != SURA_COUNT {
            return Err(ParseError::Incomplete {
                suras_found: sura,
            });
        }
        boundaries[TOTAL_ROW] = verses.len();
        Ok(Self { verses, boundaries })
    }

    /// Number of ayat in a sura, or `None` when the sura number is not 1 to 114.
    pub fn aya_count(&self, sura: u16) -> Option<u16> {
        if sura < 1 || sura > SURA_COUNT {
            return None;
        }
        let index = usize::from(sura);
        Some((self.boundaries[index + 1] - self.boundaries[index]) as u16)
    }

    /// The text of one ayah, or `None` when the reference is outside the mushaf.
    pub fn verse(&self, sura: u16, aya: u16) -> Option<&'a str> {
        if aya < 1 || aya > self.aya_count(sura)? {
            return None;
        }
        Some(self.verses[self.boundaries[usize::from(sura)] + usize::from(aya) - 1])
    }

    /// Total number of ayat in the parsed mushaf.
    pub fn total_ayat(&self) -> usize {
        self.boundaries[TOTAL_ROW]
    }

    /// Every ayah in mushaf order, as `(sura, aya, text)`.
    pub fn iter(&self) -> impl Iterator<Item = (u16, u16, &'a str)> + '_ {
        (1..=SURA_COUNT).flat_map(move |sura| {
            let first = self.boundaries[usize::from(sura)];
            let count = self.aya_count(sura).unwrap_or(0);
            (0..count).map(move |offset| (sura, offset + 1, self.verses[first + usize::from(offset)]))
        })
    }
}

/// A short excerpt of an offending line, safe to truncate in the middle of text.
fn excerpt(line: &str) -> String {
    line.chars().take(ERROR_CONTEXT_CHARS).collect()
}

fn malformed(line_number: usize, line: &str) -> ParseError {
    ParseError::MalformedLine {
        line: line_number,
        content: excerpt(line),
    }
}

/// Splits `sura|aya|text` into the `sura|aya` head and the ayah text.
fn split_record<'a>(line_number: usize, line: &'a str) -> Result<(&'a str, &'a str), ParseError> {
    let first = line
        .find('|')
        .ok_or_else(|| malformed(line_number, line))?;
    let second = line
        .get(first + 1..)
        .and_then(|rest| rest.find('|'))
        .map(|offset| first + 1 + offset)
        .ok_or_else(|| malformed(line_number, line))?;
    // The sura field must not be empty, and the aya field needs at least one byte.
    if first < 1 || second < first + 2 {
        return Err(malformed(line_number, line));
    }
    Ok((&line[..second], &line[second + 1..]))
}

/// Reads the numeric `sura|aya` head of a record.
fn parse_numbers(line_number: usize, record: &str, line: &str) -> Result<(u16, u16), ParseError> {
    let (sura_field, aya_field) = record
        .split_once('|')
        .ok_or_else(|| malformed(line_number, line))?;
    let sura: u32 = sura_field.parse().map_err(|_| ParseError::NotANumber {
        line: line_number,
        field: "sura",
        content: excerpt(sura_field),
    })?;
    let aya: u32 = aya_field.parse().map_err(|_| ParseError::NotANumber {
        line: line_number,
        field: "aya",
        content: excerpt(aya_field),
    })?;
    if sura < 1 || sura > u32::from(SURA_COUNT) || aya < 1 || aya > u32::from(u16::MAX) {
        return Err(ParseError::OutOfRange {
            line: line_number,
            sura,
            aya,
        });
    }
    Ok((sura as u16, aya as u16))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_SURA: &str = "1|1|first\n1|2|second\n# a comment block\n";

    #[test]
    fn parses_records_and_skips_comments() {
        let text = format!("{ONE_SURA}{}", (2..=SURA_COUNT).map(|sura| format!("{sura}|1|text\n")).collect::<String>());
        let quran = Quran::parse(&text).expect("this is a complete mushaf");
        assert_eq!(quran.aya_count(1), Some(2));
        assert_eq!(quran.verse(1, 1), Some("first"));
        assert_eq!(quran.verse(1, 2), Some("second"));
        assert_eq!(quran.total_ayat(), 115);
    }

    #[test]
    fn rejects_a_line_that_is_not_a_record() {
        let text = "1|1|ok\nnot a record\n";
        assert!(matches!(
            Quran::parse(text),
            Err(ParseError::MalformedLine { line: 2, .. })
        ));
    }

    #[test]
    fn rejects_non_numeric_and_empty_fields() {
        assert!(matches!(
            Quran::parse("x|1|text\n"),
            Err(ParseError::NotANumber { line: 1, field: "sura", .. })
        ));
        assert!(matches!(
            Quran::parse("1|x|text\n"),
            Err(ParseError::NotANumber { line: 1, field: "aya", .. })
        ));
        assert!(matches!(
            Quran::parse("1||text\n"),
            Err(ParseError::MalformedLine { line: 1, .. })
        ));
    }

    #[test]
    fn rejects_an_empty_ayah_and_a_repeated_one() {
        assert!(matches!(
            Quran::parse("1|1|\n"),
            Err(ParseError::EmptyAyah { sura: 1, aya: 1 })
        ));
        assert!(matches!(
            Quran::parse("1|1|text\n1|1|text\n"),
            Err(ParseError::DuplicateAyah { sura: 1, aya: 1 })
        ));
    }

    #[test]
    fn rejects_out_of_order_ayat_and_suras() {
        assert!(matches!(
            Quran::parse("1|1|text\n1|3|text\n"),
            Err(ParseError::OutOfOrder { sura: 1, aya: 3, expected: 2 })
        ));
        assert!(matches!(
            Quran::parse("1|1|text\n3|1|text\n"),
            Err(ParseError::SuraOutOfOrder { sura: 3, expected: 2 })
        ));
    }

    #[test]
    fn rejects_an_incomplete_mushaf() {
        assert!(matches!(
            Quran::parse("1|1|text\n"),
            Err(ParseError::Incomplete { suras_found: 1 })
        ));
    }

    #[test]
    fn rejects_references_outside_the_mushaf() {
        let text = format!("1|1|first\n{}", (2..=SURA_COUNT).map(|sura| format!("{sura}|1|text\n")).collect::<String>());
        let quran = Quran::parse(&text).expect("this is a complete mushaf");
        assert_eq!(quran.aya_count(0), None);
        assert_eq!(quran.aya_count(SURA_COUNT + 1), None);
        assert_eq!(quran.verse(1, 0), None);
        assert_eq!(quran.verse(1, 3), None);
        assert_eq!(quran.verse(SURA_COUNT + 1, 1), None);
    }
}
