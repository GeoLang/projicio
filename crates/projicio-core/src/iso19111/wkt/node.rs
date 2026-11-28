// adapted from PROJ src/iso19111/io.cpp (MIT), see LICENSES/PROJ-MIT.txt

use crate::Error;

// PROJ's WKTNode::createFrom stops at 16 nesting levels
const MAX_NESTING_LEVEL: usize = 16;
// the longest WKT projinfo writes for an EPSG CRS is under 8 kB
const MAX_WKT_LENGTH: usize = 1 << 20;

const START_PRINTED_QUOTE: &str = "\u{201C}";
const END_PRINTED_QUOTE: &str = "\u{201D}";

#[derive(Debug, Clone, PartialEq)]
pub(super) struct WktNode {
    pub(super) value: String,
    pub(super) children: Vec<WktNode>,
}

pub(super) fn parse_error(message: impl Into<String>) -> Error {
    Error::InvalidWkt(message.into())
}

fn is_space(byte: u8) -> bool {
    // C isspace in the C locale
    matches!(byte, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

fn skip_space(text: &[u8], start: usize) -> usize {
    let mut index = start;
    while index < text.len() && is_space(text[index]) {
        index += 1;
    }
    index
}

fn ends_token(byte: u8) -> bool {
    matches!(byte, b'[' | b'(' | b',' | b']' | b')') || is_space(byte)
}

impl WktNode {
    pub(super) fn parse(text: &str) -> Result<Self, Error> {
        Ok(Self::parse_prefix(text)?.0)
    }

    pub(super) fn parse_prefix(text: &str) -> Result<(Self, usize), Error> {
        if text.len() > MAX_WKT_LENGTH {
            return Err(parse_error(format!(
                "WKT longer than {MAX_WKT_LENGTH} bytes"
            )));
        }
        Self::create_from(text, 0, 0)
    }

    fn create_from(
        text: &str,
        index_start: usize,
        nesting_level: usize,
    ) -> Result<(Self, usize), Error> {
        if nesting_level == MAX_NESTING_LEVEL {
            return Err(parse_error("too many nesting levels"));
        }
        let bytes = text.as_bytes();
        let mut index = skip_space(bytes, index_start);
        if index == bytes.len() {
            return Err(parse_error("whitespace only string"));
        }
        let mut value: Vec<u8> = Vec::new();
        let mut closing_marker: Option<&str> = None;
        while index < bytes.len() && (closing_marker.is_some() || !ends_token(bytes[index])) {
            let rest = &bytes[index..];
            if bytes[index] == b'"' {
                match closing_marker {
                    None => closing_marker = Some("\""),
                    Some("\"") => {
                        // a doubled quote inside a string is kept once
                        if bytes.get(index + 1) == Some(&b'"') {
                            index += 1;
                        } else {
                            closing_marker = None;
                        }
                    }
                    Some(_) => {}
                }
            } else if rest.starts_with(START_PRINTED_QUOTE.as_bytes()) && closing_marker.is_none() {
                closing_marker = Some(END_PRINTED_QUOTE);
                value.push(b'"');
                index += START_PRINTED_QUOTE.len();
                continue;
            } else if closing_marker == Some(END_PRINTED_QUOTE)
                && rest.starts_with(END_PRINTED_QUOTE.as_bytes())
            {
                closing_marker = None;
                value.push(b'"');
                index += END_PRINTED_QUOTE.len();
                continue;
            }
            value.push(bytes[index]);
            index += 1;
        }
        // the token ends on ASCII or at the end of the text, so the value is whole UTF-8
        let value = String::from_utf8(value).map_err(|e| parse_error(e.to_string()))?;
        index = skip_space(bytes, index);
        if index == bytes.len() {
            return Err(parse_error(if index_start == 0 {
                "missing ["
            } else {
                "missing , or ]"
            }));
        }
        let mut node = Self {
            value,
            children: Vec::new(),
        };
        if index_start > 0 {
            if bytes[index] == b',' {
                return Ok((node, index + 1));
            }
            if bytes[index] == b']' || bytes[index] == b')' {
                return Ok((node, index));
            }
        }
        if bytes[index] != b'[' && bytes[index] != b'(' {
            return Err(parse_error("missing ["));
        }
        index = skip_space(bytes, index + 1);
        while index < bytes.len() && bytes[index] != b']' && bytes[index] != b')' {
            let (child, index_end) = Self::create_from(text, index, nesting_level + 1)?;
            node.children.push(child);
            index = skip_space(bytes, index_end);
            if index < bytes.len() && bytes[index] == b',' {
                index = skip_space(bytes, index + 1);
            }
        }
        if index == bytes.len() || (bytes[index] != b']' && bytes[index] != b')') {
            return Err(parse_error("missing ]"));
        }
        Ok((node, index + 1))
    }

    pub(super) fn look_for_child(&self, names: &[&str]) -> Option<&WktNode> {
        self.children.iter().find(|child| {
            names
                .iter()
                .any(|name| child.value.eq_ignore_ascii_case(name))
        })
    }

    pub(super) fn look_for_child_occurrence(
        &self,
        name: &str,
        occurrence: usize,
    ) -> Option<&WktNode> {
        self.children
            .iter()
            .filter(|child| child.value.eq_ignore_ascii_case(name))
            .nth(occurrence)
    }

    pub(super) fn count_children_of_name(&self, name: &str) -> usize {
        self.children
            .iter()
            .filter(|child| child.value.eq_ignore_ascii_case(name))
            .count()
    }
}

pub(super) fn strip_quotes(text: &str) -> &str {
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        return &text[1..text.len() - 1];
    }
    text
}

pub(super) fn as_double(text: &str) -> Result<f64, Error> {
    // PROJ c_locale_stod: a fast path for short strings, then istringstream
    let bytes = text.as_bytes();
    if !bytes.is_empty() && bytes.len() < 15 {
        let mut accumulator: i64 = 0;
        let mut divisor: i64 = 1;
        let mut after_dot = false;
        let mut index = 0;
        if bytes[0] == b'-' {
            index = 1;
            divisor = -1;
        } else if bytes[0] == b'+' {
            index = 1;
        }
        for &byte in &bytes[index..] {
            if byte.is_ascii_digit() {
                accumulator = accumulator * 10 + i64::from(byte - b'0');
                if after_dot {
                    divisor *= 10;
                }
            } else if byte == b'.' {
                after_dot = true;
            } else {
                divisor = 0;
            }
        }
        if divisor != 0 {
            return Ok(accumulator as f64 / divisor as f64);
        }
    }
    // istringstream reads no inf, nan or hexadecimal, which Rust's parser would
    let is_decimal = bytes
        .iter()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-'));
    match text.parse::<f64>() {
        Ok(value) if is_decimal => Ok(value),
        _ => Err(parse_error(format!("non double value: {text}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_nodes_keep_their_quotes_and_case() {
        let node =
            WktNode::parse("GEOGCS[\"WGS 84\",UNIT(\"degree\", 0.0174532925199433)]").unwrap();
        assert_eq!(node.value, "GEOGCS");
        assert_eq!(node.children[0].value, "\"WGS 84\"");
        assert_eq!(node.children[1].value, "UNIT");
        assert_eq!(node.children[1].children[1].value, "0.0174532925199433");
    }

    #[test]
    fn doubled_quotes_and_printed_quotes_are_read_into_one_string() {
        let node = WktNode::parse("A[\"say \"\"hi\"\"\",\u{201C}x\u{201D}]").unwrap();
        assert_eq!(node.children[0].value, "\"say \"hi\"\"");
        assert_eq!(node.children[1].value, "\"x\"");
    }

    #[test]
    fn deep_nesting_and_truncation_raise() {
        let deep = format!("{}{}", "A[".repeat(17), "]".repeat(17));
        assert!(WktNode::parse(&deep).is_err());
        assert!(WktNode::parse("GEOGCS[\"x\",DATUM[\"y\"").is_err());
        assert!(WktNode::parse("   ").is_err());
    }

    #[test]
    fn numbers_parse_like_proj_and_refuse_words() {
        assert_eq!(as_double("298.257223563").unwrap(), 298.257223563);
        assert_eq!(as_double("-17.6666666666667").unwrap(), -17.6666666666667);
        assert_eq!(as_double("1E-06").unwrap(), 1e-6);
        assert!(as_double("inf").is_err());
        assert!(as_double("\"0\"").is_err());
    }
}
