use projicio_core::*;
use proptest::prelude::*;

proptest! {
    /// Web Mercator forward+inverse is a roundtrip (within tolerance).
    /// Geographic stores degrees; WebMercator works in degrees.
    #[test]
    fn web_mercator_roundtrip(
        lon in -179.9f64..179.9,
        lat in -85.0f64..85.0,
    ) {
        let wm = WebMercator::new();
        let geo = Geographic::new(lon, lat);
        if let Ok(coord) = wm.forward(geo) {
            if let Ok(back) = wm.inverse(coord) {
                prop_assert!((back.lon - geo.lon).abs() < 1e-6,
                    "lon mismatch: {} vs {}", back.lon, geo.lon);
                prop_assert!((back.lat - geo.lat).abs() < 1e-6,
                    "lat mismatch: {} vs {}", back.lat, geo.lat);
            }
        }
    }

    /// UTM forward+inverse roundtrip near central meridian.
    /// Geographic stores degrees; TransverseMercator works in degrees.
    #[test]
    fn utm_roundtrip(
        zone in 1u8..60,
        lat_deg in -80.0f64..84.0,
    ) {
        let north = lat_deg >= 0.0;
        let central_meridian = (zone as f64 - 1.0) * 6.0 - 180.0 + 3.0;
        // Test at central meridian where TM is most accurate
        let tm = TransverseMercator::utm(zone, north);
        let geo = Geographic::new(central_meridian, lat_deg);
        if let Ok(coord) = tm.forward(geo) {
            if let Ok(back) = tm.inverse(coord) {
                prop_assert!((back.lon - geo.lon).abs() < 1e-4,
                    "lon mismatch: {} vs {}", back.lon, geo.lon);
                prop_assert!((back.lat - geo.lat).abs() < 1e-4,
                    "lat mismatch: {} vs {}", back.lat, geo.lat);
            }
        }
    }

    /// Geocentric<->geodetic roundtrip preserves coordinates.
    /// These functions use radians.
    #[test]
    fn geocentric_roundtrip(
        lon_deg in -180.0f64..180.0,
        lat_deg in -90.0f64..90.0,
        h in -500.0f64..50000.0,
    ) {
        let lat = lat_deg.to_radians();
        let lon = lon_deg.to_radians();
        let ecef = geodetic_to_geocentric(lat, lon, h, &Ellipsoid::WGS84);
        let (lat2, lon2, h2) = geocentric_to_geodetic(&ecef, &Ellipsoid::WGS84);
        prop_assert!((lat2 - lat).abs() < 1e-8,
            "lat mismatch: {} vs {}", lat2.to_degrees(), lat_deg);
        prop_assert!((lon2 - lon).abs() < 1e-8,
            "lon mismatch: {} vs {}", lon2.to_degrees(), lon_deg);
        prop_assert!((h2 - h).abs() < 0.01,
            "height mismatch: {} vs {}", h2, h);
    }

    /// Ellipsoid semi-minor axis is always less than semi-major.
    #[test]
    fn ellipsoid_b_less_than_a(
        a in 6_000_000.0f64..7_000_000.0,
        inv_f in 200.0f64..400.0,
    ) {
        let e = Ellipsoid::new(a, 1.0 / inv_f);
        prop_assert!(e.b() < a);
        prop_assert!(e.b() > 0.0);
    }

    /// Cassini-Soldner roundtrip over the few degrees around the central
    /// meridian the projection is used within. Trinidad Grid parameters.
    #[test]
    fn cassini_soldner_roundtrip(
        lon in -64.0f64..-58.0,
        lat in 7.0f64..14.0,
    ) {
        let projection = CassiniSoldner::new(
            Ellipsoid::CLARKE_1866, 10.441_666_667, -61.333_333_333, 86_501.464, 65_379.013,
        );
        roundtrip(&projection, lon, lat, 1e-7)?;
    }

    /// Hotine Oblique Mercator roundtrip along the oblique central line, in both
    /// variants. RSO Borneo parameters.
    #[test]
    fn hotine_oblique_mercator_roundtrip(
        lon in 109.0f64..121.0,
        lat in -2.0f64..10.0,
        offsets_at_centre in proptest::bool::ANY,
    ) {
        let projection = HotineObliqueMercator::new(
            Ellipsoid::new(6_377_298.556, 1.0 / 300.8017),
            4.0, 115.0, 53.315_820_47, 53.130_102_36, 0.99984,
            590_476.87, 442_857.65, offsets_at_centre,
        ).unwrap();
        roundtrip(&projection, lon, lat, 1e-7)?;
    }

    /// American Polyconic roundtrip over the width of the Brazilian grid, which
    /// is where the Newton iteration in the inverse has to hold up.
    #[test]
    fn american_polyconic_roundtrip(
        lon in -74.0f64..-34.0,
        lat in -34.0f64..6.0,
    ) {
        let projection = AmericanPolyconic::new(
            Ellipsoid::GRS80, 0.0, -54.0, 5_000_000.0, 10_000_000.0,
        );
        roundtrip(&projection, lon, lat, 1e-7)?;
    }

    /// Equal Earth roundtrip over the whole world, which is what it is for. The
    /// inverse goes through a truncated authalic latitude series, so it holds to
    /// about a centimetre rather than to the micrometre.
    #[test]
    fn equal_earth_roundtrip(
        lon in -179.0f64..179.0,
        lat in -89.0f64..89.0,
    ) {
        let projection = EqualEarth::new(Ellipsoid::WGS84, 0.0, 0.0, 0.0);
        roundtrip(&projection, lon, lat, 1e-6)?;
    }

    /// Laborde roundtrip over Madagascar, the extent of the only grid using it.
    #[test]
    fn laborde_roundtrip(
        lon in 42.0f64..51.0,
        lat in -26.0f64..-11.0,
    ) {
        let projection = Laborde::new(
            Ellipsoid::INTERNATIONAL_1924, -18.9, 46.437_229_167, 18.9, 0.9995,
            400_000.0, 800_000.0,
        ).unwrap();
        roundtrip(&projection, lon, lat, 1e-6)?;
    }
}

/// Project and unproject, and fail if the coordinate did not come back.
fn roundtrip(
    projection: &impl Projection,
    lon: f64,
    lat: f64,
    tolerance: f64,
) -> Result<(), TestCaseError> {
    let geo = Geographic::new(lon, lat);
    let coord = projection.forward(geo)?;
    let back = projection.inverse(coord)?;
    prop_assert!(
        (back.lon - lon).abs() < tolerance,
        "lon mismatch: {} vs {lon}",
        back.lon
    );
    prop_assert!(
        (back.lat - lat).abs() < tolerance,
        "lat mismatch: {} vs {lat}",
        back.lat
    );
    Ok(())
}
