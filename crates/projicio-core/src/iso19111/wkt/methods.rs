// adapted from PROJ src/iso19111/operation/parammappings.cpp, esriparammappings.cpp, oputils.cpp and singleoperation.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::mappings::{
    ESRI_METHOD_MAPPINGS, EsriMethodMapping, METHOD_NAME_CODES, MethodMapping,
    OTHER_METHOD_MAPPINGS, PARAM_NAME_CODES, PROJECTION_METHOD_MAPPINGS, ParamMapping,
};
use crate::iso19111::common::{Measure, UnitOfMeasure, epsg_code};
use crate::iso19111::metadata::is_equivalent_name;
use crate::iso19111::operation::{
    OperationMethod, OperationParameter, OperationParameterValue, ParameterValue,
};

pub(super) const METHOD_TRANSVERSE_MERCATOR: i32 = 9807;
pub(super) const METHOD_TRANSVERSE_MERCATOR_SOUTH_ORIENTATED: i32 = 9808;
pub(super) const METHOD_LAMBERT_CONIC_CONFORMAL_1SP: i32 = 9801;
pub(super) const METHOD_LAMBERT_CONIC_CONFORMAL_2SP: i32 = 9802;
pub(super) const METHOD_LAMBERT_CONIC_CONFORMAL_2SP_MICHIGAN: i32 = 1051;
pub(super) const METHOD_MERCATOR_VARIANT_A: i32 = 9804;
pub(super) const METHOD_MERCATOR_VARIANT_B: i32 = 9805;
pub(super) const METHOD_MERCATOR_SPHERICAL: i32 = 1026;
pub(super) const METHOD_POPULAR_VISUALISATION_PSEUDO_MERCATOR: i32 = 1024;
pub(super) const METHOD_POLAR_STEREOGRAPHIC_VARIANT_A: i32 = 9810;
pub(super) const METHOD_POLAR_STEREOGRAPHIC_VARIANT_B: i32 = 9829;
pub(super) const METHOD_HOTINE_OBLIQUE_MERCATOR_VARIANT_A: i32 = 9812;
pub(super) const METHOD_HOTINE_OBLIQUE_MERCATOR_VARIANT_B: i32 = 9815;
pub(super) const METHOD_KROVAK: i32 = 9819;
pub(super) const METHOD_KROVAK_NORTH_ORIENTED: i32 = 1041;
pub(super) const METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA: i32 = 9820;
pub(super) const METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA_SPHERICAL: i32 = 1027;
pub(super) const METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA: i32 = 9835;
pub(super) const METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA_SPHERICAL: i32 = 9834;
pub(super) const METHOD_EQUIDISTANT_CYLINDRICAL: i32 = 1028;
pub(super) const METHOD_EQUIDISTANT_CYLINDRICAL_SPHERICAL: i32 = 1029;
pub(super) const METHOD_VERTICAL_PERSPECTIVE: i32 = 9838;
pub(super) const METHOD_NTV2: i32 = 9615;
pub(super) const METHOD_NADCON: i32 = 9613;
pub(super) const METHOD_POSITION_VECTOR_GEOCENTRIC: i32 = 1033;
pub(super) const METHOD_POSITION_VECTOR_GEOGRAPHIC_2D: i32 = 9606;
pub(super) const METHOD_POSITION_VECTOR_GEOGRAPHIC_3D: i32 = 1037;

pub(super) const PARAMETER_LATITUDE_OF_NATURAL_ORIGIN: i32 = 8801;
pub(super) const PARAMETER_LONGITUDE_OF_NATURAL_ORIGIN: i32 = 8802;
pub(super) const PARAMETER_SCALE_FACTOR_AT_NATURAL_ORIGIN: i32 = 8805;
pub(super) const PARAMETER_FALSE_EASTING: i32 = 8806;
pub(super) const PARAMETER_FALSE_NORTHING: i32 = 8807;
pub(super) const PARAMETER_AZIMUTH_PROJECTION_CENTRE: i32 = 8813;
pub(super) const PARAMETER_ANGLE_RECTIFIED_TO_SKEW_GRID: i32 = 8814;
pub(super) const PARAMETER_LATITUDE_1ST_STD_PARALLEL: i32 = 8823;
pub(super) const PARAMETER_LATITUDE_STD_PARALLEL: i32 = 8832;
pub(super) const PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE: i32 = 8656;
pub(super) const PARAMETER_LATITUDE_DIFFERENCE_FILE: i32 = 8657;
pub(super) const PARAMETER_X_AXIS_TRANSLATION: i32 = 8605;
pub(super) const PARAMETER_Y_AXIS_TRANSLATION: i32 = 8606;
pub(super) const PARAMETER_Z_AXIS_TRANSLATION: i32 = 8607;
pub(super) const PARAMETER_X_AXIS_ROTATION: i32 = 8608;
pub(super) const PARAMETER_Y_AXIS_ROTATION: i32 = 8609;
pub(super) const PARAMETER_Z_AXIS_ROTATION: i32 = 8610;
pub(super) const PARAMETER_SCALE_DIFFERENCE: i32 = 8611;

pub(super) fn method_epsg_code(method: &OperationMethod) -> i32 {
    // OperationMethod::getEPSGCode
    let code = epsg_code(&method.identifiers);
    if code != 0 {
        return code;
    }
    let name = method.name.strip_suffix(" (3D)").unwrap_or(&method.name);
    METHOD_NAME_CODES
        .iter()
        .find(|(method_name, _)| is_equivalent_name(name, method_name))
        .map_or(0, |(_, code)| *code)
}

pub(super) fn parameter_epsg_code(parameter: &OperationParameter) -> i32 {
    // OperationParameter::getEPSGCode
    let code = epsg_code(&parameter.identifiers);
    if code != 0 {
        return code;
    }
    PARAM_NAME_CODES
        .iter()
        .find(|(parameter_name, _)| is_equivalent_name(&parameter.name, parameter_name))
        .map_or_else(|| parameter.epsg_code(), |(_, code)| *code)
}

pub(super) fn method_name_of_code(code: i32) -> &'static str {
    METHOD_NAME_CODES
        .iter()
        .find(|(_, method_code)| *method_code == code)
        .map(|(name, _)| *name)
        .expect("the method code is one of PROJ's method name codes")
}

pub(super) fn parameter_name_of_code(code: i32) -> &'static str {
    PARAM_NAME_CODES
        .iter()
        .find(|(_, parameter_code)| *parameter_code == code)
        .map(|(name, _)| *name)
        .expect("the parameter code is one of PROJ's parameter name codes")
}

pub(super) fn mapping_by_code(code: i32) -> Option<&'static MethodMapping> {
    // getMapping(int epsg_code)
    PROJECTION_METHOD_MAPPINGS
        .iter()
        .find(|mapping| mapping.epsg_code == code)
}

pub(super) fn mapping_for_method(method: &OperationMethod) -> Option<&'static MethodMapping> {
    // getMapping(const OperationMethod *)
    let code = method_epsg_code(method);
    PROJECTION_METHOD_MAPPINGS.iter().find(|mapping| {
        (code != 0 && mapping.epsg_code == code)
            || is_equivalent_name(mapping.wkt2_name, &method.name)
    })
}

pub(super) fn mapping_from_wkt1(wkt1_name: &str) -> Option<&'static MethodMapping> {
    // getMappingFromWKT1(const std::string &)
    // OGC 12-063r5 C.4.2 names a projection "UTM zone"
    let utm_prefix = "UTM zone";
    if wkt1_name
        .get(..utm_prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(utm_prefix))
    {
        return mapping_by_code(METHOD_TRANSVERSE_MERCATOR);
    }
    PROJECTION_METHOD_MAPPINGS.iter().find(|mapping| {
        mapping
            .wkt1_name
            .is_some_and(|name| is_equivalent_name(name, wkt1_name))
    })
}

pub(super) fn mapping_by_wkt2_name(wkt2_name: &str) -> Option<&'static MethodMapping> {
    // getMapping(const char *wkt2_name)
    PROJECTION_METHOD_MAPPINGS
        .iter()
        .chain(OTHER_METHOD_MAPPINGS.iter())
        .find(|mapping| is_equivalent_name(mapping.wkt2_name, wkt2_name))
}

pub(super) fn parameter_mapping(
    mapping: &'static MethodMapping,
    parameter: &OperationParameter,
) -> Option<&'static ParamMapping> {
    // getMapping(const MethodMapping *, const OperationParameterNNPtr &)
    let code = parameter_epsg_code(parameter);
    let params = || mapping.params.iter().copied();
    if code != 0 {
        if let Some(found) = params().find(|param| param.epsg_code == code) {
            return Some(found);
        }
    }
    if let Some(found) = params().find(|param| is_equivalent_name(param.wkt2_name, &parameter.name))
    {
        return Some(found);
    }
    params().find(|param| are_equivalent_parameters(param.wkt2_name, &parameter.name))
}

pub(super) fn parameter_mapping_from_wkt1(
    mapping: &'static MethodMapping,
    wkt1_name: &str,
) -> Option<&'static ParamMapping> {
    // getMappingFromWKT1(const MethodMapping *, const std::string &)
    mapping.params.iter().copied().find(|param| {
        param.wkt1_name.is_some_and(|name| {
            is_equivalent_name(name, wkt1_name) || are_equivalent_parameters(name, wkt1_name)
        })
    })
}

// buildSetEquivalentParameters in oputils.cpp
const EQUIVALENT_PARAMETER_NAMES: [&[&str]; 12] = [
    &["latitude_of_point_1", "Latitude_Of_1st_Point"],
    &["longitude_of_point_1", "Longitude_Of_1st_Point"],
    &["latitude_of_point_2", "Latitude_Of_2nd_Point"],
    &["longitude_of_point_2", "Longitude_Of_2nd_Point"],
    &["satellite_height", "height"],
    &[
        "False easting",
        "Easting at false origin",
        "Easting at projection centre",
    ],
    &[
        "False northing",
        "Northing at false origin",
        "Northing at projection centre",
    ],
    &[
        "Scale factor at natural origin",
        "scale_factor",
        "Scale factor on initial line",
        "Scale factor at projection centre",
        "Scale factor on pseudo standard parallel",
    ],
    &[
        "latitude_of_origin",
        "latitude_of_center",
        "Latitude of natural origin",
        "Latitude of false origin",
        "Latitude of projection centre",
        "Central_Parallel",
    ],
    &[
        "central_meridian",
        "longitude_of_center",
        "Longitude of natural origin",
        "Longitude of false origin",
        "Longitude of projection centre",
        "Longitude of origin",
    ],
    &["Azimuth of initial line", "Azimuth at projection centre"],
    &["pseudo_standard_parallel_1", "standard_parallel_1"],
];

pub(super) fn are_equivalent_parameters(a: &str, b: &str) -> bool {
    // areEquivalentParameters, which pairs two different names of one group after canonicalizing them
    EQUIVALENT_PARAMETER_NAMES.iter().any(|group| {
        group.iter().enumerate().any(|(index, name)| {
            group.iter().enumerate().any(|(other_index, other_name)| {
                index != other_index
                    && is_equivalent_name(a, name)
                    && is_equivalent_name(b, other_name)
            })
        })
    })
}

pub(super) fn esri_mapping(wkt2_name: &str, code: i32) -> Option<&'static EsriMethodMapping> {
    // getESRIMapping(wkt2_name, epsg_code) in conversion.cpp
    ESRI_METHOD_MAPPINGS.iter().find(|mapping| {
        (code != 0 && mapping.epsg_code == code)
            || mapping.wkt2_name.eq_ignore_ascii_case(wkt2_name)
    })
}

pub(super) fn esri_mappings_from_esri_name(esri_name: &str) -> Vec<&'static EsriMethodMapping> {
    // getMappingsFromESRI
    ESRI_METHOD_MAPPINGS
        .iter()
        .filter(|mapping| mapping.esri_name.eq_ignore_ascii_case(esri_name))
        .collect()
}

pub(super) fn parameter_value_by_code(
    values: &[OperationParameterValue],
    code: i32,
) -> Option<&ParameterValue> {
    // SingleOperation::parameterValue(epsg_code)
    values
        .iter()
        .find(|value| parameter_epsg_code(&value.parameter) == code)
        .map(|value| &value.value)
}

pub(super) fn parameter_measure(values: &[OperationParameterValue], code: i32) -> Option<&Measure> {
    // SingleOperation::parameterValueMeasure(epsg_code)
    match parameter_value_by_code(values, code)? {
        ParameterValue::Measure(measure) => Some(measure),
        ParameterValue::Integer(_) | ParameterValue::Filename(_) => None,
    }
}

pub(super) fn parameter_si_value(values: &[OperationParameterValue], code: i32) -> f64 {
    // SingleOperation::parameterValueNumericAsSI(epsg_code), zero when the parameter is missing
    parameter_measure(values, code).map_or(0.0, Measure::si_value)
}

pub(super) fn parameter_value_in_unit(
    values: &[OperationParameterValue],
    code: i32,
    unit: &UnitOfMeasure,
) -> f64 {
    // SingleOperation::parameterValueNumeric(epsg_code, unit), zero when the parameter is missing
    parameter_measure(values, code).map_or(0.0, |measure| measure.convert_to_unit(unit))
}

pub(super) fn parameter_value_by_name_or_code<'a>(
    values: &'a [OperationParameterValue],
    name: &str,
    code: i32,
) -> Option<&'a ParameterValue> {
    // SingleOperation::parameterValue(name, epsg_code)
    if code != 0 {
        if let Some(value) = parameter_value_by_code(values, code) {
            return Some(value);
        }
    }
    values
        .iter()
        .find(|value| is_equivalent_name(name, &value.parameter.name))
        .or_else(|| {
            values
                .iter()
                .find(|value| are_equivalent_parameters(name, &value.parameter.name))
        })
        .map(|value| &value.value)
}
