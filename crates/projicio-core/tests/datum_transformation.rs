use projicio_core::{
    Ellipsoid, GeocentricCoord, GeodeticCoord, geocentric_transformation_from_epsg_method,
    transformation_from_epsg_method,
};

const X_AXIS_TRANSLATION: u32 = 8605;
const Y_AXIS_TRANSLATION: u32 = 8606;
const Z_AXIS_TRANSLATION: u32 = 8607;
const X_AXIS_ROTATION: u32 = 8608;
const Y_AXIS_ROTATION: u32 = 8609;
const Z_AXIS_ROTATION: u32 = 8610;
const SCALE_DIFFERENCE: u32 = 8611;
const RATE_X_AXIS_TRANSLATION: u32 = 1040;
const RATE_Y_AXIS_TRANSLATION: u32 = 1041;
const RATE_Z_AXIS_TRANSLATION: u32 = 1042;
const RATE_X_AXIS_ROTATION: u32 = 1043;
const RATE_Y_AXIS_ROTATION: u32 = 1044;
const RATE_Z_AXIS_ROTATION: u32 = 1045;
const RATE_SCALE_DIFFERENCE: u32 = 1046;
const REFERENCE_EPOCH: u32 = 1047;
const ORDINATE_1_OF_EVALUATION_POINT: u32 = 8617;
const ORDINATE_2_OF_EVALUATION_POINT: u32 = 8618;
const ORDINATE_3_OF_EVALUATION_POINT: u32 = 8667;
const LATITUDE_OFFSET: u32 = 8601;
const LONGITUDE_OFFSET: u32 = 8602;

// proj.db unit_of_measure conv_factor for the units the operations below are stored in
const ARC_SECOND: f64 = 4.848_136_811_095_355e-6;
const MILLIARC_SECONDS_PER_YEAR: f64 = 1.536_314_689_320_759_8e-16;
const MICRORADIAN: f64 = 1.0e-6;
const PARTS_PER_MILLION: f64 = 1.0e-6;
const YEAR: f64 = 31_556_925.445;
const GRAD: f64 = 0.015_707_963_267_948_95;

const WGS84: Ellipsoid = Ellipsoid::WGS84;
const GRS80: Ellipsoid = Ellipsoid::GRS80;
const INTERNATIONAL_1924: Ellipsoid = Ellipsoid::INTERNATIONAL_1924;
const CLARKE_1866: Ellipsoid =
    Ellipsoid::new(6_378_206.4, (6_378_206.4 - 6_356_583.8) / 6_378_206.4);
const CLARKE_1880_IGN: Ellipsoid =
    Ellipsoid::new(6_378_249.2, (6_378_249.2 - 6_356_515.0) / 6_378_249.2);
const AIRY_1830: Ellipsoid = Ellipsoid::new(6_377_563.396, 1.0 / 299.324_964_6);
const BESSEL_1841: Ellipsoid = Ellipsoid::new(6_377_397.155, 1.0 / 299.152_812_8);
const KRASSOWSKY_1940: Ellipsoid = Ellipsoid::new(6_378_245.0, 1.0 / 298.3);

const DEGREE_TOLERANCE: f64 = 1e-9;
const METRE_TOLERANCE: f64 = 1e-3;

fn rows(code: u32, kind: &str) -> Vec<[f64; 4]> {
    let path = format!(
        "{}/tests/data/datum_transformation/epsg_{code}.{kind}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap();
    text.lines()
        .map(|line| {
            let numbers: Vec<f64> = line
                .split_whitespace()
                .map(|value| value.parse().unwrap())
                .collect();
            <[f64; 4]>::try_from(numbers).unwrap()
        })
        .collect()
}

fn translation(x: f64, y: f64, z: f64) -> [(u32, f64); 3] {
    [
        (X_AXIS_TRANSLATION, x),
        (Y_AXIS_TRANSLATION, y),
        (Z_AXIS_TRANSLATION, z),
    ]
}

fn seven_parameters(
    [x, y, z]: [f64; 3],
    [rotation_x, rotation_y, rotation_z]: [f64; 3],
    rotation_unit: f64,
    scale_difference_ppm: f64,
) -> Vec<(u32, f64)> {
    let mut parameters = translation(x, y, z).to_vec();
    parameters.extend([
        (X_AXIS_ROTATION, rotation_x * rotation_unit),
        (Y_AXIS_ROTATION, rotation_y * rotation_unit),
        (Z_AXIS_ROTATION, rotation_z * rotation_unit),
        (SCALE_DIFFERENCE, scale_difference_ppm * PARTS_PER_MILLION),
    ]);
    parameters
}

fn molodensky_badekas(
    translation_and_rotation: Vec<(u32, f64)>,
    [ordinate_1, ordinate_2, ordinate_3]: [f64; 3],
) -> Vec<(u32, f64)> {
    let mut parameters = translation_and_rotation;
    parameters.extend([
        (ORDINATE_1_OF_EVALUATION_POINT, ordinate_1),
        (ORDINATE_2_OF_EVALUATION_POINT, ordinate_2),
        (ORDINATE_3_OF_EVALUATION_POINT, ordinate_3),
    ]);
    parameters
}

fn rotation_rates_only(
    [rate_x, rate_y, rate_z]: [f64; 3],
    reference_epoch_year: f64,
) -> Vec<(u32, f64)> {
    // both time-dependent operations below have only rotation rates, every other value is zero
    let mut parameters = seven_parameters([0.0; 3], [0.0; 3], ARC_SECOND, 0.0);
    parameters.extend([
        (RATE_X_AXIS_TRANSLATION, 0.0),
        (RATE_Y_AXIS_TRANSLATION, 0.0),
        (RATE_Z_AXIS_TRANSLATION, 0.0),
        (RATE_X_AXIS_ROTATION, rate_x * MILLIARC_SECONDS_PER_YEAR),
        (RATE_Y_AXIS_ROTATION, rate_y * MILLIARC_SECONDS_PER_YEAR),
        (RATE_Z_AXIS_ROTATION, rate_z * MILLIARC_SECONDS_PER_YEAR),
        (RATE_SCALE_DIFFERENCE, 0.0),
        (REFERENCE_EPOCH, reference_epoch_year * YEAR),
    ]);
    parameters
}

#[derive(Clone, Copy, PartialEq)]
enum Height {
    // projinfo wraps every geographic 2D pipeline in a push and pop of the height
    KeptBy2dCrs,
    Transformed,
}

struct GeographicCase {
    code: u32,
    method_code: u32,
    parameters: Vec<(u32, f64)>,
    source: Ellipsoid,
    target: Ellipsoid,
    // cct rows are latitude then longitude in the source CRS unit, degrees on the target side
    source_angle_unit: f64,
    height: Height,
}

fn assert_geodetic_row(
    case: &GeographicCase,
    direction: &str,
    row: usize,
    got: GeodeticCoord,
    expected: [f64; 4],
    angle_unit: f64,
) {
    let latitude = got.latitude / angle_unit;
    let longitude = got.longitude / angle_unit;
    let tolerance = DEGREE_TOLERANCE.to_radians() / angle_unit;
    let height_matches =
        case.height == Height::KeptBy2dCrs || (got.height - expected[2]).abs() < METRE_TOLERANCE;
    assert!(
        (latitude - expected[0]).abs() < tolerance
            && (longitude - expected[1]).abs() < tolerance
            && height_matches,
        "EPSG:{} {direction} row {row}: got latitude {latitude} longitude {longitude} height {}, cct {expected:?}",
        case.code,
        got.height
    );
}

fn check_geographic(case: GeographicCase) {
    let transformation = transformation_from_epsg_method(
        case.method_code,
        &case.parameters,
        &case.source,
        &case.target,
    )
    .unwrap();
    let degree = 1.0_f64.to_radians();
    let input = rows(case.code, "input");
    let forward = rows(case.code, "cct_forward");
    let inverse = rows(case.code, "cct_inverse");
    assert!(!input.is_empty() && input.len() == forward.len() && input.len() == inverse.len());
    for (row, (given, expected)) in input.iter().zip(&forward).enumerate() {
        let point = GeodeticCoord {
            longitude: given[1] * case.source_angle_unit,
            latitude: given[0] * case.source_angle_unit,
            height: given[2],
        };
        let got = transformation.forward(point, None).unwrap();
        assert_geodetic_row(&case, "forward", row, got, *expected, degree);
    }
    for (row, (given, expected)) in forward.iter().zip(&inverse).enumerate() {
        let point = GeodeticCoord {
            longitude: given[1].to_radians(),
            latitude: given[0].to_radians(),
            height: given[2],
        };
        let got = transformation.inverse(point, None).unwrap();
        assert_geodetic_row(
            &case,
            "inverse",
            row,
            got,
            *expected,
            case.source_angle_unit,
        );
    }
}

fn assert_geocentric_row(
    code: u32,
    direction: &str,
    row: usize,
    got: GeocentricCoord,
    expected: [f64; 4],
) {
    assert!(
        (got.x - expected[0]).abs() < METRE_TOLERANCE
            && (got.y - expected[1]).abs() < METRE_TOLERANCE
            && (got.z - expected[2]).abs() < METRE_TOLERANCE,
        "EPSG:{code} {direction} row {row}: got {got:?}, cct {expected:?}"
    );
}

fn check_geocentric(code: u32, method_code: u32, parameters: &[(u32, f64)], time_dependent: bool) {
    let helmert = geocentric_transformation_from_epsg_method(method_code, parameters).unwrap();
    // time-dependent cases take the epoch from the fourth cct column, the others get none
    let epoch = |row: &[f64; 4]| time_dependent.then_some(row[3]);
    let input = rows(code, "input");
    let forward = rows(code, "cct_forward");
    let inverse = rows(code, "cct_inverse");
    assert!(!input.is_empty() && input.len() == forward.len() && input.len() == inverse.len());
    for (row, (given, expected)) in input.iter().zip(&forward).enumerate() {
        let point = GeocentricCoord {
            x: given[0],
            y: given[1],
            z: given[2],
        };
        let got = helmert.forward(point, epoch(given)).unwrap();
        assert_geocentric_row(code, "forward", row, got, *expected);
    }
    for (row, (given, expected)) in forward.iter().zip(&inverse).enumerate() {
        let point = GeocentricCoord {
            x: given[0],
            y: given[1],
            z: given[2],
        };
        let got = helmert.inverse(point, epoch(given)).unwrap();
        assert_geocentric_row(code, "inverse", row, got, *expected);
    }
}

#[test]
fn geocentric_translations_geocentric_ucs_2000_to_itrf2000() {
    check_geocentric(7817, 1031, &translation(24.322, -121.372, -75.847), false);
}

#[test]
fn geocentric_translations_geog2d_nad27_to_wgs84() {
    check_geographic(GeographicCase {
        code: 1173,
        method_code: 9603,
        parameters: translation(-8.0, 160.0, 176.0).to_vec(),
        source: CLARKE_1866,
        target: WGS84,
        source_angle_unit: 1.0_f64.to_radians(),
        height: Height::KeptBy2dCrs,
    });
}

#[test]
fn position_vector_geocentric_igs08_to_igrs() {
    let parameters = seven_parameters(
        [-0.214, 0.119, 0.156],
        [-0.01182, 0.00811, -0.01677],
        ARC_SECOND,
        -0.0059,
    );
    check_geocentric(6936, 1033, &parameters, false);
}

#[test]
fn position_vector_geog2d_osgb36_to_wgs84() {
    check_geographic(GeographicCase {
        code: 1314,
        method_code: 9606,
        parameters: seven_parameters(
            [446.448, -125.157, 542.06],
            [0.15, 0.247, 0.842],
            ARC_SECOND,
            -20.489,
        ),
        source: AIRY_1830,
        target: WGS84,
        source_angle_unit: 1.0_f64.to_radians(),
        height: Height::KeptBy2dCrs,
    });
}

#[test]
fn coordinate_frame_geocentric_ongd14_to_wgs84() {
    let parameters = seven_parameters(
        [0.819, -0.5762, -1.6446],
        [0.00378, 0.03317, -0.00318],
        ARC_SECOND,
        0.0693,
    );
    check_geocentric(7377, 1032, &parameters, false);
}

#[test]
fn coordinate_frame_geog2d_mgi_to_etrs89() {
    check_geographic(GeographicCase {
        code: 1024,
        method_code: 9607,
        parameters: seven_parameters(
            [601.705, 84.263, 485.227],
            [-4.7354, -1.3145, -5.393],
            ARC_SECOND,
            -2.3887,
        ),
        source: BESSEL_1841,
        target: GRS80,
        source_angle_unit: 1.0_f64.to_radians(),
        height: Height::KeptBy2dCrs,
    });
}

fn saba_full_matrix() -> Vec<(u32, f64)> {
    seven_parameters(
        [1138.7432, -2064.4761, 110.7016],
        [-214.615206, 479.360036, -164.703951],
        ARC_SECOND,
        -402.32073,
    )
}

#[test]
fn coordinate_frame_full_matrix_geog2d_saba_to_wgs84() {
    check_geographic(GeographicCase {
        code: 10676,
        method_code: 1133,
        parameters: saba_full_matrix(),
        source: INTERNATIONAL_1924,
        target: WGS84,
        source_angle_unit: 1.0_f64.to_radians(),
        height: Height::KeptBy2dCrs,
    });
}

#[test]
fn coordinate_frame_full_matrix_geog3d_saba_to_bes2020_saba() {
    check_geographic(GeographicCase {
        code: 10646,
        method_code: 1140,
        parameters: saba_full_matrix(),
        source: INTERNATIONAL_1924,
        target: GRS80,
        source_angle_unit: 1.0_f64.to_radians(),
        height: Height::Transformed,
    });
}

#[test]
fn time_dependent_position_vector_itrf2014_to_etrf2014_at_two_epochs() {
    let parameters = rotation_rates_only([0.085, 0.531, -0.77], 1989.0);
    check_geocentric(8366, 1053, &parameters, true);
}

#[test]
fn time_dependent_coordinate_frame_itrf2014_to_gda2020_at_two_epochs() {
    let parameters = rotation_rates_only([1.50379, 1.18346, 1.20716], 2020.0);
    check_geocentric(8049, 1056, &parameters, true);
}

#[test]
fn molodensky_badekas_position_vector_geocentric_igs08_to_igrs() {
    let parameters = molodensky_badekas(
        seven_parameters(
            [0.208, -0.012, -0.229],
            [-0.01182, 0.00811, -0.01677],
            ARC_SECOND,
            -0.0059,
        ),
        [3777505.028, 3779254.396, 3471111.632],
    );
    check_geocentric(6935, 1061, &parameters, false);
}

#[test]
fn molodensky_badekas_position_vector_geog2d_pulkovo_1942_83_to_bgs2005() {
    check_geographic(GeographicCase {
        code: 7806,
        method_code: 1063,
        parameters: molodensky_badekas(
            seven_parameters(
                [5.0, -133.0, -104.0],
                [-1.4, -2.0, 3.4],
                ARC_SECOND,
                -3.9901,
            ),
            [4223032.0, 2032778.0, 4309209.0],
        ),
        source: KRASSOWSKY_1940,
        target: GRS80,
        source_angle_unit: 1.0_f64.to_radians(),
        height: Height::KeptBy2dCrs,
    });
}

#[test]
fn molodensky_badekas_coordinate_frame_geog2d_amersfoort_to_etrs89() {
    check_geographic(GeographicCase {
        code: 1066,
        method_code: 9636,
        parameters: molodensky_badekas(
            seven_parameters(
                [593.032, 26.0, 478.741],
                [1.9848, -1.7439, 9.0587],
                MICRORADIAN,
                4.0772,
            ),
            [3903453.148, 368135.313, 5012970.306],
        ),
        source: BESSEL_1841,
        target: GRS80,
        source_angle_unit: 1.0_f64.to_radians(),
        height: Height::KeptBy2dCrs,
    });
}

#[test]
fn longitude_rotation_ntf_paris_snaps_to_proj_paris_meridian() {
    check_geographic(GeographicCase {
        code: 1763,
        method_code: 9601,
        parameters: vec![(LONGITUDE_OFFSET, 2.5969213 * GRAD)],
        source: CLARKE_1880_IGN,
        target: CLARKE_1880_IGN,
        source_angle_unit: GRAD,
        height: Height::KeptBy2dCrs,
    });
}

#[test]
fn longitude_rotation_ntf_paris_with_an_offset_proj_has_no_name_for() {
    let offset_degrees: f64 = 2.0 + 20.0 / 60.0 + 13.95 / 3600.0;
    check_geographic(GeographicCase {
        code: 1764,
        method_code: 9601,
        parameters: vec![(LONGITUDE_OFFSET, offset_degrees.to_radians())],
        source: CLARKE_1880_IGN,
        target: CLARKE_1880_IGN,
        source_angle_unit: GRAD,
        height: Height::KeptBy2dCrs,
    });
}

#[test]
fn geographic_2d_offsets_greek_to_ggrs87() {
    check_geographic(GeographicCase {
        code: 1891,
        method_code: 9619,
        parameters: vec![
            (LATITUDE_OFFSET, -5.86 * ARC_SECOND),
            (LONGITUDE_OFFSET, 0.28 * ARC_SECOND),
        ],
        source: BESSEL_1841,
        target: GRS80,
        source_angle_unit: 1.0_f64.to_radians(),
        height: Height::KeptBy2dCrs,
    });
}

#[test]
fn time_dependent_method_without_epoch_raises() {
    let parameters = rotation_rates_only([0.085, 0.531, -0.77], 1989.0);
    let point = GeodeticCoord {
        longitude: 2.35_f64.to_radians(),
        latitude: 48.8_f64.to_radians(),
        height: 50.0,
    };
    let transformation =
        transformation_from_epsg_method(1053, &parameters, &GRS80, &GRS80).unwrap();
    let error = transformation.forward(point, None).unwrap_err().to_string();
    assert!(error.contains("1053") && error.contains("epoch"), "{error}");
    assert!(transformation.forward(point, Some(2025.5)).is_ok());
}

#[test]
fn missing_parameter_raises_naming_its_code() {
    let mut parameters = seven_parameters([1.0, 2.0, 3.0], [0.1, 0.2, 0.3], ARC_SECOND, 1.0);
    parameters.retain(|(code, _)| *code != SCALE_DIFFERENCE);
    let error = geocentric_transformation_from_epsg_method(1033, &parameters)
        .unwrap_err()
        .to_string();
    assert!(error.contains("1033") && error.contains("8611"), "{error}");
}

#[test]
fn methods_outside_the_port_raise_naming_the_code() {
    let molodensky = translation(1.0, 2.0, 3.0);
    let error = transformation_from_epsg_method(9604, &molodensky, &GRS80, &WGS84)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("9604") && error.contains("not implemented"),
        "{error}"
    );

    // PROJ 9.8.1 has no PROJ string for the time-specific methods
    let time_specific = seven_parameters([1.0, 2.0, 3.0], [0.1, 0.2, 0.3], ARC_SECOND, 1.0);
    let error = geocentric_transformation_from_epsg_method(1065, &time_specific)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("1065") && error.contains("no PROJ implementation"),
        "{error}"
    );

    let rotation = [(LONGITUDE_OFFSET, 0.04)];
    let error = geocentric_transformation_from_epsg_method(9601, &rotation)
        .unwrap_err()
        .to_string();
    assert!(error.contains("9601"), "{error}");
}

#[test]
fn longitude_rotation_refuses_two_ellipsoids() {
    let rotation = [(LONGITUDE_OFFSET, 0.04)];
    let error = transformation_from_epsg_method(9601, &rotation, &BESSEL_1841, &GRS80)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("9601") && error.contains("ellipsoid"),
        "{error}"
    );
}
