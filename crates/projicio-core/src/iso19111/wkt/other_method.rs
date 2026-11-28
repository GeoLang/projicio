// adapted from PROJ src/iso19111/operation/conversion.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::{FRAC_PI_2, PI};

use super::methods::{
    METHOD_LAMBERT_CONIC_CONFORMAL_1SP, METHOD_LAMBERT_CONIC_CONFORMAL_2SP,
    METHOD_MERCATOR_VARIANT_A, METHOD_MERCATOR_VARIANT_B, PARAMETER_FALSE_EASTING,
    PARAMETER_FALSE_NORTHING, PARAMETER_LATITUDE_1ST_STD_PARALLEL,
    PARAMETER_LATITUDE_OF_NATURAL_ORIGIN, PARAMETER_LONGITUDE_OF_NATURAL_ORIGIN,
    PARAMETER_SCALE_FACTOR_AT_NATURAL_ORIGIN, mapping_by_code, method_epsg_code, parameter_measure,
    parameter_si_value,
};
use crate::iso19111::common::{Measure, UnitOfMeasure};
use crate::iso19111::datum::Ellipsoid;
use crate::iso19111::metadata::{EPSG, Identifier};
use crate::iso19111::operation::{
    Conversion, OperationMethod, OperationParameter, OperationParameterValue, ParameterValue,
};
use crate::projection::epsg_method::math::{msfn, tsfn};

const PARAMETER_LATITUDE_FALSE_ORIGIN: i32 = 8821;
const PARAMETER_LONGITUDE_FALSE_ORIGIN: i32 = 8822;
const PARAMETER_LATITUDE_2ND_STD_PARALLEL: i32 = 8824;
const PARAMETER_EASTING_FALSE_ORIGIN: i32 = 8826;
const PARAMETER_NORTHING_FALSE_ORIGIN: i32 = 8827;

pub(super) fn conversion_from_method_code(method_code: i32, values: Vec<Measure>) -> Conversion {
    // Conversion::create(properties, method_epsg_code, values), which names the method and parameters from PROJ's mapping
    let mapping = mapping_by_code(method_code).expect("the method code has a PROJ mapping");
    let epsg_identifiers = |code: i32| {
        if code == 0 {
            Vec::new()
        } else {
            vec![Identifier::new(EPSG, &code.to_string())]
        }
    };
    let parameter_values = mapping
        .params
        .iter()
        .zip(values)
        .map(|(param, value)| OperationParameterValue {
            parameter: OperationParameter {
                name: param.wkt2_name.to_string(),
                identifiers: epsg_identifiers(param.epsg_code),
            },
            value: ParameterValue::Measure(value),
        })
        .collect();
    Conversion {
        name: mapping.wkt2_name.to_string(),
        identifiers: Vec::new(),
        method: OperationMethod {
            name: mapping.wkt2_name.to_string(),
            identifiers: epsg_identifiers(mapping.epsg_code),
        },
        parameter_values,
    }
}

fn msfn_of(phi: f64, e2: f64) -> f64 {
    msfn(phi.sin(), phi.cos(), e2)
}

fn tsfn_of(phi: f64, ec: f64) -> f64 {
    tsfn(phi, phi.sin(), ec)
}

fn lcc_1sp_to_2sp_f(sinphi: f64, k: f64, ec: f64, n: f64) -> f64 {
    // the function whose zeroes are the sines of the standard parallels of LCC 2SP
    let x = sinphi;
    let ecx = ec * x;
    (1.0 - x * x) / (1.0 - ecx * ecx)
        - k * k * ((1.0 - x) / (1.0 + x) * ((1.0 + ecx) / (1.0 - ecx)).powf(ec)).powf(n)
}

fn find_zero_lcc_1sp_to_2sp_f(sinphi0: f64, north: bool, k: f64, ec: f64) -> f64 {
    let (mut a, mut b, mut f_a) = if north {
        (sinphi0, 1.0, 1.0)
    } else {
        (-1.0, sinphi0, -1.0)
    };
    for _ in 0..100 {
        let c = (a + b) / 2.0;
        let f_c = lcc_1sp_to_2sp_f(c, k, ec, sinphi0);
        if f_c == 0.0 || (b - a) < 1e-18 {
            return c;
        }
        if (f_c > 0.0 && f_a > 0.0) || (f_c < 0.0 && f_a < 0.0) {
            a = c;
            f_a = f_c;
        } else {
            b = c;
        }
    }
    (a + b) / 2.0
}

fn round_to_thousandth_if_close(degrees: f64) -> f64 {
    if (degrees * 1000.0 - (degrees * 1000.0 + 0.5).floor()).abs() < 1e-8 {
        (degrees * 1000.0 + 0.5).floor() / 1000.0
    } else {
        degrees
    }
}

fn squared_eccentricity(ellipsoid: &Ellipsoid) -> f64 {
    // Ellipsoid::squaredEccentricity
    let inverse_flattening = ellipsoid.computed_inverse_flattening();
    let flattening = if inverse_flattening != 0.0 {
        1.0 / inverse_flattening
    } else {
        0.0
    };
    flattening * (2.0 - flattening)
}

fn radians_to_degrees(radians: f64) -> f64 {
    // RadToDeg and DegToRad in conversion.cpp, which round differently from to_degrees and to_radians
    radians / PI * 180.0
}

fn degrees_to_radians(degrees: f64) -> f64 {
    degrees / 180.0 * PI
}

fn is_strictly_between_the_poles(latitude: f64) -> bool {
    // conversion.cpp writes !(fabs(phi) < M_PI / 2), which also rejects NaN
    latitude.abs() < FRAC_PI_2
}

fn null_measure() -> Measure {
    Measure::new(0.0, UnitOfMeasure::none())
}

fn degrees(value: f64) -> Measure {
    Measure::new(value, UnitOfMeasure::degree())
}

impl Conversion {
    pub fn convert_to_other_method(
        &self,
        source_ellipsoid: &Ellipsoid,
        target_method_code: i32,
    ) -> Option<Conversion> {
        // Conversion::convertToOtherMethod: Mercator A and B, LCC 1SP and 2SP, on the ellipsoid of the source CRS
        let current_method_code = method_epsg_code(&self.method);
        if current_method_code == target_method_code {
            return Some(self.clone());
        }
        let e2 = squared_eccentricity(source_ellipsoid);
        if e2 < 0.0 {
            return None;
        }
        let values = &self.parameter_values;
        let measure = |code: i32| {
            parameter_measure(values, code)
                .cloned()
                .unwrap_or_else(null_measure)
        };
        let si_value = |code: i32| parameter_si_value(values, code);

        if current_method_code == METHOD_MERCATOR_VARIANT_A
            && target_method_code == METHOD_MERCATOR_VARIANT_B
            && si_value(PARAMETER_LATITUDE_OF_NATURAL_ORIGIN) == 0.0
        {
            let k0 = si_value(PARAMETER_SCALE_FACTOR_AT_NATURAL_ORIGIN);
            if !(k0 > 0.0 && k0 <= 1.0 + 1e-10) {
                return None;
            }
            let first_parallel = if k0 >= 1.0 {
                0.0
            } else {
                ((1.0 - e2) / ((1.0 / (k0 * k0)) - e2)).sqrt().acos()
            };
            return Some(conversion_from_method_code(
                METHOD_MERCATOR_VARIANT_B,
                vec![
                    degrees(first_parallel / UnitOfMeasure::degree().conversion_to_si),
                    measure(PARAMETER_LONGITUDE_OF_NATURAL_ORIGIN),
                    measure(PARAMETER_FALSE_EASTING),
                    measure(PARAMETER_FALSE_NORTHING),
                ],
            ));
        }

        if current_method_code == METHOD_MERCATOR_VARIANT_B
            && target_method_code == METHOD_MERCATOR_VARIANT_A
        {
            let phi1 = si_value(PARAMETER_LATITUDE_1ST_STD_PARALLEL);
            if !is_strictly_between_the_poles(phi1) {
                return None;
            }
            let k0 = msfn_of(phi1, e2);
            return Some(conversion_from_method_code(
                METHOD_MERCATOR_VARIANT_A,
                vec![
                    degrees(0.0),
                    measure(PARAMETER_LONGITUDE_OF_NATURAL_ORIGIN),
                    Measure::new(k0, UnitOfMeasure::scale_unity()),
                    measure(PARAMETER_FALSE_EASTING),
                    measure(PARAMETER_FALSE_NORTHING),
                ],
            ));
        }

        if current_method_code == METHOD_LAMBERT_CONIC_CONFORMAL_1SP
            && target_method_code == METHOD_LAMBERT_CONIC_CONFORMAL_2SP
        {
            return lcc_1sp_to_2sp(source_ellipsoid, e2, &measure, &si_value);
        }

        if current_method_code == METHOD_LAMBERT_CONIC_CONFORMAL_2SP
            && target_method_code == METHOD_LAMBERT_CONIC_CONFORMAL_1SP
        {
            return lcc_2sp_to_1sp(source_ellipsoid, e2, &measure, &si_value);
        }
        None
    }
}

fn lcc_1sp_to_2sp(
    ellipsoid: &Ellipsoid,
    e2: f64,
    measure: &dyn Fn(i32) -> Measure,
    si_value: &dyn Fn(i32) -> f64,
) -> Option<Conversion> {
    // notations of EPSG guidance note 7-2 sections 1.3.1.1 and 1.3.1.2, or Snyder pages 106 to 109
    let latitude_of_origin = measure(PARAMETER_LATITUDE_OF_NATURAL_ORIGIN);
    let phi0 = latitude_of_origin.si_value();
    let k0 = si_value(PARAMETER_SCALE_FACTOR_AT_NATURAL_ORIGIN);
    if !is_strictly_between_the_poles(phi0) || !(k0 > 0.0 && k0 <= 1.0 + 1e-10) {
        return None;
    }
    let ec = e2.sqrt();
    let m0 = msfn_of(phi0, e2);
    let t0 = tsfn_of(phi0, ec);
    let n = phi0.sin();
    if n.abs() < 1e-10 {
        return None;
    }
    let longitude = measure(PARAMETER_LONGITUDE_OF_NATURAL_ORIGIN);
    let false_easting = measure(PARAMETER_FALSE_EASTING);
    if (k0 - 1.0).abs() <= 1e-10 {
        return Some(conversion_from_method_code(
            METHOD_LAMBERT_CONIC_CONFORMAL_2SP,
            vec![
                latitude_of_origin.clone(),
                longitude,
                latitude_of_origin.clone(),
                latitude_of_origin,
                false_easting,
                measure(PARAMETER_FALSE_NORTHING),
            ],
        ));
    }
    let k = k0 * m0 / t0.powf(n);
    let phi1 = find_zero_lcc_1sp_to_2sp_f(n, true, k, ec).asin();
    let phi2 = find_zero_lcc_1sp_to_2sp_f(n, false, k, ec).asin();
    let phi1_degrees = round_to_thousandth_if_close(radians_to_degrees(phi1));
    let phi2_degrees = round_to_thousandth_if_close(radians_to_degrees(phi2));

    // PROJ turns the LCC 1SP equivalent of EPSG:2154 back into its LCC 2SP form this way
    let false_northing = si_value(PARAMETER_FALSE_NORTHING);
    let latitude_of_origin_degrees = latitude_of_origin.convert_to_unit(&UnitOfMeasure::degree());
    if (latitude_of_origin_degrees * 2.0 - (latitude_of_origin_degrees * 2.0 + 0.5).floor()).abs()
        < 0.2
    {
        let rounded_latitude_of_origin = (latitude_of_origin_degrees * 2.0 + 0.5).floor() / 2.0;
        let m1 = msfn_of(phi1, e2);
        let t1 = tsfn_of(phi1, ec);
        let f = m1 / (n * t1.powf(n));
        let a = ellipsoid.semi_major_axis.si_value();
        let t_rounded = tsfn_of(degrees_to_radians(rounded_latitude_of_origin), ec);
        let correction = a * f * (t_rounded.powf(n) - t0.powf(n));
        let corrected = false_northing - correction;
        let corrected_rounded = (corrected + 0.5).floor();
        if (corrected - corrected_rounded).abs() < 1e-8 {
            return Some(conversion_from_method_code(
                METHOD_LAMBERT_CONIC_CONFORMAL_2SP,
                vec![
                    degrees(rounded_latitude_of_origin),
                    longitude,
                    degrees(phi1_degrees),
                    degrees(phi2_degrees),
                    false_easting,
                    Measure::new(corrected_rounded, UnitOfMeasure::metre()),
                ],
            ));
        }
    }
    Some(conversion_from_method_code(
        METHOD_LAMBERT_CONIC_CONFORMAL_2SP,
        vec![
            latitude_of_origin,
            longitude,
            degrees(phi1_degrees),
            degrees(phi2_degrees),
            false_easting,
            Measure::new(false_northing, UnitOfMeasure::metre()),
        ],
    ))
}

fn lcc_2sp_to_1sp(
    ellipsoid: &Ellipsoid,
    e2: f64,
    measure: &dyn Fn(i32) -> Measure,
    si_value: &dyn Fn(i32) -> f64,
) -> Option<Conversion> {
    let phi_f = measure(PARAMETER_LATITUDE_FALSE_ORIGIN).si_value();
    let phi1 = measure(PARAMETER_LATITUDE_1ST_STD_PARALLEL).si_value();
    let phi2 = measure(PARAMETER_LATITUDE_2ND_STD_PARALLEL).si_value();
    if !is_strictly_between_the_poles(phi_f)
        || !is_strictly_between_the_poles(phi1)
        || !is_strictly_between_the_poles(phi2)
    {
        return None;
    }
    let ec = e2.sqrt();
    let m1 = msfn_of(phi1, e2);
    let m2 = msfn_of(phi2, e2);
    let t1 = tsfn_of(phi1, ec);
    let t2 = tsfn_of(phi2, ec);
    let n_denominator = t1.ln() - t2.ln();
    let n = if n_denominator.abs() < 1e-10 {
        phi1.sin()
    } else {
        (m1.ln() - m2.ln()) / n_denominator
    };
    if n.abs() < 1e-10 {
        return None;
    }
    let f = m1 / (n * t1.powf(n));
    let phi0 = n.asin();
    let m0 = msfn_of(phi0, e2);
    let t0 = tsfn_of(phi0, ec);
    let f0 = m0 / (n * t0.powf(n));
    let k0 = f / f0;
    let a = ellipsoid.semi_major_axis.si_value();
    let t_f = tsfn_of(phi_f, ec);
    let correction = a * f * (t_f.powf(n) - t0.powf(n));
    let phi0_degrees = round_to_thousandth_if_close(radians_to_degrees(phi0));
    let false_northing = si_value(PARAMETER_NORTHING_FALSE_ORIGIN)
        + if correction.abs() > 1e-8 {
            correction
        } else {
            0.0
        };
    Some(conversion_from_method_code(
        METHOD_LAMBERT_CONIC_CONFORMAL_1SP,
        vec![
            degrees(phi0_degrees),
            measure(PARAMETER_LONGITUDE_FALSE_ORIGIN),
            Measure::new(k0, UnitOfMeasure::scale_unity()),
            measure(PARAMETER_EASTING_FALSE_ORIGIN),
            Measure::new(false_northing, UnitOfMeasure::metre()),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iso19111::datum::EllipsoidShape;

    fn ellipsoid(inverse_flattening: f64) -> Ellipsoid {
        Ellipsoid {
            name: String::new(),
            identifiers: Vec::new(),
            semi_major_axis: Measure::new(6378137.0, UnitOfMeasure::metre()),
            shape: EllipsoidShape::InverseFlattening(inverse_flattening),
        }
    }

    fn value_in(conversion: &Conversion, code: i32, unit: &UnitOfMeasure) -> f64 {
        parameter_measure(&conversion.parameter_values, code)
            .expect("parameter present")
            .convert_to_unit(unit)
    }

    #[test]
    fn mercator_variant_a_converts_to_variant_b_as_proj_does() {
        // PROJ test/unit/test_operation.cpp operation.mercator_variant_A_to_variant_B
        let wgs84 = ellipsoid(298.257223563);
        let variant_a = conversion_from_method_code(
            METHOD_MERCATOR_VARIANT_A,
            vec![
                degrees(0.0),
                degrees(1.0),
                Measure::new(0.9, UnitOfMeasure::scale_unity()),
                Measure::new(3.0, UnitOfMeasure::metre()),
                Measure::new(4.0, UnitOfMeasure::metre()),
            ],
        );
        let same = variant_a
            .convert_to_other_method(&wgs84, METHOD_MERCATOR_VARIANT_A)
            .expect("same method");
        assert!(same.is_equivalent_to(&variant_a, &wgs84, &wgs84));
        let variant_b = variant_a
            .convert_to_other_method(&wgs84, METHOD_MERCATOR_VARIANT_B)
            .expect("variant B");
        let degree = UnitOfMeasure::degree();
        let metre = UnitOfMeasure::metre();
        assert_eq!(
            value_in(&variant_b, PARAMETER_LATITUDE_1ST_STD_PARALLEL, &degree),
            25.917499691810534
        );
        assert_eq!(
            value_in(&variant_b, PARAMETER_LONGITUDE_OF_NATURAL_ORIGIN, &degree),
            1.0
        );
        assert_eq!(value_in(&variant_b, PARAMETER_FALSE_EASTING, &metre), 3.0);
        assert_eq!(value_in(&variant_b, PARAMETER_FALSE_NORTHING, &metre), 4.0);
        assert!(variant_a.is_equivalent_to(&variant_b, &wgs84, &wgs84));
        assert!(variant_b.is_equivalent_to(&variant_a, &wgs84, &wgs84));
    }

    #[test]
    fn lcc_2sp_converts_to_1sp_and_back_as_proj_does() {
        // PROJ test/unit/test_operation.cpp operation.lcc2sp_to_lcc1sp, equivalent to EPSG:2154
        let grs80 = ellipsoid(298.257222101);
        let two_standard_parallels = conversion_from_method_code(
            METHOD_LAMBERT_CONIC_CONFORMAL_2SP,
            vec![
                degrees(46.5),
                degrees(3.0),
                degrees(49.0),
                degrees(44.0),
                Measure::new(700000.0, UnitOfMeasure::metre()),
                Measure::new(6600000.0, UnitOfMeasure::metre()),
            ],
        );
        let one = two_standard_parallels
            .convert_to_other_method(&grs80, METHOD_LAMBERT_CONIC_CONFORMAL_1SP)
            .expect("LCC 1SP");
        let degree = UnitOfMeasure::degree();
        let metre = UnitOfMeasure::metre();
        let unity = UnitOfMeasure::scale_unity();
        let close = |value: f64, expected: f64, tolerance: f64| {
            assert!(
                (value - expected).abs() <= tolerance,
                "{value} vs {expected}"
            );
        };
        close(
            value_in(&one, PARAMETER_LATITUDE_OF_NATURAL_ORIGIN, &degree),
            46.519430223986866,
            1e-12,
        );
        close(
            value_in(&one, PARAMETER_LONGITUDE_OF_NATURAL_ORIGIN, &degree),
            3.0,
            1e-15,
        );
        close(
            value_in(&one, PARAMETER_SCALE_FACTOR_AT_NATURAL_ORIGIN, &unity),
            0.9990510286374692,
            1e-15,
        );
        close(
            value_in(&one, PARAMETER_FALSE_EASTING, &metre),
            700000.0,
            1e-15,
        );
        close(
            value_in(&one, PARAMETER_FALSE_NORTHING, &metre),
            6_602_157.838_810_337,
            1e-7,
        );

        let back = one
            .convert_to_other_method(&grs80, METHOD_LAMBERT_CONIC_CONFORMAL_2SP)
            .expect("LCC 2SP");
        close(
            value_in(&back, PARAMETER_LATITUDE_FALSE_ORIGIN, &degree),
            46.5,
            1e-15,
        );
        close(
            value_in(&back, PARAMETER_LONGITUDE_FALSE_ORIGIN, &degree),
            3.0,
            1e-15,
        );
        close(
            value_in(&back, PARAMETER_LATITUDE_1ST_STD_PARALLEL, &degree),
            49.0,
            1e-15,
        );
        close(
            value_in(&back, PARAMETER_LATITUDE_2ND_STD_PARALLEL, &degree),
            44.0,
            1e-15,
        );
        close(
            value_in(&back, PARAMETER_EASTING_FALSE_ORIGIN, &metre),
            700000.0,
            1e-15,
        );
        close(
            value_in(&back, PARAMETER_NORTHING_FALSE_ORIGIN, &metre),
            6600000.0,
            1e-15,
        );

        assert!(two_standard_parallels.is_equivalent_to(&one, &grs80, &grs80));
        assert!(one.is_equivalent_to(&two_standard_parallels, &grs80, &grs80));
    }
}
