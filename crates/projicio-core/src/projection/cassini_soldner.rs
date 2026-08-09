// ported from proj-rust (https://github.com/pka/proj-rust), MIT OR Apache-2.0

use crate::projection::Projection;
use crate::{Coord, Ellipsoid, Error, Geographic};

use super::series::normalize_longitude;

/// Cassini-Soldner projection (EPSG method 9806), used by older cadastral and
/// national grids.
#[derive(Debug, Clone)]
pub struct CassiniSoldner {
    ellipsoid: Ellipsoid,
    lon0: f64,
    false_easting: f64,
    false_northing: f64,
    meridional_arc_at_origin: f64,
    arc_coefficient0: f64,
    arc_coefficient2: f64,
    arc_coefficient4: f64,
    arc_coefficient6: f64,
    footpoint_coefficient2: f64,
    footpoint_coefficient4: f64,
    footpoint_coefficient6: f64,
    footpoint_coefficient8: f64,
}

/// Below this cosine the footpoint meridian is singular and longitude is
/// indeterminate, so the inverse returns the pole instead of amplifying noise.
const POLE_COSINE_EPSILON: f64 = 1e-12;

impl CassiniSoldner {
    /// * `lat0`, `lon0` — latitude and longitude of natural origin in degrees
    /// * `false_easting`, `false_northing` — in meters
    pub fn new(
        ellipsoid: Ellipsoid,
        lat0: f64,
        lon0: f64,
        false_easting: f64,
        false_northing: f64,
    ) -> Self {
        let a = ellipsoid.a;
        let e2 = ellipsoid.e2();
        let e2_2 = e2 * e2;
        let e2_3 = e2_2 * e2;

        let arc_coefficient0 = a * (1.0 - e2 / 4.0 - 3.0 * e2_2 / 64.0 - 5.0 * e2_3 / 256.0);
        let arc_coefficient2 = a * (3.0 * e2 / 8.0 + 3.0 * e2_2 / 32.0 + 45.0 * e2_3 / 1024.0);
        let arc_coefficient4 = a * (15.0 * e2_2 / 256.0 + 45.0 * e2_3 / 1024.0);
        let arc_coefficient6 = a * (35.0 * e2_3 / 3072.0);

        let sqrt_one_minus_e2 = (1.0 - e2).sqrt();
        let e1 = (1.0 - sqrt_one_minus_e2) / (1.0 + sqrt_one_minus_e2);
        let e1_2 = e1 * e1;
        let e1_3 = e1_2 * e1;
        let e1_4 = e1_2 * e1_2;

        Self {
            ellipsoid,
            lon0: lon0.to_radians(),
            false_easting,
            false_northing,
            meridional_arc_at_origin: meridional_arc(
                lat0.to_radians(),
                arc_coefficient0,
                arc_coefficient2,
                arc_coefficient4,
                arc_coefficient6,
            ),
            arc_coefficient0,
            arc_coefficient2,
            arc_coefficient4,
            arc_coefficient6,
            footpoint_coefficient2: 3.0 * e1 / 2.0 - 27.0 * e1_3 / 32.0,
            footpoint_coefficient4: 21.0 * e1_2 / 16.0 - 55.0 * e1_4 / 32.0,
            footpoint_coefficient6: 151.0 * e1_3 / 96.0,
            footpoint_coefficient8: 1097.0 * e1_4 / 512.0,
        }
    }

    fn meridional_arc(&self, phi: f64) -> f64 {
        meridional_arc(
            phi,
            self.arc_coefficient0,
            self.arc_coefficient2,
            self.arc_coefficient4,
            self.arc_coefficient6,
        )
    }
}

fn meridional_arc(phi: f64, coeff0: f64, coeff2: f64, coeff4: f64, coeff6: f64) -> f64 {
    coeff0 * phi - coeff2 * (2.0 * phi).sin() + coeff4 * (4.0 * phi).sin()
        - coeff6 * (6.0 * phi).sin()
}

impl Projection for CassiniSoldner {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let lat = geo.lat.to_radians();
        let delta_lon = normalize_longitude(geo.lon.to_radians() - self.lon0);

        let e2 = self.ellipsoid.e2();
        let sin_phi = lat.sin();
        let cos_phi = lat.cos();
        let tan_phi = lat.tan();
        let prime_vertical = self.ellipsoid.a / (1.0 - e2 * sin_phi * sin_phi).sqrt();
        let t = tan_phi * tan_phi;
        let c = e2 / (1.0 - e2) * cos_phi * cos_phi;
        let a1 = delta_lon * cos_phi;
        let a2 = a1 * a1;
        let a4 = a2 * a2;

        let x = self.false_easting
            + prime_vertical * (a1 - t * a2 * a1 / 6.0 - (8.0 - t + 8.0 * c) * t * a4 * a1 / 120.0);
        let y = self.false_northing + self.meridional_arc(lat) - self.meridional_arc_at_origin
            + prime_vertical * tan_phi * (a2 / 2.0 + (5.0 - t + 6.0 * c) * a4 / 24.0);

        Ok(Coord::new(x, y))
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        let arc = self.meridional_arc_at_origin + coord.y - self.false_northing;
        let mu = arc / self.arc_coefficient0;
        let footpoint = mu
            + self.footpoint_coefficient2 * (2.0 * mu).sin()
            + self.footpoint_coefficient4 * (4.0 * mu).sin()
            + self.footpoint_coefficient6 * (6.0 * mu).sin()
            + self.footpoint_coefficient8 * (8.0 * mu).sin();

        let sin_footpoint = footpoint.sin();
        let cos_footpoint = footpoint.cos();

        if cos_footpoint.abs() < POLE_COSINE_EPSILON {
            let lat = 90.0_f64.copysign(footpoint);
            return Ok(Geographic::new(self.lon0.to_degrees(), lat));
        }

        let a = self.ellipsoid.a;
        let e2 = self.ellipsoid.e2();
        let tan_footpoint = footpoint.tan();
        let one_minus = 1.0 - e2 * sin_footpoint * sin_footpoint;
        let prime_vertical = a / one_minus.sqrt();
        let meridian_radius = a * (1.0 - e2) / one_minus.powf(1.5);
        let t = tan_footpoint * tan_footpoint;
        let d = (coord.x - self.false_easting) / prime_vertical;
        let d2 = d * d;
        let d4 = d2 * d2;

        let lat = footpoint
            - (prime_vertical * tan_footpoint / meridian_radius)
                * (d2 / 2.0 - (1.0 + 3.0 * t) * d4 / 24.0);
        let lon = self.lon0
            + (d - t * d2 * d / 3.0 + (1.0 + 3.0 * t) * t * d4 * d / 15.0) / cos_footpoint;

        Ok(Geographic::new(lon.to_degrees(), lat.to_degrees()))
    }

    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The IOGP Guidance Note 7-2 worked example for method 9806: Trinidad 1903
    /// / Trinidad Grid, whose grid unit is the Clarke's link.
    const CLARKE_LINK_IN_METERS: f64 = 0.201166195164;

    fn trinidad_grid() -> CassiniSoldner {
        let clarke_1858 = Ellipsoid::new(
            6_378_293.645_208_759,
            1.0 - 6_356_617.987_679_838 / 6_378_293.645_208_759,
        );
        CassiniSoldner::new(
            clarke_1858,
            10.0 + 26.0 / 60.0 + 30.0 / 3600.0,
            -(61.0 + 20.0 / 60.0),
            430_000.0 * CLARKE_LINK_IN_METERS,
            325_000.0 * CLARKE_LINK_IN_METERS,
        )
    }

    #[test]
    fn test_guidance_note_worked_example() {
        let projected = trinidad_grid()
            .forward(Geographic::new(-62.0, 10.0))
            .unwrap();
        let easting_links = projected.x / CLARKE_LINK_IN_METERS;
        let northing_links = projected.y / CLARKE_LINK_IN_METERS;
        assert!((easting_links - 66_644.94).abs() < 0.01, "{easting_links}");
        assert!(
            (northing_links - 82_536.22).abs() < 0.01,
            "{northing_links}"
        );
    }

    #[test]
    fn test_guidance_note_worked_example_inverse() {
        let geo = trinidad_grid()
            .inverse(Coord::new(
                66_644.94 * CLARKE_LINK_IN_METERS,
                82_536.22 * CLARKE_LINK_IN_METERS,
            ))
            .unwrap();
        assert!((geo.lon - (-62.0)).abs() < 1e-7, "{}", geo.lon);
        assert!((geo.lat - 10.0).abs() < 1e-7, "{}", geo.lat);
    }

    #[test]
    fn test_roundtrip() {
        let projection = trinidad_grid();
        for lon in [-63.0, -61.5, -60.0] {
            for lat in [9.0, 10.5, 11.5] {
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
    fn test_natural_origin_maps_to_false_origin() {
        let projection = trinidad_grid();
        let projected = projection
            .forward(Geographic::new(
                -(61.0 + 20.0 / 60.0),
                10.0 + 26.0 / 60.0 + 30.0 / 3600.0,
            ))
            .unwrap();
        assert!((projected.x - 430_000.0 * CLARKE_LINK_IN_METERS).abs() < 1e-6);
        assert!((projected.y - 325_000.0 * CLARKE_LINK_IN_METERS).abs() < 1e-6);
    }
}
