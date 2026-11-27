// adapted from PROJ src/projections/eqearth.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::math::{AuthalicLatitude, authalic_q};
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const A1: f64 = 1.340264;
const A2: f64 = -0.081106;
const A3: f64 = 0.000893;
const A4: f64 = 0.003796;
// eqearth.cpp's 90 degree latitude on a sphere with radius 1
const MAX_Y: f64 = 1.3173627591574;
const EPS: f64 = 1e-11;
const MAX_ITER: usize = 12;

fn m() -> f64 {
    3.0_f64.sqrt() / 2.0
}

pub(super) struct EqualEarth {
    rqda: f64,
    ellipsoidal: Option<Ellipsoidal>,
}

struct Ellipsoidal {
    shape: EllipsoidConstants,
    authalic: AuthalicLatitude,
}

impl EqualEarth {
    pub(super) fn new(shape: &EllipsoidConstants) -> Self {
        if shape.es == 0.0 {
            return Self {
                rqda: 1.0,
                ellipsoidal: None,
            };
        }
        let qp = authalic_q(1.0, shape);
        Self {
            rqda: (0.5 * qp).sqrt(),
            ellipsoidal: Some(Ellipsoidal {
                shape: *shape,
                authalic: AuthalicLatitude::new(shape, qp),
            }),
        }
    }
}

impl ClassicProjection for EqualEarth {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let mut sbeta = lp.phi.sin();
        if let Some(ellipsoidal) = &self.ellipsoidal {
            sbeta =
                (authalic_q(sbeta, &ellipsoidal.shape) / ellipsoidal.authalic.qp).clamp(-1.0, 1.0);
        }
        let m = m();
        let psi = (m * sbeta).asin();
        let psi2 = psi * psi;
        let psi6 = psi2 * psi2 * psi2;
        let x = lp.lambda * psi.cos()
            / (m * (A1 + 3.0 * A2 * psi2 + psi6 * (7.0 * A3 + 9.0 * A4 * psi2)));
        let y = psi * (A1 + A2 * psi2 + psi6 * (A3 + A4 * psi2));
        Ok(Coord::new(x * self.rqda, y * self.rqda))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let x = xy.x / self.rqda;
        let y = (xy.y / self.rqda).clamp(-MAX_Y, MAX_Y);
        let mut yc = y;
        let mut converged = false;
        for _ in 0..MAX_ITER {
            let y2 = yc * yc;
            let y6 = y2 * y2 * y2;
            let f = yc * (A1 + A2 * y2 + y6 * (A3 + A4 * y2)) - y;
            let fder = A1 + 3.0 * A2 * y2 + y6 * (7.0 * A3 + 9.0 * A4 * y2);
            let tol = f / fder;
            yc -= tol;
            if tol.abs() < EPS {
                converged = true;
                break;
            }
        }
        if !converged {
            return Err(Error::ProjectionError(
                "eqearth: inverse did not converge".into(),
            ));
        }
        let m = m();
        let y2 = yc * yc;
        let y6 = y2 * y2 * y2;
        let lambda = m * x * (A1 + 3.0 * A2 * y2 + y6 * (7.0 * A3 + 9.0 * A4 * y2)) / yc.cos();
        let mut phi = (yc.sin() / m).asin();
        if let Some(ellipsoidal) = &self.ellipsoidal {
            phi = ellipsoidal.authalic.geographic(phi, &ellipsoidal.shape);
        }
        Ok(LambdaPhi { lambda, phi })
    }
}
