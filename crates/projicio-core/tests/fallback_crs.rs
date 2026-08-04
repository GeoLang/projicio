// Known-value tests for national grids served by the embedded EPSG table.
//
// The projection-only cases use a source CRS on the same datum as the target so
// no datum shift is involved and the published numbers apply exactly.

use projicio_core::{Support, Transform, epsg};

/// Degrees from a degree/minute/second triple, so test inputs can be written the
/// way the source documents print them.
fn dms(d: f64, m: f64, s: f64) -> f64 {
    d.signum() * (d.abs() + m / 60.0 + s / 3600.0)
}

// ═══════════════════════════════════════════════════════════════════════════
// Known-value transforms, published sources
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_bng_27700_iogp_worked_example() {
    // IOGP Geomatics Guidance Note 7 part 2 (373-07-02, September 2019) pages 54-55,
    // Transverse Mercator worked example for OSGB 1936 / British National Grid:
    // lat 50 deg 30' 00" N, lon 00 deg 30' 00" E gives E 577274.99 m, N 69740.50 m.
    // https://www.iogp.org/wp-content/uploads/2019/09/373-07-02.pdf
    // EPSG:4277 is OSGB 1936 geographic, the same datum as EPSG:27700, so this is
    // the pure projection the guidance note computes.
    let t = Transform::new("EPSG:4277", "EPSG:27700").unwrap();
    let (e, n) = t
        .convert(dms(0.0, 30.0, 0.0), dms(50.0, 30.0, 0.0))
        .unwrap();
    assert!((e - 577_274.99).abs() < 0.01, "easting {e}");
    assert!((n - 69_740.50).abs() < 0.01, "northing {n}");
}

#[test]
fn test_laea_europe_3035_iogp_worked_example() {
    // IOGP Geomatics Guidance Note 7 part 2 (373-07-02, September 2019) page 79,
    // Lambert Azimuthal Equal Area worked example for ETRS89 / ETRS-LAEA:
    // lat 50 deg N, lon 5 deg E gives E 3962799.45 m, N 2999718.85 m.
    // https://www.iogp.org/wp-content/uploads/2019/09/373-07-02.pdf
    // EPSG:4258 is ETRS89 geographic, the same datum as EPSG:3035.
    let t = Transform::new("EPSG:4258", "EPSG:3035").unwrap();
    let (e, n) = t.convert(5.0, 50.0).unwrap();
    assert!((e - 3_962_799.45).abs() < 0.01, "easting {e}");
    assert!((n - 2_999_718.85).abs() < 0.01, "northing {n}");
}

/// Lambert-93 easting and northing from the constants IGN publishes for the
/// projection, using IGN's own forward algorithm.
///
/// IGN Notes Techniques NT/G 71 "Projection cartographique conique conforme de
/// Lambert - Algorithmes" (1st ed. January 1995) tabulates, for Lambert-93,
/// n = 0.7256077650, c = 11754255.426 m, Xs = 700000.0 m, Ys = 12655612.050 m,
/// on GRS 1980 with central meridian 3 deg E.
/// https://geodesie.ign.fr/files/geodesie/2025-02/NTG_71.pdf
///
/// This is an independent check of the projection: projicio derives its own conic
/// constants from the standard parallels, while this derives nothing and applies the
/// published ones. Ys is published to a tenth of a millimetre, which sets the floor
/// on how closely the two can agree.
fn ign_lambert93(lon_deg: f64, lat_deg: f64) -> (f64, f64) {
    const N: f64 = 0.725_607_765_0;
    const C: f64 = 11_754_255.426;
    const XS: f64 = 700_000.0;
    const YS: f64 = 12_655_612.050;

    let f: f64 = 1.0 / 298.257_222_101;
    let e = (2.0 * f - f * f).sqrt();
    let phi = lat_deg.to_radians();
    let lam = lon_deg.to_radians() - 3.0_f64.to_radians();

    // isometric latitude, IGN ALG0001
    let l = ((std::f64::consts::FRAC_PI_4 + phi / 2.0).tan()
        * ((1.0 - e * phi.sin()) / (1.0 + e * phi.sin())).powf(e / 2.0))
    .ln();

    // IGN ALG0003
    let r = C * (-N * l).exp();
    let gamma = N * lam;
    (XS + r * gamma.sin(), YS - r * gamma.cos())
}

#[test]
fn test_lambert93_2154_matches_ign_published_constants() {
    let t = Transform::new("EPSG:4171", "EPSG:2154").unwrap();
    // Paris, Nantes, Strasbourg, Marseille: spread across the zone so a wrong conic
    // cannot hide near the origin.
    for (lon, lat) in [(2.35, 48.85), (-1.55, 47.22), (7.75, 48.58), (5.37, 43.3)] {
        let (e, n) = t.convert(lon, lat).unwrap();
        let (want_e, want_n) = ign_lambert93(lon, lat);
        assert!((e - want_e).abs() < 0.001, "easting {e} vs {want_e}");
        assert!((n - want_n).abs() < 0.001, "northing {n} vs {want_n}");
    }
}

#[test]
fn test_lambert93_2154_false_origin() {
    // EPSG:2154 places its false origin at lat 46.5 N, lon 3 E with easting 700000 m
    // and northing 6600000 m, so that pair is exact by definition.
    // https://epsg.io/2154, and IGN as cited on ign_lambert93 above.
    let t = Transform::new("EPSG:4171", "EPSG:2154").unwrap();
    let (e, n) = t.convert(3.0, 46.5).unwrap();
    assert!((e - 700_000.0).abs() < 1e-6, "easting {e}");
    assert!((n - 6_600_000.0).abs() < 1e-6, "northing {n}");
}

#[test]
fn test_state_plane_26985_ngs_datasheet() {
    // NOAA NGS datasheet for mark JV7114 (designation 20322, Montgomery County MD),
    // NAD83(2011) epoch 2010.00, State Plane Maryland zone 1900 = EPSG:26985:
    //   NAD 83(2011) POSITION- 39 00 38.66215(N) 077 09 15.14805(W)
    //   ;SPC MD - 149,209.682   386,644.192   MT
    // https://www.ngs.noaa.gov/cgi-bin/ds_mark.prl?PidBox=JV7114
    // EPSG:4269 is NAD83 geographic, the same datum as EPSG:26985.
    let t = Transform::new("EPSG:4269", "EPSG:26985").unwrap();
    let (e, n) = t
        .convert(dms(-77.0, 9.0, 15.14805), dms(39.0, 0.0, 38.66215))
        .unwrap();
    assert!((e - 386_644.192).abs() < 0.001, "easting {e}");
    assert!((n - 149_209.682).abs() < 0.001, "northing {n}");
}

#[test]
fn test_state_plane_2248_ngs_datasheet_us_survey_feet() {
    // Same NGS mark JV7114, the US survey foot form of the Maryland zone (EPSG:2248):
    //   ;SPC MD - 489,532.10  1,268,515.15   sFT
    // https://www.ngs.noaa.gov/cgi-bin/ds_mark.prl?PidBox=JV7114
    // This is the unit handling check: the grid is identical to EPSG:26985 but the
    // axis unit is the US survey foot, not the metre.
    let t = Transform::new("EPSG:4269", "EPSG:2248").unwrap();
    let (e, n) = t
        .convert(dms(-77.0, 9.0, 15.14805), dms(39.0, 0.0, 38.66215))
        .unwrap();
    assert!((e - 1_268_515.15).abs() < 0.01, "easting {e}");
    assert!((n - 489_532.10).abs() < 0.01, "northing {n}");
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-engine agreement
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_etrs89_utm32n_agrees_with_native_utm() {
    // EPSG:25832 (ETRS89 / UTM 32N, GRS80) and EPSG:32632 (WGS84 / UTM 32N) share the
    // same grid definition and their ellipsoids differ only in the 9th digit of the
    // flattening, so the two engines must agree to well under a millimetre. This
    // checks the fallback against projicio's own Transverse Mercator series.
    let fallback = Transform::new("EPSG:4258", "EPSG:25832").unwrap();
    let native = Transform::new("EPSG:4326", "EPSG:32632").unwrap();
    assert_eq!(fallback.path(), Support::Fallback);
    assert_eq!(native.path(), Support::Native);

    for (lon, lat) in [(9.0, 52.0), (6.5, 47.5), (11.9, 58.0), (7.1, 50.7)] {
        let (fe, fn_) = fallback.convert(lon, lat).unwrap();
        let (ne, nn) = native.convert(lon, lat).unwrap();
        assert!(
            (fe - ne).abs() < 0.001,
            "easting {fe} vs {ne} at {lon},{lat}"
        );
        assert!(
            (fn_ - nn).abs() < 0.001,
            "northing {fn_} vs {nn} at {lon},{lat}"
        );
    }
}

#[test]
fn test_etrs89_utm32n_natural_origin() {
    // EPSG:25832 has latitude of natural origin 0, longitude of natural origin 9 E,
    // false easting 500000 m and false northing 0, so that pair is exact by
    // definition. https://epsg.io/25832
    // Also guards against a units or radians/degrees slip at the engine boundary.
    let t = Transform::new("EPSG:4258", "EPSG:25832").unwrap();
    let (e, n) = t.convert(9.0, 0.0).unwrap();
    assert!((e - 500_000.0).abs() < 1e-6, "easting {e}");
    assert!(n.abs() < 1e-6, "northing {n}");
}

// ═══════════════════════════════════════════════════════════════════════════
// Roundtrips through the fallback engine
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_fallback_roundtrips() {
    let cases: &[(&str, &str, f64, f64)] = &[
        ("EPSG:4277", "EPSG:27700", -2.5, 53.4),
        ("EPSG:4258", "EPSG:3035", 5.0, 50.0),
        ("EPSG:4258", "EPSG:25832", 9.5, 52.5),
        ("EPSG:4171", "EPSG:2154", 2.35, 48.85),
        ("EPSG:4269", "EPSG:2229", -118.25, 34.05),
    ];
    // 1e-7 degrees is roughly a centimetre, which is well inside the iteration
    // tolerance of the inverse solvers and still catches a wrong inverse.
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
// Support reporting
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_support_reports_native_codes() {
    for code in [4326, 3857, 32601, 32632, 32660, 32701, 32755, 32760] {
        assert_eq!(epsg::support(code), Support::Native, "EPSG:{code}");
        assert!(epsg::is_native(code));
    }
}

#[test]
fn test_support_reports_fallback_codes() {
    for code in [27700, 2154, 25832, 3035, 2229, 31370, 3006, 28992] {
        assert_eq!(epsg::support(code), Support::Fallback, "EPSG:{code}");
        assert!(!epsg::is_native(code));
        assert!(epsg::proj4_definition(code).is_some());
    }
}

#[test]
fn test_support_reports_unsupported_for_garbage_codes() {
    for code in [0, 1, 99999, 900_913, u32::MAX] {
        assert_eq!(epsg::support(code), Support::Unsupported, "EPSG:{code}");
        assert!(epsg::proj4_definition(code).is_none(), "EPSG:{code}");
    }
}

#[test]
fn test_support_reports_needs_grid_when_definition_needs_a_grid() {
    // NAD27 resolves to a nadgrids definition and projicio embeds no grids, so these
    // codes have a definition but no working transform until a grid is registered.
    // No grid is registered in this test binary, see tests/grids.rs for the other half.
    assert!(epsg::proj4_definition(4267).is_some());
    assert_eq!(epsg::support(4267), Support::NeedsGrid);
    assert_eq!(epsg::support(32040), Support::NeedsGrid);
}

// ═══════════════════════════════════════════════════════════════════════════
// Error behaviour
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_garbage_code_errors_cleanly() {
    for bad in ["EPSG:99999", "EPSG:0", "EPSG:900913", "EPSG:4294967295"] {
        let err = Transform::new("EPSG:4326", bad).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unsupported CRS"), "{bad} gave {msg}");
    }
}

#[test]
fn test_garbage_code_as_source_errors_cleanly() {
    assert!(Transform::new("EPSG:99999", "EPSG:4326").is_err());
    assert!(Transform::new("EPSG:99999", "EPSG:27700").is_err());
}

#[test]
fn test_non_numeric_crs_errors_cleanly() {
    for bad in ["", "EPSG:", "EPSG:abc", "nonsense", "EPSG:-1"] {
        let err = Transform::new(bad, "EPSG:4326").unwrap_err();
        assert!(err.to_string().contains("unsupported CRS"), "{bad}");
    }
}

#[test]
fn test_projstring_source_is_accepted() {
    // A projstring is how a caller reaches a CRS the embedded table does not describe,
    // including one that names its own datum shift grid.
    let t = Transform::new("+proj=longlat +ellps=GRS80", "EPSG:3035").unwrap();
    assert_eq!(t.path(), Support::Fallback);
    let (e, n) = t.convert(5.0, 50.0).unwrap();
    // Same IOGP worked example as above, reached without an EPSG code.
    assert!((e - 3_962_799.45).abs() < 0.01, "easting {e}");
    assert!((n - 2_999_718.85).abs() < 0.01, "northing {n}");
}

#[test]
fn test_aeqd_projstring_keeps_distance_from_center() {
    // geodukt buffers in a local aeqd plane so metric distances hold in any crs
    let t = Transform::new("EPSG:4326", "+proj=aeqd +lat_0=45 +lon_0=7 +ellps=WGS84").unwrap();
    let (x, y) = t.convert(7.0, 45.0).unwrap();
    assert!(
        x.abs() < 1e-6 && y.abs() < 1e-6,
        "center maps to origin, got ({x}, {y})"
    );
    // one degree north of center, the northing is the meridian arc length
    let (x, y) = t.convert(7.0, 46.0).unwrap();
    assert!(
        x.abs() < 1e-3,
        "meridian point stays on the y axis, got {x}"
    );
    assert!((y - 111_131.8).abs() < 10.0, "northing {y}");
}

#[test]
fn test_bad_projstring_errors_cleanly() {
    for bad in ["+proj=nonsuch", "+proj=", "+ellps=WGS84"] {
        let err = Transform::new(bad, "EPSG:4326").unwrap_err();
        assert!(
            err.to_string().contains("unsupported CRS"),
            "{bad} gave {err}"
        );
    }
}

#[test]
fn test_grid_backed_code_reports_a_grid_error() {
    let err = Transform::new("EPSG:4326", "EPSG:4267").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("grid error"), "{msg}");
    assert!(msg.contains("4267"), "{msg}");
    assert!(msg.contains("not registered"), "{msg}");
}

// ═══════════════════════════════════════════════════════════════════════════
// Native path is unaffected by the fallback
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_native_pairs_still_take_the_native_path() {
    let pairs = [
        ("EPSG:4326", "EPSG:3857"),
        ("EPSG:3857", "EPSG:4326"),
        ("EPSG:4326", "EPSG:4326"),
        ("EPSG:4326", "EPSG:32618"),
        ("EPSG:32618", "EPSG:4326"),
        ("EPSG:32755", "EPSG:3857"),
    ];
    for (from, to) in pairs {
        let t = Transform::new(from, to).unwrap();
        assert_eq!(t.path(), Support::Native, "{from} -> {to}");
    }
}

#[test]
fn test_native_results_unchanged() {
    // Values recorded from projicio's own projection math before the fallback engine
    // was added. These must not move: the fallback is additive only.
    let t = Transform::new("EPSG:4326", "EPSG:3857").unwrap();
    let (x, y) = t.convert(-74.006, 40.7128).unwrap();
    assert!((x - -8_238_310.235_647_004).abs() < 1e-6, "x {x}");
    assert!((y - 4_970_071.579_142_425).abs() < 1e-6, "y {y}");

    let t = Transform::new("EPSG:4326", "EPSG:32618").unwrap();
    let (e, n) = t.convert(-74.006, 40.7128).unwrap();
    assert!((e - 583_959.372_324_113).abs() < 1e-6, "easting {e}");
    assert!((n - 4_507_350.998_397_536).abs() < 1e-6, "northing {n}");

    let t = Transform::new("EPSG:4326", "EPSG:32755").unwrap();
    let (e, n) = t.convert(151.2093, -33.8688).unwrap();
    assert!((e - 889_449.997_565_021).abs() < 1e-6, "easting {e}");
    assert!((n - 6_244_409.977_205_008).abs() < 1e-6, "northing {n}");
}

#[test]
fn test_mixed_native_and_fallback_pair_works() {
    // A native code paired with a fallback code hands the whole transform to the
    // fallback so the datum shift is applied once, consistently.
    let t = Transform::new("EPSG:4326", "EPSG:27700").unwrap();
    assert_eq!(t.path(), Support::Fallback);
    let (e, n) = t.convert(-0.1275, 51.50722).unwrap();
    // Central London, roughly 530 km east and 180 km north of the grid origin.
    assert!((e - 530_000.0).abs() < 1_000.0, "easting {e}");
    assert!((n - 180_000.0).abs() < 1_000.0, "northing {n}");
}
