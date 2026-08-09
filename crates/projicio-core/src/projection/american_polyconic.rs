// ported from proj-rust (https://github.com/pka/proj-rust), MIT OR Apache-2.0

use crate::projection::Projection;
use crate::{Coord, Ellipsoid, Error, Geographic};

use super::series::{meridian_arc, meridian_arc_coefficients, normalize_longitude};

const EQUATOR_TOLERANCE: f64 = 1e-10;
const NEWTON_TOLERANCE: f64 = 1e-12;
const NEWTON_ITERATIONS: usize = 20;

/// American Polyconic (EPSG method 9818).
///
/// Each parallel is projected as the arc of a tangent cone, so the projection is
/// neither conformal nor equal-area. It survives in the Brazilian national grids.
#[derive(Debug, Clone)]
pub struct AmericanPolyconic {
    ellipsoid: Ellipsoid,
    lon0: f64,
    lat0: f64,
    false_easting: f64,
    false_northing: f64,
    arc_coefficients: [f64; 7],
    /// Meridional arc at the latitude of origin, in semi-major-axis units.
    arc_at_origin: f64,
}

impl AmericanPolyconic {
    /// * `lat0`, `lon0` — latitude and longitude of natural origin in degrees
    /// * `false_easting`, `false_northing` — in meters
    pub fn new(
        ellipsoid: Ellipsoid,
        lat0: f64,
        lon0: f64,
        false_easting: f64,
        false_northing: f64,
    ) -> Self {
        let lat0 = lat0.to_radians();
        let e2 = ellipsoid.e2();
        let arc_coefficients = meridian_arc_coefficients(e2);
        let arc_at_origin = if e2 == 0.0 {
            -lat0
        } else {
            meridian_arc(lat0, lat0.sin(), lat0.cos(), &arc_coefficients)
        };

        Self {
            ellipsoid,
            lon0: lon0.to_radians(),
            lat0,
            false_easting,
            false_northing,
            arc_coefficients,
            arc_at_origin,
        }
    }
}

impl Projection for AmericanPolyconic {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let lat = geo.lat.to_radians();
        let lam = normalize_longitude(geo.lon.to_radians() - self.lon0);
        let e2 = self.ellipsoid.e2();

        let (x, y) = if e2 == 0.0 {
            if lat.abs() <= EQUATOR_TOLERANCE {
                (lam, self.arc_at_origin)
            } else {
                let cotangent = 1.0 / lat.tan();
                let e = lam * lat.sin();
                (
                    e.sin() * cotangent,
                    lat - self.lat0 + cotangent * (1.0 - e.cos()),
                )
            }
        } else if lat.abs() <= EQUATOR_TOLERANCE {
            (lam, -self.arc_at_origin)
        } else {
            let sin_lat = lat.sin();
            let cos_lat = lat.cos();
            let cone = if cos_lat.abs() > EQUATOR_TOLERANCE {
                cos_lat / (1.0 - e2 * sin_lat * sin_lat).sqrt() / sin_lat
            } else {
                0.0
            };
            let e = lam * sin_lat;
            (
                cone * e.sin(),
                (meridian_arc(lat, sin_lat, cos_lat, &self.arc_coefficients) - self.arc_at_origin)
                    + cone * (1.0 - e.cos()),
            )
        };

        let a = self.ellipsoid.a;
        Ok(Coord::new(
            self.false_easting + a * x,
            self.false_northing + a * y,
        ))
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        let a = self.ellipsoid.a;
        let e2 = self.ellipsoid.e2();
        let x = (coord.x - self.false_easting) / a;

        if e2 == 0.0 {
            let y = self.lat0 + (coord.y - self.false_northing) / a;
            if y.abs() <= EQUATOR_TOLERANCE {
                return Ok(Geographic::new(
                    normalize_longitude(self.lon0 + x).to_degrees(),
                    0.0,
                ));
            }
            let radius = x * x + y * y;
            let mut lat = y;
            for iteration in 0.. {
                if iteration == NEWTON_ITERATIONS {
                    return Err(non_convergence());
                }
                let tan_lat = lat.tan();
                let step = (y * (lat * tan_lat + 1.0) - lat - 0.5 * (lat * lat + radius) * tan_lat)
                    / ((lat - y) / tan_lat - 1.0);
                lat -= step;
                if step.abs() <= NEWTON_TOLERANCE {
                    break;
                }
            }
            let lam = (x * lat.tan()).asin() / lat.sin();
            return Ok(Geographic::new(
                normalize_longitude(self.lon0 + lam).to_degrees(),
                lat.to_degrees(),
            ));
        }

        let y = (coord.y - self.false_northing) / a + self.arc_at_origin;
        if y.abs() <= EQUATOR_TOLERANCE {
            return Ok(Geographic::new(
                normalize_longitude(self.lon0 + x).to_degrees(),
                0.0,
            ));
        }

        let radius = y * y + x * x;
        let mut lat = y;
        for iteration in 0.. {
            if iteration == NEWTON_ITERATIONS {
                return Err(non_convergence());
            }
            let sin_lat = lat.sin();
            let cos_lat = lat.cos();
            if cos_lat.abs() < NEWTON_TOLERANCE {
                return Err(Error::InvalidCoordinate(
                    "american polyconic inverse is undefined at the pole".into(),
                ));
            }
            let sin_cos = sin_lat * cos_lat;
            let nu = (1.0 - e2 * sin_lat * sin_lat).sqrt();
            let c = sin_lat * nu / cos_lat;
            let arc = meridian_arc(lat, sin_lat, cos_lat, &self.arc_coefficients);
            let arc_radius = arc * arc + radius;
            let arc_slope = (1.0 - e2) / (nu * nu * nu);
            let step = (arc + arc + c * arc_radius - 2.0 * y * (c * arc + 1.0))
                / (e2 * sin_cos * (arc_radius - 2.0 * y * arc) / c
                    + 2.0 * (y - arc) * (c * arc_slope - 1.0 / sin_cos)
                    - arc_slope
                    - arc_slope);
            lat += step;
            if step.abs() <= NEWTON_TOLERANCE {
                break;
            }
        }

        let sin_lat = lat.sin();
        let lam = (x * lat.tan() * (1.0 - e2 * sin_lat * sin_lat).sqrt()).asin() / sin_lat;
        Ok(Geographic::new(
            normalize_longitude(self.lon0 + lam).to_degrees(),
            lat.to_degrees(),
        ))
    }

    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
}

fn non_convergence() -> Error {
    Error::ProjectionError(format!(
        "american polyconic inverse latitude did not converge in {NEWTON_ITERATIONS} iterations"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SIRGAS 2000 / Brazil Polyconic, EPSG:5880.
    fn brazil_polyconic() -> AmericanPolyconic {
        AmericanPolyconic::new(Ellipsoid::GRS80, 0.0, -54.0, 5_000_000.0, 10_000_000.0)
    }

    #[test]
    fn test_natural_origin_maps_to_false_origin() {
        let projected = brazil_polyconic()
            .forward(Geographic::new(-54.0, 0.0))
            .unwrap();
        assert!((projected.x - 5_000_000.0).abs() < 1e-6, "{}", projected.x);
        assert!((projected.y - 10_000_000.0).abs() < 1e-6, "{}", projected.y);
    }

    /// The central meridian is a true-scale meridian arc, so northing there is
    /// the meridional distance from the equator. Compared against Bessel's
    /// series for the meridian arc, written out independently below.
    #[test]
    fn test_central_meridian_is_the_meridian_arc() {
        let projection = brazil_polyconic();
        for lat in [-20.0_f64, -5.0, 5.0, 25.0] {
            let projected = projection.forward(Geographic::new(-54.0, lat)).unwrap();
            let arc = meridian_arc_series(lat.to_radians(), Ellipsoid::GRS80);
            assert!(
                (projected.y - 10_000_000.0 - arc).abs() < 1e-3,
                "lat {lat}: {} vs {arc}",
                projected.y - 10_000_000.0
            );
            assert!((projected.x - 5_000_000.0).abs() < 1e-6, "{}", projected.x);
        }
    }

    /// Meridional arc from the classic expansion in the eccentricity, which is a
    /// different series from the one the projection uses.
    fn meridian_arc_series(lat: f64, ellipsoid: Ellipsoid) -> f64 {
        let e2 = ellipsoid.e2();
        let e4 = e2 * e2;
        let e6 = e4 * e2;
        ellipsoid.a
            * ((1.0 - e2 / 4.0 - 3.0 * e4 / 64.0 - 5.0 * e6 / 256.0) * lat
                - (3.0 * e2 / 8.0 + 3.0 * e4 / 32.0 + 45.0 * e6 / 1024.0) * (2.0 * lat).sin()
                + (15.0 * e4 / 256.0 + 45.0 * e6 / 1024.0) * (4.0 * lat).sin()
                - (35.0 * e6 / 3072.0) * (6.0 * lat).sin())
    }

    #[test]
    fn test_equator_is_linear() {
        let projection = AmericanPolyconic::new(Ellipsoid::GRS80, 0.0, 0.0, 0.0, 0.0);
        let projected = projection.forward(Geographic::new(2.0, 0.0)).unwrap();
        assert!(projected.y.abs() < 1e-9, "{}", projected.y);
        assert!(
            (projected.x - Ellipsoid::GRS80.a * 2.0_f64.to_radians()).abs() < 1e-6,
            "{}",
            projected.x
        );
    }

    #[test]
    fn test_roundtrip() {
        let projection = brazil_polyconic();
        for lon in [-67.8_f64, -54.0, -43.2] {
            for lat in [-22.9_f64, -9.97, 2.8] {
                let geo = Geographic::new(lon, lat);
                let recovered = projection
                    .inverse(projection.forward(geo).unwrap())
                    .unwrap();
                assert!((recovered.lon - lon).abs() < 1e-9, "{}", recovered.lon);
                assert!((recovered.lat - lat).abs() < 1e-9, "{}", recovered.lat);
            }
        }
    }

    #[test]
    fn test_spherical_roundtrip() {
        let projection = AmericanPolyconic::new(Ellipsoid::SPHERE, 0.0, 0.0, 0.0, 0.0);
        for (lon, lat) in [(2.0, 1.0), (-2.0, -1.0), (10.0, 30.0)] {
            let geo = Geographic::new(lon, lat);
            let recovered = projection
                .inverse(projection.forward(geo).unwrap())
                .unwrap();
            assert!((recovered.lon - lon).abs() < 1e-9, "{}", recovered.lon);
            assert!((recovered.lat - lat).abs() < 1e-9, "{}", recovered.lat);
        }
    }
}
