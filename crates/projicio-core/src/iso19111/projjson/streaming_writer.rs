// adapted from PROJ src/proj_json_streaming_writer.cpp (MIT), see LICENSES/PROJ-MIT.txt

struct State {
    is_object: bool,
    first_child: bool,
}

pub(super) struct StreamingWriter {
    text: String,
    indent: &'static str,
    accumulated_indent: String,
    states: Vec<State>,
    waiting_for_value: bool,
}

impl StreamingWriter {
    pub(super) fn new() -> Self {
        Self {
            text: String::new(),
            indent: "  ",
            accumulated_indent: String::new(),
            states: Vec::new(),
            waiting_for_value: false,
        }
    }

    pub(super) fn into_string(self) -> String {
        self.text
    }

    fn increase_indent(&mut self) {
        self.accumulated_indent.push_str(self.indent);
    }

    fn decrease_indent(&mut self) {
        let length = self.accumulated_indent.len() - self.indent.len();
        self.accumulated_indent.truncate(length);
    }

    fn emit_comma_if_needed(&mut self) {
        if self.waiting_for_value {
            self.waiting_for_value = false;
            return;
        }
        let Some(state) = self.states.last_mut() else {
            return;
        };
        if !state.first_child {
            self.text.push(',');
        }
        state.first_child = false;
        self.text.push('\n');
        self.text.push_str(&self.accumulated_indent);
    }

    pub(super) fn start_object(&mut self) {
        self.emit_comma_if_needed();
        self.text.push('{');
        self.increase_indent();
        self.states.push(State {
            is_object: true,
            first_child: true,
        });
    }

    pub(super) fn end_object(&mut self) {
        self.end_container(true, '}');
    }

    pub(super) fn start_array(&mut self) {
        self.emit_comma_if_needed();
        self.text.push('[');
        self.increase_indent();
        self.states.push(State {
            is_object: false,
            first_child: true,
        });
    }

    pub(super) fn end_array(&mut self) {
        self.end_container(false, ']');
    }

    fn end_container(&mut self, is_object: bool, closing: char) {
        self.decrease_indent();
        let state = self
            .states
            .pop()
            .expect("end of a JSON container that was never started");
        debug_assert_eq!(state.is_object, is_object);
        if !state.first_child {
            self.text.push('\n');
            self.text.push_str(&self.accumulated_indent);
        }
        self.text.push(closing);
    }

    pub(super) fn add_key(&mut self, key: &str) {
        self.emit_comma_if_needed();
        push_quoted(&mut self.text, key);
        self.text.push_str(": ");
        self.waiting_for_value = true;
    }

    pub(super) fn add_string(&mut self, value: &str) {
        self.emit_comma_if_needed();
        push_quoted(&mut self.text, value);
    }

    pub(super) fn add_unquoted(&mut self, value: &str) {
        self.emit_comma_if_needed();
        self.text.push_str(value);
    }

    pub(super) fn add_integer(&mut self, value: i64) {
        self.emit_comma_if_needed();
        self.text.push_str(&value.to_string());
    }

    pub(super) fn add_double(&mut self, value: f64, precision: usize) {
        self.emit_comma_if_needed();
        let text = if value.is_nan() {
            "\"NaN\"".to_string()
        } else if value.is_infinite() {
            (if value > 0.0 {
                "\"Infinity\""
            } else {
                "\"-Infinity\""
            })
            .to_string()
        } else if (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&value)
            && f64::from(value as i32) == value
        {
            (value as i32).to_string()
        } else {
            format_general(value, precision)
        };
        self.text.push_str(&text);
    }
}

fn push_quoted(text: &mut String, value: &str) {
    text.push('"');
    for ch in value.chars() {
        match ch {
            '"' => text.push_str("\\\""),
            '\\' => text.push_str("\\\\"),
            '\u{8}' => text.push_str("\\b"),
            '\u{c}' => text.push_str("\\f"),
            '\n' => text.push_str("\\n"),
            '\r' => text.push_str("\\r"),
            '\t' => text.push_str("\\t"),
            ch if (ch as u32) < 0x20 => text.push_str(&format!("\\u{:04X}", ch as u32)),
            ch => text.push(ch),
        }
    }
    text.push('"');
}

fn strip_trailing_zeros(number: &str) -> &str {
    if !number.contains('.') {
        return number;
    }
    number.trim_end_matches('0').trim_end_matches('.')
}

pub(super) fn format_general(value: f64, precision: usize) -> String {
    // sqlite3_vsnprintf, which PROJ formats numbers with, stops at 16 significant digits
    let precision = precision.clamp(1, 16);
    let mantissa_decimals = precision - 1;
    let scientific = format!("{value:.mantissa_decimals$e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("LowerExp output has an exponent");
    let exponent: i32 = exponent.parse().expect("LowerExp exponent is an integer");
    if exponent < -4 || exponent >= precision as i32 {
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!(
            "{}e{sign}{:02}",
            strip_trailing_zeros(mantissa),
            exponent.abs()
        );
    }
    let decimals = (precision as i32 - 1 - exponent) as usize;
    strip_trailing_zeros(&format!("{value:.decimals$}")).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_format_matches_printf() {
        assert_eq!(format_general(0.0157079632679489, 15), "0.0157079632679489");
        assert_eq!(format_general(298.257223563, 15), "298.257223563");
        assert_eq!(format_general(-17.666666666666668, 15), "-17.6666666666667");
        assert_eq!(format_general(1e-6, 15), "1e-06");
        assert_eq!(format_general(1.5e20, 15), "1.5e+20");
        assert_eq!(format_general(123456.5, 15), "123456.5");
    }
}
