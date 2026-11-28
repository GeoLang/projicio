#![cfg(feature = "registry")]

use projicio_core::Error;
use projicio_core::iso19111::authority_factory::{
    create_coordinate_operation, create_coordinate_reference_system, create_coordinate_system,
    create_datum_ensemble, create_ellipsoid, create_geodetic_datum, create_prime_meridian,
    create_unit_of_measure,
};
use projicio_core::iso19111::common::UnitType;
use projicio_core::iso19111::coordinate_system::{AxisDirection, CoordinateSystem};
use projicio_core::iso19111::datum::{EllipsoidShape, GeodeticDatum};
use projicio_core::iso19111::operation::ParameterValue;
use projicio_core::iso19111::{CoordinateOperation, Crs};
use projicio_core::registry::OperationCode;

// projinfo -q -o PROJJSON EPSG:<code>, with the code in the file name
const CRS_FIXTURES: [(u32, &str); 8] = [
    (4326, include_str!("data/projjson/epsg_4326.json")),
    (4269, include_str!("data/projjson/epsg_4269.json")),
    (4807, include_str!("data/projjson/epsg_4807.json")),
    (4979, include_str!("data/projjson/epsg_4979.json")),
    (4978, include_str!("data/projjson/epsg_4978.json")),
    (32632, include_str!("data/projjson/epsg_32632.json")),
    (2154, include_str!("data/projjson/epsg_2154.json")),
    (6318, include_str!("data/projjson/epsg_6318.json")),
];

// projinfo -q -k operation -o PROJJSON <authority>:<code>, with the code in the file name
const OPERATION_FIXTURES: [(OperationCode<'static>, &str); 12] = [
    (
        OperationCode::Epsg(1188),
        include_str!("data/projjson/operation_epsg_1188.json"),
    ),
    (
        OperationCode::Epsg(1149),
        include_str!("data/projjson/operation_epsg_1149.json"),
    ),
    (
        OperationCode::Epsg(8366),
        include_str!("data/projjson/operation_epsg_8366.json"),
    ),
    (
        OperationCode::Epsg(1313),
        include_str!("data/projjson/operation_epsg_1313.json"),
    ),
    (
        OperationCode::Epsg(10778),
        include_str!("data/projjson/operation_epsg_10778.json"),
    ),
    (
        OperationCode::Epsg(4837),
        include_str!("data/projjson/operation_epsg_4837.json"),
    ),
    (
        OperationCode::Epsg(8443),
        include_str!("data/projjson/operation_epsg_8443.json"),
    ),
    (
        OperationCode::Epsg(9091),
        include_str!("data/projjson/operation_epsg_9091.json"),
    ),
    (
        OperationCode::Epsg(9890),
        include_str!("data/projjson/operation_epsg_9890.json"),
    ),
    (
        OperationCode::Epsg(7811),
        include_str!("data/projjson/operation_epsg_7811.json"),
    ),
    (
        OperationCode::Proj("KKJ_TO_EUREF_FIN"),
        include_str!("data/projjson/operation_proj_kkj_to_euref_fin.json"),
    ),
    (
        OperationCode::Proj("WGS84_TO_WGS84_G1762"),
        include_str!("data/projjson/operation_proj_wgs84_to_wgs84_g1762.json"),
    ),
];

// the registry leaves out proj.db's scope text, remarks and extent descriptions
const UNSTORED_KEYS: [&str; 3] = ["\"scope\": ", "\"area\": ", "\"remarks\": "];

fn without_unstored_text(projjson: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in projjson.trim_end_matches('\n').lines() {
        let unstored = UNSTORED_KEYS
            .iter()
            .any(|key| line.trim_start().starts_with(key));
        if !unstored {
            lines.push(line.to_string());
            continue;
        }
        // a dropped last member leaves a trailing comma on the line before it
        let previous_needs_no_comma = !line.ends_with(',');
        if let Some(previous) = lines.last_mut() {
            if previous_needs_no_comma && previous.ends_with(',') {
                previous.pop();
            }
        }
    }
    lines.join("\n")
}

#[test]
fn unstored_text_is_dropped_with_its_comma() {
    let projjson =
        "{\n  \"name\": \"A\",\n  \"scope\": \"S\",\n  \"id\": 1,\n  \"remarks\": \"R\"\n}\n";
    assert_eq!(
        without_unstored_text(projjson),
        "{\n  \"name\": \"A\",\n  \"id\": 1\n}"
    );
}

#[test]
fn factory_crs_write_the_projjson_projinfo_writes() {
    for (code, projjson) in CRS_FIXTURES {
        let crs = create_coordinate_reference_system(code)
            .unwrap_or_else(|error| panic!("EPSG:{code}: {error}"));
        assert_eq!(
            without_unstored_text(&crs.to_projjson()),
            without_unstored_text(projjson),
            "EPSG:{code}"
        );
    }
}

#[test]
fn factory_operations_write_the_projjson_projinfo_writes() {
    for (code, projjson) in OPERATION_FIXTURES {
        let operation =
            create_coordinate_operation(code).unwrap_or_else(|error| panic!("{code}: {error}"));
        assert_eq!(
            without_unstored_text(&operation.to_projjson()),
            without_unstored_text(projjson),
            "{code}"
        );
    }
}

#[test]
fn projinfo_operation_projjson_reads_back_byte_for_byte() {
    for (code, projjson) in OPERATION_FIXTURES {
        let operation = CoordinateOperation::from_projjson(projjson)
            .unwrap_or_else(|error| panic!("{code}: {error}"));
        assert_eq!(
            operation.to_projjson(),
            projjson.trim_end_matches('\n'),
            "{code}"
        );
    }
}

#[test]
fn metre_is_the_si_length_unit() {
    let metre = create_unit_of_measure(9001).unwrap();
    assert_eq!(metre.name, "metre");
    assert_eq!(metre.conversion_to_si, 1.0);
    assert_eq!(metre.unit_type, UnitType::Linear);
    assert_eq!(
        (metre.code_space.as_str(), metre.code.as_str()),
        ("EPSG", "9001")
    );
}

#[test]
fn degree_takes_proj_name_and_exact_factor() {
    let degree = create_unit_of_measure(9122).unwrap();
    assert_eq!(degree.name, "degree");
    assert_eq!(degree.conversion_to_si, std::f64::consts::PI / 180.0);
    assert_eq!(degree.unit_type, UnitType::Angular);
}

#[test]
fn grs_1980_is_defined_by_inverse_flattening() {
    let grs_1980 = create_ellipsoid(7019).unwrap();
    assert_eq!(grs_1980.name, "GRS 1980");
    assert_eq!(grs_1980.semi_major_axis.value, 6_378_137.0);
    assert_eq!(grs_1980.semi_major_axis.unit.name, "metre");
    assert_eq!(
        grs_1980.shape,
        EllipsoidShape::InverseFlattening(298.257_222_101)
    );
    assert_eq!(grs_1980.identifiers[0].code, "7019");
}

#[test]
fn paris_meridian_is_in_grads() {
    let paris = create_prime_meridian(8903).unwrap();
    assert_eq!(paris.name, "Paris");
    assert_eq!(paris.longitude.value, 2.596_921_3);
    assert_eq!(paris.longitude.unit.name, "grad");
    assert_eq!(
        paris.longitude.unit.conversion_to_si,
        0.015_707_963_267_948_95
    );

    let Crs::Geographic(ntf_paris) = create_coordinate_reference_system(4807).unwrap() else {
        panic!("EPSG:4807 is geographic");
    };
    let GeodeticDatum::ReferenceFrame(datum) = &ntf_paris.datum else {
        panic!("EPSG:4807 has a datum, not an ensemble");
    };
    assert_eq!(datum.name, "Nouvelle Triangulation Francaise (Paris)");
    assert_eq!(datum.prime_meridian, paris);
}

#[test]
fn wgs84_is_an_ensemble_of_eight_realizations() {
    let Crs::Geographic(wgs84) = create_coordinate_reference_system(4326).unwrap() else {
        panic!("EPSG:4326 is geographic");
    };
    let GeodeticDatum::Ensemble(ensemble) = &wgs84.datum else {
        panic!("EPSG:4326 has a datum ensemble");
    };
    assert_eq!(ensemble.name, "World Geodetic System 1984 ensemble");
    assert_eq!(ensemble.accuracy, "2.0");
    let member_codes: Vec<&str> = ensemble
        .members
        .iter()
        .map(|member| member.identifiers[0].code.as_str())
        .collect();
    assert_eq!(
        member_codes,
        [
            "1166", "1152", "1153", "1154", "1155", "1156", "1309", "1383"
        ]
    );
    assert_eq!(
        ensemble.members[6]
            .dynamic
            .as_ref()
            .map(|dynamic| dynamic.frame_reference_epoch),
        Some(2016.0)
    );
    assert_eq!(create_datum_ensemble(6326).unwrap(), *ensemble);
    assert_eq!(
        create_geodetic_datum(6326).unwrap().name,
        "World Geodetic System 1984"
    );
}

#[test]
fn geocentric_crs_has_three_geocentric_axes() {
    let Crs::Geodetic(wgs84) = create_coordinate_reference_system(4978).unwrap() else {
        panic!("EPSG:4978 is geocentric");
    };
    let directions: Vec<AxisDirection> = wgs84
        .coordinate_system
        .axes
        .iter()
        .map(|axis| axis.direction)
        .collect();
    assert_eq!(
        directions,
        [
            AxisDirection::GeocentricX,
            AxisDirection::GeocentricY,
            AxisDirection::GeocentricZ
        ]
    );
    let Ok(CoordinateSystem::Ellipsoidal(geographic_3d)) = create_coordinate_system(6423) else {
        panic!("EPSG:6423 is ellipsoidal");
    };
    assert_eq!(geographic_3d.axes.len(), 3);
}

#[test]
fn ntv2_operation_carries_proj_grid_and_its_alternative() {
    let CoordinateOperation::Transformation(nad27_to_nad83) =
        create_coordinate_operation(OperationCode::Epsg(1313)).unwrap()
    else {
        panic!("EPSG:1313 is a transformation");
    };
    assert_eq!(nad27_to_nad83.method.name, "HORIZONTAL_SHIFT_GTIFF");
    assert_eq!(
        nad27_to_nad83.parameter_values[0].value,
        ParameterValue::Filename("ca_nrc_ntv2_0.tif".to_string())
    );
    assert_eq!(nad27_to_nad83.accuracies, ["1.5"]);
    assert_eq!(
        nad27_to_nad83.identifiers[0].code_space,
        "DERIVED_FROM(EPSG)"
    );

    let operation = CoordinateOperation::Transformation(nad27_to_nad83);
    let grids = operation.grids_needed();
    assert_eq!(grids.len(), 1);
    assert_eq!(grids[0].file_name, "ca_nrc_ntv2_0.tif");
    assert_eq!(grids[0].alternative.unwrap().original_name, "NTv2_0.gsb");
}

#[test]
fn reversed_grid_file_inverts_an_inverse() {
    let CoordinateOperation::Transformation(rgf93_to_ntf) =
        create_coordinate_operation(OperationCode::Epsg(9890)).unwrap()
    else {
        panic!("EPSG:9890 is a transformation");
    };
    assert_eq!(
        rgf93_to_ntf.name,
        "Inverse of Inverse of RGF93 v2 to NTF (2)"
    );
    assert_eq!(
        rgf93_to_ntf.method.name,
        "Inverse of HORIZONTAL_SHIFT_GTIFF"
    );
    assert_eq!(rgf93_to_ntf.source_crs.name(), "RGF93 v2");
    assert_eq!(
        rgf93_to_ntf.parameter_values[0].value,
        ParameterValue::Filename("fr_ign_ntf_r93.tif".to_string())
    );

    let CoordinateOperation::Concatenated(ntf_paris_to_rgf93) =
        create_coordinate_operation(OperationCode::Epsg(7811)).unwrap()
    else {
        panic!("EPSG:7811 is concatenated");
    };
    let CoordinateOperation::Transformation(reversed_step) = &ntf_paris_to_rgf93.steps[1] else {
        panic!("the second step of EPSG:7811 is a transformation");
    };
    assert_eq!(reversed_step.name, "Inverse of RGF93 v1 to NTF (2)");
    assert_eq!(reversed_step.method.name, "HORIZONTAL_SHIFT_GTIFF");
    assert_eq!(
        reversed_step.identifiers[0].code_space,
        "INVERSE(DERIVED_FROM(EPSG))"
    );
}

#[test]
fn time_dependent_helmert_keeps_rates_and_epoch() {
    let CoordinateOperation::Transformation(itrf2014_to_etrf2014) =
        create_coordinate_operation(OperationCode::Epsg(8366)).unwrap()
    else {
        panic!("EPSG:8366 is a transformation");
    };
    let codes: Vec<&str> = itrf2014_to_etrf2014
        .parameter_values
        .iter()
        .map(|value| value.parameter.identifiers[0].code.as_str())
        .collect();
    assert_eq!(
        codes,
        [
            "8605", "8606", "8607", "8608", "8609", "8610", "8611", "1040", "1041", "1042", "1043",
            "1044", "1045", "1046", "1047"
        ]
    );
    let ParameterValue::Measure(epoch) = &itrf2014_to_etrf2014.parameter_values[14].value else {
        panic!("the reference epoch is a measure");
    };
    assert_eq!((epoch.value, epoch.unit.name.as_str()), (1989.0, "year"));
    assert_eq!(itrf2014_to_etrf2014.accuracies, ["0.0"]);
}

#[test]
fn concatenated_operation_inverts_its_last_conversion() {
    let CoordinateOperation::Concatenated(kkj_to_euref_fin) =
        create_coordinate_operation(OperationCode::Epsg(10778)).unwrap()
    else {
        panic!("EPSG:10778 is concatenated");
    };
    let names: Vec<&str> = kkj_to_euref_fin
        .steps
        .iter()
        .map(CoordinateOperation::name)
        .collect();
    assert_eq!(
        names,
        [
            "Finland Uniform Coordinate System",
            "KKJ / Finland Uniform Coordinate System to EUREF-FIN / TM35FIN(E,N) (1)",
            "Inverse of TM35FIN"
        ]
    );
    let CoordinateOperation::Conversion { source_crs, .. } = &kkj_to_euref_fin.steps[2] else {
        panic!("the last step is a conversion");
    };
    assert_eq!(source_crs.name(), "EUREF-FIN / TM35FIN(E,N)");
}

#[test]
fn proj_authority_concatenation_sums_step_accuracies() {
    let CoordinateOperation::Concatenated(kkj_to_euref_fin) =
        create_coordinate_operation(OperationCode::Proj("KKJ_TO_EUREF_FIN")).unwrap()
    else {
        panic!("PROJ:KKJ_TO_EUREF_FIN is concatenated");
    };
    assert_eq!(kkj_to_euref_fin.accuracies, ["0.03"]);
    assert_eq!(kkj_to_euref_fin.identifiers[0].code_space, "PROJ");
    assert_eq!(kkj_to_euref_fin.identifiers[0].code, "KKJ_TO_EUREF_FIN");
}

#[test]
fn unknown_codes_name_the_code_and_kind() {
    let message = |error: Error| error.to_string();
    assert_eq!(
        message(create_ellipsoid(1).unwrap_err()),
        "ellipsoid EPSG:1 is not in the registry"
    );
    assert_eq!(
        message(create_coordinate_reference_system(999_999).unwrap_err()),
        "CRS EPSG:999999 is not in the registry"
    );
    assert_eq!(
        message(create_coordinate_operation(OperationCode::Proj("NO_SUCH_OPERATION")).unwrap_err()),
        "coordinate operation PROJ:NO_SUCH_OPERATION is not in the registry"
    );
    assert_eq!(
        message(create_datum_ensemble(6269).unwrap_err()),
        "datum ensemble EPSG:6269 is not in the registry"
    );
}
