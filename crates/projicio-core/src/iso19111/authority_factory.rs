// adapted from PROJ src/iso19111/factory.cpp, operation/concatenatedoperation.cpp, operation/singleoperation.cpp and operation/conversion.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::PI;

use super::common::{Measure, ObjectDomain, UnitOfMeasure, UnitType, epsg_code};
use super::coordinate_operation::{
    ConcatenatedOperation, CoordinateOperation, PARAMETER_GEOCENTRIC_TRANSLATION_FILE,
    PARAMETER_LATITUDE_DIFFERENCE_FILE, PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE,
    PARAMETER_LONGITUDE_DIFFERENCE_FILE, PARAMETER_ORDINATE_1_EVAL_POINT,
    PARAMETER_ORDINATE_2_EVAL_POINT, PARAMETER_ORDINATE_3_EVAL_POINT,
    PARAMETER_POINT_MOTION_VELOCITY_GRID_FILE, PARAMETER_POINT_MOTION_VELOCITY_NORTH_GRID_FILE,
    PARAMETER_RATE_SCALE_DIFFERENCE, PARAMETER_RATE_X_AXIS_ROTATION,
    PARAMETER_RATE_X_AXIS_TRANSLATION, PARAMETER_RATE_Y_AXIS_ROTATION,
    PARAMETER_RATE_Y_AXIS_TRANSLATION, PARAMETER_RATE_Z_AXIS_ROTATION,
    PARAMETER_RATE_Z_AXIS_TRANSLATION, PARAMETER_REFERENCE_EPOCH, PARAMETER_SCALE_DIFFERENCE,
    PARAMETER_TIN_OFFSET_FILE, PARAMETER_TRANSFORMATION_REFERENCE_EPOCH, PARAMETER_X_AXIS_ROTATION,
    PARAMETER_X_AXIS_TRANSLATION, PARAMETER_Y_AXIS_ROTATION, PARAMETER_Y_AXIS_TRANSLATION,
    PARAMETER_Z_AXIS_ROTATION, PARAMETER_Z_AXIS_TRANSLATION, PointMotionOperation,
    build_operation_name, epsg_parameter, inverse_transformation_name, modified_identifiers,
};
use super::coordinate_system::{
    AxisDirection, CartesianCs, CoordinateSystem, CoordinateSystemAxis, EllipsoidalCs, Meridian,
};
use super::crs::{Criterion, Crs, GeodeticCrs, GeographicCrs, ProjectedCrs};
use super::datum::{
    DatumEnsemble, DynamicFrame, Ellipsoid, EllipsoidShape, GeodeticDatum, GeodeticReferenceFrame,
    PrimeMeridian, ensemble_name_to_non_ensemble_name,
};
use super::metadata::{EPSG, Extent, GeographicBoundingBox, Identifier};
use super::operation::{
    Conversion, OperationMethod, OperationParameter, OperationParameterValue, ParameterValue,
    Transformation,
};
use super::projjson::format_general;
use crate::Error;
use crate::registry::{
    self, CoordinateSystemKind, CrsDefinitionKind, DatumRecord, GeodeticCrsType, HelmertParameters,
    OperationCode, OperationKind, OperationRecord, OperationStep, StepDirection, StepReference,
    TransformationParameters, UnitKind,
};

fn epsg_identifiers(code: u32) -> Vec<Identifier> {
    vec![Identifier::new(EPSG, &code.to_string())]
}

fn not_found(kind: &'static str, code: impl std::fmt::Display) -> Error {
    Error::NoSuchAuthorityCode {
        kind,
        code: format!("EPSG:{code}"),
    }
}

fn cannot_build(kind: &'static str, code: impl std::fmt::Display, reason: String) -> Error {
    Error::Factory {
        kind,
        code: code.to_string(),
        reason,
    }
}

const UNIT_DEGREE: u32 = 9102;
const UNIT_SEXAGESIMAL_DMS: u32 = 9110;
// proj gives these units the degree's factor rather than proj.db's
const UNITS_WITH_DEGREE_FACTOR: [u32; 2] = [9107, 9108];
const DEGREE_IN_RADIANS: f64 = PI / 180.0;
const ARC_SECOND_IN_RADIANS: f64 = PI / 180.0 / 3600.0;

pub fn create_unit_of_measure(code: u32) -> Result<UnitOfMeasure, Error> {
    let record =
        registry::unit_of_measure(code).ok_or_else(|| not_found("unit of measure", code))?;
    let name = match record.name {
        "degree (supplier to define representation)" => "degree",
        name => name,
    };
    let mut conversion_to_si = if UNITS_WITH_DEGREE_FACTOR.contains(&code) {
        DEGREE_IN_RADIANS
    } else {
        record.to_si_factor.ok_or_else(|| {
            cannot_build(
                "unit of measure",
                format!("EPSG:{code}"),
                "proj.db gives it no conversion factor".to_string(),
            )
        })?
    };
    for exact in [DEGREE_IN_RADIANS, ARC_SECOND_IN_RADIANS] {
        if (conversion_to_si - exact).abs() < 1e-10 * exact {
            conversion_to_si = exact;
        }
    }
    let unit_type = match record.kind {
        UnitKind::Length => UnitType::Linear,
        UnitKind::Angle => UnitType::Angular,
        UnitKind::Scale => UnitType::Scale,
        UnitKind::Time => UnitType::Time,
    };
    Ok(UnitOfMeasure {
        name: name.to_string(),
        conversion_to_si,
        unit_type,
        code_space: EPSG.to_string(),
        code: code.to_string(),
    })
}

const DMS_FRACTION_DIGITS: usize = 12;
// DDD.MMSSsssssss: the ten digits after the minutes are seconds times 10^8
const DMS_SECONDS_SCALE: f64 = 1e8;

fn normalize_measure(unit_code: u32, value: f64) -> (f64, u32) {
    // proj's normalizeMeasure: a sexagesimal DMS value becomes decimal degrees
    if unit_code != UNIT_SEXAGESIMAL_DMS {
        return (value, unit_code);
    }
    let formatted = format!("{value:.DMS_FRACTION_DIGITS$}");
    let (_, fraction) = formatted
        .split_once('.')
        .expect("fixed formatting with twelve decimals has a point");
    let minutes: f64 = fraction[..2].parse().expect("two minute digits");
    let seconds: f64 = fraction[2..].parse().expect("ten second digits");
    let sign = if value < 0.0 { -1.0 } else { 1.0 };
    let degrees =
        sign * (value.abs().floor() + minutes / 60.0 + (seconds / DMS_SECONDS_SCALE) / 3600.0);
    (degrees, UNIT_DEGREE)
}

fn measure_in_unit(value: f64, unit_code: u32) -> Result<Measure, Error> {
    let (value, unit_code) = normalize_measure(unit_code, value);
    Ok(Measure::new(value, create_unit_of_measure(unit_code)?))
}

pub fn create_ellipsoid(code: u32) -> Result<Ellipsoid, Error> {
    let record = registry::ellipsoid(code).ok_or_else(|| not_found("ellipsoid", code))?;
    let unit = create_unit_of_measure(record.unit_code)?;
    let shape = match record.shape {
        registry::EllipsoidShape::InverseFlattening(inverse_flattening) => {
            EllipsoidShape::InverseFlattening(inverse_flattening)
        }
        registry::EllipsoidShape::SemiMinorAxis(semi_minor_axis)
            if semi_minor_axis == record.semi_major_axis =>
        {
            EllipsoidShape::Sphere
        }
        registry::EllipsoidShape::SemiMinorAxis(semi_minor_axis) => {
            EllipsoidShape::SemiMinorAxis(Measure::new(semi_minor_axis, unit.clone()))
        }
    };
    Ok(Ellipsoid {
        name: record.name.to_string(),
        identifiers: epsg_identifiers(code),
        semi_major_axis: Measure::new(record.semi_major_axis, unit),
        shape,
    })
}

pub fn create_prime_meridian(code: u32) -> Result<PrimeMeridian, Error> {
    let record = registry::prime_meridian(code).ok_or_else(|| not_found("prime meridian", code))?;
    Ok(PrimeMeridian {
        name: record.name.to_string(),
        identifiers: epsg_identifiers(code),
        longitude: measure_in_unit(record.longitude, record.unit_code)?,
    })
}

fn reference_frame(record: &DatumRecord<'_>, name: &str) -> Result<GeodeticReferenceFrame, Error> {
    Ok(GeodeticReferenceFrame {
        name: name.to_string(),
        identifiers: epsg_identifiers(record.code),
        remarks: String::new(),
        domains: Vec::new(),
        anchor: None,
        anchor_epoch: record.anchor_epoch,
        ellipsoid: create_ellipsoid(record.ellipsoid_code)?,
        prime_meridian: create_prime_meridian(record.prime_meridian_code)?,
        dynamic: record
            .frame_reference_epoch
            .map(|frame_reference_epoch| DynamicFrame {
                frame_reference_epoch,
                deformation_model_name: None,
            }),
    })
}

pub fn create_geodetic_datum(code: u32) -> Result<GeodeticReferenceFrame, Error> {
    // proj's createGeodeticDatum, which turns an ensemble into the datum it stands for
    let record = registry::geodetic_datum(code).ok_or_else(|| not_found("geodetic datum", code))?;
    let name = ensemble_name_to_non_ensemble_name(record.name).unwrap_or(record.name);
    reference_frame(record, name)
}

pub fn create_datum_ensemble(code: u32) -> Result<DatumEnsemble, Error> {
    let record = registry::geodetic_datum(code).ok_or_else(|| not_found("datum ensemble", code))?;
    let ensemble = record
        .ensemble
        .as_ref()
        .ok_or_else(|| not_found("datum ensemble", code))?;
    let members = ensemble
        .member_codes
        .iter()
        .map(|member_code| create_geodetic_datum(*member_code))
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(DatumEnsemble {
        name: record.name.to_string(),
        identifiers: epsg_identifiers(code),
        members,
        accuracy: sqlite_real_text(ensemble.accuracy),
    })
}

fn create_geodetic_datum_or_ensemble(code: u32) -> Result<GeodeticDatum, Error> {
    let record = registry::geodetic_datum(code).ok_or_else(|| not_found("geodetic datum", code))?;
    if record.ensemble.is_some() {
        return Ok(GeodeticDatum::Ensemble(create_datum_ensemble(code)?));
    }
    Ok(GeodeticDatum::ReferenceFrame(Box::new(reference_frame(
        record,
        record.name,
    )?)))
}

fn meridian(text: &str) -> Option<Meridian> {
    // proj's createMeridian reads "<degrees>°E" or "<degrees>°W"
    let (degrees, sign) = match (text.strip_suffix("°E"), text.strip_suffix("°W")) {
        (Some(degrees), _) => (degrees, 1.0),
        (None, Some(degrees)) => (degrees, -1.0),
        (None, None) => return None,
    };
    let degrees: f64 = degrees.parse().ok()?;
    Some(Meridian {
        longitude: Measure::new(sign * degrees, UnitOfMeasure::degree()),
    })
}

fn axis_direction(orientation: &str) -> Option<(AxisDirection, Option<Meridian>)> {
    if let Some(direction) = AxisDirection::from_name(orientation) {
        return Some((direction, None));
    }
    let geocentric = match orientation {
        "Geocentre > equator/0°E" => Some(AxisDirection::GeocentricX),
        "Geocentre > equator/90°E" => Some(AxisDirection::GeocentricY),
        "Geocentre > north pole" => Some(AxisDirection::GeocentricZ),
        _ => None,
    };
    if let Some(direction) = geocentric {
        return Some((direction, None));
    }
    if let Some(rest) = orientation.strip_prefix("North along ") {
        return Some((AxisDirection::North, meridian(rest)));
    }
    if let Some(rest) = orientation.strip_prefix("South along ") {
        return Some((AxisDirection::South, meridian(rest)));
    }
    None
}

pub fn create_coordinate_system(code: u32) -> Result<CoordinateSystem, Error> {
    let record =
        registry::coordinate_system(code).ok_or_else(|| not_found("coordinate system", code))?;
    let axes = record
        .axes
        .iter()
        .map(|axis| {
            let (direction, meridian) = axis_direction(axis.orientation).ok_or_else(|| {
                cannot_build(
                    "coordinate system",
                    format!("EPSG:{code}"),
                    format!("unknown axis direction: {}", axis.orientation),
                )
            })?;
            Ok(CoordinateSystemAxis {
                name: axis.name.to_string(),
                identifiers: Vec::new(),
                abbreviation: axis.abbreviation.to_string(),
                direction,
                unit: create_unit_of_measure(axis.unit_code)?,
                minimum_value: None,
                maximum_value: None,
                range_meaning: None,
                meridian,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    if axes.len() != 2 && axes.len() != 3 {
        return Err(cannot_build(
            "coordinate system",
            format!("EPSG:{code}"),
            format!("it has {} axes, 2 or 3 expected", axes.len()),
        ));
    }
    Ok(match record.kind {
        CoordinateSystemKind::Ellipsoidal => CoordinateSystem::Ellipsoidal(EllipsoidalCs { axes }),
        CoordinateSystemKind::Cartesian => CoordinateSystem::Cartesian(CartesianCs { axes }),
    })
}

// proj pins EPSG:4326 to the usage EPSG gave it before v10.077, extent 1262
const WGS84_CRS: u32 = 4326;
const WGS84_PINNED_EXTENT: u32 = 1262;

fn usage_domains(extent_codes: &[u32]) -> Result<Vec<ObjectDomain>, Error> {
    let mut domains = Vec::with_capacity(extent_codes.len());
    for code in extent_codes {
        let extent = registry::extent(*code).ok_or_else(|| not_found("extent", code))?;
        // proj's createPropertiesSearchUsages drops a usage whose box it cannot build
        let Ok(bounding_box) =
            GeographicBoundingBox::new(extent.west, extent.south, extent.east, extent.north)
        else {
            continue;
        };
        domains.push(ObjectDomain {
            // proj.db's scope text and extent description are left out of the registry
            scope: None,
            domain_of_validity: Some(Extent {
                description: Some(extent.name.to_string()),
                geographic_bounding_box: Some(bounding_box),
                vertical_extent: None,
                temporal_extent: None,
            }),
        });
    }
    Ok(domains)
}

fn crs_name(code: u32) -> Result<&'static str, Error> {
    Ok(registry::metadata(code)
        .ok_or_else(|| not_found("CRS", code))?
        .name)
}

pub fn create_coordinate_reference_system(code: u32) -> Result<Crs, Error> {
    let definition = registry::crs_definition(code).ok_or_else(|| not_found("CRS", code))?;
    let extent_codes: &[u32] = if code == WGS84_CRS {
        &[WGS84_PINNED_EXTENT]
    } else {
        &definition.extent_codes
    };
    let name = crs_name(code)?.to_string();
    let identifiers = epsg_identifiers(code);
    let domains = usage_domains(extent_codes)?;
    let coordinate_system = create_coordinate_system(definition.coordinate_system_code)?;
    let unsupported =
        |reason: &str| cannot_build("CRS", format!("EPSG:{code}"), reason.to_string());
    match (definition.kind, coordinate_system) {
        (
            CrsDefinitionKind::Geodetic {
                geodetic_type: GeodeticCrsType::Geographic2D | GeodeticCrsType::Geographic3D,
                datum_code,
            },
            CoordinateSystem::Ellipsoidal(coordinate_system),
        ) => Ok(Crs::Geographic(GeographicCrs {
            name,
            identifiers,
            remarks: String::new(),
            domains,
            datum: create_geodetic_datum_or_ensemble(datum_code)?,
            coordinate_system,
        })),
        (
            CrsDefinitionKind::Geodetic {
                geodetic_type: GeodeticCrsType::Geocentric,
                datum_code,
            },
            CoordinateSystem::Cartesian(coordinate_system),
        ) => Ok(Crs::Geodetic(GeodeticCrs {
            name,
            identifiers,
            remarks: String::new(),
            domains,
            datum: create_geodetic_datum_or_ensemble(datum_code)?,
            coordinate_system,
        })),
        (
            CrsDefinitionKind::Projected {
                base_crs_code,
                conversion_code,
            },
            CoordinateSystem::Cartesian(coordinate_system),
        ) => {
            let Crs::Geographic(base_crs) = create_coordinate_reference_system(base_crs_code)?
            else {
                return Err(unsupported("its base CRS is not geographic"));
            };
            Ok(Crs::Projected(ProjectedCrs {
                name,
                identifiers,
                remarks: String::new(),
                domains,
                base_crs: Box::new(base_crs),
                conversion: create_conversion(conversion_code)?,
                coordinate_system,
            }))
        }
        _ => Err(unsupported(
            "its CRS type and coordinate system type do not match",
        )),
    }
}

fn parameter_value(
    code: u32,
    name: &str,
    value: f64,
    unit_code: u32,
) -> Result<OperationParameterValue, Error> {
    Ok(OperationParameterValue {
        parameter: OperationParameter {
            name: name.to_string(),
            identifiers: epsg_identifiers(code),
        },
        value: ParameterValue::Measure(measure_in_unit(value, unit_code)?),
    })
}

fn record_parameter_values(
    parameters: &[registry::ParameterValue<'_>],
) -> Result<Vec<OperationParameterValue>, Error> {
    parameters
        .iter()
        .map(|parameter| {
            parameter_value(
                parameter.code,
                parameter.name,
                parameter.value,
                parameter.unit_code,
            )
        })
        .collect()
}

fn epsg_method(method: &registry::OperationMethod<'_>) -> OperationMethod {
    OperationMethod {
        name: method.name.to_string(),
        identifiers: epsg_identifiers(method.code),
    }
}

pub fn create_conversion(code: u32) -> Result<Conversion, Error> {
    let record = registry::conversion(code).ok_or_else(|| not_found("conversion", code))?;
    Ok(Conversion {
        name: record.name.to_string(),
        identifiers: epsg_identifiers(code),
        method: epsg_method(&record.method),
        parameter_values: record_parameter_values(&record.parameters)?,
    })
}

fn sqlite_real_text(value: f64) -> String {
    // sqlite gives a REAL column as its shortest round-trip digits, with ".0" on a whole number
    let magnitude = value.abs();
    if value == 0.0 || (1e-4..1e15).contains(&magnitude) {
        return format!("{value:?}");
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("LowerExp output has an exponent");
    let exponent: i32 = exponent.parse().expect("LowerExp exponent is an integer");
    let point = if mantissa.contains('.') { "" } else { ".0" };
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}{point}e{sign}{:02}", exponent.abs())
}

// proj.db marks an unknown accuracy as 999
const UNKNOWN_ACCURACY: f64 = 999.0;

fn record_accuracies(accuracy: Option<f64>) -> Vec<String> {
    accuracy
        .filter(|accuracy| *accuracy != UNKNOWN_ACCURACY)
        .map(sqlite_real_text)
        .into_iter()
        .collect()
}

fn operation_identifiers(code: OperationCode<'_>) -> Vec<Identifier> {
    vec![Identifier::new(code.authority(), &code.code_text())]
}

struct OperationCommon {
    name: String,
    identifiers: Vec<Identifier>,
    domains: Vec<ObjectDomain>,
    source_crs: Box<Crs>,
    target_crs: Box<Crs>,
    accuracies: Vec<String>,
}

fn operation_common(record: &OperationRecord<'_>) -> Result<OperationCommon, Error> {
    Ok(OperationCommon {
        name: record.name.to_string(),
        identifiers: operation_identifiers(record.code),
        domains: usage_domains(&record.extent_codes)?,
        source_crs: Box::new(create_coordinate_reference_system(record.source_crs_code)?),
        target_crs: Box::new(create_coordinate_reference_system(record.target_crs_code)?),
        accuracies: record_accuracies(record.accuracy),
    })
}

fn build_transformation(
    common: OperationCommon,
    interpolation_crs: Option<Box<Crs>>,
    method: OperationMethod,
    parameter_values: Vec<OperationParameterValue>,
) -> Transformation {
    Transformation {
        name: common.name,
        identifiers: common.identifiers,
        remarks: String::new(),
        domains: common.domains,
        source_crs: common.source_crs,
        target_crs: common.target_crs,
        interpolation_crs,
        method,
        parameter_values,
        accuracies: common.accuracies,
    }
}

fn helmert_transformation(
    record: &OperationRecord<'_>,
    helmert: &HelmertParameters<'_>,
) -> Result<Transformation, Error> {
    let cannot = |reason: &str| cannot_build("transformation", record.code, reason.to_string());
    let measure = |code: i32,
                   value: f64,
                   unit_code: u32|
     -> Result<OperationParameterValue, Error> {
        Ok(OperationParameterValue {
            parameter: epsg_parameter(code),
            value: ParameterValue::Measure(Measure::new(value, create_unit_of_measure(unit_code)?)),
        })
    };
    let vector = |codes: [i32; 3], values: &registry::MeasuredVector| {
        codes
            .iter()
            .zip(values.values)
            .map(|(code, value)| measure(*code, value, values.unit_code))
            .collect::<Result<Vec<_>, Error>>()
    };
    let mut parameter_values = vector(
        [
            PARAMETER_X_AXIS_TRANSLATION,
            PARAMETER_Y_AXIS_TRANSLATION,
            PARAMETER_Z_AXIS_TRANSLATION,
        ],
        &helmert.translation,
    )?;
    if let Some(rotation) = &helmert.rotation {
        parameter_values.extend(vector(
            [
                PARAMETER_X_AXIS_ROTATION,
                PARAMETER_Y_AXIS_ROTATION,
                PARAMETER_Z_AXIS_ROTATION,
            ],
            rotation,
        )?);
        let scale_difference = helmert
            .scale_difference
            .ok_or_else(|| cannot("it has rotations and no scale difference"))?;
        parameter_values.push(measure(
            PARAMETER_SCALE_DIFFERENCE,
            scale_difference.value,
            scale_difference.unit_code,
        )?);
    }
    if let Some(rate_translation) = &helmert.rate_translation {
        let (Some(rate_rotation), Some(rate_scale_difference), Some(epoch)) = (
            &helmert.rate_rotation,
            helmert.rate_scale_difference,
            helmert.epoch,
        ) else {
            return Err(cannot(
                "it has translation rates without rotation and scale rates and an epoch",
            ));
        };
        parameter_values.extend(vector(
            [
                PARAMETER_RATE_X_AXIS_TRANSLATION,
                PARAMETER_RATE_Y_AXIS_TRANSLATION,
                PARAMETER_RATE_Z_AXIS_TRANSLATION,
            ],
            rate_translation,
        )?);
        parameter_values.extend(vector(
            [
                PARAMETER_RATE_X_AXIS_ROTATION,
                PARAMETER_RATE_Y_AXIS_ROTATION,
                PARAMETER_RATE_Z_AXIS_ROTATION,
            ],
            rate_rotation,
        )?);
        parameter_values.push(measure(
            PARAMETER_RATE_SCALE_DIFFERENCE,
            rate_scale_difference.value,
            rate_scale_difference.unit_code,
        )?);
        parameter_values.push(measure(
            PARAMETER_REFERENCE_EPOCH,
            epoch.value,
            epoch.unit_code,
        )?);
    } else if let Some(epoch) = helmert.epoch {
        parameter_values.push(measure(
            PARAMETER_TRANSFORMATION_REFERENCE_EPOCH,
            epoch.value,
            epoch.unit_code,
        )?);
    } else if let Some(pivot) = &helmert.pivot {
        parameter_values.extend(vector(
            [
                PARAMETER_ORDINATE_1_EVAL_POINT,
                PARAMETER_ORDINATE_2_EVAL_POINT,
                PARAMETER_ORDINATE_3_EVAL_POINT,
            ],
            pivot,
        )?);
    }
    Ok(build_transformation(
        operation_common(record)?,
        None,
        epsg_method(&helmert.method),
        parameter_values,
    ))
}

fn grid_parameter_values(
    parameters: &TransformationParameters<'_>,
) -> Vec<OperationParameterValue> {
    parameters
        .grids
        .iter()
        .map(|grid| OperationParameterValue {
            parameter: OperationParameter {
                name: grid.parameter_name.to_string(),
                identifiers: epsg_identifiers(grid.parameter_code),
            },
            value: ParameterValue::Filename(grid.file_name.to_string()),
        })
        .collect()
}

fn create_interpolation_crs(
    parameters: &TransformationParameters<'_>,
) -> Result<Option<Box<Crs>>, Error> {
    parameters
        .interpolation_crs_code
        .map(|code| create_coordinate_reference_system(code).map(Box::new))
        .transpose()
}

fn grid_transformation(
    record: &OperationRecord<'_>,
    parameters: &TransformationParameters<'_>,
) -> Result<KnownInverse<CoordinateOperation>, Error> {
    let mut parameter_values = grid_parameter_values(parameters);
    parameter_values.extend(record_parameter_values(&parameters.parameters)?);
    let method = epsg_method(&parameters.method);
    let interpolation_crs = create_interpolation_crs(parameters)?;
    let common = operation_common(record)?;
    // proj tells a point motion operation by its method name
    if parameters.method.name.starts_with("Point motion") {
        if !crs_more_or_less_equivalent(&common.source_crs, &common.target_crs) {
            return Err(cannot_build(
                "point motion operation",
                record.code,
                "source_crs and target_crs should be the same for a PointMotionOperation"
                    .to_string(),
            ));
        }
        let operation = PointMotionOperation {
            name: common.name,
            identifiers: common.identifiers,
            remarks: String::new(),
            domains: common.domains,
            crs: common.source_crs,
            method,
            parameter_values,
            accuracies: common.accuracies,
        };
        return Ok(KnownInverse::alone(CoordinateOperation::PointMotion(
            substitute_point_motion_grid_names(operation),
        )));
    }
    let transformation = build_transformation(common, interpolation_crs, method, parameter_values);
    Ok(substitute_proj_alternative_grid_names(transformation)?
        .map(CoordinateOperation::Transformation))
}

// methods proj turns into a Conversion even when proj.db files them as transformations
const CONVERSION_METHODS_FILED_AS_TRANSFORMATIONS: [i32; 5] = [9843, 9844, 1069, 1104, 1068];

fn other_transformation(
    record: &OperationRecord<'_>,
    parameters: &TransformationParameters<'_>,
) -> Result<KnownInverse<Transformation>, Error> {
    if CONVERSION_METHODS_FILED_AS_TRANSFORMATIONS.contains(&(parameters.method.code as i32)) {
        return Err(cannot_build(
            "transformation",
            record.code,
            format!(
                "method EPSG:{} makes it a conversion, which the model does not hold here",
                parameters.method.code
            ),
        ));
    }
    let mut parameter_values = record_parameter_values(&parameters.parameters)?;
    parameter_values.extend(grid_parameter_values(parameters));
    let transformation = build_transformation(
        operation_common(record)?,
        create_interpolation_crs(parameters)?,
        epsg_method(&parameters.method),
        parameter_values,
    );
    substitute_proj_alternative_grid_names(transformation)
}

struct KnownInverse<T> {
    operation: T,
    // proj's InverseTransformation keeps the transformation it inverts
    inverse: Option<Box<T>>,
}

impl<T> KnownInverse<T> {
    fn alone(operation: T) -> Self {
        Self {
            operation,
            inverse: None,
        }
    }

    fn map<U>(self, convert: impl Fn(T) -> U) -> KnownInverse<U> {
        KnownInverse {
            operation: convert(self.operation),
            inverse: self.inverse.map(|inverse| Box::new(convert(*inverse))),
        }
    }
}

fn create_operation_with_known_inverse(
    code: OperationCode<'_>,
) -> Result<KnownInverse<CoordinateOperation>, Error> {
    let record =
        registry::coordinate_operation(code).ok_or_else(|| Error::NoSuchAuthorityCode {
            kind: "coordinate operation",
            code: code.to_string(),
        })?;
    match &record.kind {
        OperationKind::HelmertTransformation(helmert) => Ok(KnownInverse::alone(
            CoordinateOperation::Transformation(helmert_transformation(record, helmert)?),
        )),
        OperationKind::GridTransformation(parameters) => grid_transformation(record, parameters),
        OperationKind::OtherTransformation(parameters) => {
            Ok(other_transformation(record, parameters)?.map(CoordinateOperation::Transformation))
        }
        OperationKind::ConcatenatedOperation(steps) => Ok(KnownInverse::alone(
            CoordinateOperation::Concatenated(concatenated_operation(record, steps)?),
        )),
    }
}

pub fn create_coordinate_operation(code: OperationCode<'_>) -> Result<CoordinateOperation, Error> {
    Ok(create_operation_with_known_inverse(code)?.operation)
}

const METHOD_NTV1: i32 = 9614;
const METHOD_NTV2: i32 = 9615;
const METHOD_NADCON: i32 = 9613;
const METHOD_NADCON5_2D: i32 = 1074;
const METHOD_NADCON5_3D: i32 = 1075;
const METHOD_GEOCENTRIC_TRANSLATIONS_BY_GRID_IGN: i32 = 1087;
const METHOD_GEOGRAPHIC3D_OFFSET_BY_VELOCITY_GRID_NTV2_VEL: i32 = 1114;
const METHOD_NEW_ZEALAND_DEFORMATION_MODEL: i32 = 1079;
const METHOD_VERTICAL_OFFSET_BY_TIN_INTERPOLATION_JSON: i32 = 1137;
const METHOD_CARTESIAN_GRID_OFFSETS_BY_TIN_INTERPOLATION_JSON: i32 = 1138;
const METHOD_GEOGRAPHIC2D_OFFSETS_BY_TIN_INTERPOLATION_JSON: i32 = 1145;
const METHOD_GEOCENTRIC_TRANSLATIONS_BY_GRID_GTG_AND_NEU_VELOCITIES_GTG: i32 = 1142;
const METHOD_POSITION_VECTOR_GEOCENTRIC_AND_NEU_VELOCITIES_GTG: i32 = 1143;
const METHOD_GEOCENTRIC_TRANSLATIONS_USING_NEU_VELOCITY_GRID_GTG: i32 = 1144;
const METHOD_POINT_MOTION_BY_GRID_CANADA_NTV2_VEL: i32 = 1070;
const METHOD_POINT_MOTION_GEOG3D_DOMAIN_USING_NEU_VELOCITY_GRID_NTV2_VEL: i32 = 1141;
const METHOD_POINT_MOTION_GEOCEN_DOMAIN_USING_NEU_VELOCITY_GRID_GRAVSOFT: i32 = 1139;

const HORIZONTAL_SHIFT_GTIFF: &str = "HORIZONTAL_SHIFT_GTIFF";
const GENERAL_SHIFT_GTIFF: &str = "GENERAL_SHIFT_GTIFF";
const CTABLE2: &str = "CTABLE2";
const LATITUDE_LONGITUDE_ELLIPSOIDAL_HEIGHT_DIFFERENCE_FILE: &str =
    "Latitude, longitude and ellipsoidal height difference file";

fn file_parameter(parameter_values: &[OperationParameterValue], code: i32) -> Option<&str> {
    parameter_values
        .iter()
        .find(|parameter_value| parameter_value.parameter.epsg_code() == code)
        .and_then(|parameter_value| match &parameter_value.value {
            ParameterValue::Filename(file_name) => Some(file_name.as_str()),
            ParameterValue::Measure(_) | ParameterValue::Integer(_) => None,
        })
}

struct GridAlternative {
    proj_file_name: &'static str,
    format: &'static str,
    inverse_direction: bool,
}

fn look_for_grid_alternative(official_name: &str) -> Option<GridAlternative> {
    // proj's DatabaseContext::lookForGridAlternative skips rows with no PROJ file name
    let record = registry::grid_alternative(official_name)?;
    Some(GridAlternative {
        proj_file_name: record.proj_grid_name?,
        format: record.format,
        inverse_direction: record.inverse_direction,
    })
}

fn filename_value(parameter: OperationParameter, file_name: &str) -> OperationParameterValue {
    OperationParameterValue {
        parameter,
        value: ParameterValue::Filename(file_name.to_string()),
    }
}

fn derived_transformation(
    forward: &Transformation,
    method: OperationMethod,
    parameter_values: Vec<OperationParameterValue>,
) -> Transformation {
    // proj's createSimilarPropertiesOperation and createSimilarPropertiesMethod
    Transformation {
        name: forward.name.clone(),
        identifiers: modified_identifiers(&forward.identifiers, false, true),
        remarks: forward.remarks.clone(),
        domains: forward.domains.clone(),
        source_crs: forward.source_crs.clone(),
        target_crs: forward.target_crs.clone(),
        interpolation_crs: forward.interpolation_crs.clone(),
        method,
        parameter_values,
        accuracies: forward.accuracies.clone(),
    }
}

fn derived_reversed_file(
    forward: &Transformation,
    method: OperationMethod,
    parameter_values: Vec<OperationParameterValue>,
    interpolation_crs: Option<Box<Crs>>,
) -> Result<KnownInverse<Transformation>, Error> {
    // proj builds a file proj.db marks as reversed into the inverse operation, then inverts it
    let reversed = Transformation {
        name: inverse_transformation_name(&forward.name, &forward.source_crs, &forward.target_crs),
        identifiers: modified_identifiers(&forward.identifiers, true, true),
        remarks: forward.remarks.clone(),
        domains: forward.domains.clone(),
        source_crs: forward.target_crs.clone(),
        target_crs: forward.source_crs.clone(),
        interpolation_crs,
        method,
        parameter_values,
        accuracies: forward.accuracies.clone(),
    };
    Ok(KnownInverse {
        operation: reversed.inverse()?,
        inverse: Some(Box::new(reversed)),
    })
}

fn unsupported_inverse_grid(transformation: &Transformation, file_name: &str) -> Error {
    Error::Factory {
        kind: "transformation",
        code: transformation.name.clone(),
        reason: format!("Inverse direction for {file_name} not supported"),
    }
}

fn substitute_horizontal_grid(
    transformation: &Transformation,
    method_code: i32,
    grid_name: &str,
    alternative: &GridAlternative,
) -> Result<Option<KnownInverse<Transformation>>, Error> {
    if grid_name == alternative.proj_file_name {
        if alternative.inverse_direction {
            return Err(unsupported_inverse_grid(transformation, grid_name));
        }
        return Ok(Some(KnownInverse::alone(transformation.clone())));
    }
    let file_value = |parameter| vec![filename_value(parameter, alternative.proj_file_name)];
    let method_by_code = |code: i32, name: &str| OperationMethod {
        name: name.to_string(),
        identifiers: epsg_identifiers(code as u32),
    };
    let (method, parameter_values, interpolation_crs) = match alternative.format {
        "GTiff" => {
            let parameter = if method_code == METHOD_NADCON5_3D {
                OperationParameter {
                    name: LATITUDE_LONGITUDE_ELLIPSOIDAL_HEIGHT_DIFFERENCE_FILE.to_string(),
                    identifiers: Vec::new(),
                }
            } else {
                epsg_parameter(PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE)
            };
            let method_name =
                if method_code == METHOD_NADCON5_2D || method_code == METHOD_NADCON5_3D {
                    GENERAL_SHIFT_GTIFF
                } else {
                    HORIZONTAL_SHIFT_GTIFF
                };
            let method = OperationMethod {
                name: method_name.to_string(),
                identifiers: Vec::new(),
            };
            (
                method,
                file_value(parameter),
                transformation.interpolation_crs.clone(),
            )
        }
        // proj's createNTv1 and createNTv2 take no interpolation crs
        "NTv1" => (
            method_by_code(METHOD_NTV1, "NTv1"),
            file_value(epsg_parameter(PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE)),
            None,
        ),
        "NTv2" => (
            method_by_code(METHOD_NTV2, "NTv2"),
            file_value(epsg_parameter(PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE)),
            None,
        ),
        "CTable2" => (
            OperationMethod {
                name: CTABLE2.to_string(),
                identifiers: Vec::new(),
            },
            file_value(epsg_parameter(PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE)),
            transformation.interpolation_crs.clone(),
        ),
        _ => return Ok(None),
    };
    if alternative.inverse_direction {
        return derived_reversed_file(transformation, method, parameter_values, interpolation_crs)
            .map(Some);
    }
    let mut derived = derived_transformation(transformation, method, parameter_values);
    derived.interpolation_crs = interpolation_crs;
    Ok(Some(KnownInverse::alone(derived)))
}

fn single_file_parameter_substitution(
    transformation: &Transformation,
    parameter_code: i32,
    allow_inverse_direction: bool,
) -> Result<Option<KnownInverse<Transformation>>, Error> {
    let Some(file_name) = file_parameter(&transformation.parameter_values, parameter_code) else {
        return Ok(None);
    };
    let Some(alternative) = look_for_grid_alternative(file_name) else {
        return Ok(None);
    };
    if alternative.inverse_direction
        && (!allow_inverse_direction || file_name == alternative.proj_file_name)
    {
        return Err(unsupported_inverse_grid(transformation, file_name));
    }
    if file_name == alternative.proj_file_name {
        return Ok(Some(KnownInverse::alone(transformation.clone())));
    }
    let method = OperationMethod {
        name: transformation.method.name.clone(),
        identifiers: transformation.method.identifiers.clone(),
    };
    let parameter_values = vec![filename_value(
        epsg_parameter(parameter_code),
        alternative.proj_file_name,
    )];
    if alternative.inverse_direction {
        return derived_reversed_file(
            transformation,
            method,
            parameter_values,
            transformation.interpolation_crs.clone(),
        )
        .map(Some);
    }
    Ok(Some(KnownInverse::alone(derived_transformation(
        transformation,
        method,
        parameter_values,
    ))))
}

fn replaced_file_parameters(
    transformation: &Transformation,
    replacements: &[(i32, &str)],
) -> Transformation {
    let parameter_values = transformation
        .parameter_values
        .iter()
        .map(|parameter_value| {
            let code = parameter_value.parameter.epsg_code();
            match replacements
                .iter()
                .find(|(replaced_code, _)| *replaced_code == code)
            {
                Some((_, file_name)) => {
                    filename_value(parameter_value.parameter.clone(), file_name)
                }
                None => parameter_value.clone(),
            }
        })
        .collect();
    derived_transformation(
        transformation,
        transformation.method.clone(),
        parameter_values,
    )
}

fn substitute_proj_alternative_grid_names(
    transformation: Transformation,
) -> Result<KnownInverse<Transformation>, Error> {
    let method_code = epsg_code(&transformation.method.identifiers);
    let parameter_values = &transformation.parameter_values;
    let horizontal_grid_name = match method_code {
        METHOD_NTV1 | METHOD_NTV2 => file_parameter(
            parameter_values,
            PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE,
        ),
        METHOD_NADCON | METHOD_NADCON5_2D | METHOD_NADCON5_3D => {
            file_parameter(parameter_values, PARAMETER_LONGITUDE_DIFFERENCE_FILE).and(
                file_parameter(parameter_values, PARAMETER_LATITUDE_DIFFERENCE_FILE),
            )
        }
        _ => None,
    };
    if let Some(grid_name) = horizontal_grid_name {
        if let Some(alternative) = look_for_grid_alternative(grid_name) {
            let substituted =
                substitute_horizontal_grid(&transformation, method_code, grid_name, &alternative)?;
            if let Some(substituted) = substituted {
                return Ok(substituted);
            }
        }
    }

    let single_file_cases = [
        (
            METHOD_GEOCENTRIC_TRANSLATIONS_BY_GRID_IGN,
            PARAMETER_GEOCENTRIC_TRANSLATION_FILE,
            false,
        ),
        (
            METHOD_GEOGRAPHIC3D_OFFSET_BY_VELOCITY_GRID_NTV2_VEL,
            PARAMETER_POINT_MOTION_VELOCITY_GRID_FILE,
            false,
        ),
        (
            METHOD_NEW_ZEALAND_DEFORMATION_MODEL,
            PARAMETER_POINT_MOTION_VELOCITY_GRID_FILE,
            true,
        ),
        (
            METHOD_CARTESIAN_GRID_OFFSETS_BY_TIN_INTERPOLATION_JSON,
            PARAMETER_TIN_OFFSET_FILE,
            true,
        ),
        (
            METHOD_VERTICAL_OFFSET_BY_TIN_INTERPOLATION_JSON,
            PARAMETER_TIN_OFFSET_FILE,
            true,
        ),
        (
            METHOD_GEOGRAPHIC2D_OFFSETS_BY_TIN_INTERPOLATION_JSON,
            PARAMETER_TIN_OFFSET_FILE,
            true,
        ),
    ];
    if let Some((_, parameter_code, allow_inverse_direction)) = single_file_cases
        .iter()
        .find(|(case_method, _, _)| *case_method == method_code)
    {
        if let Some(substituted) = single_file_parameter_substitution(
            &transformation,
            *parameter_code,
            *allow_inverse_direction,
        )? {
            return Ok(substituted);
        }
    }

    if method_code == METHOD_POSITION_VECTOR_GEOCENTRIC_AND_NEU_VELOCITIES_GTG
        || method_code == METHOD_GEOCENTRIC_TRANSLATIONS_USING_NEU_VELOCITY_GRID_GTG
    {
        let velocity_file =
            file_parameter(parameter_values, PARAMETER_POINT_MOTION_VELOCITY_GRID_FILE);
        if let Some((file_name, alternative)) = velocity_file
            .and_then(|file_name| Some((file_name, look_for_grid_alternative(file_name)?)))
        {
            if alternative.inverse_direction {
                return Err(unsupported_inverse_grid(&transformation, file_name));
            }
            if file_name == alternative.proj_file_name {
                return Ok(KnownInverse::alone(transformation));
            }
            return Ok(KnownInverse::alone(replaced_file_parameters(
                &transformation,
                &[(
                    PARAMETER_POINT_MOTION_VELOCITY_GRID_FILE,
                    alternative.proj_file_name,
                )],
            )));
        }
    }

    if method_code == METHOD_GEOCENTRIC_TRANSLATIONS_BY_GRID_GTG_AND_NEU_VELOCITIES_GTG {
        let translation_file =
            file_parameter(parameter_values, PARAMETER_GEOCENTRIC_TRANSLATION_FILE);
        let velocity_file =
            file_parameter(parameter_values, PARAMETER_POINT_MOTION_VELOCITY_GRID_FILE);
        if let (Some(translation_file), Some(velocity_file)) = (translation_file, velocity_file) {
            if let (Some(translation_alternative), Some(velocity_alternative)) = (
                look_for_grid_alternative(translation_file),
                look_for_grid_alternative(velocity_file),
            ) {
                // proj checks the direction its second lookup left behind
                if velocity_alternative.inverse_direction {
                    return Err(unsupported_inverse_grid(&transformation, velocity_file));
                }
                if translation_file == translation_alternative.proj_file_name
                    && velocity_file == velocity_alternative.proj_file_name
                {
                    return Ok(KnownInverse::alone(transformation));
                }
                return Ok(KnownInverse::alone(replaced_file_parameters(
                    &transformation,
                    &[
                        (
                            PARAMETER_GEOCENTRIC_TRANSLATION_FILE,
                            translation_alternative.proj_file_name,
                        ),
                        (
                            PARAMETER_POINT_MOTION_VELOCITY_GRID_FILE,
                            velocity_alternative.proj_file_name,
                        ),
                    ],
                )));
            }
        }
    }
    Ok(KnownInverse::alone(transformation))
}

fn substitute_point_motion_grid_names(operation: PointMotionOperation) -> PointMotionOperation {
    // proj's PointMotionOperation::substitutePROJAlternativeGridNames
    let method_code = epsg_code(&operation.method.identifiers);
    let parameter_code =
        if method_code == METHOD_POINT_MOTION_GEOCEN_DOMAIN_USING_NEU_VELOCITY_GRID_GRAVSOFT {
            PARAMETER_POINT_MOTION_VELOCITY_NORTH_GRID_FILE
        } else {
            PARAMETER_POINT_MOTION_VELOCITY_GRID_FILE
        };
    let substitutes = matches!(
        method_code,
        METHOD_POINT_MOTION_BY_GRID_CANADA_NTV2_VEL
            | METHOD_POINT_MOTION_GEOG3D_DOMAIN_USING_NEU_VELOCITY_GRID_NTV2_VEL
            | METHOD_POINT_MOTION_GEOCEN_DOMAIN_USING_NEU_VELOCITY_GRID_GRAVSOFT
    );
    let file_name =
        file_parameter(&operation.parameter_values, parameter_code).filter(|_| substitutes);
    let Some((file_name, alternative)) =
        file_name.and_then(|file_name| Some((file_name, look_for_grid_alternative(file_name)?)))
    else {
        return operation;
    };
    if file_name == alternative.proj_file_name {
        return operation;
    }
    PointMotionOperation {
        name: operation.name.clone(),
        identifiers: modified_identifiers(&operation.identifiers, false, true),
        remarks: operation.remarks.clone(),
        domains: operation.domains.clone(),
        crs: operation.crs.clone(),
        method: operation.method.clone(),
        parameter_values: vec![filename_value(
            epsg_parameter(parameter_code),
            alternative.proj_file_name,
        )],
        accuracies: operation.accuracies.clone(),
    }
}

enum ChainStep {
    Conversion {
        conversion: Conversion,
        crs: Option<(Box<Crs>, Box<Crs>)>,
    },
    Operation(KnownInverse<CoordinateOperation>),
}

impl ChainStep {
    fn source_crs(&self) -> Option<&Crs> {
        match self {
            ChainStep::Conversion { crs, .. } => crs.as_ref().map(|(source, _)| source.as_ref()),
            ChainStep::Operation(step) => Some(step.operation.source_crs()),
        }
    }

    fn target_crs(&self) -> Option<&Crs> {
        match self {
            ChainStep::Conversion { crs, .. } => crs.as_ref().map(|(_, target)| target.as_ref()),
            ChainStep::Operation(step) => Some(step.operation.target_crs()),
        }
    }

    fn inverse(self) -> Result<ChainStep, Error> {
        match self {
            ChainStep::Conversion { conversion, crs } => Ok(ChainStep::Conversion {
                conversion: conversion.inverse(),
                crs: crs.map(|(source, target)| (target, source)),
            }),
            // proj's InverseTransformation::inverse gives back the transformation it inverts
            ChainStep::Operation(KnownInverse {
                operation,
                inverse: Some(inverse),
            }) => Ok(ChainStep::Operation(KnownInverse {
                operation: *inverse,
                inverse: Some(Box::new(operation)),
            })),
            ChainStep::Operation(KnownInverse {
                operation: CoordinateOperation::Transformation(transformation),
                inverse: None,
            }) => Ok(ChainStep::Operation(KnownInverse {
                operation: CoordinateOperation::Transformation(transformation.inverse()?),
                inverse: Some(Box::new(CoordinateOperation::Transformation(
                    transformation,
                ))),
            })),
            ChainStep::Operation(step) => Err(Error::Factory {
                kind: "concatenated operation step",
                code: step.operation.name().to_string(),
                reason: "only transformations and conversions are inverted".to_string(),
            }),
        }
    }
}

fn crs_more_or_less_equivalent(crs: &Crs, other: &Crs) -> bool {
    // proj's areCRSMoreOrLessEquivalent, without its compound CRS case
    let identifiers = crs_identifiers(crs);
    let other_identifiers = crs_identifiers(other);
    if let ([identifier], [other_identifier]) = (identifiers, other_identifiers) {
        if identifier.code == other_identifier.code
            && identifier.code_space == other_identifier.code_space
        {
            return true;
        }
    }
    crs.is_equivalent_to(other, Criterion::Equivalent)
}

fn crs_identifiers(crs: &Crs) -> &[Identifier] {
    match crs {
        Crs::Geographic(crs) => &crs.identifiers,
        Crs::Geodetic(crs) => &crs.identifiers,
        Crs::Projected(crs) => &crs.identifiers,
        Crs::Bound(crs) => &crs.identifiers,
    }
}

fn geographic_and_geocentric(crs: &Crs, other: &Crs) -> bool {
    matches!(
        (crs, other),
        (Crs::Geographic(_), Crs::Geodetic(_)) | (Crs::Geodetic(_), Crs::Geographic(_))
    )
}

const METHOD_GEOGRAPHIC_GEOCENTRIC: u32 = 9602;

fn geographic_geocentric_conversion(source: &Crs, target: &Crs) -> ChainStep {
    ChainStep::Conversion {
        conversion: Conversion {
            name: build_operation_name("Conversion", source, target),
            identifiers: Vec::new(),
            method: OperationMethod {
                name: "Geographic/geocentric conversions".to_string(),
                identifiers: epsg_identifiers(METHOD_GEOGRAPHIC_GEOCENTRIC),
            },
            parameter_values: Vec::new(),
        },
        crs: Some((Box::new(source.clone()), Box::new(target.clone()))),
    }
}

fn chain_error(code: OperationCode<'_>, reason: &str) -> Error {
    cannot_build("concatenated operation", code, reason.to_string())
}

fn fix_first_and_last_direction(
    chain: &mut Vec<ChainStep>,
    source: &Crs,
    target: &Crs,
) -> Result<(), Error> {
    if chain.len() < 2 {
        return Ok(());
    }
    let reverse_first = matches!(&chain[0], ChainStep::Operation(step)
        if !crs_more_or_less_equivalent(step.operation.source_crs(), source)
            && crs_more_or_less_equivalent(step.operation.target_crs(), source));
    if reverse_first {
        invert_in_place(chain, 0)?;
    }
    let last = chain.len() - 1;
    let reverse_last = matches!(&chain[last], ChainStep::Operation(step)
        if !crs_more_or_less_equivalent(step.operation.target_crs(), target)
            && crs_more_or_less_equivalent(step.operation.source_crs(), target));
    if reverse_last {
        invert_in_place(chain, last)?;
    }
    Ok(())
}

fn invert_in_place(chain: &mut Vec<ChainStep>, index: usize) -> Result<(), Error> {
    let step = chain.remove(index);
    chain.insert(index, step.inverse()?);
    Ok(())
}

fn conversion_matches_projected(conversion: &Conversion, crs: &Crs) -> bool {
    match crs {
        Crs::Projected(projected) => conversion.is_equivalent_to(&projected.conversion, false),
        Crs::Geographic(_) | Crs::Geodetic(_) | Crs::Bound(_) => false,
    }
}

fn fix_steps(
    code: OperationCode<'_>,
    chain: &mut Vec<ChainStep>,
    source: &Crs,
    target: &Crs,
    fix_direction_allowed: bool,
) -> Result<(), Error> {
    // proj's ConcatenatedOperation::fixSteps for chains between geographic, geocentric and projected crs
    if fix_direction_allowed {
        fix_first_and_last_direction(chain, source, target)?;
    }
    let derived_endpoint = |crs: &Crs| matches!(crs, Crs::Projected(_));
    let mut index = 0;
    while index < chain.len() {
        let last = chain.len() - 1;
        let previous_target = (index > 0)
            .then(|| chain[index - 1].target_crs().cloned())
            .flatten();
        let next_source = (index < last)
            .then(|| chain[index + 1].source_crs().cloned())
            .flatten();
        match &chain[index] {
            ChainStep::Conversion { crs: None, .. } if index == 0 => {
                if derived_endpoint(source) {
                    return Err(chain_error(
                        code,
                        "a first conversion step from a projected CRS is not supported",
                    ));
                }
                if let (Some(next_source), ChainStep::Conversion { crs, .. }) =
                    (next_source, &mut chain[index])
                {
                    *crs = Some((Box::new(source.clone()), Box::new(next_source)));
                }
            }
            ChainStep::Conversion {
                crs: None,
                conversion,
            } if index == last => {
                if derived_endpoint(target) {
                    return Err(chain_error(
                        code,
                        "a last conversion step to a projected CRS is not supported",
                    ));
                }
                if let Some(previous_target) = previous_target {
                    let reverses_projection = fix_direction_allowed
                        && conversion_matches_projected(conversion, &previous_target);
                    if reverses_projection {
                        invert_in_place(chain, index)?;
                    }
                    if let ChainStep::Conversion { crs, .. } = &mut chain[index] {
                        *crs = Some((Box::new(previous_target), Box::new(target.clone())));
                    }
                }
            }
            ChainStep::Conversion { .. } if index > 0 && index < last => {
                if let (Some(previous_target), Some(next_source)) = (previous_target, next_source) {
                    // PROJ issue 2817: a conversion from a projected to a geographic crs runs inverted
                    let inverted = fix_direction_allowed
                        && matches!(previous_target, Crs::Projected(_))
                        && matches!(next_source, Crs::Geographic(_));
                    if inverted {
                        invert_in_place(chain, index)?;
                    }
                    if let ChainStep::Conversion { crs, .. } = &mut chain[index] {
                        *crs = Some((Box::new(previous_target), Box::new(next_source)));
                    }
                }
            }
            ChainStep::Conversion { .. } => {}
            ChainStep::Operation(KnownInverse { operation, .. }) => {
                let previous_target = if index == 0 {
                    source.clone()
                } else {
                    previous_target.ok_or_else(|| {
                        chain_error(
                            code,
                            &format!("Cannot determine targetCRS of operation at step {index}"),
                        )
                    })?
                };
                let step_source = operation.source_crs();
                let step_target = operation.target_crs();
                let chained = crs_more_or_less_equivalent(step_source, &previous_target);
                if !chained
                    && fix_direction_allowed
                    && crs_more_or_less_equivalent(step_target, &previous_target)
                {
                    invert_in_place(chain, index)?;
                } else if !chained
                    && step_source.name() == previous_target.name()
                    && geographic_and_geocentric(step_source, &previous_target)
                {
                    // EPSG:9103 chains NAD83(2011) geographic 2D with NAD83(2011) geocentric
                    let conversion =
                        geographic_geocentric_conversion(&previous_target, step_source);
                    chain.insert(index, conversion);
                } else if !chained
                    && step_target.name() == previous_target.name()
                    && geographic_and_geocentric(step_target, &previous_target)
                {
                    let conversion =
                        geographic_geocentric_conversion(&previous_target, step_target);
                    chain.insert(index, conversion);
                }
            }
        }
        index += 1;
    }

    let first_source = chain.first().and_then(ChainStep::source_crs);
    if first_source.is_some_and(|first_source| !crs_more_or_less_equivalent(first_source, source)) {
        return Err(chain_error(
            code,
            "The source CRS of the first step of concatenated operation is not the same as the source CRS of the concatenated operation itself",
        ));
    }
    let last_target = chain.last().and_then(ChainStep::target_crs).cloned();
    if let Some(last_target) = last_target {
        if !crs_more_or_less_equivalent(&last_target, target) {
            if last_target.name() == target.name()
                && geographic_and_geocentric(&last_target, target)
            {
                chain.push(geographic_geocentric_conversion(&last_target, target));
            } else {
                return Err(chain_error(
                    code,
                    "The target CRS of the last step of concatenated operation is not the same as the target CRS of the concatenated operation itself",
                ));
            }
        }
    }
    Ok(())
}

fn chain_step(step: &OperationStep<'_>) -> Result<ChainStep, Error> {
    let chain_step = match step.reference {
        StepReference::Conversion(code) => ChainStep::Conversion {
            conversion: create_conversion(code)?,
            crs: None,
        },
        StepReference::Operation(code) => {
            let built = create_operation_with_known_inverse(code)?;
            if matches!(built.operation, CoordinateOperation::Concatenated(_)) {
                return Err(cannot_build(
                    "concatenated operation step",
                    code,
                    "a step should not be a concatenated operation".to_string(),
                ));
            }
            ChainStep::Operation(built)
        }
    };
    match step.direction {
        Some(StepDirection::Reverse) => chain_step.inverse(),
        Some(StepDirection::Forward) | None => Ok(chain_step),
    }
}

fn summed_accuracy(steps: &[CoordinateOperation]) -> Vec<String> {
    // proj sums the step accuracies of a concatenated operation proj.db gives none
    let mut total: Option<f64> = None;
    for step in steps {
        match (step.accuracies(), step) {
            ([accuracy], _) => {
                let Ok(accuracy) = accuracy.parse::<f64>() else {
                    return Vec::new();
                };
                total = Some(total.unwrap_or(0.0) + accuracy);
            }
            (_, CoordinateOperation::Conversion { .. }) => {
                total.get_or_insert(0.0);
            }
            _ => return Vec::new(),
        }
    }
    total.map(proj_to_string).into_iter().collect()
}

fn proj_to_string(value: f64) -> String {
    // proj's toString(double) prints 15 significant digits unless that shows a run of nines
    let text = format_general(value, 15);
    if text.contains("9999999999") {
        return format_general(value, 14);
    }
    text
}

fn concatenated_operation(
    record: &OperationRecord<'_>,
    record_steps: &[OperationStep<'_>],
) -> Result<ConcatenatedOperation, Error> {
    let code = record.code;
    let explicit_directions = record_steps
        .iter()
        .filter(|step| step.direction.is_some())
        .count();
    if explicit_directions > 0 && explicit_directions != record_steps.len() {
        return Err(chain_error(code, "not all steps have a defined direction"));
    }
    let mut chain = record_steps
        .iter()
        .map(chain_step)
        .collect::<Result<Vec<_>, Error>>()?;
    let source_crs = create_coordinate_reference_system(record.source_crs_code)?;
    let target_crs = create_coordinate_reference_system(record.target_crs_code)?;
    fix_steps(
        code,
        &mut chain,
        &source_crs,
        &target_crs,
        explicit_directions == 0,
    )?;

    let steps = chain
        .into_iter()
        .map(|step| match step {
            ChainStep::Operation(step) => Ok(step.operation),
            ChainStep::Conversion {
                conversion,
                crs: Some((source_crs, target_crs)),
            } => Ok(CoordinateOperation::Conversion {
                conversion,
                source_crs,
                target_crs,
            }),
            ChainStep::Conversion { crs: None, .. } => Err(chain_error(
                code,
                "At least one of the operation lacks a source and/or target CRS",
            )),
        })
        .collect::<Result<Vec<_>, Error>>()?;
    if steps.len() < 2 {
        return Err(chain_error(
            code,
            "ConcatenatedOperation must have at least 2 operations",
        ));
    }
    let chained = steps
        .windows(2)
        .all(|pair| crs_more_or_less_equivalent(pair[1].source_crs(), pair[0].target_crs()));
    if !chained {
        return Err(chain_error(
            code,
            "Inconsistent chaining of CRS in operations",
        ));
    }
    let accuracies = match record.accuracy {
        Some(_) => record_accuracies(record.accuracy),
        None => summed_accuracy(&steps),
    };
    Ok(ConcatenatedOperation {
        name: record.name.to_string(),
        identifiers: operation_identifiers(code),
        remarks: String::new(),
        domains: usage_domains(&record.extent_codes)?,
        source_crs: Box::new(source_crs),
        target_crs: Box::new(target_crs),
        steps,
        accuracies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sexagesimal_values_become_decimal_degrees() {
        assert_eq!(normalize_measure(9110, 40.1), (40.0 + 10.0 / 60.0, 9102));
        assert_eq!(normalize_measure(9110, -0.3), (-(30.0 / 60.0), 9102));
        assert_eq!(normalize_measure(9102, 40.1), (40.1, 9102));
    }

    #[test]
    fn real_columns_print_as_sqlite_prints_them() {
        assert_eq!(sqlite_real_text(2.0), "2.0");
        assert_eq!(sqlite_real_text(0.03), "0.03");
        assert_eq!(sqlite_real_text(999.0), "999.0");
        assert_eq!(sqlite_real_text(1e-5), "1.0e-05");
    }
}
