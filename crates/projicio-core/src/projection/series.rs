// ported from proj-rust (https://github.com/pka/proj-rust), MIT OR Apache-2.0

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
