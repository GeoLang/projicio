// adapted from PROJ src/iso19111/crs.cpp, datum.cpp, coordinatesystem.cpp, common.cpp, metadata.cpp and operation/*.cpp _exportToWKT (MIT), see LICENSES/PROJ-MIT.txt

use super::database::{
    datum_with_esri_vertical_crs, esri_alias_from_official_name, esri_alias_of_code,
    esri_alias_through_epsg_old_name, esri_projected_definition, is_single_esri_object_name,
    registered_geodetic_datum, registered_geographic_crs_with_axes,
};
use super::formatter::{Formatter, OutputAxisRule, morph_name_to_esri};
use super::mappings::{
    EsriParamMapping, MethodMapping, PARAMS_ESRI_EQUIDISTANT_CYLINDRICAL, PARAMS_ESRI_GAUSS_KRUGER,
    PARAMS_ESRI_HOTINE_OBLIQUE_MERCATOR_AZIMUTH_CENTER,
    PARAMS_ESRI_HOTINE_OBLIQUE_MERCATOR_AZIMUTH_NATURAL_ORIGIN, PARAMS_ESRI_PLATE_CARREE,
    PARAMS_ESRI_RECTIFIED_SKEW_ORTHOMORPHIC_CENTER,
    PARAMS_ESRI_RECTIFIED_SKEW_ORTHOMORPHIC_NATURAL_ORIGIN, PARAMS_ESRI_TRANSVERSE_MERCATOR,
};
use super::methods::{
    METHOD_EQUIDISTANT_CYLINDRICAL, METHOD_EQUIDISTANT_CYLINDRICAL_SPHERICAL,
    METHOD_HOTINE_OBLIQUE_MERCATOR_VARIANT_A, METHOD_HOTINE_OBLIQUE_MERCATOR_VARIANT_B,
    METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA, METHOD_MERCATOR_SPHERICAL, METHOD_MERCATOR_VARIANT_A,
    METHOD_MERCATOR_VARIANT_B, METHOD_NADCON, METHOD_NTV2, METHOD_POLAR_STEREOGRAPHIC_VARIANT_A,
    METHOD_POLAR_STEREOGRAPHIC_VARIANT_B, METHOD_POPULAR_VISUALISATION_PSEUDO_MERCATOR,
    METHOD_TRANSVERSE_MERCATOR, METHOD_VERTICAL_PERSPECTIVE,
    PARAMETER_ANGLE_RECTIFIED_TO_SKEW_GRID, PARAMETER_AZIMUTH_PROJECTION_CENTRE,
    PARAMETER_FALSE_EASTING, PARAMETER_FALSE_NORTHING, PARAMETER_LATITUDE_1ST_STD_PARALLEL,
    PARAMETER_LATITUDE_DIFFERENCE_FILE, PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE,
    PARAMETER_LATITUDE_OF_NATURAL_ORIGIN, PARAMETER_LATITUDE_STD_PARALLEL,
    PARAMETER_LONGITUDE_OF_NATURAL_ORIGIN, esri_mapping, mapping_by_code, mapping_for_method,
    method_epsg_code, parameter_epsg_code, parameter_mapping, parameter_si_value,
    parameter_value_by_code, parameter_value_by_name_or_code, parameter_value_in_unit,
};
use super::node::WktNode;
use super::{WktDialect, WktLayout};
use crate::Error;
use crate::iso19111::common::{ObjectDomain, UnitOfMeasure, UnitType, leading_integer};
use crate::iso19111::coordinate_system::{
    AxisDirection, CartesianCs, CoordinateSystemAxis, EllipsoidalCs, Meridian,
};
use crate::iso19111::crs::{BoundCrs, Criterion, Crs, GeodeticCrs, GeographicCrs, ProjectedCrs};
use crate::iso19111::datum::{
    DatumEnsemble, Ellipsoid, GeodeticDatum, GeodeticReferenceFrame, PrimeMeridian,
};
use crate::iso19111::metadata::Identifier;
use crate::iso19111::operation::{
    Conversion, OperationMethod, OperationParameterValue, ParameterValue, Transformation,
    towgs84_parameters,
};
use crate::iso19111::projjson::rounded_epoch_in_decimal_year;

fn formatting_error(message: impl Into<String>) -> Error {
    Error::UnsupportedWkt(message.into())
}

impl Crs {
    pub fn to_wkt(&self, dialect: WktDialect, layout: WktLayout) -> Result<String, Error> {
        let mut formatter = Formatter::new(dialect, layout);
        write_crs(&mut formatter, self)?;
        Ok(formatter.into_string())
    }
}

fn write_crs(f: &mut Formatter, crs: &Crs) -> Result<(), Error> {
    match crs {
        Crs::Geographic(crs) => write_geodetic_crs(f, &GeodeticCrsParts::geographic(crs)),
        Crs::Geodetic(crs) => write_geodetic_crs(f, &GeodeticCrsParts::geocentric(crs)),
        Crs::Projected(crs) => write_projected_crs(f, crs),
        Crs::Bound(crs) => write_bound_crs(f, crs),
    }
}

fn write_identifier(f: &mut Formatter, identifier: &Identifier) {
    // Identifier::_exportToWKT
    if identifier.code_space.is_empty() || identifier.code.is_empty() {
        return;
    }
    if !f.is_wkt2() {
        f.start_node("AUTHORITY", false);
        f.add_quoted_string(&identifier.code_space);
        f.add_quoted_string(&identifier.code);
        f.end_node();
        return;
    }
    f.start_node("ID", false);
    f.add_quoted_string(&identifier.code_space);
    // std::stoi accepts a code that starts with an integer
    if leading_integer(&identifier.code).is_some() {
        f.add_raw(&identifier.code);
    } else {
        f.add_quoted_string(&identifier.code);
    }
    if let Some(version) = identifier
        .version
        .as_deref()
        .filter(|version| !version.is_empty())
    {
        if super::node::as_double(version).is_ok() {
            f.add_raw(version);
        } else {
            f.add_quoted_string(version);
        }
    }
    if let Some(citation) = &identifier.authority_citation {
        if *citation != identifier.code_space {
            f.start_node("CITATION", false);
            f.add_quoted_string(citation);
            f.end_node();
        }
    }
    if let Some(uri) = &identifier.uri {
        f.start_node("URI", false);
        f.add_quoted_string(uri);
        f.end_node();
    }
    f.end_node();
}

fn write_ids(f: &mut Formatter, identifiers: &[Identifier]) {
    // IdentifiedObject::formatID writes the first identifier only in WKT1
    let count = if f.is_wkt2() { identifiers.len() } else { 1 };
    for identifier in identifiers.iter().take(count) {
        write_identifier(f, identifier);
    }
}

fn is_iso_8601(text: &str) -> bool {
    // DateTime::isISO_8601: a date, optionally followed by a time
    let bytes = text.as_bytes();
    bytes.len() >= 4
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes.iter().all(|byte| {
            byte.is_ascii_digit() || matches!(byte, b'-' | b':' | b'T' | b'Z' | b'.' | b'+')
        })
}

fn write_domain(f: &mut Formatter, domain: &ObjectDomain) {
    // ObjectDomain::_exportToWKT
    if let Some(scope) = &domain.scope {
        f.start_node("SCOPE", false);
        f.add_quoted_string(scope);
        f.end_node();
    } else if f.use_2019_keywords {
        f.start_node("SCOPE", false);
        f.add_quoted_string("unknown");
        f.end_node();
    }
    let Some(extent) = &domain.domain_of_validity else {
        return;
    };
    if let Some(description) = &extent.description {
        f.start_node("AREA", false);
        f.add_quoted_string(description);
        f.end_node();
    }
    if let Some(bbox) = &extent.geographic_bounding_box {
        f.start_node("BBOX", false);
        f.add_double(bbox.south_latitude);
        f.add_double(bbox.west_longitude);
        f.add_double(bbox.north_latitude);
        f.add_double(bbox.east_longitude);
        f.end_node();
    }
    if let Some(vertical) = &extent.vertical_extent {
        f.start_node("VERTICALEXTENT", false);
        f.add_double(vertical.minimum);
        f.add_double(vertical.maximum);
        write_unit(f, &vertical.unit, None);
        f.end_node();
    }
    if let Some(temporal) = &extent.temporal_extent {
        f.start_node("TIMEEXTENT", false);
        for value in [&temporal.start, &temporal.end] {
            if is_iso_8601(value) {
                f.add_raw(value);
            } else {
                f.add_quoted_string(value);
            }
        }
        f.end_node();
    }
}

fn write_usage(
    f: &mut Formatter,
    domains: &[ObjectDomain],
    identifiers: &[Identifier],
    remarks: &str,
) {
    // ObjectUsage::baseExportToWKT
    let wkt2 = f.is_wkt2();
    if wkt2 && f.output_usage() && !domains.is_empty() {
        if f.use_2019_keywords {
            for domain in domains {
                f.start_node("USAGE", false);
                write_domain(f, domain);
                f.end_node();
            }
        } else {
            write_domain(f, &domains[0]);
        }
    }
    if f.output_id() {
        write_ids(f, identifiers);
    }
    if wkt2 && f.output_usage() && !remarks.is_empty() {
        f.start_node("REMARK", false);
        f.add_quoted_string(remarks);
        f.end_node();
    }
}

fn write_unit(f: &mut Formatter, unit: &UnitOfMeasure, keyword: Option<&str>) {
    // UnitOfMeasure::_exportToWKT
    let wkt2 = f.is_wkt2();
    let has_id = !unit.code_space.is_empty();
    let keyword = match keyword {
        Some(keyword) => keyword,
        None if f.force_unit_keyword && unit.unit_type != UnitType::Parametric => "UNIT",
        None if wkt2 => match unit.unit_type {
            UnitType::Linear => "LENGTHUNIT",
            UnitType::Angular => "ANGLEUNIT",
            UnitType::Scale => "SCALEUNIT",
            UnitType::Time => "TIMEUNIT",
            UnitType::Parametric => "PARAMETRICUNIT",
            UnitType::Unknown | UnitType::None => "UNIT",
        },
        None => "UNIT",
    };
    f.start_node(keyword, has_id);
    let name = if !f.use_esri_dialect {
        unit.name.as_str()
    } else if unit.name.eq_ignore_ascii_case("degree") {
        "Degree"
    } else if unit.name.eq_ignore_ascii_case("grad") {
        "Grad"
    } else if unit.name.eq_ignore_ascii_case("metre") {
        "Meter"
    } else {
        unit.name.as_str()
    };
    f.add_quoted_string(name);
    // some TIMEUNIT have no conversion factor
    if !wkt2 || unit.unit_type != UnitType::Time || unit.conversion_to_si != 0.0 {
        f.add_double(unit.conversion_to_si);
    }
    if has_id && f.output_id() {
        f.start_node(if wkt2 { "ID" } else { "AUTHORITY" }, false);
        f.add_quoted_string(&unit.code_space);
        if wkt2 && leading_integer(&unit.code).is_some() {
            f.add_raw(&unit.code);
        } else {
            f.add_quoted_string(&unit.code);
        }
        f.end_node();
    }
    f.end_node();
}

fn esri_name_of_object(name: &str, table: &str) -> String {
    // the alias, then an ESRI object of that name, then the morphed name
    if let Some(alias) = esri_alias_from_official_name(name, table) {
        return alias.to_string();
    }
    if is_single_esri_object_name(name, table) {
        return name.to_string();
    }
    morph_name_to_esri(name)
}

fn write_ellipsoid(f: &mut Formatter, ellipsoid: &Ellipsoid) {
    // Ellipsoid::_exportToWKT
    let wkt2 = f.is_wkt2();
    f.start_node(
        if wkt2 { "ELLIPSOID" } else { "SPHEROID" },
        !ellipsoid.identifiers.is_empty(),
    );
    let name = if ellipsoid.name.is_empty() {
        "unnamed".to_string()
    } else if !f.use_esri_dialect {
        ellipsoid.name.clone()
    } else if ellipsoid.name == "WGS 84" {
        "WGS_1984".to_string()
    } else {
        esri_name_of_object(&ellipsoid.name, "ellipsoid")
    };
    f.add_quoted_string(&name);
    if wkt2 {
        f.add_double(ellipsoid.semi_major_axis.value);
    } else {
        f.add_double(ellipsoid.semi_major_axis.si_value());
    }
    f.add_double(ellipsoid.computed_inverse_flattening());
    if wkt2 {
        write_unit(f, &ellipsoid.semi_major_axis.unit, Some("LENGTHUNIT"));
    }
    if f.output_id() {
        write_ids(f, &ellipsoid.identifiers);
    }
    f.end_node();
}

fn write_prime_meridian(f: &mut Formatter, prime_meridian: &PrimeMeridian) {
    // PrimeMeridian::_exportToWKT
    let wkt2 = f.is_wkt2();
    f.start_node("PRIMEM", !prime_meridian.identifiers.is_empty());
    let name = if f.use_esri_dialect {
        esri_name_of_object(&prime_meridian.name, "prime_meridian")
    } else {
        prime_meridian.name.clone()
    };
    f.add_quoted_string(&name);
    let longitude = &prime_meridian.longitude;
    if f.prime_meridian_in_degree {
        f.add_double(longitude.convert_to_unit(&UnitOfMeasure::degree()));
    } else {
        f.add_double(longitude.value);
    }
    if wkt2 {
        write_unit(f, &longitude.unit, Some("ANGLEUNIT"));
    } else if !f.prime_meridian_in_degree {
        write_unit(f, &longitude.unit, None);
    }
    if f.output_id() {
        write_ids(f, &prime_meridian.identifiers);
    }
    f.end_node();
}

fn esri_datum_name(frame: &GeodeticReferenceFrame) -> String {
    // the ESRI branch of GeodeticReferenceFrame::_exportToWKT
    let name = frame.name.as_str();
    if name == "World Geodetic System 1984" {
        return "D_WGS_1984".to_string();
    }
    if let Some(alias) = esri_alias_from_official_name(name, "geodetic_datum") {
        return alias.to_string();
    }
    if let Some((stem, _)) = name.split_once(" (") {
        if let Some(alias) = esri_alias_from_official_name(stem, "geodetic_datum") {
            return alias.to_string();
        }
    }
    if is_single_esri_object_name(name, "geodetic_datum") {
        return name.to_string();
    }
    if let Some(alias) = frame
        .identifiers
        .first()
        .and_then(|identifier| esri_alias_through_epsg_old_name(identifier, "geodetic_datum"))
    {
        return alias.to_string();
    }
    let morphed = morph_name_to_esri(name);
    if morphed.starts_with("D_") {
        morphed
    } else {
        format!("D_{morphed}")
    }
}

fn wkt1_gdal_datum_name(frame: &GeodeticReferenceFrame) -> String {
    // GDAL before 3 replaced the spaces of EPSG datum names with underscores
    let epsg_identified = frame.identifiers.len() == 1 && frame.identifiers[0].code_space == "EPSG";
    let name = if epsg_identified {
        morph_name_to_esri(&frame.name)
    } else {
        frame.name.clone()
    };
    if name == "World_Geodetic_System_1984" {
        "WGS_1984".to_string()
    } else {
        name
    }
}

fn write_reference_frame(f: &mut Formatter, frame: &GeodeticReferenceFrame) {
    // DynamicGeodeticReferenceFrame::_exportToWKT and GeodeticReferenceFrame::_exportToWKT
    let wkt2 = f.is_wkt2();
    if let Some(dynamic) = &frame.dynamic {
        if wkt2 && f.use_2019_keywords {
            f.start_node("DYNAMIC", false);
            f.start_node("FRAMEEPOCH", false);
            f.add_double(dynamic.frame_reference_epoch);
            f.end_node();
            if let Some(model) = dynamic
                .deformation_model_name
                .as_deref()
                .filter(|model| !model.is_empty())
            {
                f.start_node("MODEL", false);
                f.add_quoted_string(model);
                f.end_node();
            }
            f.end_node();
        }
    }
    f.start_node("DATUM", !frame.identifiers.is_empty());
    let name = if frame.name.is_empty() {
        "unnamed".to_string()
    } else if wkt2 {
        frame.name.clone()
    } else if f.use_esri_dialect {
        esri_datum_name(frame)
    } else {
        wkt1_gdal_datum_name(frame)
    };
    f.add_quoted_string(&name);
    write_ellipsoid(f, &frame.ellipsoid);
    if wkt2 {
        if let Some(anchor) = &frame.anchor {
            f.start_node("ANCHOR", false);
            f.add_quoted_string(anchor);
            f.end_node();
        }
        if let (true, Some(anchor_epoch)) = (f.use_2019_keywords, frame.anchor_epoch) {
            f.start_node("ANCHOREPOCH", false);
            f.add_double(rounded_epoch_in_decimal_year(anchor_epoch));
            f.end_node();
        }
    } else {
        if let Some(parameters) = f.towgs84_parameters {
            f.start_node("TOWGS84", false);
            for value in parameters {
                f.add_double_with_precision(value, 12);
            }
            f.end_node();
        }
        if let Some(extension) = f.hdatum_extension.clone() {
            f.start_node("EXTENSION", false);
            f.add_quoted_string("PROJ4_GRIDS");
            f.add_quoted_string(&extension);
            f.end_node();
        }
    }
    if f.output_id() {
        write_ids(f, &frame.identifiers);
    }
    f.end_node();
}

fn write_ensemble(f: &mut Formatter, ensemble: &DatumEnsemble) {
    // DatumEnsemble::_exportToWKT, which writes the datum the ensemble stands for before WKT2 2019
    if !f.is_wkt2() || !f.use_2019_keywords {
        write_reference_frame(f, &ensemble.as_datum());
        return;
    }
    f.start_node("ENSEMBLE", false);
    f.add_quoted_string(if ensemble.name.is_empty() {
        "unnamed"
    } else {
        &ensemble.name
    });
    for member in &ensemble.members {
        f.start_node("MEMBER", !member.identifiers.is_empty());
        f.add_quoted_string(if member.name.is_empty() {
            "unnamed"
        } else {
            &member.name
        });
        if f.output_id() {
            write_ids(f, &member.identifiers);
        }
        f.end_node();
    }
    write_ellipsoid(f, &ensemble.members[0].ellipsoid);
    f.start_node("ENSEMBLEACCURACY", false);
    f.add_raw(&ensemble.accuracy);
    f.end_node();
    if f.output_id() {
        write_ids(f, &ensemble.identifiers);
    }
    f.end_node();
}

fn write_datum(f: &mut Formatter, datum: &GeodeticDatum) {
    match datum {
        GeodeticDatum::ReferenceFrame(frame) => write_reference_frame(f, frame),
        GeodeticDatum::Ensemble(ensemble) => write_ensemble(f, ensemble),
    }
}

fn prime_meridian_of(datum: &GeodeticDatum) -> &PrimeMeridian {
    match datum {
        GeodeticDatum::ReferenceFrame(frame) => &frame.prime_meridian,
        GeodeticDatum::Ensemble(ensemble) => &ensemble.members[0].prime_meridian,
    }
}

fn write_meridian(f: &mut Formatter, meridian: &Meridian) {
    f.start_node("MERIDIAN", false);
    f.add_double(meridian.longitude.value);
    write_unit(f, &meridian.longitude.unit, Some("ANGLEUNIT"));
    f.end_node();
}

const WKT1_AXIS_DIRECTIONS: [&str; 7] = ["NORTH", "SOUTH", "EAST", "WEST", "UP", "DOWN", "OTHER"];

fn lowercase_first_letter(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn write_axis(
    f: &mut Formatter,
    axis: &CoordinateSystemAxis,
    order: usize,
    disable_abbreviation: bool,
) {
    // CoordinateSystemAxis::_exportToWKT
    let wkt2 = f.is_wkt2();
    f.start_node("AXIS", !axis.identifiers.is_empty());
    let name = axis.name.as_str();
    let abbreviation = axis.abbreviation.as_str();
    let parenthesized_abbreviation = format!("({abbreviation})");
    let mut designation = if name.is_empty() {
        String::new()
    } else if wkt2 {
        lowercase_first_letter(name)
    } else {
        match name {
            "Geodetic latitude" => "Latitude".to_string(),
            "Geodetic longitude" => "Longitude".to_string(),
            other => other.to_string(),
        }
    };
    // PROJ writes geographic axes named Latitude and Longitude without abbreviation
    if !disable_abbreviation && wkt2 && name != "Latitude" && name != "Longitude" {
        if !designation.is_empty() && !abbreviation.is_empty() {
            designation.push(' ');
        }
        if !abbreviation.is_empty() {
            designation.push_str(&parenthesized_abbreviation);
        }
    }
    let mut direction = axis.direction.as_str().to_string();
    if !wkt2 {
        direction = direction.to_ascii_uppercase();
        if axis.direction == AxisDirection::GeocentricZ {
            direction = "NORTH".to_string();
        } else if !WKT1_AXIS_DIRECTIONS.contains(&direction.as_str()) {
            direction = "OTHER".to_string();
        }
    } else if !abbreviation.is_empty() {
        let geocentric = matches!(
            axis.direction,
            AxisDirection::GeocentricX | AxisDirection::GeocentricY | AxisDirection::GeocentricZ
        );
        let easting_or_northing = (order == 1 && name == "Easting" && abbreviation == "E")
            || (order == 2 && name == "Northing" && abbreviation == "N");
        if geocentric || easting_or_northing {
            designation = parenthesized_abbreviation;
        }
    }
    f.add_quoted_string(&designation);
    f.add_raw(&direction);
    if let (true, Some(meridian)) = (wkt2, &axis.meridian) {
        write_meridian(f, meridian);
    }
    if f.output_axis_order && order > 0 {
        f.start_node("ORDER", false);
        f.add_integer(order as i64);
        f.end_node();
    }
    if f.output_unit() && axis.unit.unit_type != UnitType::None {
        write_unit(f, &axis.unit, None);
    }
    if wkt2 && f.use_2019_keywords {
        if let Some(minimum) = axis.minimum_value {
            f.start_node("AXISMINVALUE", false);
            f.add_double(minimum);
            f.end_node();
        }
        if let Some(maximum) = axis.maximum_value {
            f.start_node("AXISMAXVALUE", false);
            f.add_double(maximum);
            f.end_node();
        }
        if let (Some(_), Some(_), Some(range_meaning)) =
            (axis.minimum_value, axis.maximum_value, axis.range_meaning)
        {
            f.start_node("RANGEMEANING", false);
            f.add_raw(range_meaning.as_str());
            f.end_node();
        }
    }
    if f.output_id() {
        write_ids(f, &axis.identifiers);
    }
    f.end_node();
}

fn write_coordinate_system(f: &mut Formatter, wkt2_type: &str, axes: &[CoordinateSystemAxis]) {
    // CoordinateSystem::_exportToWKT
    if f.output_axis != OutputAxisRule::Yes {
        return;
    }
    let wkt2 = f.is_wkt2();
    if wkt2 {
        f.start_node("CS", false);
        f.add_raw(wkt2_type);
        f.add_integer(axes.len() as i64);
        f.end_node();
        // an anonymous node indents the axes one level under CS
        f.start_node("", false);
    }
    f.push_output_unit(wkt2);
    let disable_abbreviation = axes.len() == 3
        && axes[0].name == "Latitude"
        && axes[1].name == "Longitude"
        && axes[2].name == "Ellipsoidal height";
    for (index, axis) in axes.iter().enumerate() {
        let order = if wkt2 && axes.len() > 1 { index + 1 } else { 0 };
        write_axis(f, axis, order, disable_abbreviation);
    }
    f.pop_output_unit();
    if wkt2 {
        f.end_node();
    }
}

struct GeodeticCrsParts<'a> {
    name: &'a str,
    identifiers: &'a [Identifier],
    remarks: &'a str,
    domains: &'a [ObjectDomain],
    datum: &'a GeodeticDatum,
    axes: &'a [CoordinateSystemAxis],
    is_geographic: bool,
}

impl<'a> GeodeticCrsParts<'a> {
    fn geographic(crs: &'a GeographicCrs) -> Self {
        Self {
            name: &crs.name,
            identifiers: &crs.identifiers,
            remarks: &crs.remarks,
            domains: &crs.domains,
            datum: &crs.datum,
            axes: &crs.coordinate_system.axes,
            is_geographic: true,
        }
    }

    fn geocentric(crs: &'a GeodeticCrs) -> Self {
        Self {
            name: &crs.name,
            identifiers: &crs.identifiers,
            remarks: &crs.remarks,
            domains: &crs.domains,
            datum: &crs.datum,
            axes: &crs.coordinate_system.axes,
            is_geographic: false,
        }
    }
}

fn esri_geodetic_crs_name(crs: &GeodeticCrsParts) -> String {
    // the ESRI branch of GeodeticCRS::_exportToWKT
    let geographic_3d = crs.is_geographic && crs.axes.len() == 3;
    if crs.name == "WGS 84" {
        return if geographic_3d {
            "WGS_1984_3D"
        } else {
            "GCS_WGS_1984"
        }
        .to_string();
    }
    let table = if geographic_3d {
        "geographic_3D_crs"
    } else {
        "geodetic_crs"
    };
    let typed_table = if geographic_3d {
        "geographic_3D_crs"
    } else {
        "geographic_2D_crs"
    };
    let alias = crs
        .identifiers
        .first()
        .and_then(|identifier| esri_alias_of_code(identifier, "geodetic_crs"))
        .or_else(|| esri_alias_from_official_name(crs.name, table));
    if let Some(alias) = alias {
        return alias.to_string();
    }
    if is_single_esri_object_name(crs.name, "geodetic_crs") {
        return crs.name.to_string();
    }
    if let Some(alias) = crs
        .identifiers
        .first()
        .and_then(|identifier| esri_alias_through_epsg_old_name(identifier, typed_table))
    {
        return alias.to_string();
    }
    let morphed = morph_name_to_esri(crs.name);
    if morphed.starts_with("GCS_") {
        morphed
    } else {
        format!("GCS_{morphed}")
    }
}

fn write_geodetic_crs(f: &mut Formatter, crs: &GeodeticCrsParts) -> Result<(), Error> {
    // GeodeticCRS::_exportToWKT
    let wkt2 = f.is_wkt2();
    let axes = crs.axes;
    let geographic_3d = crs.is_geographic && axes.len() == 3;
    let old_axis_rule = f.output_axis;
    let esri = !wkt2 && f.use_esri_dialect;
    if esri && axes.len() == 3 && !crs.is_geographic {
        return Err(formatting_error(
            "Geocentric CRS not supported in WKT1_ESRI",
        ));
    }
    if !wkt2 && !esri && geographic_3d && old_axis_rule != OutputAxisRule::No {
        return Err(formatting_error("WKT1 does not support Geographic 3D CRS."));
    }
    let keyword = if wkt2 {
        if f.use_2019_keywords && crs.is_geographic {
            "GEOGCRS"
        } else {
            "GEODCRS"
        }
    } else if crs.is_geographic {
        "GEOGCS"
    } else {
        "GEOCCS"
    };
    f.start_node(keyword, !crs.identifiers.is_empty());
    let name = if esri {
        esri_geodetic_crs_name(crs)
    } else {
        crs.name.to_string()
    };
    f.add_quoted_string(&name);
    let unit = &axes[0].unit;
    f.push_axis_angular_unit(unit);
    write_datum(f, crs.datum);
    write_prime_meridian(f, prime_meridian_of(crs.datum));
    f.pop_axis_angular_unit();
    if !wkt2 {
        write_unit(f, unit, None);
    }
    if geographic_3d && esri {
        write_unit(f, &axes[2].unit, Some("LINUNIT"));
    }
    if old_axis_rule == OutputAxisRule::Wkt1GdalEpsgStyle && !crs.is_geographic {
        f.output_axis = OutputAxisRule::Yes;
    }
    let wkt2_type = if crs.is_geographic {
        "ellipsoidal"
    } else {
        "Cartesian"
    };
    write_coordinate_system(f, wkt2_type, axes);
    f.output_axis = old_axis_rule;
    write_usage(f, crs.domains, crs.identifiers, crs.remarks);
    f.end_node();
    Ok(())
}

struct ConversionContext<'a> {
    base_crs: &'a GeographicCrs,
    projected_crs_name: &'a str,
}

fn write_method(f: &mut Formatter, method: &OperationMethod) -> Result<(), Error> {
    // OperationMethod::_exportToWKT
    let wkt2 = f.is_wkt2();
    f.start_node(
        if wkt2 { "METHOD" } else { "PROJECTION" },
        !method.identifiers.is_empty(),
    );
    let name = if wkt2 {
        method.name.clone()
    } else {
        match mapping_for_method(method) {
            None => method.name.replace(' ', "_"),
            Some(_) if method.name == "Geostationary Satellite (Sweep X)" => {
                "Geostationary_Satellite".to_string()
            }
            Some(mapping) => mapping
                .wkt1_name
                .ok_or_else(|| {
                    formatting_error(format!(
                        "Unsupported conversion method: {}",
                        mapping.wkt2_name
                    ))
                })?
                .to_string(),
        }
    };
    f.add_quoted_string(&name);
    if f.output_id() {
        write_ids(f, &method.identifiers);
    }
    f.end_node();
    Ok(())
}

fn write_value(f: &mut Formatter, value: &ParameterValue) -> Result<(), Error> {
    // ParameterValue::_exportToWKT
    let wkt2 = f.is_wkt2();
    let measure = match value {
        ParameterValue::Measure(measure) => measure,
        ParameterValue::Filename(filename) => {
            f.add_quoted_string(filename);
            return Ok(());
        }
        ParameterValue::Integer(integer) => {
            f.add_integer(i64::from(*integer));
            return Ok(());
        }
    };
    let unit = &measure.unit;
    if f.abridged_transformation {
        if unit.unit_type == UnitType::Linear {
            f.add_double(measure.si_value());
        } else if unit.unit_type == UnitType::Angular {
            f.add_double(measure.convert_to_unit(&UnitOfMeasure::arc_second()));
        } else if unit.has_same_name(&UnitOfMeasure::parts_per_million()) {
            f.add_double(1.0 + measure.value * 1e-6);
        } else {
            f.add_double(measure.value);
        }
        return Ok(());
    }
    if wkt2 {
        f.add_double(measure.value);
        if !unit.has_same_name(&UnitOfMeasure::none()) {
            write_unit(f, unit, None);
        }
        return Ok(());
    }
    // WKT1 writes no parameter unit, so values are in the CRS linear and angular units
    let target_unit = match unit.unit_type {
        UnitType::Linear => Some(f.axis_linear_unit().clone()),
        UnitType::Angular => Some(f.axis_angular_unit().clone()),
        _ => None,
    };
    match target_unit {
        Some(target) if target.conversion_to_si == 0.0 => Err(formatting_error(format!(
            "cannot convert value to target {} unit",
            if unit.unit_type == UnitType::Linear {
                "linear"
            } else {
                "angular"
            }
        ))),
        Some(target) => {
            f.add_double(measure.convert_to_unit(&target));
            Ok(())
        }
        None => {
            f.add_double(measure.si_value());
            Ok(())
        }
    }
}

fn write_parameter_value(
    f: &mut Formatter,
    parameter_value: &OperationParameterValue,
    mapping: Option<&'static MethodMapping>,
) -> Result<(), Error> {
    // OperationParameterValue::_exportToWKT
    let parameter = &parameter_value.parameter;
    let parameter_mapping = mapping.and_then(|mapping| parameter_mapping(mapping, parameter));
    if parameter_mapping.is_some_and(|param| param.wkt1_name.is_none()) {
        return Ok(());
    }
    let is_file = matches!(parameter_value.value, ParameterValue::Filename(_));
    let keyword = if f.is_wkt2() && is_file {
        "PARAMETERFILE"
    } else {
        "PARAMETER"
    };
    f.start_node(keyword, !parameter.identifiers.is_empty());
    let name: &str = match parameter_mapping.and_then(|param| param.wkt1_name) {
        Some(wkt1_name) => wkt1_name,
        None => &parameter.name,
    };
    f.add_quoted_string(name);
    write_value(f, &parameter_value.value)?;
    if f.output_id() {
        write_ids(f, &parameter.identifiers);
    }
    f.end_node();
    Ok(())
}

fn esri_method_name_and_params(
    conversion: &Conversion,
    method_name: &str,
    method_code: i32,
    context: &ConversionContext,
) -> Option<(&'static str, &'static [EsriParamMapping])> {
    // getESRIMethodNameAndParams
    let mapping = esri_mapping(method_name, method_code)?;
    let values = &conversion.parameter_values;
    let target_name = context.projected_crs_name;
    let contains = |text: &str, needle: &str| {
        text.to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase())
    };
    let azimuth_equals_skew_angle = || {
        (parameter_si_value(values, PARAMETER_AZIMUTH_PROJECTION_CENTRE)
            - parameter_si_value(values, PARAMETER_ANGLE_RECTIFIED_TO_SKEW_GRID))
        .abs()
            < 1e-15
    };
    let chosen: (&'static str, &'static [EsriParamMapping]) = match mapping.epsg_code {
        METHOD_EQUIDISTANT_CYLINDRICAL | METHOD_EQUIDISTANT_CYLINDRICAL_SPHERICAL => {
            if contains(target_name, "Plate Carree")
                && parameter_si_value(values, PARAMETER_LATITUDE_OF_NATURAL_ORIGIN) == 0.0
            {
                ("Plate_Carree", &PARAMS_ESRI_PLATE_CARREE[..])
            } else {
                (
                    "Equidistant_Cylindrical",
                    &PARAMS_ESRI_EQUIDISTANT_CYLINDRICAL[..],
                )
            }
        }
        METHOD_TRANSVERSE_MERCATOR => {
            if contains(&conversion.name, "Gauss Kruger")
                || contains(target_name, "Gauss")
                || contains(target_name, "GK_")
            {
                ("Gauss_Kruger", &PARAMS_ESRI_GAUSS_KRUGER[..])
            } else {
                ("Transverse_Mercator", &PARAMS_ESRI_TRANSVERSE_MERCATOR[..])
            }
        }
        METHOD_HOTINE_OBLIQUE_MERCATOR_VARIANT_A => {
            if azimuth_equals_skew_angle() {
                (
                    "Hotine_Oblique_Mercator_Azimuth_Natural_Origin",
                    &PARAMS_ESRI_HOTINE_OBLIQUE_MERCATOR_AZIMUTH_NATURAL_ORIGIN[..],
                )
            } else {
                (
                    "Rectified_Skew_Orthomorphic_Natural_Origin",
                    &PARAMS_ESRI_RECTIFIED_SKEW_ORTHOMORPHIC_NATURAL_ORIGIN[..],
                )
            }
        }
        METHOD_HOTINE_OBLIQUE_MERCATOR_VARIANT_B => {
            if azimuth_equals_skew_angle() {
                (
                    "Hotine_Oblique_Mercator_Azimuth_Center",
                    &PARAMS_ESRI_HOTINE_OBLIQUE_MERCATOR_AZIMUTH_CENTER[..],
                )
            } else {
                (
                    "Rectified_Skew_Orthomorphic_Center",
                    &PARAMS_ESRI_RECTIFIED_SKEW_ORTHOMORPHIC_CENTER[..],
                )
            }
        }
        METHOD_POLAR_STEREOGRAPHIC_VARIANT_A => {
            // PROJ: only EPSG:5041 and 5042 use Polar_Stereographic_Variant_A in ESRI's pe_list_projcs.csv
            let ups = target_name == "WGS 84 / UPS North (E,N)"
                || target_name == "WGS 84 / UPS South (E,N)";
            let name = if ups {
                "Polar_Stereographic_Variant_A"
            } else {
                "Stereographic"
            };
            (name, mapping.params)
        }
        METHOD_POLAR_STEREOGRAPHIC_VARIANT_B => {
            let name = if parameter_si_value(values, PARAMETER_LATITUDE_STD_PARALLEL) > 0.0 {
                "Stereographic_North_Pole"
            } else {
                "Stereographic_South_Pole"
            };
            (name, mapping.params)
        }
        METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA => {
            let first_parallel = parameter_value_in_unit(
                values,
                PARAMETER_LATITUDE_1ST_STD_PARALLEL,
                &UnitOfMeasure::degree(),
            );
            let name = if (first_parallel - 30.0).abs() < 1e-10 {
                "Behrmann"
            } else {
                "Cylindrical_Equal_Area"
            };
            (name, mapping.params)
        }
        _ => (mapping.esri_name, mapping.params),
    };
    Some(chosen)
}

fn write_esri_parameters(f: &mut Formatter, conversion: &Conversion, params: &[EsriParamMapping]) {
    for param in params {
        f.start_node("PARAMETER", false);
        f.add_quoted_string(param.esri_name);
        let Some(wkt2_name) = param.wkt2_name else {
            f.add_raw(param.fixed_value);
            f.end_node();
            continue;
        };
        let value = parameter_value_by_name_or_code(
            &conversion.parameter_values,
            wkt2_name,
            param.epsg_code,
        );
        match value {
            Some(ParameterValue::Measure(measure)) => match measure.unit.unit_type {
                UnitType::Linear => {
                    let target = f.axis_linear_unit().clone();
                    f.add_double(measure.convert_to_unit(&target));
                }
                UnitType::Angular => {
                    let target = f.axis_angular_unit().clone();
                    let mut value = measure.convert_to_unit(&target);
                    if target.has_same_name(&UnitOfMeasure::degree()) {
                        if value > 180.0 {
                            value -= 360.0;
                        } else if value < -180.0 {
                            value += 360.0;
                        }
                    }
                    f.add_double(value);
                }
                _ => f.add_double(measure.si_value()),
            },
            _ if param.esri_name.to_ascii_lowercase().contains("scale") => f.add_double(1.0),
            _ => f.add_double(0.0),
        }
        f.end_node();
    }
}

fn write_conversion(
    f: &mut Formatter,
    conversion: &Conversion,
    context: &ConversionContext,
) -> Result<(), Error> {
    // Conversion::_exportToWKT
    let wkt2 = f.is_wkt2();
    let mut method_name = conversion.method.name.as_str();
    let mut method_code = method_epsg_code(&conversion.method);
    let base_ellipsoid = context.base_crs.datum.ellipsoid();

    if !wkt2 && f.use_esri_dialect && method_code == METHOD_MERCATOR_VARIANT_A {
        if let Some(equivalent) =
            conversion.convert_to_other_method(base_ellipsoid, METHOD_MERCATOR_VARIANT_B)
        {
            return write_conversion(f, &equivalent, context);
        }
    }

    if wkt2 {
        f.start_node("CONVERSION", !conversion.identifiers.is_empty());
        f.add_quoted_string(&conversion.name);
    } else {
        f.enter();
        f.push_output_unit(false);
        f.push_output_id(false);
    }

    let mut already_written = false;
    let mut method_written = false;
    let mut mapping = if !wkt2 && !f.use_esri_dialect {
        mapping_for_method(&conversion.method)
    } else {
        None
    };

    if !wkt2 && method_code == METHOD_MERCATOR_SPHERICAL && base_ellipsoid.is_sphere() {
        method_name = "Mercator (variant A)";
        method_code = METHOD_MERCATOR_VARIANT_A;
        if !f.use_esri_dialect {
            method_written = true;
            f.start_node("PROJECTION", false);
            f.add_quoted_string("Mercator_1SP");
            f.end_node();
            mapping = mapping_by_code(method_code);
        }
    }

    let values = &conversion.parameter_values;
    if !wkt2 && f.use_esri_dialect {
        if let Some((esri_name, params)) =
            esri_method_name_and_params(conversion, method_name, method_code, context)
        {
            f.start_node("PROJECTION", false);
            f.add_quoted_string(esri_name);
            f.end_node();
            write_esri_parameters(f, conversion, params);
            already_written = true;
        }
    } else if !wkt2 {
        if method_code == METHOD_POPULAR_VISUALISATION_PSEUDO_MERCATOR {
            let latitude_of_origin = parameter_value_in_unit(
                values,
                PARAMETER_LATITUDE_OF_NATURAL_ORIGIN,
                &UnitOfMeasure::degree(),
            );
            if latitude_of_origin != 0.0 {
                return Err(formatting_error(
                    "Unsupported value for Latitude of natural origin",
                ));
            }
            already_written = true;
            f.start_node("PROJECTION", false);
            f.add_quoted_string("Mercator_1SP");
            f.end_node();
            let central_meridian = parameter_value_in_unit(
                values,
                PARAMETER_LONGITUDE_OF_NATURAL_ORIGIN,
                &UnitOfMeasure::degree(),
            );
            for (name, value) in [
                ("central_meridian", central_meridian),
                ("scale_factor", 1.0),
                (
                    "false_easting",
                    parameter_si_value(values, PARAMETER_FALSE_EASTING),
                ),
                (
                    "false_northing",
                    parameter_si_value(values, PARAMETER_FALSE_NORTHING),
                ),
            ] {
                f.start_node("PARAMETER", false);
                f.add_quoted_string(name);
                f.add_double(value);
                f.end_node();
            }
        } else if method_name.starts_with("PROJ ") {
            already_written = true;
            f.start_node("PROJECTION", false);
            f.add_quoted_string("custom_proj4");
            f.end_node();
        }
    }

    if !already_written {
        if !method_written {
            write_method(f, &conversion.method)?;
        }
        if !wkt2
            && method_code == METHOD_MERCATOR_VARIANT_A
            && parameter_si_value(values, PARAMETER_LATITUDE_OF_NATURAL_ORIGIN) != 0.0
        {
            return Err(formatting_error(
                "Unsupported value for Latitude of natural origin",
            ));
        }
        for parameter_value in values {
            let parameter_code = parameter_epsg_code(&parameter_value.parameter);
            let is_zero = matches!(&parameter_value.value, ParameterValue::Measure(measure) if measure.si_value() == 0.0);
            // EPSG gives Equidistant Cylindrical no latitude of natural origin, PROJ writes it only when not zero
            let skipped_latitude = matches!(
                method_code,
                METHOD_EQUIDISTANT_CYLINDRICAL | METHOD_EQUIDISTANT_CYLINDRICAL_SPHERICAL
            ) && parameter_code == PARAMETER_LATITUDE_OF_NATURAL_ORIGIN
                && is_zero;
            let skipped_offset = method_code == METHOD_VERTICAL_PERSPECTIVE
                && (parameter_code == PARAMETER_FALSE_EASTING
                    || parameter_code == PARAMETER_FALSE_NORTHING)
                && is_zero;
            if skipped_latitude || skipped_offset {
                continue;
            }
            write_parameter_value(f, parameter_value, mapping)?;
        }
    }

    if wkt2 {
        if f.output_id() {
            write_ids(f, &conversion.identifiers);
        }
        f.end_node();
    } else {
        f.pop_output_unit();
        f.pop_output_id();
        f.leave();
    }
    Ok(())
}

fn format_proj_number(value: f64) -> String {
    // internal::toString with 15 digits, as PROJStringFormatter::addParam writes numbers
    let text = crate::iso19111::projjson::format_general(value, 15);
    if text.contains("9999999999") {
        crate::iso19111::projjson::format_general(value, 14)
    } else {
        text
    }
}

fn web_mercator_extension(crs: &ProjectedCrs) -> Option<String> {
    // createPROJ4WebMercator
    let values = &crs.conversion.parameter_values;
    let unit = &crs.coordinate_system.axes[0].unit;
    if !unit.is_equivalent_to(&UnitOfMeasure::metre()) {
        // TODO: PROJ also writes the PROJ unit name of a non-metre unit
        return None;
    }
    let a = crs.base_crs.datum.ellipsoid().semi_major_axis.si_value();
    let central_meridian = parameter_value_in_unit(
        values,
        PARAMETER_LONGITUDE_OF_NATURAL_ORIGIN,
        &UnitOfMeasure::degree(),
    );
    let false_easting = parameter_si_value(values, PARAMETER_FALSE_EASTING);
    let false_northing = parameter_si_value(values, PARAMETER_FALSE_NORTHING);
    Some(format!(
        "+proj=merc +a={a} +b={a} +lat_ts=0 +lon_0={lon_0} +x_0={x_0} +y_0={y_0} +k=1 +units=m +nadgrids=@null +wktext +no_defs",
        a = format_proj_number(a),
        lon_0 = format_proj_number(central_meridian),
        x_0 = format_proj_number(false_easting),
        y_0 = format_proj_number(false_northing),
    ))
}

fn write_extension_node(f: &mut Formatter, crs: &ProjectedCrs) {
    // Conversion::addWKTExtensionNode for the methods the model holds
    let method_code = method_epsg_code(&crs.conversion.method);
    let web_mercator = method_code == METHOD_POPULAR_VISUALISATION_PSEUDO_MERCATOR
        || crs.conversion.name == "Popular Visualisation Mercator";
    if !web_mercator {
        return;
    }
    if let Some(extension) = web_mercator_extension(crs) {
        f.start_node("EXTENSION", false);
        f.add_quoted_string("PROJ4");
        f.add_quoted_string(&extension);
        f.end_node();
    }
}

fn esri_projected_crs_name(crs: &ProjectedCrs) -> Option<String> {
    let alias = crs
        .identifiers
        .first()
        .and_then(|identifier| esri_alias_of_code(identifier, "projected_crs"))
        .or_else(|| esri_alias_from_official_name(&crs.name, "projected_crs"));
    if let Some(alias) = alias {
        return Some(alias.to_string());
    }
    is_single_esri_object_name(&crs.name, "projected_crs").then(|| crs.name.clone())
}

fn esri_definition_matching(crs: &ProjectedCrs, esri_name: &str) -> Option<WktNode> {
    // PROJ writes ESRI's own WKT for a CRS equivalent to the ESRI projected CRS of that name
    let definition = esri_projected_definition(esri_name)?;
    let Ok(Crs::Projected(esri_crs)) = Crs::from_wkt(definition) else {
        return None;
    };
    if !crs.is_equivalent_to(&esri_crs, Criterion::Equivalent) {
        return None;
    }
    WktNode::parse(definition).ok()
}

fn write_projected_axes(f: &mut Formatter, crs: &ProjectedCrs) {
    let old_axis_rule = f.output_axis;
    let axes = &crs.coordinate_system.axes;
    if old_axis_rule == OutputAxisRule::Wkt1GdalEpsgStyle
        && axes[0].direction == AxisDirection::East
        && axes[1].direction == AxisDirection::North
    {
        f.output_axis = OutputAxisRule::Yes;
    }
    write_coordinate_system(f, "Cartesian", axes);
    f.output_axis = old_axis_rule;
}

fn write_popular_visualisation_crs(f: &mut Formatter, crs: &ProjectedCrs) -> Result<(), Error> {
    // the "Popular Visualisation CRS / Mercator" branch of ProjectedCRS::_exportToWKT
    f.start_node("PROJCS", !crs.identifiers.is_empty());
    f.add_quoted_string(&crs.name);
    f.towgs84_parameters = Some([0.0; 7]);
    let base = GeodeticCrsParts::geographic(&crs.base_crs);
    let result = write_geodetic_crs(f, &base);
    f.towgs84_parameters = None;
    result?;
    f.start_node("PROJECTION", false);
    f.add_quoted_string("Mercator_1SP");
    f.end_node();
    for (name, value) in [
        ("central_meridian", 0.0),
        ("scale_factor", 1.0),
        ("false_easting", 0.0),
        ("false_northing", 0.0),
    ] {
        f.start_node("PARAMETER", false);
        f.add_quoted_string(name);
        f.add_double(value);
        f.end_node();
    }
    write_unit(f, &crs.coordinate_system.axes[0].unit, None);
    write_projected_axes(f, crs);
    write_extension_node(f, crs);
    write_usage(f, &crs.domains, &crs.identifiers, &crs.remarks);
    f.end_node();
    Ok(())
}

const PROJECTED_3D_NEEDS_WKT2_2019: &str = "Projected 3D CRS can only be exported since WKT2:2019";

fn demoted_geographic_crs(crs: &GeographicCrs) -> GeographicCrs {
    // GeographicCRS::demoteTo2D
    let axes = &crs.coordinate_system.axes;
    if axes.len() != 3 {
        return crs.clone();
    }
    let horizontal_axes = &axes[..2];
    if crs.identifiers.len() == 1 {
        if let Some(registered) = registered_geographic_crs_with_axes(crs, horizontal_axes) {
            return registered;
        }
    }
    GeographicCrs {
        name: crs.name.clone(),
        identifiers: Vec::new(),
        remarks: String::new(),
        domains: Vec::new(),
        datum: crs.datum.clone(),
        coordinate_system: EllipsoidalCs {
            axes: horizontal_axes.to_vec(),
        },
        coordinate_system_is_implicit: false,
    }
}

fn demoted_projected_crs(crs: &ProjectedCrs) -> ProjectedCrs {
    // ProjectedCRS::demoteTo2D of a projected 3D CRS
    ProjectedCrs {
        name: crs.name.clone(),
        identifiers: Vec::new(),
        remarks: String::new(),
        domains: Vec::new(),
        base_crs: Box::new(demoted_geographic_crs(&crs.base_crs)),
        conversion: crs.conversion.clone(),
        coordinate_system: CartesianCs {
            axes: crs.coordinate_system.axes[..2].to_vec(),
        },
    }
}

fn write_esri_projected_crs_with_ellipsoidal_height(
    f: &mut Formatter,
    crs: &ProjectedCrs,
) -> Result<(), Error> {
    // exportAsESRIWktCompoundCRSWithEllipsoidalHeight, which ProjectedCRS::_exportToWKT reaches for a projected 3D CRS
    let base = &crs.base_crs;
    let datum_name = match &base.datum {
        GeodeticDatum::ReferenceFrame(frame) => frame.name.clone(),
        GeodeticDatum::Ensemble(ensemble) => ensemble.as_datum().name,
    };
    let esri_datum_name = esri_alias_from_official_name(&datum_name, "geodetic_datum")
        .map_or(datum_name, str::to_string);
    let found = datum_with_esri_vertical_crs(&esri_datum_name)
        .ok_or_else(|| formatting_error(PROJECTED_3D_NEEDS_WKT2_2019))?;
    let datum = registered_geodetic_datum(&found.datum).ok_or_else(|| {
        formatting_error(format!(
            "the datum {}:{} that PROJ writes in VERTCS is not in the registry",
            found.datum.code_space, found.datum.code
        ))
    })?;
    let (vertical_crs_name, direction, unit) = match found.vertical_crs_name {
        // esri.sql gives every from_geogdatum vertical CRS the ELLPS_HEIGHT_METRE coordinate system
        Some(name) => (name, AxisDirection::Up, UnitOfMeasure::metre()),
        None => {
            let height = base.coordinate_system.axes.get(2).ok_or_else(|| {
                formatting_error("the base CRS of a projected 3D CRS has no ellipsoidal height")
            })?;
            let name = esri_datum_name
                .strip_prefix("GCS_")
                .unwrap_or(&esri_datum_name);
            (name, height.direction, height.unit.clone())
        }
    };
    write_projected_crs(f, &demoted_projected_crs(crs))?;
    f.start_node("VERTCS", false);
    f.add_quoted_string(vertical_crs_name);
    write_reference_frame(f, &datum);
    for (name, value) in [
        ("Vertical_Shift", 0.0),
        (
            "Direction",
            if direction == AxisDirection::Up {
                1.0
            } else {
                -1.0
            },
        ),
    ] {
        f.start_node("PARAMETER", false);
        f.add_quoted_string(name);
        f.add_double(value);
        f.end_node();
    }
    write_unit(f, &unit, None);
    f.end_node();
    Ok(())
}

fn write_projected_crs(f: &mut Formatter, crs: &ProjectedCrs) -> Result<(), Error> {
    // ProjectedCRS::_exportToWKT
    let wkt2 = f.is_wkt2();
    let axes = &crs.coordinate_system.axes;
    if axes.len() == 3 && !(wkt2 && f.use_2019_keywords) {
        if f.use_esri_dialect {
            return write_esri_projected_crs_with_ellipsoidal_height(f, crs);
        }
        return Err(formatting_error(PROJECTED_3D_NEEDS_WKT2_2019));
    }

    let esri_name = if f.use_esri_dialect {
        esri_projected_crs_name(crs)
    } else {
        None
    };
    if let (false, Some(esri_name)) = (wkt2, &esri_name) {
        if let Some(definition) = esri_definition_matching(crs, esri_name) {
            f.ingest_node(&definition);
            return Ok(());
        }
    }

    if !wkt2 && !f.use_esri_dialect && crs.name.starts_with("Popular Visualisation CRS / Mercator")
    {
        return write_popular_visualisation_crs(f, crs);
    }

    f.start_node(
        if wkt2 { "PROJCRS" } else { "PROJCS" },
        !crs.identifiers.is_empty(),
    );
    let name = if !f.use_esri_dialect {
        crs.name.clone()
    } else {
        esri_name.unwrap_or_else(|| morph_name_to_esri(&crs.name))
    };
    f.add_quoted_string(&name);

    let base = &crs.base_crs;
    if wkt2 {
        let keyword = if f.use_2019_keywords {
            "BASEGEOGCRS"
        } else {
            "BASEGEODCRS"
        };
        f.start_node(keyword, f.use_2019_keywords && !base.identifiers.is_empty());
        f.add_quoted_string(&base.name);
        write_datum(f, &base.datum);
        write_prime_meridian(f, prime_meridian_of(&base.datum));
        if f.use_2019_keywords {
            write_ids(f, &base.identifiers);
        }
        f.end_node();
    } else {
        let old_axis_rule = f.output_axis;
        f.output_axis = OutputAxisRule::No;
        let result = write_geodetic_crs(f, &GeodeticCrsParts::geographic(base));
        f.output_axis = old_axis_rule;
        result?;
    }

    f.push_axis_linear_unit(&axes[0].unit);
    f.push_axis_angular_unit(&base.coordinate_system.axes[0].unit);
    let context = ConversionContext {
        base_crs: base,
        projected_crs_name: &crs.name,
    };
    let result = write_conversion(f, &crs.conversion, &context);
    f.pop_axis_angular_unit();
    f.pop_axis_linear_unit();
    result?;

    if !wkt2 {
        write_unit(f, &axes[0].unit, None);
    }
    write_projected_axes(f, crs);
    if !wkt2 && !f.use_esri_dialect {
        write_extension_node(f, crs);
    }
    write_usage(f, &crs.domains, &crs.identifiers, &crs.remarks);
    f.end_node();
    Ok(())
}

fn nadgrids_filename(transformation: &Transformation) -> Option<String> {
    // Transformation::getPROJ4NadgridsCompatibleFilename for NTv2 and NADCON
    let method_code = method_epsg_code(&transformation.method);
    let values = &transformation.parameter_values;
    let file = |code: i32| match parameter_value_by_code(values, code) {
        Some(ParameterValue::Filename(filename)) => Some(filename.clone()),
        _ => None,
    };
    if method_code == METHOD_NTV2 || transformation.method.name.eq_ignore_ascii_case("NTv2") {
        return file(PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE);
    }
    if method_code == METHOD_NADCON {
        return file(PARAMETER_LATITUDE_DIFFERENCE_FILE);
    }
    None
}

fn write_transformation(f: &mut Formatter, transformation: &Transformation) -> Result<(), Error> {
    // SingleOperation::exportTransformationToWKT for the abridged transformation of a BoundCRS
    f.start_node(
        "ABRIDGEDTRANSFORMATION",
        !transformation.identifiers.is_empty(),
    );
    f.add_quoted_string(&transformation.name);
    write_method(f, &transformation.method)?;
    for parameter_value in &transformation.parameter_values {
        write_parameter_value(f, parameter_value, None)?;
    }
    write_usage(f, &[], &transformation.identifiers, "");
    f.end_node();
    Ok(())
}

fn write_bound_crs(f: &mut Formatter, crs: &BoundCrs) -> Result<(), Error> {
    // BoundCRS::_exportToWKT
    if f.is_wkt2() {
        f.start_node("BOUNDCRS", false);
        f.start_node("SOURCECRS", false);
        write_crs(f, &crs.source_crs)?;
        f.end_node();
        f.start_node("TARGETCRS", false);
        write_crs(f, &crs.target_crs)?;
        f.end_node();
        f.abridged_transformation = true;
        let result = write_transformation(f, &crs.transformation);
        f.abridged_transformation = false;
        result?;
        write_usage(f, &crs.domains, &crs.identifiers, &crs.remarks);
        f.end_node();
        return Ok(());
    }

    let hub_is_wgs84 = crs.target_crs.name().eq_ignore_ascii_case("WGS 84");
    if let Some(filename) = nadgrids_filename(&crs.transformation).filter(|_| hub_is_wgs84) {
        f.hdatum_extension = Some(filename);
        let result = write_crs(f, &crs.source_crs);
        f.hdatum_extension = None;
        return result;
    }
    let towgs84_compatible =
        matches!(&*crs.target_crs, Crs::Geographic(_) | Crs::Geodetic(_)) && hub_is_wgs84;
    if !towgs84_compatible {
        return Err(formatting_error(
            "Cannot export BoundCRS with non-WGS 84 hub CRS in WKT1",
        ));
    }
    let parameters = towgs84_parameters(
        &crs.transformation.method,
        &crs.transformation.parameter_values,
    )
    .ok_or_else(|| {
        formatting_error("Transformation cannot be formatted as WKT1 TOWGS84 parameters")
    })?;
    if !f.use_esri_dialect {
        f.towgs84_parameters = Some(parameters);
    }
    let result = write_crs(f, &crs.source_crs);
    f.towgs84_parameters = None;
    result
}
