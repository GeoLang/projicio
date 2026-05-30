// Comprehensive integration tests for projicio-core.

use projicio_core::{
    Ellipsoid, GeocentricCoord, Geographic, HelmertTransform, Transform, geocentric_to_geodetic,
    geodetic_to_geocentric, transform_datum,
};
use projicio_core::{Projection, TransverseMercator, WebMercator};

// ═══════════════════════════════════════════════════════════════════════════
// Ellipsoid tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_ellipsoid_wgs84_parameters() {
    let e = Ellipsoid::WGS84;
    assert_eq!(e.a, 6_378_137.0);
    assert!((e.f - 1.0 / 298.257_223_563).abs() < 1e-15);
}

#[test]
fn test_ellipsoid_semi_minor_axis() {
    let e = Ellipsoid::WGS84;
    // b ≈ 6356752.3 for WGS84
    let b = e.b();
    assert!((b - 6_356_752.314).abs() < 1.0);
}

#[test]
fn test_ellipsoid_eccentricity() {
    let e = Ellipsoid::WGS84;
    let e2 = e.e2();
    // e² ≈ 0.00669437999
    assert!((e2 - 0.006_694_38).abs() < 1e-6);
}

#[test]
fn test_ellipsoid_sphere_has_zero_flattening() {
    let s = Ellipsoid::SPHERE;
    assert_eq!(s.f, 0.0);
    assert_eq!(s.b(), s.a);
    assert_eq!(s.e2(), 0.0);
}

// ═══════════════════════════════════════════════════════════════════════════
// Web Mercator tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_web_mercator_origin() {
    let proj = WebMercator::new();
    let result = proj.forward(Geographic::new(0.0, 0.0)).unwrap();
    assert!((result.x).abs() < 1e-6);
    assert!((result.y).abs() < 1e-6);
}

#[test]
fn test_web_mercator_roundtrip() {
    let proj = WebMercator::new();
    let geo = Geographic::new(-0.1278, 51.5074); // London

    let projected = proj.forward(geo).unwrap();
    let back = proj.inverse(projected).unwrap();

    assert!((back.lon - geo.lon).abs() < 1e-6);
    assert!((back.lat - geo.lat).abs() < 1e-6);
}

#[test]
fn test_web_mercator_extreme_latitude_error() {
    let proj = WebMercator::new();
    // Latitude ≈ ±90° should error
    let result = proj.forward(Geographic::new(0.0, 89.99999));
    assert!(result.is_err());
}

#[test]
fn test_web_mercator_known_values() {
    // Known: London (0°, 51.5°) in EPSG:3857
    let proj = WebMercator::new();
    let result = proj.forward(Geographic::new(0.0, 51.5)).unwrap();
    assert!((result.x).abs() < 1.0); // lon=0 → x≈0
    assert!(result.y > 6_000_000.0); // northward
}

// ═══════════════════════════════════════════════════════════════════════════
// Transverse Mercator / UTM tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_utm_zone_creation() {
    let tm = TransverseMercator::utm(30, true);
    // Zone 30 central meridian = -3°
    assert!((tm.lon0 - (-3.0)).abs() < 1e-10);
    assert_eq!(tm.k0, 0.9996);
    assert_eq!(tm.false_easting, 500_000.0);
    assert_eq!(tm.false_northing, 0.0);
}

#[test]
fn test_utm_zone_south() {
    let tm = TransverseMercator::utm(35, false);
    // Zone 35 central meridian = 27°
    assert!((tm.lon0 - 27.0).abs() < 1e-10);
    assert_eq!(tm.false_northing, 10_000_000.0);
}

#[test]
fn test_utm_roundtrip() {
    let tm = TransverseMercator::utm(30, true);
    let geo = Geographic::new(-3.0, 51.0); // On central meridian

    let projected = tm.forward(geo).unwrap();
    let back = tm.inverse(projected).unwrap();

    assert!((back.lon - geo.lon).abs() < 1e-4);
    assert!((back.lat - geo.lat).abs() < 1e-4);
}

#[test]
fn test_utm_false_easting() {
    let tm = TransverseMercator::utm(30, true);
    // A point on the central meridian should have easting ≈ 500000
    let result = tm.forward(Geographic::new(-3.0, 51.0)).unwrap();
    assert!((result.x - 500_000.0).abs() < 1.0);
}

// ═══════════════════════════════════════════════════════════════════════════
// Datum transformation tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_geodetic_geocentric_roundtrip() {
    let ellipsoid = Ellipsoid::WGS84;
    let lat = 51.5074_f64.to_radians();
    let lon = (-0.1278_f64).to_radians();
    let height = 45.0;

    let ecef = geodetic_to_geocentric(lat, lon, height, &ellipsoid);
    let (back_lat, back_lon, back_h) = geocentric_to_geodetic(&ecef, &ellipsoid);

    assert!((back_lon - lon).abs() < 1e-8);
    assert!((back_lat - lat).abs() < 1e-8);
    assert!((back_h - height).abs() < 1e-3);
}

#[test]
fn test_helmert_identity() {
    // Zero-parameter transform should be identity
    let ht = HelmertTransform::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let coord = GeocentricCoord {
        x: 3_980_000.0,
        y: -10_000.0,
        z: 4_960_000.0,
    };
    let result = ht.forward(&coord);
    assert!((result.x - coord.x).abs() < 1e-6);
    assert!((result.y - coord.y).abs() < 1e-6);
    assert!((result.z - coord.z).abs() < 1e-6);
}

#[test]
fn test_helmert_translation_only() {
    let ht = HelmertTransform::new(100.0, 200.0, 300.0, 0.0, 0.0, 0.0, 0.0);
    let coord = GeocentricCoord {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let result = ht.forward(&coord);
    assert!((result.x - 100.0).abs() < 1e-6);
    assert!((result.y - 200.0).abs() < 1e-6);
    assert!((result.z - 300.0).abs() < 1e-6);
}

#[test]
fn test_transform_datum_osgb36_wgs84() {
    let ht = HelmertTransform::osgb36_to_wgs84();
    let ellipsoid_src = Ellipsoid::INTERNATIONAL_1924;
    let ellipsoid_dst = Ellipsoid::WGS84;

    // A point in London (radians)
    let lat = 51.5_f64.to_radians();
    let lon = (-0.1_f64).to_radians();
    let (result_lat, result_lon, _h) =
        transform_datum(lat, lon, 0.0, &ellipsoid_src, &ellipsoid_dst, &ht);
    // Should produce a coordinate near the original (datum shift is small)
    assert!((result_lon - lon).abs() < 0.001);
    assert!((result_lat - lat).abs() < 0.001);
}

// ═══════════════════════════════════════════════════════════════════════════
// Transform (high-level API) tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_transform_4326_to_3857() {
    let t = Transform::new("EPSG:4326", "EPSG:3857").unwrap();
    let (x, y) = t.convert(0.0, 0.0).unwrap();
    assert!(x.abs() < 1.0);
    assert!(y.abs() < 1.0);
}

#[test]
fn test_transform_3857_to_4326_london() {
    let t = Transform::new("EPSG:3857", "EPSG:4326").unwrap();
    // Web Mercator coords for London (approx)
    let (lon, lat) = t.convert(-14226.0, 6_711_568.0).unwrap();
    assert!((lon - (-0.1278)).abs() < 0.1); // approximate
    assert!((lat - 51.5074).abs() < 0.5);
}

#[test]
fn test_transform_identity_4326_to_4326() {
    let t = Transform::new("EPSG:4326", "EPSG:4326").unwrap();
    let (x, y) = t.convert(-3.0, 51.0).unwrap();
    assert!((x - (-3.0)).abs() < 1e-10);
    assert!((y - 51.0).abs() < 1e-10);
}

#[test]
fn test_transform_utm_zone() {
    let t = Transform::new("EPSG:4326", "EPSG:32630").unwrap(); // UTM zone 30N
    let (easting, northing) = t.convert(-3.0, 51.0).unwrap();
    // Central meridian → easting ≈ 500000
    assert!((easting - 500_000.0).abs() < 1.0);
    assert!(northing > 5_000_000.0);
}

#[test]
fn test_transform_unsupported_crs() {
    let result = Transform::new("EPSG:4326", "EPSG:99999");
    assert!(result.is_err());
}

#[test]
fn test_transform_batch() {
    let t = Transform::new("EPSG:4326", "EPSG:3857").unwrap();
    let coords = vec![(0.0, 0.0), (1.0, 1.0), (-1.0, -1.0)];
    let results = t.convert_batch(&coords).unwrap();
    assert_eq!(results.len(), 3);
}

#[test]
fn test_transform_invalid_epsg_format() {
    let result = Transform::new("not_a_crs", "EPSG:4326");
    assert!(result.is_err());
}
