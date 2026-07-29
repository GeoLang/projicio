// Runtime NTv2 grid registration.
//
// This is its own test binary on purpose. Registering a grid puts it in a process wide
// catalog that cannot be emptied, so these tests must not share a process with the ones
// that assert a code is unsupported without a grid (see tests/fallback_crs.rs).
//
// Within this file, every test uses its own grid name, and the one test that registers
// "conus" does the before and after assertions in sequence so it does not race.

use projicio_core::{Support, Transform, epsg, grids};

// ═══════════════════════════════════════════════════════════════════════════
// Synthetic NTv2 builder
// ═══════════════════════════════════════════════════════════════════════════

/// Build a valid single sub-grid NTv2 file applying a constant shift.
///
/// Layout per the NTv2 specification: an 11 record overview header, an 11 record
/// sub-grid header, then one 16 byte record per node. Records are 8 ASCII bytes of tag
/// followed by 8 bytes of value. Longitudes in the header are positive WEST, which is
/// why `west` is larger than `east` for a box straddling the prime meridian.
///
/// A constant shift keeps the expected output exact: bilinear interpolation of equal
/// corner values returns that value, so no interpolation tolerance enters the test.
fn synthetic_ntv2(
    south: f64,
    north: f64,
    east_pos_west: f64,
    west_pos_west: f64,
    inc: f64,
    lat_shift_sec: f32,
    lon_shift_sec: f32,
) -> Vec<u8> {
    fn rec(tag: &str, payload: &[u8]) -> Vec<u8> {
        let mut v = format!("{tag:<8}").into_bytes();
        v.extend_from_slice(payload);
        assert_eq!(v.len(), 16, "NTv2 records are 16 bytes");
        v
    }
    fn text(s: &str) -> Vec<u8> {
        format!("{s:<8}").into_bytes()
    }
    /// An i32 value padded to the 8 byte value slot.
    fn int(v: i32) -> Vec<u8> {
        let mut b = v.to_le_bytes().to_vec();
        b.extend_from_slice(&[0; 4]);
        b
    }

    let mut b = Vec::new();
    // NUM_OREC carries 11, which is also how a reader detects endianness.
    b.extend(rec("NUM_OREC", &int(11)));
    b.extend(rec("NUM_SREC", &int(11)));
    b.extend(rec("NUM_FILE", &int(1)));
    b.extend(rec("GS_TYPE", &text("SECONDS")));
    b.extend(rec("VERSION", &text("NTv2.0")));
    b.extend(rec("SYSTEM_F", &text("SRC")));
    b.extend(rec("SYSTEM_T", &text("DST")));
    for (tag, v) in [
        ("MAJOR_F", 6_378_206.4f64),
        ("MINOR_F", 6_356_583.8),
        ("MAJOR_T", 6_378_137.0),
        ("MINOR_T", 6_356_752.314),
    ] {
        b.extend(rec(tag, &v.to_le_bytes()));
    }
    assert_eq!(b.len(), 176);

    let (s, n) = (south * 3600.0, north * 3600.0);
    let (e, w) = (east_pos_west * 3600.0, west_pos_west * 3600.0);
    let i = inc * 3600.0;

    // The same node counts the reader derives, so GS_COUNT agrees with the header box.
    let rows = (((n - s).abs() / i + 0.5) + 1.0).floor() as usize;
    let cols = (((w - e).abs() / i + 0.5) + 1.0).floor() as usize;
    let count = rows * cols;

    b.extend(rec("SUB_NAME", &text("TESTGRID")));
    b.extend(rec("PARENT", &text("NONE")));
    b.extend(rec("CREATED", &text("01011990")));
    b.extend(rec("UPDATED", &text("01011990")));
    b.extend(rec("S_LAT", &s.to_le_bytes()));
    b.extend(rec("N_LAT", &n.to_le_bytes()));
    b.extend(rec("E_LONG", &e.to_le_bytes()));
    b.extend(rec("W_LONG", &w.to_le_bytes()));
    b.extend(rec("LAT_INC", &i.to_le_bytes()));
    b.extend(rec("LONG_INC", &i.to_le_bytes()));
    b.extend(rec("GS_COUNT", &int(count as i32)));

    for _ in 0..count {
        b.extend_from_slice(&lat_shift_sec.to_le_bytes());
        b.extend_from_slice(&lon_shift_sec.to_le_bytes());
        // Accuracy columns, unused by the interpolation.
        b.extend_from_slice(&0.0f32.to_le_bytes());
        b.extend_from_slice(&0.0f32.to_le_bytes());
    }
    b
}

const SEC: f64 = 1.0 / 3600.0;

// ═══════════════════════════════════════════════════════════════════════════
// NAD27, the codes a grid unlocks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_nad27_codes_before_and_after_registering_conus() {
    // Before: the definition behind +datum=NAD27 names a grid list, and nothing is
    // registered, so these codes must report the specific reason.
    assert_eq!(epsg::support(4267), Support::NeedsGrid);
    assert_eq!(epsg::support(32040), Support::NeedsGrid);
    let err = Transform::new("EPSG:4267", "EPSG:4326").unwrap_err();
    assert!(err.to_string().contains("not registered"), "{err}");
    assert!(!grids::is_registered("conus"));

    // A grid over the lower 48, 130W to 60W and 20N to 50N, shifting every point by
    // 1 arc second north and 2 arc seconds west.
    let grid = synthetic_ntv2(20.0, 50.0, 60.0, 130.0, 10.0, 1.0, 2.0);
    grids::register_bytes("conus", grid).unwrap();

    // After: the same codes resolve, through the fallback engine.
    assert!(grids::is_registered("conus"));
    assert!(grids::registered().contains(&"conus".to_string()));
    assert_eq!(epsg::support(4267), Support::Fallback);
    assert_eq!(epsg::support(32040), Support::Fallback);

    // And the shift is applied, exactly, because the grid is constant.
    let t = Transform::new("EPSG:4267", "EPSG:4326").unwrap();
    let (lon, lat) = t.convert(-100.0, 35.0).unwrap();
    assert!((lat - (35.0 + 1.0 * SEC)).abs() < 1e-12, "lat {lat}");
    assert!((lon - (-100.0 - 2.0 * SEC)).abs() < 1e-12, "lon {lon}");
}

#[test]
fn test_point_outside_the_grid_errors_cleanly() {
    // The grid this test needs covers only a small box, and every NAD27 definition
    // shares the "conus" name, so use a projstring naming this test's own grid.
    let grid = synthetic_ntv2(0.0, 4.0, 0.0, 4.0, 1.0, 1.0, 1.0);
    grids::register_bytes("projicio-test-tinybox.gsb", grid).unwrap();

    let src = "+proj=longlat +ellps=clrk66 +nadgrids=projicio-test-tinybox.gsb";
    let t = Transform::new(src, "EPSG:4326").unwrap();

    // Inside the box, which spans 4W to 0 and 0N to 4N.
    assert!(t.convert(-2.0, 2.0).is_ok());

    // Far outside it.
    let err = t.convert(50.0, 50.0).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("outside"),
        "expected an out of area error, got {err}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// OSTN15 shaped: a real national grid transform driven by a registered grid
// ═══════════════════════════════════════════════════════════════════════════

/// EPSG:27700 as OS defines the projection, with the datum shift taken from a named
/// grid rather than the 7 parameter Helmert the embedded definition carries.
///
/// This is the shape of a real OSTN15 setup. With the OS file registered as
/// "OSTN15_NTv2_OSGBtoETRS.gsb" this is the string a user writes, and the projection
/// parameters here are the published EPSG:27700 ones. See the README for how to obtain
/// the real file and validate against OS test points, which cannot be done here because
/// projicio embeds no grid data.
fn bng_via_grid(grid_name: &str) -> String {
    format!(
        "+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +x_0=400000 +y_0=-100000 \
         +ellps=airy +nadgrids={grid_name} +units=m +no_defs"
    )
}

/// The same projection with no datum information, so no shift is applied at all.
const BNG_PROJECTION_ONLY: &str = concat!(
    "+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +x_0=400000 +y_0=-100000 ",
    "+ellps=airy +units=m +no_defs"
);

const OSGB36_GEOGRAPHIC_ONLY: &str = "+proj=longlat +ellps=airy +no_defs";

/// A grid over Great Britain, 8W to 2E and 49N to 61N. In the header longitudes are
/// positive west, so 2E is -2, and so is the longitude shift for a point that moves
/// east. Read in the grid's forward direction, from the source datum towards WGS84,
/// this adds 3 arc seconds of latitude and 2 arc seconds of eastward longitude. It
/// stands in for what OSTN15 does at metre level.
fn gb_grid() -> Vec<u8> {
    synthetic_ntv2(49.0, 61.0, -2.0, 8.0, 2.0, 3.0, -2.0)
}

#[test]
fn test_ostn15_shaped_grid_drives_the_datum_shift() {
    let name = "projicio-test-ostn15-shift.gsb";
    grids::register_bytes(name, gb_grid()).unwrap();

    let (lon, lat) = (-0.1275, 51.50722); // central London
    let gridded = Transform::new("EPSG:4258", &bng_via_grid(name)).unwrap();
    let (e, n) = gridded.convert(lon, lat).unwrap();

    // The destination applies the grid in the inverse direction, so an ETRS89 point
    // lands on the OSGB36 geodetic point whose forward shift returns it. For a constant
    // grid that is the input minus the shift, so 3 arc seconds south and 2 arc seconds
    // west. Project that directly, with no datum involved, and the two must agree.
    // A millimetre of tolerance, since the inverse grid shift is solved iteratively.
    let plain = Transform::new(OSGB36_GEOGRAPHIC_ONLY, BNG_PROJECTION_ONLY).unwrap();
    let (want_e, want_n) = plain.convert(lon - 2.0 * SEC, lat - 3.0 * SEC).unwrap();
    assert!((e - want_e).abs() < 1e-3, "easting {e} vs {want_e}");
    assert!((n - want_n).abs() < 1e-3, "northing {n} vs {want_n}");

    // And the grid genuinely changed the answer: the embedded EPSG:27700 definition
    // uses a Helmert shift instead, and these two must land tens of metres apart.
    let helmert = Transform::new("EPSG:4258", "EPSG:27700").unwrap();
    let (he, hn) = helmert.convert(lon, lat).unwrap();
    let moved = (e - he).hypot(n - hn);
    assert!(
        (10.0..500.0).contains(&moved),
        "grid path should differ from the Helmert path by tens of metres, got {moved}"
    );
}

#[test]
fn test_gridded_bng_roundtrips() {
    // Its own grid name, since a name can only be registered once per process and
    // tests run in parallel.
    let name = "projicio-test-ostn15-roundtrip.gsb";
    grids::register_bytes(name, gb_grid()).unwrap();

    let spec = bng_via_grid(name);
    let forward = Transform::new("EPSG:4258", &spec).unwrap();
    let inverse = Transform::new(&spec, "EPSG:4258").unwrap();
    let (lon, lat) = (-2.5, 53.4);
    let (e, n) = forward.convert(lon, lat).unwrap();
    let (back_lon, back_lat) = inverse.convert(e, n).unwrap();
    assert!((back_lon - lon).abs() < 1e-9, "lon {back_lon}");
    assert!((back_lat - lat).abs() < 1e-9, "lat {back_lat}");
}

#[test]
#[ignore = "needs the real OS grid, set PROJICIO_OSTN15_GSB to its path"]
fn test_ostn15_real_grid_os_test_point_tp01() {
    // Validation against the real Ordnance Survey grid, which cannot ship with the
    // crate. Get OSTN15_NTv2_OSGBtoETRS.gsb from the NTv2 format files zip at
    // https://www.ordnancesurvey.co.uk/geodesy-positioning/coordinate-transformations/resources
    // then run:
    //   PROJICIO_OSTN15_GSB=/path/to/OSTN15_NTv2_OSGBtoETRS.gsb \
    //     cargo test -p projicio-core --test grids -- --ignored ostn15_real
    //
    // Expected values are OS test point TP01, from the developer pack files
    // OSTN15_OSGM15_TestInput_ETRStoOSGB.txt and OSTN15_OSGM15_TestOutput_ETRStoOSGB.txt:
    //   TP01,49.92226393730,-6.29977752014,100.000  ->  TP01,91492.146,11318.804,46.519
    // The same pair is quoted from the OS developer pack in
    // https://github.com/OSGeo/PROJ/issues/2173
    let Ok(path) = std::env::var("PROJICIO_OSTN15_GSB") else {
        panic!("set PROJICIO_OSTN15_GSB to the path of OSTN15_NTv2_OSGBtoETRS.gsb");
    };

    let name = "OSTN15_NTv2_OSGBtoETRS.gsb";
    grids::register_file(name, &path).unwrap();

    let t = Transform::new("EPSG:4258", &bng_via_grid(name)).unwrap();
    let (e, n) = t.convert(-6.299_777_520_14, 49.922_263_937_30).unwrap();

    // OS publishes the grid to a millimetre, so agree to a millimetre.
    assert!((e - 91_492.146).abs() < 0.001, "easting {e}");
    assert!((n - 11_318.804).abs() < 0.001, "northing {n}");
}

// ═══════════════════════════════════════════════════════════════════════════
// Registration errors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_registering_garbage_reports_a_grid_error() {
    let err = grids::register_bytes("projicio-test-garbage.gsb", vec![0u8; 500]).unwrap_err();
    assert!(err.to_string().contains("grid error"), "{err}");
    assert!(!grids::is_registered("projicio-test-garbage.gsb"));
}

#[test]
fn test_registering_a_truncated_grid_reports_a_grid_error() {
    let mut grid = synthetic_ntv2(0.0, 2.0, 0.0, 2.0, 1.0, 1.0, 1.0);
    grid.truncate(200); // header claims nodes that are not there
    let err = grids::register_bytes("projicio-test-truncated.gsb", grid).unwrap_err();
    assert!(err.to_string().contains("grid error"), "{err}");
}

#[test]
fn test_registering_a_missing_file_reports_the_path() {
    let err = grids::register_file("projicio-test-absent.gsb", "/no/such/grid.gsb").unwrap_err();
    assert!(err.to_string().contains("/no/such/grid.gsb"), "{err}");
}

#[test]
fn test_register_from_a_file_on_disk() {
    let dir = std::env::temp_dir().join("projicio-grid-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fromdisk.gsb");
    std::fs::write(&path, synthetic_ntv2(0.0, 2.0, 0.0, 2.0, 1.0, 4.0, 0.0)).unwrap();

    grids::register_file("projicio-test-fromdisk.gsb", &path).unwrap();
    assert!(grids::is_registered("projicio-test-fromdisk.gsb"));

    let src = "+proj=longlat +ellps=clrk66 +nadgrids=projicio-test-fromdisk.gsb";
    let t = Transform::new(src, "EPSG:4326").unwrap();
    let (_, lat) = t.convert(-1.0, 1.0).unwrap();
    assert!((lat - (1.0 + 4.0 * SEC)).abs() < 1e-12, "lat {lat}");
}

#[test]
fn test_a_name_cannot_be_registered_twice() {
    let name = "projicio-test-twice.gsb";
    grids::register_bytes(name, synthetic_ntv2(0.0, 2.0, 0.0, 2.0, 1.0, 1.0, 1.0)).unwrap();
    let err =
        grids::register_bytes(name, synthetic_ntv2(0.0, 2.0, 0.0, 2.0, 1.0, 9.0, 0.0)).unwrap_err();
    assert!(err.to_string().contains("already registered"), "{err}");
}

#[test]
fn test_unregistered_grid_in_a_projstring_errors_cleanly() {
    let src = "+proj=longlat +ellps=clrk66 +nadgrids=projicio-test-never-registered.gsb";
    let err = Transform::new(src, "EPSG:4326").unwrap_err();
    assert!(err.to_string().contains("not registered"), "{err}");
}

#[test]
fn test_null_grid_definition_still_permits_no_shift() {
    // A definition ending in @null says an ungridded result is acceptable, and that
    // must keep working rather than being turned into an error.
    let src = "+proj=longlat +ellps=clrk66 +nadgrids=@projicio-test-absent,@null";
    let t = Transform::new(src, "EPSG:4326").unwrap();
    assert!(t.convert(-100.0, 35.0).is_ok());
}
