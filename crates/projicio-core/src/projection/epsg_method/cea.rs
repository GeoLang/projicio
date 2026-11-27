// adapted from PROJ src/projections/cea.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::FRAC_PI_2;

use super::math::{AuthalicLatitude, authalic_q};
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const EPS: f64 = 1e-10;

pub(super) struct CylindricalEqualArea {
    k0: f64,
    form: Form,
}

enum Form {
    Ellipsoidal {
        shape: EllipsoidConstants,
        authalic: AuthalicLatitude,
    },
    Spherical,
}

impl CylindricalEqualArea {
    pub(super) fn new(shape: &EllipsoidConstants, lat_ts: f64) -> Result<Self, Error> {
        let mut k0 = lat_ts.cos();
        if k0 < 0.0 {
            return Err(Error::ProjectionError(
                "cea: |lat_ts| should be <= 90 degrees".into(),
            ));
        }
        if shape.es == 0.0 {
            return Ok(Self {
                k0,
                form: Form::Spherical,
            });
        }
        let t = lat_ts.sin();
        k0 /= (1.0 - shape.es * t * t).sqrt();
        let qp = authalic_q(1.0, shape);
        Ok(Self {
            k0,
            form: Form::Ellipsoidal {
                shape: *shape,
                authalic: AuthalicLatitude::new(shape, qp),
            },
        })
    }
}

impl ClassicProjection for CylindricalEqualArea {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let x = self.k0 * lp.lambda;
        let y = match &self.form {
            Form::Ellipsoidal { shape, .. } => 0.5 * authalic_q(lp.phi.sin(), shape) / self.k0,
            Form::Spherical => lp.phi.sin() / self.k0,
        };
        Ok(Coord::new(x, y))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let lambda = xy.x / self.k0;
        match &self.form {
            Form::Ellipsoidal { shape, authalic } => Ok(LambdaPhi {
                phi: authalic.geographic((2.0 * xy.y * self.k0 / authalic.qp).asin(), shape),
                lambda,
            }),
            Form::Spherical => {
                let y = xy.y * self.k0;
                let t = y.abs();
                if t - EPS > 1.0 {
                    return Err(Error::ProjectionError(
                        "cea: coordinate outside the projection domain".into(),
                    ));
                }
                let phi = if t >= 1.0 {
                    if y < 0.0 { -FRAC_PI_2 } else { FRAC_PI_2 }
                } else {
                    y.asin()
                };
                Ok(LambdaPhi { lambda, phi })
            }
        }
    }
}
