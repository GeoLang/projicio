// Codes proj4rs cannot build, transformed by projicio's own math from the
// parameters in their embedded proj4 definition.
//
// The known-value cases pair the grid with a geographic CRS on the same datum,
// so the published numbers are the projection alone with no datum shift.

use projicio_core::{Support, Transform, epsg, projstring};

/// Degrees from a degree/minute/second triple, so test inputs can be written the
/// way the source documents print them.
fn dms(d: f64, m: f64, s: f64) -> f64 {
    d.signum() * (d.abs() + m / 60.0 + s / 3600.0)
}

// ═══════════════════════════════════════════════════════════════════════════
// Known-value transforms, published sources
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_trinidad_grid_30200_iogp_worked_example() {
    // IOGP Geomatics Guidance Note 7 part 2 (373-07-02, September 2019) pages 42-43,
    // Cassini-Soldner worked example for Trinidad 1903 / Trinidad Grid:
    // lat 10 deg 00' 00" N, lon 62 deg 00' 00" W gives E 66644.94 links,
    // N 82536.22 links. https://www.iogp.org/wp-content/uploads/2019/09/373-07-02.pdf
    // EPSG:4302 is Trinidad 1903 geographic, the same datum as EPSG:30200.
    let t = Transform::new("EPSG:4302", "EPSG:30200").unwrap();
    let (e, n) = t.convert(-62.0, 10.0).unwrap();
    assert!((e - 66_644.94).abs() < 0.01, "easting {e}");
    assert!((n - 82_536.22).abs() < 0.01, "northing {n}");
}

#[test]
fn test_rso_borneo_29873_iogp_worked_example() {
    // IOGP Geomatics Guidance Note 7 part 2 (373-07-02, September 2019) pages 60-62,
    // Hotine Oblique Mercator variant B worked example for Timbalai 1948 / RSO Borneo:
    // lat 5 deg 23' 14.1129" N, lon 115 deg 48' 19.8196" E gives E 679245.73 m,
    // N 596562.78 m. https://www.iogp.org/wp-content/uploads/2019/09/373-07-02.pdf
    // EPSG:4298 is Timbalai 1948 geographic, the same datum as EPSG:29873.
    let t = Transform::new("EPSG:4298", "EPSG:29873").unwrap();
    let (e, n) = t
        .convert(dms(115.0, 48.0, 19.8196), dms(5.0, 23.0, 14.1129))
        .unwrap();
    assert!((e - 679_245.73).abs() < 0.02, "easting {e}");
    assert!((n - 596_562.78).abs() < 0.02, "northing {n}");
}

#[test]
fn test_rso_borneo_29873_iogp_worked_example_inverse() {
    let t = Transform::new("EPSG:29873", "EPSG:4298").unwrap();
    let (lon, lat) = t.convert(679_245.73, 596_562.78).unwrap();
    assert!(
        (lon - dms(115.0, 48.0, 19.8196)).abs() < 1e-7,
        "longitude {lon}"
    );
    assert!(
        (lat - dms(5.0, 23.0, 14.1129)).abs() < 1e-7,
        "latitude {lat}"
    );
}

#[test]
fn test_grid_origins_are_exact() {
    // Each grid puts a stated easting and northing at its own natural origin or
    // projection centre, so those pairs hold by definition and catch a units,
    // radians or false-origin slip at the engine boundary.
    // https://epsg.io/3377, https://epsg.io/29873
    let cassini = Transform::new("EPSG:4742", "EPSG:3377").unwrap();
    let (e, n) = cassini
        .convert(103.427_936_236_111_1, 2.121_679_744_444_445)
        .unwrap();
    assert!((e - -14_810.562).abs() < 1e-4, "easting {e}");
    assert!((n - 8_758.32).abs() < 1e-4, "northing {n}");

    let hotine = Transform::new("EPSG:4298", "EPSG:29873").unwrap();
    let (e, n) = hotine.convert(115.0, 4.0).unwrap();
    assert!((e - 590_476.87).abs() < 1e-4, "easting {e}");
    assert!((n - 442_857.65).abs() < 1e-4, "northing {n}");
}

// ═══════════════════════════════════════════════════════════════════════════
// Which engine each pair resolves to
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_pairs_of_definition_built_codes_stay_native() {
    for (from, to) in [
        ("EPSG:3377", "EPSG:3378"),
        ("EPSG:30200", "EPSG:2314"),
        ("EPSG:3376", "EPSG:3375"),
        ("EPSG:30200", "EPSG:3376"),
    ] {
        let t = Transform::new(from, to).unwrap();
        assert_eq!(t.path(), Support::Native, "{from} -> {to}");
    }
}

#[test]
fn test_definition_built_code_with_a_hand_written_code_stays_native() {
    for (from, to) in [
        ("EPSG:4326", "EPSG:30200"),
        ("EPSG:30200", "EPSG:4326"),
        ("EPSG:32620", "EPSG:30200"),
        ("EPSG:3376", "EPSG:3857"),
    ] {
        let t = Transform::new(from, to).unwrap();
        assert_eq!(t.path(), Support::Native, "{from} -> {to}");
    }
}

#[test]
fn test_definition_built_code_with_a_proj4rs_code_reports_fallback() {
    // proj4rs serves one of the two sides, so the pair is not projicio math end
    // to end and must not claim to be.
    for (from, to) in [("EPSG:30200", "EPSG:27700"), ("EPSG:4302", "EPSG:30200")] {
        let t = Transform::new(from, to).unwrap();
        assert_eq!(t.path(), Support::Fallback, "{from} -> {to}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Datum handling across the WGS84 hub
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_hub_applies_the_datum_shift_once() {
    // EPSG:2314 is the same Trinidad grid as EPSG:30200 in a different unit and
    // on the same datum, so a pair of them is a pure unit change. A hub that
    // shifted twice, or shifted one side only, would not come back.
    const CLARKE_LINK: f64 = 0.201_166_195_164;
    const CLARKE_FOOT: f64 = 0.304_797_265_4;
    let t = Transform::new("EPSG:30200", "EPSG:2314").unwrap();
    let (e, n) = t.convert(66_644.94, 82_536.22).unwrap();
    assert!(
        (e * CLARKE_FOOT - 66_644.94 * CLARKE_LINK).abs() < 0.001,
        "easting {e}"
    );
    assert!(
        (n * CLARKE_FOOT - 82_536.22 * CLARKE_LINK).abs() < 0.001,
        "northing {n}"
    );
}

#[test]
fn test_datum_shift_moves_the_coordinate() {
    // Trinidad 1903 is roughly 500 m from WGS84, so the same grid coordinate read
    // on its own datum and on WGS84 must land far apart. A silently skipped shift
    // would make these identical.
    let own_datum = Transform::new("EPSG:30200", "EPSG:4302").unwrap();
    let wgs84 = Transform::new("EPSG:30200", "EPSG:4326").unwrap();
    let (lon_a, lat_a) = own_datum.convert(66_644.94, 82_536.22).unwrap();
    let (lon_b, lat_b) = wgs84.convert(66_644.94, 82_536.22).unwrap();
    let separation = ((lon_a - lon_b).powi(2) + (lat_a - lat_b).powi(2)).sqrt() * 111_320.0;
    assert!(
        (100.0..1_000.0).contains(&separation),
        "shift of {separation} m"
    );
}

#[test]
fn test_definition_without_a_datum_is_not_shifted() {
    // EPSG:3376 names no datum, and proj skips the shift for a pair where either
    // side does not, so its coordinates reach WGS84 unmoved. EPSG:4742 (GDM2000)
    // also names no shift, so the two must agree exactly.
    let to_wgs84 = Transform::new("EPSG:3376", "EPSG:4326").unwrap();
    let to_own = Transform::new("EPSG:3376", "EPSG:4742").unwrap();
    let (lon_a, lat_a) = to_wgs84.convert(500_000.0, 500_000.0).unwrap();
    let (lon_b, lat_b) = to_own.convert(500_000.0, 500_000.0).unwrap();
    assert!((lon_a - lon_b).abs() < 1e-9, "{lon_a} vs {lon_b}");
    assert!((lat_a - lat_b).abs() < 1e-9, "{lat_a} vs {lat_b}");
}

// ═══════════════════════════════════════════════════════════════════════════
// Roundtrips
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_roundtrips_through_the_definition_path() {
    // Each case is a grid paired with a geographic CRS on its own datum, so the
    // roundtrip exercises the projection rather than the datum machinery.
    let cases: &[(&str, &str, f64, f64)] = &[
        ("EPSG:4302", "EPSG:30200", -61.5, 10.6),
        ("EPSG:4742", "EPSG:3377", 103.6, 2.3),
        ("EPSG:4742", "EPSG:3376", 115.5, 4.4),
        ("EPSG:4298", "EPSG:29873", 115.5, 5.2),
        ("EPSG:4326", "EPSG:8857", 12.5, 41.9),
        ("EPSG:4326", "EPSG:8858", -74.0, 40.7),
        ("EPSG:4674", "EPSG:5880", -47.9, -15.8),
        ("EPSG:4810", "EPSG:8441", 47.5, -18.9),
    ];
    for &(geo, grid, lon, lat) in cases {
        let forward = Transform::new(geo, grid).unwrap();
        let inverse = Transform::new(grid, geo).unwrap();
        let (x, y) = forward.convert(lon, lat).unwrap();
        let (back_lon, back_lat) = inverse.convert(x, y).unwrap();
        assert!((back_lon - lon).abs() < 1e-7, "{grid} lon {back_lon}");
        assert!((back_lat - lat).abs() < 1e-7, "{grid} lat {back_lat}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Coverage, so drift in either direction shows up
// ═══════════════════════════════════════════════════════════════════════════

/// Codes the definition path added, which is every code reporting native support
/// that projicio does not dispatch from its hand-written table.
fn definition_built_codes() -> Vec<u32> {
    (1..=u32::from(u16::MAX))
        .filter(|&code| epsg::support(code) == Support::Native && !epsg::is_native(code))
        .collect()
}

#[test]
fn test_definition_path_covers_the_expected_codes() {
    let codes = definition_built_codes();
    assert_eq!(codes.len(), 64, "covered codes: {codes:?}");
    for expected in [30200, 29873, 3376, 3377, 8441, 8857, 5880, 3068] {
        assert!(codes.contains(&expected), "EPSG:{expected} is not covered");
    }
}

#[test]
fn test_every_definition_built_code_roundtrips_at_its_false_origin() {
    // The false origin is the one point every definition names outright, so it
    // is on the grid whatever the grid covers.
    for code in definition_built_codes() {
        let name = format!("EPSG:{code}");
        let definition = projstring::parse(epsg::proj4_definition(code).unwrap())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let origin = (
            definition.false_easting / definition.to_meter,
            definition.false_northing / definition.to_meter,
        );

        let to_wgs84 = Transform::new(&name, "EPSG:4326").unwrap_or_else(|e| panic!("{name}: {e}"));
        let from_wgs84 =
            Transform::new("EPSG:4326", &name).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (lon, lat) = to_wgs84
            .convert(origin.0, origin.1)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!((-180.0..=180.0).contains(&lon), "{name} longitude {lon}");
        assert!((-90.0..=90.0).contains(&lat), "{name} latitude {lat}");

        let (x, y) = from_wgs84
            .convert(lon, lat)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        // A 2D roundtrip across a datum shift is not exact: the return leg starts
        // at height zero on the other datum, where the outbound leg left a height
        // of a few hundred metres. proj has the same asymmetry.
        let tolerance = 0.05 / definition.to_meter;
        assert!(
            (x - origin.0).abs() < tolerance && (y - origin.1).abs() < tolerance,
            "{name} gave ({x}, {y}) for ({}, {})",
            origin.0,
            origin.1
        );
    }
}

#[test]
fn test_codes_the_parser_rejects_keep_their_classification() {
    // The residue, with the reason each one stays out: a projection method
    // projicio has not implemented (3410, 3975, 6933), a prime meridian that is
    // not Greenwich (8044, 8045, 29700, 29702), a parameter with no meaning to
    // the method (8803), an empty definition (22300, 29701), and a datum name
    // that resolves to grids (26731).
    for code in [
        3410, 3975, 6933, 8044, 8045, 8803, 22300, 29701, 29700, 29702, 26731, 27200,
    ] {
        assert_eq!(epsg::support(code), Support::Unsupported, "EPSG:{code}");
    }
}

#[test]
fn test_fallback_and_grid_codes_are_untouched() {
    // The definition path only ever applies to codes proj4rs cannot build, so
    // nothing that already worked moves engine.
    for code in [
        27700, 2154, 25832, 3035, 2229, 31370, 3006, 28992, 4302, 4298,
    ] {
        assert_eq!(epsg::support(code), Support::Fallback, "EPSG:{code}");
    }
    for code in [4267, 32040] {
        assert_eq!(epsg::support(code), Support::NeedsGrid, "EPSG:{code}");
    }
}
