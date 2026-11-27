mod gie;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use gie::{
    AxisTable, Case, Check, Coordinate, Direction, Failure, Outcome, SUITES, Setup, SkipReason,
    gie_files, parse, run,
};

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn axes() -> AxisTable {
    AxisTable::load(&data().join("crs_axes.tsv")).unwrap()
}

fn cases_of(file: &str) -> Vec<Case> {
    let text = fs::read_to_string(data().join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
    parse(&text).unwrap_or_else(|e| panic!("{file}: {e}"))
}

fn case_at(cases: &[Case], line: usize) -> &Case {
    cases
        .iter()
        .find(|case| case.line == line)
        .unwrap_or_else(|| panic!("no expect or roundtrip at line {line}"))
}

fn coordinate(values: &[f64]) -> Coordinate {
    let mut padded = [0.0; 4];
    padded[..values.len()].copy_from_slice(values);
    Coordinate {
        values: padded,
        dimensions: values.len(),
    }
}

#[test]
fn every_copied_file_parses_and_its_epsg_codes_have_axes() {
    let axes = axes();
    for (_, directory) in SUITES {
        let names = gie_files(&data().join(directory)).unwrap();
        assert!(!names.is_empty(), "{directory} has no gie files");
        for name in names {
            let file = format!("{directory}/{name}");
            let cases = cases_of(&file);
            assert!(!cases.is_empty(), "{file} has no cases");
            for case in &cases {
                let (Setup::CrsPair { source, target } | Setup::InitPair { source, target }) =
                    case.setup
                else {
                    continue;
                };
                for code in [source, target] {
                    assert!(
                        axes.get(code).is_some(),
                        "{file} line {}: EPSG:{code} is missing from crs_axes.tsv",
                        case.line
                    );
                }
            }
        }
    }
}

#[test]
fn crs_pair_reads_coordinates_in_crs_axis_order_and_tolerance_in_millimetres() {
    let cases = cases_of("proj-gie/epsg_no_grid.gie");
    let case = case_at(&cases, 16);
    assert_eq!(
        case.setup,
        Setup::CrsPair {
            source: 4258,
            target: 25832
        }
    );
    assert!((case.tolerance - 1e-4).abs() < 1e-15, "{}", case.tolerance);
    assert_eq!(case.direction, Direction::Forward);
    assert_eq!(case.accept, coordinate(&[55.0, 12.0]));
    assert_eq!(
        case.check,
        Check::Coordinate(coordinate(&[691875.6321, 6098907.825]))
    );
    assert!(case.required_grids.is_empty());

    let finland = case_at(&cases, 59);
    assert_eq!(
        finland.setup,
        Setup::CrsPair {
            source: 4123,
            target: 2393
        }
    );
}

#[test]
fn expect_before_its_accept_reuses_the_previous_accept() {
    let cases = cases_of("proj-gie/epsg_no_grid.gie");
    let case = case_at(&cases, 29);
    assert_eq!(case.accept, coordinate(&[55.0, 12.0, 10.0]));
    assert!(matches!(
        run(case, &axes()),
        Outcome::Skipped(SkipReason::ThirdCoordinate)
    ));
}

#[test]
fn nonzero_height_or_time_is_skipped() {
    let cases = cases_of("proj-gie/epsg_no_grid.gie");
    let axes = axes();
    let with_time = case_at(&cases, 41);
    assert_eq!(
        with_time.accept,
        coordinate(&[-33.8623, 151.2077, 0.0, 2020.0])
    );
    assert!(matches!(
        run(with_time, &axes),
        Outcome::Skipped(SkipReason::Time)
    ));
    assert!(matches!(
        run(case_at(&cases, 47), &axes),
        Outcome::Skipped(SkipReason::ThirdCoordinate)
    ));
}

#[test]
fn require_grid_applies_until_the_next_crs_pair() {
    let cases = cases_of("proj-gie/epsg_grid.gie");
    let axes = axes();
    assert_eq!(case_at(&cases, 18).required_grids, ["us_nga_egm08_25.tif"]);
    assert_eq!(
        case_at(&cases, 32).required_grids,
        ["be_ign_bd72lb72_etrs89lb08.tif"]
    );
    let catalonia = case_at(&cases, 72);
    assert_eq!(catalonia.required_grids, ["es_cat_icgc_100800401.tif"]);
    assert!(matches!(
        run(catalonia, &axes),
        Outcome::Skipped(SkipReason::RequiredGridNotRegistered)
    ));
}

#[test]
fn strict_continuation_lines_join_into_one_init_pipeline() {
    let cases = cases_of("gigs/5201.gie");
    let case = case_at(&cases, 24);
    assert_eq!(
        case.setup,
        Setup::InitPair {
            source: 4978,
            target: 4326
        }
    );
    assert_eq!(case.tolerance, 0.01);
    assert_eq!(
        case.accept,
        coordinate(&[-962479.5924, 555687.8517, 6260738.6526])
    );
    assert_eq!(
        case.check,
        Check::Coordinate(coordinate(&[150.0, 80.0, 1214.137]))
    );
    assert!(matches!(
        run(case, &axes()),
        Outcome::Skipped(SkipReason::ThirdCoordinate)
    ));

    assert_eq!(
        case_at(&cases, 250).check,
        Check::Roundtrip {
            trips: 1000,
            tolerance: 0.01
        }
    );
}

#[test]
fn loose_block_operation_runs_until_the_decoration_line() {
    let cases = cases_of("gigs/5204.1.gie.failing");
    let case = case_at(&cases, 16);
    assert_eq!(
        case.setup,
        Setup::InitPair {
            source: 4313,
            target: 4326
        }
    );
    assert_eq!(case.tolerance, 2.77777777778e-07);
    assert_eq!(case.accept, coordinate(&[149.99825389, 80.00155167]));
    assert_eq!(case.check, Check::Coordinate(coordinate(&[150.0, 80.0])));
}

#[test]
fn direction_and_expected_failures_follow_gie() {
    let cases = cases_of("proj-gie/more_builtins.gie");

    let inverse = case_at(&cases, 22);
    assert_eq!(inverse.setup, Setup::ProjString);
    assert_eq!(inverse.direction, Direction::Inverse);
    assert_eq!(inverse.tolerance, 0.001);
    assert_eq!(inverse.accept, coordinate(&[300000.0, 400000.0]));
    assert_eq!(
        inverse.check,
        Check::Coordinate(coordinate(&[-42.7562158333, 85.5911341667]))
    );
    assert_eq!(case_at(&cases, 27).direction, Direction::Forward);

    assert_eq!(
        case_at(&cases, 78).check,
        Check::Failure {
            errno: Some("invalid_op_missing_arg".to_string())
        }
    );
    // gie counts an error code only after the word errno
    assert_eq!(case_at(&cases, 209).check, Check::Failure { errno: None });

    let reverse = case_at(&cases, 756);
    assert_eq!(reverse.direction, Direction::Inverse);
    assert_eq!(reverse.accept, coordinate(&[0.0, 0.0, 0.0, 0.0]));
    assert_eq!(reverse.check, Check::Failure { errno: None });
}

#[test]
fn roundtrip_takes_a_count_and_an_optional_tolerance() {
    let cases = cases_of("proj-gie/more_builtins.gie");
    assert_eq!(
        case_at(&cases, 44).check,
        Check::Coordinate(coordinate(&[144.968, -37.79848, 46.378, 0.0]))
    );
    assert_eq!(
        case_at(&cases, 46).check,
        Check::Roundtrip {
            trips: 100,
            tolerance: 1.0
        }
    );
    assert_eq!(
        case_at(&cases, 702).check,
        Check::Roundtrip {
            trips: 1,
            tolerance: 1.0
        }
    );
}

#[test]
fn tolerance_unit_glued_to_the_number_reads_as_metres() {
    let cases = cases_of("proj-gie/more_builtins.gie");
    let case = case_at(&cases, 701);
    assert_eq!(case.tolerance, 1.0);
    assert!(matches!(
        run(case, &axes()),
        Outcome::Skipped(SkipReason::ProjString)
    ));
}

#[test]
fn degrees_minutes_seconds_coordinates_parse() {
    let cases = cases_of("proj-gie/more_builtins.gie");
    assert_eq!(
        case_at(&cases, 320).accept,
        coordinate(&[-(83.0 + 10.0 / 60.0), 43.75])
    );
}

const WEB_MERCATOR: &str = "\
<gie>
crs_src EPSG:4326
crs_dst EPSG:3857
accept 10 20
expect 2226389.8158654715 1118889.9748579597
expect 2226389.8158654715 1118989.9748579597
crs_src EPSG:3857
crs_dst EPSG:4326
accept 2226389.8158654715 1118889.9748579597
expect 10 20
expect 20 10
</gie>
";

#[test]
fn runner_converts_and_compares_in_each_crs_axis_order() {
    let cases = parse(WEB_MERCATOR).unwrap();
    let axes = axes();
    let outcome = |line| run(case_at(&cases, line), &axes);

    let forward = outcome(5);
    assert!(matches!(forward, Outcome::Passed), "{forward:?}");
    match outcome(6) {
        Outcome::Failed(Failure::Outside { error }) => {
            assert!((error - 100.0).abs() < 1e-6, "{error}");
        }
        other => panic!("{other:?}"),
    }

    let inverse = outcome(10);
    assert!(matches!(inverse, Outcome::Passed), "{inverse:?}");
    match outcome(11) {
        Outcome::Failed(Failure::Outside { error }) => assert!(error > 1e6, "{error}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_listed_case_still_passes() {
    let axes = axes();
    let manifest = fs::read_to_string(data().join("gie_passing.txt")).unwrap();
    let mut files: HashMap<String, Vec<Case>> = HashMap::new();
    let mut regressions = Vec::new();
    for entry in manifest
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (file, line) = entry
            .split_once(' ')
            .unwrap_or_else(|| panic!("want `file line`, got {entry:?}"));
        let line: usize = line.parse().unwrap_or_else(|e| panic!("{entry:?}: {e}"));
        let cases = files
            .entry(file.to_string())
            .or_insert_with(|| cases_of(file));
        let case = case_at(cases, line);
        match run(case, &axes) {
            Outcome::Passed => {}
            Outcome::Failed(failure) => regressions.push(format!(
                "{file} line {line}: {failure}, tolerance {:.3e}",
                case.tolerance
            )),
            Outcome::Skipped(reason) => regressions.push(format!(
                "{file} line {line}: skipped for {}",
                reason.label()
            )),
        }
    }
    assert!(
        regressions.is_empty(),
        "{} listed cases no longer pass:\n{}",
        regressions.len(),
        regressions.join("\n")
    );
}
