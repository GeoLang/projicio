// adapted from PROJ src/iso19111/crs.cpp, operation/singleoperation.cpp, operation/conversion.cpp and operation/transformation.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::candidate::{CandidateOperation, steps_of};
use crate::datum_transformation::{
    DatumTransformation, GeocentricConversion, GeodeticCoord, Helmert,
    geocentric_transformation_from_epsg_method, transformation_from_epsg_method,
};
use crate::iso19111::coordinate_system::{AxisDirection, CoordinateSystemAxis};
use crate::iso19111::operation::{OperationParameterValue, ParameterValue};
use crate::iso19111::{CoordinateOperation, Crs};
use crate::projection::epsg_method::math::adjlon;
use crate::{Coord, Ellipsoid, Error, GeocentricCoord, Geographic, Projection, from_epsg_method};

const METHOD_GEOGRAPHIC_GEOCENTRIC: u32 = 9602;
const METHOD_AXIS_ORDER_REVERSAL_2D: u32 = 9843;
const METHOD_AXIS_ORDER_REVERSAL_3D: u32 = 9844;
const METHOD_LONGITUDE_ROTATION: u32 = 9601;
const METHOD_GEOGRAPHIC2D_OFFSETS: u32 = 9619;
// singleoperation.cpp writes no PROJ string for the time-specific Helmert methods
const TRANSFORMATION_METHODS_WITHOUT_PROJ: [u32; 2] = [1065, 1066];
// PROJ 9.8 has no projection for these conversion methods
const CONVERSION_METHODS_WITHOUT_PROJ: [u32; 5] = [9816, 9817, 9826, 9828, 9830];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotExecutableKind {
    NoProjImplementation,
    MissingGrid,
    Unported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotExecutable {
    pub(crate) kind: NotExecutableKind,
    pub(crate) reason: String,
}

impl NotExecutable {
    fn unported(reason: String) -> Self {
        Self {
            kind: NotExecutableKind::Unported,
            reason,
        }
    }

    pub(crate) fn into_error(self, operation: &str) -> Error {
        Error::OperationNotExecutable {
            operation: operation.to_string(),
            reason: self.reason,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct GeographicAxes {
    latitude_first: bool,
    radians_per_unit: f64,
    // None for a 2D CRS, whose third coordinate passes through untouched
    metres_per_height_unit: Option<f64>,
    prime_meridian: f64,
    ellipsoid: Ellipsoid,
}

impl GeographicAxes {
    fn decode(&self, native: [f64; 3]) -> GeodeticCoord {
        let first = native[0] * self.radians_per_unit;
        let second = native[1] * self.radians_per_unit;
        let (longitude, latitude) = if self.latitude_first {
            (second, first)
        } else {
            (first, second)
        };
        GeodeticCoord {
            longitude,
            latitude,
            height: native[2] * self.metres_per_height_unit.unwrap_or(1.0),
        }
    }

    fn encode(&self, point: GeodeticCoord) -> [f64; 3] {
        let longitude = point.longitude / self.radians_per_unit;
        let latitude = point.latitude / self.radians_per_unit;
        let height = point.height / self.metres_per_height_unit.unwrap_or(1.0);
        if self.latitude_first {
            [latitude, longitude, height]
        } else {
            [longitude, latitude, height]
        }
    }

    fn relative_to_greenwich(&self, point: GeodeticCoord) -> GeodeticCoord {
        // proj leaves out the longlat step, and its longitude wrapping, on a Greenwich CRS
        if self.prime_meridian == 0.0 {
            return point;
        }
        GeodeticCoord {
            longitude: adjlon(point.longitude + self.prime_meridian),
            ..point
        }
    }

    fn relative_to_prime_meridian(&self, point: GeodeticCoord) -> GeodeticCoord {
        if self.prime_meridian == 0.0 {
            return point;
        }
        GeodeticCoord {
            longitude: adjlon(point.longitude - self.prime_meridian),
            ..point
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PlanarAxes {
    // native axis i holds signs[i] times easting (index 0) or northing (index 1)
    indices: [usize; 2],
    signs: [f64; 2],
    metres_per_unit: f64,
    metres_per_height_unit: Option<f64>,
}

impl PlanarAxes {
    fn decode(&self, native: [f64; 3]) -> [f64; 3] {
        let mut easting_northing = [0.0; 2];
        for ((index, sign), value) in self.indices.iter().zip(self.signs).zip(native) {
            easting_northing[*index] = value * self.metres_per_unit * sign;
        }
        [
            easting_northing[0],
            easting_northing[1],
            native[2] * self.metres_per_height_unit.unwrap_or(1.0),
        ]
    }

    fn encode(&self, easting_northing_height: [f64; 3]) -> [f64; 3] {
        let [first, second] = std::array::from_fn(|axis| {
            easting_northing_height[self.indices[axis]] * self.signs[axis] / self.metres_per_unit
        });
        [
            first,
            second,
            easting_northing_height[2] / self.metres_per_height_unit.unwrap_or(1.0),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CrsCoordinates {
    Geographic(GeographicAxes),
    Planar(PlanarAxes),
    Geocentric { metres_per_unit: f64 },
}

fn geographic_axes(crs: &Crs) -> Result<GeographicAxes, NotExecutable> {
    let geographic = match crs {
        Crs::Geographic(geographic) => geographic,
        Crs::Bound(bound) => return geographic_axes(&bound.source_crs),
        Crs::Geodetic(_) | Crs::Projected(_) => {
            return Err(NotExecutable::unported(format!(
                "{} is not a geographic CRS",
                crs.name()
            )));
        }
    };
    let axes = &geographic.coordinate_system.axes;
    let latitude_first = match (axes[0].direction, axes[1].direction) {
        (AxisDirection::North, AxisDirection::East) => true,
        (AxisDirection::East, AxisDirection::North) => false,
        (first, second) => {
            return Err(NotExecutable::unported(format!(
                "{} has {} and {} axes",
                crs.name(),
                first.as_str(),
                second.as_str()
            )));
        }
    };
    let datum = &geographic.datum;
    Ok(GeographicAxes {
        latitude_first,
        radians_per_unit: axes[0].unit.conversion_to_si,
        metres_per_height_unit: axes.get(2).map(|axis| axis.unit.conversion_to_si),
        prime_meridian: super::synthesized::prime_meridian(datum)
            .longitude
            .si_value(),
        ellipsoid: Ellipsoid::from(datum.ellipsoid()),
    })
}

fn planar_axes(axes: &[CoordinateSystemAxis]) -> PlanarAxes {
    // proj's ProjectedCRS::addUnitConvertAndAxisSwap
    let identity = ([0, 1], [1.0, 1.0]);
    let mapped = |direction: AxisDirection| match direction {
        AxisDirection::West => Some((0, -1.0)),
        AxisDirection::East => Some((0, 1.0)),
        AxisDirection::South => Some((1, -1.0)),
        AxisDirection::North => Some((1, 1.0)),
        _ => None,
    };
    let (first, second) = (axes[0].direction, axes[1].direction);
    let (indices, signs) = if first == AxisDirection::East && second == AxisDirection::North {
        identity
    } else if first != second {
        match (mapped(first), mapped(second)) {
            (Some((first_index, first_sign)), Some((second_index, second_sign))) => {
                ([first_index, second_index], [first_sign, second_sign])
            }
            _ => identity,
        }
    } else {
        // proj's case for EPSG:32661 and EPSG:32761, polar CRSs with two north or two south axes
        let northing_easting = axes[0].name.to_ascii_lowercase().starts_with("northing")
            && axes[1].name.to_ascii_lowercase().starts_with("easting");
        let polar = matches!(first, AxisDirection::North | AxisDirection::South);
        if polar && northing_easting {
            ([1, 0], [1.0, 1.0])
        } else {
            identity
        }
    };
    PlanarAxes {
        indices,
        signs,
        metres_per_unit: axes[0].unit.conversion_to_si,
        metres_per_height_unit: axes.get(2).map(|axis| axis.unit.conversion_to_si),
    }
}

fn crs_coordinates(crs: &Crs) -> Result<CrsCoordinates, NotExecutable> {
    match crs {
        Crs::Geographic(_) => Ok(CrsCoordinates::Geographic(geographic_axes(crs)?)),
        Crs::Projected(projected) => Ok(CrsCoordinates::Planar(planar_axes(
            &projected.coordinate_system.axes,
        ))),
        Crs::Geodetic(geodetic) => Ok(CrsCoordinates::Geocentric {
            metres_per_unit: geodetic.coordinate_system.axes[0].unit.conversion_to_si,
        }),
        Crs::Bound(bound) => crs_coordinates(&bound.source_crs),
    }
}

enum GeodeticEnd {
    Geographic {
        axes: GeographicAxes,
        conversion: GeocentricConversion,
    },
    Geocentric {
        metres_per_unit: f64,
    },
}

impl GeodeticEnd {
    fn new(crs: &Crs) -> Result<Self, NotExecutable> {
        match crs_coordinates(crs)? {
            CrsCoordinates::Geographic(axes) => Ok(GeodeticEnd::Geographic {
                axes,
                conversion: GeocentricConversion::new(&axes.ellipsoid)
                    .map_err(|error| NotExecutable::unported(error.to_string()))?,
            }),
            CrsCoordinates::Geocentric { metres_per_unit } => {
                Ok(GeodeticEnd::Geocentric { metres_per_unit })
            }
            CrsCoordinates::Planar(_) => Err(NotExecutable::unported(format!(
                "{} is a projected CRS",
                crs.name()
            ))),
        }
    }

    fn is_geographic_2d(&self) -> bool {
        matches!(self, GeodeticEnd::Geographic { axes, .. } if axes.metres_per_height_unit.is_none())
    }

    fn native_to_geocentric(&self, native: [f64; 3]) -> Result<GeocentricCoord, Error> {
        match self {
            GeodeticEnd::Geographic { axes, conversion } => {
                conversion.forward(axes.relative_to_greenwich(axes.decode(native)))
            }
            GeodeticEnd::Geocentric { metres_per_unit } => Ok(GeocentricCoord {
                x: native[0] * metres_per_unit,
                y: native[1] * metres_per_unit,
                z: native[2] * metres_per_unit,
            }),
        }
    }

    fn native_from_geocentric(&self, point: GeocentricCoord) -> [f64; 3] {
        match self {
            GeodeticEnd::Geographic { axes, conversion } => {
                axes.encode(axes.relative_to_prime_meridian(conversion.inverse(point)))
            }
            GeodeticEnd::Geocentric { metres_per_unit } => [
                point.x / metres_per_unit,
                point.y / metres_per_unit,
                point.z / metres_per_unit,
            ],
        }
    }
}

enum ExecutableStep {
    AxisOrder {
        source: GeographicAxes,
        target: GeographicAxes,
    },
    Projection {
        geographic: GeographicAxes,
        planar: PlanarAxes,
        projection: Box<dyn Projection + Send + Sync>,
        inverse: bool,
    },
    GeographicGeocentric {
        source: GeodeticEnd,
        target: GeodeticEnd,
    },
    GeographicDatumShift {
        source: GeographicAxes,
        target: GeographicAxes,
        transformation: Box<dyn DatumTransformation + Send + Sync>,
        inverse: bool,
        greenwich: bool,
        keep_input_height: bool,
    },
    GeocentricHelmert {
        source: GeodeticEnd,
        target: GeodeticEnd,
        helmert: Box<Helmert>,
        inverse: bool,
        keep_input_height: bool,
    },
}

impl ExecutableStep {
    fn run(&self, native: [f64; 3], epoch: Option<f64>) -> Result<[f64; 3], Error> {
        match self {
            ExecutableStep::AxisOrder { source, target } => {
                Ok(target.encode(source.decode(native)))
            }
            ExecutableStep::Projection {
                geographic,
                planar,
                projection,
                inverse: false,
            } => {
                let point = geographic.decode(native);
                let projected = projection.forward(Geographic::new(
                    point.longitude.to_degrees(),
                    point.latitude.to_degrees(),
                ))?;
                Ok(planar.encode([projected.x, projected.y, point.height]))
            }
            ExecutableStep::Projection {
                geographic,
                planar,
                projection,
                inverse: true,
            } => {
                let [easting, northing, height] = planar.decode(native);
                let point = projection.inverse(Coord::new(easting, northing))?;
                Ok(geographic.encode(GeodeticCoord {
                    longitude: point.lon.to_radians(),
                    latitude: point.lat.to_radians(),
                    height,
                }))
            }
            ExecutableStep::GeographicGeocentric { source, target } => {
                Ok(target.native_from_geocentric(source.native_to_geocentric(native)?))
            }
            ExecutableStep::GeographicDatumShift {
                source,
                target,
                transformation,
                inverse,
                greenwich,
                keep_input_height,
            } => {
                let mut point = source.decode(native);
                if *greenwich {
                    point = source.relative_to_greenwich(point);
                }
                let mut shifted = if *inverse {
                    transformation.inverse(point, epoch)?
                } else {
                    transformation.forward(point, epoch)?
                };
                if *greenwich {
                    shifted = target.relative_to_prime_meridian(shifted);
                }
                if *keep_input_height {
                    shifted.height = point.height;
                }
                Ok(target.encode(shifted))
            }
            ExecutableStep::GeocentricHelmert {
                source,
                target,
                helmert,
                inverse,
                keep_input_height,
            } => {
                let geocentric = source.native_to_geocentric(native)?;
                let shifted = if *inverse {
                    helmert.inverse(geocentric, epoch)?
                } else {
                    helmert.forward(geocentric, epoch)?
                };
                let mut output = target.native_from_geocentric(shifted);
                if *keep_input_height {
                    output[2] = native[2];
                }
                Ok(output)
            }
        }
    }
}

pub(crate) struct ExecutionPlan {
    steps: Vec<ExecutableStep>,
}

impl ExecutionPlan {
    pub(crate) fn new(candidate: &CandidateOperation) -> Result<Self, NotExecutable> {
        let mut steps = Vec::new();
        for step in flattened_steps(&candidate.operation) {
            steps.push(compile_step(step)?);
        }
        Ok(Self { steps })
    }

    pub(crate) fn run(&self, point: [f64; 3], epoch: Option<f64>) -> Result<[f64; 3], Error> {
        let mut native = point;
        for step in &self.steps {
            native = step.run(native, epoch)?;
        }
        Ok(native)
    }
}

fn flattened_steps(operation: &CoordinateOperation) -> Vec<&CoordinateOperation> {
    steps_of(operation)
        .iter()
        .flat_map(|step| match step {
            CoordinateOperation::Concatenated(_) => flattened_steps(step),
            single => vec![single],
        })
        .collect()
}

struct MethodCode {
    code: u32,
    inverse: bool,
}

fn method_code(
    identifiers: &[crate::iso19111::metadata::Identifier],
    method_name: &str,
) -> Result<MethodCode, NotExecutable> {
    let identifier = identifiers
        .first()
        .ok_or_else(|| NotExecutable::unported(format!("method {method_name} has no EPSG code")))?;
    let inverse = match identifier.code_space.as_str() {
        "EPSG" => false,
        "INVERSE(EPSG)" => true,
        other => {
            return Err(NotExecutable::unported(format!(
                "method {method_name} is coded by {other}"
            )));
        }
    };
    let code = identifier.code.parse().map_err(|_| {
        NotExecutable::unported(format!(
            "method {method_name} has the code {}",
            identifier.code
        ))
    })?;
    Ok(MethodCode { code, inverse })
}

fn si_parameters(parameter_values: &[OperationParameterValue]) -> Vec<(u32, f64)> {
    parameter_values
        .iter()
        .filter_map(|parameter_value| match &parameter_value.value {
            ParameterValue::Measure(measure) => {
                let code = u32::try_from(parameter_value.parameter.epsg_code()).ok()?;
                Some((code, measure.si_value()))
            }
            ParameterValue::Integer(_) | ParameterValue::Filename(_) => None,
        })
        .collect()
}

fn file_names(parameter_values: &[OperationParameterValue]) -> Vec<&str> {
    parameter_values
        .iter()
        .filter_map(|parameter_value| match &parameter_value.value {
            ParameterValue::Filename(file_name) => Some(file_name.as_str()),
            ParameterValue::Measure(_) | ParameterValue::Integer(_) => None,
        })
        .collect()
}

fn compile_step(step: &CoordinateOperation) -> Result<ExecutableStep, NotExecutable> {
    match step {
        CoordinateOperation::Conversion {
            conversion,
            source_crs,
            target_crs,
        } => {
            let method = method_code(&conversion.method.identifiers, &conversion.method.name)?;
            compile_conversion(
                method,
                &si_parameters(&conversion.parameter_values),
                source_crs,
                target_crs,
            )
        }
        CoordinateOperation::Transformation(transformation) => {
            let grids = file_names(&transformation.parameter_values);
            if !grids.is_empty() {
                return Err(NotExecutable {
                    kind: NotExecutableKind::MissingGrid,
                    reason: format!("grid {} is not readable", grids.join(", ")),
                });
            }
            let method = method_code(
                &transformation.method.identifiers,
                &transformation.method.name,
            )?;
            if TRANSFORMATION_METHODS_WITHOUT_PROJ.contains(&method.code) {
                return Err(NotExecutable {
                    kind: NotExecutableKind::NoProjImplementation,
                    reason: format!("EPSG method {} has no PROJ implementation", method.code),
                });
            }
            compile_transformation(
                method,
                &si_parameters(&transformation.parameter_values),
                &transformation.source_crs,
                &transformation.target_crs,
            )
        }
        CoordinateOperation::PointMotion(point_motion) => {
            let grids = file_names(&point_motion.parameter_values);
            Err(NotExecutable {
                kind: NotExecutableKind::MissingGrid,
                reason: format!("grid {} is not readable", grids.join(", ")),
            })
        }
        CoordinateOperation::Concatenated(concatenated) => Err(NotExecutable::unported(format!(
            "{} is a concatenated operation inside a step",
            concatenated.name
        ))),
    }
}

fn compile_conversion(
    method: MethodCode,
    parameters: &[(u32, f64)],
    source: &Crs,
    target: &Crs,
) -> Result<ExecutableStep, NotExecutable> {
    match method.code {
        METHOD_GEOGRAPHIC_GEOCENTRIC => Ok(ExecutableStep::GeographicGeocentric {
            source: GeodeticEnd::new(source)?,
            target: GeodeticEnd::new(target)?,
        }),
        METHOD_AXIS_ORDER_REVERSAL_2D | METHOD_AXIS_ORDER_REVERSAL_3D => {
            Ok(ExecutableStep::AxisOrder {
                source: geographic_axes(source)?,
                target: geographic_axes(target)?,
            })
        }
        code if CONVERSION_METHODS_WITHOUT_PROJ.contains(&code) => Err(NotExecutable {
            kind: NotExecutableKind::NoProjImplementation,
            reason: format!("EPSG method {code} has no PROJ implementation"),
        }),
        code => {
            let (geographic_crs, projected_crs) = if method.inverse {
                (target, source)
            } else {
                (source, target)
            };
            let geographic = geographic_axes(geographic_crs)?;
            let Crs::Projected(projected) = projected_crs else {
                return Err(NotExecutable::unported(format!(
                    "{} is not a projected CRS",
                    projected_crs.name()
                )));
            };
            let projection = from_epsg_method(code, parameters, &geographic.ellipsoid)
                .map_err(|error| NotExecutable::unported(error.to_string()))?;
            Ok(ExecutableStep::Projection {
                geographic,
                planar: planar_axes(&projected.coordinate_system.axes),
                projection,
                inverse: method.inverse,
            })
        }
    }
}

fn compile_transformation(
    method: MethodCode,
    parameters: &[(u32, f64)],
    source: &Crs,
    target: &Crs,
) -> Result<ExecutableStep, NotExecutable> {
    let unported = |error: Error| NotExecutable::unported(error.to_string());
    let both_geographic = matches!(
        (crs_coordinates(source)?, crs_coordinates(target)?),
        (CrsCoordinates::Geographic(_), CrsCoordinates::Geographic(_))
    );
    if both_geographic {
        let source_axes = geographic_axes(source)?;
        let target_axes = geographic_axes(target)?;
        let (forward_source_ellipsoid, forward_target_ellipsoid) = if method.inverse {
            (&target_axes.ellipsoid, &source_axes.ellipsoid)
        } else {
            (&source_axes.ellipsoid, &target_axes.ellipsoid)
        };
        let transformation = transformation_from_epsg_method(
            method.code,
            parameters,
            forward_source_ellipsoid,
            forward_target_ellipsoid,
        )
        .map_err(unported)?;
        // singleoperation.cpp exports these two with unit conversion and axis swap only, so the prime meridian stays
        let greenwich = !matches!(
            method.code,
            METHOD_LONGITUDE_ROTATION | METHOD_GEOGRAPHIC2D_OFFSETS
        );
        let keep_input_height = source_axes.metres_per_height_unit.is_none()
            || target_axes.metres_per_height_unit.is_none();
        return Ok(ExecutableStep::GeographicDatumShift {
            source: source_axes,
            target: target_axes,
            transformation,
            inverse: method.inverse,
            greenwich,
            keep_input_height,
        });
    }
    let helmert =
        geocentric_transformation_from_epsg_method(method.code, parameters).map_err(unported)?;
    let source = GeodeticEnd::new(source)?;
    let target = GeodeticEnd::new(target)?;
    // singleoperation.cpp pushes and pops the height around a Helmert step that touches a 2D CRS
    let keep_input_height = source.is_geographic_2d() || target.is_geographic_2d();
    Ok(ExecutableStep::GeocentricHelmert {
        source,
        target,
        helmert: Box::new(helmert),
        inverse: method.inverse,
        keep_input_height,
    })
}

pub(crate) fn proj_exportable(candidate: &CandidateOperation) -> bool {
    // proj writes a PROJ string for an operation whose grid is missing
    match ExecutionPlan::new(candidate) {
        Ok(_) => true,
        Err(error) => error.kind != NotExecutableKind::NoProjImplementation,
    }
}

pub(crate) fn pipeline_key(candidate: &CandidateOperation) -> Option<String> {
    // stands in for proj's exportToPROJString when removeDuplicateOps compares operations
    let mut key = String::new();
    for step in flattened_steps(&candidate.operation) {
        let (method, parameter_values) = match step {
            CoordinateOperation::Conversion { conversion, .. } => {
                (&conversion.method, &conversion.parameter_values)
            }
            CoordinateOperation::Transformation(transformation) => {
                (&transformation.method, &transformation.parameter_values)
            }
            CoordinateOperation::PointMotion(point_motion) => {
                (&point_motion.method, &point_motion.parameter_values)
            }
            CoordinateOperation::Concatenated(_) => return None,
        };
        let source = crs_coordinates(step.source_crs()).ok()?;
        let target = crs_coordinates(step.target_crs()).ok()?;
        key.push_str(&format!(
            "{:?}|{:?}|{:?}|{:?}|{source:?}|{target:?};",
            method.identifiers,
            method.name,
            si_parameters(parameter_values),
            file_names(parameter_values),
        ));
    }
    Some(key)
}

// proj_create_crs_to_crs places an area of use in a CRS with an operation from
// longitude and latitude in degrees on the CRS's own ellipsoid, and a geocentric
// CRS the other way round
pub(crate) struct GeographicToCrs(GeographicToCrsKind);

enum GeographicToCrsKind {
    Geographic(GeographicAxes),
    Projected {
        geographic: GeographicAxes,
        planar: PlanarAxes,
        projection: Box<dyn Projection + Send + Sync>,
    },
    Geocentric {
        metres_per_unit: f64,
        conversion: GeocentricConversion,
    },
}

impl GeographicToCrs {
    pub(crate) fn new(crs: &Crs) -> Result<Self, NotExecutable> {
        match crs {
            Crs::Geographic(_) => Ok(GeographicToCrs(GeographicToCrsKind::Geographic(
                geographic_axes(crs)?,
            ))),
            Crs::Projected(projected) => {
                let base = Crs::Geographic((*projected.base_crs).clone());
                let geographic = geographic_axes(&base)?;
                let conversion = &projected.conversion;
                let method = method_code(&conversion.method.identifiers, &conversion.method.name)?;
                let projection = from_epsg_method(
                    method.code,
                    &si_parameters(&conversion.parameter_values),
                    &geographic.ellipsoid,
                )
                .map_err(|error| NotExecutable::unported(error.to_string()))?;
                Ok(GeographicToCrs(GeographicToCrsKind::Projected {
                    geographic,
                    planar: planar_axes(&projected.coordinate_system.axes),
                    projection,
                }))
            }
            Crs::Geodetic(geodetic) => Ok(GeographicToCrs(GeographicToCrsKind::Geocentric {
                metres_per_unit: geodetic.coordinate_system.axes[0].unit.conversion_to_si,
                conversion: GeocentricConversion::new(&Ellipsoid::from(geodetic.datum.ellipsoid()))
                    .map_err(|error| NotExecutable::unported(error.to_string()))?,
            })),
            Crs::Bound(bound) => GeographicToCrs::new(&bound.source_crs),
        }
    }

    pub(crate) fn is_geocentric(&self) -> bool {
        matches!(self.0, GeographicToCrsKind::Geocentric { .. })
    }

    pub(crate) fn is_longitude_first_degree(&self) -> bool {
        // coord_operation.cpp compares the unit factor with 1e-14
        matches!(&self.0, GeographicToCrsKind::Geographic(axes)
            if !axes.latitude_first && (axes.radians_per_unit - std::f64::consts::PI / 180.0).abs() < 1e-14)
    }

    pub(crate) fn is_latitude_first_degree(&self) -> bool {
        matches!(&self.0, GeographicToCrsKind::Geographic(axes)
            if axes.latitude_first && (axes.radians_per_unit - std::f64::consts::PI / 180.0).abs() < 1e-14)
    }

    pub(crate) fn convert(&self, longitude: f64, latitude: f64) -> Option<[f64; 2]> {
        let greenwich = GeodeticCoord {
            longitude: longitude.to_radians(),
            latitude: latitude.to_radians(),
            height: 0.0,
        };
        match &self.0 {
            GeographicToCrsKind::Geographic(axes) => {
                let [x, y, _] = axes.encode(axes.relative_to_prime_meridian(greenwich));
                Some([x, y])
            }
            GeographicToCrsKind::Projected {
                geographic,
                planar,
                projection,
            } => {
                let point = geographic.relative_to_prime_meridian(greenwich);
                let projected = projection
                    .forward(Geographic::new(
                        point.longitude.to_degrees(),
                        point.latitude.to_degrees(),
                    ))
                    .ok()?;
                let [x, y, _] = planar.encode([projected.x, projected.y, 0.0]);
                Some([x, y])
            }
            GeographicToCrsKind::Geocentric { .. } => Some([longitude, latitude]),
        }
    }

    pub(crate) fn bounds_coordinates(&self, native: [f64; 3]) -> [f64; 2] {
        // proj_trans compares a geocentric point with an area of use in longitude and latitude
        match &self.0 {
            GeographicToCrsKind::Geocentric {
                metres_per_unit,
                conversion,
            } => {
                let point = conversion.inverse(GeocentricCoord {
                    x: native[0] * metres_per_unit,
                    y: native[1] * metres_per_unit,
                    z: native[2] * metres_per_unit,
                });
                [point.longitude.to_degrees(), point.latitude.to_degrees()]
            }
            GeographicToCrsKind::Geographic(_) | GeographicToCrsKind::Projected { .. } => {
                [native[0], native[1]]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iso19111::authority_factory::create_coordinate_operation;
    use crate::registry::OperationCode;

    fn plan_error(code: u32) -> NotExecutable {
        let operation = create_coordinate_operation(OperationCode::Epsg(code)).unwrap();
        match ExecutionPlan::new(&CandidateOperation::new(operation)) {
            Ok(_) => panic!("EPSG:{code} runs"),
            Err(error) => error,
        }
    }

    #[test]
    fn grid_operation_names_the_grid_it_cannot_read() {
        let error = plan_error(1313);
        assert_eq!(error.kind, NotExecutableKind::MissingGrid);
        assert!(
            error.reason.contains("ca_nrc_ntv2_0.tif"),
            "{}",
            error.reason
        );
        let message = error.into_error("NAD27 to NAD83 (4)").to_string();
        assert!(message.contains("NAD27 to NAD83 (4)"), "{message}");
    }

    #[test]
    fn time_specific_helmert_has_no_proj_implementation() {
        assert_eq!(
            plan_error(7702).kind,
            NotExecutableKind::NoProjImplementation
        );
    }
}
