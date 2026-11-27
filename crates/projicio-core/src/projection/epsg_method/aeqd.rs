// adapted from PROJ src/projections/aeqd.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::{FRAC_PI_2, PI};

use super::geodesic::Geodesic;
use super::math::{MeridianDistance, aasin};
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const EPS10: f64 = 1.0e-10;
const TOL: f64 = 1.0e-14;
const DEG_TO_RAD: f64 = PI / 180.0;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    NorthPole,
    SouthPole,
    Equatorial,
    Oblique,
}

struct Aspect {
    mode: Mode,
    sinph0: f64,
    cosph0: f64,
}

fn aspect(phi0: f64) -> Aspect {
    if (phi0.abs() - FRAC_PI_2).abs() < EPS10 {
        Aspect {
            mode: if phi0 < 0.0 {
                Mode::SouthPole
            } else {
                Mode::NorthPole
            },
            sinph0: if phi0 < 0.0 { -1.0 } else { 1.0 },
            cosph0: 0.0,
        }
    } else if phi0.abs() < EPS10 {
        Aspect {
            mode: Mode::Equatorial,
            sinph0: 0.0,
            cosph0: 1.0,
        }
    } else {
        Aspect {
            mode: Mode::Oblique,
            sinph0: phi0.sin(),
            cosph0: phi0.cos(),
        }
    }
}

enum Variant {
    Spherical,
    Ellipsoidal {
        meridian: MeridianDistance,
        mp: f64,
    },
    Guam {
        es: f64,
        e: f64,
        meridian: MeridianDistance,
        m1: f64,
    },
}

pub(super) struct AzimuthalEquidistant {
    phi0: f64,
    sinph0: f64,
    cosph0: f64,
    mode: Mode,
    geodesic: Geodesic,
    variant: Variant,
}

fn outside_domain() -> Error {
    Error::ProjectionError("coordinate outside the azimuthal equidistant domain".into())
}

impl AzimuthalEquidistant {
    pub(super) fn new(shape: &EllipsoidConstants, phi0: f64) -> Result<Self, Error> {
        let aspect = aspect(phi0);
        let variant = if shape.es == 0.0 {
            Variant::Spherical
        } else {
            let meridian = MeridianDistance::new(shape.n);
            let mp = match aspect.mode {
                Mode::NorthPole => meridian.mlfn(FRAC_PI_2, 1.0, 0.0),
                Mode::SouthPole => meridian.mlfn(-FRAC_PI_2, -1.0, 0.0),
                Mode::Equatorial | Mode::Oblique => 0.0,
            };
            Variant::Ellipsoidal { meridian, mp }
        };
        Ok(Self {
            phi0,
            sinph0: aspect.sinph0,
            cosph0: aspect.cosph0,
            mode: aspect.mode,
            geodesic: Geodesic::new(shape.f),
            variant,
        })
    }

    pub(super) fn guam(shape: &EllipsoidConstants, phi0: f64) -> Result<Self, Error> {
        let aspect = aspect(phi0);
        // aeqd.cpp only reads +guam on an ellipsoid
        let variant = if shape.es == 0.0 {
            Variant::Spherical
        } else {
            let meridian = MeridianDistance::new(shape.n);
            let m1 = meridian.mlfn(phi0, aspect.sinph0, aspect.cosph0);
            Variant::Guam {
                es: shape.es,
                e: shape.e,
                meridian,
                m1,
            }
        };
        Ok(Self {
            phi0,
            sinph0: aspect.sinph0,
            cosph0: aspect.cosph0,
            mode: aspect.mode,
            geodesic: Geodesic::new(shape.f),
            variant,
        })
    }

    fn geodesic_forward(&self, lp: LambdaPhi) -> Coord {
        if lp.lambda.abs() < EPS10 && (lp.phi - self.phi0).abs() < EPS10 {
            return Coord::new(0.0, 0.0);
        }
        let lat1 = self.phi0 / DEG_TO_RAD;
        let lon1 = 0.0;
        let lat2 = lp.phi / DEG_TO_RAD;
        let lon2 = lp.lambda / DEG_TO_RAD;
        let inverse = self.geodesic.inverse(lat1, lon1, lat2, lon2);
        let azi1 = inverse.azi1 * DEG_TO_RAD;
        Coord::new(inverse.s12 * azi1.sin(), inverse.s12 * azi1.cos())
    }

    fn ellipsoidal_forward(&self, lp: LambdaPhi, meridian: &MeridianDistance, mp: f64) -> Coord {
        let mut coslam = lp.lambda.cos();
        match self.mode {
            Mode::NorthPole | Mode::SouthPole => {
                if self.mode == Mode::NorthPole {
                    coslam = -coslam;
                }
                let cosphi = lp.phi.cos();
                let sinphi = lp.phi.sin();
                let rho = (mp - meridian.mlfn(lp.phi, sinphi, cosphi)).abs();
                Coord::new(rho * lp.lambda.sin(), rho * coslam)
            }
            Mode::Equatorial | Mode::Oblique => self.geodesic_forward(lp),
        }
    }

    fn spherical_forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let cosphi = lp.phi.cos();
        let sinphi = lp.phi.sin();
        let coslam = lp.lambda.cos();
        let sinlam = lp.lambda.sin();
        match self.mode {
            Mode::Equatorial | Mode::Oblique => {
                let cosphi_x_coslam = cosphi * coslam;
                let y = if self.mode == Mode::Equatorial {
                    cosphi_x_coslam
                } else {
                    self.sinph0 * sinphi + self.cosph0 * cosphi_x_coslam
                };
                if (y.abs() - 1.0).abs() < TOL {
                    if y < 0.0 {
                        return Err(outside_domain());
                    }
                    return Ok(self.geodesic_forward(lp));
                }
                let c = y.acos();
                let k = c / c.sin();
                let x = k * cosphi * sinlam;
                let y = if self.mode == Mode::Equatorial {
                    k * sinphi
                } else {
                    k * (self.cosph0 * sinphi - self.sinph0 * cosphi_x_coslam)
                };
                Ok(Coord::new(x, y))
            }
            Mode::NorthPole | Mode::SouthPole => {
                let (phi, coslam) = if self.mode == Mode::NorthPole {
                    (-lp.phi, -coslam)
                } else {
                    (lp.phi, coslam)
                };
                if (phi - FRAC_PI_2).abs() < EPS10 {
                    return Err(outside_domain());
                }
                let y = FRAC_PI_2 + phi;
                Ok(Coord::new(y * sinlam, y * coslam))
            }
        }
    }

    fn ellipsoidal_inverse(&self, xy: Coord, meridian: &MeridianDistance, mp: f64) -> LambdaPhi {
        let s12 = xy.x.hypot(xy.y);
        if s12 < EPS10 {
            return LambdaPhi {
                lambda: 0.0,
                phi: self.phi0,
            };
        }
        match self.mode {
            Mode::Oblique | Mode::Equatorial => {
                let lat1 = self.phi0 / DEG_TO_RAD;
                let lon1 = 0.0;
                // aeqd.cpp measures the azimuth clockwise from north
                let azi1 = xy.x.atan2(xy.y) / DEG_TO_RAD;
                let direct = self.geodesic.direct(lat1, lon1, azi1, s12);
                LambdaPhi {
                    lambda: direct.lon2 * DEG_TO_RAD,
                    phi: direct.lat2 * DEG_TO_RAD,
                }
            }
            Mode::NorthPole => LambdaPhi {
                lambda: xy.x.atan2(-xy.y),
                phi: meridian.inv_mlfn(mp - s12),
            },
            Mode::SouthPole => LambdaPhi {
                lambda: xy.x.atan2(xy.y),
                phi: meridian.inv_mlfn(mp + s12),
            },
        }
    }

    fn spherical_inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let mut c_rh = xy.x.hypot(xy.y);
        if c_rh > PI {
            if c_rh - EPS10 > PI {
                return Err(outside_domain());
            }
            c_rh = PI;
        } else if c_rh < EPS10 {
            return Ok(LambdaPhi {
                lambda: 0.0,
                phi: self.phi0,
            });
        }
        match self.mode {
            Mode::Oblique | Mode::Equatorial => {
                let sinc = c_rh.sin();
                let cosc = c_rh.cos();
                let (phi, x, y) = if self.mode == Mode::Equatorial {
                    let phi = aasin(xy.y * sinc / c_rh)?;
                    (phi, xy.x * sinc, cosc * c_rh)
                } else {
                    let phi = aasin(cosc * self.sinph0 + xy.y * sinc * self.cosph0 / c_rh)?;
                    let y = (cosc - self.sinph0 * phi.sin()) * c_rh;
                    (phi, xy.x * (sinc * self.cosph0), y)
                };
                let lambda = if y == 0.0 { 0.0 } else { x.atan2(y) };
                Ok(LambdaPhi { lambda, phi })
            }
            Mode::NorthPole => Ok(LambdaPhi {
                lambda: xy.x.atan2(-xy.y),
                phi: FRAC_PI_2 - c_rh,
            }),
            Mode::SouthPole => Ok(LambdaPhi {
                lambda: xy.x.atan2(xy.y),
                phi: c_rh - FRAC_PI_2,
            }),
        }
    }
}

impl ClassicProjection for AzimuthalEquidistant {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        match &self.variant {
            Variant::Spherical => self.spherical_forward(lp),
            Variant::Ellipsoidal { meridian, mp } => {
                Ok(self.ellipsoidal_forward(lp, meridian, *mp))
            }
            Variant::Guam {
                es, meridian, m1, ..
            } => {
                let cosphi = lp.phi.cos();
                let sinphi = lp.phi.sin();
                let t = 1.0 / (1.0 - es * sinphi * sinphi).sqrt();
                Ok(Coord::new(
                    lp.lambda * cosphi * t,
                    meridian.mlfn(lp.phi, sinphi, cosphi) - m1
                        + 0.5 * lp.lambda * lp.lambda * cosphi * sinphi * t,
                ))
            }
        }
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        match &self.variant {
            Variant::Spherical => self.spherical_inverse(xy),
            Variant::Ellipsoidal { meridian, mp } => {
                Ok(self.ellipsoidal_inverse(xy, meridian, *mp))
            }
            Variant::Guam {
                e, meridian, m1, ..
            } => {
                let x2 = 0.5 * xy.x * xy.x;
                let mut phi = self.phi0;
                let mut t = 0.0;
                for _ in 0..3 {
                    t = e * phi.sin();
                    t = (1.0 - t * t).sqrt();
                    phi = meridian.inv_mlfn(m1 + xy.y - x2 * phi.tan() * t);
                }
                Ok(LambdaPhi {
                    lambda: xy.x * t / phi.cos(),
                    phi,
                })
            }
        }
    }
}
