use std::fs;
use std::path::{Path, PathBuf};

use projicio_core::Error;
use projicio_core::iso19111::Crs;
use projicio_core::iso19111::authority_factory::create_coordinate_reference_system;
use projicio_core::operation_factory::{
    AreaOfUse, CandidateOperation, CandidateOperations, GridAvailabilityUse, IntermediateCrsUse,
    OperationAuthority, OperationContext, SpatialCriterion, create_operations,
};

fn data(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/candidate_operations")
        .join(file)
}

fn read(file: &str) -> String {
    fs::read_to_string(data(file)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

fn crs(code: u32) -> Crs {
    create_coordinate_reference_system(code).unwrap_or_else(|e| panic!("EPSG:{code}: {e}"))
}

fn names(operations: &[CandidateOperation]) -> Vec<String> {
    operations
        .iter()
        .map(|operation| operation.name().to_string())
        .collect()
}

fn projinfo_names(file: &str) -> Vec<String> {
    // projinfo --summary prints "id, name, accuracy, area" after a count line
    read(file)
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with("Candidate operations found"))
        .map(|line| {
            line.split(", ")
                .nth(1)
                .unwrap_or_else(|| panic!("no name in {line:?}"))
                .to_string()
        })
        .collect()
}

fn assert_candidates_match_projinfo(source: u32, target: u32) {
    // printf the pair through projinfo -s EPSG:a -t EPSG:b --spatial-test intersects --grid-check known_available --summary with PROJ_DATA holding only proj.db
    let file = format!("projinfo/epsg_{source}_to_epsg_{target}.txt");
    let context = OperationContext::crs_to_crs(GridAvailabilityUse::KnownAvailable);
    let operations = create_operations(&crs(source), &crs(target), &context).unwrap();
    assert_eq!(names(&operations), projinfo_names(&file));
}

#[test]
fn nad83_to_wgs84_candidates_match_projinfo() {
    assert_candidates_match_projinfo(4269, 4326);
}

#[test]
fn nad27_to_wgs84_candidates_match_projinfo() {
    assert_candidates_match_projinfo(4267, 4326);
}

#[test]
fn ntf_paris_to_wgs84_candidates_match_projinfo() {
    assert_candidates_match_projinfo(4807, 4326);
}

#[test]
fn lambert_93_to_utm_31n_candidates_match_projinfo() {
    assert_candidates_match_projinfo(2154, 32631);
}

#[derive(Debug, Default)]
struct Expectations {
    count: Option<usize>,
    names: Vec<(usize, String)>,
    inverse_names: Vec<(usize, String)>,
    codes: Vec<(usize, u32)>,
    accuracies: Vec<(usize, String)>,
    ballpark: Vec<usize>,
}

struct Cursor<'a> {
    text: &'a str,
}

impl<'a> Cursor<'a> {
    fn skip_space(&mut self) {
        self.text = self.text.trim_start();
    }

    fn literals(&mut self) -> String {
        // c++ joins adjacent string literals
        let mut joined = String::new();
        loop {
            self.skip_space();
            let Some(rest) = self.text.strip_prefix('"') else {
                return joined;
            };
            let end = rest.find('"').expect("string literal is closed");
            joined.push_str(&rest[..end]);
            self.text = &rest[end + 1..];
        }
    }

    fn number(&mut self) -> &'a str {
        self.skip_space();
        let end = self
            .text
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(self.text.len());
        let digits = &self.text[..end];
        self.text = &self.text[end..];
        digits
    }
}

fn index_after(text: &str, prefix: &str) -> Option<(usize, String)> {
    let rest = text.strip_prefix(prefix)?;
    let end = rest.find(']')?;
    Some((rest[..end].parse().ok()?, rest[end + 1..].to_string()))
}

fn expectations(block: &str) -> Expectations {
    // reads PROJ's EXPECT_EQ and ASSERT_EQ lines on a list of operations
    let mut found = Expectations::default();
    let mut text = block;
    while let Some(position) = text.find("list") {
        text = &text[position..];
        if let Some(rest) = text.strip_prefix("list.size(),") {
            let preceding = &block[..block.len() - text.len()];
            let mut cursor = Cursor { text: rest };
            let count = cursor.number().parse().expect("a count");
            if preceding.trim_end().ends_with("ASSERT_EQ(") {
                found.count = Some(count);
            }
            text = cursor.text;
            continue;
        }
        let Some((index, rest)) = index_after(text, "list[") else {
            text = &text["list".len()..];
            continue;
        };
        let mut cursor = Cursor { text: &rest };
        if let Some(after) = rest.strip_prefix("->nameStr(),") {
            cursor.text = after;
            found.names.push((index, cursor.literals()));
        } else if let Some(after) = rest.strip_prefix("->inverse()->nameStr(),") {
            cursor.text = after;
            found.inverse_names.push((index, cursor.literals()));
        } else if let Some(after) = rest.strip_prefix("->getEPSGCode(),") {
            cursor.text = after;
            found
                .codes
                .push((index, cursor.number().parse().expect("a code")));
        } else if let Some(after) =
            rest.strip_prefix("->coordinateOperationAccuracies()[0]->value(),")
        {
            cursor.text = after;
            found.accuracies.push((index, cursor.literals()));
        } else if rest.starts_with("->hasBallparkTransformation()") {
            found.ballpark.push(index);
        }
        let consumed = rest.len() - cursor.text.len();
        let rest_offset = text.len() - rest.len();
        text = &text[rest_offset + consumed.max(1)..];
    }
    found
}

fn proj_test_blocks(test: &str) -> Vec<Expectations> {
    // each createOperations call in a test of PROJ's test_operationfactory.cpp opens a block of assertions
    let text = read(&format!("test_operationfactory/{test}.cpp"));
    text.split("createOperations(")
        .skip(1)
        .map(expectations)
        .collect()
}

fn epsg_code(operation: &CandidateOperation) -> Option<u32> {
    operation
        .operation()
        .identifiers()
        .iter()
        .find(|identifier| identifier.code_space == "EPSG")
        .and_then(|identifier| identifier.code.parse().ok())
}

fn assert_expectations(operations: &[CandidateOperation], expected: &Expectations) {
    let listed = names(operations);
    if let Some(count) = expected.count {
        assert_eq!(operations.len(), count, "{listed:#?}");
    }
    for (index, name) in &expected.names {
        assert_eq!(&listed[*index], name, "{listed:#?}");
    }
    for (index, name) in &expected.inverse_names {
        let inverse = operations[*index].inverse().unwrap();
        assert_eq!(inverse.name(), name);
    }
    for (index, code) in &expected.codes {
        assert_eq!(epsg_code(&operations[*index]), Some(*code), "{listed:#?}");
    }
    for (index, accuracy) in &expected.accuracies {
        assert_eq!(
            operations[*index].operation().accuracies(),
            std::slice::from_ref(accuracy),
            "{listed:#?}"
        );
    }
    for index in &expected.ballpark {
        assert!(
            operations[*index].has_ballpark_transformation(),
            "{listed:#?}"
        );
    }
}

fn epsg_context() -> OperationContext {
    OperationContext::new(OperationAuthority::Epsg)
}

fn check_proj_test(test: &str, pairs_and_contexts: &[(u32, u32, OperationContext)]) {
    let blocks = proj_test_blocks(test);
    assert_eq!(blocks.len(), pairs_and_contexts.len(), "{test}");
    for ((source, target, context), expected) in pairs_and_contexts.iter().zip(&blocks) {
        let operations = create_operations(&crs(*source), &crs(*target), context)
            .unwrap_or_else(|e| panic!("{test} EPSG:{source} to EPSG:{target}: {e}"));
        assert_expectations(&operations, expected);
    }
}

#[test]
fn excerpts_parse_into_assertions() {
    let blocks = proj_test_blocks("geogCRS_to_geogCRS_context_default");
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].count, Some(3));
    assert_eq!(blocks[0].codes, [(0, 15994), (1, 1644)]);
    assert_eq!(blocks[0].names.len(), 1);
    assert_eq!(blocks[1].names[0].0, 0);
}

#[test]
fn proj_geog_to_geog_context_default() {
    let context = OperationContext {
        spatial_criterion: SpatialCriterion::PartialIntersection,
        intermediate_crs_use: IntermediateCrsUse::Never,
        ..epsg_context()
    };
    check_proj_test(
        "geogCRS_to_geogCRS_context_default",
        &[(4179, 4258, context.clone()), (4258, 4179, context)],
    );
}

#[test]
fn proj_geog_to_geog_context_filter_accuracy() {
    let within = |desired_accuracy| OperationContext {
        desired_accuracy: Some(desired_accuracy),
        spatial_criterion: SpatialCriterion::PartialIntersection,
        ..epsg_context()
    };
    check_proj_test(
        "geogCRS_to_geogCRS_context_filter_accuracy",
        &[(4179, 4258, within(1.0)), (4179, 4258, within(0.9))],
    );
}

#[test]
fn proj_geog_to_geog_context_filter_bbox() {
    let around = |margin: f64| OperationContext {
        area_of_interest: Some(
            AreaOfUse::from_bounding_box(
                20.26 - margin,
                43.44 - margin,
                31.41 + margin,
                48.27 + margin,
            )
            .unwrap(),
        ),
        ..epsg_context()
    };
    check_proj_test(
        "geogCRS_to_geogCRS_context_filter_bbox",
        &[
            (4179, 4258, around(0.0)),
            (4179, 4258, around(-0.1)),
            (4179, 4258, around(0.1)),
        ],
    );
}

#[test]
fn proj_geog_to_geog_context_incompatible_area() {
    check_proj_test(
        "geogCRS_to_geogCRS_context_incompatible_area",
        &[(4267, 4258, epsg_context())],
    );
}

#[test]
fn proj_geog_to_geog_longitude_rotation_context() {
    check_proj_test(
        "geogCRS_to_geogCRS_longitude_rotation_context",
        &[(4807, 4275, epsg_context())],
    );
}

#[test]
fn proj_geog_to_geog_context_lonlat_vs_latlon_crs() {
    let context = OperationContext {
        grid_availability: GridAvailabilityUse::IgnoreGridAvailability,
        spatial_criterion: SpatialCriterion::PartialIntersection,
        ..epsg_context()
    };
    check_proj_test(
        "geogCRS_to_geogCRS_context_lonlat_vs_latlon_crs",
        &[(4749, 10310, context)],
    );
}

#[test]
fn proj_geog_to_geog_concatenated_operation_egypt_1907_to_wgs84() {
    check_proj_test(
        "geogCRS_to_geogCRS_context_concatenated_operation_Egypt1907_to_WGS84",
        &[(4229, 4326, epsg_context())],
    );
}

#[test]
fn proj_geog_to_geog_geographic_offset_context() {
    check_proj_test(
        "geogCRS_to_geogCRS_geographic_offset_context",
        &[(4120, 4121, epsg_context())],
    );
}

#[test]
fn proj_geographic_to_geocentric_same_datum_context() {
    check_proj_test(
        "geocentricCRS_to_geogCRS_same_datum_context",
        &[(4326, 4978, epsg_context())],
    );
}

#[test]
fn proj_geocentric_to_geocentric_different_datum_context() {
    check_proj_test(
        "geocentricCRS_to_geocentricCRS_different_datum_context",
        &[(4919, 4896, epsg_context())],
    );
}

#[test]
fn proj_geocentric_to_geographic_same_datum_context() {
    check_proj_test(
        "geogCRS_geocentricCRS_same_datum_to_context",
        &[(4978, 4326, epsg_context())],
    );
}

#[test]
fn proj_projected_to_projected_context_compatible_area_bis() {
    check_proj_test(
        "projCRS_to_projCRS_context_compatible_area_bis",
        &[(3844, 32634, epsg_context())],
    );
}

#[test]
fn proj_projected_to_projected_context_incompatible_areas() {
    check_proj_test(
        "projCRS_to_projCRS_context_incompatible_areas",
        &[(32631, 32633, epsg_context())],
    );
}

#[test]
fn proj_projected_to_projected_context_incompatible_areas_ballpark() {
    check_proj_test(
        "projCRS_to_projCRS_context_incompatible_areas_ballpark",
        &[(26711, 3034, epsg_context())],
    );
}

#[test]
fn proj_amersfoort_rd_new_to_wgs84() {
    check_proj_test(
        "transform_from_amersfoort_rd_new_to_epsg_4326",
        &[(28992, 4326, epsg_context())],
    );
}

fn numbers(line: &str) -> Vec<f64> {
    line.split_whitespace()
        .map(|value| value.parse().unwrap_or_else(|e| panic!("{value:?}: {e}")))
        .collect()
}

struct Cs2csCase {
    input: [f64; 3],
    epoch: Option<f64>,
    expected: Vec<f64>,
}

fn cs2cs_cases(pair: &str) -> Vec<Cs2csCase> {
    // printf the input | PROJ_DATA=<directory holding only proj.db> PROJ_NETWORK=OFF cs2cs -d 12 EPSG:a EPSG:b > the .cs2cs file
    let inputs = read(&format!("cs2cs/{pair}.input"));
    let outputs = read(&format!("cs2cs/{pair}.cs2cs"));
    inputs
        .lines()
        .zip(outputs.lines())
        .map(|(input, output)| {
            let input = numbers(input);
            Cs2csCase {
                input: [input[0], input[1], input.get(2).copied().unwrap_or(0.0)],
                epoch: input.get(3).copied(),
                expected: numbers(output),
            }
        })
        .collect()
}

fn assert_matches_cs2cs(operations: &CandidateOperations, pair: &str, tolerance: f64) {
    let cases = cs2cs_cases(pair);
    assert!(!cases.is_empty());
    for case in cases {
        let converted = operations
            .convert(case.input)
            .unwrap_or_else(|e| panic!("{pair} {:?}: {e}", case.input));
        for (value, expected) in converted.iter().zip(&case.expected).take(2) {
            let error = (value - expected).abs();
            assert!(
                error <= tolerance,
                "{pair} {:?}: got {converted:?}, cs2cs {:?}",
                case.input,
                case.expected
            );
        }
    }
}

// about a millimetre of latitude
const DEGREE_TOLERANCE: f64 = 1e-8;
const METRE_TOLERANCE: f64 = 1e-3;

fn epsg(source: u32, target: u32) -> CandidateOperations {
    CandidateOperations::from_epsg(source, target)
        .unwrap_or_else(|e| panic!("EPSG:{source} to EPSG:{target}: {e}"))
}

#[test]
fn nad83_to_wgs84_matches_cs2cs() {
    assert_matches_cs2cs(
        &epsg(4269, 4326),
        "epsg_4269_to_epsg_4326",
        DEGREE_TOLERANCE,
    );
}

#[test]
fn ntf_paris_in_grads_to_wgs84_matches_cs2cs() {
    assert_matches_cs2cs(
        &epsg(4807, 4326),
        "epsg_4807_to_epsg_4326",
        DEGREE_TOLERANCE,
    );
}

#[test]
fn etrs89_to_wgs84_matches_cs2cs() {
    assert_matches_cs2cs(
        &epsg(4258, 4326),
        "epsg_4258_to_epsg_4326",
        DEGREE_TOLERANCE,
    );
}

#[test]
fn british_national_grid_to_wgs84_matches_cs2cs() {
    assert_matches_cs2cs(
        &epsg(27700, 4326),
        "epsg_27700_to_epsg_4326",
        DEGREE_TOLERANCE,
    );
}

#[test]
fn lambert_93_to_utm_31n_matches_cs2cs() {
    assert_matches_cs2cs(
        &epsg(2154, 32631),
        "epsg_2154_to_epsg_32631",
        METRE_TOLERANCE,
    );
}

#[test]
fn ed50_to_mgi_through_a_pivot_matches_cs2cs() {
    let operations = epsg(4230, 4312);
    assert!(
        operations
            .operations()
            .all(|operation| operation.name().contains(" + ")),
        "{:?}",
        operations
            .operations()
            .map(CandidateOperation::name)
            .collect::<Vec<_>>()
    );
    assert_matches_cs2cs(&operations, "epsg_4230_to_epsg_4312", DEGREE_TOLERANCE);
}

#[test]
fn itrf2014_to_etrf2014_at_an_epoch_matches_cs2cs() {
    let (source, target) = (crs(9000), crs(9069));
    let cases = cs2cs_cases("epsg_9000_to_epsg_9069");
    let epoch = cases[0].epoch.expect("the input carries an epoch");
    let operations = CandidateOperations::at_epoch(&source, &target, epoch).unwrap();
    assert_matches_cs2cs(&operations, "epsg_9000_to_epsg_9069", DEGREE_TOLERANCE);
    assert!(
        operations
            .operations()
            .any(|operation| operation.name().contains("ITRF2014 to ETRF2014"))
    );
}

#[test]
fn time_dependent_operations_need_an_epoch() {
    let without_epoch = CandidateOperations::new(&crs(9000), &crs(9069));
    let offers_time_dependent = without_epoch.is_ok_and(|operations| {
        operations
            .operations()
            .any(|operation| operation.name().contains("ITRF2014 to ETRF2014"))
    });
    assert!(!offers_time_dependent);
}

#[test]
fn point_outside_every_operation_raises_unless_ballpark_is_allowed() {
    let operations = epsg(4269, 4326);
    let case = &cs2cs_cases("epsg_4269_to_epsg_4326_outside")[0];
    assert!(matches!(
        operations.convert(case.input),
        Err(Error::NoOperation { .. })
    ));
    let converted = operations.convert_allowing_ballpark(case.input).unwrap();
    for (value, expected) in converted.iter().zip(&case.expected).take(2) {
        assert!((value - expected).abs() <= DEGREE_TOLERANCE);
    }
}
