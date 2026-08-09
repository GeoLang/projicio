// ported from proj-rust (https://github.com/pka/proj-rust), MIT OR Apache-2.0

use crate::projection::Projection;
use crate::{Coord, Ellipsoid, Error, Geographic};

use super::series::{latitude_from_conformal_t, normalize_longitude};

const POLE_EPSILON: f64 = 1e-12;
const ECCENTRICITY_EPSILON: f64 = 1e-15;

/// Hotine Oblique Mercator, also called the Rectified Skew Orthomorphic
/// (EPSG methods 9812 and 9815).
#[derive(Debug, Clone)]
pub struct HotineObliqueMercator {
    ellipsoid: Ellipsoid,
    a_scaled: f64,
    b: f64,
    h: f64,
    gamma0: f64,
    rectified_grid_angle: f64,
    lon0: f64,
    /// The along-line offset of the projection centre, zero for the variant
    /// whose false easting and northing sit at the natural origin.
    centre_offset: f64,
    false_easting: f64,
    false_northing: f64,
}

impl HotineObliqueMercator {
    /// * `lat_c`, `lon_c` — latitude and longitude of projection centre in degrees
    /// * `azimuth` — azimuth of the central line at the centre, in degrees
    /// * `rectified_grid_angle` — angle from north to the grid axis, in degrees
    /// * `k0` — scale factor on the central line
    /// * `offsets_at_centre` — true for variant B, where the false easting and
    ///   northing are at the projection centre rather than the natural origin
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ellipsoid: Ellipsoid,
        lat_c: f64,
        lon_c: f64,
        azimuth: f64,
        rectified_grid_angle: f64,
        k0: f64,
        false_easting: f64,
        false_northing: f64,
        offsets_at_centre: bool,
    ) -> Result<Self, Error> {
        let latc = lat_c.to_radians();
        let lonc = lon_c.to_radians();
        let azimuth = azimuth.to_radians();

        if (latc.abs() - std::f64::consts::FRAC_PI_2).abs() < POLE_EPSILON {
            return Err(Error::UnsupportedCrs(
                "hotine oblique mercator centre cannot be at a pole".into(),
            ));
        }

        let e2 = ellipsoid.e2();
        let e = ellipsoid.e();
        let sin_latc = latc.sin();
        let cos_latc = latc.cos();
        let b = (1.0 + e2 * cos_latc.powi(4) / (1.0 - e2)).sqrt();
        let a_scaled = ellipsoid.a * b * k0 * (1.0 - e2).sqrt() / (1.0 - e2 * sin_latc * sin_latc);
        let t0 = conformal_t(latc, e);
        let d = b * (1.0 - e2).sqrt() / (cos_latc * (1.0 - e2 * sin_latc * sin_latc).sqrt());
        let d_squared = (d * d).max(1.0);
        let f = d + (d_squared - 1.0).sqrt() * latc.signum();
        if !f.is_finite() || f <= 0.0 {
            return Err(Error::UnsupportedCrs(
                "hotine oblique mercator origin constants are invalid".into(),
            ));
        }
        let h = f * t0.powf(b);
        let g = (f - 1.0 / f) / 2.0;
        let gamma0 = (azimuth.sin() / d).clamp(-1.0, 1.0).asin();
        let lon0 = lonc - (g * gamma0.tan()).clamp(-1.0, 1.0).asin() / b;

        let centre_offset = if offsets_at_centre {
            (a_scaled / b) * (d_squared - 1.0).sqrt().atan2(azimuth.cos()) * latc.signum()
        } else {
            0.0
        };

        Ok(Self {
            ellipsoid,
            a_scaled,
            b,
            h,
            gamma0,
            rectified_grid_angle: rectified_grid_angle.to_radians(),
            lon0,
            centre_offset,
            false_easting,
            false_northing,
        })
    }
}

fn conformal_t(lat: f64, e: f64) -> f64 {
    let tan_half = (std::f64::consts::FRAC_PI_4 - lat / 2.0).tan();
    if e.abs() < ECCENTRICITY_EPSILON {
        return tan_half;
    }
    let e_sin = e * lat.sin();
    tan_half / ((1.0 - e_sin) / (1.0 + e_sin)).powf(e / 2.0)
}

impl Projection for HotineObliqueMercator {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let lat = geo.lat.to_radians();
        let delta_lon = normalize_longitude(geo.lon.to_radians() - self.lon0);
        let b_delta_lon = self.b * delta_lon;

        let q = self.h / conformal_t(lat, self.ellipsoid.e()).powf(self.b);
        let s = (q - 1.0 / q) / 2.0;
        let t = (q + 1.0 / q) / 2.0;
        let v = b_delta_lon.sin();
        let ratio = (-v * self.gamma0.cos() + s * self.gamma0.sin()) / t;
        if ratio.abs() >= 1.0 {
            return Err(Error::InvalidCoordinate(
                "hotine oblique mercator is undefined for this coordinate".into(),
            ));
        }

        let skew_v = self.a_scaled * ((1.0 - ratio) / (1.0 + ratio)).ln() / (2.0 * self.b);
        let skew_u = self.a_scaled
            * (s * self.gamma0.cos() + v * self.gamma0.sin()).atan2(b_delta_lon.cos())
            / self.b
            - self.centre_offset;

        let gamma = self.rectified_grid_angle;
        Ok(Coord::new(
            self.false_easting + skew_v * gamma.cos() + skew_u * gamma.sin(),
            self.false_northing + skew_u * gamma.cos() - skew_v * gamma.sin(),
        ))
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        let dx = coord.x - self.false_easting;
        let dy = coord.y - self.false_northing;
        let gamma = self.rectified_grid_angle;
        let skew_v = dx * gamma.cos() - dy * gamma.sin();
        let skew_u = dy * gamma.cos() + dx * gamma.sin() + self.centre_offset;

        let q = (-self.b * skew_v / self.a_scaled).exp();
        let s = (q - 1.0 / q) / 2.0;
        let t = (q + 1.0 / q) / 2.0;
        let v = (self.b * skew_u / self.a_scaled).sin();
        let ratio = (v * self.gamma0.cos() + s * self.gamma0.sin()) / t;
        if ratio.abs() >= 1.0 {
            return Err(Error::InvalidCoordinate(
                "hotine oblique mercator inverse is undefined for this coordinate".into(),
            ));
        }

        let conformal = (self.h / ((1.0 + ratio) / (1.0 - ratio)).sqrt()).powf(1.0 / self.b);
        let lat = latitude_from_conformal_t(
            "hotine oblique mercator inverse latitude",
            conformal,
            self.ellipsoid.e(),
        )?;
        let lon = self.lon0
            - (s * self.gamma0.cos() - v * self.gamma0.sin())
                .atan2((self.b * skew_u / self.a_scaled).cos())
                / self.b;

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

    fn degrees(deg: f64, min: f64, sec: f64) -> f64 {
        deg + min / 60.0 + sec / 3600.0
    }

    fn everest_sabah_sarawak() -> Ellipsoid {
        Ellipsoid::new(6_377_298.556, 1.0 / 300.8017)
    }

    /// The IOGP Guidance Note 7-2 worked example shared by methods 9812 and
    /// 9815: Timbalai 1948 / RSO Borneo, EPSG:29873.
    fn borneo(offsets_at_centre: bool) -> HotineObliqueMercator {
        let (false_easting, false_northing) = if offsets_at_centre {
            (590_476.87, 442_857.65)
        } else {
            (0.0, 0.0)
        };
        HotineObliqueMercator::new(
            everest_sabah_sarawak(),
            4.0,
            115.0,
            degrees(53.0, 18.0, 56.9537),
            degrees(53.0, 7.0, 48.3685),
            0.99984,
            false_easting,
            false_northing,
            offsets_at_centre,
        )
        .unwrap()
    }

    fn borneo_test_point() -> Geographic {
        Geographic::new(degrees(115.0, 48.0, 19.8196), degrees(5.0, 23.0, 14.1129))
    }

    #[test]
    fn test_guidance_note_variant_b_worked_example() {
        let projected = borneo(true).forward(borneo_test_point()).unwrap();
        assert!((projected.x - 679_245.73).abs() < 0.02, "{}", projected.x);
        assert!((projected.y - 596_562.78).abs() < 0.02, "{}", projected.y);
    }

    /// Variant A measures from the natural origin with no false origin at all.
    /// Borneo's variant B false origin is exactly the centre offset rotated
    /// into grid axes, which is why both variants land on the same coordinate.
    #[test]
    fn test_guidance_note_variant_a_worked_example() {
        let projected = borneo(false).forward(borneo_test_point()).unwrap();
        assert!((projected.x - 679_245.73).abs() < 0.02, "{}", projected.x);
        assert!((projected.y - 596_562.78).abs() < 0.02, "{}", projected.y);
    }

    #[test]
    fn test_guidance_note_worked_example_inverse() {
        let geo = borneo(true)
            .inverse(Coord::new(679_245.73, 596_562.78))
            .unwrap();
        let expected = borneo_test_point();
        assert!((geo.lon - expected.lon).abs() < 1e-7, "{}", geo.lon);
        assert!((geo.lat - expected.lat).abs() < 1e-7, "{}", geo.lat);
    }

    #[test]
    fn test_centre_maps_to_false_origin() {
        let projected = borneo(true).forward(Geographic::new(115.0, 4.0)).unwrap();
        assert!((projected.x - 590_476.87).abs() < 1e-6, "{}", projected.x);
        assert!((projected.y - 442_857.65).abs() < 1e-6, "{}", projected.y);
    }

    #[test]
    fn test_roundtrip_both_variants() {
        for offsets_at_centre in [true, false] {
            let projection = borneo(offsets_at_centre);
            for lon in [113.0, 115.0, 118.0] {
                for lat in [1.0, 4.0, 7.0] {
                    let geo = Geographic::new(lon, lat);
                    let recovered = projection
                        .inverse(projection.forward(geo).unwrap())
                        .unwrap();
                    assert!((recovered.lon - lon).abs() < 1e-9, "{}", recovered.lon);
                    assert!((recovered.lat - lat).abs() < 1e-9, "{}", recovered.lat);
                }
            }
        }
    }

    #[test]
    fn test_southern_hemisphere_centre_roundtrip() {
        let projection = HotineObliqueMercator::new(
            Ellipsoid::GRS80,
            -18.9,
            44.1,
            18.9,
            18.9,
            0.9995,
            400_000.0,
            800_000.0,
            true,
        )
        .unwrap();
        let geo = Geographic::new(47.0, -20.0);
        let recovered = projection
            .inverse(projection.forward(geo).unwrap())
            .unwrap();
        assert!((recovered.lon - geo.lon).abs() < 1e-9);
        assert!((recovered.lat - geo.lat).abs() < 1e-9);
    }

    #[test]
    fn test_rejects_polar_centre() {
        assert!(
            HotineObliqueMercator::new(
                Ellipsoid::GRS80,
                90.0,
                0.0,
                45.0,
                45.0,
                1.0,
                0.0,
                0.0,
                true
            )
            .is_err()
        );
    }
}
