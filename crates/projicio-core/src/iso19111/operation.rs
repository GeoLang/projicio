// adapted from PROJ src/iso19111/operation/singleoperation.cpp, conversion.cpp, transformation.cpp and parametervalue.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::common::{Measure, UnitOfMeasure, UnitType, epsg_code, names_are_equivalent};
use super::crs::Crs;
use super::metadata::{Identifier, is_equivalent_name};

const METHOD_LAMBERT_CONIC_CONFORMAL_2SP: i32 = 9802;
const METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA: i32 = 9820;
const METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA_SPHERICAL: i32 = 1027;
const METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA: i32 = 9835;
const METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA_SPHERICAL: i32 = 9834;
const METHOD_EQUIDISTANT_CYLINDRICAL: i32 = 1028;
const METHOD_EQUIDISTANT_CYLINDRICAL_SPHERICAL: i32 = 1029;

const METHOD_GEOCENTRIC_TRANSLATION_GEOCENTRIC: i32 = 1031;
const METHOD_COORDINATE_FRAME_GEOCENTRIC: i32 = 1032;
const METHOD_POSITION_VECTOR_GEOCENTRIC: i32 = 1033;
const METHOD_GEOCENTRIC_TRANSLATION_GEOGRAPHIC_2D: i32 = 9603;
const METHOD_POSITION_VECTOR_GEOGRAPHIC_2D: i32 = 9606;
const METHOD_COORDINATE_FRAME_GEOGRAPHIC_2D: i32 = 9607;
const METHOD_GEOCENTRIC_TRANSLATION_GEOGRAPHIC_3D: i32 = 1035;
const METHOD_POSITION_VECTOR_GEOGRAPHIC_3D: i32 = 1037;
const METHOD_COORDINATE_FRAME_GEOGRAPHIC_3D: i32 = 1038;
const METHOD_COORDINATE_FRAME_GEOG3D_TO_COMPOUND: i32 = 1149;
const METHOD_COORDINATE_FRAME_FULL_MATRIX_GEOCENTRIC: i32 = 1132;
const METHOD_COORDINATE_FRAME_FULL_MATRIX_GEOGRAPHIC_2D: i32 = 1133;
const METHOD_COORDINATE_FRAME_FULL_MATRIX_GEOGRAPHIC_3D: i32 = 1140;

const PARAMETER_LATITUDE_OF_NATURAL_ORIGIN: i32 = 8801;
const PARAMETER_SCALE_FACTOR_AT_NATURAL_ORIGIN: i32 = 8805;
const PARAMETER_AZIMUTH_PROJECTION_CENTRE: i32 = 8813;
const PARAMETER_ANGLE_RECTIFIED_TO_SKEW_GRID: i32 = 8814;
const PARAMETER_LATITUDE_1ST_STD_PARALLEL: i32 = 8823;
const PARAMETER_LATITUDE_2ND_STD_PARALLEL: i32 = 8824;
const PARAMETER_X_AXIS_TRANSLATION: i32 = 8605;
const PARAMETER_Y_AXIS_TRANSLATION: i32 = 8606;
const PARAMETER_Z_AXIS_TRANSLATION: i32 = 8607;
const PARAMETER_X_AXIS_ROTATION: i32 = 8608;
const PARAMETER_Y_AXIS_ROTATION: i32 = 8609;
const PARAMETER_Z_AXIS_ROTATION: i32 = 8610;
const PARAMETER_SCALE_DIFFERENCE: i32 = 8611;
pub(crate) const PARAMETER_EPSG_CODE_FOR_INTERPOLATION_CRS: i32 = 1048;
pub(crate) const PARAMETER_EPSG_CODE_FOR_HORIZONTAL_CRS: i32 = 1037;

#[derive(Debug, Clone, PartialEq)]
pub struct OperationMethod {
    pub name: String,
    pub identifiers: Vec<Identifier>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationParameter {
    pub name: String,
    pub identifiers: Vec<Identifier>,
}

impl OperationParameter {
    pub fn epsg_code(&self) -> i32 {
        // TODO: PROJ also looks the name up in its parameter table when there is no EPSG identifier
        let code = epsg_code(&self.identifiers);
        if code != 0 {
            return code;
        }
        if is_equivalent_name(&self.name, "Latitude of origin") {
            return PARAMETER_LATITUDE_OF_NATURAL_ORIGIN;
        }
        if is_equivalent_name(&self.name, "Scale factor") {
            return PARAMETER_SCALE_FACTOR_AT_NATURAL_ORIGIN;
        }
        0
    }

    fn is_equivalent_to(&self, other: &Self) -> bool {
        if names_are_equivalent(&self.name, &other.name) {
            return true;
        }
        let code = self.epsg_code();
        code != 0 && code == other.epsg_code()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParameterValue {
    Measure(Measure),
    Integer(i32),
    Filename(String),
}

impl ParameterValue {
    fn is_equivalent_to(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Measure(measure), Self::Measure(other_measure)) => {
                measure.is_equivalent_to(other_measure, 2e-10)
            }
            (Self::Integer(value), Self::Integer(other_value)) => value == other_value,
            (Self::Filename(name), Self::Filename(other_name)) => name == other_name,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationParameterValue {
    pub parameter: OperationParameter,
    pub value: ParameterValue,
}

impl OperationParameterValue {
    fn is_equivalent_to(&self, other: &Self) -> bool {
        if !self.parameter.is_equivalent_to(&other.parameter) {
            return false;
        }
        if self.value.is_equivalent_to(&other.value) {
            return true;
        }
        let code = self.parameter.epsg_code();
        if code != PARAMETER_AZIMUTH_PROJECTION_CENTRE
            && code != PARAMETER_ANGLE_RECTIFIED_TO_SKEW_GRID
        {
            return false;
        }
        let (ParameterValue::Measure(measure), ParameterValue::Measure(other_measure)) =
            (&self.value, &other.value)
        else {
            return false;
        };
        let degree = UnitOfMeasure::degree();
        let a = (measure.convert_to_unit(&degree) + 360.0) % 360.0;
        let b = (other_measure.convert_to_unit(&degree) + 360.0) % 360.0;
        (a - b).abs() <= 1e-10 * a.abs()
    }
}

fn parameter_value_by_code(
    values: &[OperationParameterValue],
    code: i32,
) -> Option<&ParameterValue> {
    values
        .iter()
        .find(|value| value.parameter.epsg_code() == code)
        .map(|value| &value.value)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conversion {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub method: OperationMethod,
    pub parameter_values: Vec<OperationParameterValue>,
}

impl Conversion {
    pub(crate) fn is_equivalent_to(&self, other: &Self, both_sources_are_spheres: bool) -> bool {
        let operation = SingleOperation {
            method: &self.method,
            values: &self.parameter_values,
        };
        let other_operation = SingleOperation {
            method: &other.method,
            values: &other.parameter_values,
        };
        operation.is_equivalent_to(&other_operation, both_sources_are_spheres, false)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transformation {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub source_crs: Box<Crs>,
    pub target_crs: Box<Crs>,
    pub method: OperationMethod,
    pub parameter_values: Vec<OperationParameterValue>,
}

impl Transformation {
    pub(crate) fn is_equivalent_to(&self, other: &Self) -> bool {
        let operation = SingleOperation {
            method: &self.method,
            values: &self.parameter_values,
        };
        let other_operation = SingleOperation {
            method: &other.method,
            values: &other.parameter_values,
        };
        operation.is_equivalent_to(&other_operation, false, false)
    }
}

fn contains_ignoring_case(text: &str, needle: &str) -> bool {
    text.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn towgs84_parameters(
    method: &OperationMethod,
    values: &[OperationParameterValue],
) -> Option<[f64; 7]> {
    let method_code = epsg_code(&method.identifiers);
    let parameter_count = values.len();
    let is_coordinate_frame = (parameter_count == 7
        && contains_ignoring_case(&method.name, "Coordinate Frame"))
        || matches!(
            method_code,
            METHOD_COORDINATE_FRAME_GEOCENTRIC
                | METHOD_COORDINATE_FRAME_FULL_MATRIX_GEOCENTRIC
                | METHOD_COORDINATE_FRAME_GEOGRAPHIC_2D
                | METHOD_COORDINATE_FRAME_FULL_MATRIX_GEOGRAPHIC_2D
                | METHOD_COORDINATE_FRAME_FULL_MATRIX_GEOGRAPHIC_3D
                | METHOD_COORDINATE_FRAME_GEOGRAPHIC_3D
                | METHOD_COORDINATE_FRAME_GEOG3D_TO_COMPOUND
        );
    let is_position_vector = (parameter_count == 7
        && contains_ignoring_case(&method.name, "Position Vector"))
        || matches!(
            method_code,
            METHOD_POSITION_VECTOR_GEOCENTRIC
                | METHOD_POSITION_VECTOR_GEOGRAPHIC_2D
                | METHOD_POSITION_VECTOR_GEOGRAPHIC_3D
        );
    let is_translation = (parameter_count == 3
        && contains_ignoring_case(&method.name, "Geocentric translations"))
        || matches!(
            method_code,
            METHOD_GEOCENTRIC_TRANSLATION_GEOCENTRIC
                | METHOD_GEOCENTRIC_TRANSLATION_GEOGRAPHIC_2D
                | METHOD_GEOCENTRIC_TRANSLATION_GEOGRAPHIC_3D
        );
    let seven_parameters = is_coordinate_frame || is_position_vector;
    if !seven_parameters && !is_translation {
        return None;
    }
    // GDAL WKT1 TOWGS84 uses the position vector convention
    let rotation_sign = if is_coordinate_frame { -1.0 } else { 1.0 };
    let fix_negative_zero = |value: f64| if value == 0.0 { 0.0 } else { value };
    let arc_second = UnitOfMeasure::arc_second();
    let mut parameters = [0.0; 7];
    let mut found = [false; 7];
    for value in values {
        let ParameterValue::Measure(measure) = &value.value else {
            continue;
        };
        let (index, number) = match value.parameter.epsg_code() {
            PARAMETER_X_AXIS_TRANSLATION => (0, measure.si_value()),
            PARAMETER_Y_AXIS_TRANSLATION => (1, measure.si_value()),
            PARAMETER_Z_AXIS_TRANSLATION => (2, measure.si_value()),
            PARAMETER_X_AXIS_ROTATION => (
                3,
                fix_negative_zero(rotation_sign * measure.convert_to_unit(&arc_second)),
            ),
            PARAMETER_Y_AXIS_ROTATION => (
                4,
                fix_negative_zero(rotation_sign * measure.convert_to_unit(&arc_second)),
            ),
            PARAMETER_Z_AXIS_ROTATION => (
                5,
                fix_negative_zero(rotation_sign * measure.convert_to_unit(&arc_second)),
            ),
            PARAMETER_SCALE_DIFFERENCE => (
                6,
                measure.convert_to_unit(&UnitOfMeasure::parts_per_million()),
            ),
            _ => continue,
        };
        parameters[index] = number;
        found[index] = true;
    }
    let translations_found = found[..3].iter().all(|&f| f);
    let rotations_and_scale_found = found[3..].iter().all(|&f| f);
    if translations_found && (is_translation || rotations_and_scale_found) {
        Some(parameters)
    } else {
        None
    }
}

fn is_towgs84_method(code: i32) -> bool {
    matches!(
        code,
        METHOD_GEOCENTRIC_TRANSLATION_GEOCENTRIC
            | METHOD_POSITION_VECTOR_GEOCENTRIC
            | METHOD_COORDINATE_FRAME_GEOCENTRIC
            | METHOD_GEOCENTRIC_TRANSLATION_GEOGRAPHIC_2D
            | METHOD_POSITION_VECTOR_GEOGRAPHIC_2D
            | METHOD_COORDINATE_FRAME_GEOGRAPHIC_2D
            | METHOD_GEOCENTRIC_TRANSLATION_GEOGRAPHIC_3D
            | METHOD_POSITION_VECTOR_GEOGRAPHIC_3D
            | METHOD_COORDINATE_FRAME_GEOGRAPHIC_3D
            | METHOD_COORDINATE_FRAME_GEOG3D_TO_COMPOUND
    )
}

fn are_spherical_variants(code: i32, other_code: i32) -> bool {
    let pairs = [
        (
            METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA,
            METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA_SPHERICAL,
        ),
        (
            METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA,
            METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA_SPHERICAL,
        ),
        (
            METHOD_EQUIDISTANT_CYLINDRICAL,
            METHOD_EQUIDISTANT_CYLINDRICAL_SPHERICAL,
        ),
    ];
    pairs
        .iter()
        .any(|&(a, b)| (code == a && other_code == b) || (code == b && other_code == a))
}

struct SingleOperation<'a> {
    method: &'a OperationMethod,
    values: &'a [OperationParameterValue],
}

impl SingleOperation<'_> {
    fn methods_are_equivalent(&self, other: &Self) -> bool {
        if !names_are_equivalent(&self.method.name, &other.method.name)
            || self.values.len() != other.values.len()
        {
            return false;
        }
        let mut candidates = vec![true; other.values.len()];
        for value in self.values {
            let matched = (0..other.values.len()).find(|&index| {
                candidates[index]
                    && value
                        .parameter
                        .is_equivalent_to(&other.values[index].parameter)
            });
            let Some(index) = matched else {
                return false;
            };
            candidates[index] = false;
        }
        true
    }

    fn is_equivalent_to(
        &self,
        other: &Self,
        both_sources_are_spheres: bool,
        in_other_direction: bool,
    ) -> bool {
        // TODO: PROJ also looks a method name up in its method table when there is no EPSG identifier
        let method_code = epsg_code(&self.method.identifiers);
        let other_method_code = epsg_code(&other.method.identifiers);
        let equivalent_methods = (method_code != 0 && method_code == other_method_code)
            || self.methods_are_equivalent(other)
            || (are_spherical_variants(method_code, other_method_code) && both_sources_are_spheres);

        if !equivalent_methods {
            if is_towgs84_method(method_code) && is_towgs84_method(other_method_code) {
                let (Some(parameters), Some(other_parameters)) = (
                    towgs84_parameters(self.method, self.values),
                    towgs84_parameters(other.method, other.values),
                ) else {
                    return false;
                };
                return parameters
                    .iter()
                    .zip(other_parameters)
                    .all(|(a, b)| (a - b).abs() <= 1e-10 * a.abs());
            }
            // TODO: PROJ also matches LCC 1SP with 2SP and Mercator A with B by converting one method to the other
            return false;
        }

        let mut candidates = vec![true; other.values.len()];
        let mut equivalent = true;
        let mut found_missing_arguments = self.values.len() != other.values.len();

        for value in self.values {
            if !equivalent {
                break;
            }
            equivalent = false;
            let mut same_name_different_value = false;
            for (index, other_value) in other.values.iter().enumerate() {
                if !candidates[index] {
                    continue;
                }
                if value.is_equivalent_to(other_value) {
                    candidates[index] = false;
                    equivalent = true;
                    break;
                }
                same_name_different_value =
                    value.parameter.is_equivalent_to(&other_value.parameter);
                if same_name_different_value {
                    candidates[index] = false;
                    break;
                }
            }

            if !equivalent && method_code == METHOD_LAMBERT_CONIC_CONFORMAL_2SP {
                // PROJ: the two standard parallels of LCC 2SP can be swapped
                let parameter_code = value.parameter.epsg_code();
                if parameter_code == PARAMETER_LATITUDE_1ST_STD_PARALLEL
                    || parameter_code == PARAMETER_LATITUDE_2ND_STD_PARALLEL
                {
                    let first =
                        parameter_value_by_code(self.values, PARAMETER_LATITUDE_1ST_STD_PARALLEL);
                    let second =
                        parameter_value_by_code(self.values, PARAMETER_LATITUDE_2ND_STD_PARALLEL);
                    let other_first =
                        parameter_value_by_code(other.values, PARAMETER_LATITUDE_1ST_STD_PARALLEL);
                    let other_second =
                        parameter_value_by_code(other.values, PARAMETER_LATITUDE_2ND_STD_PARALLEL);
                    if let (Some(first), Some(second), Some(other_first), Some(other_second)) =
                        (first, second, other_first, other_second)
                    {
                        equivalent = first.is_equivalent_to(other_second)
                            && second.is_equivalent_to(other_first);
                    }
                }
            }

            if equivalent {
                continue;
            }
            if same_name_different_value {
                break;
            }

            // PROJ: a parameter missing from the other operation must hold its neutral value
            found_missing_arguments = true;
            let ParameterValue::Measure(measure) = &value.value else {
                break;
            };
            equivalent = if measure.unit.unit_type == UnitType::Scale {
                measure.si_value() == 1.0
            } else {
                measure.si_value() == 0.0
            };
        }

        if equivalent && found_missing_arguments && !in_other_direction {
            return other.is_equivalent_to(self, both_sources_are_spheres, true);
        }
        // TODO: PROJ retries differing LCC 2SP formulations by converting both to LCC 1SP
        equivalent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameter(
        name: &str,
        code: &str,
        value: f64,
        unit: UnitOfMeasure,
    ) -> OperationParameterValue {
        OperationParameterValue {
            parameter: OperationParameter {
                name: name.to_string(),
                identifiers: vec![Identifier::new("EPSG", code)],
            },
            value: ParameterValue::Measure(Measure::new(value, unit)),
        }
    }

    fn lcc(first_parallel: f64, second_parallel: f64) -> Conversion {
        Conversion {
            name: "unknown".to_string(),
            identifiers: Vec::new(),
            method: OperationMethod {
                name: "Lambert Conic Conformal (2SP)".to_string(),
                identifiers: vec![Identifier::new("EPSG", "9802")],
            },
            parameter_values: vec![
                parameter(
                    "Latitude of false origin",
                    "8821",
                    42.5,
                    UnitOfMeasure::degree(),
                ),
                parameter(
                    "Latitude of 1st standard parallel",
                    "8823",
                    first_parallel,
                    UnitOfMeasure::degree(),
                ),
                parameter(
                    "Latitude of 2nd standard parallel",
                    "8824",
                    second_parallel,
                    UnitOfMeasure::degree(),
                ),
            ],
        }
    }

    #[test]
    fn lcc_2sp_with_swapped_standard_parallels_is_equivalent() {
        assert!(lcc(25.0, 60.0).is_equivalent_to(&lcc(60.0, 25.0), false));
        assert!(!lcc(25.0, 60.0).is_equivalent_to(&lcc(26.0, 60.0), false));
    }

    #[test]
    fn missing_parameter_at_its_neutral_value_is_equivalent() {
        let mut with_false_easting = lcc(25.0, 60.0);
        with_false_easting.parameter_values.push(parameter(
            "Easting at false origin",
            "8826",
            0.0,
            UnitOfMeasure::metre(),
        ));
        assert!(with_false_easting.is_equivalent_to(&lcc(25.0, 60.0), false));

        with_false_easting.parameter_values[3] = parameter(
            "Easting at false origin",
            "8826",
            500000.0,
            UnitOfMeasure::metre(),
        );
        assert!(!with_false_easting.is_equivalent_to(&lcc(25.0, 60.0), false));
    }
}
