// adapted from PROJ src/projections/tmerc.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::FRAC_PI_2;

use super::math::{
    AUXILIARY_ORDER, AuxiliaryLatitude, auxiliary_coefficients, auxiliary_convert,
    auxiliary_convert_with_sine, rectifying_radius,
};
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const EPS10: f64 = 1e-10;

// tmerc.cpp refuses easting beyond 150 degrees of arc in the normalized plane
const MAX_NORMALIZED_EASTING: f64 = 2.623395162778;

pub(super) enum TransverseMercator {
    PoderEngsager(PoderEngsager),
    Spherical(Spherical),
}

pub(super) struct PoderEngsager {
    qn: f64,
    zb: f64,
    cgb: [f64; AUXILIARY_ORDER],
    cbg: [f64; AUXILIARY_ORDER],
    utg: [f64; AUXILIARY_ORDER],
    gtu: [f64; AUXILIARY_ORDER],
}

pub(super) struct Spherical {
    esp: f64,
    ml0: f64,
    phi0: f64,
}

impl TransverseMercator {
    pub(super) fn new(shape: &EllipsoidConstants, phi0: f64, k0: f64) -> Self {
        if shape.es == 0.0 {
            return Self::Spherical(Spherical {
                esp: k0,
                ml0: 0.5 * k0,
                phi0,
            });
        }
        Self::PoderEngsager(PoderEngsager::new(shape, phi0, k0))
    }
}

impl ClassicProjection for TransverseMercator {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        match self {
            Self::PoderEngsager(exact) => exact.forward(lp),
            Self::Spherical(spherical) => spherical.forward(lp),
        }
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        match self {
            Self::PoderEngsager(exact) => exact.inverse(xy),
            Self::Spherical(spherical) => spherical.inverse(xy),
        }
    }
}

fn outside_domain() -> Error {
    Error::ProjectionError("coordinate outside the transverse Mercator domain".into())
}

struct ComplexClenshaw {
    real: f64,
    imaginary: f64,
}

fn clen_s(
    a: &[f64],
    sin_arg_r: f64,
    cos_arg_r: f64,
    sinh_arg_i: f64,
    cosh_arg_i: f64,
) -> ComplexClenshaw {
    let r = 2.0 * cos_arg_r * cosh_arg_i;
    let i = -2.0 * sin_arg_r * sinh_arg_i;
    let (mut hr1, mut hi1, mut hi) = (0.0, 0.0, 0.0);
    let mut coefficients = a.iter().rev();
    let mut hr = *coefficients.next().expect("series has coefficients");
    for &coefficient in coefficients {
        let hr2 = hr1;
        let hi2 = hi1;
        hr1 = hr;
        hi1 = hi;
        hr = -hr2 + r * hr1 - i * hi1 + coefficient;
        hi = -hi2 + i * hr1 + r * hi1;
    }
    let r = sin_arg_r * cosh_arg_i;
    let i = cos_arg_r * sinh_arg_i;
    ComplexClenshaw {
        real: r * hr - i * hi,
        imaginary: r * hi + i * hr,
    }
}

impl PoderEngsager {
    fn new(shape: &EllipsoidConstants, phi0: f64, k0: f64) -> Self {
        let n = shape.n;
        let cgb = auxiliary_coefficients(
            n,
            AuxiliaryLatitude::Conformal,
            AuxiliaryLatitude::Geographic,
        );
        let cbg = auxiliary_coefficients(
            n,
            AuxiliaryLatitude::Geographic,
            AuxiliaryLatitude::Conformal,
        );
        let qn = k0 * rectifying_radius(n);
        let utg = auxiliary_coefficients(
            n,
            AuxiliaryLatitude::Rectifying,
            AuxiliaryLatitude::Conformal,
        );
        let gtu = auxiliary_coefficients(
            n,
            AuxiliaryLatitude::Conformal,
            AuxiliaryLatitude::Rectifying,
        );
        let z = auxiliary_convert(phi0, &cbg);
        let zb = -qn * auxiliary_convert(z, &gtu);
        Self {
            qn,
            zb,
            cgb,
            cbg,
            utg,
            gtu,
        }
    }

    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let mut cn = auxiliary_convert(lp.phi, &self.cbg);
        let sin_cn = cn.sin();
        let cos_cn = cn.cos();
        let sin_ce = lp.lambda.sin();
        let cos_ce = lp.lambda.cos();

        let cos_cn_cos_ce = cos_cn * cos_ce;
        cn = sin_cn.atan2(cos_cn_cos_ce);

        let inv_denom_tan_ce = 1.0 / sin_cn.hypot(cos_cn_cos_ce);
        let tan_ce = sin_ce * cos_cn * inv_denom_tan_ce;

        let mut ce = tan_ce.asinh();

        let two_inv_denom_tan_ce = 2.0 * inv_denom_tan_ce;
        let two_inv_denom_tan_ce_square = two_inv_denom_tan_ce * inv_denom_tan_ce;
        let tmp_r = cos_cn_cos_ce * two_inv_denom_tan_ce_square;
        let sin_arg_r = sin_cn * tmp_r;
        let cos_arg_r = cos_cn_cos_ce * tmp_r - 1.0;

        let sinh_arg_i = tan_ce * two_inv_denom_tan_ce;
        let cosh_arg_i = two_inv_denom_tan_ce_square - 1.0;

        let correction = clen_s(&self.gtu, sin_arg_r, cos_arg_r, sinh_arg_i, cosh_arg_i);
        cn += correction.real;
        ce += correction.imaginary;
        if ce.abs() > MAX_NORMALIZED_EASTING {
            return Err(outside_domain());
        }
        Ok(Coord::new(self.qn * ce, self.qn * cn + self.zb))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let mut cn = (xy.y - self.zb) / self.qn;
        let mut ce = xy.x / self.qn;
        if ce.abs() > MAX_NORMALIZED_EASTING {
            return Err(outside_domain());
        }
        let sin_arg_r = (2.0 * cn).sin();
        let cos_arg_r = (2.0 * cn).cos();

        let exp_2_ce = (2.0 * ce).exp();
        let half_inv_exp_2_ce = 0.5 / exp_2_ce;
        let sinh_arg_i = 0.5 * exp_2_ce - half_inv_exp_2_ce;
        let cosh_arg_i = 0.5 * exp_2_ce + half_inv_exp_2_ce;

        let correction = clen_s(&self.utg, sin_arg_r, cos_arg_r, sinh_arg_i, cosh_arg_i);
        cn += correction.real;
        ce += correction.imaginary;

        let sin_cn = cn.sin();
        let cos_cn = cn.cos();

        let sinh_ce = ce.sinh();
        ce = sinh_ce.atan2(cos_cn);
        let modulus_ce = sinh_ce.hypot(cos_cn);
        let rr = sin_cn.hypot(modulus_ce);
        cn = sin_cn.atan2(modulus_ce);

        let phi = auxiliary_convert_with_sine(cn, sin_cn / rr, modulus_ce / rr, &self.cgb);
        Ok(LambdaPhi { lambda: ce, phi })
    }
}

impl Spherical {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let cosphi = lp.phi.cos();
        let mut b = cosphi * lp.lambda.sin();
        if (b.abs() - 1.0).abs() <= EPS10 {
            return Err(outside_domain());
        }

        let x = self.ml0 * ((1.0 + b) / (1.0 - b)).ln();
        let mut y;
        if cosphi == 1.0 {
            y = if lp.lambda < -FRAC_PI_2 || lp.lambda > FRAC_PI_2 {
                std::f64::consts::PI
            } else {
                0.0
            };
        } else {
            y = cosphi * lp.lambda.cos() / (1.0 - b * b).sqrt();
            b = y.abs();
            if b >= 1.0 {
                if (b - 1.0) > EPS10 {
                    return Err(outside_domain());
                }
                y = 0.0;
            } else {
                y = y.acos();
            }
        }

        if lp.phi < 0.0 {
            y = -y;
        }
        y = self.esp * (y - self.phi0);
        Ok(Coord::new(x, y))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let mut h = (xy.x / self.esp).exp();
        if h == 0.0 {
            return Err(outside_domain());
        }
        let g = 0.5 * (h - 1.0 / h);
        let d = self.phi0 + xy.y / self.esp;
        h = d.cos();
        let phi = ((1.0 - h * h) / (1.0 + g * g)).sqrt().asin().copysign(d);
        let lambda = if g != 0.0 || h != 0.0 {
            g.atan2(h)
        } else {
            0.0
        };
        Ok(LambdaPhi { lambda, phi })
    }
}
