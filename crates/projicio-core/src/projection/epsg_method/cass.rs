// adapted from PROJ src/projections/cass.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::math::{MeridianDistance, generic_inverse_2d};
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const C1: f64 = 1.0 / 6.0;
const C2: f64 = 1.0 / 120.0;
const C3: f64 = 1.0 / 24.0;
const C4: f64 = 1.0 / 3.0;
const C5: f64 = 1.0 / 15.0;

pub(super) enum CassiniSoldner {
    Ellipsoidal(Ellipsoidal),
    Spherical { phi0: f64 },
}

pub(super) struct Ellipsoidal {
    es: f64,
    meridian: MeridianDistance,
    m0: f64,
    hyperbolic: bool,
}

impl CassiniSoldner {
    pub(super) fn new(shape: &EllipsoidConstants, phi0: f64) -> Self {
        Self::build(shape, phi0, false)
    }

    pub(super) fn hyperbolic(shape: &EllipsoidConstants, phi0: f64) -> Self {
        Self::build(shape, phi0, true)
    }

    fn build(shape: &EllipsoidConstants, phi0: f64, hyperbolic: bool) -> Self {
        if shape.es == 0.0 {
            return Self::Spherical { phi0 };
        }
        let meridian = MeridianDistance::new(shape.n);
        let m0 = meridian.mlfn(phi0, phi0.sin(), phi0.cos());
        Self::Ellipsoidal(Ellipsoidal {
            es: shape.es,
            meridian,
            m0,
            hyperbolic,
        })
    }
}

impl Ellipsoidal {
    fn forward(&self, lp: LambdaPhi) -> Coord {
        let sinphi = lp.phi.sin();
        let cosphi = lp.phi.cos();
        let m = self.meridian.mlfn(lp.phi, sinphi, cosphi);

        let nu_square = 1.0 / (1.0 - self.es * sinphi * sinphi);
        let nu = nu_square.sqrt();
        let tanphi = lp.phi.tan();
        let t = tanphi * tanphi;
        let a = lp.lambda * cosphi;
        let c = self.es * (cosphi * cosphi) / (1.0 - self.es);
        let a2 = a * a;

        let x = nu * a * (1.0 - a2 * t * (C1 + (8.0 - t + 8.0 * c) * a2 * C2));
        let mut y = m - self.m0 + nu * tanphi * a2 * (0.5 + (5.0 - t + 6.0 * c) * a2 * C3);
        if self.hyperbolic {
            let rho = nu_square * (1.0 - self.es) * nu;
            y -= y * y * y / (6.0 * rho * nu);
        }
        Coord::new(x, y)
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let phi1 = self.meridian.inv_mlfn(self.m0 + xy.y);
        let tanphi1 = phi1.tan();
        let t1 = tanphi1 * tanphi1;
        let sinphi1 = phi1.sin();
        let nu1_square = 1.0 / (1.0 - self.es * sinphi1 * sinphi1);
        let nu1 = nu1_square.sqrt();
        let rho1 = nu1_square * (1.0 - self.es) * nu1;
        let d = xy.x / nu1;
        let d2 = d * d;
        let initial = LambdaPhi {
            phi: phi1 - (nu1 * tanphi1 / rho1) * d2 * (0.5 - (1.0 + 3.0 * t1) * d2 * C3),
            lambda: d * (1.0 + t1 * d2 * (-C4 + (1.0 + 3.0 * t1) * d2 * C5)) / phi1.cos(),
        };
        // cass.cpp refines its series inverse with the generic Newton inverse, also for the hyperbolic case
        const DELTA_XY_TOLERANCE: f64 = 1e-12;
        generic_inverse_2d(xy, initial, DELTA_XY_TOLERANCE, |lp| self.forward(lp))
    }
}

impl ClassicProjection for CassiniSoldner {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        match self {
            Self::Ellipsoidal(ellipsoidal) => Ok(ellipsoidal.forward(lp)),
            Self::Spherical { phi0 } => Ok(Coord::new(
                (lp.phi.cos() * lp.lambda.sin()).asin(),
                lp.phi.tan().atan2(lp.lambda.cos()) - phi0,
            )),
        }
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        match self {
            Self::Ellipsoidal(ellipsoidal) => ellipsoidal.inverse(xy),
            Self::Spherical { phi0 } => {
                let dd = xy.y + phi0;
                Ok(LambdaPhi {
                    phi: (dd.sin() * xy.x.cos()).asin(),
                    lambda: xy.x.tan().atan2(dd.cos()),
                })
            }
        }
    }
}
