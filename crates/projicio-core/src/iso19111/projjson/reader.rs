// adapted from PROJ src/iso19111/io.cpp (MIT), see LICENSES/PROJ-MIT.txt

use serde_json::{Map, Value};

use super::streaming_writer::format_general;
use crate::Error;
use crate::iso19111::common::{Measure, ObjectDomain, UnitOfMeasure, UnitType, leading_integer};
use crate::iso19111::coordinate_system::{
    AxisDirection, CartesianCs, CoordinateSystemAxis, EllipsoidalCs, Meridian, RangeMeaning,
};
use crate::iso19111::crs::{BoundCrs, Crs, GeodeticCrs, GeographicCrs, ProjectedCrs};
use crate::iso19111::datum::{
    DatumEnsemble, DynamicFrame, Ellipsoid, EllipsoidShape, GeodeticDatum, GeodeticReferenceFrame,
    PrimeMeridian,
};
use crate::iso19111::metadata::{
    EPSG, Extent, GeographicBoundingBox, Identifier, TemporalExtent, VerticalExtent,
};
use crate::iso19111::operation::{
    Conversion, OperationMethod, OperationParameter, OperationParameterValue,
    PARAMETER_EPSG_CODE_FOR_HORIZONTAL_CRS, PARAMETER_EPSG_CODE_FOR_INTERPOLATION_CRS,
    ParameterValue, Transformation,
};

type Object = Map<String, Value>;

fn parse_error(message: impl Into<String>) -> Error {
    Error::InvalidProjJson(message.into())
}

impl Crs {
    pub fn from_projjson(text: &str) -> Result<Self, Error> {
        let value: Value = serde_json::from_str(text).map_err(|e| parse_error(e.to_string()))?;
        let Value::Object(object) = &value else {
            return Err(parse_error("JSON object expected"));
        };
        build_crs(object)
    }
}

fn get<'a>(j: &'a Object, key: &str) -> Result<&'a Value, Error> {
    j.get(key)
        .ok_or_else(|| parse_error(format!("Missing \"{key}\" key")))
}

fn get_string(j: &Object, key: &str) -> Result<String, Error> {
    get(j, key)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| parse_error(format!("The value of \"{key}\" should be a string")))
}

fn get_optional_string(j: &Object, key: &str) -> Result<Option<String>, Error> {
    if j.contains_key(key) {
        return get_string(j, key).map(Some);
    }
    Ok(None)
}

fn get_object<'a>(j: &'a Object, key: &str) -> Result<&'a Object, Error> {
    get(j, key)?
        .as_object()
        .ok_or_else(|| parse_error(format!("The value of \"{key}\" should be a object")))
}

fn get_array<'a>(j: &'a Object, key: &str) -> Result<&'a Vec<Value>, Error> {
    get(j, key)?
        .as_array()
        .ok_or_else(|| parse_error(format!("The value of \"{key}\" should be a array")))
}

fn get_number(j: &Object, key: &str) -> Result<f64, Error> {
    get(j, key)?
        .as_f64()
        .ok_or_else(|| parse_error(format!("The value of \"{key}\" should be a number")))
}

fn get_integer(j: &Object, key: &str) -> Result<i32, Error> {
    let not_an_integer = || parse_error(format!("The value of \"{key}\" should be an integer"));
    let value = get(j, key)?.as_f64().ok_or_else(not_an_integer)?;
    let in_range = (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&value);
    if !in_range || f64::from(value as i32) != value {
        return Err(not_an_integer());
    }
    Ok(value as i32)
}

fn code_to_string(code: &Value) -> Result<String, Error> {
    if let Some(code) = code.as_str() {
        return Ok(code.to_string());
    }
    if code.is_i64() || code.is_u64() {
        return Ok(code.to_string());
    }
    Err(parse_error("Unexpected type for value of \"code\""))
}

fn get_unit(j: &Object, key: &str) -> Result<UnitOfMeasure, Error> {
    let value = get(j, key)?;
    if let Some(name) = value.as_str() {
        return [
            UnitOfMeasure::metre(),
            UnitOfMeasure::degree(),
            UnitOfMeasure::scale_unity(),
        ]
        .into_iter()
        .find(|unit| unit.name == name)
        .ok_or_else(|| parse_error(format!("Unknown unit name: {name}")));
    }
    let Some(unit) = value.as_object() else {
        return Err(parse_error(format!(
            "The value of \"{key}\" should be a string or an object"
        )));
    };
    let unit_type = match get_string(unit, "type")?.as_str() {
        "LinearUnit" => UnitType::Linear,
        "AngularUnit" => UnitType::Angular,
        "ScaleUnit" => UnitType::Scale,
        "TimeUnit" => UnitType::Time,
        "ParametricUnit" => UnitType::Parametric,
        "Unit" => UnitType::Unknown,
        _ => return Err(parse_error("Unsupported value of \"type\"")),
    };
    let name = get_string(unit, "name")?;
    let conversion_to_si = get_number(unit, "conversion_factor")?;
    // PROJ reads a unit's identifier from "authority" and "code" beside the name, not from "id"
    let (code_space, code) = if unit.contains_key("authority") && unit.contains_key("code") {
        (
            get_string(unit, "authority")?,
            code_to_string(&unit["code"])?,
        )
    } else {
        (String::new(), String::new())
    };
    Ok(UnitOfMeasure {
        name,
        conversion_to_si,
        unit_type,
        code_space,
        code,
    })
}

fn get_measure(j: &Object) -> Result<Measure, Error> {
    Ok(Measure::new(get_number(j, "value")?, get_unit(j, "unit")?))
}

fn get_value_in_unit(
    j: &Object,
    key: &str,
    implicit_unit: UnitOfMeasure,
) -> Result<Measure, Error> {
    let value = get(j, key)?;
    if let Some(number) = value.as_f64() {
        return Ok(Measure::new(number, implicit_unit));
    }
    if let Some(object) = value.as_object() {
        return get_measure(object);
    }
    Err(parse_error(format!(
        "Unexpected type for value of \"{key}\""
    )))
}

fn build_object_domain(j: &Object) -> Result<Option<ObjectDomain>, Error> {
    let scope = get_optional_string(j, "scope")?;
    let area = get_optional_string(j, "area")?.filter(|area| !area.is_empty());
    let geographic_bounding_box = if j.contains_key("bbox") {
        let bbox = get_object(j, "bbox")?;
        let south = get_number(bbox, "south_latitude")?;
        let west = get_number(bbox, "west_longitude")?;
        let north = get_number(bbox, "north_latitude")?;
        let east = get_number(bbox, "east_longitude")?;
        Some(GeographicBoundingBox::new(west, south, east, north)?)
    } else {
        None
    };
    let vertical_extent = if j.contains_key("vertical_extent") {
        let vertical = get_object(j, "vertical_extent")?;
        let unit = if vertical.contains_key("unit") {
            get_unit(vertical, "unit")?
        } else {
            UnitOfMeasure::metre()
        };
        Some(VerticalExtent {
            minimum: get_number(vertical, "minimum")?,
            maximum: get_number(vertical, "maximum")?,
            unit,
        })
    } else {
        None
    };
    let temporal_extent = if j.contains_key("temporal_extent") {
        let temporal = get_object(j, "temporal_extent")?;
        Some(TemporalExtent {
            start: get_string(temporal, "start")?,
            end: get_string(temporal, "end")?,
        })
    } else {
        None
    };

    let has_extent = area.is_some()
        || geographic_bounding_box.is_some()
        || vertical_extent.is_some()
        || temporal_extent.is_some();
    if scope.is_none() && !has_extent {
        return Ok(None);
    }
    let domain_of_validity = if has_extent {
        Some(Extent {
            description: area,
            geographic_bounding_box,
            vertical_extent,
            temporal_extent,
        })
    } else {
        None
    };
    Ok(Some(ObjectDomain {
        scope,
        domain_of_validity,
    }))
}

fn build_id(parent: &Object, j: &Object) -> Result<Identifier, Error> {
    let code_space = get_string(j, "authority")?;
    let version = match j.get("version") {
        None => None,
        Some(Value::String(version)) => Some(version.clone()),
        Some(version) => {
            let version = version
                .as_f64()
                .ok_or_else(|| parse_error("Unexpected type for value of \"version\""))?;
            let is_integer = (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&version)
                && f64::from(version as i32) == version;
            Some(if is_integer {
                (version as i32).to_string()
            } else {
                format_general(version, 15)
            })
        }
    };
    let mut code = code_to_string(get(j, "code")?)?;

    // PROJ before 9.5 wrote the wrong code for a synthesized "UTM Zone <n>S" conversion
    let parent_is_conversion = parent.get("type").and_then(Value::as_str) == Some("Conversion");
    if parent_is_conversion && code_space == EPSG {
        if let Some(parent_name) = parent.get("name").and_then(Value::as_str) {
            let prefix = "UTM Zone ";
            let starts_with_prefix = parent_name
                .get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix));
            if starts_with_prefix && parent_name.contains('S') {
                let zone = leading_integer(&parent_name[prefix.len()..]).unwrap_or(0);
                if (1..=60).contains(&zone) {
                    code = (16100 + zone).to_string();
                }
            }
        }
    }

    Ok(Identifier {
        code_space,
        code,
        version: version.filter(|version| !version.is_empty()),
        authority_citation: get_optional_string(j, "authority_citation")?,
        uri: get_optional_string(j, "uri")?,
    })
}

fn build_identifiers(j: &Object) -> Result<Vec<Identifier>, Error> {
    if j.contains_key("ids") {
        return get_array(j, "ids")?
            .iter()
            .map(|id| {
                let id = id
                    .as_object()
                    .ok_or_else(|| parse_error("Unexpected type for value of \"ids\" child"))?;
                build_id(j, id)
            })
            .collect();
    }
    if j.contains_key("id") {
        return Ok(vec![build_id(j, get_object(j, "id")?)?]);
    }
    Ok(Vec::new())
}

fn build_domains(j: &Object) -> Result<Vec<ObjectDomain>, Error> {
    if !j.contains_key("usages") {
        return Ok(build_object_domain(j)?.into_iter().collect());
    }
    let usages = j["usages"]
        .as_array()
        .ok_or_else(|| parse_error("Unexpected type for value of \"usages\""))?;
    usages
        .iter()
        .map(|usage| {
            let usage = usage
                .as_object()
                .ok_or_else(|| parse_error("Unexpected type for value of \"usages\" child"))?;
            build_object_domain(usage)?
                .ok_or_else(|| parse_error("missing children in \"usages\" child"))
        })
        .collect()
}

struct Usage {
    name: String,
    identifiers: Vec<Identifier>,
    remarks: String,
    domains: Vec<ObjectDomain>,
}

fn build_usage(j: &Object) -> Result<Usage, Error> {
    Ok(Usage {
        name: get_string(j, "name")?,
        identifiers: build_identifiers(j)?,
        remarks: get_optional_string(j, "remarks")?.unwrap_or_default(),
        domains: build_domains(j)?,
    })
}

fn build_crs(j: &Object) -> Result<Crs, Error> {
    let object_type = get_string(j, "type")?;
    match object_type.as_str() {
        "GeographicCRS" => Ok(Crs::Geographic(build_geographic_crs(j)?)),
        "GeodeticCRS" => Ok(Crs::Geodetic(build_geodetic_crs(j)?)),
        "ProjectedCRS" => Ok(Crs::Projected(build_projected_crs(j)?)),
        "BoundCRS" => Ok(Crs::Bound(build_bound_crs(j)?)),
        "VerticalCRS"
        | "CompoundCRS"
        | "EngineeringCRS"
        | "ParametricCRS"
        | "TemporalCRS"
        | "DerivedGeodeticCRS"
        | "DerivedGeographicCRS"
        | "DerivedProjectedCRS"
        | "DerivedVerticalCRS"
        | "DerivedEngineeringCRS"
        | "DerivedParametricCRS"
        | "DerivedTemporalCRS" => Err(parse_error(format!("{object_type} is not supported"))),
        _ => Err(parse_error("Object is not a CRS")),
    }
}

fn build_geodetic_datum(j: &Object) -> Result<GeodeticDatum, Error> {
    if !j.contains_key("datum") {
        return Ok(GeodeticDatum::Ensemble(build_datum_ensemble(get_object(
            j,
            "datum_ensemble",
        )?)?));
    }
    let datum = get_object(j, "datum")?;
    // PROJ handles one deformation model
    let deformation_model_name = if j.contains_key("deformation_models") {
        match get_array(j, "deformation_models")?.first() {
            Some(model) => {
                let model = model.as_object().ok_or_else(|| {
                    parse_error("The value of \"deformation_models\" should be a array of objects")
                })?;
                Some(get_string(model, "name")?)
            }
            None => None,
        }
    } else {
        None
    };
    let dynamic = match get_string(datum, "type")?.as_str() {
        "GeodeticReferenceFrame" => None,
        "DynamicGeodeticReferenceFrame" => {
            // PROJJSON before v0.5 put the model name on the datum
            let deformation_model_name =
                get_optional_string(datum, "deformation_model")?.or(deformation_model_name);
            Some(DynamicFrame {
                frame_reference_epoch: get_number(datum, "frame_reference_epoch")?,
                deformation_model_name,
            })
        }
        _ => return Err(parse_error("datum of wrong type")),
    };
    Ok(GeodeticDatum::ReferenceFrame(Box::new(
        build_reference_frame(datum, dynamic)?,
    )))
}

fn build_reference_frame(
    j: &Object,
    dynamic: Option<DynamicFrame>,
) -> Result<GeodeticReferenceFrame, Error> {
    let ellipsoid = build_ellipsoid(get_object(j, "ellipsoid")?)?;
    let prime_meridian = if j.contains_key("prime_meridian") {
        build_prime_meridian(get_object(j, "prime_meridian")?)?
    } else {
        PrimeMeridian::greenwich()
    };
    let anchor_epoch = if j.contains_key("anchor_epoch") {
        Some(get_number(j, "anchor_epoch")?)
    } else {
        None
    };
    let usage = build_usage(j)?;
    Ok(GeodeticReferenceFrame {
        name: usage.name,
        identifiers: usage.identifiers,
        remarks: usage.remarks,
        domains: usage.domains,
        anchor: get_optional_string(j, "anchor")?,
        // PROJ ignores anchor_epoch on a dynamic frame
        anchor_epoch: if dynamic.is_some() {
            None
        } else {
            anchor_epoch
        },
        ellipsoid,
        prime_meridian,
        dynamic,
    })
}

fn build_datum_ensemble(j: &Object) -> Result<DatumEnsemble, Error> {
    let name = get_string(j, "name")?;
    if !j.contains_key("members") {
        return Err(parse_error(format!(
            "Datum ensemble '{name}' has no explicit member specified and no connection to database"
        )));
    }
    if !j.contains_key("ellipsoid") {
        return Err(parse_error(format!(
            "Datum ensemble '{name}' has no ellipsoid, vertical datum ensembles are not supported"
        )));
    }
    let ellipsoid = build_ellipsoid(get_object(j, "ellipsoid")?)?;
    let members = get_array(j, "members")?
        .iter()
        .map(|member| {
            let member = member
                .as_object()
                .ok_or_else(|| parse_error("Unexpected type for value of a \"members\" member"))?;
            let usage = build_usage(member)?;
            Ok(GeodeticReferenceFrame {
                name: usage.name,
                identifiers: usage.identifiers,
                remarks: usage.remarks,
                domains: usage.domains,
                anchor: None,
                anchor_epoch: None,
                ellipsoid: ellipsoid.clone(),
                prime_meridian: PrimeMeridian::greenwich(),
                dynamic: None,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    if members.len() < 2 {
        return Err(parse_error("ensemble should have at least 2 datums"));
    }
    Ok(DatumEnsemble {
        name,
        identifiers: build_identifiers(j)?,
        members,
        accuracy: get_string(j, "accuracy")?,
    })
}

fn build_ellipsoid(j: &Object) -> Result<Ellipsoid, Error> {
    let name = get_string(j, "name")?;
    let identifiers = build_identifiers(j)?;
    if j.contains_key("semi_major_axis") {
        let semi_major_axis = get_value_in_unit(j, "semi_major_axis", UnitOfMeasure::metre())?;
        let shape = if j.contains_key("semi_minor_axis") {
            EllipsoidShape::SemiMinorAxis(get_value_in_unit(
                j,
                "semi_minor_axis",
                UnitOfMeasure::metre(),
            )?)
        } else if j.contains_key("inverse_flattening") {
            let inverse_flattening = get_number(j, "inverse_flattening")?;
            if inverse_flattening == 0.0 {
                EllipsoidShape::Sphere
            } else {
                EllipsoidShape::InverseFlattening(inverse_flattening)
            }
        } else {
            return Err(parse_error("Missing semi_minor_axis or inverse_flattening"));
        };
        return Ok(Ellipsoid {
            name,
            identifiers,
            semi_major_axis,
            shape,
        });
    }
    if j.contains_key("radius") {
        return Ok(Ellipsoid {
            name,
            identifiers,
            semi_major_axis: get_value_in_unit(j, "radius", UnitOfMeasure::metre())?,
            shape: EllipsoidShape::Sphere,
        });
    }
    Err(parse_error("Missing semi_major_axis or radius"))
}

fn build_prime_meridian(j: &Object) -> Result<PrimeMeridian, Error> {
    Ok(PrimeMeridian {
        name: get_string(j, "name")?,
        identifiers: build_identifiers(j)?,
        longitude: get_value_in_unit(j, "longitude", UnitOfMeasure::degree())?,
    })
}

fn build_axis(j: &Object) -> Result<CoordinateSystemAxis, Error> {
    let direction_name = get_string(j, "direction")?;
    let direction = AxisDirection::from_name(&direction_name)
        .ok_or_else(|| parse_error(format!("unhandled axis direction: {direction_name}")))?;
    let unit = if j.contains_key("unit") {
        get_unit(j, "unit")?
    } else {
        UnitOfMeasure::none()
    };
    let meridian = if j.contains_key("meridian") {
        let meridian = get_object(j, "meridian")?;
        Some(Meridian {
            longitude: get_value_in_unit(meridian, "longitude", UnitOfMeasure::degree())?,
        })
    } else {
        None
    };
    let range_meaning = match get_optional_string(j, "range_meaning")? {
        Some(name) => Some(RangeMeaning::from_name(&name).ok_or_else(|| {
            parse_error(format!("buildAxis: invalid range_meaning value: {name}"))
        })?),
        None => None,
    };
    let minimum_value = if j.contains_key("minimum_value") {
        Some(get_number(j, "minimum_value")?)
    } else {
        None
    };
    let maximum_value = if j.contains_key("maximum_value") {
        Some(get_number(j, "maximum_value")?)
    } else {
        None
    };
    Ok(CoordinateSystemAxis {
        name: get_string(j, "name")?,
        identifiers: build_identifiers(j)?,
        abbreviation: get_string(j, "abbreviation")?,
        direction,
        unit,
        minimum_value,
        maximum_value,
        range_meaning,
        meridian,
    })
}

enum CoordinateSystem {
    Ellipsoidal(EllipsoidalCs),
    Cartesian(CartesianCs),
}

fn build_coordinate_system(j: &Object) -> Result<CoordinateSystem, Error> {
    let subtype = get_string(j, "subtype")?;
    let axes = get(j, "axis")?
        .as_array()
        .ok_or_else(|| parse_error("Unexpected type for value of \"axis\""))?
        .iter()
        .map(|axis| {
            let axis = axis
                .as_object()
                .ok_or_else(|| parse_error("Unexpected type for value of a \"axis\" member"))?;
            build_axis(axis)
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let axis_count_is_valid = axes.len() == 2 || axes.len() == 3;
    match subtype.as_str() {
        "ellipsoidal" if axis_count_is_valid => {
            Ok(CoordinateSystem::Ellipsoidal(EllipsoidalCs { axes }))
        }
        "Cartesian" if axis_count_is_valid => Ok(CoordinateSystem::Cartesian(CartesianCs { axes })),
        "ellipsoidal" | "Cartesian" => Err(parse_error("Expected 2 or 3 axis")),
        "affine" | "vertical" | "spherical" | "ordinal" | "parametric" | "TemporalDateTime"
        | "TemporalCount" | "TemporalMeasure" => Err(parse_error(format!(
            "{subtype} coordinate systems are not supported"
        ))),
        _ => Err(parse_error("Unhandled value for subtype")),
    }
}

fn build_geographic_crs(j: &Object) -> Result<GeographicCrs, Error> {
    let datum = build_geodetic_datum(j)?;
    let CoordinateSystem::Ellipsoidal(coordinate_system) =
        build_coordinate_system(get_object(j, "coordinate_system")?)?
    else {
        return Err(parse_error("expected an ellipsoidal CS"));
    };
    let usage = build_usage(j)?;
    Ok(GeographicCrs {
        name: usage.name,
        identifiers: usage.identifiers,
        remarks: usage.remarks,
        domains: usage.domains,
        datum,
        coordinate_system,
    })
}

fn build_geodetic_crs(j: &Object) -> Result<GeodeticCrs, Error> {
    let datum = build_geodetic_datum(j)?;
    let CoordinateSystem::Cartesian(coordinate_system) =
        build_coordinate_system(get_object(j, "coordinate_system")?)?
    else {
        return Err(parse_error("expected a Cartesian CS"));
    };
    if coordinate_system.axes.len() != 3 {
        return Err(parse_error(
            "Cartesian CS for a GeodeticCRS should have 3 axis",
        ));
    }
    let usage = build_usage(j)?;
    Ok(GeodeticCrs {
        name: usage.name,
        identifiers: usage.identifiers,
        remarks: usage.remarks,
        domains: usage.domains,
        datum,
        coordinate_system,
    })
}

fn build_projected_crs(j: &Object) -> Result<ProjectedCrs, Error> {
    let base = get_object(j, "base_crs")?;
    let base_coordinate_system = build_coordinate_system(get_object(base, "coordinate_system")?)?;
    if !matches!(base_coordinate_system, CoordinateSystem::Ellipsoidal(_)) {
        return Err(parse_error(
            "a ProjectedCRS on a geocentric base CRS is not supported",
        ));
    }
    let base_crs = build_geographic_crs(base)?;
    let CoordinateSystem::Cartesian(coordinate_system) =
        build_coordinate_system(get_object(j, "coordinate_system")?)?
    else {
        return Err(parse_error("expected a Cartesian CS"));
    };
    let conversion = build_conversion(get_object(j, "conversion")?)?;
    let usage = build_usage(j)?;
    Ok(ProjectedCrs {
        name: usage.name,
        identifiers: usage.identifiers,
        remarks: usage.remarks,
        domains: usage.domains,
        base_crs: Box::new(base_crs),
        conversion,
        coordinate_system,
    })
}

fn build_method(j: &Object) -> Result<OperationMethod, Error> {
    Ok(OperationMethod {
        name: get_string(j, "name")?,
        identifiers: build_identifiers(j)?,
    })
}

fn build_parameter(j: &Value) -> Result<(&Object, OperationParameter), Error> {
    let j = j
        .as_object()
        .ok_or_else(|| parse_error("Unexpected type for a \"parameters\" child"))?;
    let parameter = OperationParameter {
        name: get_string(j, "name")?,
        identifiers: build_identifiers(j)?,
    };
    Ok((j, parameter))
}

fn build_conversion(j: &Object) -> Result<Conversion, Error> {
    let method = build_method(get_object(j, "method")?)?;
    let parameter_values = if j.contains_key("parameters") {
        get_array(j, "parameters")?
            .iter()
            .map(|parameter| {
                let (parameter_object, parameter) = build_parameter(parameter)?;
                let code = parameter.epsg_code();
                let is_crs_code = parameter.name == "EPSG code for Interpolation CRS"
                    || code == PARAMETER_EPSG_CODE_FOR_INTERPOLATION_CRS
                    || parameter.name == "EPSG code for Horizontal CRS"
                    || code == PARAMETER_EPSG_CODE_FOR_HORIZONTAL_CRS;
                let value = if is_crs_code {
                    ParameterValue::Integer(get_integer(parameter_object, "value")?)
                } else {
                    ParameterValue::Measure(get_measure(parameter_object)?)
                };
                Ok(OperationParameterValue { parameter, value })
            })
            .collect::<Result<Vec<_>, Error>>()?
    } else {
        Vec::new()
    };
    // TODO: PROJ builds the inverse conversion when both names start with "Inverse of "
    Ok(Conversion {
        name: get_string(j, "name")?,
        identifiers: build_identifiers(j)?,
        method,
        parameter_values,
    })
}

fn bound_crs_transformation_source(source_crs: &Crs, target_crs: &Crs) -> Result<Crs, Error> {
    if !matches!(target_crs, Crs::Geographic(_)) {
        return Ok(source_crs.clone());
    }
    let source_geographic = source_crs
        .extract_geographic_crs()
        .ok_or_else(|| parse_error("Cannot find GeographicCRS or VerticalCRS in sourceCRS"))?;
    let GeodeticDatum::ReferenceFrame(frame) = &source_geographic.datum else {
        return Ok(Crs::Geographic(source_geographic.clone()));
    };
    if frame.prime_meridian.longitude.si_value() == 0.0 {
        return Ok(Crs::Geographic(source_geographic.clone()));
    }
    let suffix = " (with Greenwich prime meridian)";
    Ok(Crs::Geographic(GeographicCrs {
        name: format!("{}{suffix}", source_geographic.name),
        identifiers: Vec::new(),
        remarks: String::new(),
        domains: Vec::new(),
        datum: GeodeticDatum::ReferenceFrame(Box::new(GeodeticReferenceFrame {
            name: format!("{}{suffix}", frame.name),
            identifiers: Vec::new(),
            remarks: String::new(),
            domains: Vec::new(),
            anchor: None,
            anchor_epoch: None,
            ellipsoid: frame.ellipsoid.clone(),
            prime_meridian: PrimeMeridian::greenwich(),
            dynamic: None,
        })),
        coordinate_system: source_geographic.coordinate_system.clone(),
    }))
}

fn build_bound_crs(j: &Object) -> Result<BoundCrs, Error> {
    let source_crs = build_crs(get_object(j, "source_crs")?)?;
    let target_crs = build_crs(get_object(j, "target_crs")?)?;
    let transformation_object = get_object(j, "transformation")?;
    let method = build_method(get_object(transformation_object, "method")?)?;
    let parameter_values = get_array(transformation_object, "parameters")?
        .iter()
        .map(|parameter| {
            let (parameter_object, parameter) = build_parameter(parameter)?;
            let value = match parameter_object.get("value") {
                Some(Value::String(filename)) => ParameterValue::Filename(filename.clone()),
                _ => ParameterValue::Measure(get_measure(parameter_object)?),
            };
            Ok(OperationParameterValue { parameter, value })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    // PROJ accepts an explicit source_crs on the transformation, PROJ issue 3428
    let transformation_source = if transformation_object.contains_key("source_crs") {
        build_crs(get_object(transformation_object, "source_crs")?)?
    } else {
        bound_crs_transformation_source(&source_crs, &target_crs)?
    };
    let transformation = Transformation {
        name: get_string(transformation_object, "name")?,
        identifiers: build_identifiers(transformation_object)?,
        source_crs: Box::new(transformation_source),
        target_crs: Box::new(target_crs.clone()),
        method,
        parameter_values,
    };
    let name = get_optional_string(j, "name")?.unwrap_or_else(|| source_crs.name().to_string());
    Ok(BoundCrs {
        name,
        identifiers: build_identifiers(j)?,
        remarks: get_optional_string(j, "remarks")?.unwrap_or_default(),
        domains: build_domains(j)?,
        source_crs: Box::new(source_crs),
        target_crs: Box::new(target_crs),
        transformation,
    })
}
