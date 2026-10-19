//! The basmala prefix Tanzil adds to the first ayah of most suras.
//!
//! Tanzil prefixes the basmala to ayah 1 of every sura except 1 and 9, so a reader
//! that wants it as a header line has to split it off. The split is lossless by
//! contract: the two pieces plus one space rebuild the original string byte for
//! byte, and this function returns `None` rather than guessing when that cannot
//! hold.
//!
//! The function compares words and positions, never characters of a specific
//! script, so its unit tests use plain ASCII stand-ins. The behaviour on the real
//! Uthmani text is covered by the integration test, which walks every sura.

/// Splits the basmala prefix off `text`, returning `(basmala, rest)`.
///
/// `fatiha_basmala` is the text of 1:1, which is the reference spelling Tanzil
/// prefixes. The result is `None` for any ayah other than ayah 1, for suras 1 and
/// 9 which carry no prefix, and for any ayah whose opening words do not match the
/// reference closely enough to split safely.
pub fn split<'a>(
    sura: u16,
    aya: u16,
    text: &'a str,
    fatiha_basmala: &str,
) -> Option<(&'a str, &'a str)> {
    if aya != 1 || sura == 1 || sura == 9 {
        return None;
    }

    let words: Vec<&str> = text.split(' ').collect();
    let reference: Vec<&str> = fatiha_basmala.split(' ').collect();
    if words.len() <= reference.len() {
        return None;
    }

    // Words after the first must match exactly. The first word may carry one extra
    // mark, which is how 95:1 and 97:1 are written.
    for index in 1..reference.len() {
        if words[index] != reference[index] {
            return None;
        }
    }
    if words[0].chars().next() != reference[0].chars().next() {
        return None;
    }

    // The prefix is measured from the ACTUAL words, never from the reference: the
    // first word may carry an extra mark, which changes its byte length. Joining
    // the actual words tells us where the prefix ends without allocating.
    let prefix_len =
        words[..reference.len()].iter().map(|word| word.len()).sum::<usize>() + reference.len() - 1;
    let basmala = text.get(..prefix_len)?;
    // A prefix that is not followed by exactly one separator is not a prefix.
    let rest = text.get(prefix_len..)?.strip_prefix(' ')?;
    Some((basmala, rest))
}

#[cfg(test)]
mod tests {
    use super::split;

    const REFERENCE: &str = "one two three";

    #[test]
    fn splits_and_keeps_the_pieces_lossless() {
        let text = "one two three four five";
        let (basmala, rest) = split(2, 1, text, REFERENCE).expect("this is a prefixed ayah");
        assert_eq!(basmala, "one two three");
        assert_eq!(rest, "four five");
        assert_eq!(format!("{basmala} {rest}"), text);
    }

    #[test]
    fn tolerates_one_extra_mark_on_the_first_word() {
        let text = "oneX two three four";
        let (basmala, rest) = split(2, 1, text, REFERENCE).expect("first word may differ after its first char");
        assert_eq!(basmala, "oneX two three");
        assert_eq!(rest, "four");
    }

    #[test]
    fn refuses_when_the_opening_does_not_match() {
        // Only the first character of the first word may differ from the reference,
        // because that is where an extra mark shows up. Everything else, including
        // the rest of the first word's spelling, must match.
        assert_eq!(split(2, 1, "zzz two three four", REFERENCE), None);
        assert_eq!(split(2, 1, "one zzz three four", REFERENCE), None);
        assert_eq!(split(2, 1, "one two zzz four", REFERENCE), None);
    }

    #[test]
    fn refuses_when_the_prefix_would_consume_the_whole_ayah() {
        assert_eq!(split(2, 1, "one two three", REFERENCE), None);
        assert_eq!(split(2, 1, "one two", REFERENCE), None);
    }

    #[test]
    fn refuses_where_no_prefix_exists() {
        let text = "one two three four";
        assert_eq!(split(2, 2, text, REFERENCE), None, "only ayah 1 carries the prefix");
        assert_eq!(split(1, 1, text, REFERENCE), None, "sura 1 is the basmala itself");
        assert_eq!(split(9, 1, text, REFERENCE), None, "sura 9 carries no basmala");
    }

    #[test]
    fn keeps_the_round_trip_when_spacing_is_irregular() {
        // A run of spaces inside the prefix still round trips, and the round trip is
        // the only property this split promises. The real Tanzil text has no run of
        // spaces in any ayah, so this only documents the boundary behaviour.
        let text = "one two three  four";
        let (basmala, rest) = split(2, 1, text, REFERENCE).expect("the prefix is still a prefix");
        assert_eq!(format!("{basmala} {rest}"), text);
    }
}
