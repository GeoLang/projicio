// adapted from PROJ src/fwd.cpp, src/inv.cpp, src/ell_set.cpp, src/iso19111/operation/parammappings.cpp, src/iso19111/operation/conversion.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::FRAC_PI_2;

use super::Projection;
use crate::{Coord, Ellipsoid, Error, Geographic};

mod aea;
mod aeqd;
mod cass;
mod cea;
mod col_urban;
mod eqc;
mod eqearth;
mod geodesic;
mod krovak;
mod labrd;
mod laea;
mod lcc;
mod math;
mod merc;
mod nzmg;
mod omerc;
mod ortho;
mod poly;
mod somerc;
mod stere;
mod sterea;
mod tmerc;
mod tmerczoned;

use math::adjlon;

const LATITUDE_OF_NATURAL_ORIGIN: u32 = 8801;
const LONGITUDE_OF_NATURAL_ORIGIN: u32 = 8802;
const SCALE_FACTOR_AT_NATURAL_ORIGIN: u32 = 8805;
const FALSE_EASTING: u32 = 8806;
const FALSE_NORTHING: u32 = 8807;
const LATITUDE_OF_PROJECTION_CENTRE: u32 = 8811;
const LONGITUDE_OF_PROJECTION_CENTRE: u32 = 8812;
const AZIMUTH_AT_PROJECTION_CENTRE: u32 = 8813;
const ANGLE_FROM_RECTIFIED_TO_SKEW_GRID: u32 = 8814;
const SCALE_FACTOR_AT_PROJECTION_CENTRE: u32 = 8815;
const EASTING_AT_PROJECTION_CENTRE: u32 = 8816;
const NORTHING_AT_PROJECTION_CENTRE: u32 = 8817;
const LATITUDE_OF_PSEUDO_STANDARD_PARALLEL: u32 = 8818;
const SCALE_FACTOR_ON_PSEUDO_STANDARD_PARALLEL: u32 = 8819;
const LATITUDE_OF_FALSE_ORIGIN: u32 = 8821;
const LONGITUDE_OF_FALSE_ORIGIN: u32 = 8822;
const LATITUDE_OF_FIRST_STANDARD_PARALLEL: u32 = 8823;
const LATITUDE_OF_SECOND_STANDARD_PARALLEL: u32 = 8824;
const EASTING_AT_FALSE_ORIGIN: u32 = 8826;
const NORTHING_AT_FALSE_ORIGIN: u32 = 8827;
const INITIAL_LONGITUDE: u32 = 8830;
const ZONE_WIDTH: u32 = 8831;
const LATITUDE_OF_STANDARD_PARALLEL: u32 = 8832;
const LONGITUDE_OF_ORIGIN: u32 = 8833;
const COLATITUDE_OF_CONE_AXIS: u32 = 1036;
const ELLIPSOID_SCALING_FACTOR: u32 = 1038;
const PROJECTION_PLANE_ORIGIN_HEIGHT: u32 = 1039;

#[derive(Debug, Clone, Copy)]
pub(super) struct LambdaPhi {
    pub(super) lambda: f64,
    pub(super) phi: f64,
}

pub(super) trait ClassicProjection {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error>;
    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error>;
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EllipsoidConstants {
    pub(super) a: f64,
    pub(super) f: f64,
    pub(super) es: f64,
    pub(super) e: f64,
    pub(super) one_es: f64,
    pub(super) rone_es: f64,
    pub(super) n: f64,
}

impl EllipsoidConstants {
    fn new(a: f64, f: f64) -> Result<Self, Error> {
        if a.is_nan() || a <= 0.0 {
            return Err(Error::ProjectionError(format!(
                "invalid semi-major axis {a}"
            )));
        }
        if !(0.0..1.0).contains(&f) {
            return Err(Error::ProjectionError(format!("invalid flattening {f}")));
        }
        let es = 2.0 * f - f * f;
        let e = es.sqrt();
        let alpha = e.asin();
        let one_es = 1.0 - es;
        Ok(Self {
            a,
            f,
            es,
            e,
            one_es,
            rone_es: 1.0 / one_es,
            n: (alpha / 2.0).tan().powi(2),
        })
    }

    fn from_ellipsoid(ellipsoid: &Ellipsoid) -> Result<Self, Error> {
        Self::new(ellipsoid.a, ellipsoid.f)
    }

    // PROJ's +R_A, the sphere with the ellipsoid's surface area
    fn authalic_sphere(&self) -> Result<Self, Error> {
        const SIXTH: f64 = 1.0 / 6.0;
        const RA4: f64 = 17.0 / 360.0;
        const RA6: f64 = 67.0 / 3024.0;
        let es = self.es;
        Self::new(self.a * (1.0 - es * (SIXTH + es * (RA4 + es * RA6))), 0.0)
    }
}

struct EpsgMethodProjection {
    ellipsoid: Ellipsoid,
    a: f64,
    lam0: f64,
    x0: f64,
    y0: f64,
    classic: Box<dyn ClassicProjection + Send + Sync>,
}

const PJ_EPS_LAT: f64 = 1e-12;

impl Projection for EpsgMethodProjection {
    fn forward(&self, geo: Geographic) -> Result<Coord, Error> {
        let mut phi = geo.lat.to_radians();
        let lambda = geo.lon.to_radians();
        if phi.is_nan() || phi.abs() - FRAC_PI_2 > PJ_EPS_LAT {
            return Err(Error::InvalidCoordinate(format!(
                "latitude {} out of range",
                geo.lat
            )));
        }
        if lambda.is_nan() || lambda.abs() > 10.0 {
            return Err(Error::InvalidCoordinate(format!(
                "longitude {} out of range",
                geo.lon
            )));
        }
        phi = phi.clamp(-FRAC_PI_2, FRAC_PI_2);
        let lambda = adjlon(adjlon(lambda) - self.lam0);
        let xy = self.classic.forward(LambdaPhi { lambda, phi })?;
        if !(xy.x.is_finite() && xy.y.is_finite()) {
            return Err(Error::ProjectionError(
                "coordinate outside the projection domain".into(),
            ));
        }
        Ok(Coord::new(xy.x * self.a + self.x0, xy.y * self.a + self.y0))
    }

    fn inverse(&self, coord: Coord) -> Result<Geographic, Error> {
        if !(coord.x.is_finite() && coord.y.is_finite()) {
            return Err(Error::InvalidCoordinate(format!(
                "projected coordinate {coord:?} is not finite"
            )));
        }
        let ra = 1.0 / self.a;
        let xy = Coord::new((coord.x - self.x0) * ra, (coord.y - self.y0) * ra);
        let lp = self.classic.inverse(xy)?;
        if !(lp.lambda.is_finite() && lp.phi.is_finite()) {
            return Err(Error::ProjectionError(
                "coordinate outside the projection domain".into(),
            ));
        }
        let lambda = adjlon(lp.lambda + self.lam0);
        Ok(Geographic::new(lambda.to_degrees(), lp.phi.to_degrees()))
    }

    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
}

struct Parameters<'a> {
    method_code: u32,
    values: &'a [(u32, f64)],
}

impl Parameters<'_> {
    fn get(&self, parameter_code: u32) -> Result<f64, Error> {
        self.values
            .iter()
            .find(|(code, _)| *code == parameter_code)
            .map(|(_, value)| *value)
            .ok_or_else(|| {
                Error::ProjectionError(format!(
                    "EPSG method {}: missing parameter {parameter_code}",
                    self.method_code
                ))
            })
    }

    // init.cpp refuses a scale factor that is not positive, for every projection
    fn scale_factor(&self, parameter_code: u32) -> Result<f64, Error> {
        let value = self.get(parameter_code)?;
        if value.is_nan() || value <= 0.0 {
            return Err(self.unsupported_value(parameter_code, value));
        }
        Ok(value)
    }

    fn require_zero(&self, parameter_code: u32) -> Result<(), Error> {
        let value = self.get(parameter_code)?;
        if value != 0.0 {
            return Err(self.unsupported_value(parameter_code, value));
        }
        Ok(())
    }

    fn unsupported_value(&self, parameter_code: u32, value: f64) -> Error {
        Error::ProjectionError(format!(
            "EPSG method {}: unsupported value {value} for parameter {parameter_code}",
            self.method_code
        ))
    }
}

struct Origin {
    lam0: f64,
    x0: f64,
    y0: f64,
}

impl Origin {
    fn natural(parameters: &Parameters) -> Result<Self, Error> {
        Ok(Self {
            lam0: parameters.get(LONGITUDE_OF_NATURAL_ORIGIN)?,
            x0: parameters.get(FALSE_EASTING)?,
            y0: parameters.get(FALSE_NORTHING)?,
        })
    }

    fn false_origin(parameters: &Parameters) -> Result<Self, Error> {
        Ok(Self {
            lam0: parameters.get(LONGITUDE_OF_FALSE_ORIGIN)?,
            x0: parameters.get(EASTING_AT_FALSE_ORIGIN)?,
            y0: parameters.get(NORTHING_AT_FALSE_ORIGIN)?,
        })
    }
}

fn boxed(
    ellipsoid: &Ellipsoid,
    a: f64,
    origin: Origin,
    classic: impl ClassicProjection + Send + Sync + 'static,
) -> Box<dyn Projection + Send + Sync> {
    Box::new(EpsgMethodProjection {
        ellipsoid: *ellipsoid,
        a,
        lam0: origin.lam0,
        x0: origin.x0,
        y0: origin.y0,
        classic: Box::new(classic),
    })
}

fn oblique_mercator(
    parameters: &Parameters,
    shape: &EllipsoidConstants,
    ellipsoid: &Ellipsoid,
    x0: f64,
    y0: f64,
) -> Result<Box<dyn Projection + Send + Sync>, Error> {
    let phi0 = parameters.get(LATITUDE_OF_PROJECTION_CENTRE)?;
    let lonc = parameters.get(LONGITUDE_OF_PROJECTION_CENTRE)?;
    let alpha = parameters.get(AZIMUTH_AT_PROJECTION_CENTRE)?;
    let gamma = parameters.get(ANGLE_FROM_RECTIFIED_TO_SKEW_GRID)?;
    let k0 = parameters.scale_factor(SCALE_FACTOR_AT_PROJECTION_CENTRE)?;
    // conversion.cpp maps a 90 degree azimuth and skew angle to somerc, comparing in degrees
    let swiss =
        (alpha.to_degrees() - 90.0).abs() < 1e-4 && (gamma.to_degrees() - 90.0).abs() < 1e-4;
    if swiss {
        let origin = Origin { lam0: lonc, x0, y0 };
        let somerc = somerc::SwissObliqueMercator::new(shape, phi0, k0)?;
        return Ok(boxed(ellipsoid, shape.a, origin, somerc));
    }
    let omerc = if parameters.method_code == 9812 {
        omerc::ObliqueMercator::variant_a(shape, phi0, lonc, alpha, gamma, k0)?
    } else {
        omerc::ObliqueMercator::variant_b(shape, phi0, lonc, alpha, gamma, k0)?
    };
    let origin = Origin {
        lam0: omerc.lam0,
        x0,
        y0,
    };
    Ok(boxed(ellipsoid, shape.a, origin, omerc))
}

fn require_semi_major_axis(
    parameters: &Parameters,
    ellipsoid: &Ellipsoid,
    a: f64,
) -> Result<(), Error> {
    if (ellipsoid.a - a).abs() > 1e-3 {
        return Err(Error::ProjectionError(format!(
            "EPSG method {}: PROJ fixes the semi-major axis at {a} m, got {} m",
            parameters.method_code, ellipsoid.a
        )));
    }
    Ok(())
}

fn krovak(
    parameters: &Parameters,
    ellipsoid: &Ellipsoid,
) -> Result<Box<dyn Projection + Send + Sync>, Error> {
    require_semi_major_axis(parameters, ellipsoid, krovak::BESSEL_SEMI_MAJOR_AXIS)?;
    // conversion.cpp refuses any other cone axis or pseudo standard parallel, krovak.cpp hardcodes them
    let colatitude = parameters.get(COLATITUDE_OF_CONE_AXIS)?;
    if (colatitude.to_degrees() - 30.2881397).abs() > 1e-7 {
        return Err(parameters.unsupported_value(COLATITUDE_OF_CONE_AXIS, colatitude));
    }
    let pseudo_standard_parallel = parameters.get(LATITUDE_OF_PSEUDO_STANDARD_PARALLEL)?;
    if (pseudo_standard_parallel.to_degrees() - 78.5).abs() > 1e-8 {
        return Err(parameters.unsupported_value(
            LATITUDE_OF_PSEUDO_STANDARD_PARALLEL,
            pseudo_standard_parallel,
        ));
    }
    let origin = Origin {
        lam0: parameters.get(LONGITUDE_OF_ORIGIN)?,
        x0: parameters.get(FALSE_EASTING)?,
        y0: parameters.get(FALSE_NORTHING)?,
    };
    let phi0 = parameters.get(LATITUDE_OF_PROJECTION_CENTRE)?;
    let k0 = parameters.scale_factor(SCALE_FACTOR_ON_PSEUDO_STANDARD_PARALLEL)?;
    let projection = if matches!(parameters.method_code, 1042 | 1043) {
        krovak::Krovak::modified(phi0, origin.lam0, k0, origin.x0, origin.y0)?
    } else {
        krovak::Krovak::standard(phi0, origin.lam0, k0, origin.x0, origin.y0)?
    };
    Ok(boxed(
        ellipsoid,
        krovak::BESSEL_SEMI_MAJOR_AXIS,
        origin,
        projection,
    ))
}

fn new_zealand_map_grid(
    parameters: &Parameters,
    ellipsoid: &Ellipsoid,
) -> Result<Box<dyn Projection + Send + Sync>, Error> {
    require_semi_major_axis(parameters, ellipsoid, nzmg::SEMI_MAJOR_AXIS)?;
    let origin = Origin {
        lam0: nzmg::LONGITUDE_OF_ORIGIN_DEGREES.to_radians(),
        x0: nzmg::FALSE_EASTING,
        y0: nzmg::FALSE_NORTHING,
    };
    // nzmg.cpp ignores the conversion's own origin, so any other origin is refused rather than dropped
    let given = [
        (
            LATITUDE_OF_NATURAL_ORIGIN,
            nzmg::LATITUDE_OF_ORIGIN_DEGREES.to_radians(),
            1e-12,
        ),
        (LONGITUDE_OF_NATURAL_ORIGIN, origin.lam0, 1e-12),
        (FALSE_EASTING, origin.x0, 1e-6),
        (FALSE_NORTHING, origin.y0, 1e-6),
    ];
    for (code, fixed, tolerance) in given {
        let value = parameters.get(code)?;
        if (value - fixed).abs() > tolerance {
            return Err(parameters.unsupported_value(code, value));
        }
    }
    Ok(boxed(
        ellipsoid,
        nzmg::SEMI_MAJOR_AXIS,
        origin,
        nzmg::NewZealandMapGrid::new(),
    ))
}

pub fn from_epsg_method(
    method_code: u32,
    parameters: &[(u32, f64)],
    ellipsoid: &Ellipsoid,
) -> Result<Box<dyn Projection + Send + Sync>, Error> {
    let p = Parameters {
        method_code,
        values: parameters,
    };
    let shape = EllipsoidConstants::from_ellipsoid(ellipsoid)?;
    let a = shape.a;
    let projection: Box<dyn Projection + Send + Sync> = match method_code {
        9807 | 1111 => {
            let tmerc = tmerc::TransverseMercator::new(
                &shape,
                p.get(LATITUDE_OF_NATURAL_ORIGIN)?,
                p.scale_factor(SCALE_FACTOR_AT_NATURAL_ORIGIN)?,
            );
            boxed(ellipsoid, a, Origin::natural(&p)?, tmerc)
        }
        // conversion.cpp maps this to tmerc with +axis=wsu, which matches EPSG only with a zero false origin
        9808 => {
            p.require_zero(FALSE_EASTING)?;
            p.require_zero(FALSE_NORTHING)?;
            let tmerc = tmerc::TransverseMercator::new(
                &shape,
                p.get(LATITUDE_OF_NATURAL_ORIGIN)?,
                p.scale_factor(SCALE_FACTOR_AT_NATURAL_ORIGIN)?,
            );
            boxed(ellipsoid, a, Origin::natural(&p)?, tmerc)
        }
        // tmerczoned.cpp ignores the latitude of natural origin
        9824 => {
            p.require_zero(LATITUDE_OF_NATURAL_ORIGIN)?;
            let zoned = tmerczoned::TransverseMercatorZoned::new(
                &shape,
                p.scale_factor(SCALE_FACTOR_AT_NATURAL_ORIGIN)?,
                p.get(ZONE_WIDTH)?,
            )?;
            let origin = Origin {
                lam0: p.get(INITIAL_LONGITUDE)?,
                x0: p.get(FALSE_EASTING)?,
                y0: p.get(FALSE_NORTHING)?,
            };
            boxed(ellipsoid, a, origin, zoned)
        }
        9801 => {
            let phi1 = p.get(LATITUDE_OF_NATURAL_ORIGIN)?;
            let lcc = lcc::LambertConformalConic::new(
                &shape,
                phi1,
                phi1,
                phi1,
                p.scale_factor(SCALE_FACTOR_AT_NATURAL_ORIGIN)?,
            )?;
            boxed(ellipsoid, a, Origin::natural(&p)?, lcc)
        }
        1102 => {
            let phi1 = p.get(LATITUDE_OF_NATURAL_ORIGIN)?;
            let lcc = lcc::LambertConformalConic::new(
                &shape,
                p.get(LATITUDE_OF_FALSE_ORIGIN)?,
                phi1,
                phi1,
                p.scale_factor(SCALE_FACTOR_AT_NATURAL_ORIGIN)?,
            )?;
            boxed(ellipsoid, a, Origin::false_origin(&p)?, lcc)
        }
        // conversion.cpp maps the Belgian variant to plain lcc and marks it FIXME
        9802 | 9803 | 1051 => {
            let k0 = if method_code == 1051 {
                p.scale_factor(ELLIPSOID_SCALING_FACTOR)?
            } else {
                1.0
            };
            let lcc = lcc::LambertConformalConic::new(
                &shape,
                p.get(LATITUDE_OF_FALSE_ORIGIN)?,
                p.get(LATITUDE_OF_FIRST_STANDARD_PARALLEL)?,
                p.get(LATITUDE_OF_SECOND_STANDARD_PARALLEL)?,
                k0,
            )?;
            boxed(ellipsoid, a, Origin::false_origin(&p)?, lcc)
        }
        9822 => {
            let aea = aea::AlbersEqualArea::new(
                &shape,
                p.get(LATITUDE_OF_FALSE_ORIGIN)?,
                p.get(LATITUDE_OF_FIRST_STANDARD_PARALLEL)?,
                p.get(LATITUDE_OF_SECOND_STANDARD_PARALLEL)?,
            )?;
            boxed(ellipsoid, a, Origin::false_origin(&p)?, aea)
        }
        // parammappings.cpp maps the modified variant to the exact azimuthal equidistant too
        1125 | 9832 => {
            let aeqd = aeqd::AzimuthalEquidistant::new(&shape, p.get(LATITUDE_OF_NATURAL_ORIGIN)?)?;
            boxed(ellipsoid, a, Origin::natural(&p)?, aeqd)
        }
        9831 => {
            let guam =
                aeqd::AzimuthalEquidistant::guam(&shape, p.get(LATITUDE_OF_NATURAL_ORIGIN)?)?;
            boxed(ellipsoid, a, Origin::natural(&p)?, guam)
        }
        9806 => {
            let cass = cass::CassiniSoldner::new(&shape, p.get(LATITUDE_OF_NATURAL_ORIGIN)?);
            boxed(ellipsoid, a, Origin::natural(&p)?, cass)
        }
        9833 => {
            let cass = cass::CassiniSoldner::hyperbolic(&shape, p.get(LATITUDE_OF_NATURAL_ORIGIN)?);
            boxed(ellipsoid, a, Origin::natural(&p)?, cass)
        }
        9809 => {
            let sterea = sterea::ObliqueStereographic::new(
                &shape,
                p.get(LATITUDE_OF_NATURAL_ORIGIN)?,
                p.scale_factor(SCALE_FACTOR_AT_NATURAL_ORIGIN)?,
            )?;
            boxed(ellipsoid, a, Origin::natural(&p)?, sterea)
        }
        // stere.cpp puts the true-scale latitude on the pole when lat_ts is absent
        9810 => {
            let stere = stere::Stereographic::new(
                &shape,
                p.get(LATITUDE_OF_NATURAL_ORIGIN)?,
                FRAC_PI_2,
                p.scale_factor(SCALE_FACTOR_AT_NATURAL_ORIGIN)?,
            );
            boxed(ellipsoid, a, Origin::natural(&p)?, stere)
        }
        // conversion.cpp puts the origin on the pole on the standard parallel's side
        9829 => {
            let lat_ts = p.get(LATITUDE_OF_STANDARD_PARALLEL)?;
            let phi0 = if lat_ts >= 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 };
            let stere = stere::Stereographic::new(&shape, phi0, lat_ts, 1.0);
            let origin = Origin {
                lam0: p.get(LONGITUDE_OF_ORIGIN)?,
                x0: p.get(FALSE_EASTING)?,
                y0: p.get(FALSE_NORTHING)?,
            };
            boxed(ellipsoid, a, origin, stere)
        }
        9812 => oblique_mercator(
            &p,
            &shape,
            ellipsoid,
            p.get(FALSE_EASTING)?,
            p.get(FALSE_NORTHING)?,
        )?,
        9815 => oblique_mercator(
            &p,
            &shape,
            ellipsoid,
            p.get(EASTING_AT_PROJECTION_CENTRE)?,
            p.get(NORTHING_AT_PROJECTION_CENTRE)?,
        )?,
        9820 => {
            let laea =
                laea::LambertAzimuthalEqualArea::new(&shape, p.get(LATITUDE_OF_NATURAL_ORIGIN)?)?;
            boxed(ellipsoid, a, Origin::natural(&p)?, laea)
        }
        1027 => {
            let sphere = shape.authalic_sphere()?;
            let laea =
                laea::LambertAzimuthalEqualArea::new(&sphere, p.get(LATITUDE_OF_NATURAL_ORIGIN)?)?;
            boxed(ellipsoid, sphere.a, Origin::natural(&p)?, laea)
        }
        // conversion.cpp refuses a non-zero latitude of natural origin for Mercator
        9804 => {
            p.require_zero(LATITUDE_OF_NATURAL_ORIGIN)?;
            let merc =
                merc::Mercator::with_scale(&shape, p.scale_factor(SCALE_FACTOR_AT_NATURAL_ORIGIN)?);
            boxed(ellipsoid, a, Origin::natural(&p)?, merc)
        }
        9805 => {
            let merc = merc::Mercator::with_standard_parallel(
                &shape,
                p.get(LATITUDE_OF_FIRST_STANDARD_PARALLEL)?,
            )?;
            boxed(ellipsoid, a, Origin::natural(&p)?, merc)
        }
        1024 => boxed(ellipsoid, a, Origin::natural(&p)?, merc::Mercator::web()),
        9819 | 1041 | 1042 | 1043 => krovak(&p, ellipsoid)?,
        9818 => {
            let poly = poly::Polyconic::new(&shape, p.get(LATITUDE_OF_NATURAL_ORIGIN)?);
            boxed(ellipsoid, a, Origin::natural(&p)?, poly)
        }
        9813 => {
            let labrd = labrd::Laborde::new(
                &shape,
                p.get(LATITUDE_OF_PROJECTION_CENTRE)?,
                p.get(AZIMUTH_AT_PROJECTION_CENTRE)?,
                p.scale_factor(SCALE_FACTOR_AT_PROJECTION_CENTRE)?,
            )?;
            let origin = Origin {
                lam0: p.get(LONGITUDE_OF_PROJECTION_CENTRE)?,
                x0: p.get(FALSE_EASTING)?,
                y0: p.get(FALSE_NORTHING)?,
            };
            boxed(ellipsoid, a, origin, labrd)
        }
        9811 => new_zealand_map_grid(&p, ellipsoid)?,
        9835 => {
            let cea = cea::CylindricalEqualArea::new(
                &shape,
                p.get(LATITUDE_OF_FIRST_STANDARD_PARALLEL)?,
            )?;
            boxed(ellipsoid, a, Origin::natural(&p)?, cea)
        }
        9834 => {
            let sphere = shape.authalic_sphere()?;
            let cea = cea::CylindricalEqualArea::new(
                &sphere,
                p.get(LATITUDE_OF_FIRST_STANDARD_PARALLEL)?,
            )?;
            boxed(ellipsoid, sphere.a, Origin::natural(&p)?, cea)
        }
        // EPSG's method has no latitude of origin and eqc.cpp defaults lat_0 to zero
        1028 => {
            let eqc = eqc::EquidistantCylindrical::new(
                &shape,
                p.get(LATITUDE_OF_FIRST_STANDARD_PARALLEL)?,
                0.0,
            )?;
            boxed(ellipsoid, a, Origin::natural(&p)?, eqc)
        }
        1078 => boxed(
            ellipsoid,
            a,
            Origin::natural(&p)?,
            eqearth::EqualEarth::new(&shape),
        ),
        1130 => {
            let ortho = ortho::Orthographic::new(
                &shape,
                p.get(LATITUDE_OF_PROJECTION_CENTRE)?,
                p.get(AZIMUTH_AT_PROJECTION_CENTRE)?,
                p.scale_factor(SCALE_FACTOR_AT_PROJECTION_CENTRE)?,
            );
            let origin = Origin {
                lam0: p.get(LONGITUDE_OF_PROJECTION_CENTRE)?,
                x0: p.get(EASTING_AT_PROJECTION_CENTRE)?,
                y0: p.get(NORTHING_AT_PROJECTION_CENTRE)?,
            };
            boxed(ellipsoid, a, origin, ortho)
        }
        1052 => {
            let col_urban = col_urban::ColombiaUrban::new(
                &shape,
                p.get(LATITUDE_OF_NATURAL_ORIGIN)?,
                p.get(PROJECTION_PLANE_ORIGIN_HEIGHT)?,
            );
            boxed(ellipsoid, a, Origin::natural(&p)?, col_urban)
        }
        // PROJ 9.8 has no projection for these and refuses to export them to a PROJ string
        9816 | 9817 | 9826 | 9828 | 9830 => {
            return Err(Error::ProjectionError(format!(
                "EPSG method {method_code} has no PROJ implementation"
            )));
        }
        _ => {
            return Err(Error::ProjectionError(format!(
                "EPSG method {method_code} is not implemented"
            )));
        }
    };
    Ok(projection)
}
