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

    /// Lambert Conformal Conic 2SP forward+inverse roundtrip.
    #[test]
    fn lambert_conformal_conic_roundtrip(
        lon in -130.0f64..-106.0,
        lat in 25.0f64..45.0,
    ) {
        let lcc = LambertConformalConic::new_2sp(
            Ellipsoid::GRS80, 34.0, 40.5, 33.5, -118.0, 2000000.0, 500000.0,
        );
        let geo = Geographic::new(lon, lat);
        if let Ok(coord) = lcc.forward(geo) {
            if let Ok(back) = lcc.inverse(coord) {
                prop_assert!((back.lon - geo.lon).abs() < 1e-7,
                    "lon mismatch: {} vs {}", back.lon, geo.lon);
                prop_assert!((back.lat - geo.lat).abs() < 1e-7,
                    "lat mismatch: {} vs {}", back.lat, geo.lat);
            }
        }
    }

    /// Polar stereographic forward+inverse roundtrip, both aspects.
    #[test]
    fn polar_stereographic_roundtrip(
        lon in -179.9f64..179.9,
        lat_abs in 60.0f64..89.9,
        north in proptest::bool::ANY,
    ) {
        let ps = if north {
            PolarStereographic::north(Ellipsoid::WGS84)
        } else {
            PolarStereographic::south(Ellipsoid::WGS84)
        };
        let geo = Geographic::new(lon, if north { lat_abs } else { -lat_abs });
        if let Ok(coord) = ps.forward(geo) {
            if let Ok(back) = ps.inverse(coord) {
                prop_assert!((back.lon - geo.lon).abs() < 1e-7,
                    "lon mismatch: {} vs {}", back.lon, geo.lon);
                prop_assert!((back.lat - geo.lat).abs() < 1e-7,
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
}
