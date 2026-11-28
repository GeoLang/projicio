// adapted from PROJ src/iso19111/io.cpp WKTParser (MIT), see LICENSES/PROJ-MIT.txt

use std::collections::BTreeMap;
use std::f64::consts::PI;

use super::database::{
    first_geodetic_datum_named, geodetic_datum_aliases, geographic_crs_coordinate_system,
    is_known_datum_name, official_name_from_esri_alias, projected_crs_named,
    registered_geodetic_datum, registered_geographic_crs_with_axes, registered_projected_crs,
};
use super::mappings::{EsriMethodMapping, MethodMapping};
use super::methods::{
    METHOD_EQUIDISTANT_CYLINDRICAL, METHOD_EQUIDISTANT_CYLINDRICAL_SPHERICAL,
    METHOD_HOTINE_OBLIQUE_MERCATOR_VARIANT_A, METHOD_HOTINE_OBLIQUE_MERCATOR_VARIANT_B,
    METHOD_KROVAK, METHOD_KROVAK_NORTH_ORIENTED, METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA,
    METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA_SPHERICAL, METHOD_LAMBERT_CONIC_CONFORMAL_1SP,
    METHOD_LAMBERT_CONIC_CONFORMAL_2SP, METHOD_LAMBERT_CONIC_CONFORMAL_2SP_MICHIGAN,
    METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA, METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA_SPHERICAL,
    METHOD_MERCATOR_VARIANT_B, METHOD_NTV2, METHOD_POLAR_STEREOGRAPHIC_VARIANT_A,
    METHOD_POLAR_STEREOGRAPHIC_VARIANT_B, METHOD_POPULAR_VISUALISATION_PSEUDO_MERCATOR,
    METHOD_POSITION_VECTOR_GEOCENTRIC, METHOD_POSITION_VECTOR_GEOGRAPHIC_2D,
    METHOD_POSITION_VECTOR_GEOGRAPHIC_3D, METHOD_TRANSVERSE_MERCATOR,
    METHOD_TRANSVERSE_MERCATOR_SOUTH_ORIENTATED, PARAMETER_ANGLE_RECTIFIED_TO_SKEW_GRID,
    PARAMETER_AZIMUTH_PROJECTION_CENTRE, PARAMETER_FALSE_EASTING, PARAMETER_FALSE_NORTHING,
    PARAMETER_LATITUDE_1ST_STD_PARALLEL, PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE,
    PARAMETER_LATITUDE_OF_NATURAL_ORIGIN, PARAMETER_LATITUDE_STD_PARALLEL,
    PARAMETER_LONGITUDE_OF_NATURAL_ORIGIN, PARAMETER_SCALE_DIFFERENCE,
    PARAMETER_SCALE_FACTOR_AT_NATURAL_ORIGIN, PARAMETER_X_AXIS_ROTATION,
    PARAMETER_X_AXIS_TRANSLATION, PARAMETER_Y_AXIS_ROTATION, PARAMETER_Y_AXIS_TRANSLATION,
    PARAMETER_Z_AXIS_ROTATION, PARAMETER_Z_AXIS_TRANSLATION, are_equivalent_parameters,
    esri_mappings_from_esri_name, mapping_by_code, mapping_by_wkt2_name, mapping_from_wkt1,
    method_epsg_code, method_name_of_code, parameter_epsg_code, parameter_mapping_from_wkt1,
    parameter_name_of_code, parameter_value_in_unit,
};
use super::node::{WktNode, as_double, parse_error, strip_quotes};
use super::other_method::conversion_from_method_code;
use crate::Error;
use crate::iso19111::common::{Measure, ObjectDomain, UnitOfMeasure, UnitType, leading_integer};
use crate::iso19111::coordinate_system::{
    AxisDirection, CartesianCs, CoordinateSystemAxis, EllipsoidalCs, Meridian, RangeMeaning,
};
use crate::iso19111::crs::{BoundCrs, Criterion, Crs, GeodeticCrs, GeographicCrs, ProjectedCrs};
use crate::iso19111::datum::{
    DatumEnsemble, DynamicFrame, Ellipsoid, EllipsoidShape, GeodeticDatum, GeodeticReferenceFrame,
    PrimeMeridian, ensemble_name_to_non_ensemble_name,
};
use crate::iso19111::metadata::{
    EPSG, Extent, GeographicBoundingBox, Identifier, TemporalExtent, VerticalExtent,
    is_equivalent_name,
};
use crate::iso19111::operation::{
    Conversion, OperationMethod, OperationParameter, OperationParameterValue,
    PARAMETER_EPSG_CODE_FOR_HORIZONTAL_CRS, PARAMETER_EPSG_CODE_FOR_INTERPOLATION_CRS,
    ParameterValue, Transformation,
};
use crate::iso19111::projjson::bound_crs_transformation_source;

// io.cpp MAX_PROPERTY_SIZE, the number of property maps one WKT may build
const MAX_PROPERTY_COUNT: usize = 1024;

fn unsupported(kind: &str) -> Error {
    Error::UnsupportedCrs(kind.to_string())
}

fn not_enough_children(node_name: &str) -> Error {
    parse_error(format!("not enough children in {node_name} node"))
}

fn is_keyword(name: &str, keywords: &[&str]) -> bool {
    keywords
        .iter()
        .any(|keyword| name.eq_ignore_ascii_case(keyword))
}

fn starts_with_ignoring_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

fn contains_ignoring_case(text: &str, needle: &str) -> bool {
    text.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn epsg_identifier(code: i32) -> Identifier {
    Identifier::new(EPSG, &code.to_string())
}

fn grad() -> UnitOfMeasure {
    UnitOfMeasure {
        name: "grad".to_string(),
        conversion_to_si: PI / 200.0,
        unit_type: UnitType::Angular,
        code_space: EPSG.to_string(),
        code: "9105".to_string(),
    }
}

fn axis(
    name: &str,
    abbreviation: &str,
    direction: AxisDirection,
    unit: &UnitOfMeasure,
) -> CoordinateSystemAxis {
    CoordinateSystemAxis {
        name: name.to_string(),
        identifiers: Vec::new(),
        abbreviation: abbreviation.to_string(),
        direction,
        unit: unit.clone(),
        minimum_value: None,
        maximum_value: None,
        range_meaning: None,
        meridian: None,
    }
}

fn with_meridian(mut axis: CoordinateSystemAxis, longitude_degrees: f64) -> CoordinateSystemAxis {
    axis.meridian = Some(Meridian {
        longitude: Measure::new(longitude_degrees, UnitOfMeasure::degree()),
    });
    axis
}

fn easting_northing(unit: &UnitOfMeasure) -> CartesianCs {
    // CartesianCS::createEastingNorthing
    CartesianCs {
        axes: vec![
            axis("Easting", "E", AxisDirection::East, unit),
            axis("Northing", "N", AxisDirection::North, unit),
        ],
    }
}

fn geocentric(unit: &UnitOfMeasure) -> CartesianCs {
    // CartesianCS::createGeocentric
    CartesianCs {
        axes: vec![
            axis("Geocentric X", "X", AxisDirection::GeocentricX, unit),
            axis("Geocentric Y", "Y", AxisDirection::GeocentricY, unit),
            axis("Geocentric Z", "Z", AxisDirection::GeocentricZ, unit),
        ],
    }
}

fn wgs84_datum() -> GeodeticReferenceFrame {
    // GeodeticReferenceFrame::EPSG_6326
    GeodeticReferenceFrame {
        name: "World Geodetic System 1984".to_string(),
        identifiers: vec![epsg_identifier(6326)],
        remarks: String::new(),
        domains: Vec::new(),
        anchor: None,
        anchor_epoch: None,
        ellipsoid: Ellipsoid {
            name: "WGS 84".to_string(),
            identifiers: vec![epsg_identifier(7030)],
            semi_major_axis: Measure::new(6378137.0, UnitOfMeasure::metre()),
            shape: EllipsoidShape::InverseFlattening(298.257223563),
        },
        prime_meridian: PrimeMeridian::greenwich(),
        dynamic: None,
    }
}

fn wgs84_geographic_crs() -> GeographicCrs {
    // GeographicCRS::EPSG_4326
    GeographicCrs {
        name: "WGS 84".to_string(),
        identifiers: vec![epsg_identifier(4326)],
        remarks: String::new(),
        domains: Vec::new(),
        datum: GeodeticDatum::ReferenceFrame(Box::new(wgs84_datum())),
        coordinate_system: EllipsoidalCs::latitude_longitude(&UnitOfMeasure::degree()),
        coordinate_system_is_implicit: false,
    }
}

fn wgs84_geocentric_crs() -> GeodeticCrs {
    // GeodeticCRS::EPSG_4978
    GeodeticCrs {
        name: "WGS 84".to_string(),
        identifiers: vec![epsg_identifier(4978)],
        remarks: String::new(),
        domains: Vec::new(),
        datum: GeodeticDatum::ReferenceFrame(Box::new(wgs84_datum())),
        coordinate_system: geocentric(&UnitOfMeasure::metre()),
    }
}

fn epsg_parameter(code: i32, value: ParameterValue) -> OperationParameterValue {
    OperationParameterValue {
        parameter: OperationParameter {
            name: parameter_name_of_code(code).to_string(),
            identifiers: vec![epsg_identifier(code)],
        },
        value,
    }
}

fn epsg_method(code: i32) -> OperationMethod {
    OperationMethod {
        name: method_name_of_code(code).to_string(),
        identifiers: vec![epsg_identifier(code)],
    }
}

fn is_dialect_wkt1_esri(text: &str) -> bool {
    // WKTParser::guessDialect answering WKT1_ESRI
    let wkt = text.trim_start_matches([' ', '\t', '\r', '\n']);
    if starts_with_ignoring_case(wkt, "VERTCS") {
        return true;
    }
    let wkt1_keyword = [
        "GEOCCS", "GEOGCS", "COMPD_CS", "PROJCS", "VERT_CS", "LOCAL_CS",
    ]
    .iter()
    .any(|keyword| starts_with_ignoring_case(wkt, keyword));
    if !wkt1_keyword {
        return false;
    }
    let looks_esri = contains_ignoring_case(wkt, "GEOGCS[\"GCS_")
        || (!starts_with_ignoring_case(wkt, "LOCAL_CS")
            && !contains_ignoring_case(wkt, "AXIS[")
            && !contains_ignoring_case(wkt, "AUTHORITY["));
    looks_esri && !contains_ignoring_case(wkt, "PARAMETER[\"rectified_grid_angle")
}

fn promoted_remarks(identifiers: &[Identifier], remarks: &str) -> String {
    // the remarks of CRS::promoteTo3D's createProperties
    match (identifiers, remarks) {
        ([identifier], "") => format!(
            "Promoted to 3D from {}:{}",
            identifier.code_space, identifier.code
        ),
        ([identifier], remarks) => format!(
            "Promoted to 3D from {}:{}. {remarks}",
            identifier.code_space, identifier.code
        ),
        (_, remarks) => remarks.to_string(),
    }
}

fn promoted_domains(domains: &[ObjectDomain]) -> Vec<ObjectDomain> {
    // CRS::promoteTo3D keeps the extent of each domain and drops its scope
    domains
        .iter()
        .filter_map(|domain| {
            let extent = domain.domain_of_validity.clone()?;
            Some(ObjectDomain {
                scope: None,
                domain_of_validity: Some(extent),
            })
        })
        .collect()
}

fn promoted_geographic_crs(
    crs: GeographicCrs,
    name: String,
    vertical_axis: CoordinateSystemAxis,
) -> GeographicCrs {
    // CRS::promoteTo3D of a GeographicCRS
    if crs.coordinate_system.axes.len() != 2 {
        return crs;
    }
    let mut axes = crs.coordinate_system.axes.clone();
    axes.push(vertical_axis);
    if crs.identifiers.len() == 1 {
        if let Some(registered) = registered_geographic_crs_with_axes(&crs, &axes) {
            return registered;
        }
    }
    GeographicCrs {
        name,
        identifiers: Vec::new(),
        remarks: promoted_remarks(&crs.identifiers, &crs.remarks),
        domains: promoted_domains(&crs.domains),
        datum: crs.datum,
        coordinate_system: EllipsoidalCs { axes },
        coordinate_system_is_implicit: false,
    }
}

fn promoted_projected_crs(
    crs: ProjectedCrs,
    name: String,
    vertical_axis: CoordinateSystemAxis,
) -> ProjectedCrs {
    // CRS::promoteTo3D of a ProjectedCRS, whose base CRS takes an ellipsoidal height in metres
    if crs.coordinate_system.axes.len() != 2 {
        return crs;
    }
    let remarks = promoted_remarks(&crs.identifiers, &crs.remarks);
    let domains = promoted_domains(&crs.domains);
    let base_name = crs.base_crs.name.clone();
    let base_height = axis(
        "Ellipsoidal height",
        "h",
        AxisDirection::Up,
        &UnitOfMeasure::metre(),
    );
    let mut axes = crs.coordinate_system.axes;
    axes.push(vertical_axis);
    ProjectedCrs {
        name,
        identifiers: Vec::new(),
        remarks,
        domains,
        base_crs: Box::new(promoted_geographic_crs(
            *crs.base_crs,
            base_name,
            base_height,
        )),
        conversion: crs.conversion,
        coordinate_system: CartesianCs { axes },
    }
}

impl Crs {
    pub fn from_wkt(text: &str) -> Result<Self, Error> {
        let (root, index_end) = WktNode::parse_prefix(text)?;
        let maybe_esri_style = is_dialect_wkt1_esri(text);
        let mut parser = Parser {
            esri_style: false,
            maybe_esri_style,
            towgs84_parameters: Vec::new(),
            datum_proj4_grids: None,
            property_count: 0,
        };
        if maybe_esri_style && text.contains("PARAMETER[\"X_Scale\",") {
            parser.esri_style = true;
            parser.maybe_esri_style = false;
        }
        if is_keyword(&root.value, &["GEOGCS", "PROJCS"]) {
            // ESRI writes a compound CRS as PROJCS[...],VERTCS[...]
            let rest = text[index_end..].trim_start();
            if let Some(after_comma) = rest.strip_prefix(',') {
                let vertical_text = after_comma.trim_start();
                if starts_with_ignoring_case(vertical_text, "VERTCS") {
                    let horizontal = parser.build_crs(&root)?;
                    let (vertical_node, _) = WktNode::parse_prefix(vertical_text)?;
                    return parser.build_esri_compound_crs(horizontal, &vertical_node);
                }
            }
        }
        parser.build_crs(&root)
    }
}

struct Properties {
    name: String,
    identifiers: Vec<Identifier>,
    remarks: String,
    domains: Vec<ObjectDomain>,
}

impl Properties {
    fn without_usage(self, keyword: &str) -> Result<(String, Vec<Identifier>), Error> {
        if !self.remarks.is_empty() || !self.domains.is_empty() {
            return Err(parse_error(format!(
                "REMARK and USAGE on {keyword} are not supported"
            )));
        }
        Ok((self.name, self.identifiers))
    }
}

enum CoordinateSystem {
    Ellipsoidal(EllipsoidalCs),
    Cartesian(CartesianCs),
}

struct Parser {
    esri_style: bool,
    maybe_esri_style: bool,
    towgs84_parameters: Vec<f64>,
    datum_proj4_grids: Option<String>,
    property_count: usize,
}

impl Parser {
    fn build_crs(&mut self, node: &WktNode) -> Result<Crs, Error> {
        let name = node.value.as_str();
        if is_keyword(
            name,
            &[
                "GEODCRS",
                "GEODETICCRS",
                "GEOGCRS",
                "GEOGRAPHICCRS",
                "GEOGCS",
                "GEOCCS",
            ],
        ) {
            if node
                .look_for_child(&["BASEGEOGCRS", "BASEGEODCRS"])
                .is_some()
            {
                return Err(unsupported("DerivedGeodeticCRS"));
            }
            let crs = self.build_geodetic_crs(node)?;
            return self.apply_horizontal_bound_crs_parameters(crs);
        }
        if is_keyword(name, &["PROJCS", "PROJCRS", "PROJECTEDCRS"]) {
            let ob_tran = extension_proj4(node)
                .is_some_and(|proj_string| proj_string.starts_with("+proj=ob_tran +o_proj=l"));
            if ob_tran {
                return Err(unsupported("DerivedGeographicCRS"));
            }
            let crs = Crs::Projected(self.build_projected_crs(node)?);
            return self.apply_horizontal_bound_crs_parameters(crs);
        }
        if is_keyword(name, &["BOUNDCRS"]) {
            return Ok(Crs::Bound(self.build_bound_crs(node)?));
        }
        let unsupported_kind = if is_keyword(name, &["VERT_CS", "VERTCS", "VERTCRS", "VERTICALCRS"])
        {
            if node.look_for_child(&["BASEVERTCRS"]).is_some() {
                "DerivedVerticalCRS"
            } else {
                "VerticalCRS"
            }
        } else if is_keyword(name, &["COMPD_CS", "COMPOUNDCRS"]) {
            "CompoundCRS"
        } else if is_keyword(name, &["TIMECRS"]) {
            if node.look_for_child(&["BASETIMECRS"]).is_some() {
                "DerivedTemporalCRS"
            } else {
                "TemporalCRS"
            }
        } else if is_keyword(name, &["DERIVEDPROJCRS"]) {
            "DerivedProjectedCRS"
        } else if is_keyword(name, &["ENGCRS", "ENGINEERINGCRS", "LOCAL_CS"]) {
            if node.look_for_child(&["BASEENGCRS"]).is_some() {
                "DerivedEngineeringCRS"
            } else {
                "EngineeringCRS"
            }
        } else if is_keyword(name, &["PARAMETRICCRS"]) {
            if node.look_for_child(&["BASEPARAMCRS"]).is_some() {
                "DerivedParametricCRS"
            } else {
                "ParametricCRS"
            }
        } else {
            return Err(parse_error(format!("{name} is not a CRS")));
        };
        Err(unsupported(unsupported_kind))
    }

    fn apply_horizontal_bound_crs_parameters(&mut self, crs: Crs) -> Result<Crs, Error> {
        if !self.towgs84_parameters.is_empty() {
            let parameters = std::mem::take(&mut self.towgs84_parameters);
            return bound_crs_from_towgs84(crs, &parameters).map(Crs::Bound);
        }
        if let Some(filename) = self.datum_proj4_grids.take() {
            return bound_crs_from_nadgrids(crs, filename).map(Crs::Bound);
        }
        Ok(crs)
    }

    fn build_esri_compound_crs(
        &mut self,
        horizontal: Crs,
        vertical_node: &WktNode,
    ) -> Result<Crs, Error> {
        // PROJ's buildVerticalCRS reads a VERTCS holding a DATUM as ellipsoidal heights, datum type 2002
        if vertical_node.look_for_child(&["DATUM"]).is_none() {
            let vertical_datum = vertical_node.look_for_child(&[
                "VDATUM",
                "VERT_DATUM",
                "VERTICALDATUM",
                "VRF",
                "ENSEMBLE",
            ]);
            return Err(match vertical_datum {
                Some(_) => unsupported("CompoundCRS"),
                None => parse_error("Missing VDATUM or ENSEMBLE node"),
            });
        }
        if vertical_node.look_for_child(&["AXIS"]).is_some() {
            return Err(unsupported("ESRI VERTCS with AXIS"));
        }
        let parameters: Vec<(&str, &str)> = vertical_node
            .children
            .iter()
            .filter(|child| child.children.len() == 2 && is_keyword(&child.value, &["PARAMETER"]))
            .map(|child| {
                (
                    child.children[0].value.as_str(),
                    child.children[1].value.as_str(),
                )
            })
            .collect();
        if parameters
            .iter()
            .any(|(name, _)| *name == "\"Vertical_Shift\"")
        {
            self.esri_style = true;
        }
        let mut direction = AxisDirection::Up;
        for (name, value) in &parameters {
            if *name == "\"Direction\"" && as_double(value)? == -1.0 {
                direction = AxisDirection::Down;
            }
        }
        let mut unit = self.build_unit_in_sub_node(vertical_node, UnitType::Linear)?;
        if unit.has_same_name(&UnitOfMeasure::none()) {
            unit = UnitOfMeasure::metre();
        }
        // TODO: PROJ also renames the VERTCS through its ESRI vertical CRS aliases and the NAVD88 geoid names
        let vertical_name = self.build_properties(vertical_node, true)?.name;
        let in_metres_up =
            unit.is_equivalent_to(&UnitOfMeasure::metre()) && direction == AxisDirection::Up;
        let name = if in_metres_up {
            horizontal.name().to_string()
        } else {
            format!("{} ({vertical_name})", horizontal.name())
        };
        let vertical_axis = axis("Ellipsoidal height", "h", direction, &unit);
        match horizontal {
            Crs::Geographic(geographic) => Ok(Crs::Geographic(promoted_geographic_crs(
                geographic,
                name,
                vertical_axis,
            ))),
            Crs::Projected(projected) => Ok(Crs::Projected(promoted_projected_crs(
                projected,
                name,
                vertical_axis,
            ))),
            Crs::Bound(_) => Err(unsupported("BoundCRS promoted to 3D")),
            Crs::Geodetic(_) => Err(unsupported("CompoundCRS")),
        }
    }

    fn build_id(
        &mut self,
        parent: &WktNode,
        node: &WktNode,
        tolerant: bool,
    ) -> Result<Option<Identifier>, Error> {
        let children = &node.children;
        if children.len() < 2 {
            // PROJ only warns about a short ID outside strict mode
            return if tolerant {
                Ok(None)
            } else {
                Err(not_enough_children(&node.value))
            };
        }
        let code_space = strip_quotes(&children[0].value).to_string();
        let version = children
            .get(2)
            .filter(|child| child.children.is_empty())
            .map(|child| strip_quotes(&child.value).to_string())
            .filter(|version| !version.is_empty());
        let mut code = strip_quotes(&children[1].value).to_string();
        // PROJ before 9.5 wrote a wrong code for a synthesized "UTM Zone <n>S" conversion
        if parent.value == "CONVERSION" && code_space == EPSG {
            if let Some(parent_name) = parent.children.first() {
                let parent_name = strip_quotes(&parent_name.value);
                let prefix = "UTM Zone ";
                if starts_with_ignoring_case(parent_name, prefix) && parent_name.contains('S') {
                    let zone = leading_integer(&parent_name[prefix.len()..]).unwrap_or(0);
                    if (1..=60).contains(&zone) {
                        code = (16100 + zone).to_string();
                    }
                }
            }
        }
        let single_child_text = |keyword: &str| {
            node.look_for_child(&[keyword])
                .filter(|child| child.children.len() == 1)
                .map(|child| strip_quotes(&child.children[0].value).to_string())
        };
        Ok(Some(Identifier {
            code_space,
            code,
            version,
            authority_citation: single_child_text("CITATION"),
            uri: single_child_text("URI"),
        }))
    }

    fn build_properties(&mut self, node: &WktNode, has_name: bool) -> Result<Properties, Error> {
        self.property_count += 1;
        if self.property_count > MAX_PROPERTY_COUNT {
            return Err(parse_error("MAX_PROPERTY_SIZE reached"));
        }
        let mut identifiers = Vec::new();
        for child in &node.children {
            if is_keyword(&child.value, &["ID", "AUTHORITY"]) {
                if let Some(identifier) = self.build_id(node, child, true)? {
                    identifiers.push(identifier);
                }
            }
        }

        let mut name = String::new();
        let mut identifier_from_alias: Option<Identifier> = None;
        if let (true, Some(first)) = (has_name, node.children.first()) {
            name = strip_quotes(&first.value).to_string();
            if name.ends_with(" (deprecated)") {
                return Err(unsupported("deprecated objects"));
            }
            // Oracle WKT names an EPSG object "... (EPSG ID 6171)"
            if identifiers.is_empty() {
                if let Some(position) = name.find(" (EPSG ID ") {
                    if name.ends_with(')') {
                        let code = name[position + " (EPSG ID ".len()..name.len() - 1].to_string();
                        name.truncate(position);
                        identifiers.push(Identifier::new(EPSG, &code));
                    }
                }
            }
            let mut table_for_alias = None;
            if node.value.eq_ignore_ascii_case("GEOGCS") {
                if name.starts_with("GCS_") {
                    self.esri_style = true;
                    if name == "GCS_WGS_1984" {
                        name = "WGS 84".to_string();
                    } else if name == "GCS_unknown" {
                        name = "unknown".to_string();
                    } else {
                        table_for_alias = Some("geodetic_crs");
                    }
                }
            } else if self.esri_style && node.value.eq_ignore_ascii_case("SPHEROID") {
                if name == "WGS_1984" {
                    name = "WGS 84".to_string();
                    identifier_from_alias = Some(epsg_identifier(7030));
                } else {
                    table_for_alias = Some("ellipsoid");
                }
            }
            if let Some(table) = table_for_alias {
                if let Some(official) = official_name_from_esri_alias(&name, table) {
                    name = official.name.to_string();
                    // PROJ drops the alias code of a geodetic CRS, whose axis order may differ
                    if table != "geodetic_crs" {
                        identifier_from_alias = Some(official.identifier);
                    }
                }
            }
        }
        if identifiers.is_empty() {
            identifiers.extend(identifier_from_alias);
        }

        let remarks = match node.look_for_child(&["REMARK"]) {
            None => String::new(),
            Some(remark) if remark.children.len() == 1 => {
                strip_quotes(&remark.children[0].value).to_string()
            }
            Some(remark) => {
                return Err(parse_error(format!(
                    "not required number of children in {} node",
                    remark.value
                )));
            }
        };

        let mut domains = Vec::new();
        for child in &node.children {
            if child.value.eq_ignore_ascii_case("USAGE") {
                let domain = self
                    .build_object_domain(child)?
                    .ok_or_else(|| not_enough_children(&child.value))?;
                domains.push(domain);
            }
        }
        if domains.is_empty() {
            domains.extend(self.build_object_domain(node)?);
        }

        if node.look_for_child(&["VERSION"]).is_some() {
            return Err(unsupported("operation version"));
        }
        Ok(Properties {
            name,
            identifiers,
            remarks,
            domains,
        })
    }

    fn build_object_domain(&mut self, node: &WktNode) -> Result<Option<ObjectDomain>, Error> {
        let scope_node = node.look_for_child(&["SCOPE"]);
        let area_node = node.look_for_child(&["AREA"]);
        let bbox_node = node.look_for_child(&["BBOX"]);
        let vertical_node = node.look_for_child(&["VERTICALEXTENT"]);
        let temporal_node = node.look_for_child(&["TIMEEXTENT"]);
        let any = scope_node.is_some()
            || area_node.is_some()
            || bbox_node.is_some()
            || vertical_node.is_some()
            || temporal_node.is_some();
        if !any {
            return Ok(None);
        }
        let scope = scope_node
            .filter(|scope| scope.children.len() == 1)
            .map(|scope| strip_quotes(&scope.children[0].value).to_string());
        // PROJ builds the extent only when there is an AREA or a BBOX
        if area_node.is_none() && bbox_node.is_none() {
            return Ok(Some(ObjectDomain {
                scope,
                domain_of_validity: None,
            }));
        }
        let description = match area_node {
            None => None,
            Some(area) if area.children.len() == 1 => {
                Some(strip_quotes(&area.children[0].value).to_string())
            }
            Some(area) => {
                return Err(parse_error(format!(
                    "not required number of children in {} node",
                    area.value
                )));
            }
        };
        let geographic_bounding_box = match bbox_node {
            None => None,
            Some(bbox) if bbox.children.len() == 4 => {
                let values = bbox
                    .children
                    .iter()
                    .map(|child| as_double(&child.value))
                    .collect::<Result<Vec<_>, Error>>()
                    .map_err(|_| {
                        parse_error(format!("not 4 double values in {} node", bbox.value))
                    })?;
                let (south, west, north, east) = (values[0], values[1], values[2], values[3]);
                Some(
                    GeographicBoundingBox::new(west, south, east, north)
                        .map_err(|e| parse_error(format!("Invalid {} node: {e}", bbox.value)))?,
                )
            }
            Some(bbox) => {
                return Err(parse_error(format!(
                    "not required number of children in {} node",
                    bbox.value
                )));
            }
        };
        let vertical_extent = match vertical_node {
            None => None,
            Some(vertical) if (2..=3).contains(&vertical.children.len()) => {
                let minimum = as_double(&vertical.children[0].value);
                let maximum = as_double(&vertical.children[1].value);
                let (Ok(minimum), Ok(maximum)) = (minimum, maximum) else {
                    return Err(parse_error(format!(
                        "not 2 double values in {} node",
                        vertical.value
                    )));
                };
                let unit = match vertical.children.get(2) {
                    Some(unit_node) => self.build_unit(unit_node, UnitType::Linear)?,
                    None => UnitOfMeasure::metre(),
                };
                Some(VerticalExtent {
                    minimum,
                    maximum,
                    unit,
                })
            }
            Some(vertical) => {
                return Err(parse_error(format!(
                    "not required number of children in {} node",
                    vertical.value
                )));
            }
        };
        let temporal_extent = match temporal_node {
            None => None,
            Some(temporal) if temporal.children.len() == 2 => Some(TemporalExtent {
                start: strip_quotes(&temporal.children[0].value).to_string(),
                end: strip_quotes(&temporal.children[1].value).to_string(),
            }),
            Some(temporal) => {
                return Err(parse_error(format!(
                    "not required number of children in {} node",
                    temporal.value
                )));
            }
        };
        Ok(Some(ObjectDomain {
            scope,
            domain_of_validity: Some(Extent {
                description,
                geographic_bounding_box,
                vertical_extent,
                temporal_extent,
            }),
        }))
    }

    fn build_unit(&mut self, node: &WktNode, unit_type: UnitType) -> Result<UnitOfMeasure, Error> {
        let children = &node.children;
        let minimum_children = if unit_type == UnitType::Time { 1 } else { 2 };
        if children.len() < minimum_children {
            return Err(not_enough_children(&node.value));
        }
        let mut unit_type = unit_type;
        let mut name = strip_quotes(&children[0].value).to_string();
        self.build_properties(node, true)?
            .without_usage(&node.value)?;
        let identifier_node = node
            .look_for_child(&["ID", "AUTHORITY"])
            .filter(|identifier| identifier.children.len() >= 2);
        let mut code_space = identifier_node
            .map(|identifier| strip_quotes(&identifier.children[0].value).to_string())
            .unwrap_or_default();
        let mut code = identifier_node
            .map(|identifier| strip_quotes(&identifier.children[1].value).to_string())
            .unwrap_or_default();
        let mut query_database = true;
        if unit_type == UnitType::Unknown {
            if name.eq_ignore_ascii_case("METER") || name.eq_ignore_ascii_case("METRE") {
                unit_type = UnitType::Linear;
                name = "metre".to_string();
                if code_space.is_empty() {
                    code_space = EPSG.to_string();
                    code = "9001".to_string();
                    query_database = false;
                }
            } else if name.eq_ignore_ascii_case("DEGREE") || name.eq_ignore_ascii_case("GRAD") {
                unit_type = UnitType::Angular;
            }
        }
        if self.esri_style && query_database {
            if let Some(official) = official_name_from_esri_alias(&name, "unit_of_measure") {
                name = official.name.to_string();
                code_space = official.identifier.code_space;
                code = official.identifier.code;
            }
        }
        let mut conversion_to_si = match children.get(1) {
            Some(factor) => as_double(&factor.value)?,
            None => 0.0,
        };
        // PROJ fixes common rounding of the degree and the US survey foot
        let degree = UnitOfMeasure::degree().conversion_to_si;
        let us_foot = 12.0 / 39.37;
        if (conversion_to_si - degree).abs() < 1e-10 * conversion_to_si {
            conversion_to_si = degree;
        } else if (conversion_to_si - us_foot).abs() < 1e-10 * conversion_to_si {
            conversion_to_si = us_foot;
        }
        Ok(UnitOfMeasure {
            name,
            conversion_to_si,
            unit_type,
            code_space,
            code,
        })
    }

    fn build_unit_in_sub_node(
        &mut self,
        node: &WktNode,
        unit_type: UnitType,
    ) -> Result<UnitOfMeasure, Error> {
        let typed_keywords = [
            ("LENGTHUNIT", UnitType::Linear),
            ("ANGLEUNIT", UnitType::Angular),
            ("SCALEUNIT", UnitType::Scale),
            ("TIMEUNIT", UnitType::Time),
            ("TEMPORALQUANTITY", UnitType::Time),
            ("PARAMETRICUNIT", UnitType::Parametric),
            ("UNIT", unit_type),
        ];
        for (keyword, keyword_unit_type) in typed_keywords {
            if let Some(unit_node) = node.look_for_child(&[keyword]) {
                return self.build_unit(unit_node, keyword_unit_type);
            }
        }
        Ok(UnitOfMeasure::none())
    }

    fn build_ellipsoid(&mut self, node: &WktNode) -> Result<Ellipsoid, Error> {
        let children = &node.children;
        if children.len() < 3 {
            return Err(not_enough_children(&node.value));
        }
        let mut unit = self.build_unit_in_sub_node(node, UnitType::Linear)?;
        if unit.name.is_empty() {
            unit = UnitOfMeasure::metre();
        }
        let semi_major_axis = Measure::new(as_double(&children[1].value)?, unit);
        // some WKT in the wild writes SPHEROID["unnamed",6370997,"inf"]
        let inverse_flattening = if children[2].value == "\"inf\"" {
            0.0
        } else {
            as_double(&children[2].value)?
        };
        let (name, identifiers) = self
            .build_properties(node, true)?
            .without_usage(&node.value)?;
        // Ellipsoid::guessBodyName: within 0.7 % of the Earth's mean radius is the Earth
        let earth_mean_radius = 6375000.0;
        if (semi_major_axis.si_value() - earth_mean_radius).abs() >= 0.007 * earth_mean_radius {
            return Err(unsupported("ellipsoid of a body other than the Earth"));
        }
        let shape = if inverse_flattening == 0.0 {
            EllipsoidShape::Sphere
        } else {
            EllipsoidShape::InverseFlattening(inverse_flattening)
        };
        Ok(Ellipsoid {
            name,
            identifiers,
            semi_major_axis,
            shape,
        })
    }

    fn build_prime_meridian(
        &mut self,
        node: &WktNode,
        default_angular_unit: &UnitOfMeasure,
    ) -> Result<PrimeMeridian, Error> {
        let children = &node.children;
        if children.len() < 2 {
            return Err(not_enough_children(&node.value));
        }
        let name = strip_quotes(&children[0].value).to_string();
        let mut unit = self.build_unit_in_sub_node(node, UnitType::Angular)?;
        if unit.name.is_empty() {
            unit = default_angular_unit.clone();
            if unit.name.is_empty() {
                unit = UnitOfMeasure::degree();
            }
        }
        let mut angle = as_double(&children[1].value)?;
        // GDAL WKT1 and ESRI write the Paris meridian in degrees under a grad unit
        if name == "Paris" && (angle - 2.33722917).abs() < 1e-8 && unit.is_equivalent_to(&grad()) {
            angle = 2.5969213;
        } else {
            let meridians_dms: [(&str, f64, f64, f64); 13] = [
                ("Lisbon", -9.0, 7.0, 54.862),
                ("Bogota", -74.0, 4.0, 51.3),
                ("Madrid", -3.0, 41.0, 14.55),
                ("Rome", 12.0, 27.0, 8.4),
                ("Bern", 7.0, 26.0, 22.5),
                ("Jakarta", 106.0, 48.0, 27.79),
                ("Ferro", -17.0, 40.0, 0.0),
                ("Brussels", 4.0, 22.0, 4.71),
                ("Stockholm", 18.0, 3.0, 29.8),
                ("Athens", 23.0, 42.0, 58.815),
                ("Oslo", 10.0, 43.0, 22.5),
                ("Paris RGS", 2.0, 20.0, 13.95),
                ("Paris_RGS", 2.0, 20.0, 13.95),
            ];
            if let Some((_, degrees, minutes, seconds)) = meridians_dms
                .iter()
                .find(|(meridian_name, ..)| *meridian_name == name)
            {
                // epsg.org may give the sexagesimal DMS unit 9110, or WKT1 a decimal degree value under a grad unit
                let sign = if *degrees >= 0.0 { 1.0 } else { -1.0 };
                let as_dms = sign * (degrees.abs() + minutes / 100.0 + seconds / 10000.0);
                let as_decimal_degrees = sign * (degrees.abs() + minutes / 60.0 + seconds / 3600.0);
                if (angle - as_dms).abs() < 1e-8 || (angle - as_decimal_degrees).abs() < 1e-8 {
                    angle = as_decimal_degrees;
                    unit = UnitOfMeasure::degree();
                }
            }
        }
        let (mut name, mut identifiers) = self
            .build_properties(node, true)?
            .without_usage(&node.value)?;
        if self.esri_style {
            if let Some(official) = official_name_from_esri_alias(&name, "prime_meridian") {
                name = official.name.to_string();
                identifiers = vec![official.identifier];
            }
        }
        Ok(PrimeMeridian {
            name,
            identifiers,
            longitude: Measure::new(angle, unit),
        })
    }

    fn identify_datum_from_name(
        &mut self,
        node: &WktNode,
        name: &str,
        properties: &mut Properties,
    ) -> bool {
        // the identifyFromName lambda of WKTParser::Private::buildGeodeticReferenceFrame
        if let Some(datum) = first_geodetic_datum_named(name) {
            let found = is_equivalent_name(name, datum.name)
                || datum
                    .names_and_aliases
                    .iter()
                    .any(|alias| is_equivalent_name(name, alias));
            if found {
                properties.name = datum.name.to_string();
                properties.identifiers = vec![datum.identifier];
            }
            return found;
        }
        // createObjectsFromName found nothing, so PROJ takes the name of the AUTHORITY datum
        let Some(authority_node) = node.look_for_child(&["AUTHORITY"]) else {
            return false;
        };
        let Ok(Some(identifier)) = self.build_id(node, authority_node, false) else {
            return false;
        };
        let Some(datum) = registered_geodetic_datum(&identifier) else {
            return false;
        };
        properties.name = datum.name;
        true
    }

    fn build_geodetic_reference_frame(
        &mut self,
        node: &WktNode,
        prime_meridian: &PrimeMeridian,
        dynamic_node: Option<&WktNode>,
    ) -> Result<GeodeticReferenceFrame, Error> {
        let ellipsoid_node = node
            .look_for_child(&["ELLIPSOID", "SPHEROID"])
            .ok_or_else(|| parse_error("missing ELLIPSOID node"))?;
        let mut properties = self.build_properties(node, true)?;
        let mut name = node
            .children
            .first()
            .map(|first| strip_quotes(&first.value).to_string())
            .ok_or_else(|| not_enough_children(&node.value))?;

        // GDAL writes WGS_1984 for the EPSG v9 name, EPSG v10 names datum ensembles
        let mut name_set = false;
        if name == "WGS_1984" {
            name_set = true;
            properties.name = "World Geodetic System 1984".to_string();
        } else if name.ends_with(" ensemble") {
            if let Some(non_ensemble_name) = ensemble_name_to_non_ensemble_name(&name) {
                name_set = true;
                properties.name = non_ensemble_name.to_string();
            }
        }

        let mut official = None;
        if !name_set && self.maybe_esri_style && !(name.starts_with("D_") || self.esri_style) {
            official = official_name_from_esri_alias(&name, "geodetic_datum");
            if official.is_some() {
                self.maybe_esri_style = false;
                self.esri_style = true;
            }
        }

        if !name_set && (name.starts_with("D_") || self.esri_style) {
            self.esri_style = true;
            let mut identifier_from_alias = official.as_ref().map(|found| found.identifier.clone());
            let mut look_up_alias = false;
            match name.as_str() {
                "D_WGS_1984" => {
                    name = "World Geodetic System 1984".to_string();
                    identifier_from_alias = Some(epsg_identifier(6326));
                }
                "D_ETRS_1989" => {
                    name = "European Terrestrial Reference System 1989".to_string();
                    identifier_from_alias = Some(epsg_identifier(6258));
                }
                "D_unknown" => name = "unknown".to_string(),
                "D_Unknown_based_on_WGS_84_ellipsoid" => {
                    name = "Unknown based on WGS 84 ellipsoid".to_string();
                }
                _ => look_up_alias = true,
            }
            let mut set_name_and_identifier = true;
            if look_up_alias {
                if official.is_none() {
                    official = official_name_from_esri_alias(&name, "geodetic_datum");
                    identifier_from_alias = official.as_ref().map(|found| found.identifier.clone());
                }
                match &official {
                    None => {
                        // there is no D_GDA2020 ESRI alias, so PROJ tries the name without its prefix
                        if let Some(stem) = name.strip_prefix("D_") {
                            let stem = stem.to_string();
                            if self.identify_datum_from_name(node, &stem, &mut properties) {
                                set_name_and_identifier = false;
                            }
                        }
                    }
                    Some(found) => {
                        let mut official_name = found.name.to_string();
                        if prime_meridian.name != "Greenwich" {
                            let with_meridian =
                                format!("{official_name} ({})", prime_meridian.name);
                            if is_known_datum_name(&with_meridian) {
                                official_name = with_meridian;
                            }
                        }
                        name = official_name;
                    }
                }
            }
            if set_name_and_identifier {
                properties.name = name;
                if let Some(identifier) = identifier_from_alias {
                    properties.identifiers = vec![identifier];
                }
            }
        } else if !name_set && name.contains('_') {
            // likely WKT1
            self.identify_datum_from_name(node, &name, &mut properties);
        }

        let ellipsoid = self.build_ellipsoid(ellipsoid_node)?;

        if let Some(towgs84) = node.look_for_child(&["TOWGS84"]) {
            let count = towgs84.children.len();
            if count != 3 && count != 7 {
                return Err(parse_error("Invalid TOWGS84 node"));
            }
            let mut parameters = towgs84
                .children
                .iter()
                .map(|child| as_double(&child.value))
                .collect::<Result<Vec<_>, Error>>()
                .map_err(|_| parse_error("Invalid TOWGS84 node"))?;
            // TODO: PROJ also corrects TOWGS84 values its database knows to be wrongly signed
            parameters.resize(7, 0.0);
            self.towgs84_parameters = parameters;
        }

        if let Some(extension) = node.look_for_child(&["EXTENSION"]) {
            if extension.children.len() == 2
                && strip_quotes(&extension.children[0].value).eq_ignore_ascii_case("PROJ4_GRIDS")
            {
                self.datum_proj4_grids =
                    Some(strip_quotes(&extension.children[1].value).to_string());
            }
        }

        let dynamic = match dynamic_node {
            Some(dynamic_node) => Some(parse_dynamic(dynamic_node)?),
            None => None,
        };
        let anchor = node
            .look_for_child(&["ANCHOR"])
            .filter(|anchor| anchor.children.len() == 1)
            .map(|anchor| strip_quotes(&anchor.children[0].value).to_string());
        let anchor_epoch = match node
            .look_for_child(&["ANCHOREPOCH"])
            .filter(|epoch| epoch.children.len() == 1)
        {
            Some(epoch) if dynamic.is_none() => Some(as_double(&epoch.children[0].value)?),
            _ => None,
        };
        Ok(GeodeticReferenceFrame {
            name: properties.name,
            identifiers: properties.identifiers,
            remarks: properties.remarks,
            domains: properties.domains,
            anchor,
            anchor_epoch,
            ellipsoid,
            prime_meridian: prime_meridian.clone(),
            dynamic,
        })
    }

    fn build_datum_ensemble(
        &mut self,
        node: &WktNode,
        prime_meridian: &PrimeMeridian,
    ) -> Result<DatumEnsemble, Error> {
        let ellipsoid_node = node
            .look_for_child(&["ELLIPSOID", "SPHEROID"])
            .ok_or_else(|| parse_error("missing ELLIPSOID node"))?;
        let (name, identifiers) = self
            .build_properties(node, true)?
            .without_usage(&node.value)?;
        let mut members = Vec::new();
        for child in &node.children {
            if !child.value.eq_ignore_ascii_case("MEMBER") {
                continue;
            }
            if child.children.is_empty() {
                return Err(parse_error("Invalid MEMBER node"));
            }
            let (member_name, member_identifiers) = self
                .build_properties(child, true)?
                .without_usage(&child.value)?;
            members.push(GeodeticReferenceFrame {
                name: member_name,
                identifiers: member_identifiers,
                remarks: String::new(),
                domains: Vec::new(),
                anchor: None,
                anchor_epoch: None,
                ellipsoid: self.build_ellipsoid(ellipsoid_node)?,
                prime_meridian: prime_meridian.clone(),
                dynamic: None,
            });
        }
        if members.is_empty() {
            // TODO: PROJ finds the members of a known ensemble in its database
            return Err(parse_error(format!(
                "Datum ensemble '{name}' has no explicit member specified and no connection to database"
            )));
        }
        let accuracy = node
            .look_for_child(&["ENSEMBLEACCURACY"])
            .and_then(|accuracy| accuracy.children.first())
            .map(|accuracy| accuracy.value.clone())
            .ok_or_else(|| parse_error("missing ENSEMBLEACCURACY node"))?;
        if members.len() < 2 {
            return Err(parse_error("ensemble should have at least 2 datums"));
        }
        Ok(DatumEnsemble {
            name,
            identifiers,
            members,
            accuracy,
        })
    }

    fn build_meridian(&mut self, node: &WktNode) -> Result<Meridian, Error> {
        if node.children.len() < 2 {
            return Err(not_enough_children(&node.value));
        }
        let unit = self.build_unit_in_sub_node(node, UnitType::Angular)?;
        let angle = as_double(&node.children[0].value)?;
        Ok(Meridian {
            longitude: Measure::new(angle, unit),
        })
    }

    fn build_axis(
        &mut self,
        node: &WktNode,
        unit_in: &UnitOfMeasure,
        unit_type: UnitType,
        is_geocentric: bool,
        expected_order: usize,
    ) -> Result<CoordinateSystemAxis, Error> {
        let children = &node.children;
        if children.len() < 2 {
            return Err(not_enough_children(&node.value));
        }
        if let Some(order_node) = node.look_for_child(&["ORDER"]) {
            if order_node.children.len() != 1 {
                return Err(not_enough_children("ORDER"));
            }
            let order = &order_node.children[0].value;
            let order_number = leading_integer(order)
                .ok_or_else(|| parse_error(format!("buildAxis: invalid ORDER value: {order}")))?;
            if usize::try_from(order_number).ok() != Some(expected_order) {
                return Err(parse_error(format!(
                    "buildAxis: did not get expected ORDER value: {order}"
                )));
            }
        }

        // the WKT2 axis designation is "name", "(abbrev)" or "name (abbrev)"
        let designation = strip_quotes(&children[0].value);
        let (mut axis_name, mut abbreviation) =
            if let (Some(separator), true) = (designation.find(" ("), designation.ends_with(')')) {
                (
                    normalize_axis_name(&designation[..separator]),
                    designation[separator + 2..designation.len() - 1].to_string(),
                )
            } else if designation.len() >= 2
                && designation.starts_with('(')
                && designation.ends_with(')')
            {
                let abbreviation = designation[1..designation.len() - 1].to_string();
                let axis_name = match abbreviation.as_str() {
                    "E" => "Easting",
                    "N" => "Northing",
                    "lat" => "Latitude",
                    "lon" => "Longitude",
                    _ => "",
                }
                .to_string();
                (axis_name, abbreviation)
            } else {
                let axis_name = normalize_axis_name(designation);
                let abbreviation = match axis_name.as_str() {
                    "Latitude" => "lat",
                    "Longitude" => "lon",
                    "Ellipsoidal height" => "h",
                    _ => "",
                }
                .to_string();
                (axis_name, abbreviation)
            };
        let direction_text = children[1].value.as_str();
        let mut direction = AxisDirection::from_name(direction_text);

        // WKT2 omits the names of geocentric axes
        if axis_name.is_empty() {
            axis_name = match (direction, abbreviation.as_str()) {
                (Some(AxisDirection::GeocentricX), "X") => "Geocentric X",
                (Some(AxisDirection::GeocentricY), "Y") => "Geocentric Y",
                (Some(AxisDirection::GeocentricZ), "Z") => "Geocentric Z",
                _ => "",
            }
            .to_string();
        }

        if direction.is_none() && is_geocentric && axis_name == "Geocentric X" {
            abbreviation = "X".to_string();
            direction = Some(AxisDirection::GeocentricX);
        } else if direction.is_none() && is_geocentric && axis_name == "Geocentric Y" {
            abbreviation = "Y".to_string();
            direction = Some(AxisDirection::GeocentricY);
        } else if is_geocentric
            && axis_name == "Geocentric Z"
            && (direction_text == "NORTH" || direction_text == "OTHER")
        {
            abbreviation = "Z".to_string();
            direction = Some(AxisDirection::GeocentricZ);
        } else if direction_text == "OTHER" || direction_text == "UNKNOWN" {
            direction = Some(AxisDirection::Unspecified);
        }

        let direction = direction
            .ok_or_else(|| parse_error(format!("unhandled axis direction: {direction_text}")))?;
        let mut unit = self.build_unit_in_sub_node(node, UnitType::Unknown)?;
        if unit.name.is_empty() {
            unit = unit_in.clone();
            if unit.name.is_empty() && unit_type != UnitType::None && unit_type != UnitType::Time {
                return Err(parse_error("buildCS: missing UNIT"));
            }
        }

        let meridian = match node.look_for_child(&["MERIDIAN"]) {
            Some(meridian_node) => Some(self.build_meridian(meridian_node)?),
            None => None,
        };
        let single_value = |keyword: &str| -> Result<Option<f64>, Error> {
            let Some(value_node) = node.look_for_child(&[keyword]) else {
                return Ok(None);
            };
            if value_node.children.len() != 1 {
                return Err(not_enough_children(keyword));
            }
            as_double(&value_node.children[0].value)
                .map(Some)
                .map_err(|_| {
                    parse_error(format!(
                        "buildAxis: invalid {keyword} value: {}",
                        value_node.children[0].value
                    ))
                })
        };
        let minimum_value = single_value("AXISMINVALUE")?;
        let maximum_value = single_value("AXISMAXVALUE")?;
        let range_meaning = match node.look_for_child(&["RANGEMEANING"]) {
            None => None,
            Some(range_node) => {
                if range_node.children.len() != 1 {
                    return Err(not_enough_children("RANGEMEANING"));
                }
                let value = &range_node.children[0].value;
                Some(RangeMeaning::from_name(value).ok_or_else(|| {
                    parse_error(format!("buildAxis: invalid RANGEMEANING value: {value}"))
                })?)
            }
        };
        let (_, identifiers) = self
            .build_properties(node, true)?
            .without_usage(&node.value)?;
        Ok(CoordinateSystemAxis {
            name: axis_name,
            identifiers,
            abbreviation,
            direction,
            unit,
            minimum_value,
            maximum_value,
            range_meaning,
            meridian,
        })
    }

    fn build_cs(
        &mut self,
        cs_node: Option<&WktNode>,
        parent: &WktNode,
        default_angular_unit: &UnitOfMeasure,
    ) -> Result<CoordinateSystem, Error> {
        let parent_name = parent.value.as_str();
        let number_of_axes = parent.count_children_of_name("AXIS");
        let mut axis_count = number_of_axes;
        let mut is_geocentric = false;
        let cs_type = if let Some(cs_node) = cs_node {
            if cs_node.children.len() < 2 {
                return Err(not_enough_children(&cs_node.value));
            }
            let count_text = &cs_node.children[1].value;
            axis_count = leading_integer(count_text)
                .and_then(|count| usize::try_from(count).ok())
                .ok_or_else(|| {
                    parse_error(format!("buildCS: invalid CS axis count: {count_text}"))
                })?;
            cs_node.children[0].value.clone()
        } else if parent_name.eq_ignore_ascii_case("GEOCCS") {
            is_geocentric = true;
            if axis_count == 0 {
                let mut unit = self.build_unit_in_sub_node(parent, UnitType::Linear)?;
                if unit.name.is_empty() {
                    unit = UnitOfMeasure::metre();
                }
                return Ok(CoordinateSystem::Cartesian(geocentric(&unit)));
            }
            "Cartesian".to_string()
        } else if parent_name.eq_ignore_ascii_case("GEOGCS") {
            if axis_count == 0 {
                // WKT1 without AXIS means longitude, latitude
                let mut unit = self.build_unit_in_sub_node(parent, UnitType::Angular)?;
                if unit.name.is_empty() {
                    unit = default_angular_unit.clone();
                }
                if let Some(linear_unit_node) = parent.look_for_child(&["LINUNIT"]) {
                    let linear_unit = self.build_unit(linear_unit_node, UnitType::Linear)?;
                    return Ok(CoordinateSystem::Ellipsoidal(
                        EllipsoidalCs::longitude_latitude_ellipsoidal_height(&unit, &linear_unit),
                    ));
                }
                return Ok(CoordinateSystem::Ellipsoidal(
                    EllipsoidalCs::longitude_latitude(&unit),
                ));
            }
            "ellipsoidal".to_string()
        } else if is_keyword(parent_name, &["BASEGEODCRS", "BASEGEOGCRS"]) {
            if axis_count == 0 {
                // a WKT2 base CRS without CS is presumably latitude, longitude
                let mut unit = self.build_unit_in_sub_node(parent, UnitType::Angular)?;
                if unit.name.is_empty() {
                    unit = default_angular_unit.clone();
                }
                return Ok(CoordinateSystem::Ellipsoidal(
                    EllipsoidalCs::latitude_longitude(&unit),
                ));
            }
            "ellipsoidal".to_string()
        } else if is_keyword(parent_name, &["PROJCS", "BASEPROJCRS", "BASEENGCRS"]) {
            if axis_count == 0 {
                let mut unit = self.build_unit_in_sub_node(parent, UnitType::Linear)?;
                if unit.name.is_empty() {
                    unit = UnitOfMeasure::metre();
                }
                return Ok(CoordinateSystem::Cartesian(easting_northing(&unit)));
            }
            "Cartesian".to_string()
        } else {
            return Err(parse_error(format!(
                "buildCS: unexpected parent node: {parent_name}"
            )));
        };

        if !(1..=3).contains(&axis_count) {
            return Err(parse_error(format!(
                "buildCS: invalid CS axis count for {cs_type}"
            )));
        }
        if number_of_axes != axis_count {
            return Err(parse_error(
                "buildCS: declared number of axis by CS node and number of AXIS are inconsistent",
            ));
        }
        let unit_type = if cs_type.eq_ignore_ascii_case("ellipsoidal") {
            UnitType::Angular
        } else if cs_type.eq_ignore_ascii_case("ordinal") {
            UnitType::None
        } else if cs_type.eq_ignore_ascii_case("parametric") {
            UnitType::Parametric
        } else if is_keyword(&cs_type, &["Cartesian", "vertical", "affine"]) {
            UnitType::Linear
        } else if is_keyword(
            &cs_type,
            &[
                "temporal",
                "TemporalDateTime",
                "TemporalCount",
                "TemporalMeasure",
            ],
        ) {
            UnitType::Time
        } else {
            UnitType::Unknown
        };
        let unit = self.build_unit_in_sub_node(parent, unit_type)?;
        let axes = (0..axis_count)
            .map(|index| {
                let axis_node = parent
                    .look_for_child_occurrence("AXIS", index)
                    .ok_or_else(|| parse_error("missing AXIS node"))?;
                self.build_axis(axis_node, &unit, unit_type, is_geocentric, index + 1)
            })
            .collect::<Result<Vec<_>, Error>>()?;

        let has_two_or_three_axes = axis_count == 2 || axis_count == 3;
        if cs_type.eq_ignore_ascii_case("ellipsoidal") && has_two_or_three_axes {
            return Ok(CoordinateSystem::Ellipsoidal(EllipsoidalCs { axes }));
        }
        if cs_type.eq_ignore_ascii_case("Cartesian") && has_two_or_three_axes {
            return Ok(CoordinateSystem::Cartesian(CartesianCs { axes }));
        }
        if is_keyword(
            &cs_type,
            &[
                "affine",
                "vertical",
                "spherical",
                "ordinal",
                "parametric",
                "temporal",
                "TemporalDateTime",
                "TemporalCount",
                "TemporalMeasure",
            ],
        ) {
            return Err(unsupported(&format!("{cs_type} coordinate system")));
        }
        if is_keyword(&cs_type, &["ellipsoidal", "Cartesian"]) {
            return Err(parse_error(format!(
                "buildCS: invalid CS axis count for {cs_type}"
            )));
        }
        Err(parse_error(format!("unhandled CS type: {cs_type}")))
    }

    fn build_geodetic_crs(&mut self, node: &WktNode) -> Result<Crs, Error> {
        let node_name = node.value.as_str();
        let datum_node = node.look_for_child(&["DATUM", "GEODETICDATUM", "TRF"]);
        let ensemble_node = node.look_for_child(&["ENSEMBLE"]);
        if datum_node.is_some() && ensemble_node.is_some() {
            return Err(parse_error("Datum and DatumEnsemble should not be defined"));
        }
        // built first so that an ESRI name marks the WKT as ESRI before the prime meridian
        let properties = self.build_properties(node, true)?;
        let dynamic_node = node.look_for_child(&["DYNAMIC"]);
        let cs_node = node.look_for_child(&["CS"]);
        if cs_node.is_none()
            && !is_keyword(
                node_name,
                &["GEOGCS", "GEOCCS", "BASEGEODCRS", "BASEGEOGCRS"],
            )
        {
            return Err(parse_error("missing CS node"));
        }

        let unit_type = if node_name.eq_ignore_ascii_case("GEOGCS") {
            UnitType::Angular
        } else {
            UnitType::Unknown
        };
        let mut angular_unit = self.build_unit_in_sub_node(node, unit_type)?;
        if angular_unit.unit_type != UnitType::Angular {
            angular_unit = UnitOfMeasure::none();
        }
        let prime_meridian = match node.look_for_child(&["PRIMEM", "PRIMEMERIDIAN"]) {
            Some(prime_meridian_node) => {
                self.build_prime_meridian(prime_meridian_node, &angular_unit)?
            }
            None => PrimeMeridian::greenwich(),
        };
        if angular_unit.name.is_empty() {
            angular_unit = prime_meridian.longitude.unit.clone();
        }
        if extension_proj4(node).is_some() {
            return Err(unsupported("EXTENSION PROJ4 on a geodetic CRS"));
        }
        let coordinate_system_is_implicit = node.look_for_child(&["AXIS"]).is_none();

        let mut name = properties.name;
        if self.esri_style {
            let crs_name = node
                .children
                .first()
                .map(|first| strip_quotes(&first.value))
                .unwrap_or_default();
            if let Some(official) = official_name_from_esri_alias(crs_name, "geodetic_crs") {
                name = official.name.to_string();
            }
        }

        let datum = match (datum_node, ensemble_node) {
            (Some(datum_node), _) => GeodeticDatum::ReferenceFrame(Box::new(
                self.build_geodetic_reference_frame(datum_node, &prime_meridian, dynamic_node)?,
            )),
            (None, Some(ensemble_node)) => {
                GeodeticDatum::Ensemble(self.build_datum_ensemble(ensemble_node, &prime_meridian)?)
            }
            (None, None) => return Err(parse_error("Missing DATUM or ENSEMBLE node")),
        };
        let coordinate_system = self.build_cs(cs_node, node, &angular_unit)?;

        match coordinate_system {
            CoordinateSystem::Ellipsoidal(mut ellipsoidal) => {
                if node_name.eq_ignore_ascii_case("GEOCCS") {
                    return Err(parse_error("ellipsoidal CS not expected in GEOCCS"));
                }
                let mut identifiers = properties.identifiers;
                // with an identifier, PROJ checks or takes the coordinate system from its database
                let database_cs = identifiers
                    .first()
                    .and_then(geographic_crs_coordinate_system);
                if let Some(database_cs) = database_cs {
                    let has_axes = cs_node.is_some() || node.count_children_of_name("AXIS") != 0;
                    if has_axes {
                        if !ellipsoidal.is_equivalent_to(&database_cs) {
                            identifiers.clear();
                        }
                    } else {
                        let mut altered = database_cs.clone();
                        if node.look_for_child(&["UNIT"]).is_some() {
                            altered = alter_angular_unit(&database_cs, &angular_unit);
                            if !altered.is_equivalent_to(&database_cs) {
                                identifiers.clear();
                            }
                        }
                        ellipsoidal = altered;
                    }
                }
                Ok(Crs::Geographic(GeographicCrs {
                    name,
                    identifiers,
                    remarks: properties.remarks,
                    domains: properties.domains,
                    datum,
                    coordinate_system: ellipsoidal,
                    coordinate_system_is_implicit,
                }))
            }
            CoordinateSystem::Cartesian(cartesian) => {
                if is_keyword(node_name, &["GEOGCRS", "GEOGRAPHICCRS", "BASEGEOGCRS"]) {
                    return Err(parse_error("ellipsoidal CS expected, but found Cartesian"));
                }
                if cartesian.axes.len() != 3 {
                    return Err(parse_error(
                        "Cartesian CS for a GeodeticCRS should have 3 axis",
                    ));
                }
                Ok(Crs::Geodetic(GeodeticCrs {
                    name,
                    identifiers: properties.identifiers,
                    remarks: properties.remarks,
                    domains: properties.domains,
                    datum,
                    coordinate_system: cartesian,
                }))
            }
        }
    }

    fn consume_parameters(
        &mut self,
        node: &WktNode,
        is_abridged: bool,
        default_linear_unit: &UnitOfMeasure,
        default_angular_unit: &UnitOfMeasure,
    ) -> Result<Vec<OperationParameterValue>, Error> {
        let mut parameter_values = Vec::new();
        for child in &node.children {
            let is_parameter = child.value.eq_ignore_ascii_case("PARAMETER");
            let is_file = child.value.eq_ignore_ascii_case("PARAMETERFILE");
            if !is_parameter && !is_file {
                continue;
            }
            if child.children.len() < 2 {
                return Err(not_enough_children(&child.value));
            }
            let (name, mut identifiers) = self
                .build_properties(child, true)?
                .without_usage(&child.value)?;
            let value_text = &child.children[1].value;
            if is_file {
                parameter_values.push(OperationParameterValue {
                    parameter: OperationParameter { name, identifiers },
                    value: ParameterValue::Filename(strip_quotes(value_text).to_string()),
                });
                continue;
            }
            if value_text.starts_with('"') {
                return Err(unsupported("string parameter values"));
            }
            let unhandled =
                || parse_error(format!("unhandled parameter value type : {value_text}"));
            let mut value = as_double(value_text).map_err(|_| unhandled())?;
            let mut unit = self.build_unit_in_sub_node(child, UnitType::Unknown)?;
            if unit.name.is_empty() {
                unit = guess_unit_for_parameter(
                    &child.children[0].value,
                    default_linear_unit,
                    default_angular_unit,
                );
            }
            if is_abridged {
                let epsg_code = match identifiers.as_slice() {
                    [identifier] if identifier.code_space.eq_ignore_ascii_case(EPSG) => {
                        leading_integer(&identifier.code).unwrap_or(0)
                    }
                    _ => 0,
                };
                if let Some((abridged_unit, abridged_code)) =
                    convert_from_abridged(&name, &mut value, epsg_code)
                {
                    unit = abridged_unit;
                    // PROJ rebuilds the parameter properties here, counting one more property map
                    self.build_properties(child, true)?;
                    if identifiers.is_empty() {
                        identifiers.push(epsg_identifier(abridged_code));
                    }
                }
            }
            let parameter = OperationParameter { name, identifiers };
            let code = parameter_epsg_code(&parameter);
            let is_integer = parameter.name == "EPSG code for Interpolation CRS"
                || code == PARAMETER_EPSG_CODE_FOR_INTERPOLATION_CRS
                || parameter.name == "EPSG code for Horizontal CRS"
                || code == PARAMETER_EPSG_CODE_FOR_HORIZONTAL_CRS;
            let value = if is_integer {
                ParameterValue::Integer(leading_integer(value_text).ok_or_else(unhandled)?)
            } else {
                ParameterValue::Measure(Measure::new(value, unit))
            };
            parameter_values.push(OperationParameterValue { parameter, value });
        }
        Ok(parameter_values)
    }

    fn build_conversion(
        &mut self,
        node: &WktNode,
        default_linear_unit: &UnitOfMeasure,
        default_angular_unit: &UnitOfMeasure,
    ) -> Result<Conversion, Error> {
        let method_node = node
            .look_for_child(&["METHOD", "PROJECTION"])
            .ok_or_else(|| parse_error("missing METHOD node"))?;
        if method_node.children.is_empty() {
            return Err(not_enough_children("METHOD"));
        }
        let parameter_values =
            self.consume_parameters(node, false, default_linear_unit, default_angular_unit)?;
        let (name, identifiers) = self
            .build_properties(node, true)?
            .without_usage(&node.value)?;
        let (method_name, method_identifiers) = self
            .build_properties(method_node, true)?
            .without_usage(&method_node.value)?;
        if name.starts_with("Inverse of ") && method_name.starts_with("Inverse of ") {
            return Err(unsupported("inverse conversion"));
        }
        Ok(Conversion {
            name,
            identifiers,
            method: OperationMethod {
                name: method_name,
                identifiers: method_identifiers,
            },
            parameter_values,
        })
    }

    fn build_projection(
        &mut self,
        base: &GeographicCrs,
        projected_node: &WktNode,
        projection_node: &WktNode,
        default_linear_unit: &UnitOfMeasure,
        default_angular_unit: &UnitOfMeasure,
    ) -> Result<Conversion, Error> {
        if projection_node.children.is_empty() {
            return Err(not_enough_children("PROJECTION"));
        }
        let units = DefaultUnits {
            linear: default_linear_unit,
            angular: default_angular_unit,
        };
        if self.esri_style || self.maybe_esri_style {
            let mut parameters = ParametersByName::default();
            if let Some(mapping) =
                self.esri_mapping(projected_node, projection_node, &mut parameters)?
            {
                return build_projection_from_esri(
                    base,
                    projected_node,
                    projection_node,
                    &units,
                    mapping,
                    &parameters,
                );
            }
        }
        self.build_projection_standard(base, projected_node, projection_node, &units)
    }

    fn esri_mapping(
        &self,
        projected_node: &WktNode,
        projection_node: &WktNode,
        parameters: &mut ParametersByName,
    ) -> Result<Option<&'static EsriMethodMapping>, Error> {
        // WKTParser::Private::getESRIMapping
        let esri_name = strip_quotes(&projection_node.children[0].value);
        let mappings = esri_mappings_from_esri_name(esri_name);
        if mappings.is_empty() {
            return Ok(None);
        }
        for child in &projected_node.children {
            if child.value.eq_ignore_ascii_case("PARAMETER") {
                if child.children.len() < 2 {
                    return Err(not_enough_children("PARAMETER"));
                }
                parameters.insert(
                    strip_quotes(&child.children[0].value),
                    &child.children[1].value,
                );
            }
        }
        let mut best = None;
        let mut best_match_count = -1;
        for mapping in mappings {
            let mut match_count = 0;
            let mut unmatch_count = 0;
            for param in mapping.params {
                match parameters.get(param.esri_name) {
                    Some(value) if param.wkt2_name.is_none() => {
                        let equal = matches!(
                            (as_double(param.fixed_value), as_double(value)),
                            (Ok(fixed), Ok(found)) if fixed == found
                        );
                        if !equal {
                            match_count = -1;
                            break;
                        }
                        match_count += 1;
                    }
                    Some(_) => match_count += 1,
                    None if param.is_fixed_value => {
                        parameters.insert(param.esri_name, param.fixed_value)
                    }
                    None => unmatch_count += 1,
                }
            }
            if match_count > best_match_count
                && !(self.maybe_esri_style && unmatch_count >= match_count)
            {
                best = Some(mapping);
                best_match_count = match_count;
            }
        }
        Ok(best)
    }

    fn build_projection_standard(
        &mut self,
        base: &GeographicCrs,
        projected_node: &WktNode,
        projection_node: &WktNode,
        units: &DefaultUnits,
    ) -> Result<Conversion, Error> {
        let mut projection_name = strip_quotes(&projection_node.children[0].value).to_string();
        let mut parameter_values = Vec::new();
        let mut try_to_identify_wkt1_method = true;
        let mut gdal_3026_hack = false;

        if is_equivalent_name(&projection_name, "Mercator_1SP")
            && projection_parameter(projected_node, "center_latitude").is_none()
        {
            // GDAL ticket 3026: Mercator_1SP with a latitude of origin is Mercator_2SP
            let latitude_of_origin = projection_parameter(projected_node, "latitude_of_origin");
            if latitude_of_origin.is_some_and(|value| value != "0" && value != "0.0") {
                projection_name = "Mercator_2SP".to_string();
                gdal_3026_hack = true;
            } else {
                // GDAL WKT1 leaves out the latitude of origin of Mercator_1SP, which EPSG has
                parameter_values.push(epsg_parameter(
                    PARAMETER_LATITUDE_OF_NATURAL_ORIGIN,
                    ParameterValue::Measure(Measure::new(0.0, UnitOfMeasure::degree())),
                ));
            }
        } else if is_equivalent_name(&projection_name, "Polar_Stereographic") {
            if let Some(conversion) = polar_stereographic(projected_node, units) {
                return Ok(conversion);
            }
            try_to_identify_wkt1_method = false;
        } else if extension_proj4(projected_node).is_some() {
            // TODO: PROJ builds the conversion from the PROJ string of the EXTENSION node
            return Err(unsupported("EXTENSION PROJ4 on a projected CRS"));
        }

        let mut mapping = if try_to_identify_wkt1_method {
            mapping_from_wkt1(&projection_name)
        } else {
            None
        };
        if mapping.is_none() {
            // some WKT1 that does not look like ESRI uses ESRI projection names
            let mut parameters = ParametersByName::default();
            if let Some(esri) =
                self.esri_mapping(projected_node, projection_node, &mut parameters)?
            {
                return build_projection_from_esri(
                    base,
                    projected_node,
                    projection_node,
                    units,
                    esri,
                    &parameters,
                );
            }
        }

        if let Some(found) = mapping {
            mapping = Some(select_spherical_or_ellipsoidal(found, base));
        } else if is_equivalent_name(&projection_name, "Lambert Conformal Conic") {
            // Oracle and Trimble name both LCC variants this way, so the parameters decide
            let mut found_second_parallel = false;
            let mut found_scale_factor = false;
            for child in &projected_node.children {
                if !child.value.eq_ignore_ascii_case("PARAMETER") {
                    continue;
                }
                if child.children.len() < 2 {
                    return Err(not_enough_children("PARAMETER"));
                }
                let parameter_name = strip_quotes(&child.children[0].value);
                if is_equivalent_name(parameter_name, "standard_parallel_2") {
                    found_second_parallel = true;
                } else if is_equivalent_name(parameter_name, "scale_factor") {
                    found_scale_factor = true;
                }
            }
            mapping = match (found_second_parallel, found_scale_factor) {
                (true, false) => mapping_by_code(METHOD_LAMBERT_CONIC_CONFORMAL_2SP),
                (false, true) => mapping_by_code(METHOD_LAMBERT_CONIC_CONFORMAL_1SP),
                (true, true) => mapping_by_code(METHOD_LAMBERT_CONIC_CONFORMAL_2SP_MICHIGAN),
                (false, false) => None,
            };
        }

        // the axes tell the Krovak method from Krovak East-North Oriented
        if projection_name.eq_ignore_ascii_case("Krovak")
            && projected_node.count_children_of_name("AXIS") == 2
        {
            let first = projected_node.look_for_child_occurrence("AXIS", 0);
            let second = projected_node.look_for_child_occurrence("AXIS", 1);
            if let (Some(first), Some(second)) = (first, second) {
                let first = self.build_axis(first, units.linear, UnitType::Linear, false, 1)?;
                let second = self.build_axis(second, units.linear, UnitType::Linear, false, 2)?;
                if first.direction == AxisDirection::South
                    && second.direction == AxisDirection::West
                {
                    mapping = mapping_by_code(METHOD_KROVAK);
                }
            }
        }

        let method = match mapping {
            Some(found) => OperationMethod {
                name: found.wkt2_name.to_string(),
                identifiers: identifiers_of_code(found.epsg_code),
            },
            None => OperationMethod {
                name: projection_name.clone(),
                identifiers: Vec::new(),
            },
        };
        let mut found_parameters = vec![false; mapping.map_or(0, |found| found.params.len())];

        for child in &projected_node.children {
            if !child.value.eq_ignore_ascii_case("PARAMETER") {
                continue;
            }
            if child.children.len() < 2 {
                return Err(not_enough_children("PARAMETER"));
            }
            let value_text = &child.children[1].value;
            let wkt1_name = strip_quotes(&child.children[0].value);
            let mut parameter_name = wkt1_name.to_string();
            if gdal_3026_hack {
                if parameter_name.eq_ignore_ascii_case("latitude_of_origin") {
                    parameter_name = "standard_parallel_1".to_string();
                } else if parameter_name.eq_ignore_ascii_case("scale_factor") && value_text == "1" {
                    continue;
                }
            }
            let param =
                mapping.and_then(|found| parameter_mapping_from_wkt1(found, &parameter_name));
            let mut identifiers = Vec::new();
            let mercator_2sp_latitude = mapping
                .is_some_and(|found| found.epsg_code == METHOD_MERCATOR_VARIANT_B)
                && parameter_name.eq_ignore_ascii_case("latitude_of_origin");
            if mercator_2sp_latitude {
                // PROJ accepts the illegal latitude_of_origin of some Mercator_2SP
                parameter_name =
                    parameter_name_of_code(PARAMETER_LATITUDE_OF_NATURAL_ORIGIN).to_string();
                identifiers = vec![epsg_identifier(PARAMETER_LATITUDE_OF_NATURAL_ORIGIN)];
            } else if let (Some(found), Some(param)) = (mapping, param) {
                if let Some(index) = found
                    .params
                    .iter()
                    .position(|candidate| std::ptr::eq(*candidate, param))
                {
                    found_parameters[index] = true;
                }
                parameter_name = param.wkt2_name.to_string();
                identifiers = identifiers_of_code(param.epsg_code);
            }
            let value = as_double(value_text).map_err(|_| {
                parse_error(format!("unhandled parameter value type : {value_text}"))
            })?;
            let unit = guess_unit_for_parameter(wkt1_name, units.linear, units.angular);
            parameter_values.push(OperationParameterValue {
                parameter: OperationParameter {
                    name: parameter_name,
                    identifiers,
                },
                value: ParameterValue::Measure(Measure::new(value, unit)),
            });
        }

        if let Some(found) = mapping {
            // PROJ adds a missing scale factor at natural origin as 1, so that 0 does not slip in
            for (index, param) in found.params.iter().enumerate() {
                if !found_parameters[index]
                    && param.epsg_code == PARAMETER_SCALE_FACTOR_AT_NATURAL_ORIGIN
                {
                    parameter_values.push(epsg_parameter(
                        PARAMETER_SCALE_FACTOR_AT_NATURAL_ORIGIN,
                        ParameterValue::Measure(Measure::new(1.0, UnitOfMeasure::scale_unity())),
                    ));
                }
            }
            let hotine = found.epsg_code == METHOD_HOTINE_OBLIQUE_MERCATOR_VARIANT_A
                || found.epsg_code == METHOD_HOTINE_OBLIQUE_MERCATOR_VARIANT_B;
            if hotine {
                // GDAL WKT of Hotine Oblique Mercator can have the azimuth but not the rectified grid angle
                let found_code =
                    |code: i32| {
                        found.params.iter().enumerate().any(|(index, param)| {
                            found_parameters[index] && param.epsg_code == code
                        })
                    };
                if !found_code(PARAMETER_ANGLE_RECTIFIED_TO_SKEW_GRID)
                    && found_code(PARAMETER_AZIMUTH_PROJECTION_CENTRE)
                {
                    let azimuths: Vec<ParameterValue> = parameter_values
                        .iter()
                        .filter(|value| {
                            parameter_epsg_code(&value.parameter)
                                == PARAMETER_AZIMUTH_PROJECTION_CENTRE
                        })
                        .map(|value| value.value.clone())
                        .collect();
                    for azimuth in azimuths {
                        parameter_values.push(epsg_parameter(
                            PARAMETER_ANGLE_RECTIFIED_TO_SKEW_GRID,
                            azimuth,
                        ));
                    }
                }
            }
        }

        Ok(identified(Conversion {
            name: "unnamed".to_string(),
            identifiers: Vec::new(),
            method,
            parameter_values,
        }))
    }

    fn build_projected_crs(&mut self, node: &WktNode) -> Result<ProjectedCrs, Error> {
        let conversion_node = node.look_for_child(&["CONVERSION"]);
        let projection_node = node.look_for_child(&["PROJECTION"]);
        if conversion_node.is_none() && projection_node.is_none() {
            return Err(parse_error("missing CONVERSION node"));
        }
        let base_node = node
            .look_for_child(&["BASEGEODCRS", "BASEGEOGCRS", "GEOGCS"])
            .ok_or_else(|| parse_error("Missing BASEGEODCRS / BASEGEOGCRS / GEOGCS node"))?;
        let Crs::Geographic(mut base) = self.build_geodetic_crs(base_node)? else {
            return Err(unsupported("ProjectedCRS on a geocentric base CRS"));
        };
        let mut properties = self.build_properties(node, true)?;
        let cs_node = node.look_for_child(&["CS"]);
        if cs_node.is_none() && !is_keyword(&node.value, &["PROJCS", "BASEPROJCRS"]) {
            return Err(parse_error("missing CS node"));
        }
        let projected_name = node
            .children
            .first()
            .map(|first| strip_quotes(&first.value).to_string())
            .unwrap_or_default();
        let coordinate_system = self.build_cs(cs_node, node, &UnitOfMeasure::none())?;
        let mut cartesian = match coordinate_system {
            CoordinateSystem::Cartesian(cartesian) => Some(cartesian),
            CoordinateSystem::Ellipsoidal(_) => None,
        };

        let etrf2000_pl_cs92 = if self.esri_style {
            projected_name == "ETRF2000-PL_CS92"
                && (base.name == "GCS_ETRF2000-PL" || base.name == "ETRF2000-PL")
        } else {
            projected_name == "ETRF2000-PL / CS92" && base.name == "ETRF2000-PL"
        };
        let national_etrs89 = ((projected_name.starts_with("ETRS89 / ")
            || (self.esri_style && projected_name.starts_with("ETRS_1989_")))
            && base.name == "ETRS89")
            || projected_name.starts_with("ETRF2000-PL /");
        if etrf2000_pl_cs92 {
            if let Some(epsg_2180) = registered_projected_crs(&epsg_identifier(2180)) {
                properties.name = epsg_2180.name;
                base = *epsg_2180.base_crs;
            }
        } else if national_etrs89 && base.coordinate_system.axes.len() == 2 {
            // EPSG v12.025 moved the Norway projected CRSs from ETRS89 to ETRS89-NOR [EUREF89], and later releases other national realizations
            let candidate = projected_crs_named(&projected_name).filter(|candidate| {
                candidate.base_crs.name != base.name
                    && candidate.base_crs.is_equivalent_to(
                        &with_datum_alias_resolved(&base, &candidate.base_crs),
                        Criterion::EquivalentExceptAxisOrderGeogcrs,
                    )
            });
            if let Some(candidate) = candidate {
                properties.name = candidate.name;
                base = *candidate.base_crs;
            }
        }
        let mut projected_name = projected_name;
        if let (true, Some(cartesian)) = (self.esri_style, &cartesian) {
            if let Some(official) = official_name_from_esri_alias(&projected_name, "projected_crs")
            {
                let mut official_name = official.name.to_string();
                // PROJ issue 2086: the _FIPS_ names of the US survey foot CRSs lack their _Feet suffix
                let us_foot = 12.0 / 39.37;
                if projected_name.contains("_FIPS_")
                    && !projected_name.contains("_Feet")
                    && (cartesian.axes[0].unit.conversion_to_si - us_foot).abs() < 1e-10 * us_foot
                {
                    if let Some(feet) = official_name_from_esri_alias(
                        &format!("{projected_name}_Feet"),
                        "projected_crs",
                    ) {
                        official_name = feet.name.to_string();
                    }
                }
                projected_name = official_name.clone();
                properties.name = official_name;
            }
        }

        let web_mercator_extension = conversion_node.is_none()
            && projection_node.is_some_and(|projection_node| {
                has_web_mercator_proj4_string(node, projection_node)
            });
        let web_mercator_name = is_equivalent_name(&projected_name, "WGS_84_Pseudo_Mercator")
            || is_equivalent_name(&projected_name, "WGS_1984_Web_Mercator");
        if let (true, Some(cartesian)) = (web_mercator_extension || web_mercator_name, &cartesian) {
            self.towgs84_parameters.clear();
            return Ok(pseudo_mercator(properties, cartesian.clone()));
        }

        let (linear_unit, angular_unit) = if conversion_node.is_some() {
            (UnitOfMeasure::metre(), UnitOfMeasure::degree())
        } else {
            (
                self.build_unit_in_sub_node(node, UnitType::Linear)?,
                base.coordinate_system.axes[0].unit.clone(),
            )
        };
        let conversion = match (conversion_node, projection_node) {
            (Some(conversion_node), _) => {
                self.build_conversion(conversion_node, &linear_unit, &angular_unit)?
            }
            (None, Some(projection_node)) => {
                self.build_projection(&base, node, projection_node, &linear_unit, &angular_unit)?
            }
            (None, None) => return Err(parse_error("missing CONVERSION node")),
        };

        if cs_node.is_none() && node.count_children_of_name("AXIS") == 0 {
            let method_code = method_epsg_code(&conversion.method);
            let values = &conversion.parameter_values;
            let degree = UnitOfMeasure::degree();
            if method_code == METHOD_KROVAK {
                cartesian = Some(CartesianCs {
                    axes: vec![
                        axis("Southing", "", AxisDirection::South, &linear_unit),
                        axis("Westing", "", AxisDirection::West, &linear_unit),
                    ],
                });
            } else if method_code == METHOD_POLAR_STEREOGRAPHIC_VARIANT_A
                || method_code == METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA
            {
                // ESRI and WKT1 of the UPS CRSs likely use easting, northing rather than EPSG's order
                let latitude_of_origin =
                    parameter_value_in_unit(values, PARAMETER_LATITUDE_OF_NATURAL_ORIGIN, &degree);
                if (latitude_of_origin - 90.0).abs() < 1e-10 {
                    cartesian = Some(north_pole_easting_south_northing_south(&linear_unit));
                } else if (latitude_of_origin + 90.0).abs() < 1e-10 {
                    cartesian = Some(south_pole_easting_north_northing_north(&linear_unit));
                }
            } else if method_code == METHOD_POLAR_STEREOGRAPHIC_VARIANT_B {
                let latitude_of_standard_parallel =
                    parameter_value_in_unit(values, PARAMETER_LATITUDE_STD_PARALLEL, &degree);
                if latitude_of_standard_parallel > 0.0 {
                    cartesian = Some(north_pole_easting_south_northing_south(&linear_unit));
                } else if latitude_of_standard_parallel < 0.0 {
                    cartesian = Some(south_pole_easting_north_northing_north(&linear_unit));
                }
            } else if method_code == METHOD_TRANSVERSE_MERCATOR_SOUTH_ORIENTATED {
                cartesian = Some(CartesianCs {
                    axes: vec![
                        axis("Easting", "Y", AxisDirection::West, &linear_unit),
                        axis("Northing", "X", AxisDirection::South, &linear_unit),
                    ],
                });
            }
        }
        let cartesian = cartesian.ok_or_else(|| parse_error("CS node is not of type Cartesian"))?;

        if cartesian.axes.len() == 3 && base.coordinate_system.axes.len() == 2 {
            // GeographicCRS::promoteTo3D without a database identifies no 3D CRS, so the identifiers go
            base.coordinate_system.axes.push(axis(
                "Ellipsoidal height",
                "h",
                AxisDirection::Up,
                &UnitOfMeasure::metre(),
            ));
            base.identifiers.clear();
        }
        if extension_proj4(node).is_some() && !web_mercator_extension {
            return Err(unsupported("EXTENSION PROJ4 on a projected CRS"));
        }
        Ok(ProjectedCrs {
            name: properties.name,
            identifiers: properties.identifiers,
            remarks: properties.remarks,
            domains: properties.domains,
            base_crs: Box::new(base),
            conversion,
            coordinate_system: cartesian,
        })
    }

    fn build_bound_crs(&mut self, node: &WktNode) -> Result<BoundCrs, Error> {
        let abridged_node = node
            .look_for_child(&["ABRIDGEDTRANSFORMATION"])
            .ok_or_else(|| not_enough_children("ABRIDGEDTRANSFORMATION"))?;
        let method_node = abridged_node
            .look_for_child(&["METHOD"])
            .ok_or_else(|| parse_error("missing METHOD node"))?;
        if method_node.children.is_empty() {
            return Err(not_enough_children("METHOD"));
        }
        let single_crs = |keyword: &str| {
            node.look_for_child(&[keyword])
                .filter(|wrapper| wrapper.children.len() == 1)
                .map(|wrapper| &wrapper.children[0])
                .ok_or_else(|| not_enough_children(keyword))
        };
        let source_crs = self.build_crs(single_crs("SOURCECRS")?)?;
        let target_crs = self.build_crs(single_crs("TARGETCRS")?)?;
        let parameter_values = self.consume_parameters(
            abridged_node,
            true,
            &UnitOfMeasure::none(),
            &UnitOfMeasure::none(),
        )?;
        let transformation_source = bound_crs_transformation_source(&source_crs, &target_crs)
            .map_err(|_| parse_error("Cannot find GeographicCRS or VerticalCRS in sourceCRS"))?;
        let (transformation_name, transformation_identifiers) = self
            .build_properties(abridged_node, true)?
            .without_usage(&abridged_node.value)?;
        let (method_name, method_identifiers) = self
            .build_properties(method_node, true)?
            .without_usage(&method_node.value)?;
        let properties = self.build_properties(node, false)?;
        Ok(BoundCrs {
            name: source_crs.name().to_string(),
            identifiers: properties.identifiers,
            remarks: properties.remarks,
            domains: properties.domains,
            transformation: Transformation {
                name: transformation_name,
                identifiers: transformation_identifiers,
                source_crs: Box::new(transformation_source),
                target_crs: Box::new(target_crs.clone()),
                method: OperationMethod {
                    name: method_name,
                    identifiers: method_identifiers,
                },
                parameter_values,
                remarks: String::new(),
                domains: Vec::new(),
                interpolation_crs: None,
                accuracies: Vec::new(),
            },
            source_crs: Box::new(source_crs),
            target_crs: Box::new(target_crs),
        })
    }
}

struct DefaultUnits<'a> {
    linear: &'a UnitOfMeasure,
    angular: &'a UnitOfMeasure,
}

// io.cpp keeps ESRI parameters in a map compared without case, sorted the same way
#[derive(Default)]
struct ParametersByName {
    by_lowercase_name: BTreeMap<String, (String, String)>,
}

impl ParametersByName {
    fn insert(&mut self, name: &str, value: &str) {
        let entry = self
            .by_lowercase_name
            .entry(name.to_ascii_lowercase())
            .or_insert_with(|| (name.to_string(), String::new()));
        entry.1 = value.to_string();
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.by_lowercase_name
            .get(&name.to_ascii_lowercase())
            .map(|(_, value)| value.as_str())
    }

    fn find_equivalent(&self, name: &str) -> Option<&str> {
        self.get(name).or_else(|| {
            self.by_lowercase_name
                .values()
                .find(|(original, _)| are_equivalent_parameters(original, name))
                .map(|(_, value)| value.as_str())
        })
    }
}

fn identifiers_of_code(code: i32) -> Vec<Identifier> {
    if code == 0 {
        Vec::new()
    } else {
        vec![epsg_identifier(code)]
    }
}

fn with_datum_alias_resolved(crs: &GeographicCrs, reference: &GeographicCrs) -> GeographicCrs {
    // IdentifiedObject::hasEquivalentNameToUsingAlias: a datum named by an alias of the reference datum takes its name
    let (GeodeticDatum::ReferenceFrame(frame), GeodeticDatum::ReferenceFrame(reference_frame)) =
        (&crs.datum, &reference.datum)
    else {
        return crs.clone();
    };
    let Some(reference_identifier) = reference_frame.identifiers.first() else {
        return crs.clone();
    };
    let is_alias = geodetic_datum_aliases(reference_identifier)
        .iter()
        .any(|alias| is_equivalent_name(alias, &frame.name));
    if !is_alias {
        return crs.clone();
    }
    let mut resolved = crs.clone();
    if let GeodeticDatum::ReferenceFrame(resolved_frame) = &mut resolved.datum {
        resolved_frame.name = reference_frame.name.clone();
    }
    resolved
}

fn normalize_axis_name(name: &str) -> String {
    // CoordinateSystemAxis::normalizeAxisName turns the WKT2 "longitude" into EPSG's "Longitude"
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn alter_angular_unit(cs: &EllipsoidalCs, unit: &UnitOfMeasure) -> EllipsoidalCs {
    // EllipsoidalCS::alterAngularUnit changes the latitude and longitude axes
    let mut altered = cs.clone();
    for axis in altered.axes.iter_mut().take(2) {
        axis.unit = unit.clone();
    }
    altered
}

fn north_pole_easting_south_northing_south(unit: &UnitOfMeasure) -> CartesianCs {
    CartesianCs {
        axes: vec![
            with_meridian(axis("Easting", "E", AxisDirection::South, unit), 90.0),
            with_meridian(axis("Northing", "N", AxisDirection::South, unit), 180.0),
        ],
    }
}

fn south_pole_easting_north_northing_north(unit: &UnitOfMeasure) -> CartesianCs {
    CartesianCs {
        axes: vec![
            with_meridian(axis("Easting", "E", AxisDirection::North, unit), 90.0),
            with_meridian(axis("Northing", "N", AxisDirection::North, unit), 0.0),
        ],
    }
}

fn extension_proj4(node: &WktNode) -> Option<String> {
    // WKTParser::Private::getExtensionProj4
    let extension = node.look_for_child(&["EXTENSION"])?;
    let [kind, value] = extension.children.as_slice() else {
        return None;
    };
    strip_quotes(&kind.value)
        .eq_ignore_ascii_case("PROJ4")
        .then(|| strip_quotes(&value.value).to_string())
}

fn projection_parameter<'a>(projected_node: &'a WktNode, name: &str) -> Option<&'a str> {
    // WKTParser::Private::projectionGetParameter
    projected_node
        .children
        .iter()
        .filter(|child| child.value.eq_ignore_ascii_case("PARAMETER") && child.children.len() == 2)
        .find(|child| is_equivalent_name(strip_quotes(&child.children[0].value), name))
        .map(|child| child.children[1].value.as_str())
}

fn has_web_mercator_proj4_string(projected_node: &WktNode, projection_node: &WktNode) -> bool {
    // GDAL WKT1 encodes web mercator as Mercator_1SP with a PROJ4 extension
    let Some(projection_name) = projection_node.children.first() else {
        return false;
    };
    if !is_equivalent_name(strip_quotes(&projection_name.value), "Mercator_1SP")
        || projected_node.count_children_of_name("center_latitude") != 0
    {
        return false;
    }
    let Some(proj_string) = extension_proj4(projected_node) else {
        return false;
    };
    let has = |part: &str| proj_string.contains(part);
    has("+proj=merc")
        && has("+a=6378137")
        && has("+b=6378137")
        && has("+lon_0=0")
        && has("+x_0=0")
        && has("+y_0=0")
        && has("+nadgrids=@null")
        && (!has("+lat_ts=") || has("+lat_ts=0"))
        && (!has("+k=") || has("+k=1"))
        && (!has("+units=") || has("+units=m"))
}

fn pseudo_mercator(properties: Properties, coordinate_system: CartesianCs) -> ProjectedCrs {
    // createPseudoMercator
    let conversion = conversion_from_method_code(
        METHOD_POPULAR_VISUALISATION_PSEUDO_MERCATOR,
        vec![
            Measure::new(0.0, UnitOfMeasure::degree()),
            Measure::new(0.0, UnitOfMeasure::degree()),
            Measure::new(0.0, UnitOfMeasure::metre()),
            Measure::new(0.0, UnitOfMeasure::metre()),
        ],
    );
    ProjectedCrs {
        name: properties.name,
        identifiers: properties.identifiers,
        remarks: properties.remarks,
        domains: properties.domains,
        base_crs: Box::new(wgs84_geographic_crs()),
        conversion: Conversion {
            name: "unnamed".to_string(),
            ..conversion
        },
        coordinate_system,
    }
}

fn guess_unit_for_parameter(
    name: &str,
    default_linear_unit: &UnitOfMeasure,
    default_angular_unit: &UnitOfMeasure,
) -> UnitOfMeasure {
    // "scale" comes first because of "Scale factor on pseudo standard parallel"
    let has = |part: &str| contains_ignoring_case(name, part);
    if has("scale") || has("scaling factor") {
        return UnitOfMeasure::scale_unity();
    }
    let angular = [
        "latitude",
        "longitude",
        "meridian",
        "parallel",
        "azimuth",
        "angle",
        "heading",
        "rotation",
    ];
    if angular.iter().any(|part| has(part)) {
        return default_angular_unit.clone();
    }
    if has("easting") || has("northing") || has("height") {
        return default_linear_unit.clone();
    }
    UnitOfMeasure {
        name: String::new(),
        conversion_to_si: 1.0,
        unit_type: UnitType::Unknown,
        code_space: String::new(),
        code: String::new(),
    }
}

fn convert_from_abridged(
    name: &str,
    value: &mut f64,
    epsg_code: i32,
) -> Option<(UnitOfMeasure, i32)> {
    // OperationParameterValue::convertFromAbridged
    let matches =
        |code: i32| epsg_code == code || is_equivalent_name(name, parameter_name_of_code(code));
    for code in [
        PARAMETER_X_AXIS_TRANSLATION,
        PARAMETER_Y_AXIS_TRANSLATION,
        PARAMETER_Z_AXIS_TRANSLATION,
    ] {
        if matches(code) {
            return Some((UnitOfMeasure::metre(), code));
        }
    }
    for code in [
        PARAMETER_X_AXIS_ROTATION,
        PARAMETER_Y_AXIS_ROTATION,
        PARAMETER_Z_AXIS_ROTATION,
    ] {
        if matches(code) {
            return Some((UnitOfMeasure::arc_second(), code));
        }
    }
    if matches(PARAMETER_SCALE_DIFFERENCE) {
        *value = (*value - 1.0) * 1e6;
        return Some((
            UnitOfMeasure::parts_per_million(),
            PARAMETER_SCALE_DIFFERENCE,
        ));
    }
    None
}

fn select_spherical_or_ellipsoidal(
    mapping: &'static MethodMapping,
    base: &GeographicCrs,
) -> &'static MethodMapping {
    let sphere = base.datum.ellipsoid().is_sphere();
    let pick = |spherical: i32, ellipsoidal: i32| {
        mapping_by_code(if sphere { spherical } else { ellipsoidal }).unwrap_or(mapping)
    };
    match mapping.epsg_code {
        METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA_SPHERICAL | METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA => {
            pick(
                METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA_SPHERICAL,
                METHOD_LAMBERT_CYLINDRICAL_EQUAL_AREA,
            )
        }
        METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA_SPHERICAL | METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA => {
            pick(
                METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA_SPHERICAL,
                METHOD_LAMBERT_AZIMUTHAL_EQUAL_AREA,
            )
        }
        METHOD_EQUIDISTANT_CYLINDRICAL_SPHERICAL | METHOD_EQUIDISTANT_CYLINDRICAL => pick(
            METHOD_EQUIDISTANT_CYLINDRICAL_SPHERICAL,
            METHOD_EQUIDISTANT_CYLINDRICAL,
        ),
        _ => mapping,
    }
}

fn polar_stereographic(projected_node: &WktNode, units: &DefaultUnits) -> Option<Conversion> {
    // the Polar_Stereographic branch of buildProjectionStandard
    let mut parameters: BTreeMap<String, Measure> = BTreeMap::new();
    for child in &projected_node.children {
        if !child.value.eq_ignore_ascii_case("PARAMETER") || child.children.len() != 2 {
            continue;
        }
        let name = strip_quotes(&child.children[0].value);
        if let Ok(value) = as_double(&child.children[1].value) {
            let unit = guess_unit_for_parameter(name, units.linear, units.angular);
            parameters.insert(name.to_ascii_lowercase(), Measure::new(value, unit));
        }
    }
    let parameter = |name: &str| {
        parameters
            .get(name)
            .cloned()
            .unwrap_or_else(|| Measure::new(0.0, UnitOfMeasure::none()))
    };
    let latitude_of_origin = parameter("latitude_of_origin");
    let central_meridian = parameter("central_meridian");
    let scale_factor = match parameters.get("scale_factor") {
        Some(measure) if !measure.unit.name.is_empty() => measure.clone(),
        _ => Measure::new(1.0, UnitOfMeasure::scale_unity()),
    };
    let false_easting = parameter("false_easting");
    let false_northing = parameter("false_northing");
    let has_latitude = !latitude_of_origin.unit.name.is_empty();
    let unnamed = |conversion: Conversion| Conversion {
        name: "unnamed".to_string(),
        ..conversion
    };
    if has_latitude && scale_factor.si_value() == 1.0 {
        return Some(unnamed(conversion_from_method_code(
            METHOD_POLAR_STEREOGRAPHIC_VARIANT_B,
            vec![
                latitude_of_origin,
                central_meridian,
                false_easting,
                false_northing,
            ],
        )));
    }
    let latitude_degrees = latitude_of_origin.convert_to_unit(&UnitOfMeasure::degree());
    if has_latitude && (latitude_degrees.abs() - 90.0).abs() < 1e-10 {
        return Some(unnamed(conversion_from_method_code(
            METHOD_POLAR_STEREOGRAPHIC_VARIANT_A,
            vec![
                latitude_of_origin,
                central_meridian,
                scale_factor,
                false_easting,
                false_northing,
            ],
        )));
    }
    None
}

struct UtmZone {
    zone: i32,
    north: bool,
}

const PARAMETER_LONGITUDE_OF_ORIGIN: i32 = 8833;

fn utm_zone(conversion: &Conversion) -> Option<UtmZone> {
    // Conversion::isUTM
    if method_epsg_code(&conversion.method) != METHOD_TRANSVERSE_MERCATOR {
        return None;
    }
    let mut latitude_of_origin_is_utm = false;
    let mut scale_factor_is_utm = false;
    let mut false_easting_is_utm = false;
    let mut false_northing_is_utm = false;
    let mut zone = 0;
    let mut north = true;
    for value in &conversion.parameter_values {
        let ParameterValue::Measure(measure) = &value.value else {
            continue;
        };
        let code = parameter_epsg_code(&value.parameter);
        if code == PARAMETER_LATITUDE_OF_NATURAL_ORIGIN && measure.value.abs() < 1e-10 {
            latitude_of_origin_is_utm = true;
        } else if (code == PARAMETER_LONGITUDE_OF_NATURAL_ORIGIN
            || code == PARAMETER_LONGITUDE_OF_ORIGIN)
            && measure.unit.is_equivalent_to(&UnitOfMeasure::degree())
        {
            let zone_number = (measure.value + 183.0) / 6.0;
            if zone_number > 0.9
                && zone_number < 60.1
                && (zone_number - zone_number.round()).abs() < 1e-10
            {
                zone = zone_number.round() as i32;
            }
        } else if code == PARAMETER_SCALE_FACTOR_AT_NATURAL_ORIGIN
            && measure.unit.is_equivalent_to(&UnitOfMeasure::scale_unity())
            && (measure.value - 0.9996).abs() < 1e-10
        {
            scale_factor_is_utm = true;
        } else if code == PARAMETER_FALSE_EASTING
            && measure.value == 500000.0
            && measure.unit.is_equivalent_to(&UnitOfMeasure::metre())
        {
            false_easting_is_utm = true;
        } else if code == PARAMETER_FALSE_NORTHING
            && measure.unit.is_equivalent_to(&UnitOfMeasure::metre())
        {
            if measure.value.abs() < 1e-10 {
                false_northing_is_utm = true;
                north = true;
            } else if (measure.value - 10000000.0).abs() < 1e-10 {
                false_northing_is_utm = true;
                north = false;
            }
        }
    }
    let is_utm = latitude_of_origin_is_utm
        && zone > 0
        && scale_factor_is_utm
        && false_easting_is_utm
        && false_northing_is_utm;
    is_utm.then_some(UtmZone { zone, north })
}

fn identified(conversion: Conversion) -> Conversion {
    // Conversion::identify names a UTM conversion as EPSG does, with its EPSG code
    let Some(UtmZone { zone, north }) = utm_zone(&conversion) else {
        return conversion;
    };
    let (hemisphere, first_code) = if north { ('N', 16000) } else { ('S', 16100) };
    Conversion {
        name: format!("UTM zone {zone}{hemisphere}"),
        identifiers: vec![epsg_identifier(first_code + zone)],
        ..conversion
    }
}

fn build_projection_from_esri(
    base: &GeographicCrs,
    projected_node: &WktNode,
    projection_node: &WktNode,
    units: &DefaultUnits,
    esri_mapping: &'static EsriMethodMapping,
    parameters: &ParametersByName,
) -> Result<Conversion, Error> {
    // WKTParser::Private::buildProjectionFromESRI
    let mut wkt2_to_esri: BTreeMap<&str, &str> = esri_mapping
        .params
        .iter()
        .filter_map(|param| {
            param
                .wkt2_name
                .map(|wkt2_name| (wkt2_name, param.esri_name))
        })
        .collect();
    let esri_name = strip_quotes(&projection_node.children[0].value);
    let mut method_name = esri_mapping.wkt2_name;
    if esri_name.eq_ignore_ascii_case("Krovak") {
        let projected_name = projected_node
            .children
            .first()
            .map(|first| strip_quotes(&first.value))
            .unwrap_or_default();
        if projected_name.contains("_East_North") {
            method_name = method_name_of_code(METHOD_KROVAK_NORTH_ORIENTED);
        }
    }
    let mut wkt2_mapping = mapping_by_wkt2_name(method_name);
    if esri_name.eq_ignore_ascii_case("Stereographic") {
        let polar = parameters
            .get("Latitude_Of_Origin")
            .and_then(|value| as_double(value).ok())
            .is_some_and(|latitude| latitude.abs() == 90.0);
        if polar {
            wkt2_mapping = mapping_by_code(METHOD_POLAR_STEREOGRAPHIC_VARIANT_A);
        }
    }
    let wkt2_mapping =
        wkt2_mapping.ok_or_else(|| unsupported(&format!("ESRI projection {esri_name}")))?;
    let wkt2_mapping = select_spherical_or_ellipsoidal(wkt2_mapping, base);

    let mut extra_parameters = ParametersByName::default();
    if wkt2_mapping.epsg_code == METHOD_EQUIDISTANT_CYLINDRICAL
        && esri_name.eq_ignore_ascii_case("Plate_Carree")
    {
        // Equidistant Cylindrical expects a latitude of 1st standard parallel, zero for Plate Carree
        wkt2_to_esri.insert(
            parameter_name_of_code(PARAMETER_LATITUDE_1ST_STD_PARALLEL),
            "Standard_Parallel_1",
        );
        extra_parameters.insert("Standard_Parallel_1", "0");
    } else if (wkt2_mapping.epsg_code == METHOD_HOTINE_OBLIQUE_MERCATOR_VARIANT_A
        || wkt2_mapping.epsg_code == METHOD_HOTINE_OBLIQUE_MERCATOR_VARIANT_B)
        && !esri_name.eq_ignore_ascii_case("Rectified_Skew_Orthomorphic_Natural_Origin")
        && !esri_name.eq_ignore_ascii_case("Rectified_Skew_Orthomorphic_Center")
    {
        // ESRI WKT lacks the angle to the skew grid, which takes the azimuth
        wkt2_to_esri.insert(
            parameter_name_of_code(PARAMETER_ANGLE_RECTIFIED_TO_SKEW_GRID),
            "Azimuth",
        );
    }

    let mut parameter_values = Vec::new();
    for param in wkt2_mapping.params {
        let Some(esri_parameter_name) = wkt2_to_esri.get(param.wkt2_name) else {
            continue;
        };
        let value_text = extra_parameters
            .find_equivalent(esri_parameter_name)
            .or_else(|| parameters.find_equivalent(esri_parameter_name));
        let Some(value_text) = value_text else {
            continue;
        };
        let value = as_double(value_text)
            .map_err(|_| parse_error(format!("unhandled parameter value type : {value_text}")))?;
        let unit = guess_unit_for_parameter(param.wkt2_name, units.linear, units.angular);
        parameter_values.push(OperationParameterValue {
            parameter: OperationParameter {
                name: param.wkt2_name.to_string(),
                identifiers: identifiers_of_code(param.epsg_code),
            },
            value: ParameterValue::Measure(Measure::new(value, unit)),
        });
    }
    let name = if esri_name == "Gauss_Kruger" {
        // PROJ 9.8.1 spells it this way
        "unnnamed (Gauss Kruger)"
    } else {
        "unnamed"
    };
    Ok(identified(Conversion {
        name: name.to_string(),
        identifiers: Vec::new(),
        method: OperationMethod {
            name: wkt2_mapping.wkt2_name.to_string(),
            identifiers: identifiers_of_code(wkt2_mapping.epsg_code),
        },
        parameter_values,
    }))
}

fn parse_dynamic(dynamic_node: &WktNode) -> Result<DynamicFrame, Error> {
    // WKTParser::Private::parseDynamic
    let frame_epoch = dynamic_node
        .look_for_child(&["FRAMEEPOCH"])
        .and_then(|epoch| epoch.children.first())
        .ok_or_else(|| parse_error("missing FRAMEEPOCH node"))?;
    let frame_reference_epoch =
        as_double(&frame_epoch.value).map_err(|_| parse_error("Invalid FRAMEEPOCH node"))?;
    let deformation_model_name = dynamic_node
        .look_for_child(&["MODEL", "VELOCITYGRID"])
        .filter(|model| model.children.len() == 1)
        .map(|model| strip_quotes(&model.children[0].value).to_string());
    Ok(DynamicFrame {
        frame_reference_epoch,
        deformation_model_name,
    })
}

fn geodetic_source_of(crs: &Crs) -> Result<Crs, Error> {
    // CRS::extractGeodeticCRS
    match crs {
        Crs::Geographic(geographic) => Ok(Crs::Geographic(geographic.clone())),
        Crs::Geodetic(geodetic) => Ok(Crs::Geodetic(geodetic.clone())),
        Crs::Projected(projected) => Ok(Crs::Geographic((*projected.base_crs).clone())),
        Crs::Bound(_) => Err(parse_error(
            "Cannot find GeodeticCRS in sourceCRS of TOWGS84 transformation",
        )),
    }
}

fn bound_crs_from_towgs84(base: Crs, parameters: &[f64]) -> Result<BoundCrs, Error> {
    // BoundCRS::createFromTOWGS84 and Transformation::createTOWGS84, seven position vector parameters
    let source = geodetic_source_of(&base)?;
    let (target, method_code) = match &source {
        Crs::Geodetic(_) => (
            Crs::Geodetic(wgs84_geocentric_crs()),
            METHOD_POSITION_VECTOR_GEOCENTRIC,
        ),
        Crs::Geographic(geographic) if geographic.coordinate_system.axes.len() == 2 => (
            Crs::Geographic(wgs84_geographic_crs()),
            METHOD_POSITION_VECTOR_GEOGRAPHIC_2D,
        ),
        _ => (
            Crs::Geographic(wgs84_geographic_crs()),
            METHOD_POSITION_VECTOR_GEOGRAPHIC_3D,
        ),
    };
    let units = [
        (PARAMETER_X_AXIS_TRANSLATION, UnitOfMeasure::metre()),
        (PARAMETER_Y_AXIS_TRANSLATION, UnitOfMeasure::metre()),
        (PARAMETER_Z_AXIS_TRANSLATION, UnitOfMeasure::metre()),
        (PARAMETER_X_AXIS_ROTATION, UnitOfMeasure::arc_second()),
        (PARAMETER_Y_AXIS_ROTATION, UnitOfMeasure::arc_second()),
        (PARAMETER_Z_AXIS_ROTATION, UnitOfMeasure::arc_second()),
        (
            PARAMETER_SCALE_DIFFERENCE,
            UnitOfMeasure::parts_per_million(),
        ),
    ];
    let parameter_values = units
        .into_iter()
        .zip(parameters)
        .map(|((code, unit), value)| {
            epsg_parameter(code, ParameterValue::Measure(Measure::new(*value, unit)))
        })
        .collect();
    let transformation = Transformation {
        name: format!("Transformation from {} to WGS84", source.name()),
        identifiers: Vec::new(),
        source_crs: Box::new(source),
        target_crs: Box::new(target.clone()),
        method: epsg_method(method_code),
        parameter_values,
        remarks: String::new(),
        domains: Vec::new(),
        interpolation_crs: None,
        accuracies: Vec::new(),
    };
    Ok(BoundCrs {
        name: base.name().to_string(),
        identifiers: Vec::new(),
        remarks: String::new(),
        domains: Vec::new(),
        source_crs: Box::new(base),
        target_crs: Box::new(target),
        transformation,
    })
}

fn bound_crs_from_nadgrids(base: Crs, filename: String) -> Result<BoundCrs, Error> {
    // BoundCRS::createFromNadgrids
    let source = match base.extract_geographic_crs() {
        None => base.clone(),
        Some(geographic) => {
            let prime_meridian = match &geographic.datum {
                GeodeticDatum::ReferenceFrame(frame) => frame.prime_meridian.clone(),
                GeodeticDatum::Ensemble(ensemble) => ensemble.members[0].prime_meridian.clone(),
            };
            if prime_meridian.longitude.si_value() == 0.0 {
                Crs::Geographic(geographic.clone())
            } else {
                let suffix = " (with Greenwich prime meridian)";
                let datum = match &geographic.datum {
                    GeodeticDatum::ReferenceFrame(frame) => (**frame).clone(),
                    GeodeticDatum::Ensemble(ensemble) => ensemble.as_datum(),
                };
                Crs::Geographic(GeographicCrs {
                    name: format!("{}{suffix}", geographic.name),
                    identifiers: Vec::new(),
                    remarks: String::new(),
                    domains: Vec::new(),
                    datum: GeodeticDatum::ReferenceFrame(Box::new(GeodeticReferenceFrame {
                        name: format!("{}{suffix}", datum.name),
                        identifiers: Vec::new(),
                        remarks: String::new(),
                        domains: Vec::new(),
                        anchor: None,
                        anchor_epoch: None,
                        ellipsoid: datum.ellipsoid,
                        prime_meridian: PrimeMeridian::greenwich(),
                        dynamic: None,
                    })),
                    coordinate_system: EllipsoidalCs::latitude_longitude(&UnitOfMeasure::degree()),
                    coordinate_system_is_implicit: false,
                })
            }
        }
    };
    let target = Crs::Geographic(wgs84_geographic_crs());
    let transformation = Transformation {
        name: format!("{} to WGS84", source.name()),
        identifiers: Vec::new(),
        source_crs: Box::new(source),
        target_crs: Box::new(target.clone()),
        method: epsg_method(METHOD_NTV2),
        parameter_values: vec![epsg_parameter(
            PARAMETER_LATITUDE_LONGITUDE_DIFFERENCE_FILE,
            ParameterValue::Filename(filename),
        )],
        remarks: String::new(),
        domains: Vec::new(),
        interpolation_crs: None,
        accuracies: Vec::new(),
    };
    Ok(BoundCrs {
        name: base.name().to_string(),
        identifiers: Vec::new(),
        remarks: String::new(),
        domains: Vec::new(),
        source_crs: Box::new(base),
        target_crs: Box::new(target),
        transformation,
    })
}
