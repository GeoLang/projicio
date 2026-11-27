// adapted from PROJ src/projections/stere.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use super::math::tsfn;
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const EPS10: f64 = 1e-10;
const TOL: f64 = 1e-8;
const NITER: usize = 8;
const CONV: f64 = 1e-10;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    SouthPole,
    NorthPole,
    Oblique,
    Equatorial,
}

pub(super) struct Stereographic {
    e: f64,
    spherical: bool,
    phi0: f64,
    sin_x1: f64,
    cos_x1: f64,
    akm1: f64,
    mode: Mode,
}

fn outside_domain() -> Error {
    Error::ProjectionError("stere: coordinate outside the projection domain".into())
}

fn ssfn(phit: f64, sinphi: f64, eccen: f64) -> f64 {
    let sinphi = sinphi * eccen;
    (0.5 * (FRAC_PI_2 + phit)).tan() * ((1.0 - sinphi) / (1.0 + sinphi)).powf(0.5 * eccen)
}

impl Stereographic {
    pub(super) fn new(shape: &EllipsoidConstants, phi0: f64, phits: f64, k0: f64) -> Self {
        let t = phi0.abs();
        let mode = if (t - FRAC_PI_2).abs() < EPS10 {
            if phi0 < 0.0 {
                Mode::SouthPole
            } else {
                Mode::NorthPole
            }
        } else if t > EPS10 {
            Mode::Oblique
        } else {
            Mode::Equatorial
        };
        let phits = phits.abs();
        let e = shape.e;
        let mut sin_x1 = 0.0;
        let mut cos_x1 = 0.0;
        let akm1;
        if shape.es != 0.0 {
            match mode {
                Mode::NorthPole | Mode::SouthPole => {
                    akm1 = if (phits - FRAC_PI_2).abs() < EPS10 {
                        2.0 * k0 / ((1.0 + e).powf(1.0 + e) * (1.0 - e).powf(1.0 - e)).sqrt()
                    } else {
                        let t = phits.sin();
                        let akm1 = phits.cos() / tsfn(phits, t, e);
                        let t = t * e;
                        akm1 / (1.0 - t * t).sqrt()
                    };
                }
                Mode::Equatorial | Mode::Oblique => {
                    let t = phi0.sin();
                    let x = 2.0 * ssfn(phi0, t, e).atan() - FRAC_PI_2;
                    let t = t * e;
                    akm1 = 2.0 * k0 * phi0.cos() / (1.0 - t * t).sqrt();
                    sin_x1 = x.sin();
                    cos_x1 = x.cos();
                }
            }
        } else {
            match mode {
                Mode::Oblique | Mode::Equatorial => {
                    if mode == Mode::Oblique {
                        sin_x1 = phi0.sin();
                        cos_x1 = phi0.cos();
                    }
                    akm1 = 2.0 * k0;
                }
                Mode::SouthPole | Mode::NorthPole => {
                    akm1 = if (phits - FRAC_PI_2).abs() >= EPS10 {
                        phits.cos() / (FRAC_PI_4 - 0.5 * phits).tan()
                    } else {
                        2.0 * k0
                    };
                }
            }
        }
        Self {
            e,
            spherical: shape.es == 0.0,
            phi0,
            sin_x1,
            cos_x1,
            akm1,
            mode,
        }
    }

    fn e_forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let mut coslam = lp.lambda.cos();
        let sinlam = lp.lambda.sin();
        let mut sinphi = lp.phi.sin();
        let mut phi = lp.phi;
        let (mut sin_x, mut cos_x) = (0.0, 0.0);
        if matches!(self.mode, Mode::Oblique | Mode::Equatorial) {
            let x = 2.0 * ssfn(phi, sinphi, self.e).atan() - FRAC_PI_2;
            sin_x = x.sin();
            cos_x = x.cos();
        }

        let x;
        let y;
        match self.mode {
            Mode::Oblique => {
                let denom =
                    self.cos_x1 * (1.0 + self.sin_x1 * sin_x + self.cos_x1 * cos_x * coslam);
                if denom == 0.0 {
                    return Err(outside_domain());
                }
                let a = self.akm1 / denom;
                y = a * (self.cos_x1 * sin_x - self.sin_x1 * cos_x * coslam);
                x = a * cos_x;
            }
            Mode::Equatorial => {
                if 1.0 + cos_x * coslam == 0.0 {
                    return Err(outside_domain());
                }
                let a = self.akm1 / (1.0 + cos_x * coslam);
                y = a * sin_x;
                x = a * cos_x;
            }
            Mode::SouthPole | Mode::NorthPole => {
                if self.mode == Mode::SouthPole {
                    phi = -phi;
                    coslam = -coslam;
                    sinphi = -sinphi;
                }
                x = if (phi - FRAC_PI_2).abs() < 1e-15 {
                    0.0
                } else {
                    self.akm1 * tsfn(phi, sinphi, self.e)
                };
                y = -x * coslam;
            }
        }
        Ok(Coord::new(x * sinlam, y))
    }

    fn s_forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let sinphi = lp.phi.sin();
        let cosphi = lp.phi.cos();
        let mut coslam = lp.lambda.cos();
        let sinlam = lp.lambda.sin();

        match self.mode {
            Mode::Equatorial | Mode::Oblique => {
                let mut y = if self.mode == Mode::Equatorial {
                    1.0 + cosphi * coslam
                } else {
                    1.0 + self.sin_x1 * sinphi + self.cos_x1 * cosphi * coslam
                };
                if y <= EPS10 {
                    return Err(outside_domain());
                }
                y = self.akm1 / y;
                let x = y * cosphi * sinlam;
                y *= if self.mode == Mode::Equatorial {
                    sinphi
                } else {
                    self.cos_x1 * sinphi - self.sin_x1 * cosphi * coslam
                };
                Ok(Coord::new(x, y))
            }
            Mode::NorthPole | Mode::SouthPole => {
                let mut phi = lp.phi;
                if self.mode == Mode::NorthPole {
                    coslam = -coslam;
                    phi = -phi;
                }
                if (phi - FRAC_PI_2).abs() < TOL {
                    return Err(outside_domain());
                }
                let y = self.akm1 * (FRAC_PI_4 + 0.5 * phi).tan();
                Ok(Coord::new(sinlam * y, y * coslam))
            }
        }
    }

    fn e_inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let mut x = xy.x;
        let mut y = xy.y;
        let rho = x.hypot(y);
        let tp;
        let mut phi_l;
        let halfpi;
        let halfe;
        match self.mode {
            Mode::Oblique | Mode::Equatorial => {
                let tp0 = 2.0 * (rho * self.cos_x1).atan2(self.akm1);
                let cosphi = tp0.cos();
                let sinphi = tp0.sin();
                phi_l = if rho == 0.0 {
                    (cosphi * self.sin_x1).asin()
                } else {
                    (cosphi * self.sin_x1 + (y * sinphi * self.cos_x1 / rho)).asin()
                };
                tp = (0.5 * (FRAC_PI_2 + phi_l)).tan();
                x *= sinphi;
                y = rho * self.cos_x1 * cosphi - y * self.sin_x1 * sinphi;
                halfpi = FRAC_PI_2;
                halfe = 0.5 * self.e;
            }
            Mode::NorthPole | Mode::SouthPole => {
                if self.mode == Mode::NorthPole {
                    y = -y;
                }
                tp = -rho / self.akm1;
                phi_l = FRAC_PI_2 - 2.0 * tp.atan();
                halfpi = -FRAC_PI_2;
                halfe = -0.5 * self.e;
            }
        }

        for _ in 0..NITER {
            let sinphi = self.e * phi_l.sin();
            let mut phi =
                2.0 * (tp * ((1.0 + sinphi) / (1.0 - sinphi)).powf(halfe)).atan() - halfpi;
            if (phi_l - phi).abs() < CONV {
                if self.mode == Mode::SouthPole {
                    phi = -phi;
                }
                let lambda = if x == 0.0 && y == 0.0 {
                    0.0
                } else {
                    x.atan2(y)
                };
                return Ok(LambdaPhi { lambda, phi });
            }
            phi_l = phi;
        }
        Err(outside_domain())
    }

    fn s_inverse(&self, xy: Coord) -> LambdaPhi {
        let x = xy.x;
        let mut y = xy.y;
        let rh = x.hypot(y);
        let c = 2.0 * (rh / self.akm1).atan();
        let sinc = c.sin();
        let cosc = c.cos();
        let mut lambda = 0.0;
        let phi;

        match self.mode {
            Mode::Equatorial => {
                phi = if rh.abs() <= EPS10 {
                    0.0
                } else {
                    (y * sinc / rh).asin()
                };
                if cosc != 0.0 || x != 0.0 {
                    lambda = (x * sinc).atan2(cosc * rh);
                }
            }
            Mode::Oblique => {
                phi = if rh.abs() <= EPS10 {
                    self.phi0
                } else {
                    (cosc * self.sin_x1 + y * sinc * self.cos_x1 / rh).asin()
                };
                let c = cosc - self.sin_x1 * phi.sin();
                if c != 0.0 || x != 0.0 {
                    lambda = (x * sinc * self.cos_x1).atan2(c * rh);
                }
            }
            Mode::NorthPole | Mode::SouthPole => {
                if self.mode == Mode::NorthPole {
                    y = -y;
                }
                phi = if rh.abs() <= EPS10 {
                    self.phi0
                } else if self.mode == Mode::SouthPole {
                    (-cosc).asin()
                } else {
                    cosc.asin()
                };
                lambda = if x == 0.0 && y == 0.0 {
                    0.0
                } else {
                    x.atan2(y)
                };
            }
        }
        LambdaPhi { lambda, phi }
    }
}

impl ClassicProjection for Stereographic {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        if self.spherical {
            self.s_forward(lp)
        } else {
            self.e_forward(lp)
        }
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        if self.spherical {
            Ok(self.s_inverse(xy))
        } else {
            self.e_inverse(xy)
        }
    }
}
