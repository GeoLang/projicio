// adapted from PROJ src/iso19111/metadata.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::common::UnitOfMeasure;
use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct Identifier {
    pub code_space: String,
    pub code: String,
    pub version: Option<String>,
    pub authority_citation: Option<String>,
    pub uri: Option<String>,
}

impl Identifier {
    pub fn new(code_space: &str, code: &str) -> Self {
        Self {
            code_space: code_space.to_string(),
            code: code.to_string(),
            version: None,
            authority_citation: None,
            uri: None,
        }
    }
}

pub(crate) const EPSG: &str = "EPSG";

fn next_toward_negative_infinity(value: f64) -> f64 {
    // f64::next_down needs Rust 1.86, above the workspace rust-version
    if value == 0.0 {
        return -f64::from_bits(1);
    }
    let bits = value.to_bits();
    f64::from_bits(if value > 0.0 { bits - 1 } else { bits + 1 })
}

fn next_toward_positive_infinity(value: f64) -> f64 {
    // f64::next_up needs Rust 1.86, above the workspace rust-version
    if value == 0.0 {
        return f64::from_bits(1);
    }
    let bits = value.to_bits();
    f64::from_bits(if value > 0.0 { bits + 1 } else { bits - 1 })
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeographicBoundingBox {
    pub west_longitude: f64,
    pub south_latitude: f64,
    pub east_longitude: f64,
    pub north_latitude: f64,
}

impl GeographicBoundingBox {
    pub fn new(
        mut west: f64,
        mut south: f64,
        mut east: f64,
        mut north: f64,
    ) -> Result<Self, Error> {
        if west.is_nan() || south.is_nan() || east.is_nan() || north.is_nan() {
            return Err(Error::InvalidProjJson(
                "GeographicBoundingBox::create() does not accept NaN values".into(),
            ));
        }
        if south > north {
            return Err(Error::InvalidProjJson(
                "GeographicBoundingBox::create() does not accept south > north".into(),
            ));
        }
        // PROJ widens a box reduced to a point or a line by one ulp on each side
        if west == east {
            if west > -180.0 {
                west = next_toward_negative_infinity(west);
            }
            if east < 180.0 {
                east = next_toward_positive_infinity(east);
            }
        }
        if south == north {
            if south > -90.0 {
                south = next_toward_negative_infinity(south);
            }
            if north < 90.0 {
                north = next_toward_positive_infinity(north);
            }
        }
        Ok(Self {
            west_longitude: west,
            south_latitude: south,
            east_longitude: east,
            north_latitude: north,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerticalExtent {
    pub minimum: f64,
    pub maximum: f64,
    pub unit: UnitOfMeasure,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TemporalExtent {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Extent {
    pub description: Option<String>,
    pub geographic_bounding_box: Option<GeographicBoundingBox>,
    pub vertical_extent: Option<VerticalExtent>,
    pub temporal_extent: Option<TemporalExtent>,
}

fn is_ignored_char(ch: u8) -> bool {
    matches!(
        ch,
        b' ' | b'_' | b'-' | b'/' | b'(' | b')' | b'.' | b'&' | b','
    )
}

const UTF8_TO_LOWER_ASCII: [(&[u8], u8); 10] = [
    (b"\xc3\xa1", b'a'),
    (b"\xc3\xa4", b'a'),
    (b"\xc4\x9b", b'e'),
    (b"\xc3\xa8", b'e'),
    (b"\xc3\xa9", b'e'),
    (b"\xc3\xab", b'e'),
    (b"\xc3\xad", b'i'),
    (b"\xc3\xb4", b'o'),
    (b"\xc3\xb6", b'o'),
    (b"\xc3\xa7", b'c'),
];

fn ascii_replacement(rest: &[u8]) -> Option<(&'static [u8], u8)> {
    UTF8_TO_LOWER_ASCII
        .iter()
        .find(|(utf8, _)| rest.starts_with(utf8))
        .copied()
}

fn byte_at(text: &[u8], index: usize) -> u8 {
    // PROJ reads names as C strings, which end in a zero byte
    text.get(index).copied().unwrap_or(0)
}

fn starts_with_ignoring_case(text: &[u8], needle: &[u8]) -> bool {
    text.len() >= needle.len() && text[..needle.len()].eq_ignore_ascii_case(needle)
}

fn is_whole_suffix_ignoring_case(text: &[u8], index: usize, suffix: &[u8]) -> bool {
    index <= text.len()
        && text.len() - index == suffix.len()
        && starts_with_ignoring_case(&text[index..], suffix)
}

fn skip_word_after_ignored(ch: u8, text: &[u8], index: &mut usize, word: &[u8]) -> bool {
    if ch == word[0]
        && *index > 0
        && is_ignored_char(text[*index - 1])
        && starts_with_ignoring_case(&text[*index..], word)
    {
        *index += word.len();
        return true;
    }
    false
}

fn shorten_word_to_first_char(ch: u8, text: &[u8], index: &mut usize, word: &[u8]) -> bool {
    if ch == word[0]
        && *index > 0
        && (is_ignored_char(text[*index - 1]) || text[*index - 1].is_ascii_digit())
        && starts_with_ignoring_case(&text[*index..], word)
    {
        *index += word.len() - 1;
        return true;
    }
    false
}

pub fn is_equivalent_name(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut i = 0usize;
    let mut j = 0usize;
    let mut last_valid_a = 0u8;
    let mut last_valid_b = 0u8;
    while byte_at(a, i) != 0 || byte_at(b, j) != 0 {
        let mut a_ch = byte_at(a, i).to_ascii_lowercase();
        let mut b_ch = byte_at(b, j).to_ascii_lowercase();
        if a_ch == b' '
            && byte_at(a, i + 1) == b'+'
            && byte_at(a, i + 2) == b' '
            && byte_at(a, i + 3) != 0
        {
            i += 3;
            continue;
        }
        if b_ch == b' '
            && byte_at(b, j + 1) == b'+'
            && byte_at(b, j + 2) == b' '
            && byte_at(b, j + 3) != 0
        {
            j += 3;
            continue;
        }

        let a_is_intl_feet = is_whole_suffix_ignoring_case(a, i, b"_IntlFeet");
        let b_is_intl_feet = is_whole_suffix_ignoring_case(b, j, b"_IntlFeet");
        let a_is_feet = is_whole_suffix_ignoring_case(a, i, b"_Feet");
        let b_is_feet = is_whole_suffix_ignoring_case(b, j, b"_Feet");
        if (a_is_intl_feet && b_is_feet) || (a_is_feet && b_is_intl_feet) {
            return true;
        }

        if is_ignored_char(a_ch) {
            i += 1;
            continue;
        }
        if is_ignored_char(b_ch) {
            j += 1;
            continue;
        }

        if a_ch == b'1'
            && !last_valid_a.is_ascii_digit()
            && byte_at(a, i + 1) == b'9'
            && byte_at(a, i + 2).is_ascii_digit()
            && byte_at(a, i + 3).is_ascii_digit()
        {
            i += 2;
            last_valid_a = b'9';
            continue;
        }
        if b_ch == b'1'
            && !last_valid_b.is_ascii_digit()
            && byte_at(b, j + 1) == b'9'
            && byte_at(b, j + 2).is_ascii_digit()
            && byte_at(b, j + 3).is_ascii_digit()
        {
            j += 2;
            last_valid_b = b'9';
            continue;
        }

        let skipped_zone_a = skip_word_after_ignored(a_ch, a, &mut i, b"zone");
        let skipped_zone_b = skip_word_after_ignored(b_ch, b, &mut j, b"zone");
        if skipped_zone_a || skipped_zone_b {
            continue;
        }
        let skipped_height_a = skip_word_after_ignored(a_ch, a, &mut i, b"height");
        let skipped_height_b = skip_word_after_ignored(b_ch, b, &mut j, b"height");
        if skipped_height_a || skipped_height_b {
            continue;
        }

        if !shorten_word_to_first_char(a_ch, a, &mut i, b"north") {
            shorten_word_to_first_char(a_ch, a, &mut i, b"south");
        }
        if !shorten_word_to_first_char(b_ch, b, &mut j, b"north") {
            shorten_word_to_first_char(b_ch, b, &mut j, b"south");
        }

        if a_ch > 127 {
            if let Some((utf8, ascii)) = ascii_replacement(&a[i..]) {
                a_ch = ascii;
                i += utf8.len() - 1;
            }
        }
        if b_ch > 127 {
            if let Some((utf8, ascii)) = ascii_replacement(&b[j..]) {
                b_ch = ascii;
                j += utf8.len() - 1;
            }
        }

        if a_ch != b_ch {
            return false;
        }
        last_valid_a = a_ch;
        last_valid_b = b_ch;
        if a_ch != 0 {
            i += 1;
        }
        if b_ch != 0 {
            j += 1;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_differing_in_case_punctuation_and_century_are_equivalent() {
        assert!(is_equivalent_name("WGS_1984", "wgs 84"));
        assert!(is_equivalent_name("UTM zone 32N", "UTM 32 North"));
        assert!(!is_equivalent_name(
            "North American Datum 1983",
            "World Geodetic System 1984"
        ));
    }
}
