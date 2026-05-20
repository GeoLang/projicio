use crate::Ellipsoid;

/// Helmert 7-parameter datum transformation (Bursa-Wolf model).
///
/// Transforms geocentric (ECEF) coordinates between two datums using:
///   X' = T + (1 + s) * R * X
///
/// where T is translation, s is scale factor, and R is the rotation matrix.
/// Parameters follow the Position Vector convention (EPSG method 1033).
#[derive(Debug, Clone)]
pub struct HelmertTransform {
    /// Translation in X (meters)
    pub dx: f64,
    /// Translation in Y (meters)
    pub dy: f64,
    /// Translation in Z (meters)
    pub dz: f64,
    /// Rotation about X axis (arc-seconds)
    pub rx: f64,
    /// Rotation about Y axis (arc-seconds)
    pub ry: f64,
    /// Rotation about Z axis (arc-seconds)
    pub rz: f64,
    /// Scale factor (parts per million)
    pub ds: f64,
}

/// Geocentric (ECEF) coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeocentricCoord {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl HelmertTransform {
    /// Create a new Helmert transform with the given parameters.
    ///
    /// # Arguments
    /// * `dx, dy, dz` — translations in meters
    /// * `rx, ry, rz` — rotations in arc-seconds
    /// * `ds` — scale difference in parts per million
    pub fn new(dx: f64, dy: f64, dz: f64, rx: f64, ry: f64, rz: f64, ds: f64) -> Self {
        Self {
            dx,
            dy,
            dz,
            rx,
            ry,
            rz,
            ds,
        }
    }

    /// WGS84 ↔ NAD83 (CORS96) — effectively identity for most purposes,
    /// but includes the official ITRF2008 parameters.
    pub fn wgs84_to_nad83() -> Self {
        Self::new(
            0.9956, -1.9013, -0.5215, 0.025915, 0.009426, 0.011599, 0.00062,
        )
    }

    /// ED50 → WGS84 (Europe, mean values).
    pub fn ed50_to_wgs84() -> Self {
        Self::new(-87.0, -98.0, -121.0, 0.0, 0.0, 0.0, 0.0)
    }

    /// NAD27 → WGS84 (CONUS mean values).
    pub fn nad27_to_wgs84_conus() -> Self {
        Self::new(-8.0, 160.0, 176.0, 0.0, 0.0, 0.0, 0.0)
    }

    /// OSGB36 → WGS84 (UK Ordnance Survey).
    pub fn osgb36_to_wgs84() -> Self {
        Self::new(446.448, -125.157, 542.060, 0.1502, 0.2470, 0.8421, -20.4894)
    }

    /// Apply forward transformation (source datum → target datum).
    pub fn forward(&self, coord: &GeocentricCoord) -> GeocentricCoord {
        // Convert rotation from arc-seconds to radians
        let as_to_rad = std::f64::consts::PI / (180.0 * 3600.0);
        let rx = self.rx * as_to_rad;
        let ry = self.ry * as_to_rad;
        let rz = self.rz * as_to_rad;

        // Scale factor: 1 + ds * 1e-6
        let s = 1.0 + self.ds * 1e-6;

        // Apply Bursa-Wolf: X' = T + (1+s) * R * X
        // Using small angle approximation for rotation matrix:
        // R = | 1   -rz  ry |
        //     | rz   1  -rx |
        //     |-ry  rx   1  |
        let x = self.dx + s * (coord.x - rz * coord.y + ry * coord.z);
        let y = self.dy + s * (rz * coord.x + coord.y - rx * coord.z);
        let z = self.dz + s * (-ry * coord.x + rx * coord.y + coord.z);

        GeocentricCoord { x, y, z }
    }

    /// Apply inverse transformation (target datum → source datum).
    pub fn inverse(&self, coord: &GeocentricCoord) -> GeocentricCoord {
        // Inverse is the same formula with negated parameters
        let inv = Self::new(
            -self.dx, -self.dy, -self.dz, -self.rx, -self.ry, -self.rz, -self.ds,
        );
        inv.forward(coord)
    }
}

/// Convert geodetic coordinates (lat, lon, height) to geocentric ECEF.
///
/// # Arguments
/// * `lat` — latitude in radians
/// * `lon` — longitude in radians
/// * `h` — ellipsoidal height in meters
/// * `ellipsoid` — reference ellipsoid
pub fn geodetic_to_geocentric(
    lat: f64,
    lon: f64,
    h: f64,
    ellipsoid: &Ellipsoid,
) -> GeocentricCoord {
    let a = ellipsoid.a;
    let e2 = ellipsoid.e2();

    let sin_lat = lat.sin();
    let cos_lat = lat.cos();

    // Radius of curvature in the prime vertical
    let n = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();

    GeocentricCoord {
        x: (n + h) * cos_lat * lon.cos(),
        y: (n + h) * cos_lat * lon.sin(),
        z: (n * (1.0 - e2) + h) * sin_lat,
    }
}

/// Convert geocentric ECEF coordinates to geodetic (lat, lon, height).
///
/// Uses Bowring's iterative method for high accuracy.
///
/// # Returns
/// (latitude_radians, longitude_radians, height_meters)
pub fn geocentric_to_geodetic(coord: &GeocentricCoord, ellipsoid: &Ellipsoid) -> (f64, f64, f64) {
    let a = ellipsoid.a;
    let b = ellipsoid.b();
    let e2 = ellipsoid.e2();
    let ep2 = (a * a - b * b) / (b * b); // second eccentricity squared

    let p = (coord.x * coord.x + coord.y * coord.y).sqrt();
    let lon = coord.y.atan2(coord.x);

    // Bowring's iterative method
    let mut lat = (coord.z / p * (1.0 + ep2)).atan(); // initial approximation

    for _ in 0..10 {
        let sin_lat = lat.sin();
        let cos_lat = lat.cos();
        let n = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();

        let lat_new = (coord.z + e2 * n * sin_lat).atan2(p);
        if (lat_new - lat).abs() < 1e-12 {
            lat = lat_new;
            break;
        }
        lat = lat_new;
        let _ = cos_lat; // suppress unused warning
    }

    let sin_lat = lat.sin();
    let n = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();
    let h = if lat.cos().abs() > 1e-10 {
        p / lat.cos() - n
    } else {
        coord.z / lat.sin() - n * (1.0 - e2)
    };

    (lat, lon, h)
}

/// High-level datum transformation: geodetic on source ellipsoid → geodetic on target ellipsoid.
///
/// Performs: geodetic→geocentric→Helmert→geocentric→geodetic
pub fn transform_datum(
    lat: f64,
    lon: f64,
    h: f64,
    source_ellipsoid: &Ellipsoid,
    target_ellipsoid: &Ellipsoid,
    helmert: &HelmertTransform,
) -> (f64, f64, f64) {
    let geocentric = geodetic_to_geocentric(lat, lon, h, source_ellipsoid);
    let transformed = helmert.forward(&geocentric);
    geocentric_to_geodetic(&transformed, target_ellipsoid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geodetic_geocentric_roundtrip() {
        let wgs84 = Ellipsoid::WGS84;
        let lat = 51.0_f64.to_radians(); // London-ish
        let lon = -1.0_f64.to_radians();
        let h = 100.0;

        let ecef = geodetic_to_geocentric(lat, lon, h, &wgs84);
        let (lat2, lon2, h2) = geocentric_to_geodetic(&ecef, &wgs84);

        assert!(
            (lat - lat2).abs() < 1e-10,
            "lat diff: {}",
            (lat - lat2).abs()
        );
        assert!(
            (lon - lon2).abs() < 1e-10,
            "lon diff: {}",
            (lon - lon2).abs()
        );
        assert!((h - h2).abs() < 1e-4, "h diff: {}", (h - h2).abs());
    }

    #[test]
    fn test_geodetic_geocentric_equator() {
        let wgs84 = Ellipsoid::WGS84;
        let lat = 0.0;
        let lon = 0.0;
        let h = 0.0;

        let ecef = geodetic_to_geocentric(lat, lon, h, &wgs84);
        // On equator at prime meridian, X should equal semi-major axis
        assert!((ecef.x - wgs84.a).abs() < 0.001);
        assert!(ecef.y.abs() < 1e-10);
        assert!(ecef.z.abs() < 1e-10);
    }

    #[test]
    fn test_helmert_identity() {
        // Zero parameters should be identity
        let identity = HelmertTransform::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let coord = GeocentricCoord {
            x: 3_000_000.0,
            y: 4_000_000.0,
            z: 5_000_000.0,
        };
        let result = identity.forward(&coord);
        assert!((result.x - coord.x).abs() < 1e-6);
        assert!((result.y - coord.y).abs() < 1e-6);
        assert!((result.z - coord.z).abs() < 1e-6);
    }

    #[test]
    fn test_helmert_osgb36_to_wgs84() {
        // Known test: convert a point from OSGB36 to WGS84
        let osgb36 = Ellipsoid::new(6_377_563.396, 1.0 / 299.3249646);
        let wgs84 = Ellipsoid::WGS84;
        let helmert = HelmertTransform::osgb36_to_wgs84();

        // Tower of London approximate OSGB36 coordinates
        let lat = 51.5081_f64.to_radians();
        let lon = -0.0761_f64.to_radians();
        let h = 0.0;

        let (lat_wgs, lon_wgs, _) = transform_datum(lat, lon, h, &osgb36, &wgs84, &helmert);

        // Should shift by roughly 70m north and 120m east
        let dlat = (lat_wgs - lat).to_degrees() * 111_000.0; // approximate meters
        let dlon = (lon_wgs - lon).to_degrees() * 111_000.0 * lat.cos();

        // OSGB36→WGS84 shift is typically ~120m
        let shift = (dlat * dlat + dlon * dlon).sqrt();
        assert!(
            shift > 50.0 && shift < 200.0,
            "Helmert shift should be ~120m, got {shift:.1}m"
        );
    }

    #[test]
    fn test_helmert_forward_inverse_roundtrip() {
        let helmert = HelmertTransform::osgb36_to_wgs84();
        let coord = GeocentricCoord {
            x: 3_790_644.9,
            y: -110_149.2,
            z: 4_984_924.4,
        };

        let forward = helmert.forward(&coord);
        let back = helmert.inverse(&forward);

        // Should be approximately back to original (not exact due to linearization)
        assert!(
            (back.x - coord.x).abs() < 0.1,
            "x diff: {}",
            (back.x - coord.x).abs()
        );
        assert!(
            (back.y - coord.y).abs() < 0.1,
            "y diff: {}",
            (back.y - coord.y).abs()
        );
        assert!(
            (back.z - coord.z).abs() < 0.1,
            "z diff: {}",
            (back.z - coord.z).abs()
        );
    }
}
