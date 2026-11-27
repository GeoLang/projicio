// adapted from PROJ src/conversions/cart.cpp and src/ell_set.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::FRAC_PI_2;

use super::GeodeticCoord;
use crate::projection::epsg_method::EllipsoidConstants;
use crate::projection::epsg_method::math::prepare_angular_input;
use crate::{Ellipsoid, Error, GeocentricCoord};

#[derive(Debug, Clone, Copy)]
pub struct GeocentricConversion {
    a: f64,
    ra: f64,
    f: f64,
    es: f64,
    second_eccentricity_squared: f64,
}

fn normal_radius_of_curvature(a: f64, es: f64, sin_phi: f64) -> f64 {
    if es == 0.0 {
        return a;
    }
    a / (1.0 - es * sin_phi * sin_phi).sqrt()
}

fn geocentric_radius(a: f64, b_div_a: f64, cos_phi: f64, sin_phi: f64) -> f64 {
    let cos_phi_squared = cos_phi * cos_phi;
    let sin_phi_squared = sin_phi * sin_phi;
    let b_div_a_squared = b_div_a * b_div_a;
    let b_div_a_squared_mul_sin_phi_squared = b_div_a_squared * sin_phi_squared;
    a * ((cos_phi_squared + b_div_a_squared * b_div_a_squared_mul_sin_phi_squared)
        / (cos_phi_squared + b_div_a_squared_mul_sin_phi_squared))
        .sqrt()
}

impl GeocentricConversion {
    pub fn new(ellipsoid: &Ellipsoid) -> Result<Self, Error> {
        let shape = EllipsoidConstants::from_ellipsoid(ellipsoid)?;
        // ell_set.cpp takes the second eccentricity as the tangent of the angular eccentricity
        let second_eccentricity = shape.e.asin().tan();
        Ok(Self {
            a: shape.a,
            ra: 1.0 / shape.a,
            f: shape.f,
            es: shape.es,
            second_eccentricity_squared: second_eccentricity * second_eccentricity,
        })
    }

    pub fn forward(&self, point: GeodeticCoord) -> Result<GeocentricCoord, Error> {
        let prepared = prepare_angular_input(point.longitude, point.latitude)?;
        let (sin_phi, cos_phi) = prepared.phi.sin_cos();
        let n = normal_radius_of_curvature(self.a, self.es, sin_phi);
        Ok(GeocentricCoord {
            x: (n + point.height) * cos_phi * prepared.lambda.cos(),
            y: (n + point.height) * cos_phi * prepared.lambda.sin(),
            z: (n * (1.0 - self.es) + point.height) * sin_phi,
        })
    }

    pub fn inverse(&self, point: GeocentricCoord) -> GeodeticCoord {
        let x_div_a = point.x * self.ra;
        let y_div_a = point.y * self.ra;
        let z_div_a = point.z * self.ra;
        let p_div_a = (x_div_a * x_div_a + y_div_a * y_div_a).sqrt();

        let b_div_a = 1.0 - self.f;
        let p_div_a_b_div_a = p_div_a * b_div_a;
        let norm = (z_div_a * z_div_a + p_div_a_b_div_a * p_div_a_b_div_a).sqrt();
        let (cos_theta, sin_theta) = if norm != 0.0 {
            let inv_norm = 1.0 / norm;
            (p_div_a_b_div_a * inv_norm, z_div_a * inv_norm)
        } else {
            (1.0, 0.0)
        };

        let y_phi = z_div_a
            + self.second_eccentricity_squared * b_div_a * sin_theta * sin_theta * sin_theta;
        let x_phi = p_div_a - self.es * cos_theta * cos_theta * cos_theta;
        let norm_phi = (y_phi * y_phi + x_phi * x_phi).sqrt();
        let (cos_phi, sin_phi) = if norm_phi != 0.0 {
            let inv_norm_phi = 1.0 / norm_phi;
            (x_phi * inv_norm_phi, y_phi * inv_norm_phi)
        } else {
            (1.0, 0.0)
        };
        let (latitude, cos_phi, sin_phi) = if x_phi > 0.0 {
            ((y_phi / x_phi).atan(), cos_phi, sin_phi)
        } else if point.z >= 0.0 {
            // cart.cpp: points this close to the centre have no single geodetic solution
            (FRAC_PI_2, 0.0, 1.0)
        } else {
            (-FRAC_PI_2, 0.0, -1.0)
        };
        let longitude = y_div_a.atan2(x_div_a);

        // cart.cpp avoids dividing by a cosine near zero poleward of 89.99994 degrees
        let height = if cos_phi < 1e-6 {
            point.z.abs() - geocentric_radius(self.a, b_div_a, cos_phi, sin_phi)
        } else {
            let n = normal_radius_of_curvature(self.a, self.es, sin_phi);
            self.a * p_div_a / cos_phi - n
        };

        GeodeticCoord {
            longitude,
            latitude,
            height,
        }
    }
}
