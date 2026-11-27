// adapted from PROJ src/projections/omerc.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use super::math::{aasin, phi2, tsfn};
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const TOL: f64 = 1e-7;
const EPS: f64 = 1e-10;

fn outside_domain() -> Error {
    Error::ProjectionError("coordinate outside the projection domain".into())
}

struct CentreConstants {
    a: f64,
    b: f64,
    big_e: f64,
    d: f64,
    f: f64,
}

pub(super) struct ObliqueMercator {
    pub(super) lam0: f64,
    e: f64,
    a: f64,
    b: f64,
    big_e: f64,
    ar_b: f64,
    br_a: f64,
    r_b: f64,
    singam: f64,
    cosgam: f64,
    sinrot: f64,
    cosrot: f64,
    v_pole_n: f64,
    v_pole_s: f64,
    u_0: f64,
}

impl ObliqueMercator {
    pub(super) fn variant_a(
        shape: &EllipsoidConstants,
        phi0: f64,
        lonc: f64,
        alpha: f64,
        gamma: f64,
        k0: f64,
    ) -> Result<Self, Error> {
        Self::setup(shape, phi0, lonc, alpha, gamma, k0, true)
    }

    pub(super) fn variant_b(
        shape: &EllipsoidConstants,
        phi0: f64,
        lonc: f64,
        alpha: f64,
        gamma: f64,
        k0: f64,
    ) -> Result<Self, Error> {
        Self::setup(shape, phi0, lonc, alpha, gamma, k0, false)
    }

    fn setup(
        shape: &EllipsoidConstants,
        phi0: f64,
        lamc: f64,
        alpha_c: f64,
        gamma: f64,
        k0: f64,
        no_off: bool,
    ) -> Result<Self, Error> {
        let com = shape.one_es.sqrt();
        let CentreConstants { a, b, big_e, d, f } = if phi0.abs() > EPS {
            let sinph0 = phi0.sin();
            let cosph0 = phi0.cos();
            let con = 1.0 - shape.es * sinph0 * sinph0;
            let cos_squared = cosph0 * cosph0;
            let b = (1.0 + shape.es * cos_squared * cos_squared / shape.one_es).sqrt();
            let d = b * com / (cosph0 * con.sqrt());
            let mut f = d * d - 1.0;
            if f <= 0.0 {
                f = 0.0;
            } else {
                f = f.sqrt();
                if phi0 < 0.0 {
                    f = -f;
                }
            }
            f += d;
            CentreConstants {
                a: b * k0 * com / con,
                b,
                big_e: f * tsfn(phi0, sinph0, shape.e).powf(b),
                d,
                f,
            }
        } else {
            CentreConstants {
                a: k0,
                b: 1.0 / com,
                big_e: 1.0,
                d: 1.0,
                f: 1.0,
            }
        };

        let gamma0 = aasin(alpha_c.sin() / d)?;

        if (phi0.abs() - FRAC_PI_2).abs() <= TOL {
            return Err(Error::ProjectionError(
                "invalid value for lat_0: |lat_0| should be < 90 degrees".into(),
            ));
        }

        let lam0 = lamc - aasin(0.5 * (f - 1.0 / f) * gamma0.tan())? / b;

        let r_b = 1.0 / b;
        let ar_b = a * r_b;
        let br_a = 1.0 / ar_b;
        let u_0 = if no_off {
            0.0
        } else {
            let u_0 = (ar_b * ((d * d - 1.0).sqrt() / alpha_c.cos()).atan()).abs();
            if phi0 < 0.0 { -u_0 } else { u_0 }
        };
        let half_gamma0 = 0.5 * gamma0;
        Ok(Self {
            lam0,
            e: shape.e,
            a,
            b,
            big_e,
            ar_b,
            br_a,
            r_b,
            singam: gamma0.sin(),
            cosgam: gamma0.cos(),
            sinrot: gamma.sin(),
            cosrot: gamma.cos(),
            v_pole_n: ar_b * (FRAC_PI_4 - half_gamma0).tan().ln(),
            v_pole_s: ar_b * (FRAC_PI_4 + half_gamma0).tan().ln(),
            u_0,
        })
    }
}

impl ClassicProjection for ObliqueMercator {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let mut u;
        let v;
        if (lp.phi.abs() - FRAC_PI_2).abs() > EPS {
            let w = self.big_e / tsfn(lp.phi, lp.phi.sin(), self.e).powf(self.b);
            let one_div_w = 1.0 / w;
            let s = 0.5 * (w - one_div_w);
            let t = 0.5 * (w + one_div_w);
            let big_v = (self.b * lp.lambda).sin();
            let big_u = (s * self.singam - big_v * self.cosgam) / t;
            if (big_u.abs() - 1.0).abs() < EPS {
                return Err(outside_domain());
            }
            v = 0.5 * self.ar_b * ((1.0 - big_u) / (1.0 + big_u)).ln();
            let temp = (self.b * lp.lambda).cos();
            u = if temp.abs() < TOL {
                self.a * lp.lambda
            } else {
                self.ar_b * (s * self.cosgam + big_v * self.singam).atan2(temp)
            };
        } else {
            v = if lp.phi > 0.0 {
                self.v_pole_n
            } else {
                self.v_pole_s
            };
            u = self.ar_b * lp.phi;
        }
        u -= self.u_0;
        Ok(Coord::new(
            v * self.cosrot + u * self.sinrot,
            u * self.cosrot - v * self.sinrot,
        ))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let v = xy.x * self.cosrot - xy.y * self.sinrot;
        let u = xy.y * self.cosrot + xy.x * self.sinrot + self.u_0;
        let qp = (-self.br_a * v).exp();
        if qp == 0.0 {
            return Err(outside_domain());
        }
        let sp = 0.5 * (qp - 1.0 / qp);
        let tp = 0.5 * (qp + 1.0 / qp);
        let vp = (self.br_a * u).sin();
        let up = (vp * self.cosgam + sp * self.singam) / tp;
        if (up.abs() - 1.0).abs() < EPS {
            return Ok(LambdaPhi {
                lambda: 0.0,
                phi: if up < 0.0 { -FRAC_PI_2 } else { FRAC_PI_2 },
            });
        }
        let phi = self.big_e / ((1.0 + up) / (1.0 - up)).sqrt();
        let phi = phi2(phi.powf(1.0 / self.b), self.e)?;
        let lambda = -self.r_b * (sp * self.cosgam - vp * self.singam).atan2((self.br_a * u).cos());
        Ok(LambdaPhi { lambda, phi })
    }
}
