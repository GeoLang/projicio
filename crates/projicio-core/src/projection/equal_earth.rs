// ported from proj-rust (https://github.com/pka/proj-rust), MIT OR Apache-2.0

use crate::projection::Projection;
use crate::{Coord, Ellipsoid, Error, Geographic};

use super::series::{authalic_q, converge, geodetic_from_authalic, normalize_longitude};

/// Equal Earth (EPSG method 1078).
///
/// Pseudocylindrical equal-area world projection (Šavrič, Patterson and Jenny,
/// 2018). The ellipsoidal form maps through the authalic latitude.
#[derive(Debug, Clone)]
pub struct EqualEarth {
    ellipsoid: Ellipsoid,
    lon0: f64,
    false_easting: f64,
    false_northing: f64,
    /// Polar authalic `q`, for converting to the authalic latitude.
    polar_q: f64,
    /// Authalic radius divided by the semi-major axis.
    authalic_radius_ratio: f64,
}

/// Polynomial coefficients of the Equal Earth projection.
const A1: f64 = 1.340264;
const A2: f64 = -0.081106;
const A3: f64 = 0.000893;
const A4: f64 = 0.003796;

/// The polynomial's value at the pole on the unit sphere, which bounds the
/// northing the inverse can be asked for.
const MAX_NORTHING_RATIO: f64 = 1.3173627591574;

const INVERSE_ITERATIONS: usize = 15;
const INVERSE_TOLERANCE: f64 = 1e-11;

fn parametric_scale() -> f64 {
    3.0_f64.sqrt() / 2.0
}

/// Derivative of the Equal Earth polynomial with respect to its argument.
fn polynomial_derivative(psi2: f64, psi6: f64) -> f64 {
    A1 + 3.0 * A2 * psi2 + psi6 * (7.0 * A3 + 9.0 * A4 * psi2)
}

impl EqualEarth {
    /// * `lon0` — longitude of natural origin in degrees
    /// * `false_easting`, `false_northing` — in meters
    pub fn new(ellipsoid: Ellipsoid, lon0: f64, false_easting: f64, false_northing: f64) -> Self {
        let e2 = ellipsoid.e2();
        let (polar_q, authalic_radius_ratio) = if e2 == 0.0 {
            (2.0, 1.0)
        } else {
            let polar_q = authalic_q(std::f64::consts::FRAC_PI_2, e2);
            (polar_q, (0.5 * polar_q).sqrt())
        };

        Self {
            ellipsoid,
            lon0: lon0.to_radians(),
            false_easting,
            false_northing,
            polar_q,
            authalic_radius_ratio,
        }
    }

    fn scale(&self) -> f64 {
        self.ellipsoid.a * self.authalic_radius_ratio
    }
}

impl Projection for EqualEarth {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let lat = geo.lat.to_radians();
        let lam = normalize_longitude(geo.lon.to_radians() - self.lon0);
        let e2 = self.ellipsoid.e2();

        let sin_beta = if e2 == 0.0 {
            lat.sin()
        } else {
            (authalic_q(lat, e2) / self.polar_q).clamp(-1.0, 1.0)
        };

        let psi = (parametric_scale() * sin_beta).asin();
        let psi2 = psi * psi;
        let psi6 = psi2 * psi2 * psi2;
        let scale = self.scale();

        Ok(Coord::new(
            self.false_easting
                + scale * lam * psi.cos()
                    / (parametric_scale() * polynomial_derivative(psi2, psi6)),
            self.false_northing + scale * psi * (A1 + A2 * psi2 + psi6 * (A3 + A4 * psi2)),
        ))
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        let scale = self.scale();
        let x = (coord.x - self.false_easting) / scale;
        // beyond the pole the polynomial has no root, so clamp to the pole
        let y = ((coord.y - self.false_northing) / scale)
            .clamp(-MAX_NORTHING_RATIO, MAX_NORTHING_RATIO);

        let psi = converge(
            "equal earth inverse latitude",
            y,
            INVERSE_ITERATIONS,
            INVERSE_TOLERANCE,
            |candidate| {
                let c2 = candidate * candidate;
                let c6 = c2 * c2 * c2;
                let residual = candidate * (A1 + A2 * c2 + c6 * (A3 + A4 * c2)) - y;
                candidate - residual / polynomial_derivative(c2, c6)
            },
        )?;

        let psi2 = psi * psi;
        let psi6 = psi2 * psi2 * psi2;
        let lon =
            self.lon0 + parametric_scale() * x * polynomial_derivative(psi2, psi6) / psi.cos();

        let beta = (psi.sin() / parametric_scale()).clamp(-1.0, 1.0).asin();
        let e2 = self.ellipsoid.e2();
        let lat = if e2 == 0.0 {
            beta
        } else {
            geodetic_from_authalic(beta, e2)
        };

        Ok(Geographic::new(
            normalize_longitude(lon).to_degrees(),
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

    fn greenwich() -> EqualEarth {
        EqualEarth::new(Ellipsoid::WGS84, 0.0, 0.0, 0.0)
    }

    #[test]
    fn test_origin_maps_to_origin() {
        let projected = greenwich().forward(Geographic::new(0.0, 0.0)).unwrap();
        assert!(projected.x.abs() < 1e-9, "{}", projected.x);
        assert!(projected.y.abs() < 1e-9, "{}", projected.y);
    }

    /// Šavrič, Patterson and Jenny (2018) give the projection's equatorial
    /// half-width as 2.7066 sphere radii and its polar half-height as 1.3173627
    /// radii, which is what the polynomial is normalised to.
    #[test]
    fn test_published_extents() {
        let sphere = EqualEarth::new(Ellipsoid::SPHERE, 0.0, 0.0, 0.0);
        let radius = Ellipsoid::SPHERE.a;
        let equator = sphere.forward(Geographic::new(180.0, 0.0)).unwrap();
        assert!(
            (equator.x / radius - 2.7066).abs() < 1e-4,
            "{}",
            equator.x / radius
        );
        let pole = sphere.forward(Geographic::new(0.0, 90.0)).unwrap();
        assert!(
            (pole.y / radius - MAX_NORTHING_RATIO).abs() < 1e-9,
            "{}",
            pole.y / radius
        );
    }

    /// The projection is equal-area, so a spherical graticule cell maps to a
    /// planar cell of the same area as the sphere gives it.
    #[test]
    fn test_equal_area_on_the_sphere() {
        let sphere = EqualEarth::new(Ellipsoid::SPHERE, 0.0, 0.0, 0.0);
        let radius = Ellipsoid::SPHERE.a;
        for (lat_low, lat_high) in [(0.0_f64, 10.0_f64), (30.0, 40.0), (60.0, 70.0)] {
            let (lon_low, lon_high) = (0.0, 10.0_f64);
            let corners = [
                sphere.forward(Geographic::new(lon_low, lat_low)).unwrap(),
                sphere.forward(Geographic::new(lon_high, lat_low)).unwrap(),
                sphere.forward(Geographic::new(lon_high, lat_high)).unwrap(),
                sphere.forward(Geographic::new(lon_low, lat_high)).unwrap(),
            ];
            let planar = polygon_area(&corners);
            let spherical = radius
                * radius
                * (lon_high - lon_low).to_radians()
                * (lat_high.to_radians().sin() - lat_low.to_radians().sin());
            // the cell edges are curved, so a straight-edged quadrilateral
            // undercounts by a fraction of a percent over ten degrees
            assert!(
                (planar / spherical - 1.0).abs() < 0.01,
                "{lat_low}..{lat_high}: {planar} vs {spherical}"
            );
        }
    }

    fn polygon_area(corners: &[Coord]) -> f64 {
        let mut sum = 0.0;
        for index in 0..corners.len() {
            let current = corners[index];
            let next = corners[(index + 1) % corners.len()];
            sum += current.x * next.y - next.x * current.y;
        }
        (sum / 2.0).abs()
    }

    #[test]
    fn test_roundtrip() {
        let projection = EqualEarth::new(Ellipsoid::WGS84, -90.0, 500.0, -200.0);
        for lon in [-170.0_f64, -60.0, 0.0, 45.0, 179.0] {
            for lat in [-80.0_f64, -30.0, 0.0, 30.0, 80.0] {
                let geo = Geographic::new(lon, lat);
                let recovered = projection
                    .inverse(projection.forward(geo).unwrap())
                    .unwrap();
                assert!((recovered.lon - lon).abs() < 1e-7, "{}", recovered.lon);
                assert!((recovered.lat - lat).abs() < 1e-7, "{}", recovered.lat);
            }
        }
    }

    #[test]
    fn test_northing_beyond_the_pole_clamps() {
        let recovered = greenwich().inverse(Coord::new(0.0, 9_000_000.0)).unwrap();
        assert!((recovered.lat - 90.0).abs() < 1e-4, "{}", recovered.lat);
    }
}
