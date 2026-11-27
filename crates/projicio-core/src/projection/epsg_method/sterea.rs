// adapted from PROJ src/projections/sterea.cpp, src/gauss.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const MAX_ITER: usize = 20;
const DEL_TOL: f64 = 1e-14;

fn srat(esinp: f64, ratexp: f64) -> f64 {
    ((1.0 - esinp) / (1.0 + esinp)).powf(ratexp)
}

struct Gauss {
    c: f64,
    k: f64,
    e: f64,
    ratexp: f64,
}

struct GaussSetup {
    gauss: Gauss,
    chi: f64,
    rc: f64,
}

fn gauss_ini(e: f64, phi0: f64) -> Result<GaussSetup, Error> {
    let es = e * e;
    let sphi = phi0.sin();
    let mut cphi = phi0.cos();
    cphi *= cphi;
    let rc = (1.0 - es).sqrt() / (1.0 - es * sphi * sphi);
    let c = (1.0 + es * cphi * cphi / (1.0 - es)).sqrt();
    if c == 0.0 {
        return Err(Error::ProjectionError(
            "oblique stereographic setup failed".into(),
        ));
    }
    let chi = (sphi / c).asin();
    let ratexp = 0.5 * c * e;
    let srat_val = srat(e * sphi, ratexp);
    if srat_val == 0.0 {
        return Err(Error::ProjectionError(
            "oblique stereographic setup failed".into(),
        ));
    }
    let k = if 0.5 * phi0 + FRAC_PI_4 < 1e-10 {
        1.0 / srat_val
    } else {
        (0.5 * chi + FRAC_PI_4).tan() / ((0.5 * phi0 + FRAC_PI_4).tan().powf(c) * srat_val)
    };
    Ok(GaussSetup {
        gauss: Gauss { c, k, e, ratexp },
        chi,
        rc,
    })
}

impl Gauss {
    fn forward(&self, elp: LambdaPhi) -> LambdaPhi {
        LambdaPhi {
            phi: 2.0
                * (self.k
                    * (0.5 * elp.phi + FRAC_PI_4).tan().powf(self.c)
                    * srat(self.e * elp.phi.sin(), self.ratexp))
                .atan()
                - FRAC_PI_2,
            lambda: self.c * elp.lambda,
        }
    }

    fn inverse(&self, slp: LambdaPhi) -> Result<LambdaPhi, Error> {
        let lambda = slp.lambda / self.c;
        let num = ((0.5 * slp.phi + FRAC_PI_4).tan() / self.k).powf(1.0 / self.c);
        let mut slp_phi = slp.phi;
        for _ in 0..MAX_ITER {
            let phi = 2.0 * (num * srat(self.e * slp_phi.sin(), -0.5 * self.e)).atan() - FRAC_PI_2;
            if (phi - slp_phi).abs() < DEL_TOL {
                return Ok(LambdaPhi { lambda, phi });
            }
            slp_phi = phi;
        }
        Err(Error::ProjectionError(
            "oblique stereographic inverse did not converge".into(),
        ))
    }
}

pub(super) struct ObliqueStereographic {
    gauss: Gauss,
    k0: f64,
    phic0: f64,
    cosc0: f64,
    sinc0: f64,
    r2: f64,
}

impl ObliqueStereographic {
    pub(super) fn new(shape: &EllipsoidConstants, phi0: f64, k0: f64) -> Result<Self, Error> {
        let GaussSetup { gauss, chi, rc } = gauss_ini(shape.e, phi0)?;
        Ok(Self {
            gauss,
            k0,
            phic0: chi,
            sinc0: chi.sin(),
            cosc0: chi.cos(),
            r2: 2.0 * rc,
        })
    }
}

impl ClassicProjection for ObliqueStereographic {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let lp = self.gauss.forward(lp);
        let sinc = lp.phi.sin();
        let cosc = lp.phi.cos();
        let cosl = lp.lambda.cos();
        let denom = 1.0 + self.sinc0 * sinc + self.cosc0 * cosc * cosl;
        if denom == 0.0 {
            return Err(Error::ProjectionError(
                "coordinate outside the projection domain".into(),
            ));
        }
        let k = self.k0 * self.r2 / denom;
        Ok(Coord::new(
            k * cosc * lp.lambda.sin(),
            k * (self.cosc0 * sinc - self.sinc0 * cosc * cosl),
        ))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let x = xy.x / self.k0;
        let y = xy.y / self.k0;
        let rho = x.hypot(y);
        let lp = if rho != 0.0 {
            let c = 2.0 * rho.atan2(self.r2);
            let sinc = c.sin();
            let cosc = c.cos();
            LambdaPhi {
                phi: (cosc * self.sinc0 + y * sinc * self.cosc0 / rho).asin(),
                lambda: (x * sinc).atan2(rho * self.cosc0 * cosc - y * self.sinc0 * sinc),
            }
        } else {
            LambdaPhi {
                phi: self.phic0,
                lambda: 0.0,
            }
        };
        self.gauss.inverse(lp)
    }
}
