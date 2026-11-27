// adapted from PROJ src/projections/somerc.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use super::math::aasin;
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const EPS: f64 = 1e-10;
const NITER: usize = 6;

pub(super) struct SwissObliqueMercator {
    e: f64,
    rone_es: f64,
    k: f64,
    c: f64,
    hlf_e: f64,
    k_r: f64,
    cosp0: f64,
    sinp0: f64,
}

impl SwissObliqueMercator {
    pub(super) fn new(shape: &EllipsoidConstants, phi0: f64, k0: f64) -> Result<Self, Error> {
        let hlf_e = 0.5 * shape.e;
        let mut cp = phi0.cos();
        cp *= cp;
        let c = (1.0 + shape.es * cp * cp * shape.rone_es).sqrt();
        let mut sp = phi0.sin();
        let sinp0 = sp / c;
        let phip0 = aasin(sinp0)?;
        let cosp0 = phip0.cos();
        sp *= shape.e;
        let k = (FRAC_PI_4 + 0.5 * phip0).tan().ln()
            - c * ((FRAC_PI_4 + 0.5 * phi0).tan().ln() - hlf_e * ((1.0 + sp) / (1.0 - sp)).ln());
        let k_r = k0 * shape.one_es.sqrt() / (1.0 - sp * sp);
        Ok(Self {
            e: shape.e,
            rone_es: shape.rone_es,
            k,
            c,
            hlf_e,
            k_r,
            cosp0,
            sinp0,
        })
    }
}

impl ClassicProjection for SwissObliqueMercator {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let sp = self.e * lp.phi.sin();
        let phip = 2.0
            * (self.c
                * ((FRAC_PI_4 + 0.5 * lp.phi).tan().ln()
                    - self.hlf_e * ((1.0 + sp) / (1.0 - sp)).ln())
                + self.k)
                .exp()
                .atan()
            - FRAC_PI_2;
        let lamp = self.c * lp.lambda;
        let cp = phip.cos();
        let phipp = aasin(self.cosp0 * phip.sin() - self.sinp0 * cp * lamp.cos())?;
        let lampp = aasin(cp * lamp.sin() / phipp.cos())?;
        Ok(Coord::new(
            self.k_r * lampp,
            self.k_r * (FRAC_PI_4 + 0.5 * phipp).tan().ln(),
        ))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let phipp = 2.0 * ((xy.y / self.k_r).exp().atan() - FRAC_PI_4);
        let lampp = xy.x / self.k_r;
        let cp = phipp.cos();
        let mut phip = aasin(self.cosp0 * phipp.sin() + self.sinp0 * cp * lampp.cos())?;
        let lamp = aasin(cp * lampp.sin() / phip.cos())?;
        let con = (self.k - (FRAC_PI_4 + 0.5 * phip).tan().ln()) / self.c;
        for _ in 0..NITER {
            let esp = self.e * phip.sin();
            let delp = (con + (FRAC_PI_4 + 0.5 * phip).tan().ln()
                - self.hlf_e * ((1.0 + esp) / (1.0 - esp)).ln())
                * (1.0 - esp * esp)
                * phip.cos()
                * self.rone_es;
            phip -= delp;
            if delp.abs() < EPS {
                return Ok(LambdaPhi {
                    lambda: lamp / self.c,
                    phi: phip,
                });
            }
        }
        Err(Error::ProjectionError(
            "Swiss oblique Mercator inverse did not converge".into(),
        ))
    }
}
