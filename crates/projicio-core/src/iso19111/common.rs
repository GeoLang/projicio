// adapted from PROJ src/iso19111/common.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::PI;

use super::metadata::{EPSG, Extent, Identifier, is_equivalent_name};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitType {
    Unknown,
    None,
    Angular,
    Linear,
    Scale,
    Time,
    Parametric,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnitOfMeasure {
    pub name: String,
    pub conversion_to_si: f64,
    pub unit_type: UnitType,
    pub code_space: String,
    pub code: String,
}

impl UnitOfMeasure {
    fn epsg(name: &str, conversion_to_si: f64, unit_type: UnitType, code: &str) -> Self {
        Self {
            name: name.to_string(),
            conversion_to_si,
            unit_type,
            code_space: EPSG.to_string(),
            code: code.to_string(),
        }
    }

    pub fn none() -> Self {
        Self {
            name: String::new(),
            conversion_to_si: 1.0,
            unit_type: UnitType::None,
            code_space: String::new(),
            code: String::new(),
        }
    }

    pub fn metre() -> Self {
        Self::epsg("metre", 1.0, UnitType::Linear, "9001")
    }

    pub fn degree() -> Self {
        Self::epsg("degree", PI / 180.0, UnitType::Angular, "9122")
    }

    pub fn arc_second() -> Self {
        Self::epsg("arc-second", PI / 180.0 / 3600.0, UnitType::Angular, "9104")
    }

    pub fn scale_unity() -> Self {
        Self::epsg("unity", 1.0, UnitType::Scale, "9201")
    }

    pub fn parts_per_million() -> Self {
        Self::epsg("parts per million", 1e-6, UnitType::Scale, "9202")
    }

    pub fn has_same_name(&self, other: &Self) -> bool {
        // PROJ's UnitOfMeasure::operator== compares names only
        self.name == other.name
    }

    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        (self.conversion_to_si - other.conversion_to_si).abs()
            <= 1e-10 * self.conversion_to_si.abs()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Measure {
    pub value: f64,
    pub unit: UnitOfMeasure,
}

pub(crate) const DEFAULT_MAX_RELATIVE_ERROR: f64 = 1e-10;

impl Measure {
    pub fn new(value: f64, unit: UnitOfMeasure) -> Self {
        Self { value, unit }
    }

    pub fn si_value(&self) -> f64 {
        self.value * self.unit.conversion_to_si
    }

    pub fn convert_to_unit(&self, unit: &UnitOfMeasure) -> f64 {
        self.si_value() / unit.conversion_to_si
    }

    pub fn is_identical_to(&self, other: &Self) -> bool {
        self.value == other.value && self.unit.has_same_name(&other.unit)
    }

    pub fn is_equivalent_to(&self, other: &Self, max_relative_error: f64) -> bool {
        let si_value = self.si_value();
        let other_si_value = other.si_value();
        if si_value.is_infinite() && other_si_value.is_infinite() {
            return si_value * other_si_value > 0.0;
        }
        (si_value - other_si_value).abs() <= max_relative_error * si_value.abs()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectDomain {
    pub scope: Option<String>,
    pub domain_of_validity: Option<Extent>,
}

pub(crate) fn epsg_code(identifiers: &[Identifier]) -> i32 {
    identifiers
        .iter()
        .find(|id| id.code_space.eq_ignore_ascii_case(EPSG))
        .map_or(0, |id| leading_integer(&id.code).unwrap_or(0))
}

pub(crate) fn leading_integer(text: &str) -> Option<i32> {
    // C's atoi and std::stoi read leading digits and ignore the rest
    let trimmed = text.trim_start();
    let sign_length = usize::from(trimmed.starts_with(['+', '-']));
    let digit_count = trimmed[sign_length..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    if digit_count == 0 {
        return None;
    }
    trimmed[..sign_length + digit_count].parse().ok()
}

pub(crate) fn names_are_equivalent(name: &str, other_name: &str) -> bool {
    if is_equivalent_name(name, other_name) {
        return true;
    }
    (name == "unknown" && other_name == "unnamed") || (name == "unnamed" && other_name == "unknown")
}
