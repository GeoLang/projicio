// adapted from PROJ src/projections/col_urban.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

pub(super) struct ColombiaUrban {
    es: f64,
    phi0: f64,
    h0: f64,
    rho0: f64,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

impl ColombiaUrban {
    pub(super) fn new(shape: &EllipsoidConstants, phi0: f64, h0_unscaled: f64) -> Self {
        let es = shape.es;
        let h0 = h0_unscaled / shape.a;
        let sinphi0 = phi0.sin();
        let nu0 = 1.0 / (1.0 - es * sinphi0 * sinphi0).sqrt();
        let rho0 = (1.0 - es) / (1.0 - es * sinphi0 * sinphi0).powf(1.5);
        Self {
            es,
            phi0,
            h0,
            rho0,
            a: 1.0 + h0 / nu0,
            b: phi0.tan() / (2.0 * rho0 * nu0),
            c: 1.0 + h0,
            d: rho0 * (1.0 + h0 / (1.0 - es)),
        }
    }
}

impl ClassicProjection for ColombiaUrban {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let cosphi = lp.phi.cos();
        let sinphi = lp.phi.sin();
        let nu = 1.0 / (1.0 - self.es * sinphi * sinphi).sqrt();
        let lam_nu_cosphi = lp.lambda * nu * cosphi;
        let x = self.a * lam_nu_cosphi;
        let sinphi_m = (0.5 * (lp.phi + self.phi0)).sin();
        let rho_m = (1.0 - self.es) / (1.0 - self.es * sinphi_m * sinphi_m).powf(1.5);
        let g = 1.0 + self.h0 / rho_m;
        let y = g * self.rho0 * ((lp.phi - self.phi0) + self.b * lam_nu_cosphi * lam_nu_cosphi);
        Ok(Coord::new(x, y))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let phi = self.phi0 + xy.y / self.d - self.b * (xy.x / self.c) * (xy.x / self.c);
        let sinphi = phi.sin();
        let nu = 1.0 / (1.0 - self.es * sinphi * sinphi).sqrt();
        Ok(LambdaPhi {
            lambda: xy.x / (self.c * nu * phi.cos()),
            phi,
        })
    }
}
