// adapted from PROJ src/projections/krovak.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use super::{ClassicProjection, LambdaPhi};
use crate::{Coord, Error};

pub(super) const BESSEL_SEMI_MAJOR_AXIS: f64 = 6377397.155;
const BESSEL_ECCENTRICITY_SQUARED: f64 = 0.006674372230614;

const EPS: f64 = 1e-15;
// DU(2, 59, 42, 42.69689)
const UQ: f64 = 1.04216856380474;
// latitude of pseudo standard parallel 78deg 30'00" N
const S0: f64 = 1.37008346281555;
const MAX_ITER: usize = 100;

const X0: f64 = 1089000.0;
const Y0: f64 = 654000.0;
const C1: f64 = 2.946529277E-02;
const C2: f64 = 2.515965696E-02;
const C3: f64 = 1.193845912E-07;
const C4: f64 = -4.668270147E-07;
const C5: f64 = 9.233980362E-12;
const C6: f64 = 1.523735715E-12;
const C7: f64 = 1.696780024E-18;
const C8: f64 = 4.408314235E-18;
const C9: f64 = -8.331083518E-24;
const C10: f64 = -3.689471323E-24;

struct Correction {
    dx: f64,
    dy: f64,
}

fn mod_krovak_compute_dx_dy(xr: f64, yr: f64) -> Correction {
    let xr2 = xr * xr;
    let yr2 = yr * yr;
    let xr4 = xr2 * xr2;
    let yr4 = yr2 * yr2;

    let dx = C1 + C3 * xr - C4 * yr - 2.0 * C6 * xr * yr
        + C5 * (xr2 - yr2)
        + C7 * xr * (xr2 - 3.0 * yr2)
        - C8 * yr * (3.0 * xr2 - yr2)
        + 4.0 * C9 * xr * yr * (xr2 - yr2)
        + C10 * (xr4 + yr4 - 6.0 * xr2 * yr2);
    let dy = C2
        + C3 * yr
        + C4 * xr
        + 2.0 * C5 * xr * yr
        + C6 * (xr2 - yr2)
        + C8 * xr * (xr2 - 3.0 * yr2)
        + C7 * yr * (3.0 * xr2 - yr2)
        - 4.0 * C10 * xr * yr * (xr2 - yr2)
        + C9 * (xr4 + yr4 - 6.0 * xr2 * yr2);
    Correction { dx, dy }
}

pub(super) struct Krovak {
    e: f64,
    lam0: f64,
    x0: f64,
    y0: f64,
    alpha: f64,
    k: f64,
    n: f64,
    rho0: f64,
    ad: f64,
    modified: bool,
}

impl Krovak {
    pub(super) fn standard(phi0: f64, lam0: f64, k0: f64, x0: f64, y0: f64) -> Result<Self, Error> {
        Self::setup(phi0, lam0, k0, x0, y0, false)
    }

    pub(super) fn modified(phi0: f64, lam0: f64, k0: f64, x0: f64, y0: f64) -> Result<Self, Error> {
        Self::setup(phi0, lam0, k0, x0, y0, true)
    }

    fn setup(
        phi0: f64,
        lam0: f64,
        k0: f64,
        x0: f64,
        y0: f64,
        modified: bool,
    ) -> Result<Self, Error> {
        let es = BESSEL_ECCENTRICITY_SQUARED;
        let e = es.sqrt();

        let alpha = (1.0 + (es * phi0.cos().powf(4.0)) / (1.0 - es)).sqrt();
        let u0 = (phi0.sin() / alpha).asin();
        let g = ((1.0 + e * phi0.sin()) / (1.0 - e * phi0.sin())).powf(alpha * e / 2.0);
        let tan_half_phi0_plus_pi_4 = (phi0 / 2.0 + FRAC_PI_4).tan();
        if tan_half_phi0_plus_pi_4 == 0.0 {
            return Err(Error::ProjectionError(
                "invalid value for lat_0: lat_0 + PI/4 should be different from 0".into(),
            ));
        }
        let k = (u0 / 2.0 + FRAC_PI_4).tan() / tan_half_phi0_plus_pi_4.powf(alpha) * g;
        let n0 = (1.0 - es).sqrt() / (1.0 - es * phi0.sin().powf(2.0));
        let n = S0.sin();
        let rho0 = k0 * n0 / S0.tan();
        let ad = FRAC_PI_2 - UQ;

        Ok(Self {
            e,
            lam0,
            x0,
            y0,
            alpha,
            k,
            n,
            rho0,
            ad,
            modified,
        })
    }
}

impl ClassicProjection for Krovak {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let a = BESSEL_SEMI_MAJOR_AXIS;
        let e = self.e;
        let gfi = ((1.0 + e * lp.phi.sin()) / (1.0 - e * lp.phi.sin())).powf(self.alpha * e / 2.0);

        let u = 2.0
            * ((self.k * (lp.phi / 2.0 + FRAC_PI_4).tan().powf(self.alpha) / gfi).atan()
                - FRAC_PI_4);
        let deltav = -lp.lambda * self.alpha;

        let s = (self.ad.cos() * u.sin() + self.ad.sin() * u.cos() * deltav.cos()).asin();
        let cos_s = s.cos();
        if cos_s < 1e-12 {
            return Ok(Coord::new(0.0, 0.0));
        }
        let d = (u.cos() * deltav.sin() / cos_s).asin();

        let eps = self.n * d;
        let rho = self.rho0 * (S0 / 2.0 + FRAC_PI_4).tan().powf(self.n)
            / (s / 2.0 + FRAC_PI_4).tan().powf(self.n);

        // x is a southing and y a westing here
        let mut x = rho * eps.cos();
        let mut y = rho * eps.sin();

        if self.modified {
            let xp = x;
            let yp = y;
            let xr = xp * a - X0;
            let yr = yp * a - Y0;
            let correction = mod_krovak_compute_dx_dy(xr, yr);
            x = xp - correction.dx / a;
            y = yp - correction.dy / a;
        }

        std::mem::swap(&mut x, &mut y);

        // krovak.cpp folds the false origin into the sign flip to easting and northing
        x = -x - 2.0 * self.x0 / a;
        y = -y - 2.0 * self.y0 / a;

        Ok(Coord::new(x, y))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let a = BESSEL_SEMI_MAJOR_AXIS;
        let e = self.e;
        // krovak.cpp pairs y with x0 and x with y0 here, which only inverts the forward when x0 equals y0
        let mut y = -xy.y - 2.0 * self.x0 / a;
        let mut x = -xy.x - 2.0 * self.y0 / a;

        std::mem::swap(&mut x, &mut y);

        if self.modified {
            let xr = x * a - X0;
            let yr = y * a - Y0;
            let correction = mod_krovak_compute_dx_dy(xr, yr);
            x += correction.dx / a;
            y += correction.dy / a;
        }

        let rho = (x * x + y * y).sqrt();
        let eps = y.atan2(x);

        let d = eps / S0.sin();
        let s = if rho == 0.0 {
            FRAC_PI_2
        } else {
            2.0 * (((self.rho0 / rho).powf(1.0 / self.n) * (S0 / 2.0 + FRAC_PI_4).tan()).atan()
                - FRAC_PI_4)
        };

        let u = (self.ad.cos() * s.sin() - self.ad.sin() * s.cos() * d.cos()).asin();
        let deltav = (s.cos() * d.sin() / u.cos()).asin();

        let lambda = self.lam0 - deltav / self.alpha;

        let mut fi1 = u;
        for _ in 0..MAX_ITER {
            let phi = 2.0
                * ((self.k.powf(-1.0 / self.alpha)
                    * (u / 2.0 + FRAC_PI_4).tan().powf(1.0 / self.alpha)
                    * ((1.0 + e * fi1.sin()) / (1.0 - e * fi1.sin())).powf(e / 2.0))
                .atan()
                    - FRAC_PI_4);
            if (fi1 - phi).abs() < EPS {
                return Ok(LambdaPhi {
                    lambda: lambda - self.lam0,
                    phi,
                });
            }
            fi1 = phi;
        }
        Err(Error::ProjectionError(
            "Krovak inverse latitude did not converge".into(),
        ))
    }
}
