#![cfg(feature = "registry")]

use projicio_core::{GeodeticCoord, grid_transformation_from_epsg_method, horizontal_shift_grid};

const NADCON: u32 = 9613;
const NTV1: u32 = 9614;
const NTV2: u32 = 9615;
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

fn rows(code: u32, kind: &str) -> Vec<[f64; 4]> {
    let text = std::fs::read_to_string(data_path(&format!("epsg_{code}.{kind}.txt"))).unwrap();
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

struct EpsgCase<'a> {
    code: u32,
    method_code: u32,
    // the operation's grid files as the registry holds them
    grid_files: &'a [(u32, &'a str)],
    // proj.db's old_proj_grid_name, the file name of the grid in its NTv1, NTv2 or CTable2 form
    proj_file: &'a str,
}

fn check(case: EpsgCase) {
    horizontal_shift_grid::register_file(case.proj_file, data_path(case.proj_file)).unwrap();
    let transformation =
        grid_transformation_from_epsg_method(case.method_code, case.grid_files).unwrap();
    let input = rows(case.code, "input");
    let forward = rows(case.code, "cct_forward");
    let inverse = rows(case.code, "cct_inverse");
    assert!(!input.is_empty() && input.len() == forward.len() && input.len() == inverse.len());
    // cct rows are latitude then longitude in degrees
    let point = |row: &[f64; 4]| GeodeticCoord {
        longitude: row[1].to_radians(),
        latitude: row[0].to_radians(),
        height: row[2],
    };
    let assert_row = |direction: &str, row: usize, got: GeodeticCoord, expected: &[f64; 4]| {
        let longitude = got.longitude.to_degrees();
        let latitude = got.latitude.to_degrees();
        assert!(
            (longitude - expected[1]).abs() < DEGREE_TOLERANCE
                && (latitude - expected[0]).abs() < DEGREE_TOLERANCE,
            "EPSG:{} {direction} row {row}: got latitude {latitude} longitude {longitude}, cct {expected:?}",
            case.code
        );
    };
    for (row, (given, expected)) in input.iter().zip(&forward).enumerate() {
        let got = transformation.forward(point(given), None).unwrap();
        assert_row("forward", row, got, expected);
    }
    for (row, (given, expected)) in forward.iter().zip(&inverse).enumerate() {
        let got = transformation.inverse(point(given), None).unwrap();
        assert_row("inverse", row, got, expected);
    }
}

#[test]
fn ntv2_ntf_to_rgf93_runs_proj_file_in_reverse() {
    check(EpsgCase {
        code: 15940,
        method_code: NTV2,
        grid_files: &[(LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE, "rgf93_ntf.gsb")],
        proj_file: "ntf_r93.gsb",
    });
}

#[test]
fn nadcon_nad27_to_nad83_conus() {
    check(EpsgCase {
        code: 1241,
        method_code: NADCON,
        grid_files: &[
            (LATITUDE_DIFFERENCE_FILE, "conus.las"),
            (LONGITUDE_DIFFERENCE_FILE, "conus.los"),
        ],
        proj_file: "conus",
    });
}

#[test]
fn ntv1_nad27_to_nad83_canada() {
    check(EpsgCase {
        code: 1312,
        method_code: NTV1,
        grid_files: &[(LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE, "NTv1_0.gsb")],
        proj_file: "ntv1_can.dat",
    });
}
