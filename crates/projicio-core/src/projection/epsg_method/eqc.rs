// adapted from PROJ src/projections/eqc.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::math::MeridianDistance;
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

pub(super) struct EquidistantCylindrical {
    rc: f64,
    form: Form,
}

enum Form {
    Ellipsoidal { meridian: MeridianDistance, m0: f64 },
    Spherical { phi0: f64 },
}

impl EquidistantCylindrical {
    pub(super) fn new(shape: &EllipsoidConstants, lat_ts: f64, phi0: f64) -> Result<Self, Error> {
        let cos_phi1 = lat_ts.cos();
        if cos_phi1 <= 0.0 {
            return Err(Error::ProjectionError(
                "eqc: |lat_ts| should be < 90 degrees".into(),
            ));
        }
        if shape.es == 0.0 {
            return Ok(Self {
                rc: cos_phi1,
                form: Form::Spherical { phi0 },
            });
        }
        let sin_phi1 = lat_ts.sin();
        let nu1 = 1.0 / (1.0 - shape.es * sin_phi1 * sin_phi1).sqrt();
        let meridian = MeridianDistance::new(shape.n);
        let m0 = meridian.mlfn(phi0, phi0.sin(), phi0.cos());
        Ok(Self {
            rc: nu1 * cos_phi1,
            form: Form::Ellipsoidal { meridian, m0 },
        })
    }
}

impl ClassicProjection for EquidistantCylindrical {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let x = self.rc * lp.lambda;
        let y = match &self.form {
            Form::Ellipsoidal { meridian, m0 } => {
                meridian.mlfn(lp.phi, lp.phi.sin(), lp.phi.cos()) - m0
            }
            Form::Spherical { phi0 } => lp.phi - phi0,
        };
        Ok(Coord::new(x, y))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let lambda = xy.x / self.rc;
        let phi = match &self.form {
            Form::Ellipsoidal { meridian, m0 } => meridian.inv_mlfn(xy.y + m0),
            Form::Spherical { phi0 } => xy.y + phi0,
        };
        Ok(LambdaPhi { lambda, phi })
    }
}
