// adapted from PROJ src/iso19111/io.cpp, crs.cpp, datum.cpp, coordinatesystem.cpp, common.cpp, metadata.cpp and operation/*.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::streaming_writer::StreamingWriter;
use crate::iso19111::common::{Measure, ObjectDomain, UnitOfMeasure, UnitType, leading_integer};
use crate::iso19111::coordinate_system::{CoordinateSystemAxis, Meridian};
use crate::iso19111::crs::{BoundCrs, Criterion, Crs, GeodeticCrs, GeographicCrs, ProjectedCrs};
use crate::iso19111::datum::{
    DatumEnsemble, Ellipsoid, EllipsoidShape, GeodeticDatum, GeodeticReferenceFrame, PrimeMeridian,
};
use crate::iso19111::metadata::Identifier;
use crate::iso19111::operation::{
    Conversion, OperationMethod, OperationParameterValue, ParameterValue, Transformation,
};

const PROJJSON_SCHEMA: &str = "https://proj.org/schemas/v0.7/projjson.schema.json";

// CPLJSonStreamingWriter's default precision for doubles
const DEFAULT_PRECISION: usize = 18;

struct Formatter {
    writer: StreamingWriter,
    stack_has_id: Vec<bool>,
    output_id_stack: Vec<bool>,
    allow_id_in_immediate_child: bool,
    omit_type_in_immediate_child: bool,
}

impl Formatter {
    fn new() -> Self {
        Self {
            writer: StreamingWriter::new(),
            stack_has_id: vec![false],
            output_id_stack: vec![true],
            allow_id_in_immediate_child: false,
            omit_type_in_immediate_child: false,
        }
    }

    fn output_id(&self) -> bool {
        *self
            .output_id_stack
            .last()
            .expect("output id stack starts non-empty")
    }

    fn output_usage(&self) -> bool {
        self.output_id() && self.output_id_stack.len() == 2
    }

    fn parent_has_id(&self) -> bool {
        *self.stack_has_id.last().expect("id stack starts non-empty")
    }

    fn object(&mut self, object_type: Option<&str>, has_id: bool, body: impl FnOnce(&mut Self)) {
        self.writer.start_object();
        if self.output_id_stack.len() == 1 {
            self.writer.add_key("$schema");
            self.writer.add_string(PROJJSON_SCHEMA);
        }
        if let Some(object_type) = object_type {
            if !self.omit_type_in_immediate_child {
                self.writer.add_key("type");
                self.writer.add_string(object_type);
            }
        }
        self.omit_type_in_immediate_child = false;
        // PROJ writes no id on an object whose parent has one, unless the parent allows it
        let root_outputs_id = self.output_id_stack[0];
        let output_id = if self.allow_id_in_immediate_child {
            self.allow_id_in_immediate_child = false;
            root_outputs_id
        } else {
            root_outputs_id && !self.parent_has_id()
        };
        self.output_id_stack.push(output_id);
        let has_id_in_stack = has_id || self.parent_has_id();
        self.stack_has_id.push(has_id_in_stack);

        body(self);

        self.writer.end_object();
        self.stack_has_id.pop();
        self.output_id_stack.pop();
    }

    fn array(&mut self, body: impl FnOnce(&mut Self)) {
        self.writer.start_array();
        body(self);
        self.writer.end_array();
    }

    fn key_string(&mut self, key: &str, value: &str) {
        self.writer.add_key(key);
        self.writer.add_string(value);
    }

    fn key_double(&mut self, key: &str, value: f64, precision: usize) {
        self.writer.add_key(key);
        self.writer.add_double(value, precision);
    }

    fn name_or_unnamed(&mut self, name: &str) {
        self.key_string("name", if name.is_empty() { "unnamed" } else { name });
    }
}

impl Crs {
    pub fn to_projjson(&self) -> String {
        let mut formatter = Formatter::new();
        write_crs(&mut formatter, self);
        formatter.writer.into_string()
    }
}

fn write_identifier(f: &mut Formatter, identifier: &Identifier) {
    if identifier.code_space.is_empty() || identifier.code.is_empty() {
        return;
    }
    f.object(None, false, |f| {
        f.key_string("authority", &identifier.code_space);
        f.writer.add_key("code");
        match leading_integer(&identifier.code) {
            Some(code) => f.writer.add_integer(i64::from(code)),
            None => f.writer.add_string(&identifier.code),
        }
        if let Some(version) = &identifier.version {
            f.writer.add_key("version");
            if version.parse::<f64>().is_ok_and(f64::is_finite) {
                f.writer.add_unquoted(version);
            } else {
                f.writer.add_string(version);
            }
        }
        if let Some(citation) = &identifier.authority_citation {
            if *citation != identifier.code_space {
                f.key_string("authority_citation", citation);
            }
        }
        if let Some(uri) = &identifier.uri {
            f.key_string("uri", uri);
        }
    });
}

fn write_ids(f: &mut Formatter, identifiers: &[Identifier]) {
    match identifiers {
        [] => {}
        [identifier] => {
            f.writer.add_key("id");
            write_identifier(f, identifier);
        }
        _ => {
            f.writer.add_key("ids");
            f.array(|f| {
                for identifier in identifiers {
                    write_identifier(f, identifier);
                }
            });
        }
    }
}

fn write_domain(f: &mut Formatter, domain: &ObjectDomain) {
    if let Some(scope) = &domain.scope {
        f.key_string("scope", scope);
    }
    let Some(extent) = &domain.domain_of_validity else {
        return;
    };
    if let Some(description) = &extent.description {
        f.key_string("area", description);
    }
    if let Some(bbox) = &extent.geographic_bounding_box {
        f.writer.add_key("bbox");
        f.writer.start_object();
        f.key_double("south_latitude", bbox.south_latitude, 15);
        f.key_double("west_longitude", bbox.west_longitude, 15);
        f.key_double("north_latitude", bbox.north_latitude, 15);
        f.key_double("east_longitude", bbox.east_longitude, 15);
        f.writer.end_object();
    }
    if let Some(vertical) = &extent.vertical_extent {
        f.writer.add_key("vertical_extent");
        f.writer.start_object();
        f.key_double("minimum", vertical.minimum, 15);
        f.key_double("maximum", vertical.maximum, 15);
        if !vertical.unit.has_same_name(&UnitOfMeasure::metre()) {
            f.writer.add_key("unit");
            write_unit(f, &vertical.unit);
        }
        f.writer.end_object();
    }
    if let Some(temporal) = &extent.temporal_extent {
        f.writer.add_key("temporal_extent");
        f.writer.start_object();
        f.key_string("start", &temporal.start);
        f.key_string("end", &temporal.end);
        f.writer.end_object();
    }
}

fn write_usage(
    f: &mut Formatter,
    domains: &[ObjectDomain],
    identifiers: &[Identifier],
    remarks: &str,
) {
    if f.output_usage() {
        match domains {
            [] => {}
            [domain] => write_domain(f, domain),
            _ => {
                f.writer.add_key("usages");
                f.array(|f| {
                    for domain in domains {
                        f.writer.start_object();
                        write_domain(f, domain);
                        f.writer.end_object();
                    }
                });
            }
        }
    }
    if f.output_id() {
        write_ids(f, identifiers);
    }
    if !remarks.is_empty() {
        f.key_string("remarks", remarks);
    }
}

fn write_unit(f: &mut Formatter, unit: &UnitOfMeasure) {
    f.object(None, !unit.code_space.is_empty(), |f| {
        let type_name = match unit.unit_type {
            UnitType::Linear => "LinearUnit",
            UnitType::Angular => "AngularUnit",
            UnitType::Scale => "ScaleUnit",
            UnitType::Time => "TimeUnit",
            UnitType::Parametric => "ParametricUnit",
            UnitType::Unknown | UnitType::None => "Unit",
        };
        f.key_string("type", type_name);
        f.key_string("name", &unit.name);
        f.key_double("conversion_factor", unit.conversion_to_si, 15);
        if !unit.code_space.is_empty() && f.output_id() {
            f.writer.add_key("id");
            f.object(None, false, |f| {
                f.key_string("authority", &unit.code_space);
                f.writer.add_key("code");
                match leading_integer(&unit.code) {
                    Some(code) => f.writer.add_integer(i64::from(code)),
                    None => f.writer.add_string(&unit.code),
                }
            });
        }
    });
}

fn write_value_in_unit(
    f: &mut Formatter,
    key: &str,
    measure: &Measure,
    implicit_unit: &UnitOfMeasure,
) {
    if measure.unit.has_same_name(implicit_unit) {
        f.key_double(key, measure.value, 15);
        return;
    }
    f.writer.add_key(key);
    f.object(None, false, |f| {
        f.key_double("value", measure.value, 15);
        f.writer.add_key("unit");
        write_unit(f, &measure.unit);
    });
}

fn write_ellipsoid(f: &mut Formatter, ellipsoid: &Ellipsoid) {
    f.object(Some("Ellipsoid"), !ellipsoid.identifiers.is_empty(), |f| {
        f.name_or_unnamed(&ellipsoid.name);
        let metre = UnitOfMeasure::metre();
        let semi_major_key = if ellipsoid.is_sphere() {
            "radius"
        } else {
            "semi_major_axis"
        };
        write_value_in_unit(f, semi_major_key, &ellipsoid.semi_major_axis, &metre);
        if !ellipsoid.is_sphere() {
            match &ellipsoid.shape {
                EllipsoidShape::InverseFlattening(inverse_flattening) => {
                    f.key_double("inverse_flattening", *inverse_flattening, 15);
                }
                EllipsoidShape::SemiMinorAxis(semi_minor_axis) => {
                    write_value_in_unit(f, "semi_minor_axis", semi_minor_axis, &metre);
                }
                EllipsoidShape::Sphere => {}
            }
        }
        if f.output_id() {
            write_ids(f, &ellipsoid.identifiers);
        }
    });
}

fn write_prime_meridian(f: &mut Formatter, prime_meridian: &PrimeMeridian) {
    f.object(
        Some("PrimeMeridian"),
        !prime_meridian.identifiers.is_empty(),
        |f| {
            f.key_string("name", &prime_meridian.name);
            write_value_in_unit(
                f,
                "longitude",
                &prime_meridian.longitude,
                &UnitOfMeasure::degree(),
            );
            if f.output_id() {
                write_ids(f, &prime_meridian.identifiers);
            }
        },
    );
}

fn rounded_epoch_in_decimal_year(year: f64) -> f64 {
    // PROJ rounds away the error of a year to second to year round trip
    if (1000.0 * year - (1000.0 * year).round()).abs() <= 1e-3 {
        return (1000.0 * year).round() / 1000.0;
    }
    year
}

fn write_reference_frame(f: &mut Formatter, frame: &GeodeticReferenceFrame) {
    let object_type = if frame.dynamic.is_some() {
        "DynamicGeodeticReferenceFrame"
    } else {
        "GeodeticReferenceFrame"
    };
    f.object(Some(object_type), !frame.identifiers.is_empty(), |f| {
        f.name_or_unnamed(&frame.name);
        if let Some(anchor) = &frame.anchor {
            f.key_string("anchor", anchor);
        }
        if let Some(anchor_epoch) = frame.anchor_epoch {
            f.key_double(
                "anchor_epoch",
                rounded_epoch_in_decimal_year(anchor_epoch),
                DEFAULT_PRECISION,
            );
        }
        if let Some(dynamic) = &frame.dynamic {
            f.key_double(
                "frame_reference_epoch",
                dynamic.frame_reference_epoch,
                DEFAULT_PRECISION,
            );
        }
        f.writer.add_key("ellipsoid");
        f.omit_type_in_immediate_child = true;
        write_ellipsoid(f, &frame.ellipsoid);
        if frame.prime_meridian.name != "Greenwich" {
            f.writer.add_key("prime_meridian");
            f.omit_type_in_immediate_child = true;
            write_prime_meridian(f, &frame.prime_meridian);
        }
        write_usage(f, &frame.domains, &frame.identifiers, &frame.remarks);
    });
}

fn write_ensemble(f: &mut Formatter, ensemble: &DatumEnsemble) {
    f.object(
        Some("DatumEnsemble"),
        !ensemble.identifiers.is_empty(),
        |f| {
            f.name_or_unnamed(&ensemble.name);
            f.writer.add_key("members");
            f.array(|f| {
                for member in &ensemble.members {
                    f.writer.start_object();
                    f.name_or_unnamed(&member.name);
                    write_ids(f, &member.identifiers);
                    f.writer.end_object();
                }
            });
            f.writer.add_key("ellipsoid");
            f.omit_type_in_immediate_child = true;
            write_ellipsoid(f, &ensemble.members[0].ellipsoid);
            f.key_string("accuracy", &ensemble.accuracy);
            write_ids(f, &ensemble.identifiers);
        },
    );
}

fn write_meridian(f: &mut Formatter, meridian: &Meridian) {
    f.object(Some("Meridian"), false, |f| {
        write_value_in_unit(
            f,
            "longitude",
            &meridian.longitude,
            &UnitOfMeasure::degree(),
        );
    });
}

fn write_axis(f: &mut Formatter, axis: &CoordinateSystemAxis) {
    f.object(Some("Axis"), !axis.identifiers.is_empty(), |f| {
        f.key_string("name", &axis.name);
        f.key_string("abbreviation", &axis.abbreviation);
        f.key_string("direction", axis.direction.as_str());
        if let Some(meridian) = &axis.meridian {
            f.writer.add_key("meridian");
            f.omit_type_in_immediate_child = true;
            write_meridian(f, meridian);
        }
        let unit = &axis.unit;
        if unit.has_same_name(&UnitOfMeasure::metre())
            || unit.has_same_name(&UnitOfMeasure::degree())
        {
            f.key_string("unit", &unit.name);
        } else if unit.unit_type != UnitType::None {
            f.writer.add_key("unit");
            write_unit(f, unit);
        }
        if let Some(minimum) = axis.minimum_value {
            f.key_double("minimum_value", minimum, DEFAULT_PRECISION);
        }
        if let Some(maximum) = axis.maximum_value {
            f.key_double("maximum_value", maximum, DEFAULT_PRECISION);
        }
        if let (Some(_), Some(_), Some(range_meaning)) =
            (axis.minimum_value, axis.maximum_value, axis.range_meaning)
        {
            f.key_string("range_meaning", range_meaning.as_str());
        }
        if f.output_id() {
            write_ids(f, &axis.identifiers);
        }
    });
}

fn write_coordinate_system(f: &mut Formatter, subtype: &str, axes: &[CoordinateSystemAxis]) {
    f.object(Some("CoordinateSystem"), false, |f| {
        f.key_string("subtype", subtype);
        f.writer.add_key("axis");
        f.array(|f| {
            for axis in axes {
                f.omit_type_in_immediate_child = true;
                write_axis(f, axis);
            }
        });
    });
}

struct GeodeticCrsParts<'a> {
    object_type: &'a str,
    name: &'a str,
    identifiers: &'a [Identifier],
    remarks: &'a str,
    domains: &'a [ObjectDomain],
    datum: &'a GeodeticDatum,
    coordinate_system_subtype: &'a str,
    axes: &'a [CoordinateSystemAxis],
}

fn write_geodetic_crs_parts(f: &mut Formatter, crs: &GeodeticCrsParts) {
    f.object(Some(crs.object_type), !crs.identifiers.is_empty(), |f| {
        f.name_or_unnamed(crs.name);
        match crs.datum {
            GeodeticDatum::ReferenceFrame(frame) => {
                f.writer.add_key("datum");
                write_reference_frame(f, frame);
            }
            GeodeticDatum::Ensemble(ensemble) => {
                f.writer.add_key("datum_ensemble");
                f.omit_type_in_immediate_child = true;
                write_ensemble(f, ensemble);
            }
        }
        f.writer.add_key("coordinate_system");
        f.omit_type_in_immediate_child = true;
        write_coordinate_system(f, crs.coordinate_system_subtype, crs.axes);
        if let GeodeticDatum::ReferenceFrame(frame) = crs.datum {
            let deformation_model = frame
                .dynamic
                .as_ref()
                .and_then(|dynamic| dynamic.deformation_model_name.as_deref());
            if let Some(deformation_model) = deformation_model {
                f.writer.add_key("deformation_models");
                f.array(|f| {
                    f.object(None, false, |f| f.key_string("name", deformation_model));
                });
            }
        }
        write_usage(f, crs.domains, crs.identifiers, crs.remarks);
    });
}

fn write_geographic_crs(f: &mut Formatter, crs: &GeographicCrs) {
    write_geodetic_crs_parts(
        f,
        &GeodeticCrsParts {
            object_type: "GeographicCRS",
            name: &crs.name,
            identifiers: &crs.identifiers,
            remarks: &crs.remarks,
            domains: &crs.domains,
            datum: &crs.datum,
            coordinate_system_subtype: "ellipsoidal",
            axes: &crs.coordinate_system.axes,
        },
    );
}

fn write_geodetic_crs(f: &mut Formatter, crs: &GeodeticCrs) {
    write_geodetic_crs_parts(
        f,
        &GeodeticCrsParts {
            object_type: "GeodeticCRS",
            name: &crs.name,
            identifiers: &crs.identifiers,
            remarks: &crs.remarks,
            domains: &crs.domains,
            datum: &crs.datum,
            coordinate_system_subtype: "Cartesian",
            axes: &crs.coordinate_system.axes,
        },
    );
}

fn write_method(f: &mut Formatter, method: &OperationMethod) {
    f.object(
        Some("OperationMethod"),
        !method.identifiers.is_empty(),
        |f| {
            f.key_string("name", &method.name);
            if f.output_id() {
                write_ids(f, &method.identifiers);
            }
        },
    );
}

fn write_parameter_value(f: &mut Formatter, parameter_value: &OperationParameterValue) {
    let parameter = &parameter_value.parameter;
    f.object(
        Some("ParameterValue"),
        !parameter.identifiers.is_empty(),
        |f| {
            f.key_string("name", &parameter.name);
            match &parameter_value.value {
                ParameterValue::Measure(measure) => {
                    f.key_double("value", measure.value, 15);
                    f.writer.add_key("unit");
                    let unit = &measure.unit;
                    let unit_has_short_name = unit.has_same_name(&UnitOfMeasure::metre())
                        || unit.has_same_name(&UnitOfMeasure::degree())
                        || unit.has_same_name(&UnitOfMeasure::scale_unity());
                    if unit_has_short_name {
                        f.writer.add_string(&unit.name);
                    } else {
                        write_unit(f, unit);
                    }
                }
                ParameterValue::Filename(filename) => f.key_string("value", filename),
                ParameterValue::Integer(value) => {
                    f.writer.add_key("value");
                    f.writer.add_integer(i64::from(*value));
                }
            }
            if f.output_id() {
                write_ids(f, &parameter.identifiers);
            }
        },
    );
}

fn write_parameter_values(f: &mut Formatter, parameter_values: &[OperationParameterValue]) {
    f.writer.add_key("parameters");
    f.array(|f| {
        for parameter_value in parameter_values {
            f.allow_id_in_immediate_child = true;
            f.omit_type_in_immediate_child = true;
            write_parameter_value(f, parameter_value);
        }
    });
}

fn write_conversion(f: &mut Formatter, conversion: &Conversion) {
    f.object(
        Some("Conversion"),
        !conversion.identifiers.is_empty(),
        |f| {
            f.name_or_unnamed(&conversion.name);
            f.writer.add_key("method");
            f.omit_type_in_immediate_child = true;
            f.allow_id_in_immediate_child = true;
            write_method(f, &conversion.method);
            if !conversion.parameter_values.is_empty() {
                write_parameter_values(f, &conversion.parameter_values);
            }
            if f.output_id() {
                write_ids(f, &conversion.identifiers);
            }
        },
    );
}

fn write_projected_crs(f: &mut Formatter, crs: &ProjectedCrs) {
    f.object(Some("ProjectedCRS"), !crs.identifiers.is_empty(), |f| {
        f.name_or_unnamed(&crs.name);
        f.writer.add_key("base_crs");
        f.allow_id_in_immediate_child = true;
        write_geographic_crs(f, &crs.base_crs);
        f.writer.add_key("conversion");
        f.omit_type_in_immediate_child = true;
        write_conversion(f, &crs.conversion);
        f.writer.add_key("coordinate_system");
        f.omit_type_in_immediate_child = true;
        write_coordinate_system(f, "Cartesian", &crs.coordinate_system.axes);
        write_usage(f, &crs.domains, &crs.identifiers, &crs.remarks);
    });
}

fn write_abridged_transformation(
    f: &mut Formatter,
    transformation: &Transformation,
    write_source_crs: bool,
) {
    f.object(
        Some("AbridgedTransformation"),
        !transformation.identifiers.is_empty(),
        |f| {
            f.name_or_unnamed(&transformation.name);
            if write_source_crs {
                f.writer.add_key("source_crs");
                f.allow_id_in_immediate_child = true;
                write_crs(f, &transformation.source_crs);
            }
            f.writer.add_key("method");
            f.omit_type_in_immediate_child = true;
            f.allow_id_in_immediate_child = true;
            write_method(f, &transformation.method);
            write_parameter_values(f, &transformation.parameter_values);
            if f.output_id() {
                write_ids(f, &transformation.identifiers);
            }
        },
    );
}

fn write_bound_crs(f: &mut Formatter, crs: &BoundCrs) {
    f.object(Some("BoundCRS"), false, |f| {
        if !crs.name.is_empty() && crs.name != crs.source_crs.name() {
            f.key_string("name", &crs.name);
        }
        f.writer.add_key("source_crs");
        write_crs(f, &crs.source_crs);
        f.writer.add_key("target_crs");
        write_crs(f, &crs.target_crs);
        f.writer.add_key("transformation");
        f.omit_type_in_immediate_child = true;
        // PROJ writes the transformation's source CRS only where it differs from the BoundCRS source
        let transformation_source = &*crs.transformation.source_crs;
        let source_differs = !crs.source_crs.is_equivalent_to(transformation_source, Criterion::Equivalent);
        let projected_base_differs = match (&*crs.source_crs, &*crs.target_crs) {
            (Crs::Projected(projected), Crs::Geographic(_)) => !matches!(
                transformation_source,
                Crs::Geographic(source) if projected.base_crs.is_equivalent_to(source, Criterion::Equivalent)
            ),
            (Crs::Projected(_), _) => false,
            _ => true,
        };
        write_abridged_transformation(f, &crs.transformation, source_differs && projected_base_differs);
        write_usage(f, &crs.domains, &crs.identifiers, &crs.remarks);
    });
}

fn write_crs(f: &mut Formatter, crs: &Crs) {
    match crs {
        Crs::Geographic(crs) => write_geographic_crs(f, crs),
        Crs::Geodetic(crs) => write_geodetic_crs(f, crs),
        Crs::Projected(crs) => write_projected_crs(f, crs),
        Crs::Bound(crs) => write_bound_crs(f, crs),
    }
}
