// ported from proj-rust (https://github.com/roteiro-gis/proj-rust), MIT OR Apache-2.0

use crate::Error;

const ECCENTRICITY_EPSILON: f64 = 1e-15;

/// Run the fixed-point iteration `step` from `initial` until successive iterates
/// differ by less than `tolerance`. Exhausting `max_iterations` is an error
/// rather than a silently unconverged answer.
pub(super) fn converge(
    context: &str,
    initial: f64,
    max_iterations: usize,
    tolerance: f64,
    mut step: impl FnMut(f64) -> f64,
) -> Result<f64, Error> {
    let mut value = initial;
    for _ in 0..max_iterations {
        let next = step(value);
        if (next - value).abs() < tolerance {
            return Ok(next);
        }
        value = next;
    }
    Err(Error::ProjectionError(format!(
        "{context} did not converge in {max_iterations} iterations"
    )))
}

pub(super) fn normalize_longitude(lon: f64) -> f64 {
    let normalized =
        (lon + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI;

    if normalized == -std::f64::consts::PI && lon > 0.0 {
        std::f64::consts::PI
    } else {
        normalized
    }
}

/// Geodetic latitude from the conformal parameter
/// `t = tan(π/4 − φ/2) / ((1 − e·sinφ)/(1 + e·sinφ))^(e/2)`, EPSG Guidance Note 7-2.
pub(super) fn latitude_from_conformal_t(context: &str, t: f64, e: f64) -> Result<f64, Error> {
    let initial = std::f64::consts::FRAC_PI_2 - 2.0 * t.atan();
    if e.abs() < ECCENTRICITY_EPSILON {
        return Ok(initial);
    }
    converge(context, initial, 15, 1e-14, |lat| {
        let e_sin = e * lat.sin();
        std::f64::consts::FRAC_PI_2
            - 2.0 * (t * ((1.0 - e_sin) / (1.0 + e_sin)).powf(e / 2.0)).atan()
    })
}

/// Order of the meridional-arc expansion in the third flattening, full double
/// precision for a flattening up to 1/150.
const MERIDIAN_ARC_ORDER: usize = 6;

/// Coefficients for the meridional-arc series in the third flattening (Karney,
/// arXiv:2212.05818). `[0]` is the rectifying-radius multiplier and `[1..=6]`
/// convert latitude to the rectifying latitude.
pub(super) fn meridian_arc_coefficients(e2: f64) -> [f64; MERIDIAN_ARC_ORDER + 1] {
    let one_minus_f = (1.0 - e2).sqrt();
    let n = (1.0 - one_minus_f) / (1.0 + one_minus_f);

    const COEFFICIENTS_RADIUS: [f64; 4] = [1.0, 1.0 / 4.0, 1.0 / 64.0, 1.0 / 256.0];
    const COEFFICIENTS_RECTIFYING: [f64; 12] = [
        -3.0 / 2.0,
        9.0 / 16.0,
        -3.0 / 32.0,
        15.0 / 16.0,
        -15.0 / 32.0,
        135.0 / 2048.0,
        -35.0 / 48.0,
        105.0 / 256.0,
        315.0 / 512.0,
        -189.0 / 512.0,
        -693.0 / 1280.0,
        1001.0 / 2048.0,
    ];

    fn polynomial(x: f64, coefficients: &[f64]) -> f64 {
        coefficients.iter().rev().fold(0.0, |y, &c| y * x + c)
    }

    let n2 = n * n;
    let mut en = [0.0; MERIDIAN_ARC_ORDER + 1];
    en[0] = polynomial(n2, &COEFFICIENTS_RADIUS[..=MERIDIAN_ARC_ORDER / 2]) / (1.0 + n);
    let mut power = n;
    let mut offset = 0;
    for term in 0..MERIDIAN_ARC_ORDER {
        let last = (MERIDIAN_ARC_ORDER - term - 1) / 2;
        en[term + 1] = power * polynomial(n2, &COEFFICIENTS_RECTIFYING[offset..=offset + last]);
        power *= n;
        offset += last + 1;
    }
    en
}

/// Evaluate `sum(c[k] * sin((2k+2)·zeta))` by Clenshaw summation.
fn clenshaw(sin_zeta: f64, cos_zeta: f64, coefficients: &[f64]) -> f64 {
    let x = 2.0 * (cos_zeta - sin_zeta) * (cos_zeta + sin_zeta);
    let (mut u0, mut u1) = (0.0, 0.0);
    for &coefficient in coefficients.iter().rev() {
        let t = x * u0 - u1 + coefficient;
        u1 = u0;
        u0 = t;
    }
    2.0 * sin_zeta * cos_zeta * u0
}

/// Meridional arc length from the equator to `phi`, in semi-major-axis units.
pub(super) fn meridian_arc(
    phi: f64,
    sin_phi: f64,
    cos_phi: f64,
    en: &[f64; MERIDIAN_ARC_ORDER + 1],
) -> f64 {
    en[0] * (phi + clenshaw(sin_phi, cos_phi, &en[1..=MERIDIAN_ARC_ORDER]))
}

/// Authalic `q` for geodetic latitude `lat` (Snyder 3-12).
pub(super) fn authalic_q(lat: f64, e2: f64) -> f64 {
    if e2.abs() < ECCENTRICITY_EPSILON {
        return 2.0 * lat.sin();
    }

    let e = e2.sqrt();
    let sin_lat = lat.sin();
    let e_sin = e * sin_lat;
    (1.0 - e2)
        * (sin_lat / (1.0 - e2 * sin_lat * sin_lat)
            - (1.0 / (2.0 * e)) * ((1.0 - e_sin) / (1.0 + e_sin)).ln())
}

/// Geodetic latitude from the authalic latitude (Snyder 3-18 series).
pub(super) fn geodetic_from_authalic(beta: f64, e2: f64) -> f64 {
    if e2.abs() < ECCENTRICITY_EPSILON {
        return beta;
    }

    let e4 = e2 * e2;
    let e6 = e4 * e2;
    beta + (e2 / 3.0 + 31.0 * e4 / 180.0 + 517.0 * e6 / 5040.0) * (2.0 * beta).sin()
        + (23.0 * e4 / 360.0 + 251.0 * e6 / 3780.0) * (4.0 * beta).sin()
        + (761.0 * e6 / 45360.0) * (6.0 * beta).sin()
}
