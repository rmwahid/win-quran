//! Presentation helpers for the ayah-end marker.

/// Arabic-Indic digits, U+0660 to U+0669.
const ARABIC_INDIC: [char; 10] = ['٠', '١', '٢', '٣', '٤', '٥', '٦', '٧', '٨', '٩'];

/// The Arabic-Indic spelling of a number, used to number the ayah in the end
/// marker.
///
/// Presentation only: these digits are generated for the marker and never enter
/// the Tanzil text. In the shape they are drawn in, the marker codepoint U+06DD
/// and the digits after it form one cluster, which is why they travel together.
pub fn digits(value: u32) -> String {
    value
        .to_string()
        .chars()
        .map(|digit| ARABIC_INDIC[digit as usize - '0' as usize])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::digits;

    #[test]
    fn maps_every_decimal_digit() {
        assert_eq!(digits(0), "٠");
        assert_eq!(digits(19), "١٩");
        assert_eq!(digits(286), "٢٨٦");
    }

    #[test]
    fn keeps_the_digit_count() {
        for value in [1u32, 9, 10, 99, 100, 604, 6236] {
            assert_eq!(digits(value).chars().count(), value.to_string().len());
        }
    }
}
