// adapted from PROJ src/latitudes.cpp, src/mlfn.cpp, src/tsfn.cpp, src/msfn.cpp, src/phi2.cpp, src/adjlon.cpp, src/aasincos.cpp, src/generic_inverse.cpp, src/fwd.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::{FRAC_PI_2, PI};

use super::{EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

pub(super) const AUXILIARY_ORDER: usize = 6;

#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub(super) enum AuxiliaryLatitude {
    Geographic = 0,
    Rectifying = 3,
    Conformal = 4,
    Authalic = 5,
}

const AUXILIARY_LATITUDE_COUNT: usize = 6;

// PROJ generates this table with GeographicLib's auxlat.mac, only the conversions PROJ uses are present
#[rustfmt::skip]
const AUXILIARY_COEFFICIENTS: [f64; 150] = [
    // C[phi,mu], even coefficients only
    3.0 / 2.0, -27.0 / 32.0, 269.0 / 512.0, 21.0 / 16.0, -55.0 / 32.0,
    6759.0 / 4096.0, 151.0 / 96.0, -417.0 / 128.0, 1097.0 / 512.0,
    -15543.0 / 2560.0, 8011.0 / 2560.0, 293393.0 / 61440.0,
    // C[phi,chi]
    2.0, -2.0 / 3.0, -2.0, 116.0 / 45.0, 26.0 / 45.0, -2854.0 / 675.0,
    7.0 / 3.0, -8.0 / 5.0, -227.0 / 45.0, 2704.0 / 315.0, 2323.0 / 945.0,
    56.0 / 15.0, -136.0 / 35.0, -1262.0 / 105.0, 73814.0 / 2835.0,
    4279.0 / 630.0, -332.0 / 35.0, -399572.0 / 14175.0, 4174.0 / 315.0,
    -144838.0 / 6237.0, 601676.0 / 22275.0,
    // C[phi,xi]
    4.0 / 3.0, 4.0 / 45.0, -16.0 / 35.0, -2582.0 / 14175.0,
    60136.0 / 467775.0, 28112932.0 / 212837625.0, 46.0 / 45.0,
    152.0 / 945.0, -11966.0 / 14175.0, -21016.0 / 51975.0,
    251310128.0 / 638512875.0, 3044.0 / 2835.0, 3802.0 / 14175.0,
    -94388.0 / 66825.0, -8797648.0 / 10945935.0, 6059.0 / 4725.0,
    41072.0 / 93555.0, -1472637812.0 / 638512875.0, 768272.0 / 467775.0,
    455935736.0 / 638512875.0, 4210684958.0 / 1915538625.0,
    // C[mu,phi], even coefficients only
    -3.0 / 2.0, 9.0 / 16.0, -3.0 / 32.0, 15.0 / 16.0, -15.0 / 32.0,
    135.0 / 2048.0, -35.0 / 48.0, 105.0 / 256.0, 315.0 / 512.0,
    -189.0 / 512.0, -693.0 / 1280.0, 1001.0 / 2048.0,
    // C[mu,chi]
    1.0 / 2.0, -2.0 / 3.0, 5.0 / 16.0, 41.0 / 180.0, -127.0 / 288.0,
    7891.0 / 37800.0, 13.0 / 48.0, -3.0 / 5.0, 557.0 / 1440.0,
    281.0 / 630.0, -1983433.0 / 1935360.0, 61.0 / 240.0, -103.0 / 140.0,
    15061.0 / 26880.0, 167603.0 / 181440.0, 49561.0 / 161280.0,
    -179.0 / 168.0, 6601661.0 / 7257600.0, 34729.0 / 80640.0,
    -3418889.0 / 1995840.0, 212378941.0 / 319334400.0,
    // C[chi,phi]
    -2.0, 2.0 / 3.0, 4.0 / 3.0, -82.0 / 45.0, 32.0 / 45.0, 4642.0 / 4725.0,
    5.0 / 3.0, -16.0 / 15.0, -13.0 / 9.0, 904.0 / 315.0, -1522.0 / 945.0,
    -26.0 / 15.0, 34.0 / 21.0, 8.0 / 5.0, -12686.0 / 2835.0, 1237.0 / 630.0,
    -12.0 / 5.0, -24832.0 / 14175.0, -734.0 / 315.0, 109598.0 / 31185.0,
    444337.0 / 155925.0,
    // C[chi,mu]
    -1.0 / 2.0, 2.0 / 3.0, -37.0 / 96.0, 1.0 / 360.0, 81.0 / 512.0,
    -96199.0 / 604800.0, -1.0 / 48.0, -1.0 / 15.0, 437.0 / 1440.0,
    -46.0 / 105.0, 1118711.0 / 3870720.0, -17.0 / 480.0, 37.0 / 840.0,
    209.0 / 4480.0, -5569.0 / 90720.0, -4397.0 / 161280.0, 11.0 / 504.0,
    830251.0 / 7257600.0, -4583.0 / 161280.0, 108847.0 / 3991680.0,
    -20648693.0 / 638668800.0,
    // C[xi,phi]
    -4.0 / 3.0, -4.0 / 45.0, 88.0 / 315.0, 538.0 / 4725.0,
    20824.0 / 467775.0, -44732.0 / 2837835.0, 34.0 / 45.0, 8.0 / 105.0,
    -2482.0 / 14175.0, -37192.0 / 467775.0, -12467764.0 / 212837625.0,
    -1532.0 / 2835.0, -898.0 / 14175.0, 54968.0 / 467775.0,
    100320856.0 / 1915538625.0, 6007.0 / 14175.0, 24496.0 / 467775.0,
    -5884124.0 / 70945875.0, -23356.0 / 66825.0, -839792.0 / 19348875.0,
    570284222.0 / 1915538625.0,
];

#[rustfmt::skip]
const AUXILIARY_OFFSETS: [usize; AUXILIARY_LATITUDE_COUNT * AUXILIARY_LATITUDE_COUNT + 1] = [
    0,   0,   0,   0,   12,  33,  54,  54,  54,  54,  54,  54, 54,
    54,  54,  54,  54,  54,  54,  66,  66,  66,  66,  87,  87, 108,
    108, 108, 129, 129, 129, 150, 150, 150, 150, 150, 150,
];

pub(super) fn auxiliary_coefficients(
    n: f64,
    from: AuxiliaryLatitude,
    to: AuxiliaryLatitude,
) -> [f64; AUXILIARY_ORDER] {
    let index = AUXILIARY_LATITUDE_COUNT * to as usize + from as usize;
    let mut offset = AUXILIARY_OFFSETS[index];
    assert!(
        offset != AUXILIARY_OFFSETS[index + 1],
        "PROJ has no series between these auxiliary latitudes"
    );
    let mut coefficients = [0.0; AUXILIARY_ORDER];
    let mut power = n;
    let n2 = n * n;
    let even_only = from <= AuxiliaryLatitude::Rectifying && to <= AuxiliaryLatitude::Rectifying;
    for (order, coefficient) in coefficients.iter_mut().enumerate() {
        let degree = if even_only {
            (AUXILIARY_ORDER - order - 1) / 2
        } else {
            AUXILIARY_ORDER - order - 1
        };
        let argument = if even_only { n2 } else { n };
        *coefficient = power * polyval(argument, &AUXILIARY_COEFFICIENTS[offset..=offset + degree]);
        offset += degree + 1;
        power *= n;
    }
    coefficients
}

pub(super) fn polyval(x: f64, p: &[f64]) -> f64 {
    p.iter()
        .rev()
        .fold(0.0, |y, &coefficient| y * x + coefficient)
}

pub(super) fn clenshaw(sin_zeta: f64, cos_zeta: f64, f: &[f64]) -> f64 {
    let (mut u0, mut u1) = (0.0, 0.0);
    let x = 2.0 * (cos_zeta - sin_zeta) * (cos_zeta + sin_zeta);
    for &coefficient in f.iter().rev() {
        let t = x * u0 - u1 + coefficient;
        u1 = u0;
        u0 = t;
    }
    2.0 * sin_zeta * cos_zeta * u0
}

pub(super) fn auxiliary_convert(zeta: f64, f: &[f64]) -> f64 {
    auxiliary_convert_with_sine(zeta, zeta.sin(), zeta.cos(), f)
}

pub(super) fn auxiliary_convert_with_sine(
    zeta: f64,
    sin_zeta: f64,
    cos_zeta: f64,
    f: &[f64],
) -> f64 {
    zeta + clenshaw(sin_zeta, cos_zeta, f)
}

pub(super) fn rectifying_radius(n: f64) -> f64 {
    const COEFFICIENTS: [f64; 4] = [1.0, 1.0 / 4.0, 1.0 / 64.0, 1.0 / 256.0];
    polyval(n * n, &COEFFICIENTS) / (1.0 + n)
}

#[derive(Clone)]
pub(super) struct MeridianDistance {
    radius: f64,
    to_rectifying: [f64; AUXILIARY_ORDER],
    to_geographic: [f64; AUXILIARY_ORDER],
}

impl MeridianDistance {
    pub(super) fn new(n: f64) -> Self {
        Self {
            radius: rectifying_radius(n),
            to_rectifying: auxiliary_coefficients(
                n,
                AuxiliaryLatitude::Geographic,
                AuxiliaryLatitude::Rectifying,
            ),
            to_geographic: auxiliary_coefficients(
                n,
                AuxiliaryLatitude::Rectifying,
                AuxiliaryLatitude::Geographic,
            ),
        }
    }

    pub(super) fn mlfn(&self, phi: f64, sin_phi: f64, cos_phi: f64) -> f64 {
        self.radius * auxiliary_convert_with_sine(phi, sin_phi, cos_phi, &self.to_rectifying)
    }

    pub(super) fn inv_mlfn(&self, mu: f64) -> f64 {
        auxiliary_convert(mu / self.radius, &self.to_geographic)
    }
}

pub(super) fn tsfn(phi: f64, sin_phi: f64, e: f64) -> f64 {
    let cos_phi = phi.cos();
    (e * (e * sin_phi).atanh()).exp()
        * if sin_phi > 0.0 {
            cos_phi / (1.0 + sin_phi)
        } else {
            (1.0 - sin_phi) / cos_phi
        }
}

pub(super) fn msfn(sin_phi: f64, cos_phi: f64, es: f64) -> f64 {
    cos_phi / (1.0 - es * sin_phi * sin_phi).sqrt()
}

pub(super) fn sinhpsi2tanphi(taup: f64, e: f64) -> Result<f64, Error> {
    const ITERATIONS: usize = 5;
    let rooteps = f64::EPSILON.sqrt();
    let tol = rooteps / 10.0;
    let tmax = 2.0 / rooteps;
    let e2m = 1.0 - e * e;
    let stol = tol * taup.abs().max(1.0);
    let mut tau = if taup.abs() > 70.0 {
        taup * (e * e.atanh()).exp()
    } else {
        taup / e2m
    };
    if tau.is_nan() || tau.abs() >= tmax {
        return Ok(tau);
    }
    for _ in 0..ITERATIONS {
        let tau1 = (1.0 + tau * tau).sqrt();
        let sig = (e * (e * tau / tau1).atanh()).sinh();
        let taupa = (1.0 + sig * sig).sqrt() * tau - sig * tau1;
        let dtau = (taup - taupa) * (1.0 + e2m * (tau * tau))
            / (e2m * tau1 * (1.0 + taupa * taupa).sqrt());
        tau += dtau;
        if dtau.is_nan() || dtau.abs() < stol {
            return Ok(tau);
        }
    }
    Err(Error::ProjectionError(
        "latitude from isometric latitude did not converge".into(),
    ))
}

pub(super) fn phi2(ts0: f64, e: f64) -> Result<f64, Error> {
    Ok(sinhpsi2tanphi((1.0 / ts0 - ts0) / 2.0, e)?.atan())
}

pub(super) fn authalic_q(sin_phi: f64, shape: &EllipsoidConstants) -> f64 {
    const EPSILON: f64 = 1e-7;
    if shape.e < EPSILON {
        return 2.0 * sin_phi;
    }
    let e_sin_phi = shape.e * sin_phi;
    let one_minus_e_sin_phi_squared = 1.0 - e_sin_phi * e_sin_phi;
    if one_minus_e_sin_phi_squared == 0.0 {
        return f64::INFINITY;
    }
    shape.one_es * (sin_phi / one_minus_e_sin_phi_squared + e_sin_phi.atanh() / shape.e)
}

fn authalic_series_valid(n: f64) -> bool {
    n.abs() < 0.01
}

#[derive(Clone)]
pub(super) struct AuthalicLatitude {
    to_geographic: [f64; AUXILIARY_ORDER],
    to_authalic: Option<[f64; AUXILIARY_ORDER]>,
    pub(super) qp: f64,
}

impl AuthalicLatitude {
    pub(super) fn new(shape: &EllipsoidConstants, qp: f64) -> Self {
        let to_authalic = authalic_series_valid(shape.n).then(|| {
            auxiliary_coefficients(
                shape.n,
                AuxiliaryLatitude::Geographic,
                AuxiliaryLatitude::Authalic,
            )
        });
        Self {
            to_geographic: auxiliary_coefficients(
                shape.n,
                AuxiliaryLatitude::Authalic,
                AuxiliaryLatitude::Geographic,
            ),
            to_authalic,
            qp,
        }
    }

    pub(super) fn authalic(
        &self,
        phi: f64,
        sin_phi: f64,
        cos_phi: f64,
        shape: &EllipsoidConstants,
    ) -> f64 {
        if let Some(to_authalic) = &self.to_authalic {
            return auxiliary_convert_with_sine(phi, sin_phi, cos_phi, to_authalic);
        }
        let q = authalic_q(sin_phi, shape);
        let ratio = (q / self.qp).clamp(-1.0, 1.0);
        ratio.asin()
    }

    pub(super) fn geographic(&self, beta: f64, shape: &EllipsoidConstants) -> f64 {
        let mut phi = auxiliary_convert(beta, &self.to_geographic);
        if self.to_authalic.is_some() {
            return phi;
        }
        let q = beta.sin() * self.qp;
        let q_div_one_minus_es = q / shape.one_es;
        for _ in 0..10 {
            let sin_phi = phi.sin();
            let cos_phi = phi.cos();
            let one_minus_es_sin2phi = 1.0 - shape.es * (sin_phi * sin_phi);
            let dphi = (one_minus_es_sin2phi * one_minus_es_sin2phi) / (2.0 * cos_phi)
                * (q_div_one_minus_es
                    - sin_phi / one_minus_es_sin2phi
                    - (shape.e * sin_phi).atanh() / shape.e);
            if dphi.is_nan() || dphi.abs() < 1e-15 {
                break;
            }
            phi += dphi;
        }
        phi
    }
}

pub(crate) fn adjlon(longitude: f64) -> f64 {
    if longitude.abs() < PI + 1e-12 {
        return longitude;
    }
    let mut longitude = longitude + PI;
    longitude -= 2.0 * PI * (longitude / (2.0 * PI)).floor();
    longitude - PI
}

const PJ_EPS_LAT: f64 = 1e-12;

pub(crate) fn prepare_angular_input(lambda: f64, phi: f64) -> Result<LambdaPhi, Error> {
    // fwd.cpp's fwd_prepare runs these checks before every step that takes angular input
    if phi.is_nan() || phi.abs() - FRAC_PI_2 > PJ_EPS_LAT {
        return Err(Error::InvalidCoordinate(format!(
            "latitude {} out of range",
            phi.to_degrees()
        )));
    }
    if lambda.is_nan() || lambda.abs() > 10.0 {
        return Err(Error::InvalidCoordinate(format!(
            "longitude {} out of range",
            lambda.to_degrees()
        )));
    }
    Ok(LambdaPhi {
        lambda: adjlon(lambda),
        phi: phi.clamp(-FRAC_PI_2, FRAC_PI_2),
    })
}

const ONE_TOL: f64 = 1.000_000_000_000_01;

fn outside_domain() -> Error {
    Error::ProjectionError("coordinate outside the projection domain".into())
}

pub(super) fn aasin(v: f64) -> Result<f64, Error> {
    let av = v.abs();
    if av >= 1.0 {
        if av > ONE_TOL {
            return Err(outside_domain());
        }
        return Ok(if v < 0.0 { -FRAC_PI_2 } else { FRAC_PI_2 });
    }
    Ok(v.asin())
}

struct InverseJacobian {
    lambda_x: f64,
    lambda_y: f64,
    phi_x: f64,
    phi_y: f64,
}

pub(super) fn generic_inverse_2d(
    xy: Coord,
    initial: LambdaPhi,
    delta_xy_tolerance: f64,
    forward: impl Fn(LambdaPhi) -> Coord,
) -> Result<LambdaPhi, Error> {
    let mut lp = initial;
    let mut jacobian = InverseJacobian {
        lambda_x: 0.0,
        lambda_y: 0.0,
        phi_x: 0.0,
        phi_y: 0.0,
    };
    for iteration in 0..15 {
        let xy_approx = forward(lp);
        let delta_x = xy_approx.x - xy.x;
        let delta_y = xy_approx.y - xy.y;
        if delta_x.abs() < delta_xy_tolerance && delta_y.abs() < delta_xy_tolerance {
            return Ok(lp);
        }

        if iteration == 0 || delta_x.abs() > 1e-6 || delta_y.abs() > 1e-6 {
            let d_lambda = if lp.lambda > 0.0 { -1e-6 } else { 1e-6 };
            let xy2 = forward(LambdaPhi {
                lambda: lp.lambda + d_lambda,
                phi: lp.phi,
            });
            let deriv_x_lambda = (xy2.x - xy_approx.x) / d_lambda;
            let deriv_y_lambda = (xy2.y - xy_approx.y) / d_lambda;

            let d_phi = if lp.phi > 0.0 { -1e-6 } else { 1e-6 };
            let xy2 = forward(LambdaPhi {
                lambda: lp.lambda,
                phi: lp.phi + d_phi,
            });
            let deriv_x_phi = (xy2.x - xy_approx.x) / d_phi;
            let deriv_y_phi = (xy2.y - xy_approx.y) / d_phi;

            let det = deriv_x_lambda * deriv_y_phi - deriv_x_phi * deriv_y_lambda;
            if det != 0.0 {
                jacobian = InverseJacobian {
                    lambda_x: deriv_y_phi / det,
                    lambda_y: -deriv_x_phi / det,
                    phi_x: -deriv_y_lambda / det,
                    phi_y: deriv_x_lambda / det,
                };
            }
        }

        let delta_lambda =
            (delta_x * jacobian.lambda_x + delta_y * jacobian.lambda_y).clamp(-0.3, 0.3);
        lp.lambda = (lp.lambda - delta_lambda).clamp(-PI, PI);

        let delta_phi = (delta_x * jacobian.phi_x + delta_y * jacobian.phi_y).clamp(-0.3, 0.3);
        lp.phi = (lp.phi - delta_phi).clamp(-FRAC_PI_2, FRAC_PI_2);
    }
    Err(outside_domain())
}
