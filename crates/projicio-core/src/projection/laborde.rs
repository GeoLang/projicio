// ported from proj-rust (https://github.com/pka/proj-rust), MIT OR Apache-2.0

use std::f64::consts::FRAC_PI_4;

use crate::projection::Projection;
use crate::{Coord, Ellipsoid, Error, Geographic};

use super::series::normalize_longitude;

const INVERSE_TOLERANCE: f64 = 1e-10;
const INVERSE_ITERATIONS: usize = 20;

/// Laborde Oblique Mercator (EPSG method 9813), the Madagascar grid.
///
/// A conformal-sphere construction with a cubic rotation correction for the grid
/// azimuth.
#[derive(Debug, Clone)]
pub struct Laborde {
    ellipsoid: Ellipsoid,
    lon0: f64,
    lat0: f64,
    k0: f64,
    false_easting: f64,
    false_northing: f64,
    /// The scaled Gaussian sphere radius, k0 times the geometric mean of the
    /// two radii of curvature at the centre.
    scaled_radius: f64,
    /// Latitude of the origin on the conformal sphere.
    sphere_lat0: f64,
    /// Conformal sphere exponent.
    exponent: f64,
    /// Integration constant of the conformal mapping.
    constant: f64,
    rotation_a: f64,
    rotation_b: f64,
    rotation_c: f64,
    rotation_d: f64,
}

impl Laborde {
    /// * `lat0`, `lon0` — latitude and longitude of projection centre in degrees
    /// * `azimuth` — azimuth of the central line at the centre, in degrees
    /// * `k0` — scale factor at the projection centre
    /// * `false_easting`, `false_northing` — in meters
    pub fn new(
        ellipsoid: Ellipsoid,
        lat0: f64,
        lon0: f64,
        azimuth: f64,
        k0: f64,
        false_easting: f64,
        false_northing: f64,
    ) -> Result<Self, Error> {
        let lat0 = lat0.to_radians();
        if lat0 == 0.0 {
            return Err(Error::UnsupportedCrs(
                "laborde needs a projection centre off the equator".into(),
            ));
        }
        let azimuth = azimuth.to_radians();

        let e2 = ellipsoid.e2();
        let e = ellipsoid.e();
        let one_minus_e2 = 1.0 - e2;
        let sin_lat0 = lat0.sin();
        let t = 1.0 - e2 * sin_lat0 * sin_lat0;
        let prime_vertical = 1.0 / t.sqrt();
        let meridian_radius = one_minus_e2 * prime_vertical / t;
        let scaled_radius = k0 * (prime_vertical * meridian_radius).sqrt();
        let sphere_lat0 = ((meridian_radius / prime_vertical).sqrt() * lat0.tan()).atan();
        let exponent = sin_lat0 / sphere_lat0.sin();
        let e_sin = e * sin_lat0;
        let constant = 0.5 * e * exponent * ((1.0 + e_sin) / (1.0 - e_sin)).ln()
            - exponent * (FRAC_PI_4 + 0.5 * lat0).tan().ln()
            + (FRAC_PI_4 + 0.5 * sphere_lat0).tan().ln();

        let double_azimuth = azimuth + azimuth;
        let rotation_scale = 1.0 / (12.0 * scaled_radius * scaled_radius);
        let rotation_a = (1.0 - double_azimuth.cos()) * rotation_scale;
        let rotation_b = double_azimuth.sin() * rotation_scale;

        Ok(Self {
            ellipsoid,
            lon0: lon0.to_radians(),
            lat0,
            k0,
            false_easting,
            false_northing,
            scaled_radius,
            sphere_lat0,
            exponent,
            constant,
            rotation_a,
            rotation_b,
            rotation_c: 3.0 * (rotation_a * rotation_a - rotation_b * rotation_b),
            rotation_d: 6.0 * rotation_a * rotation_b,
        })
    }

    /// Conformal-sphere latitude for a geodetic latitude.
    fn conformal_sphere_lat(&self, lat: f64) -> f64 {
        let first = self.exponent * (FRAC_PI_4 + 0.5 * lat).tan().ln();
        let e_sin = self.ellipsoid.e() * lat.sin();
        let second =
            0.5 * self.ellipsoid.e() * self.exponent * ((1.0 + e_sin) / (1.0 - e_sin)).ln();
        2.0 * ((first - second + self.constant).exp().atan() - FRAC_PI_4)
    }
}

impl Projection for Laborde {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let lat = geo.lat.to_radians();
        let lam = normalize_longitude(geo.lon.to_radians() - self.lon0);

        let sphere_lat = self.conformal_sphere_lat(lat);
        let term1 = sphere_lat - self.sphere_lat0;
        let cos_lat = sphere_lat.cos();
        let cos2 = cos_lat * cos_lat;
        let sin_lat = sphere_lat.sin();
        let sin2 = sin_lat * sin_lat;
        let exponent2 = self.exponent * self.exponent;
        let term4 = self.exponent * cos_lat;
        let term2 = 0.5 * self.exponent * term4 * sin_lat;
        let term3 = term2 * exponent2 * (5.0 * cos2 - sin2) / 12.0;
        let mut term6 = term4 * exponent2;
        let term5 = term6 * (cos2 - sin2) / 6.0;
        term6 *= exponent2 * (5.0 * cos2 * cos2 + sin2 * (sin2 - 18.0 * cos2)) / 120.0;

        let lam2 = lam * lam;
        let x = self.scaled_radius * lam * (term4 + lam2 * (term5 + lam2 * term6));
        let y = self.scaled_radius * (term1 + lam2 * (term2 + lam2 * term3));
        let x2 = x * x;
        let y2 = y * y;
        let cubic1 = 3.0 * x * y2 - x * x2;
        let cubic2 = y * y2 - 3.0 * x2 * y;
        let x = x + self.rotation_a * cubic1 + self.rotation_b * cubic2;
        let y = y + self.rotation_a * cubic2 - self.rotation_b * cubic1;

        let a = self.ellipsoid.a;
        Ok(Coord::new(
            self.false_easting + a * x,
            self.false_northing + a * y,
        ))
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        let a = self.ellipsoid.a;
        let mut x = (coord.x - self.false_easting) / a;
        let mut y = (coord.y - self.false_northing) / a;

        let x2 = x * x;
        let y2 = y * y;
        let cubic1 = 3.0 * x * y2 - x * x2;
        let cubic2 = y * y2 - 3.0 * x2 * y;
        let quintic1 = x * (5.0 * y2 * y2 + x2 * (-10.0 * y2 + x2));
        let quintic2 = y * (5.0 * x2 * x2 + y2 * (-10.0 * x2 + y2));
        x += -self.rotation_a * cubic1 - self.rotation_b * cubic2
            + self.rotation_c * quintic1
            + self.rotation_d * quintic2;
        y += self.rotation_b * cubic1 - self.rotation_a * cubic2 - self.rotation_d * quintic1
            + self.rotation_c * quintic2;

        let sphere_lat = self.sphere_lat0 + y / self.scaled_radius;
        let mut lat = sphere_lat + self.lat0 - self.sphere_lat0;
        let mut converged = false;
        for _ in 0..INVERSE_ITERATIONS {
            let step = sphere_lat - self.conformal_sphere_lat(lat);
            lat += step;
            if step.abs() < INVERSE_TOLERANCE {
                converged = true;
                break;
            }
        }
        if !converged {
            return Err(Error::ProjectionError(format!(
                "laborde inverse latitude did not converge in {INVERSE_ITERATIONS} iterations"
            )));
        }

        let e_sin = self.ellipsoid.e() * lat.sin();
        let t = 1.0 - e_sin * e_sin;
        let meridian_radius = (1.0 - self.ellipsoid.e2()) / (t * t.sqrt());
        let tan_sphere = sphere_lat.tan();
        let tan2 = tan_sphere * tan_sphere;
        let radius2 = self.scaled_radius * self.scaled_radius;
        let along = meridian_radius * self.k0 * self.scaled_radius;
        let term7 = tan_sphere / (2.0 * along);
        let term8 = tan_sphere * (5.0 + 3.0 * tan2) / (24.0 * along * radius2);
        let across = sphere_lat.cos() * self.scaled_radius * self.exponent;
        let term9 = 1.0 / across;
        let across = across * radius2;
        let term10 = (1.0 + 2.0 * tan2) / (6.0 * across);
        let term11 = (5.0 + tan2 * (28.0 + 24.0 * tan2)) / (120.0 * across * radius2);

        let x2 = x * x;
        let lat = lat + x2 * (-term7 + term8 * x2);
        let lam = x * (term9 + x2 * (-term10 + x2 * term11));
        Ok(Geographic::new(
            normalize_longitude(self.lon0 + lam).to_degrees(),
            lat.to_degrees(),
        ))
    }

    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tananarive 1925 / Laborde Grid, EPSG:8441.
    fn madagascar() -> Laborde {
        Laborde::new(
            Ellipsoid::INTERNATIONAL_1924,
            -18.9,
            46.437_229_166_666_67,
            18.9,
            0.9995,
            400_000.0,
            800_000.0,
        )
        .unwrap()
    }

    #[test]
    fn test_centre_maps_to_false_origin() {
        let projected = madagascar()
            .forward(Geographic::new(46.437_229_166_666_67, -18.9))
            .unwrap();
        assert!((projected.x - 400_000.0).abs() < 1e-6, "{}", projected.x);
        assert!((projected.y - 800_000.0).abs() < 1e-6, "{}", projected.y);
    }

    /// The projection is conformal, so the scale at a point is the same in every
    /// direction. Dividing the grid distance by the ground distance east and
    /// north catches a wrong exponent or rotation term.
    #[test]
    fn test_scale_is_isotropic() {
        let projection = madagascar();
        for (lon, lat) in [(47.5_f64, -18.9_f64), (44.5, -16.2), (48.8, -22.3)] {
            let step = 1e-4_f64;
            let centre = projection.forward(Geographic::new(lon, lat)).unwrap();
            let east = projection
                .forward(Geographic::new(lon + step, lat))
                .unwrap();
            let north = projection
                .forward(Geographic::new(lon, lat + step))
                .unwrap();
            let east_scale = ((east.x - centre.x).powi(2) + (east.y - centre.y).powi(2)).sqrt()
                / (prime_vertical_radius(lat.to_radians())
                    * lat.to_radians().cos()
                    * step.to_radians());
            let north_scale = ((north.x - centre.x).powi(2) + (north.y - centre.y).powi(2)).sqrt()
                / (meridian_radius(lat.to_radians()) * step.to_radians());
            assert!(
                (east_scale / north_scale - 1.0).abs() < 1e-6,
                "at {lon},{lat}: {east_scale} vs {north_scale}"
            );
        }
    }

    #[test]
    fn test_scale_at_the_centre_is_the_scale_factor() {
        let projection = madagascar();
        let (lon, lat) = (46.437_229_166_666_67_f64, -18.9_f64);
        let step = 1e-5;
        let centre = projection.forward(Geographic::new(lon, lat)).unwrap();
        let north = projection
            .forward(Geographic::new(lon, lat + step))
            .unwrap();
        let distance = ((north.x - centre.x).powi(2) + (north.y - centre.y).powi(2)).sqrt();
        let meridian_arc = meridian_radius(lat.to_radians()) * step.to_radians();
        assert!(
            (distance / meridian_arc - 0.9995).abs() < 1e-6,
            "{}",
            distance / meridian_arc
        );
    }

    fn meridian_radius(lat: f64) -> f64 {
        let ellipsoid = Ellipsoid::INTERNATIONAL_1924;
        let e2 = ellipsoid.e2();
        let t = 1.0 - e2 * lat.sin() * lat.sin();
        ellipsoid.a * (1.0 - e2) / (t * t.sqrt())
    }

    fn prime_vertical_radius(lat: f64) -> f64 {
        let ellipsoid = Ellipsoid::INTERNATIONAL_1924;
        let e2 = ellipsoid.e2();
        ellipsoid.a / (1.0 - e2 * lat.sin() * lat.sin()).sqrt()
    }

    #[test]
    fn test_roundtrip() {
        let projection = madagascar();
        for (lon, lat) in [
            (47.5_f64, -18.9_f64),
            (44.5, -16.2),
            (48.8, -22.3),
            (46.437_229_166_666_67, -18.9),
        ] {
            let geo = Geographic::new(lon, lat);
            let recovered = projection
                .inverse(projection.forward(geo).unwrap())
                .unwrap();
            assert!((recovered.lon - lon).abs() < 1e-7, "{}", recovered.lon);
            assert!((recovered.lat - lat).abs() < 1e-7, "{}", recovered.lat);
        }
    }

    #[test]
    fn test_rejects_equatorial_centre() {
        assert!(Laborde::new(Ellipsoid::GRS80, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0).is_err());
    }
}
