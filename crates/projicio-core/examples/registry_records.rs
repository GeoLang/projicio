use projicio_core::registry::{
    self, CoordinateSystemKind, CrsDefinitionKind, EllipsoidShape, GeodeticCrsType,
    GridFileReference, Measure, MeasuredVector, OperationKind, OperationStep, ParameterValue,
    TransformationParameters, UnitKind,
};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

fn measure(value: Option<Measure>) -> Value {
    value.map_or(
        Value::Null,
        |measure| json!({"value": measure.value, "unit_code": measure.unit_code}),
    )
}

fn vector(value: Option<MeasuredVector>) -> Value {
    value.map_or(
        Value::Null,
        |vector| json!({"values": vector.values, "unit_code": vector.unit_code}),
    )
}

fn parameters(values: &[ParameterValue<'_>]) -> Value {
    values
        .iter()
        .map(|parameter| {
            json!({
                "code": parameter.code,
                "name": parameter.name,
                "value": parameter.value,
                "unit_code": parameter.unit_code,
            })
        })
        .collect()
}

fn grids(values: &[GridFileReference<'_>]) -> Value {
    values
        .iter()
        .map(|grid| {
            json!({
                "parameter_code": grid.parameter_code,
                "parameter_name": grid.parameter_name,
                "file_name": grid.file_name,
            })
        })
        .collect()
}

fn transformation(kind: &str, values: &TransformationParameters<'_>) -> Value {
    json!({
        "kind": kind,
        "method_code": values.method.code,
        "method_name": values.method.name,
        "parameters": parameters(&values.parameters),
        "grids": grids(&values.grids),
        "interpolation_crs_code": values.interpolation_crs_code,
    })
}

fn operation_kind(kind: &OperationKind<'_>) -> Value {
    match kind {
        OperationKind::HelmertTransformation(helmert) => json!({
            "kind": "helmert",
            "method_code": helmert.method.code,
            "method_name": helmert.method.name,
            "translation": vector(Some(helmert.translation)),
            "rotation": vector(helmert.rotation),
            "scale_difference": measure(helmert.scale_difference),
            "rate_translation": vector(helmert.rate_translation),
            "rate_rotation": vector(helmert.rate_rotation),
            "rate_scale_difference": measure(helmert.rate_scale_difference),
            "epoch": measure(helmert.epoch),
            "pivot": vector(helmert.pivot),
        }),
        OperationKind::GridTransformation(values) => transformation("grid", values),
        OperationKind::OtherTransformation(values) => transformation("other", values),
        OperationKind::ConcatenatedOperation(steps) => json!({
            "kind": "concatenated",
            "steps": steps
                .iter()
                .map(|step| match step {
                    OperationStep::Operation(code) => json!(["operation", code]),
                    OperationStep::Conversion(code) => json!(["conversion", code]),
                })
                .collect::<Vec<_>>(),
        }),
    }
}

fn record(kind: &str, key: &str) -> Value {
    let code = || key.parse::<u32>().expect("numeric code");
    match kind {
        "unit" => registry::unit_of_measure(code()).map_or(Value::Null, |unit| {
            let unit_kind = match unit.kind {
                UnitKind::Length => "length",
                UnitKind::Angle => "angle",
                UnitKind::Scale => "scale",
                UnitKind::Time => "time",
            };
            json!({"name": unit.name, "kind": unit_kind, "to_si_factor": unit.to_si_factor})
        }),
        "ellipsoid" => registry::ellipsoid(code()).map_or(Value::Null, |ellipsoid| {
            let (inverse_flattening, semi_minor_axis) = match ellipsoid.shape {
                EllipsoidShape::InverseFlattening(value) => (Some(value), None),
                EllipsoidShape::SemiMinorAxis(value) => (None, Some(value)),
            };
            json!({
                "name": ellipsoid.name,
                "semi_major_axis": ellipsoid.semi_major_axis,
                "unit_code": ellipsoid.unit_code,
                "inverse_flattening": inverse_flattening,
                "semi_minor_axis": semi_minor_axis,
            })
        }),
        "prime_meridian" => registry::prime_meridian(code()).map_or(Value::Null, |meridian| {
            json!({
                "name": meridian.name,
                "longitude": meridian.longitude,
                "unit_code": meridian.unit_code,
            })
        }),
        "datum" => registry::geodetic_datum(code()).map_or(Value::Null, |datum| {
            json!({
                "name": datum.name,
                "ellipsoid_code": datum.ellipsoid_code,
                "prime_meridian_code": datum.prime_meridian_code,
                "frame_reference_epoch": datum.frame_reference_epoch,
                "ensemble": datum.ensemble.as_ref().map(|ensemble| json!({
                    "accuracy": ensemble.accuracy,
                    "member_codes": ensemble.member_codes,
                })),
            })
        }),
        "coordinate_system" => registry::coordinate_system(code()).map_or(Value::Null, |system| {
            let system_kind = match system.kind {
                CoordinateSystemKind::Cartesian => "Cartesian",
                CoordinateSystemKind::Ellipsoidal => "ellipsoidal",
            };
            json!({
                "kind": system_kind,
                "axes": system.axes.iter().map(|axis| json!({
                    "name": axis.name,
                    "abbreviation": axis.abbreviation,
                    "orientation": axis.orientation,
                    "unit_code": axis.unit_code,
                })).collect::<Vec<_>>(),
            })
        }),
        "crs" => registry::crs_definition(code()).map_or(Value::Null, |crs| {
            let kind = match crs.kind {
                CrsDefinitionKind::Geodetic {
                    geodetic_type,
                    datum_code,
                } => {
                    let geodetic_type = match geodetic_type {
                        GeodeticCrsType::Geographic2D => "geographic 2D",
                        GeodeticCrsType::Geographic3D => "geographic 3D",
                        GeodeticCrsType::Geocentric => "geocentric",
                    };
                    json!({"type": geodetic_type, "datum_code": datum_code})
                }
                CrsDefinitionKind::Projected {
                    base_crs_code,
                    conversion_code,
                } => json!({
                    "type": "projected",
                    "base_crs_code": base_crs_code,
                    "conversion_code": conversion_code,
                }),
            };
            json!({
                "kind": kind,
                "coordinate_system_code": crs.coordinate_system_code,
                "extent_codes": crs.extent_codes,
            })
        }),
        "conversion" => registry::conversion(code()).map_or(Value::Null, |conversion| {
            json!({
                "name": conversion.name,
                "method_code": conversion.method.code,
                "method_name": conversion.method.name,
                "parameters": parameters(&conversion.parameters),
            })
        }),
        "operation" => registry::coordinate_operation(code()).map_or(Value::Null, |operation| {
            json!({
                "name": operation.name,
                "source_crs_code": operation.source_crs_code,
                "target_crs_code": operation.target_crs_code,
                "accuracy": operation.accuracy,
                "extent_codes": operation.extent_codes,
                "kind": operation_kind(&operation.kind),
            })
        }),
        "supersession" => registry::supersessions(code())
            .iter()
            .map(|supersession| {
                json!({
                    "replacement_code": supersession.replacement_code,
                    "same_source_target_crs": supersession.same_source_target_crs,
                })
            })
            .collect(),
        "grid" => registry::grid_alternative(key).map_or(Value::Null, |grid| {
            json!({
                "proj_grid_name": grid.proj_grid_name,
                "old_proj_grid_name": grid.old_proj_grid_name,
                "format": grid.format,
                "proj_method": grid.proj_method,
                "inverse_direction": grid.inverse_direction,
                "url": grid.url,
                "open_license": grid.open_license,
                "direct_download": grid.direct_download,
            })
        }),
        other => panic!("unknown record kind {other}"),
    }
}

fn main() {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in io::stdin().lock().lines() {
        let line = line.expect("stdin");
        let (kind, key) = line.split_once(' ').expect("a line is `<kind> <key>`");
        writeln!(out, "{}", record(kind, key)).expect("stdout");
    }
}
