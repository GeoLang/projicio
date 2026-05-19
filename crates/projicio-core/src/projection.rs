use crate::{Coord, Ellipsoid, Error, Geographic};

/// Trait for map projections (geographic → projected and inverse).
pub trait Projection {
    /// Project geographic coordinates (lon/lat in radians) to planar (x, y) in meters.
    fn forward(&self, geo: Geographic) -> Result<Coord, Error>;

    /// Inverse projection: planar (x, y) in meters to geographic (lon/lat in degrees).
    fn inverse(&self, coord: Coord) -> Result<Geographic, Error>;

    /// The ellipsoid used by this projection.
    fn ellipsoid(&self) -> &Ellipsoid;
}

/// Web Mercator (EPSG:3857) — the projection used by web mapping services.
#[derive(Debug, Clone)]
pub struct WebMercator {
    ellipsoid: Ellipsoid,
}

impl Default for WebMercator {
    fn default() -> Self {
        Self {
            ellipsoid: Ellipsoid::WGS84,
        }
    }
}

impl WebMercator {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Projection for WebMercator {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let lon_rad = geo.lon.to_radians();
        let lat_rad = geo.lat.to_radians();

        if lat_rad.abs() > std::f64::consts::FRAC_PI_2 * 0.9999 {
            return Err(Error::InvalidCoordinate(
                "latitude out of range for Web Mercator".into(),
            ));
        }

        let a = self.ellipsoid.a;
        let x = a * lon_rad;
        let y = a * ((std::f64::consts::FRAC_PI_4 + lat_rad / 2.0).tan()).ln();

        Ok(Coord::new(x, y))
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        let a = self.ellipsoid.a;
        let lon = (coord.x / a).to_degrees();
        let lat = (2.0 * (coord.y / a).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees();

        Ok(Geographic::new(lon, lat))
    }

    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
}

/// Transverse Mercator projection (basis for UTM).
#[derive(Debug, Clone)]
pub struct TransverseMercator {
    ellipsoid: Ellipsoid,
    /// Central meridian in degrees.
    pub lon0: f64,
    /// Scale factor at central meridian.
    pub k0: f64,
    /// False easting in meters.
    pub false_easting: f64,
    /// False northing in meters.
    pub false_northing: f64,
}

impl TransverseMercator {
    pub fn new(
        ellipsoid: Ellipsoid,
        lon0: f64,
        k0: f64,
        false_easting: f64,
        false_northing: f64,
    ) -> Self {
        Self {
            ellipsoid,
            lon0,
            k0,
            false_easting,
            false_northing,
        }
    }

    /// Create a UTM zone projection.
    pub fn utm(zone: u8, north: bool) -> Self {
        let lon0 = (zone as f64 - 1.0) * 6.0 - 180.0 + 3.0;
        Self::new(
            Ellipsoid::WGS84,
            lon0,
            0.9996,
            500_000.0,
            if north { 0.0 } else { 10_000_000.0 },
        )
    }
}

impl Projection for TransverseMercator {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let lat = geo.lat.to_radians();
        let lon = geo.lon.to_radians();
        let lon0 = self.lon0.to_radians();

        let e2 = self.ellipsoid.e2();
        let a = self.ellipsoid.a;
        let k0 = self.k0;

        let n = a / (1.0 - e2 * lat.sin().powi(2)).sqrt();
        let t = lat.tan();
        let c = e2 / (1.0 - e2) * lat.cos().powi(2);
        let a_coeff = (lon - lon0) * lat.cos();

        // Meridian arc length
        let e2_2 = e2 * e2;
        let e2_3 = e2_2 * e2;
        let m = a
            * ((1.0 - e2 / 4.0 - 3.0 * e2_2 / 64.0 - 5.0 * e2_3 / 256.0) * lat
                - (3.0 * e2 / 8.0 + 3.0 * e2_2 / 32.0 + 45.0 * e2_3 / 1024.0) * (2.0 * lat).sin()
                + (15.0 * e2_2 / 256.0 + 45.0 * e2_3 / 1024.0) * (4.0 * lat).sin()
                - (35.0 * e2_3 / 3072.0) * (6.0 * lat).sin());

        let x = k0
            * n
            * (a_coeff
                + (1.0 - t * t + c) * a_coeff.powi(3) / 6.0
                + (5.0 - 18.0 * t * t + t.powi(4) + 72.0 * c - 58.0 * e2 / (1.0 - e2))
                    * a_coeff.powi(5)
                    / 120.0)
            + self.false_easting;

        let y = k0
            * (m + n
                * lat.tan()
                * (a_coeff.powi(2) / 2.0
                    + (5.0 - t * t + 9.0 * c + 4.0 * c * c) * a_coeff.powi(4) / 24.0
                    + (61.0 - 58.0 * t * t + t.powi(4) + 600.0 * c - 330.0 * e2 / (1.0 - e2))
                        * a_coeff.powi(6)
                        / 720.0))
            + self.false_northing;

        Ok(Coord::new(x, y))
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        let a = self.ellipsoid.a;
        let e2 = self.ellipsoid.e2();
        let k0 = self.k0;
        let lon0 = self.lon0.to_radians();

        let e1 = (1.0 - (1.0 - e2).sqrt()) / (1.0 + (1.0 - e2).sqrt());
        let m = (coord.y - self.false_northing) / k0;

        let mu = m / (a * (1.0 - e2 / 4.0 - 3.0 * e2 * e2 / 64.0 - 5.0 * e2.powi(3) / 256.0));

        let phi1 = mu
            + (3.0 * e1 / 2.0 - 27.0 * e1.powi(3) / 32.0) * (2.0 * mu).sin()
            + (21.0 * e1 * e1 / 16.0 - 55.0 * e1.powi(4) / 32.0) * (4.0 * mu).sin()
            + (151.0 * e1.powi(3) / 96.0) * (6.0 * mu).sin();

        let n1 = a / (1.0 - e2 * phi1.sin().powi(2)).sqrt();
        let t1 = phi1.tan();
        let c1 = e2 / (1.0 - e2) * phi1.cos().powi(2);
        let r1 = a * (1.0 - e2) / (1.0 - e2 * phi1.sin().powi(2)).powf(1.5);
        let d = (coord.x - self.false_easting) / (n1 * k0);

        let lat = phi1
            - (n1 * t1 / r1)
                * (d * d / 2.0
                    - (5.0 + 3.0 * t1 * t1 + 10.0 * c1 - 4.0 * c1 * c1 - 9.0 * e2 / (1.0 - e2))
                        * d.powi(4)
                        / 24.0
                    + (61.0 + 90.0 * t1 * t1 + 298.0 * c1 + 45.0 * t1.powi(4)
                        - 252.0 * e2 / (1.0 - e2)
                        - 3.0 * c1 * c1)
                        * d.powi(6)
                        / 720.0);

        let lon = lon0
            + (d - (1.0 + 2.0 * t1 * t1 + c1) * d.powi(3) / 6.0
                + (5.0 - 2.0 * c1 + 28.0 * t1 * t1 - 3.0 * c1 * c1
                    + 8.0 * e2 / (1.0 - e2)
                    + 24.0 * t1.powi(4))
                    * d.powi(5)
                    / 120.0)
                / phi1.cos();

        Ok(Geographic::new(lon.to_degrees(), lat.to_degrees()))
    }

    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
}

/// Mercator projection (EPSG:3395).
#[derive(Debug, Clone)]
pub struct Mercator {
    ellipsoid: Ellipsoid,
    pub lon0: f64,
    pub k0: f64,
}

impl Mercator {
    pub fn new(ellipsoid: Ellipsoid, lon0: f64, k0: f64) -> Self {
        Self {
            ellipsoid,
            lon0,
            k0,
        }
    }
}

impl Projection for Mercator {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let lon_rad = geo.lon.to_radians();
        let lat_rad = geo.lat.to_radians();
        let lon0_rad = self.lon0.to_radians();
        let a = self.ellipsoid.a;
        let e = self.ellipsoid.e();

        let x = a * self.k0 * (lon_rad - lon0_rad);
        let e_sin_lat = e * lat_rad.sin();
        let y = a
            * self.k0
            * ((std::f64::consts::FRAC_PI_4 + lat_rad / 2.0).tan()
                * ((1.0 - e_sin_lat) / (1.0 + e_sin_lat)).powf(e / 2.0))
            .ln();

        Ok(Coord::new(x, y))
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        let a = self.ellipsoid.a;
        let e = self.ellipsoid.e();
        let lon0_rad = self.lon0.to_radians();

        let lon = coord.x / (a * self.k0) + lon0_rad;
        let t = (-coord.y / (a * self.k0)).exp();

        // Iterative solution for latitude
        let mut lat = std::f64::consts::FRAC_PI_2 - 2.0 * t.atan();
        for _ in 0..10 {
            let e_sin_lat = e * lat.sin();
            let new_lat = std::f64::consts::FRAC_PI_2
                - 2.0 * (t * ((1.0 - e_sin_lat) / (1.0 + e_sin_lat)).powf(e / 2.0)).atan();
            if (new_lat - lat).abs() < 1e-12 {
                break;
            }
            lat = new_lat;
        }

        Ok(Geographic::new(lon.to_degrees(), lat.to_degrees()))
    }

    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
}

/// Lambert Conformal Conic projection (1SP and 2SP).
#[derive(Debug, Clone)]
pub struct LambertConformalConic {
    ellipsoid: Ellipsoid,
    n: f64,
    f_coeff: f64,
    rho0: f64,
    lon0: f64,
    false_easting: f64,
    false_northing: f64,
}

impl LambertConformalConic {
    /// Create a 2SP Lambert Conformal Conic projection.
    pub fn new_2sp(
        ellipsoid: Ellipsoid,
        lat1: f64,
        lat2: f64,
        lat0: f64,
        lon0: f64,
        false_easting: f64,
        false_northing: f64,
    ) -> Self {
        let phi1 = lat1.to_radians();
        let phi2 = lat2.to_radians();
        let phi0 = lat0.to_radians();
        let e = ellipsoid.e();
        let a = ellipsoid.a;

        let m = |phi: f64| -> f64 { phi.cos() / (1.0 - e * e * phi.sin().powi(2)).sqrt() };

        let t = |phi: f64| -> f64 {
            let e_sin = e * phi.sin();
            (std::f64::consts::FRAC_PI_4 - phi / 2.0).tan()
                / ((1.0 - e_sin) / (1.0 + e_sin)).powf(e / 2.0)
        };

        let m1 = m(phi1);
        let m2 = m(phi2);
        let t0 = t(phi0);
        let t1 = t(phi1);
        let t2 = t(phi2);

        let n = (m1.ln() - m2.ln()) / (t1.ln() - t2.ln());
        let f_coeff = m1 / (n * t1.powf(n));
        let rho0 = a * f_coeff * t0.powf(n);

        Self {
            ellipsoid,
            n,
            f_coeff,
            rho0,
            lon0,
            false_easting,
            false_northing,
        }
    }
}

impl Projection for LambertConformalConic {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let lat = geo.lat.to_radians();
        let lon = geo.lon.to_radians();
        let lon0 = self.lon0.to_radians();
        let e = self.ellipsoid.e();
        let a = self.ellipsoid.a;

        let e_sin = e * lat.sin();
        let t = (std::f64::consts::FRAC_PI_4 - lat / 2.0).tan()
            / ((1.0 - e_sin) / (1.0 + e_sin)).powf(e / 2.0);

        let rho = a * self.f_coeff * t.powf(self.n);
        let theta = self.n * (lon - lon0);

        let x = self.false_easting + rho * theta.sin();
        let y = self.false_northing + self.rho0 - rho * theta.cos();

        Ok(Coord::new(x, y))
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        let a = self.ellipsoid.a;
        let e = self.ellipsoid.e();
        let lon0 = self.lon0.to_radians();

        let x = coord.x - self.false_easting;
        let y = self.rho0 - (coord.y - self.false_northing);

        let rho = self.n.signum() * (x * x + y * y).sqrt();
        let theta = y.atan2(x);
        let t = (rho / (a * self.f_coeff)).powf(1.0 / self.n);

        let lon = theta / self.n + lon0;

        // Iterative latitude from t
        let mut lat = std::f64::consts::FRAC_PI_2 - 2.0 * t.atan();
        for _ in 0..10 {
            let e_sin = e * lat.sin();
            let new_lat = std::f64::consts::FRAC_PI_2
                - 2.0 * (t * ((1.0 - e_sin) / (1.0 + e_sin)).powf(e / 2.0)).atan();
            if (new_lat - lat).abs() < 1e-12 {
                break;
            }
            lat = new_lat;
        }

        Ok(Geographic::new(lon.to_degrees(), lat.to_degrees()))
    }

    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
}

/// Albers Equal Area Conic projection.
///
/// Commonly used for continental-scale maps (e.g., CONUS, Europe).
#[derive(Debug, Clone)]
pub struct AlbersEqualArea {
    ellipsoid: Ellipsoid,
    n: f64,
    c: f64,
    rho0: f64,
    lon0: f64,
}

impl AlbersEqualArea {
    /// Create Albers Equal Area Conic with two standard parallels.
    ///
    /// * `lat1`, `lat2` — standard parallels in degrees
    /// * `lat0` — latitude of origin in degrees
    /// * `lon0` — central meridian in degrees
    pub fn new(ellipsoid: Ellipsoid, lat1: f64, lat2: f64, lat0: f64, lon0: f64) -> Self {
        let phi1 = lat1.to_radians();
        let phi2 = lat2.to_radians();
        let phi0 = lat0.to_radians();
        let e = ellipsoid.e();

        let m1 = Self::m_func(phi1, e);
        let m2 = Self::m_func(phi2, e);
        let q0 = Self::q_func(phi0, e);
        let q1 = Self::q_func(phi1, e);
        let q2 = Self::q_func(phi2, e);

        let n = (m1 * m1 - m2 * m2) / (q2 - q1);
        let c = m1 * m1 + n * q1;
        let rho0 = ellipsoid.a * (c - n * q0).sqrt() / n;

        Self {
            ellipsoid,
            n,
            c,
            rho0,
            lon0,
        }
    }

    fn m_func(phi: f64, e: f64) -> f64 {
        phi.cos() / (1.0 - e * e * phi.sin().powi(2)).sqrt()
    }

    fn q_func(phi: f64, e: f64) -> f64 {
        let sin_phi = phi.sin();
        let e_sin = e * sin_phi;
        (1.0 - e * e)
            * (sin_phi / (1.0 - e_sin * e_sin)
                - (1.0 / (2.0 * e)) * ((1.0 - e_sin) / (1.0 + e_sin)).ln())
    }
}

impl Projection for AlbersEqualArea {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let phi = geo.lat.to_radians();
        let lam = geo.lon.to_radians();
        let lam0 = self.lon0.to_radians();
        let e = self.ellipsoid.e();
        let a = self.ellipsoid.a;

        let q = Self::q_func(phi, e);
        let rho = a * (self.c - self.n * q).sqrt() / self.n;
        let theta = self.n * (lam - lam0);

        let x = rho * theta.sin();
        let y = self.rho0 - rho * theta.cos();
        Ok(Coord::new(x, y))
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        let a = self.ellipsoid.a;
        let e = self.ellipsoid.e();
        let lam0 = self.lon0.to_radians();

        let rho = (coord.x.powi(2) + (self.rho0 - coord.y).powi(2)).sqrt() * self.n.signum();
        let theta = (coord.x / (self.rho0 - coord.y)).atan();
        let q = (self.c - (rho * self.n / a).powi(2)) / self.n;

        // Iterative inverse for latitude
        let mut phi = (q / 2.0).asin();
        for _ in 0..20 {
            let sin_phi = phi.sin();
            let e_sin = e * sin_phi;
            let one_minus = 1.0 - e_sin * e_sin;
            let dphi = one_minus * one_minus / (2.0 * phi.cos())
                * (q / (1.0 - e * e) - sin_phi / one_minus
                    + (1.0 / (2.0 * e)) * ((1.0 - e_sin) / (1.0 + e_sin)).ln());
            phi += dphi;
            if dphi.abs() < 1e-12 {
                break;
            }
        }

        let lon = (theta / self.n + lam0).to_degrees();
        let lat = phi.to_degrees();
        Ok(Geographic::new(lon, lat))
    }

    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
}

/// Polar Stereographic projection (UPS and polar regions).
#[derive(Debug, Clone)]
pub struct PolarStereographic {
    ellipsoid: Ellipsoid,
    north: bool,
    k0: f64,
}

impl PolarStereographic {
    pub fn north(ellipsoid: Ellipsoid) -> Self {
        Self {
            ellipsoid,
            north: true,
            k0: 0.994,
        }
    }

    pub fn south(ellipsoid: Ellipsoid) -> Self {
        Self {
            ellipsoid,
            north: false,
            k0: 0.994,
        }
    }
}

impl Projection for PolarStereographic {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let a = self.ellipsoid.a;
        let e = self.ellipsoid.e();
        let lon_rad = geo.lon.to_radians();
        let lat_rad = if self.north {
            geo.lat.to_radians()
        } else {
            -geo.lat.to_radians()
        };
        let lam = if self.north { lon_rad } else { -lon_rad };

        let sin_lat = lat_rad.sin();
        let e_sin = e * sin_lat;
        let t = ((std::f64::consts::FRAC_PI_4 - lat_rad / 2.0).tan())
            / ((1.0 - e_sin) / (1.0 + e_sin)).powf(e / 2.0);

        let rho = 2.0 * a * self.k0 * t
            / ((1.0 + e).powf((1.0 + e) / 2.0) * (1.0 - e).powf((1.0 - e) / 2.0)).sqrt();

        let x = rho * lam.sin();
        let y = -rho * lam.cos();

        if self.north {
            Ok(Coord::new(x, y))
        } else {
            Ok(Coord::new(x, -y))
        }
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        let a = self.ellipsoid.a;
        let e = self.ellipsoid.e();

        let (x, y) = if self.north {
            (coord.x, coord.y)
        } else {
            (coord.x, -coord.y)
        };

        let rho = (x * x + y * y).sqrt();
        let t = rho * ((1.0 + e).powf((1.0 + e) / 2.0) * (1.0 - e).powf((1.0 - e) / 2.0)).sqrt()
            / (2.0 * a * self.k0);

        // Iterative latitude from t
        let mut phi = std::f64::consts::FRAC_PI_2 - 2.0 * t.atan();
        for _ in 0..20 {
            let e_sin = e * phi.sin();
            let new_phi = std::f64::consts::FRAC_PI_2
                - 2.0 * (t * ((1.0 - e_sin) / (1.0 + e_sin)).powf(e / 2.0)).atan();
            if (new_phi - phi).abs() < 1e-12 {
                break;
            }
            phi = new_phi;
        }

        let lam = x.atan2(-y);

        let (lon, lat) = if self.north {
            (lam.to_degrees(), phi.to_degrees())
        } else {
            ((-lam).to_degrees(), (-phi).to_degrees())
        };

        Ok(Geographic::new(lon, lat))
    }

    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_mercator_roundtrip() {
        let proj = WebMercator::new();
        let geo = Geographic::new(-74.006, 40.7128);
        let projected = proj.forward(geo).unwrap();
        let recovered = proj.inverse(projected).unwrap();
        assert!((recovered.lon - geo.lon).abs() < 1e-10);
        assert!((recovered.lat - geo.lat).abs() < 1e-10);
    }

    #[test]
    fn test_utm_zone_18n_roundtrip() {
        let proj = TransverseMercator::utm(18, true);
        let geo = Geographic::new(-74.006, 40.7128);
        let projected = proj.forward(geo).unwrap();
        let recovered = proj.inverse(projected).unwrap();
        assert!((recovered.lon - geo.lon).abs() < 1e-6);
        assert!((recovered.lat - geo.lat).abs() < 1e-6);
    }

    #[test]
    fn test_web_mercator_known_values() {
        let proj = WebMercator::new();
        // (0, 0) should project to (0, 0)
        let result = proj.forward(Geographic::new(0.0, 0.0)).unwrap();
        assert!((result.x).abs() < 1e-6);
        assert!((result.y).abs() < 1e-6);
    }

    #[test]
    fn test_albers_roundtrip() {
        let proj = AlbersEqualArea::new(Ellipsoid::WGS84, 29.5, 45.5, 23.0, -96.0);
        let geo = Geographic::new(-90.0, 35.0);
        let projected = proj.forward(geo).unwrap();
        let recovered = proj.inverse(projected).unwrap();
        assert!((recovered.lon - geo.lon).abs() < 1e-5);
        assert!((recovered.lat - geo.lat).abs() < 1e-5);
    }

    #[test]
    fn test_polar_stereographic_roundtrip() {
        let proj = PolarStereographic::north(Ellipsoid::WGS84);
        let geo = Geographic::new(10.0, 80.0);
        let projected = proj.forward(geo).unwrap();
        let recovered = proj.inverse(projected).unwrap();
        assert!((recovered.lon - geo.lon).abs() < 1e-6);
        assert!((recovered.lat - geo.lat).abs() < 1e-6);
    }
}
