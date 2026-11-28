// adapted from PROJ src/iso19111/operation/coordinateoperation.cpp, concatenatedoperation.cpp, singleoperation.cpp, transformation.cpp, conversion.cpp and oputils.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::common::{Measure, ObjectDomain, UnitOfMeasure, epsg_code};
use super::crs::Crs;
use super::metadata::Identifier;
use super::operation::{
    Conversion, OperationMethod, OperationParameter, OperationParameterValue, ParameterValue,
    Transformation,
};
use crate::Error;

pub(crate) const PARAMETER_X_AXIS_TRANSLATION: i32 = 8605;
pub(crate) const PARAMETER_Y_AXIS_TRANSLATION: i32 = 8606;
pub(crate) const PARAMETER_Z_AXIS_TRANSLATION: i32 = 8607;
pub(crate) const PARAMETER_X_AXIS_ROTATION: i32 = 8608;
pub(crate) const PARAMETER_Y_AXIS_ROTATION: i32 = 8609;
pub(crate) const PARAMETER_Z_AXIS_ROTATION: i32 = 8610;
pub(crate) const PARAMETER_SCALE_DIFFERENCE: i32 = 8611;
pub(crate) const PARAMETER_RATE_X_AXIS_TRANSLATION: i32 = 1040;
pub(crate) const PARAMETER_RATE_Y_AXIS_TRANSLATION: i32 = 1041;
pub(crate) const PARAMETER_RATE_Z_AXIS_TRANSLATION: i32 = 1042;
pub(crate) const PARAMETER_RATE_X_AXIS_ROTATION: i32 = 1043;
pub(crate) const PARAMETER_RATE_Y_AXIS_ROTATION: i32 = 1044;
pub(crate) const PARAMETER_RATE_Z_AXIS_ROTATION: i32 = 1045;
pub(crate) const PARAMETER_RATE_SCALE_DIFFERENCE: i32 = 1046;
pub(crate) const PARAMETER_REFERENCE_EPOCH: i32 = 1047;
pub(crate) const PARAMETER_TRANSFORMATION_REFERENCE_EPOCH: i32 = 1049;
pub(crate) const PARAMETER_ORDINATE_1_EVAL_POINT: i32 = 8617;
pub(crate) const PARAMETER_ORDINATE_2_EVAL_POINT: i32 = 8618;
pub(crate) const PARAMETER_ORDINATE_3_EVAL_POINT: i32 = 8667;
pub(crate) const PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE: i32 = 8656;
pub(crate) const PARAMETER_LATITUDE_DIFFERENCE_FILE: i32 = 8657;
pub(crate) const PARAMETER_LONGITUDE_DIFFERENCE_FILE: i32 = 8658;
pub(crate) const PARAMETER_GEOCENTRIC_TRANSLATION_FILE: i32 = 8727;
pub(crate) const PARAMETER_POINT_MOTION_VELOCITY_GRID_FILE: i32 = 1050;
pub(crate) const PARAMETER_POINT_MOTION_VELOCITY_NORTH_GRID_FILE: i32 = 1072;
pub(crate) const PARAMETER_TIN_OFFSET_FILE: i32 = 1064;
const PARAMETER_LATITUDE_OFFSET: i32 = 8601;
const PARAMETER_LONGITUDE_OFFSET: i32 = 8602;
const PARAMETER_VERTICAL_OFFSET: i32 = 8603;
const PARAMETER_GEOID_HEIGHT: i32 = 8604;
const PARAMETER_EASTING_OFFSET: i32 = 8728;
const PARAMETER_NORTHING_OFFSET: i32 = 8729;
const PARAMETER_SEMI_MAJOR_AXIS_DIFFERENCE: i32 = 8654;
const PARAMETER_FLATTENING_DIFFERENCE: i32 = 8655;

// proj_constants.h has these names, which proj's createOpParamNameEPSGCode gives the codes
const EPSG_PARAMETER_NAMES: [(i32, &str); 34] = [
    (PARAMETER_X_AXIS_TRANSLATION, "X-axis translation"),
    (PARAMETER_Y_AXIS_TRANSLATION, "Y-axis translation"),
    (PARAMETER_Z_AXIS_TRANSLATION, "Z-axis translation"),
    (PARAMETER_X_AXIS_ROTATION, "X-axis rotation"),
    (PARAMETER_Y_AXIS_ROTATION, "Y-axis rotation"),
    (PARAMETER_Z_AXIS_ROTATION, "Z-axis rotation"),
    (PARAMETER_SCALE_DIFFERENCE, "Scale difference"),
    (
        PARAMETER_RATE_X_AXIS_TRANSLATION,
        "Rate of change of X-axis translation",
    ),
    (
        PARAMETER_RATE_Y_AXIS_TRANSLATION,
        "Rate of change of Y-axis translation",
    ),
    (
        PARAMETER_RATE_Z_AXIS_TRANSLATION,
        "Rate of change of Z-axis translation",
    ),
    (
        PARAMETER_RATE_X_AXIS_ROTATION,
        "Rate of change of X-axis rotation",
    ),
    (
        PARAMETER_RATE_Y_AXIS_ROTATION,
        "Rate of change of Y-axis rotation",
    ),
    (
        PARAMETER_RATE_Z_AXIS_ROTATION,
        "Rate of change of Z-axis rotation",
    ),
    (
        PARAMETER_RATE_SCALE_DIFFERENCE,
        "Rate of change of Scale difference",
    ),
    (PARAMETER_REFERENCE_EPOCH, "Parameter reference epoch"),
    (
        PARAMETER_TRANSFORMATION_REFERENCE_EPOCH,
        "Transformation reference epoch",
    ),
    (
        PARAMETER_ORDINATE_1_EVAL_POINT,
        "Ordinate 1 of evaluation point",
    ),
    (
        PARAMETER_ORDINATE_2_EVAL_POINT,
        "Ordinate 2 of evaluation point",
    ),
    (
        PARAMETER_ORDINATE_3_EVAL_POINT,
        "Ordinate 3 of evaluation point",
    ),
    (
        PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE,
        "Latitude and longitude difference file",
    ),
    (
        PARAMETER_LATITUDE_DIFFERENCE_FILE,
        "Latitude difference file",
    ),
    (
        PARAMETER_LONGITUDE_DIFFERENCE_FILE,
        "Longitude difference file",
    ),
    (
        PARAMETER_GEOCENTRIC_TRANSLATION_FILE,
        "Geocentric translation file",
    ),
    (
        PARAMETER_POINT_MOTION_VELOCITY_GRID_FILE,
        "Point motion velocity grid file",
    ),
    (
        PARAMETER_POINT_MOTION_VELOCITY_NORTH_GRID_FILE,
        "Point motion velocity north grid file",
    ),
    (PARAMETER_TIN_OFFSET_FILE, "TIN offset file"),
    (PARAMETER_LATITUDE_OFFSET, "Latitude offset"),
    (PARAMETER_LONGITUDE_OFFSET, "Longitude offset"),
    (PARAMETER_VERTICAL_OFFSET, "Vertical Offset"),
    (PARAMETER_GEOID_HEIGHT, "Geoid height"),
    (PARAMETER_EASTING_OFFSET, "Easting offset"),
    (PARAMETER_NORTHING_OFFSET, "Northing offset"),
    (
        PARAMETER_SEMI_MAJOR_AXIS_DIFFERENCE,
        "Semi-major axis length difference",
    ),
    (PARAMETER_FLATTENING_DIFFERENCE, "Flattening difference"),
];

pub(crate) fn epsg_parameter(code: i32) -> OperationParameter {
    let (_, name) = EPSG_PARAMETER_NAMES
        .iter()
        .find(|(parameter_code, _)| *parameter_code == code)
        .unwrap_or_else(|| panic!("EPSG parameter {code} is missing from the name table"));
    OperationParameter {
        name: (*name).to_string(),
        identifiers: vec![Identifier::new("EPSG", &code.to_string())],
    }
}

const METHOD_GEOCENTRIC_TRANSLATION_GEOCENTRIC: i32 = 1031;
const METHOD_GEOCENTRIC_TRANSLATION_GEOGRAPHIC_2D: i32 = 9603;
const METHOD_GEOCENTRIC_TRANSLATION_GEOGRAPHIC_3D: i32 = 1035;
const METHOD_MOLODENSKY: i32 = 9604;
const METHOD_ABRIDGED_MOLODENSKY: i32 = 9605;
const METHOD_LONGITUDE_ROTATION: i32 = 9601;
const METHOD_GEOGRAPHIC_2D_OFFSETS: i32 = 9619;
const METHOD_GEOGRAPHIC_2D_WITH_HEIGHT_OFFSETS: i32 = 9618;
const METHOD_GEOGRAPHIC_3D_OFFSETS: i32 = 9660;
const METHOD_CARTESIAN_GRID_OFFSETS: i32 = 9656;

const INVERSE_OF: &str = "Inverse of ";

#[derive(Debug, Clone, PartialEq)]
pub struct PointMotionOperation {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub remarks: String,
    pub domains: Vec<ObjectDomain>,
    // a point motion operation moves points within one crs
    pub crs: Box<Crs>,
    pub method: OperationMethod,
    pub parameter_values: Vec<OperationParameterValue>,
    pub accuracies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConcatenatedOperation {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub remarks: String,
    pub domains: Vec<ObjectDomain>,
    pub source_crs: Box<Crs>,
    pub target_crs: Box<Crs>,
    pub steps: Vec<CoordinateOperation>,
    pub accuracies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoordinateOperation {
    // proj gives a conversion the crs it runs between when it chains the conversion
    Conversion {
        conversion: Conversion,
        source_crs: Box<Crs>,
        target_crs: Box<Crs>,
    },
    Transformation(Transformation),
    PointMotion(PointMotionOperation),
    Concatenated(ConcatenatedOperation),
}

impl CoordinateOperation {
    pub fn name(&self) -> &str {
        match self {
            Self::Conversion { conversion, .. } => &conversion.name,
            Self::Transformation(transformation) => &transformation.name,
            Self::PointMotion(operation) => &operation.name,
            Self::Concatenated(operation) => &operation.name,
        }
    }

    pub fn identifiers(&self) -> &[Identifier] {
        match self {
            Self::Conversion { conversion, .. } => &conversion.identifiers,
            Self::Transformation(transformation) => &transformation.identifiers,
            Self::PointMotion(operation) => &operation.identifiers,
            Self::Concatenated(operation) => &operation.identifiers,
        }
    }

    pub fn source_crs(&self) -> &Crs {
        match self {
            Self::Conversion { source_crs, .. } => source_crs,
            Self::Transformation(transformation) => &transformation.source_crs,
            Self::PointMotion(operation) => &operation.crs,
            Self::Concatenated(operation) => &operation.source_crs,
        }
    }

    pub fn target_crs(&self) -> &Crs {
        match self {
            Self::Conversion { target_crs, .. } => target_crs,
            Self::Transformation(transformation) => &transformation.target_crs,
            Self::PointMotion(operation) => &operation.crs,
            Self::Concatenated(operation) => &operation.target_crs,
        }
    }

    pub fn accuracies(&self) -> &[String] {
        match self {
            // proj counts a conversion as exact without giving it an accuracy
            Self::Conversion { .. } => &[],
            Self::Transformation(transformation) => &transformation.accuracies,
            Self::PointMotion(operation) => &operation.accuracies,
            Self::Concatenated(operation) => &operation.accuracies,
        }
    }
}

#[cfg(feature = "registry")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridNeeded<'a> {
    pub file_name: &'a str,
    pub alternative: Option<&'static crate::registry::GridAlternativeRecord<'static>>,
}

#[cfg(feature = "registry")]
impl CoordinateOperation {
    fn file_names(&self) -> Vec<&str> {
        let parameter_values = match self {
            Self::Conversion { conversion, .. } => &conversion.parameter_values,
            Self::Transformation(transformation) => &transformation.parameter_values,
            Self::PointMotion(operation) => &operation.parameter_values,
            Self::Concatenated(operation) => {
                return operation
                    .steps
                    .iter()
                    .flat_map(CoordinateOperation::file_names)
                    .collect();
            }
        };
        parameter_values
            .iter()
            .filter_map(|parameter_value| match &parameter_value.value {
                ParameterValue::Filename(file_name) => Some(file_name.as_str()),
                ParameterValue::Measure(_) | ParameterValue::Integer(_) => None,
            })
            .collect()
    }

    pub fn grids_needed(&self) -> Vec<GridNeeded<'_>> {
        let mut file_names = self.file_names();
        file_names.sort_unstable();
        file_names.dedup();
        file_names
            .into_iter()
            .map(|file_name| GridNeeded {
                file_name,
                alternative: crate::registry::grid_alternative_for_file(file_name),
            })
            .collect()
    }
}

fn crs_qualifier(crs: &Crs) -> &'static str {
    match crs {
        Crs::Geodetic(_) => " (geocentric)",
        Crs::Geographic(geographic) if geographic.coordinate_system.axes.len() == 2 => " (geog2D)",
        Crs::Geographic(_) => " (geog3D)",
        Crs::Projected(_) | Crs::Bound(_) => "",
    }
}

pub(crate) fn build_operation_name(operation_type: &str, source: &Crs, target: &Crs) -> String {
    let (mut source_qualifier, mut target_qualifier) = ("", "");
    if source.name() == target.name() {
        source_qualifier = crs_qualifier(source);
        target_qualifier = crs_qualifier(target);
        if source_qualifier == target_qualifier {
            source_qualifier = "";
            target_qualifier = "";
        }
    }
    format!(
        "{operation_type} from {}{source_qualifier} to {}{target_qualifier}",
        source.name(),
        target.name()
    )
}

pub(crate) fn modified_identifiers(
    identifiers: &[Identifier],
    inverse: bool,
    derived_from: bool,
) -> Vec<Identifier> {
    identifiers
        .iter()
        .map(|identifier| {
            let mut authority = identifier.code_space.clone();
            if derived_from {
                authority = format!("DERIVED_FROM({authority})");
            }
            if inverse {
                authority = match authority
                    .strip_prefix("INVERSE(")
                    .and_then(|rest| rest.strip_suffix(')'))
                {
                    Some(forward_authority) => forward_authority.to_string(),
                    None => format!("INVERSE({authority})"),
                };
            }
            Identifier::new(&authority, &identifier.code)
        })
        .collect()
}

fn inverse_method(method: &OperationMethod) -> OperationMethod {
    let name = match method.name.strip_prefix(INVERSE_OF) {
        Some(forward_name) => forward_name.to_string(),
        None => format!("{INVERSE_OF}{}", method.name),
    };
    OperationMethod {
        name,
        identifiers: modified_identifiers(&method.identifiers, true, false),
    }
}

fn split_on_plus_outside_quotes(name: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut characters = name.char_indices();
    while let Some((index, character)) = characters.next() {
        if in_string {
            current.push(character);
            in_string = character != '\'';
        } else if index + 3 < name.len() && name[index..].starts_with(" + ") {
            tokens.push(std::mem::take(&mut current));
            characters.next();
            characters.next();
        } else {
            in_string = character == '\'';
            current.push(character);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

const AXIS_ORDER_CHANGE_2D_NAME: &str = "axis order change (2D)";
const AXIS_ORDER_CHANGE_3D_NAME: &str = "axis order change (geographic3D horizontal)";

fn inverse_conversion_name(forward_name: &str) -> String {
    // proj's createPropertiesForInverse for a conversion chained without its crs
    if forward_name.is_empty() {
        return String::new();
    }
    if !forward_name.starts_with(INVERSE_OF) && !forward_name.contains(" + ") {
        return format!("{INVERSE_OF}{forward_name}");
    }
    let tokens = split_on_plus_outside_quotes(forward_name);
    let inverted: Vec<String> = tokens
        .iter()
        .rev()
        .map(|token| match token.strip_prefix(INVERSE_OF) {
            Some(forward_token) => forward_token.to_string(),
            None if token == AXIS_ORDER_CHANGE_2D_NAME || token == AXIS_ORDER_CHANGE_3D_NAME => {
                token.clone()
            }
            None => format!("{INVERSE_OF}{token}"),
        })
        .collect();
    inverted.join(" + ")
}

pub(crate) fn inverse_transformation_name(
    forward_name: &str,
    source: &Crs,
    target: &Crs,
) -> String {
    // proj 9.8.1's createPropertiesForInverse keeps a transformation's leading "Inverse of"
    let operation_type = "Transformation";
    if forward_name.is_empty()
        || forward_name == build_operation_name(operation_type, source, target)
    {
        return build_operation_name(operation_type, target, source);
    }
    if forward_name.contains(" + ") {
        return format!("{INVERSE_OF}'{forward_name}'");
    }
    format!("{INVERSE_OF}{forward_name}")
}

impl Conversion {
    pub fn inverse(&self) -> Conversion {
        // proj's InverseConversion, written as a Conversion with inverted names
        Conversion {
            name: inverse_conversion_name(&self.name),
            identifiers: modified_identifiers(&self.identifiers, true, false),
            method: inverse_method(&self.method),
            parameter_values: self.parameter_values.clone(),
        }
    }
}

fn negate(value: f64) -> f64 {
    // proj avoids writing -0
    if value == 0.0 { 0.0 } else { -value }
}

fn parameter_measure(parameter_values: &[OperationParameterValue], code: i32) -> Option<&Measure> {
    parameter_values
        .iter()
        .find(|parameter_value| parameter_value.parameter.epsg_code() == code)
        .and_then(|parameter_value| match &parameter_value.value {
            ParameterValue::Measure(measure) => Some(measure),
            ParameterValue::Integer(_) | ParameterValue::Filename(_) => None,
        })
}

fn contains_ignoring_case(text: &str, needle: &str) -> bool {
    text.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

impl Transformation {
    pub fn inverse(&self) -> Result<Transformation, Error> {
        // proj's Transformation::inverseAsTransformation: negated parameters where the
        // method has a closed-form inverse, an "Inverse of" wrapper otherwise
        let method_code = epsg_code(&self.method.identifiers);
        let measure = |code: i32| {
            parameter_measure(&self.parameter_values, code).ok_or_else(|| Error::Factory {
                kind: "inverse of transformation",
                code: self.name.clone(),
                reason: format!("it has no EPSG parameter {code}"),
            })
        };
        let negated_si = |code: i32, unit: &UnitOfMeasure| -> Result<_, Error> {
            Ok(OperationParameterValue {
                parameter: epsg_parameter(code),
                value: ParameterValue::Measure(Measure::new(
                    negate(measure(code)?.si_value()),
                    unit.clone(),
                )),
            })
        };
        let negated_in_unit = |code: i32| -> Result<_, Error> {
            let forward = measure(code)?;
            Ok(OperationParameterValue {
                parameter: epsg_parameter(code),
                value: ParameterValue::Measure(Measure::new(
                    negate(forward.value),
                    forward.unit.clone(),
                )),
            })
        };
        let metre = UnitOfMeasure::metre();
        let translations = || -> Result<Vec<_>, Error> {
            [
                PARAMETER_X_AXIS_TRANSLATION,
                PARAMETER_Y_AXIS_TRANSLATION,
                PARAMETER_Z_AXIS_TRANSLATION,
            ]
            .into_iter()
            .map(|code| negated_si(code, &metre))
            .collect()
        };
        let is_geocentric_translation =
            (contains_ignoring_case(&self.method.name, "Geocentric translations")
                && !contains_ignoring_case(&self.method.name, "grid"))
                || matches!(
                    method_code,
                    METHOD_GEOCENTRIC_TRANSLATION_GEOCENTRIC
                        | METHOD_GEOCENTRIC_TRANSLATION_GEOGRAPHIC_2D
                        | METHOD_GEOCENTRIC_TRANSLATION_GEOGRAPHIC_3D
                );
        let offsets = |codes: &[i32]| -> Result<Vec<_>, Error> {
            codes.iter().map(|code| negated_in_unit(*code)).collect()
        };
        let negated_parameters = if is_geocentric_translation {
            Some(translations()?)
        } else {
            match method_code {
                METHOD_MOLODENSKY | METHOD_ABRIDGED_MOLODENSKY => {
                    let mut values = translations()?;
                    values.push(negated_si(PARAMETER_SEMI_MAJOR_AXIS_DIFFERENCE, &metre)?);
                    values.push(negated_si(
                        PARAMETER_FLATTENING_DIFFERENCE,
                        &UnitOfMeasure::scale_unity(),
                    )?);
                    Some(values)
                }
                METHOD_LONGITUDE_ROTATION => Some(offsets(&[PARAMETER_LONGITUDE_OFFSET])?),
                METHOD_GEOGRAPHIC_2D_OFFSETS => Some(offsets(&[
                    PARAMETER_LATITUDE_OFFSET,
                    PARAMETER_LONGITUDE_OFFSET,
                ])?),
                METHOD_GEOGRAPHIC_3D_OFFSETS => Some(offsets(&[
                    PARAMETER_LATITUDE_OFFSET,
                    PARAMETER_LONGITUDE_OFFSET,
                    PARAMETER_VERTICAL_OFFSET,
                ])?),
                METHOD_GEOGRAPHIC_2D_WITH_HEIGHT_OFFSETS => Some(offsets(&[
                    PARAMETER_LATITUDE_OFFSET,
                    PARAMETER_LONGITUDE_OFFSET,
                    PARAMETER_GEOID_HEIGHT,
                ])?),
                METHOD_CARTESIAN_GRID_OFFSETS => Some(offsets(&[
                    PARAMETER_EASTING_OFFSET,
                    PARAMETER_NORTHING_OFFSET,
                ])?),
                _ => None,
            }
        };
        let name = inverse_transformation_name(&self.name, &self.source_crs, &self.target_crs);
        let identifiers = modified_identifiers(&self.identifiers, true, false);
        let inverse = match negated_parameters {
            Some(parameter_values) => Transformation {
                name,
                identifiers,
                remarks: self.remarks.clone(),
                domains: self.domains.clone(),
                source_crs: self.target_crs.clone(),
                target_crs: self.source_crs.clone(),
                interpolation_crs: None,
                method: self.method.clone(),
                parameter_values,
                // proj's createLongitudeRotation takes no accuracy
                accuracies: if method_code == METHOD_LONGITUDE_ROTATION {
                    Vec::new()
                } else {
                    self.accuracies.clone()
                },
            },
            None => Transformation {
                name,
                identifiers,
                remarks: self.remarks.clone(),
                domains: self.domains.clone(),
                source_crs: self.target_crs.clone(),
                target_crs: self.source_crs.clone(),
                interpolation_crs: self.interpolation_crs.clone(),
                method: inverse_method(&self.method),
                parameter_values: self.parameter_values.clone(),
                accuracies: self.accuracies.clone(),
            },
        };
        Ok(inverse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iso19111::coordinate_system::EllipsoidalCs;
    use crate::iso19111::crs::GeographicCrs;
    use crate::iso19111::datum::{
        Ellipsoid, EllipsoidShape, GeodeticDatum, GeodeticReferenceFrame, PrimeMeridian,
    };

    fn geographic(name: &str) -> Crs {
        let ellipsoid = Ellipsoid {
            name: "GRS 1980".to_string(),
            identifiers: Vec::new(),
            semi_major_axis: Measure::new(6_378_137.0, UnitOfMeasure::metre()),
            shape: EllipsoidShape::InverseFlattening(298.257_222_101),
        };
        Crs::Geographic(GeographicCrs {
            name: name.to_string(),
            identifiers: Vec::new(),
            remarks: String::new(),
            domains: Vec::new(),
            datum: GeodeticDatum::ReferenceFrame(Box::new(GeodeticReferenceFrame {
                name: name.to_string(),
                identifiers: Vec::new(),
                remarks: String::new(),
                domains: Vec::new(),
                anchor: None,
                anchor_epoch: None,
                ellipsoid,
                prime_meridian: PrimeMeridian::greenwich(),
                dynamic: None,
            })),
            coordinate_system: EllipsoidalCs::latitude_longitude(&UnitOfMeasure::degree()),
            coordinate_system_is_implicit: false,
        })
    }

    #[test]
    fn inverse_names_follow_proj() {
        let a = geographic("A");
        let b = geographic("B");
        assert_eq!(
            inverse_transformation_name("ED50 to WGS 84 (18)", &a, &b),
            "Inverse of ED50 to WGS 84 (18)"
        );
        assert_eq!(
            inverse_transformation_name("Inverse of X", &a, &b),
            "Inverse of Inverse of X"
        );
        assert_eq!(
            inverse_transformation_name("Transformation from A to B", &a, &b),
            "Transformation from B to A"
        );
        assert_eq!(inverse_conversion_name("TM35FIN"), "Inverse of TM35FIN");
        assert_eq!(inverse_conversion_name("Inverse of TM35FIN"), "TM35FIN");
        assert_eq!(
            inverse_conversion_name("Inverse of P + Q"),
            "Inverse of Q + P"
        );
    }

    #[test]
    fn inverse_identifiers_wrap_and_unwrap_the_authority() {
        let forward = [Identifier::new("EPSG", "1311")];
        let inverse = modified_identifiers(&forward, true, false);
        assert_eq!(inverse, [Identifier::new("INVERSE(EPSG)", "1311")]);
        assert_eq!(modified_identifiers(&inverse, true, false), forward);
        assert_eq!(
            modified_identifiers(&forward, true, true),
            [Identifier::new("INVERSE(DERIVED_FROM(EPSG))", "1311")]
        );
    }
}
