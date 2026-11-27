// adapted from PROJ src/iso19111/operation/singleoperation.cpp, src/iso19111/datum.cpp, src/datums.cpp, src/transformations/affine.cpp, src/projections/latlong.cpp, src/fwd.cpp and src/inv.cpp (MIT), see LICENSES/PROJ-MIT.txt

mod cart;
mod helmert;

pub use cart::GeocentricConversion;
pub use helmert::Helmert;

use crate::projection::epsg_method::Parameters;
use crate::projection::epsg_method::math::{adjlon, prepare_angular_input};
use crate::{Ellipsoid, Error};
use helmert::{HelmertTerms, RotationConvention, RotationForm};

const LONGITUDE_ROTATION: u32 = 9601;
const GEOGRAPHIC2D_OFFSETS: u32 = 9619;
const LATITUDE_OFFSET: u32 = 8601;
const LONGITUDE_OFFSET: u32 = 8602;

const HELMERT_METHODS: [(u32, HelmertTerms); 24] = {
    use HelmertTerms::{MolodenskyBadekas, SevenParameter, TimeDependent, Translation};
    use RotationConvention::{CoordinateFrame, PositionVector};
    use RotationForm::{Exact, SmallAngle};
    [
        (1031, Translation),
        (9603, Translation),
        (1035, Translation),
        (1033, SevenParameter(PositionVector, SmallAngle)),
        (9606, SevenParameter(PositionVector, SmallAngle)),
        (1037, SevenParameter(PositionVector, SmallAngle)),
        (1032, SevenParameter(CoordinateFrame, SmallAngle)),
        (9607, SevenParameter(CoordinateFrame, SmallAngle)),
        (1038, SevenParameter(CoordinateFrame, SmallAngle)),
        (1132, SevenParameter(CoordinateFrame, Exact)),
        (1133, SevenParameter(CoordinateFrame, Exact)),
        (1140, SevenParameter(CoordinateFrame, Exact)),
        (1053, TimeDependent(PositionVector)),
        (1054, TimeDependent(PositionVector)),
        (1055, TimeDependent(PositionVector)),
        (1056, TimeDependent(CoordinateFrame)),
        (1057, TimeDependent(CoordinateFrame)),
        (1058, TimeDependent(CoordinateFrame)),
        (1061, MolodenskyBadekas(PositionVector)),
        (1062, MolodenskyBadekas(PositionVector)),
        (1063, MolodenskyBadekas(PositionVector)),
        (1034, MolodenskyBadekas(CoordinateFrame)),
        (1039, MolodenskyBadekas(CoordinateFrame)),
        (9636, MolodenskyBadekas(CoordinateFrame)),
    ]
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeodeticCoord {
    pub longitude: f64,
    pub latitude: f64,
    pub height: f64,
}

pub trait DatumTransformation {
    fn forward(&self, point: GeodeticCoord, epoch: Option<f64>) -> Result<GeodeticCoord, Error>;
    fn inverse(&self, point: GeodeticCoord, epoch: Option<f64>) -> Result<GeodeticCoord, Error>;
}

struct GeographicHelmert {
    source: GeocentricConversion,
    helmert: Helmert,
    target: GeocentricConversion,
}

impl DatumTransformation for GeographicHelmert {
    fn forward(&self, point: GeodeticCoord, epoch: Option<f64>) -> Result<GeodeticCoord, Error> {
        let geocentric = self.source.forward(point)?;
        let shifted = self.helmert.forward(geocentric, epoch)?;
        Ok(self.target.inverse(shifted))
    }

    fn inverse(&self, point: GeodeticCoord, epoch: Option<f64>) -> Result<GeodeticCoord, Error> {
        let geocentric = self.target.forward(point)?;
        let shifted = self.helmert.inverse(geocentric, epoch)?;
        Ok(self.source.inverse(shifted))
    }
}

struct LongitudeRotation {
    offset: f64,
}

impl DatumTransformation for LongitudeRotation {
    fn forward(&self, point: GeodeticCoord, _epoch: Option<f64>) -> Result<GeodeticCoord, Error> {
        // PROJ runs this as an inverse latlong step, and inv.cpp adds its prime meridian without a latitude check
        Ok(GeodeticCoord {
            longitude: adjlon(point.longitude + self.offset),
            ..point
        })
    }

    fn inverse(&self, point: GeodeticCoord, _epoch: Option<f64>) -> Result<GeodeticCoord, Error> {
        let prepared = prepare_angular_input(point.longitude, point.latitude)?;
        Ok(GeodeticCoord {
            longitude: adjlon(prepared.lambda - self.offset),
            latitude: prepared.phi,
            height: point.height,
        })
    }
}

struct GeographicOffsets {
    latitude: f64,
    longitude: f64,
}

impl DatumTransformation for GeographicOffsets {
    fn forward(&self, point: GeodeticCoord, _epoch: Option<f64>) -> Result<GeodeticCoord, Error> {
        let prepared = prepare_angular_input(point.longitude, point.latitude)?;
        Ok(GeodeticCoord {
            longitude: prepared.lambda + self.longitude,
            latitude: prepared.phi + self.latitude,
            height: point.height,
        })
    }

    fn inverse(&self, point: GeodeticCoord, _epoch: Option<f64>) -> Result<GeodeticCoord, Error> {
        Ok(GeodeticCoord {
            longitude: adjlon(point.longitude - self.longitude),
            latitude: point.latitude - self.latitude,
            height: point.height,
        })
    }
}

// datums.cpp's prime meridians in its order, greenwich first
const PROJ_PRIME_MERIDIAN_DEGREES: [f64; 14] = [
    0.0,
    -(9.0 + 7.0 / 60.0 + 54.862 / 3600.0),
    2.0 + 20.0 / 60.0 + 14.025 / 3600.0,
    -(74.0 + 4.0 / 60.0 + 51.3 / 3600.0),
    -(3.0 + 41.0 / 60.0 + 16.58 / 3600.0),
    12.0 + 27.0 / 60.0 + 8.4 / 3600.0,
    7.0 + 26.0 / 60.0 + 22.5 / 3600.0,
    106.0 + 48.0 / 60.0 + 27.79 / 3600.0,
    -(17.0 + 40.0 / 60.0),
    4.0 + 22.0 / 60.0 + 4.71 / 3600.0,
    18.0 + 3.0 / 60.0 + 29.8 / 3600.0,
    23.0 + 42.0 / 60.0 + 58.815 / 3600.0,
    10.0 + 43.0 / 60.0 + 22.5 / 3600.0,
    12.0 + 34.0 / 60.0 + 40.35 / 3600.0,
];

fn proj_longitude_offset(offset: f64) -> f64 {
    // getPROJStringWellKnownName swaps in a named meridian, or its negation, for any offset within 1e-10 radians of it
    PROJ_PRIME_MERIDIAN_DEGREES
        .iter()
        .flat_map(|degrees| [-degrees.to_radians(), degrees.to_radians()])
        .find(|meridian| (offset - meridian).abs() < 1e-10)
        .unwrap_or(offset)
}

fn same_ellipsoid(source: &Ellipsoid, target: &Ellipsoid) -> bool {
    // datum.cpp compares ellipsoids with a relative tolerance of 1e-10
    (source.a - target.a).abs() <= 1e-10 * source.a.abs()
        && (source.f - target.f).abs() <= 1e-10 * source.f.abs()
}

pub fn geocentric_transformation_from_epsg_method(
    method_code: u32,
    parameters: &[(u32, f64)],
) -> Result<Helmert, Error> {
    let terms = HELMERT_METHODS
        .iter()
        .find(|(code, _)| *code == method_code)
        .map(|(_, terms)| *terms);
    if let Some(terms) = terms {
        let parameters = Parameters {
            method_code,
            values: parameters,
        };
        return Helmert::from_epsg_parameters(&parameters, terms);
    }
    let reason = match method_code {
        LONGITUDE_ROTATION | GEOGRAPHIC2D_OFFSETS => "works on geographic coordinates only",
        // singleoperation.cpp writes no PROJ string for the time-specific Helmert methods
        1065 | 1066 => "has no PROJ implementation",
        _ => "is not implemented",
    };
    Err(Error::ProjectionError(format!(
        "EPSG method {method_code} {reason}"
    )))
}

pub fn transformation_from_epsg_method(
    method_code: u32,
    parameters: &[(u32, f64)],
    source_ellipsoid: &Ellipsoid,
    target_ellipsoid: &Ellipsoid,
) -> Result<Box<dyn DatumTransformation + Send + Sync>, Error> {
    let epsg_parameters = Parameters {
        method_code,
        values: parameters,
    };
    match method_code {
        LONGITUDE_ROTATION => {
            // singleoperation.cpp refuses a longitude rotation between two ellipsoids
            if !same_ellipsoid(source_ellipsoid, target_ellipsoid) {
                return Err(Error::ProjectionError(format!(
                    "EPSG method {method_code} needs the same ellipsoid on both sides"
                )));
            }
            Ok(Box::new(LongitudeRotation {
                offset: proj_longitude_offset(epsg_parameters.get(LONGITUDE_OFFSET)?),
            }))
        }
        GEOGRAPHIC2D_OFFSETS => Ok(Box::new(GeographicOffsets {
            latitude: epsg_parameters.get(LATITUDE_OFFSET)?,
            longitude: epsg_parameters.get(LONGITUDE_OFFSET)?,
        })),
        _ => {
            let helmert = geocentric_transformation_from_epsg_method(method_code, parameters)?;
            Ok(Box::new(GeographicHelmert {
                source: GeocentricConversion::new(source_ellipsoid)?,
                helmert,
                target: GeocentricConversion::new(target_ellipsoid)?,
            }))
        }
    }
}
