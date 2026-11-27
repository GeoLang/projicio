// adapted from PROJ src/projections/nzmg.cpp, src/zpoly1.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::{ClassicProjection, LambdaPhi};
use crate::{Coord, Error};

// nzmg.cpp forces the International 1924 major axis and the grid's own origin whatever the CRS gives
pub(super) const SEMI_MAJOR_AXIS: f64 = 6378388.0;
pub(super) const LONGITUDE_OF_ORIGIN_DEGREES: f64 = 173.0;
pub(super) const LATITUDE_OF_ORIGIN_DEGREES: f64 = -41.0;
pub(super) const FALSE_EASTING: f64 = 2510000.0;
pub(super) const FALSE_NORTHING: f64 = 6023150.0;

const EPSLN: f64 = 1e-10;
const SEC5_TO_RAD: f64 = 0.484813681109536;
const RAD_TO_SEC5: f64 = 2.0626480624709638;

#[derive(Clone, Copy)]
struct Complex {
    r: f64,
    i: f64,
}

const BF: [Complex; 6] = [
    Complex {
        r: 0.7557853228,
        i: 0.0,
    },
    Complex {
        r: 0.249204646,
        i: 0.003371507,
    },
    Complex {
        r: -0.001541739,
        i: 0.041058560,
    },
    Complex {
        r: -0.10162907,
        i: 0.01727609,
    },
    Complex {
        r: -0.26623489,
        i: -0.36249218,
    },
    Complex {
        r: -0.6870983,
        i: -1.1651967,
    },
];

const TPSI: [f64; 10] = [
    0.6399175073,
    -0.1358797613,
    0.063294409,
    -0.02526853,
    0.0117879,
    -0.0055161,
    0.0026906,
    -0.001333,
    0.00067,
    -0.00034,
];

const TPHI: [f64; 9] = [
    1.5627014243,
    0.5185406398,
    -0.03333098,
    -0.1052906,
    -0.0368594,
    0.007317,
    0.01220,
    0.00394,
    -0.0013,
];

// zpoly1.cpp's coefficients run from C_1 to C_n, C_0 is zero
fn zpoly1(z: Complex, c: &[Complex]) -> Complex {
    let mut a = c[c.len() - 1];
    for coefficient in c[..c.len() - 1].iter().rev() {
        let t = a.r;
        a.r = coefficient.r + z.r * t - z.i * a.i;
        a.i = coefficient.i + z.r * a.i + z.i * t;
    }
    let t = a.r;
    Complex {
        r: z.r * t - z.i * a.i,
        i: z.r * a.i + z.i * t,
    }
}

struct ValueAndDerivative {
    value: Complex,
    derivative: Complex,
}

fn zpolyd1(z: Complex, c: &[Complex]) -> ValueAndDerivative {
    let mut a = c[c.len() - 1];
    let mut b = a;
    let mut first = true;
    for coefficient in c[..c.len() - 1].iter().rev() {
        if first {
            first = false;
        } else {
            let t = b.r;
            b.r = a.r + z.r * t - z.i * b.i;
            b.i = a.i + z.r * b.i + z.i * t;
        }
        let t = a.r;
        a.r = coefficient.r + z.r * t - z.i * a.i;
        a.i = coefficient.i + z.r * a.i + z.i * t;
    }
    let t = b.r;
    b.r = a.r + z.r * t - z.i * b.i;
    b.i = a.i + z.r * b.i + z.i * t;
    let t = a.r;
    a.r = z.r * t - z.i * a.i;
    a.i = z.r * a.i + z.i * t;
    ValueAndDerivative {
        value: a,
        derivative: b,
    }
}

pub(super) struct NewZealandMapGrid {
    phi0: f64,
}

impl NewZealandMapGrid {
    pub(super) fn new() -> Self {
        Self {
            phi0: LATITUDE_OF_ORIGIN_DEGREES.to_radians(),
        }
    }
}

impl ClassicProjection for NewZealandMapGrid {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let phi = (lp.phi - self.phi0) * RAD_TO_SEC5;
        let mut r = TPSI[TPSI.len() - 1];
        for &coefficient in TPSI[..TPSI.len() - 1].iter().rev() {
            r = coefficient + phi * r;
        }
        r *= phi;
        let p = zpoly1(Complex { r, i: lp.lambda }, &BF);
        Ok(Coord::new(p.i, p.r))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let mut p = Complex { r: xy.y, i: xy.x };
        for _ in 0..20 {
            let ValueAndDerivative {
                value: mut f,
                derivative: fp,
            } = zpolyd1(p, &BF);
            f.r -= xy.y;
            f.i -= xy.x;
            let den = fp.r * fp.r + fp.i * fp.i;
            let dp_r = -(f.r * fp.r + f.i * fp.i) / den;
            let dp_i = -(f.i * fp.r - f.r * fp.i) / den;
            p.r += dp_r;
            p.i += dp_i;
            if dp_r.abs() + dp_i.abs() <= EPSLN {
                let mut phi = TPHI[TPHI.len() - 1];
                for &coefficient in TPHI[..TPHI.len() - 1].iter().rev() {
                    phi = coefficient + p.r * phi;
                }
                return Ok(LambdaPhi {
                    lambda: p.i,
                    phi: self.phi0 + p.r * phi * SEC5_TO_RAD,
                });
            }
        }
        Err(Error::ProjectionError(
            "New Zealand Map Grid inverse did not converge".into(),
        ))
    }
}
