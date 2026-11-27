// adapted from PROJ src/projections/poly.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::math::{MeridianDistance, msfn};
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const TOL: f64 = 1e-10;
const CONV: f64 = 1e-10;
const N_ITER: usize = 10;
const I_ITER: usize = 20;
const ITOL: f64 = 1e-12;

pub(super) struct Polyconic {
    ml0: f64,
    phi0: f64,
    form: Form,
}

enum Form {
    Ellipsoidal {
        es: f64,
        one_es: f64,
        meridian: MeridianDistance,
    },
    Spherical,
}

fn outside_domain() -> Error {
    Error::ProjectionError("poly: coordinate outside the projection domain".into())
}

impl Polyconic {
    pub(super) fn new(shape: &EllipsoidConstants, phi0: f64) -> Self {
        if shape.es == 0.0 {
            return Self {
                ml0: -phi0,
                phi0,
                form: Form::Spherical,
            };
        }
        let meridian = MeridianDistance::new(shape.n);
        Self {
            ml0: meridian.mlfn(phi0, phi0.sin(), phi0.cos()),
            phi0,
            form: Form::Ellipsoidal {
                es: shape.es,
                one_es: shape.one_es,
                meridian,
            },
        }
    }

    fn e_forward(&self, es: f64, meridian: &MeridianDistance, lp: LambdaPhi) -> Coord {
        if lp.phi.abs() <= TOL {
            return Coord::new(lp.lambda, -self.ml0);
        }
        let sp = lp.phi.sin();
        let cp = lp.phi.cos();
        let ms = if cp.abs() > TOL {
            msfn(sp, cp, es) / sp
        } else {
            0.0
        };
        let lambda = lp.lambda * sp;
        Coord::new(
            ms * lambda.sin(),
            (meridian.mlfn(lp.phi, sp, cp) - self.ml0) + ms * (1.0 - lambda.cos()),
        )
    }

    fn s_forward(&self, lp: LambdaPhi) -> Coord {
        if lp.phi.abs() <= TOL {
            return Coord::new(lp.lambda, self.ml0);
        }
        let cot = 1.0 / lp.phi.tan();
        let e = lp.lambda * lp.phi.sin();
        Coord::new(e.sin() * cot, lp.phi - self.phi0 + cot * (1.0 - e.cos()))
    }

    fn e_inverse(
        &self,
        es: f64,
        one_es: f64,
        meridian: &MeridianDistance,
        xy: Coord,
    ) -> Result<LambdaPhi, Error> {
        let y = xy.y + self.ml0;
        if y.abs() <= TOL {
            return Ok(LambdaPhi {
                lambda: xy.x,
                phi: 0.0,
            });
        }
        let r = y * y + xy.x * xy.x;
        let mut phi = y;
        let mut converged = false;
        for _ in 0..I_ITER {
            let sp = phi.sin();
            let cp = phi.cos();
            let s2ph = sp * cp;
            if cp.abs() < ITOL {
                return Err(outside_domain());
            }
            let mut mlp = (1.0 - es * sp * sp).sqrt();
            let c = sp * mlp / cp;
            let ml = meridian.mlfn(phi, sp, cp);
            let mlb = ml * ml + r;
            mlp = one_es / (mlp * mlp * mlp);
            let d_phi = (ml + ml + c * mlb - 2.0 * y * (c * ml + 1.0))
                / (es * s2ph * (mlb - 2.0 * y * ml) / c + 2.0 * (y - ml) * (c * mlp - 1.0 / s2ph)
                    - mlp
                    - mlp);
            phi += d_phi;
            if d_phi.abs() <= ITOL {
                converged = true;
                break;
            }
        }
        if !converged {
            return Err(outside_domain());
        }
        let c = phi.sin();
        Ok(LambdaPhi {
            lambda: (xy.x * phi.tan() * (1.0 - es * c * c).sqrt()).asin() / phi.sin(),
            phi,
        })
    }

    fn s_inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let y = self.phi0 + xy.y;
        if y.abs() <= TOL {
            return Ok(LambdaPhi {
                lambda: xy.x,
                phi: 0.0,
            });
        }
        let mut phi = y;
        let b = xy.x * xy.x + y * y;
        let mut remaining = N_ITER;
        loop {
            let tp = phi.tan();
            let dphi =
                (y * (phi * tp + 1.0) - phi - 0.5 * (phi * phi + b) * tp) / ((phi - y) / tp - 1.0);
            phi -= dphi;
            if dphi.is_nan() || dphi.abs() <= CONV {
                break;
            }
            remaining -= 1;
            if remaining == 0 {
                return Err(outside_domain());
            }
        }
        Ok(LambdaPhi {
            lambda: (xy.x * phi.tan()).asin() / phi.sin(),
            phi,
        })
    }
}

impl ClassicProjection for Polyconic {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        match &self.form {
            Form::Ellipsoidal { es, meridian, .. } => Ok(self.e_forward(*es, meridian, lp)),
            Form::Spherical => Ok(self.s_forward(lp)),
        }
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        match &self.form {
            Form::Ellipsoidal {
                es,
                one_es,
                meridian,
            } => self.e_inverse(*es, *one_es, meridian, xy),
            Form::Spherical => self.s_inverse(xy),
        }
    }
}
