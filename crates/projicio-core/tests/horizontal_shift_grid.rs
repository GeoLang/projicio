#![cfg(feature = "registry")]

use projicio_core::{
    DatumTransformation, GeodeticCoord, grid_transformation_from_epsg_method, horizontal_shift_grid,
};

const NADCON: u32 = 9613;
const NTV1: u32 = 9614;
const NTV2: u32 = 9615;
const NADCON5_2D: u32 = 1074;
const GEOCENTRIC_TRANSLATIONS_BY_GRID_IGN: u32 = 1087;
const LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE: u32 = 8656;
const LATITUDE_DIFFERENCE_FILE: u32 = 8657;
const LONGITUDE_DIFFERENCE_FILE: u32 = 8658;

const DEGREE_TOLERANCE: f64 = 1e-9;

fn data_path(file_name: &str) -> String {
    format!(
        "{}/tests/data/horizontal_shift_grid/{file_name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn rows(fixture: &str, kind: &str) -> Vec<[f64; 4]> {
    let text = std::fs::read_to_string(data_path(&format!("{fixture}.{kind}.txt"))).unwrap();
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

fn point(longitude_degrees: f64, latitude_degrees: f64) -> GeodeticCoord {
    GeodeticCoord {
        longitude: longitude_degrees.to_radians(),
        latitude: latitude_degrees.to_radians(),
        height: 0.0,
    }
}

fn assert_degrees(
    context: &str,
    got: GeodeticCoord,
    longitude: f64,
    latitude: f64,
    tolerance: f64,
) {
    let got_longitude = got.longitude.to_degrees();
    let got_latitude = got.latitude.to_degrees();
    assert!(
        (got_longitude - longitude).abs() < tolerance
            && (got_latitude - latitude).abs() < tolerance,
        "{context}: got longitude {got_longitude} latitude {got_latitude}, want {longitude} {latitude}"
    );
}

struct CctCase<'a> {
    fixture: &'a str,
    grid_file: &'a str,
    method_code: u32,
    parameter_code: u32,
    with_inverse: bool,
}

fn check_against_cct(case: CctCase) {
    horizontal_shift_grid::register_file(case.grid_file, data_path(case.grid_file)).unwrap();
    let transformation = grid_transformation_from_epsg_method(
        case.method_code,
        &[(case.parameter_code, case.grid_file)],
    )
    .unwrap();
    let input = rows(case.fixture, "input");
    let forward = rows(case.fixture, "cct_forward");
    assert!(!input.is_empty() && input.len() == forward.len());
    // cct rows are latitude then longitude in degrees
    for (row, (given, expected)) in input.iter().zip(&forward).enumerate() {
        let got = transformation
            .forward(point(given[1], given[0]), None)
            .unwrap();
        let context = format!("{} forward row {row}", case.fixture);
        assert_degrees(&context, got, expected[1], expected[0], DEGREE_TOLERANCE);
    }
    if !case.with_inverse {
        return;
    }
    let inverse = rows(case.fixture, "cct_inverse");
    assert_eq!(forward.len(), inverse.len());
    for (row, (given, expected)) in forward.iter().zip(&inverse).enumerate() {
        let got = transformation
            .inverse(point(given[1], given[0]), None)
            .unwrap();
        let context = format!("{} inverse row {row}", case.fixture);
        assert_degrees(&context, got, expected[1], expected[0], DEGREE_TOLERANCE);
    }
}

#[test]
fn ntv2_with_a_subgrid_matches_cct_both_ways() {
    // the point at latitude 46.5 longitude 2.5 sits on the subgrid edge, and cct inverts its shift through the parent grid
    check_against_cct(CctCase {
        fixture: "nonlinear_with_subgrid",
        grid_file: "nonlinear_with_subgrid.gsb",
        method_code: NTV2,
        parameter_code: LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE,
        with_inverse: true,
    });
}

#[test]
fn ntv1_matches_cct_both_ways() {
    check_against_cct(CctCase {
        fixture: "nonlinear_ntv1",
        grid_file: "nonlinear_ntv1.dat",
        method_code: NTV1,
        parameter_code: LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE,
        with_inverse: true,
    });
}

#[test]
fn big_endian_ntv2_matches_cct() {
    // the grid shifts every point past its own north edge, so cct has no inverse to compare
    check_against_cct(CctCase {
        fixture: "test_hgrid_big_endian",
        grid_file: "test_hgrid_big_endian.gsb",
        method_code: NTV2,
        parameter_code: LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE,
        with_inverse: false,
    });
}

#[test]
fn ctable2_matches_cct() {
    // the grid's shifts are velocities read as radians, which move every point off the grid
    check_against_cct(CctCase {
        fixture: "nkgrf03vel_realigned_xy_extract",
        grid_file: "nkgrf03vel_realigned_xy_extract.ct2",
        method_code: NADCON,
        parameter_code: LATITUDE_DIFFERENCE_FILE,
        with_inverse: false,
    });
}

const ARC_SECONDS_PER_DEGREE: f64 = 3600.0;

struct GeneratedGrid {
    south: f64,
    west: f64,
    step: f64,
    columns: usize,
    rows: usize,
}

const GENERATED: GeneratedGrid = GeneratedGrid {
    south: 50.0,
    west: 10.0,
    step: 1.0,
    columns: 3,
    rows: 2,
};

fn generated_shift(column: usize, row: usize) -> (f64, f64) {
    (
        1.0 + column as f64 + 10.0 * row as f64,
        2.0 * (column as f64 + 1.0) + 0.5 * row as f64,
    )
}

fn ntv2_record(out: &mut Vec<u8>, label: &str, value: [u8; 8]) {
    out.extend_from_slice(format!("{label:<8}").as_bytes());
    out.extend_from_slice(&value);
}

fn ntv2_text(text: &str) -> [u8; 8] {
    let mut value = [b' '; 8];
    value[..text.len()].copy_from_slice(text.as_bytes());
    value
}

fn ntv2_integer(number: i32) -> [u8; 8] {
    let mut value = [0; 8];
    value[..4].copy_from_slice(&number.to_le_bytes());
    value
}

fn ntv2_overview(out: &mut Vec<u8>, subfile_count: i32) {
    ntv2_record(out, "NUM_OREC", ntv2_integer(11));
    ntv2_record(out, "NUM_SREC", ntv2_integer(11));
    ntv2_record(out, "NUM_FILE", ntv2_integer(subfile_count));
    ntv2_record(out, "GS_TYPE", ntv2_text("SECONDS"));
    ntv2_record(out, "VERSION", ntv2_text("NTv2.0"));
    for label in ["SYSTEM_F", "SYSTEM_T"] {
        ntv2_record(out, label, ntv2_text("TEST"));
    }
    for label in ["MAJOR_F", "MINOR_F", "MAJOR_T", "MINOR_T"] {
        ntv2_record(out, label, 6_378_137.0_f64.to_le_bytes());
    }
}

fn ntv2_subfile(
    out: &mut Vec<u8>,
    name: &str,
    parent: &str,
    grid: &GeneratedGrid,
    shift: impl Fn(usize, usize) -> (f64, f64),
) {
    let north = grid.south + grid.step * (grid.rows - 1) as f64;
    let east = grid.west + grid.step * (grid.columns - 1) as f64;
    ntv2_record(out, "SUB_NAME", ntv2_text(name));
    ntv2_record(out, "PARENT", ntv2_text(parent));
    ntv2_record(out, "CREATED", ntv2_text("20261127"));
    ntv2_record(out, "UPDATED", ntv2_text("20261127"));
    // ntv2 counts longitude positive west
    let arc_seconds = |degrees: f64| (degrees * ARC_SECONDS_PER_DEGREE).to_le_bytes();
    ntv2_record(out, "S_LAT", arc_seconds(grid.south));
    ntv2_record(out, "N_LAT", arc_seconds(north));
    ntv2_record(out, "E_LONG", arc_seconds(-east));
    ntv2_record(out, "W_LONG", arc_seconds(-grid.west));
    ntv2_record(out, "LAT_INC", arc_seconds(grid.step));
    ntv2_record(out, "LONG_INC", arc_seconds(grid.step));
    ntv2_record(
        out,
        "GS_COUNT",
        ntv2_integer((grid.columns * grid.rows) as i32),
    );
    for row in 0..grid.rows {
        // ntv2 stores each row east to west
        for column in (0..grid.columns).rev() {
            let (north_shift, east_shift) = shift(column, row);
            for value in [north_shift as f32, -east_shift as f32, 0.0, 0.0] {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
}

fn ntv2_with_shifts(grid: &GeneratedGrid, shift: impl Fn(usize, usize) -> (f64, f64)) -> Vec<u8> {
    let mut out = Vec::new();
    ntv2_overview(&mut out, 1);
    ntv2_subfile(&mut out, "ONLY", "NONE", grid, shift);
    out
}

fn generated_ntv2() -> Vec<u8> {
    ntv2_with_shifts(&GENERATED, generated_shift)
}

fn registered_transformation(
    name: &str,
    bytes: &[u8],
) -> Box<dyn DatumTransformation + Send + Sync> {
    horizontal_shift_grid::register_bytes(name, bytes).unwrap();
    grid_transformation_from_epsg_method(NTV2, &[(LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE, name)])
        .unwrap()
}

#[test]
fn generated_ntv2_registers_and_shifts_by_its_node_values() {
    let name = "projicio-test-generated.gsb";
    assert!(!horizontal_shift_grid::is_available(name));
    let transformation = registered_transformation(name, &generated_ntv2());
    assert!(horizontal_shift_grid::is_registered(name));
    assert!(horizontal_shift_grid::registered().contains(&name.to_string()));
    assert!(horizontal_shift_grid::is_available(name));
    assert_eq!(horizontal_shift_grid::accepted_names(name), [name]);

    // f32 shifts of a few arc-seconds keep well inside 1e-10 degrees
    let tolerance = 1e-10;
    let (north_shift, east_shift) = generated_shift(1, 0);
    let node = transformation.forward(point(11.0, 50.0), None).unwrap();
    assert_degrees(
        "node",
        node,
        11.0 + east_shift / ARC_SECONDS_PER_DEGREE,
        50.0 + north_shift / ARC_SECONDS_PER_DEGREE,
        tolerance,
    );

    let corners =
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(column, row)| generated_shift(column, row));
    let mean = |shift: fn(&(f64, f64)) -> f64| corners.iter().map(shift).sum::<f64>() / 4.0;
    let centre = transformation.forward(point(10.5, 50.5), None).unwrap();
    assert_degrees(
        "cell centre",
        centre,
        10.5 + mean(|shift| shift.1) / ARC_SECONDS_PER_DEGREE,
        50.5 + mean(|shift| shift.0) / ARC_SECONDS_PER_DEGREE,
        tolerance,
    );

    let back = transformation.inverse(centre, None).unwrap();
    assert_degrees(
        "inverse of the cell centre",
        back,
        10.5,
        50.5,
        DEGREE_TOLERANCE,
    );
    assert_eq!(back.height, 0.0);
}

#[test]
fn point_outside_the_grid_raises_naming_it() {
    let name = "projicio-test-outside.gsb";
    let transformation = registered_transformation(name, &generated_ntv2());
    for result in [
        transformation.forward(point(20.0, 50.5), None),
        transformation.inverse(point(10.5, 40.0), None),
    ] {
        let error = result.unwrap_err().to_string();
        assert!(error.contains(name) && error.contains("outside"), "{error}");
    }
}

#[test]
fn missing_grid_raises_naming_every_name_that_satisfies_it() {
    let error = grid_transformation_from_epsg_method(
        NTV2,
        &[(LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE, "NTv2_0.gsb")],
    )
    .err()
    .unwrap()
    .to_string();
    for name in ["ca_nrc_ntv2_0.tif", "ntv2_0.gsb", "NTv2_0.gsb"] {
        assert!(error.contains(name), "{error}");
    }
    assert_eq!(
        horizontal_shift_grid::accepted_names("NTv2_0.gsb"),
        ["ca_nrc_ntv2_0.tif", "ntv2_0.gsb", "NTv2_0.gsb"]
    );
    assert!(!horizontal_shift_grid::is_available("NTv2_0.gsb"));
}

#[test]
fn epsg_name_of_a_file_marked_inverse_direction_runs_backwards_and_proj_names_run_forward() {
    // proj.db marks ntf_r93.gsb as applying EPSG's rgf93_ntf.gsb operation in reverse
    let reversed_name = "ntf_r93.gsb";
    let plain_name = "projicio-test-plain-direction.gsb";
    horizontal_shift_grid::register_bytes(reversed_name, &generated_ntv2()).unwrap();
    let plain = registered_transformation(plain_name, &generated_ntv2());
    assert!(horizontal_shift_grid::is_available("rgf93_ntf.gsb"));
    let reversed = grid_transformation_from_epsg_method(
        NTV2,
        &[(LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE, "rgf93_ntf.gsb")],
    )
    .unwrap();

    let start = point(10.7, 50.3);
    let reversed_forward = reversed.forward(start, None).unwrap();
    let plain_inverse = plain.inverse(start, None).unwrap();
    assert_eq!(reversed_forward, plain_inverse);
    assert_ne!(reversed_forward, plain.forward(start, None).unwrap());
    assert_eq!(
        reversed.inverse(start, None).unwrap(),
        plain.forward(start, None).unwrap()
    );

    for proj_name in [reversed_name, "fr_ign_ntf_r93.tif"] {
        let direct = grid_transformation_from_epsg_method(
            NTV2,
            &[(LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE, proj_name)],
        )
        .unwrap();
        assert_eq!(
            direct.forward(start, None).unwrap(),
            plain.forward(start, None).unwrap(),
            "{proj_name}"
        );
    }
}

#[test]
fn bad_bytes_raise_at_registration_naming_the_grid() {
    let garbage_name = "projicio-test-garbage.gsb";
    let error = horizontal_shift_grid::register_bytes(garbage_name, &[7; 400])
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(garbage_name) && error.contains("not an NTv1, NTv2 or CTable2"),
        "{error}"
    );
    assert!(!horizontal_shift_grid::is_registered(garbage_name));

    let truncated_name = "projicio-test-truncated.gsb";
    let bytes = generated_ntv2();
    let error = horizontal_shift_grid::register_bytes(truncated_name, &bytes[..bytes.len() - 20])
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(truncated_name) && error.contains("file ends"),
        "{error}"
    );

    let tiff_name = "projicio-test.tif";
    let mut tiff = b"II*\0".to_vec();
    tiff.resize(400, 0);
    let error = horizontal_shift_grid::register_bytes(tiff_name, &tiff)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(tiff_name) && error.contains("GeoTIFF"),
        "{error}"
    );
}

#[test]
fn ntv2_without_a_usable_subfile_raises_at_registration_naming_the_grid() {
    let mut empty = Vec::new();
    ntv2_overview(&mut empty, 0);

    let empty_name = "projicio-test-no-subfiles.gsb";
    let error = horizontal_shift_grid::register_bytes(empty_name, &empty)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(empty_name) && error.contains("no subfiles"),
        "{error}"
    );

    let short_name = "projicio-test-short-overview.gsb";
    let error = horizontal_shift_grid::register_bytes(short_name, &empty[..64])
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(short_name) && error.contains("overview header"),
        "{error}"
    );

    let mut single_column = generated_ntv2();
    // ntv2 subfile records follow the 176 byte overview, 16 bytes each with the value 8 bytes in
    let value_offset = |record: usize| 176 + 16 * record + 8;
    let longitude_step = 5.0 * ARC_SECONDS_PER_DEGREE;
    single_column[value_offset(9)..value_offset(9) + 8]
        .copy_from_slice(&longitude_step.to_le_bytes());
    let node_count = (GENERATED.rows as i32).to_le_bytes();
    single_column[value_offset(10)..value_offset(10) + 4].copy_from_slice(&node_count);
    let single_column_name = "projicio-test-single-column.gsb";
    let error = horizontal_shift_grid::register_bytes(single_column_name, &single_column)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(single_column_name) && error.contains("no cell"),
        "{error}"
    );
    for name in [empty_name, short_name, single_column_name] {
        assert!(!horizontal_shift_grid::is_registered(name));
    }
}

#[test]
fn inverse_through_a_huge_shift_raises_outside_the_grid() {
    let name = "projicio-test-huge-shift.gsb";
    let bytes = ntv2_with_shifts(&GENERATED, |column, row| {
        (generated_shift(column, row).0, -3.0e38)
    });
    let transformation = registered_transformation(name, &bytes);
    let error = transformation
        .inverse(point(10.5, 50.5), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains(name) && error.contains("outside"), "{error}");
}

#[test]
fn infinite_forward_result_raises_outside_the_grid() {
    let name = "projicio-test-infinite-shift.gsb";
    let bytes = ntv2_with_shifts(&GENERATED, |column, row| {
        (generated_shift(column, row).0, f64::INFINITY)
    });
    let transformation = registered_transformation(name, &bytes);
    let error = transformation
        .forward(point(10.5, 50.5), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains(name) && error.contains("outside"), "{error}");
}

const FULL_WORLD: GeneratedGrid = GeneratedGrid {
    south: -90.0,
    west: -180.0,
    step: 90.0,
    columns: 5,
    rows: 3,
};

#[test]
fn infinite_longitude_raises_on_a_full_world_grid() {
    let name = "projicio-test-full-world.gsb";
    let transformation =
        registered_transformation(name, &ntv2_with_shifts(&FULL_WORLD, generated_shift));
    assert!(transformation.forward(point(30.0, 10.0), None).is_ok());

    let infinite = point(f64::INFINITY, 10.0);
    let error = transformation
        .inverse(infinite, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("not finite"), "{error}");
    let error = transformation
        .forward(infinite, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("out of range"), "{error}");
}

#[test]
fn deeply_nested_subgrids_resolve_to_the_deepest_and_drop() {
    const DEPTH: usize = 100_000;
    let deepest_shift = (5.0, 7.0);
    let mut bytes = Vec::new();
    ntv2_overview(&mut bytes, DEPTH as i32);
    for level in 0..DEPTH {
        let parent = match level {
            0 => "NONE".to_string(),
            _ => format!("S{}", level - 1),
        };
        let shift = if level == DEPTH - 1 {
            deepest_shift
        } else {
            (0.0, 0.0)
        };
        ntv2_subfile(
            &mut bytes,
            &format!("S{level}"),
            &parent,
            &GENERATED,
            |_, _| shift,
        );
    }
    let name = "projicio-test-deep-subgrids.gsb";
    let transformation = registered_transformation(name, &bytes);
    let shifted = transformation.forward(point(10.5, 50.5), None).unwrap();
    assert_degrees(
        "deepest subgrid",
        shifted,
        10.5 + deepest_shift.1 / ARC_SECONDS_PER_DEGREE,
        50.5 + deepest_shift.0 / ARC_SECONDS_PER_DEGREE,
        DEGREE_TOLERANCE,
    );

    let error = horizontal_shift_grid::register_bytes(name, &bytes)
        .unwrap_err()
        .to_string();
    assert!(error.contains("already registered"), "{error}");
}

#[test]
fn a_name_registers_once() {
    let name = "projicio-test-twice.gsb";
    horizontal_shift_grid::register_bytes(name, &generated_ntv2()).unwrap();
    let error = horizontal_shift_grid::register_bytes(name, &generated_ntv2())
        .unwrap_err()
        .to_string();
    assert!(error.contains("already registered"), "{error}");

    let error = horizontal_shift_grid::register_bytes("  ", &generated_ntv2())
        .unwrap_err()
        .to_string();
    assert!(error.contains("empty"), "{error}");
}

#[test]
fn missing_file_raises_naming_its_path() {
    let error =
        horizontal_shift_grid::register_file("projicio-test-missing.gsb", "/nonexistent/grid.gsb")
            .unwrap_err()
            .to_string();
    assert!(error.contains("/nonexistent/grid.gsb"), "{error}");
}

#[test]
fn methods_without_a_readable_grid_raise_naming_the_method() {
    const NADCON5_GRID_FILES: [(u32, &str); 2] = [
        (
            LATITUDE_DIFFERENCE_FILE,
            "nadcon5.nad27.nad83_1986.conus.lat.trn.20160901.b",
        ),
        (
            LONGITUDE_DIFFERENCE_FILE,
            "nadcon5.nad27.nad83_1986.conus.lon.trn.20160901.b",
        ),
    ];
    type GridFiles = &'static [(u32, &'static str)];
    let cases: [(u32, GridFiles, &str); 4] = [
        (NADCON5_2D, &NADCON5_GRID_FILES, "GeoTIFF"),
        (
            GEOCENTRIC_TRANSLATIONS_BY_GRID_IGN,
            &[(8727, "gr3df97a.txt")],
            "GeoTIFF",
        ),
        (9603, &[], "not a horizontal grid method"),
        (
            NADCON,
            &[(LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE, "conus.las")],
            "8657",
        ),
    ];
    for (method_code, grid_files, reason) in cases {
        let error = grid_transformation_from_epsg_method(method_code, grid_files)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains(&method_code.to_string()) && error.contains(reason),
            "{error}"
        );
    }
}
