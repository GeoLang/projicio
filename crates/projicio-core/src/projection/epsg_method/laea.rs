// adapted from PROJ src/projections/laea.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use super::math::{AuthalicLatitude, authalic_q};
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const EPS10: f64 = 1e-10;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    NorthPole,
    SouthPole,
    Equatorial,
    Oblique,
}

pub(super) struct LambertAzimuthalEqualArea {
    phi0: f64,
    mode: Mode,
    sinb1: f64,
    cosb1: f64,
    form: Form,
}

enum Form {
    Ellipsoidal(Ellipsoidal),
    Spherical,
}

struct Ellipsoidal {
    shape: EllipsoidConstants,
    xmf: f64,
    ymf: f64,
    dd: f64,
    rq: f64,
    authalic: AuthalicLatitude,
}

fn outside_domain() -> Error {
    Error::ProjectionError("laea: coordinate outside the projection domain".into())
}

impl LambertAzimuthalEqualArea {
    pub(super) fn new(shape: &EllipsoidConstants, phi0: f64) -> Result<Self, Error> {
        let t = phi0.abs();
        if t > FRAC_PI_2 + EPS10 {
            return Err(Error::ProjectionError(
                "laea: |lat_0| should be <= 90 degrees".into(),
            ));
        }
        let mode = if (t - FRAC_PI_2).abs() < EPS10 {
            if phi0 < 0.0 {
                Mode::SouthPole
            } else {
                Mode::NorthPole
            }
        } else if t.abs() < EPS10 {
            Mode::Equatorial
        } else {
            Mode::Oblique
        };
        if shape.es == 0.0 {
            let (sinb1, cosb1) = if mode == Mode::Oblique {
                (phi0.sin(), phi0.cos())
            } else {
                (0.0, 0.0)
            };
            return Ok(Self {
                phi0,
                mode,
                sinb1,
                cosb1,
                form: Form::Spherical,
            });
        }
        let qp = authalic_q(1.0, shape);
        let authalic = AuthalicLatitude::new(shape, qp);
        let (mut sinb1, mut cosb1) = (0.0, 0.0);
        let (mut xmf, mut ymf, mut rq) = (0.0, 0.0, 0.0);
        let dd;
        match mode {
            Mode::NorthPole | Mode::SouthPole => {
                dd = 1.0;
            }
            Mode::Equatorial => {
                rq = (0.5 * qp).sqrt();
                dd = 1.0 / rq;
                xmf = 1.0;
                ymf = 0.5 * qp;
            }
            Mode::Oblique => {
                rq = (0.5 * qp).sqrt();
                let sinphi = phi0.sin();
                let cosphi = phi0.cos();
                let b1 = authalic.authalic(phi0, sinphi, cosphi, shape);
                sinb1 = b1.sin();
                cosb1 = b1.cos();
                dd = phi0.cos() / ((1.0 - shape.es * sinphi * sinphi).sqrt() * rq * cosb1);
                xmf = rq;
                ymf = xmf / dd;
                xmf *= dd;
            }
        }
        Ok(Self {
            phi0,
            mode,
            sinb1,
            cosb1,
            form: Form::Ellipsoidal(Ellipsoidal {
                shape: *shape,
                xmf,
                ymf,
                dd,
                rq,
                authalic,
            }),
        })
    }

    fn e_forward(&self, ellipsoidal: &Ellipsoidal, lp: LambdaPhi) -> Result<Coord, Error> {
        let coslam = lp.lambda.cos();
        let sinlam = lp.lambda.sin();
        let sinphi = lp.phi.sin();
        let cosphi = lp.phi.cos();
        let qp = ellipsoidal.authalic.qp;
        let xi = ellipsoidal
            .authalic
            .authalic(lp.phi, sinphi, cosphi, &ellipsoidal.shape);
        let mut q = xi.sin() * qp;
        let (mut sinb, mut cosb) = (0.0, 0.0);
        if matches!(self.mode, Mode::Oblique | Mode::Equatorial) {
            sinb = xi.sin();
            cosb = xi.cos();
        }

        let mut b = match self.mode {
            Mode::Oblique => 1.0 + self.sinb1 * sinb + self.cosb1 * cosb * coslam,
            Mode::Equatorial => 1.0 + cosb * coslam,
            Mode::NorthPole => {
                q = qp - q;
                FRAC_PI_2 + lp.phi
            }
            Mode::SouthPole => {
                q += qp;
                lp.phi - FRAC_PI_2
            }
        };
        if b.abs() < EPS10 {
            return Err(outside_domain());
        }

        match self.mode {
            Mode::Oblique => {
                b = (2.0 / b).sqrt();
                let y = ellipsoidal.ymf * b * (self.cosb1 * sinb - self.sinb1 * cosb * coslam);
                Ok(Coord::new(ellipsoidal.xmf * b * cosb * sinlam, y))
            }
            Mode::Equatorial => {
                b = (2.0 / (1.0 + cosb * coslam)).sqrt();
                let y = b * sinb * ellipsoidal.ymf;
                Ok(Coord::new(ellipsoidal.xmf * b * cosb * sinlam, y))
            }
            Mode::NorthPole | Mode::SouthPole => {
                if q >= 1e-15 {
                    b = q.sqrt();
                    let y = coslam * if self.mode == Mode::SouthPole { b } else { -b };
                    Ok(Coord::new(b * sinlam, y))
                } else {
                    Ok(Coord::new(0.0, 0.0))
                }
            }
        }
    }

    fn s_forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let sinphi = lp.phi.sin();
        let cosphi = lp.phi.cos();
        let mut coslam = lp.lambda.cos();
        match self.mode {
            Mode::Equatorial | Mode::Oblique => {
                let mut y = if self.mode == Mode::Equatorial {
                    1.0 + cosphi * coslam
                } else {
                    1.0 + self.sinb1 * sinphi + self.cosb1 * cosphi * coslam
                };
                if y <= EPS10 {
                    return Err(outside_domain());
                }
                y = (2.0 / y).sqrt();
                let x = y * cosphi * lp.lambda.sin();
                y *= if self.mode == Mode::Equatorial {
                    sinphi
                } else {
                    self.cosb1 * sinphi - self.sinb1 * cosphi * coslam
                };
                Ok(Coord::new(x, y))
            }
            Mode::NorthPole | Mode::SouthPole => {
                if self.mode == Mode::NorthPole {
                    coslam = -coslam;
                }
                if (lp.phi + self.phi0).abs() < EPS10 {
                    return Err(outside_domain());
                }
                let mut y = FRAC_PI_4 - lp.phi * 0.5;
                y = 2.0
                    * if self.mode == Mode::SouthPole {
                        y.cos()
                    } else {
                        y.sin()
                    };
                let x = y * lp.lambda.sin();
                Ok(Coord::new(x, y * coslam))
            }
        }
    }

    fn e_inverse(&self, ellipsoidal: &Ellipsoidal, xy: Coord) -> Result<LambdaPhi, Error> {
        let mut x = xy.x;
        let mut y = xy.y;
        let qp = ellipsoidal.authalic.qp;
        let mut ab;
        match self.mode {
            Mode::Equatorial | Mode::Oblique => {
                x /= ellipsoidal.dd;
                y *= ellipsoidal.dd;
                let rho = x.hypot(y);
                if rho < EPS10 {
                    return Ok(LambdaPhi {
                        lambda: 0.0,
                        phi: self.phi0,
                    });
                }
                let asin_argument = 0.5 * rho / ellipsoidal.rq;
                if asin_argument > 1.0 {
                    return Err(outside_domain());
                }
                let s_ce_angle = 2.0 * asin_argument.asin();
                let c_ce = s_ce_angle.cos();
                let s_ce = s_ce_angle.sin();
                x *= s_ce;
                if self.mode == Mode::Oblique {
                    ab = c_ce * self.sinb1 + y * s_ce * self.cosb1 / rho;
                    y = rho * self.cosb1 * c_ce - y * self.sinb1 * s_ce;
                } else {
                    ab = y * s_ce / rho;
                    y = rho * c_ce;
                }
            }
            Mode::NorthPole | Mode::SouthPole => {
                if self.mode == Mode::NorthPole {
                    y = -y;
                }
                let q = x * x + y * y;
                if q == 0.0 {
                    return Ok(LambdaPhi {
                        lambda: 0.0,
                        phi: self.phi0,
                    });
                }
                ab = 1.0 - q / qp;
                if self.mode == Mode::SouthPole {
                    ab = -ab;
                }
            }
        }
        Ok(LambdaPhi {
            lambda: x.atan2(y),
            phi: ellipsoidal
                .authalic
                .geographic(ab.asin(), &ellipsoidal.shape),
        })
    }

    fn s_inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let mut x = xy.x;
        let mut y = xy.y;
        let rh = x.hypot(y);
        let mut phi = rh * 0.5;
        if phi > 1.0 {
            return Err(outside_domain());
        }
        phi = 2.0 * phi.asin();
        let (mut sinz, mut cosz) = (0.0, 0.0);
        if matches!(self.mode, Mode::Oblique | Mode::Equatorial) {
            sinz = phi.sin();
            cosz = phi.cos();
        }
        match self.mode {
            Mode::Equatorial => {
                phi = if rh.abs() <= EPS10 {
                    0.0
                } else {
                    (y * sinz / rh).asin()
                };
                x *= sinz;
                y = cosz * rh;
            }
            Mode::Oblique => {
                phi = if rh.abs() <= EPS10 {
                    self.phi0
                } else {
                    (cosz * self.sinb1 + y * sinz * self.cosb1 / rh).asin()
                };
                x *= sinz * self.cosb1;
                y = (cosz - phi.sin() * self.sinb1) * rh;
            }
            Mode::NorthPole => {
                y = -y;
                phi = FRAC_PI_2 - phi;
            }
            Mode::SouthPole => {
                phi -= FRAC_PI_2;
            }
        }
        let lambda = if y == 0.0 && matches!(self.mode, Mode::Equatorial | Mode::Oblique) {
            0.0
        } else {
            x.atan2(y)
        };
        Ok(LambdaPhi { lambda, phi })
    }
}

impl ClassicProjection for LambertAzimuthalEqualArea {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        match &self.form {
            Form::Ellipsoidal(ellipsoidal) => self.e_forward(ellipsoidal, lp),
            Form::Spherical => self.s_forward(lp),
        }
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        match &self.form {
            Form::Ellipsoidal(ellipsoidal) => self.e_inverse(ellipsoidal, xy),
            Form::Spherical => self.s_inverse(xy),
        }
    }
}
