// adapted from PROJ src/iso19111/io.cpp WKTFormatter (MIT), see LICENSES/PROJ-MIT.txt

use super::{WktDialect, WktLayout};
use crate::iso19111::common::UnitOfMeasure;
use crate::iso19111::projjson::format_general;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Version {
    Wkt1,
    Wkt2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OutputAxisRule {
    Yes,
    No,
    Wkt1GdalEpsgStyle,
}

pub(super) struct Formatter {
    pub(super) version: Version,
    pub(super) use_2019_keywords: bool,
    pub(super) use_esri_dialect: bool,
    multi_line: bool,
    pub(super) output_axis_order: bool,
    pub(super) force_unit_keyword: bool,
    pub(super) prime_meridian_in_degree: bool,
    pub(super) output_axis: OutputAxisRule,
    indent_level: usize,
    level: usize,
    stack_has_child: Vec<bool>,
    stack_has_id: Vec<bool>,
    stack_empty_keyword: Vec<bool>,
    output_unit_stack: Vec<bool>,
    output_id_stack: Vec<bool>,
    axis_linear_unit_stack: Vec<UnitOfMeasure>,
    axis_angular_unit_stack: Vec<UnitOfMeasure>,
    pub(super) abridged_transformation: bool,
    pub(super) towgs84_parameters: Option<[f64; 7]>,
    pub(super) hdatum_extension: Option<String>,
    result: String,
}

const INDENT_WIDTH: usize = 4;

impl Formatter {
    pub(super) fn new(dialect: WktDialect, layout: WktLayout) -> Self {
        let wkt2 = matches!(dialect, WktDialect::Wkt2_2019 | WktDialect::Wkt2_2015);
        Self {
            version: if wkt2 { Version::Wkt2 } else { Version::Wkt1 },
            use_2019_keywords: dialect == WktDialect::Wkt2_2019,
            use_esri_dialect: dialect == WktDialect::Wkt1Esri,
            multi_line: layout == WktLayout::MultiLine,
            output_axis_order: wkt2,
            force_unit_keyword: !wkt2,
            prime_meridian_in_degree: !wkt2,
            output_axis: match dialect {
                WktDialect::Wkt2_2019 | WktDialect::Wkt2_2015 => OutputAxisRule::Yes,
                WktDialect::Wkt1Gdal => OutputAxisRule::Wkt1GdalEpsgStyle,
                WktDialect::Wkt1Esri => OutputAxisRule::No,
            },
            indent_level: 0,
            level: 0,
            stack_has_child: Vec::new(),
            stack_has_id: vec![false],
            stack_empty_keyword: Vec::new(),
            output_unit_stack: vec![true],
            output_id_stack: vec![true],
            axis_linear_unit_stack: vec![UnitOfMeasure::metre()],
            axis_angular_unit_stack: vec![UnitOfMeasure::degree()],
            abridged_transformation: false,
            towgs84_parameters: None,
            hdatum_extension: None,
            result: String::new(),
        }
    }

    pub(super) fn is_wkt2(&self) -> bool {
        self.version == Version::Wkt2
    }

    pub(super) fn into_string(self) -> String {
        debug_assert!(self.indent_level == 0 && self.level == 0);
        self.result
    }

    fn add_new_line(&mut self) {
        self.result.push('\n');
    }

    fn add_indentation(&mut self) {
        self.result
            .push_str(&" ".repeat(self.indent_level * INDENT_WIDTH));
    }

    pub(super) fn enter(&mut self) {
        if self.indent_level == 0 && self.level == 0 {
            self.stack_has_child.push(false);
        }
        self.level += 1;
    }

    pub(super) fn leave(&mut self) {
        self.level -= 1;
        if self.indent_level == 0 && self.level == 0 {
            self.stack_has_child.pop();
        }
    }

    pub(super) fn start_node(&mut self, keyword: &str, has_id: bool) {
        if !self.stack_has_child.is_empty() {
            self.start_new_child();
        } else if !self.result.is_empty() {
            self.result.push(',');
            if self.multi_line && !keyword.is_empty() {
                self.add_new_line();
            }
        }
        if self.multi_line && (self.indent_level > 0 || self.level > 0) && !keyword.is_empty() {
            if !self.result.is_empty() {
                self.add_new_line();
            }
            self.add_indentation();
        }
        if !keyword.is_empty() {
            self.result.push_str(keyword);
            self.result.push('[');
        }
        self.indent_level += 1;
        self.stack_has_child.push(false);
        self.stack_empty_keyword.push(keyword.is_empty());

        // PROJ writes ID under a node that has one only for WKT2 METHOD and PARAMETER
        let parent_has_id = *self.stack_has_id.last().expect("id stack starts non-empty");
        let output_id = if self.indent_level >= 2
            && self.version == Version::Wkt2
            && (keyword == "METHOD" || keyword == "PARAMETER")
        {
            self.output_id_stack[0]
        } else if self.indent_level >= 2 && self.version == Version::Wkt2 {
            self.output_id_stack[0] && !parent_has_id
        } else {
            self.output_id()
        };
        self.output_id_stack.push(output_id);
        self.stack_has_id.push(has_id || parent_has_id);
    }

    pub(super) fn end_node(&mut self) {
        self.stack_has_id.pop();
        self.output_id_stack.pop();
        self.indent_level -= 1;
        let empty_keyword = self
            .stack_empty_keyword
            .pop()
            .expect("end of a WKT node that was never started");
        self.stack_has_child.pop();
        if !empty_keyword {
            self.result.push(']');
        }
    }

    fn start_new_child(&mut self) {
        let has_child = self
            .stack_has_child
            .last_mut()
            .expect("a child is added inside a node");
        if *has_child {
            self.result.push(',');
        }
        *has_child = true;
    }

    pub(super) fn add_quoted_string(&mut self, text: &str) {
        self.start_new_child();
        self.result.push('"');
        self.result.push_str(&text.replace('"', "\"\""));
        self.result.push('"');
    }

    pub(super) fn add_raw(&mut self, text: &str) {
        self.start_new_child();
        self.result.push_str(text);
    }

    pub(super) fn add_integer(&mut self, number: i64) {
        self.start_new_child();
        self.result.push_str(&number.to_string());
    }

    pub(super) fn add_double(&mut self, number: f64) {
        self.add_double_with_precision(number, 15);
    }

    pub(super) fn add_double_with_precision(&mut self, number: f64, precision: usize) {
        self.start_new_child();
        if number == 0.0 {
            self.result
                .push_str(if self.use_esri_dialect { "0.0" } else { "0" });
            return;
        }
        let mut text = format_general(number, precision);
        // PROJ internal::toString retries with 14 digits when 15 show a run of nines
        if precision == 15 && text.contains("9999999999") {
            text = format_general(number, 14);
        }
        self.result.push_str(&text.replace('e', "E"));
        if self.use_esri_dialect && !text.contains('.') {
            self.result.push_str(".0");
        }
    }

    pub(super) fn push_output_unit(&mut self, output_unit: bool) {
        self.output_unit_stack.push(output_unit);
    }

    pub(super) fn pop_output_unit(&mut self) {
        self.output_unit_stack.pop();
    }

    pub(super) fn output_unit(&self) -> bool {
        *self
            .output_unit_stack
            .last()
            .expect("output unit stack starts non-empty")
    }

    pub(super) fn push_output_id(&mut self, output_id: bool) {
        self.output_id_stack.push(output_id);
    }

    pub(super) fn pop_output_id(&mut self) {
        self.output_id_stack.pop();
    }

    pub(super) fn output_id(&self) -> bool {
        !self.use_esri_dialect
            && *self
                .output_id_stack
                .last()
                .expect("output id stack starts non-empty")
    }

    pub(super) fn output_usage(&self) -> bool {
        self.output_id()
    }

    pub(super) fn push_axis_linear_unit(&mut self, unit: &UnitOfMeasure) {
        self.axis_linear_unit_stack.push(unit.clone());
    }

    pub(super) fn pop_axis_linear_unit(&mut self) {
        self.axis_linear_unit_stack.pop();
    }

    pub(super) fn axis_linear_unit(&self) -> &UnitOfMeasure {
        self.axis_linear_unit_stack
            .last()
            .expect("linear unit stack starts non-empty")
    }

    pub(super) fn push_axis_angular_unit(&mut self, unit: &UnitOfMeasure) {
        self.axis_angular_unit_stack.push(unit.clone());
    }

    pub(super) fn pop_axis_angular_unit(&mut self) {
        self.axis_angular_unit_stack.pop();
    }

    pub(super) fn axis_angular_unit(&self) -> &UnitOfMeasure {
        self.axis_angular_unit_stack
            .last()
            .expect("angular unit stack starts non-empty")
    }

    pub(super) fn ingest_node(&mut self, node: &super::node::WktNode) {
        self.start_node(&node.value, true);
        for child in &node.children {
            if child.children.is_empty() {
                self.add_raw(&child.value);
            } else {
                self.ingest_node(child);
            }
        }
        self.end_node();
    }
}

pub(super) fn morph_name_to_esri(name: &str) -> String {
    // WKTFormatter::morphNameToESRI
    for suffix in ["(m)", "(ftUS)", "(E-N)", "(N-E)"] {
        if let Some(stem) = name.strip_suffix(suffix) {
            return morph_name_to_esri(stem) + suffix;
        }
    }
    let mut morphed = String::new();
    let mut insert_underscore = false;
    for ch in name.chars() {
        if ch == '+' || ch == '-' || ch.is_ascii_alphanumeric() {
            if insert_underscore && !morphed.is_empty() {
                morphed.push('_');
            }
            morphed.push(ch);
            insert_underscore = false;
        } else {
            insert_underscore = true;
        }
    }
    morphed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esri_names_replace_punctuation_runs_with_one_underscore() {
        assert_eq!(
            morph_name_to_esri("Nouvelle Triangulation Francaise (Paris)"),
            "Nouvelle_Triangulation_Francaise_Paris"
        );
        assert_eq!(
            morph_name_to_esri("NAD83 / Idaho East (ftUS)"),
            "NAD83_Idaho_East(ftUS)"
        );
        assert_eq!(
            morph_name_to_esri("World Geodetic System 1984"),
            "World_Geodetic_System_1984"
        );
    }

    #[test]
    fn numbers_are_written_with_fifteen_digits_and_esri_decimal_points() {
        let mut formatter = Formatter::new(WktDialect::Wkt1Esri, WktLayout::SingleLine);
        formatter.start_node("A", false);
        formatter.add_double(500000.0);
        formatter.add_double(0.0);
        formatter.add_double(1e-6);
        formatter.end_node();
        assert_eq!(formatter.into_string(), "A[500000.0,0.0,1E-06.0]");
    }
}
