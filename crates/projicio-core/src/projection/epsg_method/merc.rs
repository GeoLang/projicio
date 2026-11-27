// adapted from PROJ src/projections/merc.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::FRAC_PI_2;

use super::math::{msfn, sinhpsi2tanphi};
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

pub(super) struct Mercator {
    k0: f64,
    e: f64,
    spherical: bool,
}

impl Mercator {
    pub(super) fn with_scale(shape: &EllipsoidConstants, k0: f64) -> Self {
        Self {
            k0,
            e: shape.e,
            spherical: shape.es == 0.0,
        }
    }

    pub(super) fn with_standard_parallel(
        shape: &EllipsoidConstants,
        lat_ts: f64,
    ) -> Result<Self, Error> {
        let phits = lat_ts.abs();
        if phits >= FRAC_PI_2 {
            return Err(Error::ProjectionError(
                "merc: |lat_ts| should be < 90 degrees".into(),
            ));
        }
        let k0 = if shape.es != 0.0 {
            msfn(phits.sin(), phits.cos(), shape.es)
        } else {
            phits.cos()
        };
        Ok(Self::with_scale(shape, k0))
    }

    pub(super) fn web() -> Self {
        // merc.cpp's webmerc runs the spherical formulas on the ellipsoid's semi-major axis
        Self {
            k0: 1.0,
            e: 0.0,
            spherical: true,
        }
    }
}

impl ClassicProjection for Mercator {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let x = self.k0 * lp.lambda;
        if self.spherical {
            return Ok(Coord::new(x, self.k0 * lp.phi.tan().asinh()));
        }
        let sphi = lp.phi.sin();
        let cphi = lp.phi.cos();
        let y = self.k0 * ((sphi / cphi).asinh() - self.e * (self.e * sphi).atanh());
        Ok(Coord::new(x, y))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let lambda = xy.x / self.k0;
        let phi = if self.spherical {
            (xy.y / self.k0).sinh().atan()
        } else {
            sinhpsi2tanphi((xy.y / self.k0).sinh(), self.e)?.atan()
        };
        Ok(LambdaPhi { lambda, phi })
    }
}
