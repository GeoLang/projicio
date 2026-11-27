// adapted from PROJ src/projections/aea.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::FRAC_PI_2;

use super::math::{AuthalicLatitude, authalic_q, msfn};
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const EPS10: f64 = 1e-10;
const TOL7: f64 = 1e-7;

pub(super) struct AlbersEqualArea {
    n: f64,
    c: f64,
    dd: f64,
    rho0: f64,
    form: Form,
}

enum Form {
    Ellipsoidal {
        shape: EllipsoidConstants,
        ec: f64,
        authalic: AuthalicLatitude,
    },
    Spherical {
        n2: f64,
    },
}

fn invalid(message: &str) -> Error {
    Error::ProjectionError(format!("aea: {message}"))
}

fn outside_domain() -> Error {
    Error::ProjectionError("aea: coordinate outside the projection domain".into())
}

impl AlbersEqualArea {
    pub(super) fn new(
        shape: &EllipsoidConstants,
        phi0: f64,
        phi1: f64,
        phi2: f64,
    ) -> Result<Self, Error> {
        if phi1.abs() > FRAC_PI_2 {
            return Err(invalid("|lat_1| should be <= 90 degrees"));
        }
        if phi2.abs() > FRAC_PI_2 {
            return Err(invalid("|lat_2| should be <= 90 degrees"));
        }
        if (phi1 + phi2).abs() < EPS10 {
            return Err(invalid("|lat_1 + lat_2| should be > 0"));
        }
        let mut sinphi = phi1.sin();
        let mut n = sinphi;
        let mut cosphi = phi1.cos();
        let secant = (phi1 - phi2).abs() >= EPS10;
        if shape.es > 0.0 {
            let qp = authalic_q(1.0, shape);
            let m1 = msfn(sinphi, cosphi, shape.es);
            let ml1 = authalic_q(sinphi, shape);
            if secant {
                sinphi = phi2.sin();
                cosphi = phi2.cos();
                let m2 = msfn(sinphi, cosphi, shape.es);
                let ml2 = authalic_q(sinphi, shape);
                if ml2 == ml1 {
                    return Err(invalid("standard parallels give the same authalic q"));
                }
                n = (m1 * m1 - m2 * m2) / (ml2 - ml1);
                if n == 0.0 {
                    return Err(invalid("invalid eccentricity"));
                }
            }
            let ec = 1.0 - 0.5 * shape.one_es * ((1.0 - shape.e) / (1.0 + shape.e)).ln() / shape.e;
            let c = m1 * m1 + n * ml1;
            let dd = 1.0 / n;
            let rho0 = dd * (c - n * authalic_q(phi0.sin(), shape)).sqrt();
            return Ok(Self {
                n,
                c,
                dd,
                rho0,
                form: Form::Ellipsoidal {
                    shape: *shape,
                    ec,
                    authalic: AuthalicLatitude::new(shape, qp),
                },
            });
        }
        if secant {
            n = 0.5 * (n + phi2.sin());
        }
        let n2 = n + n;
        let c = cosphi * cosphi + n2 * sinphi;
        let dd = 1.0 / n;
        let rho0 = dd * (c - n2 * phi0.sin()).sqrt();
        Ok(Self {
            n,
            c,
            dd,
            rho0,
            form: Form::Spherical { n2 },
        })
    }
}

impl ClassicProjection for AlbersEqualArea {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let mut rho = self.c
            - match &self.form {
                Form::Ellipsoidal { shape, .. } => self.n * authalic_q(lp.phi.sin(), shape),
                Form::Spherical { n2 } => n2 * lp.phi.sin(),
            };
        if rho < 0.0 {
            return Err(outside_domain());
        }
        rho = self.dd * rho.sqrt();
        let lambda = lp.lambda * self.n;
        Ok(Coord::new(
            rho * lambda.sin(),
            self.rho0 - rho * lambda.cos(),
        ))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let mut x = xy.x;
        let mut y = self.rho0 - xy.y;
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
        let rho_over_dd = rho / self.dd;
        let phi = match &self.form {
            Form::Ellipsoidal {
                shape,
                ec,
                authalic,
            } => {
                let qs = (self.c - rho_over_dd * rho_over_dd) / self.n;
                if (ec - qs.abs()).abs() > TOL7 {
                    if qs.abs() > 2.0 {
                        return Err(outside_domain());
                    }
                    authalic.geographic((qs / authalic.qp).asin(), shape)
                } else if qs < 0.0 {
                    -FRAC_PI_2
                } else {
                    FRAC_PI_2
                }
            }
            Form::Spherical { n2 } => {
                let qs_div_2 = (self.c - rho_over_dd * rho_over_dd) / n2;
                if qs_div_2.abs() <= 1.0 {
                    qs_div_2.asin()
                } else if qs_div_2 < 0.0 {
                    -FRAC_PI_2
                } else {
                    FRAC_PI_2
                }
            }
        };
        Ok(LambdaPhi {
            lambda: x.atan2(y) / self.n,
            phi,
        })
    }
}
