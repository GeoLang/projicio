// adapted from PROJ src/projections/lcc.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use super::math::{self, msfn, tsfn};
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const EPS10: f64 = 1e-10;

pub(super) struct LambertConformalConic {
    es: f64,
    e: f64,
    k0: f64,
    n: f64,
    rho0: f64,
    c: f64,
}

fn invalid(message: &str) -> Error {
    Error::ProjectionError(format!("lcc: {message}"))
}

impl LambertConformalConic {
    pub(super) fn new(
        shape: &EllipsoidConstants,
        phi0: f64,
        phi1: f64,
        phi2: f64,
        k0: f64,
    ) -> Result<Self, Error> {
        if (phi1 + phi2).abs() < EPS10 {
            return Err(invalid("|lat_1 + lat_2| should be > 0"));
        }
        let mut sinphi = phi1.sin();
        let mut n = sinphi;
        let cosphi = phi1.cos();
        if cosphi.abs() < EPS10 || phi1.abs() >= FRAC_PI_2 {
            return Err(invalid("|lat_1| should be < 90 degrees"));
        }
        if phi2.cos().abs() < EPS10 || phi2.abs() >= FRAC_PI_2 {
            return Err(invalid("|lat_2| should be < 90 degrees"));
        }
        let secant = (phi1 - phi2).abs() >= EPS10;
        let (c, rho0) = if shape.es != 0.0 {
            let m1 = msfn(sinphi, cosphi, shape.es);
            let ml1 = tsfn(phi1, sinphi, shape.e);
            if secant {
                sinphi = phi2.sin();
                n = (m1 / msfn(sinphi, phi2.cos(), shape.es)).ln();
                if n == 0.0 {
                    return Err(invalid("invalid eccentricity"));
                }
                let ml2 = tsfn(phi2, sinphi, shape.e);
                let denom = (ml1 / ml2).ln();
                if denom == 0.0 {
                    return Err(invalid("invalid eccentricity"));
                }
                n /= denom;
            }
            let c = m1 * ml1.powf(-n) / n;
            let rho0 = c * if ((phi0.abs()) - FRAC_PI_2).abs() < EPS10 {
                0.0
            } else {
                tsfn(phi0, phi0.sin(), shape.e).powf(n)
            };
            (c, rho0)
        } else {
            if secant {
                n = (cosphi / phi2.cos()).ln()
                    / ((FRAC_PI_4 + 0.5 * phi2).tan() / (FRAC_PI_4 + 0.5 * phi1).tan()).ln();
            }
            if n == 0.0 {
                return Err(invalid("|lat_1 + lat_2| should be > 0"));
            }
            let c = cosphi * (FRAC_PI_4 + 0.5 * phi1).tan().powf(n) / n;
            let rho0 = if ((phi0.abs()) - FRAC_PI_2).abs() < EPS10 {
                0.0
            } else {
                c * (FRAC_PI_4 + 0.5 * phi0).tan().powf(-n)
            };
            (c, rho0)
        };
        Ok(Self {
            es: shape.es,
            e: shape.e,
            k0,
            n,
            rho0,
            c,
        })
    }
}

impl ClassicProjection for LambertConformalConic {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let rho = if (lp.phi.abs() - FRAC_PI_2).abs() < EPS10 {
            if lp.phi * self.n <= 0.0 {
                return Err(Error::ProjectionError(
                    "lcc: pole outside the projection domain".into(),
                ));
            }
            0.0
        } else if self.es != 0.0 {
            self.c * tsfn(lp.phi, lp.phi.sin(), self.e).powf(self.n)
        } else {
            self.c * (FRAC_PI_4 + 0.5 * lp.phi).tan().powf(-self.n)
        };
        let lambda = lp.lambda * self.n;
        Ok(Coord::new(
            self.k0 * (rho * lambda.sin()),
            self.k0 * (self.rho0 - rho * lambda.cos()),
        ))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let mut x = xy.x / self.k0;
        let mut y = xy.y / self.k0;
        y = self.rho0 - y;
        let mut rho = x.hypot(y);
        if rho == 0.0 {
            return Ok(LambdaPhi {
                lambda: 0.0,
                phi: if self.n > 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 },
            });
        }
        if self.n < 0.0 {
            rho = -rho;
            x = -x;
            y = -y;
        }
        let phi = if self.es != 0.0 {
            math::phi2((rho / self.c).powf(1.0 / self.n), self.e)?
        } else {
            2.0 * (self.c / rho).powf(1.0 / self.n).atan() - FRAC_PI_2
        };
        Ok(LambdaPhi {
            lambda: x.atan2(y) / self.n,
            phi,
        })
    }
}
