use std::f64::consts::FRAC_PI_2;

use crate::projection::Projection;
use crate::{Coord, Ellipsoid, Error, Geographic};

use super::series::{authalic_q, geodetic_from_authalic, normalize_longitude};

/// A pole is treated as polar when the origin is this close to ±90 degrees.
const POLAR_COSINE: f64 = 1e-10;

/// Lambert Azimuthal Equal Area (EPSG method 9820).
///
/// Ellipsoidal form used by ETRS-LAEA (EPSG:3035) and polar equal-area grids.
#[derive(Debug, Clone)]
pub struct LambertAzimuthalEqualArea {
    ellipsoid: Ellipsoid,
    lat0: f64,
    lon0: f64,
    false_easting: f64,
    false_northing: f64,
    qp: f64,
    rq: f64,
    sin_beta0: f64,
    cos_beta0: f64,
    d: f64,
    aspect: Aspect,
}

#[derive(Debug, Clone, Copy)]
enum Aspect {
    Oblique,
    NorthPolar,
    SouthPolar,
}

impl LambertAzimuthalEqualArea {
    /// * `lat0`, `lon0` — latitude and longitude of origin in degrees
    /// * `false_easting`, `false_northing` — in meters
    pub fn new(
        ellipsoid: Ellipsoid,
        lat0: f64,
        lon0: f64,
        false_easting: f64,
        false_northing: f64,
    ) -> Self {
        let phi0 = lat0.to_radians();
        let e2 = ellipsoid.e2();
        let qp = authalic_q(FRAC_PI_2, e2);
        let q0 = authalic_q(phi0, e2);
        let rq = ellipsoid.a * (qp / 2.0).sqrt();
        let sin_beta0 = (q0 / qp).clamp(-1.0, 1.0);
        let beta0 = sin_beta0.asin();
        let cos_beta0 = beta0.cos();

        let aspect = if phi0.cos().abs() < POLAR_COSINE {
            if phi0 >= 0.0 {
                Aspect::NorthPolar
            } else {
                Aspect::SouthPolar
            }
        } else {
            Aspect::Oblique
        };

        let d = match aspect {
            Aspect::Oblique => {
                ellipsoid.a * phi0.cos() / ((1.0 - e2 * phi0.sin().powi(2)).sqrt() * rq * cos_beta0)
            }
            Aspect::NorthPolar | Aspect::SouthPolar => 1.0,
        };

        Self {
            ellipsoid,
            lat0: phi0,
            lon0: lon0.to_radians(),
            false_easting,
            false_northing,
            qp,
            rq,
            sin_beta0,
            cos_beta0,
            d,
            aspect,
        }
    }
}

impl Projection for LambertAzimuthalEqualArea {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let lat = geo.lat.to_radians();
        let lam = normalize_longitude(geo.lon.to_radians() - self.lon0);
        let q = authalic_q(lat, self.ellipsoid.e2());
        let a = self.ellipsoid.a;

        match self.aspect {
            Aspect::NorthPolar => {
                if lat >= 0.0 && lat.cos().abs() < POLAR_COSINE {
                    return Ok(Coord::new(self.false_easting, self.false_northing));
                }
                let rho = a * (self.qp - q).max(0.0).sqrt();
                Ok(Coord::new(
                    self.false_easting + rho * lam.sin(),
                    self.false_northing - rho * lam.cos(),
                ))
            }
            Aspect::SouthPolar => {
                if lat <= 0.0 && lat.cos().abs() < POLAR_COSINE {
                    return Ok(Coord::new(self.false_easting, self.false_northing));
                }
                let rho = a * (self.qp + q).max(0.0).sqrt();
                Ok(Coord::new(
                    self.false_easting + rho * lam.sin(),
                    self.false_northing + rho * lam.cos(),
                ))
            }
            Aspect::Oblique => {
                let sin_beta = (q / self.qp).clamp(-1.0, 1.0);
                let cos_beta = (1.0 - sin_beta * sin_beta).sqrt();
                let denom = 1.0 + self.sin_beta0 * sin_beta + self.cos_beta0 * cos_beta * lam.cos();
                if denom <= 0.0 {
                    return Err(Error::InvalidCoordinate(
                        "point is antipodal to the Lambert Azimuthal Equal Area origin".into(),
                    ));
                }
                let b = self.rq * (2.0 / denom).sqrt();
                Ok(Coord::new(
                    self.false_easting + b * self.d * cos_beta * lam.sin(),
                    self.false_northing
                        + (b / self.d)
                            * (self.cos_beta0 * sin_beta - self.sin_beta0 * cos_beta * lam.cos()),
                ))
            }
        }
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        let x = coord.x - self.false_easting;
        let y = coord.y - self.false_northing;
        let e2 = self.ellipsoid.e2();
        let a = self.ellipsoid.a;

        let (beta, lon) = match self.aspect {
            Aspect::NorthPolar => {
                let rho = x.hypot(y);
                if rho == 0.0 {
                    return Ok(Geographic::new(self.lon0.to_degrees(), 90.0));
                }
                let sin_beta = (1.0 - (rho / a).powi(2) / self.qp).clamp(-1.0, 1.0);
                (sin_beta.asin(), self.lon0 + x.atan2(-y))
            }
            Aspect::SouthPolar => {
                let rho = x.hypot(y);
                if rho == 0.0 {
                    return Ok(Geographic::new(self.lon0.to_degrees(), -90.0));
                }
                let sin_beta = ((rho / a).powi(2) / self.qp - 1.0).clamp(-1.0, 1.0);
                (sin_beta.asin(), self.lon0 + x.atan2(y))
            }
            Aspect::Oblique => {
                let rho = (x / self.d).hypot(self.d * y);
                if rho == 0.0 {
                    return Ok(Geographic::new(
                        self.lon0.to_degrees(),
                        self.lat0.to_degrees(),
                    ));
                }
                let c_arg = (rho / (2.0 * self.rq)).clamp(-1.0, 1.0);
                let c = 2.0 * c_arg.asin();
                let sin_c = c.sin();
                let cos_c = c.cos();
                let sin_beta = (cos_c * self.sin_beta0
                    + (self.d * y * sin_c * self.cos_beta0) / rho)
                    .clamp(-1.0, 1.0);
                let lon = self.lon0
                    + (x * sin_c).atan2(
                        self.d * rho * self.cos_beta0 * cos_c
                            - self.d * self.d * y * self.sin_beta0 * sin_c,
                    );
                (sin_beta.asin(), lon)
            }
        };

        Ok(Geographic::new(
            normalize_longitude(lon).to_degrees(),
            geodetic_from_authalic(beta, e2).to_degrees(),
        ))
    }

    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn etrs_laea() -> LambertAzimuthalEqualArea {
        LambertAzimuthalEqualArea::new(Ellipsoid::GRS80, 52.0, 10.0, 4_321_000.0, 3_210_000.0)
    }

    /// IOGP Geomatics Guidance Note 7 part 2 (373-07-02, September 2019) page 79,
    /// Lambert Azimuthal Equal Area worked example for ETRS89 / ETRS-LAEA:
    /// lat 50 deg N, lon 5 deg E gives E 3962799.45 m, N 2999718.85 m.
    #[test]
    fn test_etrs_laea_iogp_worked_example() {
        let projected = etrs_laea().forward(Geographic::new(5.0, 50.0)).unwrap();
        assert!(
            (projected.x - 3_962_799.45).abs() < 0.01,
            "easting {}",
            projected.x
        );
        assert!(
            (projected.y - 2_999_718.85).abs() < 0.01,
            "northing {}",
            projected.y
        );
    }

    #[test]
    fn test_etrs_laea_iogp_worked_example_inverse() {
        let geo = etrs_laea()
            .inverse(Coord::new(3_962_799.45, 2_999_718.85))
            .unwrap();
        assert!((geo.lon - 5.0).abs() < 1e-7, "longitude {}", geo.lon);
        assert!((geo.lat - 50.0).abs() < 1e-7, "latitude {}", geo.lat);
    }

    #[test]
    fn test_origin_maps_to_false_origin() {
        let projected = etrs_laea().forward(Geographic::new(10.0, 52.0)).unwrap();
        assert!((projected.x - 4_321_000.0).abs() < 1e-6, "{}", projected.x);
        assert!((projected.y - 3_210_000.0).abs() < 1e-6, "{}", projected.y);
    }

    #[test]
    fn test_origin_inverse() {
        let geo = etrs_laea()
            .inverse(Coord::new(4_321_000.0, 3_210_000.0))
            .unwrap();
        assert!((geo.lon - 10.0).abs() < 1e-10, "{}", geo.lon);
        assert!((geo.lat - 52.0).abs() < 1e-10, "{}", geo.lat);
    }

    #[test]
    fn test_roundtrip() {
        let projection = etrs_laea();
        for lon in [-10.0_f64, 0.0, 5.0, 10.0, 25.0, 40.0] {
            for lat in [35.0_f64, 50.0, 52.0, 65.0, 80.0] {
                let geo = Geographic::new(lon, lat);
                let recovered = projection
                    .inverse(projection.forward(geo).unwrap())
                    .unwrap();
                assert!(
                    (recovered.lon - lon).abs() < 1e-7,
                    "lon {lon}: {}",
                    recovered.lon
                );
                assert!(
                    (recovered.lat - lat).abs() < 1e-7,
                    "lat {lat}: {}",
                    recovered.lat
                );
            }
        }
    }

    /// On the sphere, 90 degrees of longitude at the equator from an equatorial
    /// origin maps to (R√2, 0).
    #[test]
    fn test_sphere_equator_quarter() {
        let sphere = LambertAzimuthalEqualArea::new(Ellipsoid::SPHERE, 0.0, 0.0, 0.0, 0.0);
        let projected = sphere.forward(Geographic::new(90.0, 0.0)).unwrap();
        let expected = Ellipsoid::SPHERE.a * 2.0_f64.sqrt();
        assert!((projected.x - expected).abs() < 1e-6, "{}", projected.x);
        assert!(projected.y.abs() < 1e-6, "{}", projected.y);
    }

    #[test]
    fn test_north_polar_origin() {
        let projection = LambertAzimuthalEqualArea::new(Ellipsoid::WGS84, 90.0, 10.0, 0.0, 0.0);
        let pole = projection.forward(Geographic::new(0.0, 90.0)).unwrap();
        assert!(pole.x.abs() < 1e-6, "{}", pole.x);
        assert!(pole.y.abs() < 1e-6, "{}", pole.y);

        let on_meridian = projection.forward(Geographic::new(10.0, 80.0)).unwrap();
        assert!(on_meridian.x.abs() < 1e-6, "{}", on_meridian.x);
        assert!(on_meridian.y < 0.0, "{}", on_meridian.y);

        let recovered = projection.inverse(on_meridian).unwrap();
        assert!((recovered.lon - 10.0).abs() < 1e-7, "{}", recovered.lon);
        assert!((recovered.lat - 80.0).abs() < 1e-7, "{}", recovered.lat);
    }

    #[test]
    fn test_south_polar_origin() {
        let projection = LambertAzimuthalEqualArea::new(Ellipsoid::WGS84, -90.0, 0.0, 0.0, 0.0);
        let pole = projection.forward(Geographic::new(20.0, -90.0)).unwrap();
        assert!(pole.x.abs() < 1e-6, "{}", pole.x);
        assert!(pole.y.abs() < 1e-6, "{}", pole.y);

        let recovered = projection
            .inverse(projection.forward(Geographic::new(-40.0, -70.0)).unwrap())
            .unwrap();
        assert!((recovered.lon - (-40.0)).abs() < 1e-7, "{}", recovered.lon);
        assert!((recovered.lat - (-70.0)).abs() < 1e-7, "{}", recovered.lat);
    }

    #[test]
    fn test_antipode_is_invalid() {
        let projection = LambertAzimuthalEqualArea::new(Ellipsoid::SPHERE, 0.0, 0.0, 0.0, 0.0);
        let err = projection.forward(Geographic::new(180.0, 0.0)).unwrap_err();
        assert!(matches!(err, Error::InvalidCoordinate(_)), "{err}");
    }
}
