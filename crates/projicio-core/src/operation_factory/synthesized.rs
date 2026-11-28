// adapted from PROJ src/iso19111/operation/coordinateoperationfactory.cpp, transformation.cpp and conversion.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::candidate::{
    BALLPARK_GEOCENTRIC_TRANSLATION, BALLPARK_GEOGRAPHIC_OFFSET, CandidateOperation,
    NULL_GEOCENTRIC_TRANSLATION, NULL_GEOGRAPHIC_OFFSET,
};
use super::extent::{AreaOfUse, crs_area_of_use};
use crate::iso19111::common::{Measure, ObjectDomain, UnitOfMeasure};
use crate::iso19111::coordinate_operation::{build_operation_name, epsg_parameter};
use crate::iso19111::coordinate_system::EllipsoidalCs;
use crate::iso19111::datum::{Ellipsoid, GeodeticDatum, GeodeticReferenceFrame, PrimeMeridian};
use crate::iso19111::metadata::Identifier;
use crate::iso19111::operation::{
    Conversion, OperationMethod, OperationParameterValue, ParameterValue, Transformation,
};
use crate::iso19111::{CoordinateOperation, Crs, GeographicCrs};

const LATITUDE_OFFSET: i32 = 8601;
const LONGITUDE_OFFSET: i32 = 8602;
const VERTICAL_OFFSET: i32 = 8603;
const X_AXIS_TRANSLATION: i32 = 8605;
const Y_AXIS_TRANSLATION: i32 = 8606;
const Z_AXIS_TRANSLATION: i32 = 8607;

fn epsg_method(code: u32, name: &str) -> OperationMethod {
    OperationMethod {
        name: name.to_string(),
        identifiers: vec![Identifier::new("EPSG", &code.to_string())],
    }
}

fn measure_value(code: i32, measure: Measure) -> OperationParameterValue {
    OperationParameterValue {
        parameter: epsg_parameter(code),
        value: ParameterValue::Measure(measure),
    }
}

fn transformation(
    name: String,
    source: &Crs,
    target: &Crs,
    method: OperationMethod,
    parameter_values: Vec<OperationParameterValue>,
    accuracies: Vec<String>,
    area: Option<AreaOfUse>,
) -> CandidateOperation {
    CandidateOperation::new(CoordinateOperation::Transformation(Transformation {
        name,
        identifiers: Vec::new(),
        remarks: String::new(),
        domains: area.iter().map(AreaOfUse::to_domain).collect(),
        source_crs: Box::new(source.clone()),
        target_crs: Box::new(target.clone()),
        interpolation_crs: None,
        method,
        parameter_values,
        accuracies,
    }))
}

pub(crate) fn geodetic_datum(crs: &Crs) -> Option<&GeodeticDatum> {
    match crs {
        Crs::Geographic(geographic) => Some(&geographic.datum),
        Crs::Geodetic(geodetic) => Some(&geodetic.datum),
        Crs::Projected(_) | Crs::Bound(_) => None,
    }
}

fn datum_name(datum: &GeodeticDatum) -> String {
    match datum {
        GeodeticDatum::ReferenceFrame(frame) => frame.name.clone(),
        GeodeticDatum::Ensemble(ensemble) => ensemble.as_datum().name,
    }
}

pub(crate) fn is_same_geodetic_datum(datum: &GeodeticDatum, other: &GeodeticDatum) -> bool {
    // proj's isSameGeodeticDatum
    let (name, other_name) = (datum_name(datum), datum_name(other));
    if (name == "unknown") != (other_name == "unknown") {
        return false;
    }
    datum.is_equivalent_to(other)
}

pub(crate) fn axis_count(crs: &Crs) -> usize {
    match crs {
        Crs::Geographic(geographic) => geographic.coordinate_system.axes.len(),
        Crs::Geodetic(geodetic) => geodetic.coordinate_system.axes.len(),
        Crs::Projected(projected) => projected.coordinate_system.axes.len(),
        Crs::Bound(bound) => axis_count(&bound.source_crs),
    }
}

pub(crate) fn ballpark_geographic_offset(
    source: &Crs,
    target: &Crs,
    force_ballpark: bool,
) -> CandidateOperation {
    // proj's createBallparkGeographicOffset
    let same_datum = !force_ballpark
        && matches!((source, target), (Crs::Geographic(_), Crs::Geographic(_)))
        && match (geodetic_datum(source), geodetic_datum(target)) {
            (Some(datum), Some(other)) => is_same_geodetic_datum(datum, other),
            _ => false,
        };
    let name_prefix = if same_datum {
        NULL_GEOGRAPHIC_OFFSET
    } else {
        BALLPARK_GEOGRAPHIC_OFFSET
    };
    let source_extent = crs_area_of_use(source);
    let same_extent = source_extent.is_some() && source_extent == crs_area_of_use(target);
    let area = match source_extent {
        Some(extent) if same_extent => extent,
        _ => AreaOfUse::world(),
    };
    let accuracies = if same_datum {
        vec!["0".to_string()]
    } else {
        Vec::new()
    };
    let zero_angle = || Measure::new(0.0, UnitOfMeasure::degree());
    let three_dimensional = axis_count(source) == 3 || axis_count(target) == 3;
    let method = if three_dimensional {
        epsg_method(9660, "Geographic3D offsets")
    } else {
        epsg_method(9619, "Geographic2D offsets")
    };
    let mut parameter_values = vec![
        measure_value(LATITUDE_OFFSET, zero_angle()),
        measure_value(LONGITUDE_OFFSET, zero_angle()),
    ];
    if three_dimensional {
        parameter_values.push(measure_value(
            VERTICAL_OFFSET,
            Measure::new(0.0, UnitOfMeasure::metre()),
        ));
    }
    transformation(
        build_operation_name(name_prefix, source, target),
        source,
        target,
        method,
        parameter_values,
        accuracies,
        Some(area),
    )
}

pub(crate) fn transformation_name(source: &Crs, target: &Crs) -> String {
    // proj's buildTransfName, which adds no 2D, 3D or geocentric qualifier
    format!("Transformation from {} to {}", source.name(), target.name())
}

pub(crate) fn longitude_rotation(
    name: String,
    source: &Crs,
    target: &Crs,
    offset: Measure,
) -> CandidateOperation {
    // proj's Transformation::createLongitudeRotation gives it no accuracy
    transformation(
        name,
        source,
        target,
        epsg_method(9601, "Longitude rotation"),
        vec![measure_value(LONGITUDE_OFFSET, offset)],
        Vec::new(),
        Some(AreaOfUse::world()),
    )
}

pub(crate) fn null_geocentric_translation(source: &Crs, target: &Crs) -> CandidateOperation {
    zero_geocentric_translation(
        NULL_GEOCENTRIC_TRANSLATION,
        source,
        target,
        vec!["0".to_string()],
    )
}

pub(crate) fn ballpark_geocentric_translation(source: &Crs, target: &Crs) -> CandidateOperation {
    zero_geocentric_translation(BALLPARK_GEOCENTRIC_TRANSLATION, source, target, Vec::new())
}

fn zero_geocentric_translation(
    name_prefix: &str,
    source: &Crs,
    target: &Crs,
    accuracies: Vec<String>,
) -> CandidateOperation {
    // proj names these without the qualifiers buildOpName adds
    let name = format!("{name_prefix} from {} to {}", source.name(), target.name());
    let zero_metres = || Measure::new(0.0, UnitOfMeasure::metre());
    transformation(
        name,
        source,
        target,
        epsg_method(1031, "Geocentric translations (geocentric domain)"),
        vec![
            measure_value(X_AXIS_TRANSLATION, zero_metres()),
            measure_value(Y_AXIS_TRANSLATION, zero_metres()),
            measure_value(Z_AXIS_TRANSLATION, zero_metres()),
        ],
        accuracies,
        Some(AreaOfUse::world()),
    )
}

fn conversion(
    name: String,
    method: OperationMethod,
    source: &Crs,
    target: &Crs,
) -> CandidateOperation {
    CandidateOperation::new(CoordinateOperation::Conversion {
        conversion: Conversion {
            name,
            identifiers: Vec::new(),
            method,
            parameter_values: Vec::new(),
        },
        source_crs: Box::new(source.clone()),
        target_crs: Box::new(target.clone()),
    })
}

pub(crate) fn axis_order_reversal(
    three_dimensional: bool,
    source: &Crs,
    target: &Crs,
) -> CandidateOperation {
    // proj's Conversion::createAxisOrderReversal
    if three_dimensional {
        return conversion(
            "axis order change (geographic3D horizontal)".to_string(),
            epsg_method(9844, "Axis Order Reversal (Geographic3D horizontal)"),
            source,
            target,
        );
    }
    conversion(
        "axis order change (2D)".to_string(),
        epsg_method(9843, "Axis Order Reversal (2D)"),
        source,
        target,
    )
}

pub(crate) fn geographic_geocentric_conversion(source: &Crs, target: &Crs) -> CandidateOperation {
    // proj's Conversion::createGeographicGeocentric
    conversion(
        build_operation_name("Conversion", source, target),
        epsg_method(9602, "Geographic/geocentric conversions"),
        source,
        target,
    )
}

pub(crate) fn unnamed_datum(
    ellipsoid: &Ellipsoid,
    prime_meridian: &PrimeMeridian,
) -> GeodeticDatum {
    // proj's GeodeticReferenceFrame::create with an empty property map
    GeodeticDatum::ReferenceFrame(Box::new(GeodeticReferenceFrame {
        name: String::new(),
        identifiers: Vec::new(),
        remarks: String::new(),
        domains: Vec::new(),
        anchor: None,
        anchor_epoch: None,
        ellipsoid: ellipsoid.clone(),
        prime_meridian: prime_meridian.clone(),
        dynamic: None,
    }))
}

pub(crate) fn prime_meridian(datum: &GeodeticDatum) -> &PrimeMeridian {
    match datum {
        GeodeticDatum::ReferenceFrame(frame) => &frame.prime_meridian,
        GeodeticDatum::Ensemble(ensemble) => &ensemble.members[0].prime_meridian,
    }
}

pub(crate) fn intermediate_geographic_crs(
    name: String,
    domains: Vec<ObjectDomain>,
    datum: GeodeticDatum,
    coordinate_system: EllipsoidalCs,
) -> Crs {
    Crs::Geographic(GeographicCrs {
        name,
        identifiers: Vec::new(),
        remarks: String::new(),
        domains,
        datum,
        coordinate_system,
    })
}
