// adapted from PROJ src/projections/ortho.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::{FRAC_PI_2, PI};

use super::math::adjlon;
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

pub(super) struct Orthographic {
    phi0: f64,
    k0: f64,
    sinph0: f64,
    cosph0: f64,
    sinalpha: f64,
    cosalpha: f64,
    mode: Mode,
    ellipsoidal: Option<Ellipsoidal>,
}

struct Ellipsoidal {
    es: f64,
    one_es: f64,
    a_over_b: f64,
    nu0: f64,
    y_shift: f64,
    y_scale: f64,
}

fn outside_domain() -> Error {
    Error::ProjectionError("ortho: coordinate outside the projection domain".into())
}

impl Orthographic {
    pub(super) fn new(shape: &EllipsoidConstants, phi0: f64, alpha: f64, k0: f64) -> Self {
        let sinph0 = phi0.sin();
        let cosph0 = phi0.cos();
        let mode = if (phi0.abs() - FRAC_PI_2).abs() <= EPS10 {
            if phi0 < 0.0 {
                Mode::SouthPole
            } else {
                Mode::NorthPole
            }
        } else if phi0.abs() > EPS10 {
            Mode::Oblique
        } else {
            Mode::Equatorial
        };
        let ellipsoidal = (shape.es != 0.0).then(|| {
            let nu0 = 1.0 / (1.0 - shape.es * sinph0 * sinph0).sqrt();
            Ellipsoidal {
                es: shape.es,
                one_es: shape.one_es,
                a_over_b: 1.0 / (1.0 - shape.f),
                nu0,
                y_shift: shape.es * nu0 * sinph0 * cosph0,
                y_scale: 1.0 / (1.0 - shape.es * cosph0 * cosph0).sqrt(),
            }
        });
        Self {
            phi0,
            k0,
            sinph0,
            cosph0,
            sinalpha: alpha.sin(),
            cosalpha: alpha.cos(),
            mode,
            ellipsoidal,
        }
    }

    fn s_forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let cosphi = lp.phi.cos();
        let mut coslam = lp.lambda.cos();
        let y = match self.mode {
            Mode::Equatorial => {
                if cosphi * coslam < -EPS10 {
                    return Err(outside_domain());
                }
                lp.phi.sin()
            }
            Mode::Oblique => {
                let sinphi = lp.phi.sin();
                if self.sinph0 * sinphi + self.cosph0 * cosphi * coslam < -EPS10 {
                    return Err(outside_domain());
                }
                self.cosph0 * sinphi - self.sinph0 * cosphi * coslam
            }
            Mode::NorthPole | Mode::SouthPole => {
                if self.mode == Mode::NorthPole {
                    coslam = -coslam;
                }
                if (lp.phi - self.phi0).abs() - EPS10 > FRAC_PI_2 {
                    return Err(outside_domain());
                }
                cosphi * coslam
            }
        };
        let xp = cosphi * lp.lambda.sin();
        let yp = y;
        Ok(Coord::new(
            (xp * self.cosalpha - yp * self.sinalpha) * self.k0,
            (xp * self.sinalpha + yp * self.cosalpha) * self.k0,
        ))
    }

    fn s_inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let mut x = (self.cosalpha * xy.x + self.sinalpha * xy.y) / self.k0;
        let mut y = (-self.sinalpha * xy.x + self.cosalpha * xy.y) / self.k0;

        let rh = x.hypot(y);
        let mut sinc = rh;
        if sinc > 1.0 {
            if (sinc - 1.0) > EPS10 {
                return Err(outside_domain());
            }
            sinc = 1.0;
        }
        let cosc = (1.0 - sinc * sinc).sqrt();
        if rh.abs() <= EPS10 {
            return Ok(LambdaPhi {
                lambda: 0.0,
                phi: self.phi0,
            });
        }
        let mut phi = match self.mode {
            Mode::NorthPole => {
                y = -y;
                sinc.acos()
            }
            Mode::SouthPole => -sinc.acos(),
            Mode::Equatorial => {
                let phi = y * sinc / rh;
                x *= sinc;
                y = cosc * rh;
                phi
            }
            Mode::Oblique => {
                let phi = cosc * self.sinph0 + y * sinc * self.cosph0 / rh;
                y = (cosc - self.sinph0 * phi) * rh;
                x *= sinc * self.cosph0;
                phi
            }
        };
        let equatorial_or_oblique = matches!(self.mode, Mode::Equatorial | Mode::Oblique);
        if equatorial_or_oblique {
            phi = if phi.abs() >= 1.0 {
                if phi < 0.0 { -FRAC_PI_2 } else { FRAC_PI_2 }
            } else {
                phi.asin()
            };
        }
        let lambda = if y == 0.0 && equatorial_or_oblique {
            if x == 0.0 {
                0.0
            } else if x < 0.0 {
                -FRAC_PI_2
            } else {
                FRAC_PI_2
            }
        } else {
            x.atan2(y)
        };
        Ok(LambdaPhi { lambda, phi })
    }

    fn e_forward(&self, ellipsoidal: &Ellipsoidal, lp: LambdaPhi) -> Result<Coord, Error> {
        let cosphi = lp.phi.cos();
        let sinphi = lp.phi.sin();
        let coslam = lp.lambda.cos();
        let sinlam = lp.lambda.sin();
        if self.sinph0 * sinphi + self.cosph0 * cosphi * coslam < -EPS10 {
            return Err(outside_domain());
        }
        let es = ellipsoidal.es;
        let nu = 1.0 / (1.0 - es * sinphi * sinphi).sqrt();
        let xp = nu * cosphi * sinlam;
        let yp = nu * (sinphi * self.cosph0 - cosphi * self.sinph0 * coslam)
            + es * (ellipsoidal.nu0 * self.sinph0 - nu * sinphi) * self.cosph0;
        Ok(Coord::new(
            (self.cosalpha * xp - self.sinalpha * yp) * self.k0,
            (self.sinalpha * xp + self.cosalpha * yp) * self.k0,
        ))
    }

    fn e_inverse(&self, ellipsoidal: &Ellipsoidal, xy_in: Coord) -> Result<LambdaPhi, Error> {
        let square = |value: f64| value * value;
        let es = ellipsoidal.es;
        let x = (self.cosalpha * xy_in.x + self.sinalpha * xy_in.y) / self.k0;
        let y = (-self.sinalpha * xy_in.x + self.cosalpha * xy_in.y) / self.k0;

        if matches!(self.mode, Mode::NorthPole | Mode::SouthPole) {
            let rh2 = square(x) + square(y);
            let phi = if rh2 >= 1.0 - 1e-15 {
                if (rh2 - 1.0) > EPS10 {
                    return Err(outside_domain());
                }
                0.0
            } else {
                (rh2 * ellipsoidal.one_es / (1.0 - es * rh2)).sqrt().acos()
                    * if self.mode == Mode::NorthPole {
                        1.0
                    } else {
                        -1.0
                    }
            };
            let lambda = x.atan2(
                y * if self.mode == Mode::NorthPole {
                    -1.0
                } else {
                    1.0
                },
            );
            return Ok(LambdaPhi { lambda, phi });
        }

        if self.mode == Mode::Equatorial {
            if square(x) + square(y * ellipsoidal.a_over_b) > 1.0 + 1e-11 {
                return Err(outside_domain());
            }
            let sinphi2 = if y == 0.0 {
                0.0
            } else {
                1.0 / (square((1.0 - es) / y) + es)
            };
            let sign_y = if y > 0.0 { 1.0 } else { -1.0 };
            if sinphi2 > 1.0 - 1e-11 {
                return Ok(LambdaPhi {
                    lambda: 0.0,
                    phi: FRAC_PI_2 * sign_y,
                });
            }
            let phi = sinphi2.sqrt().asin() * sign_y;
            let sinlam = x * ((1.0 - es * sinphi2) / (1.0 - sinphi2)).sqrt();
            let lambda = if sinlam.abs() - 1.0 > -1e-15 {
                FRAC_PI_2 * if x > 0.0 { 1.0 } else { -1.0 }
            } else {
                sinlam.asin()
            };
            return Ok(LambdaPhi { lambda, phi });
        }

        let xy_recentered = Coord::new(x, (y - ellipsoidal.y_shift) / ellipsoidal.y_scale);
        if square(x) + square(xy_recentered.y) > 1.0 + 1e-11 {
            return Err(outside_domain());
        }
        // ortho.cpp seeds Newton with the spherical inverse, which rotates and scales its input a second time
        let mut lp = self.s_inverse(xy_recentered)?;
        for _ in 0..20 {
            let cosphi = lp.phi.cos();
            let sinphi = lp.phi.sin();
            let coslam = lp.lambda.cos();
            let sinlam = lp.lambda.sin();
            let one_minus_es_sinphi2 = 1.0 - es * sinphi * sinphi;
            let nu = 1.0 / one_minus_es_sinphi2.sqrt();
            let x_new = nu * cosphi * sinlam;
            let y_new = nu * (sinphi * self.cosph0 - cosphi * self.sinph0 * coslam)
                + es * (ellipsoidal.nu0 * self.sinph0 - nu * sinphi) * self.cosph0;
            let rho = (1.0 - es) * nu / one_minus_es_sinphi2;
            let j11 = -rho * sinphi * sinlam;
            let j12 = nu * cosphi * coslam;
            let j21 = rho * (cosphi * self.cosph0 + sinphi * self.sinph0 * coslam);
            let j22 = nu * self.sinph0 * cosphi * sinlam;
            let d = j11 * j22 - j12 * j21;
            let dx = x - x_new;
            let dy = y - y_new;
            let dphi = (j22 * dx - j12 * dy) / d;
            let dlam = (-j21 * dx + j11 * dy) / d;
            lp.phi += dphi;
            if lp.phi > FRAC_PI_2 {
                lp.phi = FRAC_PI_2 - (lp.phi - FRAC_PI_2);
                lp.lambda = adjlon(lp.lambda + PI);
            } else if lp.phi < -FRAC_PI_2 {
                lp.phi = -FRAC_PI_2 + (-FRAC_PI_2 - lp.phi);
                lp.lambda = adjlon(lp.lambda + PI);
            }
            lp.lambda += dlam;
            if dphi.abs() < 1e-12 && dlam.abs() < 1e-12 {
                return Ok(lp);
            }
        }
        Err(outside_domain())
    }
}

impl ClassicProjection for Orthographic {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        match &self.ellipsoidal {
            Some(ellipsoidal) => self.e_forward(ellipsoidal, lp),
            None => self.s_forward(lp),
        }
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        match &self.ellipsoidal {
            Some(ellipsoidal) => self.e_inverse(ellipsoidal, xy),
            None => self.s_inverse(xy),
        }
    }
}
