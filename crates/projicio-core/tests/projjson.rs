use projicio_core::iso19111::common::UnitOfMeasure;
use projicio_core::iso19111::coordinate_system::{AxisDirection, EllipsoidalCs};
use projicio_core::iso19111::datum::GeodeticDatum;
use projicio_core::iso19111::operation::ParameterValue;
use projicio_core::iso19111::{Criterion, Crs};
use projicio_core::{Ellipsoid, Error};
use serde_json::Value;

// projinfo -q -o PROJJSON EPSG:<code>, with the code in the file name
const EPSG_FIXTURES: [(&str, &str); 14] = [
    ("EPSG:4326", include_str!("data/projjson/epsg_4326.json")),
    ("EPSG:4269", include_str!("data/projjson/epsg_4269.json")),
    ("EPSG:4258", include_str!("data/projjson/epsg_4258.json")),
    ("EPSG:4979", include_str!("data/projjson/epsg_4979.json")),
    ("EPSG:4978", include_str!("data/projjson/epsg_4978.json")),
    ("EPSG:4807", include_str!("data/projjson/epsg_4807.json")),
    ("EPSG:32632", include_str!("data/projjson/epsg_32632.json")),
    ("EPSG:2154", include_str!("data/projjson/epsg_2154.json")),
    ("EPSG:27700", include_str!("data/projjson/epsg_27700.json")),
    ("EPSG:3857", include_str!("data/projjson/epsg_3857.json")),
    ("EPSG:5514", include_str!("data/projjson/epsg_5514.json")),
    ("EPSG:2065", include_str!("data/projjson/epsg_2065.json")),
    ("EPSG:6318", include_str!("data/projjson/epsg_6318.json")),
    ("EPSG:7912", include_str!("data/projjson/epsg_7912.json")),
];

// projinfo -q -o PROJJSON OGC:CRS84
const OGC_CRS84: &str = include_str!("data/projjson/ogc_crs84.json");

// projinfo -q -o PROJJSON "+proj=lcc +lat_0=42.5 +lon_0=-100 +lat_1=25 +lat_2=60 +datum=WGS84 +type=crs"
const CODELESS_LCC: &str = include_str!("data/projjson/lcc_wgs84.json");

// projinfo -q -o PROJJSON "+proj=longlat +ellps=intl +towgs84=-87,-98,-121 +type=crs"
const TOWGS84_BOUND_CRS: &str = include_str!("data/projjson/longlat_intl_towgs84.json");

// projinfo -q -o PROJJSON "+proj=longlat +ellps=clrk80ign +pm=paris +towgs84=-168,-60,320 +type=crs"
const PARIS_TOWGS84_BOUND_CRS: &str = include_str!("data/projjson/longlat_paris_towgs84.json");

// projinfo -q -o PROJJSON @longlat_paris_towgs84.json
const PARIS_TOWGS84_BOUND_CRS_REREAD: &str =
    include_str!("data/projjson/longlat_paris_towgs84_reread.json");

fn all_fixtures() -> Vec<(&'static str, &'static str)> {
    let mut fixtures = EPSG_FIXTURES.to_vec();
    fixtures.push(("OGC:CRS84", OGC_CRS84));
    fixtures.push(("codeless LCC", CODELESS_LCC));
    fixtures.push(("+towgs84 BoundCRS", TOWGS84_BOUND_CRS));
    fixtures.push(("reread Paris BoundCRS", PARIS_TOWGS84_BOUND_CRS_REREAD));
    fixtures
}

fn parse(name: &str, projjson: &str) -> Crs {
    Crs::from_projjson(projjson).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn fixture(name: &str) -> Crs {
    let (_, projjson) = all_fixtures()
        .into_iter()
        .find(|(fixture_name, _)| *fixture_name == name)
        .unwrap_or_else(|| panic!("no fixture named {name}"));
    parse(name, projjson)
}

fn json_value(text: &str) -> Value {
    serde_json::from_str(text).expect("valid JSON")
}

#[test]
fn every_fixture_writes_back_the_json_projinfo_wrote() {
    for (name, projjson) in all_fixtures() {
        let written = parse(name, projjson).to_projjson();
        assert_eq!(json_value(&written), json_value(projjson), "{name}");
    }
}

#[test]
fn every_fixture_writes_back_projinfo_text_byte_for_byte() {
    for (name, projjson) in all_fixtures() {
        let written = parse(name, projjson).to_projjson();
        assert_eq!(written, projjson.trim_end_matches('\n'), "{name}");
    }
}

#[test]
fn paris_bound_crs_gains_the_greenwich_source_projinfo_adds_on_reading() {
    let written = parse("Paris BoundCRS", PARIS_TOWGS84_BOUND_CRS).to_projjson();
    assert_eq!(
        json_value(&written),
        json_value(PARIS_TOWGS84_BOUND_CRS_REREAD)
    );
}

#[test]
fn nad83_is_not_wgs84() {
    let nad83 = fixture("EPSG:4269");
    let wgs84 = fixture("EPSG:4326");
    let Crs::Geographic(nad83_geographic) = &nad83 else {
        panic!("EPSG:4269 is geographic");
    };
    let Crs::Geographic(wgs84_geographic) = &wgs84 else {
        panic!("EPSG:4326 is geographic");
    };
    assert!(matches!(
        &nad83_geographic.datum,
        GeodeticDatum::ReferenceFrame(frame) if frame.name == "North American Datum 1983"
    ));
    assert!(matches!(
        &wgs84_geographic.datum,
        GeodeticDatum::Ensemble(ensemble) if ensemble.identifiers[0].code == "6326"
    ));
    assert!(!nad83.is_equivalent_to(&wgs84, Criterion::Equivalent));
    assert!(!nad83.is_equivalent_to(&wgs84, Criterion::EquivalentExceptAxisOrderGeogcrs));
}

#[test]
fn wgs84_is_equivalent_to_its_own_round_trip() {
    let wgs84 = fixture("EPSG:4326");
    let round_trip = parse("EPSG:4326 round trip", &wgs84.to_projjson());
    assert_eq!(round_trip, wgs84);
    assert!(wgs84.is_equivalent_to(&round_trip, Criterion::Equivalent));
}

#[test]
fn crs84_differs_from_4326_only_by_axis_order() {
    let wgs84 = fixture("EPSG:4326");
    let crs84 = fixture("OGC:CRS84");
    assert!(!wgs84.is_equivalent_to(&crs84, Criterion::Equivalent));
    assert!(wgs84.is_equivalent_to(&crs84, Criterion::EquivalentExceptAxisOrderGeogcrs));
}

#[test]
fn krovak_south_west_and_east_north_are_not_equivalent() {
    let krovak_ferro = fixture("EPSG:2065");
    let krovak_east_north = fixture("EPSG:5514");
    assert!(!krovak_ferro.is_equivalent_to(&krovak_east_north, Criterion::Equivalent));
    assert!(!krovak_ferro.is_equivalent_to(
        &krovak_east_north,
        Criterion::EquivalentExceptAxisOrderGeogcrs
    ));
}

#[test]
fn projected_axis_order_matters_under_both_criteria_but_base_axis_order_does_not() {
    let krovak = fixture("EPSG:2065");
    let Crs::Projected(projected) = &krovak else {
        panic!("EPSG:2065 is projected");
    };

    let mut east_north = projected.clone();
    east_north.coordinate_system.axes[0].direction = AxisDirection::East;
    east_north.coordinate_system.axes[1].direction = AxisDirection::North;
    let east_north = Crs::Projected(east_north);
    assert!(!krovak.is_equivalent_to(&east_north, Criterion::Equivalent));
    assert!(!krovak.is_equivalent_to(&east_north, Criterion::EquivalentExceptAxisOrderGeogcrs));

    let mut base_longitude_latitude = projected.clone();
    base_longitude_latitude.base_crs.coordinate_system =
        EllipsoidalCs::longitude_latitude(&UnitOfMeasure::degree());
    let base_longitude_latitude = Crs::Projected(base_longitude_latitude);
    assert!(!krovak.is_equivalent_to(&base_longitude_latitude, Criterion::Equivalent));
    assert!(krovak.is_equivalent_to(
        &base_longitude_latitude,
        Criterion::EquivalentExceptAxisOrderGeogcrs
    ));
}

#[test]
fn towgs84_bound_crs_carries_a_geocentric_translation_to_wgs84() {
    let Crs::Bound(bound) = parse("+towgs84 BoundCRS", TOWGS84_BOUND_CRS) else {
        panic!("+towgs84 parses to a BoundCRS");
    };
    assert!(
        bound
            .target_crs
            .is_equivalent_to(&fixture("EPSG:4326"), Criterion::Equivalent)
    );
    assert_eq!(bound.transformation.method.identifiers[0].code, "9603");
    let translations: Vec<f64> = bound
        .transformation
        .parameter_values
        .iter()
        .map(|parameter_value| match &parameter_value.value {
            ParameterValue::Measure(measure) => measure.si_value(),
            other => panic!("translation is a measure, got {other:?}"),
        })
        .collect();
    assert_eq!(translations, [-87.0, -98.0, -121.0]);
}

#[test]
fn model_ellipsoid_converts_to_the_projection_ellipsoid() {
    let Crs::Geographic(wgs84) = fixture("EPSG:4326") else {
        panic!("EPSG:4326 is geographic");
    };
    assert_eq!(Ellipsoid::from(wgs84.datum.ellipsoid()), Ellipsoid::WGS84);
}

#[test]
fn unsupported_crs_types_raise() {
    let vertical = r#"{"type": "VerticalCRS", "name": "EGM96 height"}"#;
    let err = Crs::from_projjson(vertical).unwrap_err();
    assert!(matches!(err, Error::InvalidProjJson(_)), "{err}");
    assert!(err.to_string().contains("VerticalCRS"), "{err}");
}
