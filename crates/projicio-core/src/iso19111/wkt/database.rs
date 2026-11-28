// adapted from PROJ src/iso19111/factory.cpp DatabaseContext and AuthorityFactory (MIT), see LICENSES/PROJ-MIT.txt

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::iso19111::metadata::Identifier;

type ByTable<V> = HashMap<&'static str, HashMap<&'static str, V>>;

#[derive(Default)]
struct EsriNames {
    alias_by_code: ByTable<&'static str>,
    alias_by_official_name: ByTable<&'static str>,
    official_by_alias: ByTable<(&'static str, &'static str)>,
    esri_object_names: HashMap<&'static str, HashSet<&'static str>>,
    epsg_old_aliases: ByTable<&'static str>,
    datum_names: HashSet<&'static str>,
    esri_projected_definitions: HashMap<&'static str, &'static str>,
    esri_vertical_by_datum_name:
        HashMap<&'static str, ((&'static str, &'static str), &'static str)>,
}

#[cfg(feature = "registry")]
fn genuine_table(object_table: &'static str) -> &'static str {
    match object_table {
        "geographic_2D_crs" | "geographic_3D_crs" | "geocentric_crs" => "geodetic_crs",
        other => other,
    }
}

#[cfg(feature = "registry")]
impl EsriNames {
    fn parse(text: &'static str) -> Self {
        // tools/gen-wkt-tables/generate.py writes this table from proj.db and documents each line kind
        let mut names = Self::default();
        let mut by_name_overrides = Vec::new();
        let mut official_overrides = Vec::new();
        for line in text.lines() {
            let fields: Vec<&'static str> = line.split('\t').collect();
            match fields.as_slice() {
                ["object", _, _, _, ""] => {}
                ["object", table, code, name, alias] => {
                    names
                        .alias_by_code
                        .entry(genuine_table(table))
                        .or_default()
                        .insert(*code, *alias);
                    if *table != "geocentric_crs" {
                        names
                            .alias_by_official_name
                            .entry(*table)
                            .or_default()
                            .entry(*name)
                            .or_insert(*alias);
                    }
                    names
                        .official_by_alias
                        .entry(genuine_table(table))
                        .or_default()
                        .entry(*alias)
                        .or_insert((*name, *code));
                }
                ["by_name", table, name, alias] => by_name_overrides.push((*table, *name, *alias)),
                ["official", table, alias, name, code] => {
                    official_overrides.push((*table, *alias, *name, *code));
                }
                ["esri_object", table, name, _] => {
                    names
                        .esri_object_names
                        .entry(*table)
                        .or_default()
                        .insert(*name);
                }
                ["epsg_old", table, code, alias] => {
                    names
                        .epsg_old_aliases
                        .entry(*table)
                        .or_default()
                        .insert(*code, *alias);
                }
                ["datum_name", _, name, _] => {
                    names.datum_names.insert(*name);
                }
                ["esri_definition", _, name, definition] => {
                    names.esri_projected_definitions.insert(*name, *definition);
                }
                ["esri_vertical", datum_name, datum_code, vertical_crs_name] => {
                    let datum = datum_code
                        .split_once(':')
                        .unwrap_or_else(|| panic!("esri_names.tsv has a malformed line: {line}"));
                    names
                        .esri_vertical_by_datum_name
                        .insert(*datum_name, (datum, *vertical_crs_name));
                }
                _ => panic!("esri_names.tsv has a malformed line: {line}"),
            }
        }
        for (table, name, alias) in by_name_overrides {
            let by_name = names.alias_by_official_name.entry(table).or_default();
            if alias.is_empty() {
                by_name.remove(name);
            } else {
                by_name.insert(name, alias);
            }
        }
        for (table, alias, name, code) in official_overrides {
            let official = names.official_by_alias.entry(table).or_default();
            if name.is_empty() {
                official.remove(alias);
            } else {
                official.insert(alias, (name, code));
            }
        }
        names
    }
}

fn esri_names() -> &'static EsriNames {
    static NAMES: OnceLock<EsriNames> = OnceLock::new();
    #[cfg(feature = "registry")]
    {
        NAMES.get_or_init(|| EsriNames::parse(include_str!("../../../data/esri_names.tsv")))
    }
    #[cfg(not(feature = "registry"))]
    {
        NAMES.get_or_init(EsriNames::default)
    }
}

fn lookup<V: Copy>(by_table: &ByTable<V>, table: &str, key: &str) -> Option<V> {
    by_table.get(table)?.get(key).copied()
}

fn code_key(identifier: &Identifier) -> String {
    format!("{}:{}", identifier.code_space, identifier.code)
}

pub(super) fn esri_alias_of_code(identifier: &Identifier, table: &str) -> Option<&'static str> {
    // DatabaseContext::getAliases(auth, code, "", table, "ESRI") finding one alias
    lookup(&esri_names().alias_by_code, table, &code_key(identifier))
}

pub(super) fn esri_alias_from_official_name(name: &str, table: &str) -> Option<&'static str> {
    // DatabaseContext::getAliasFromOfficialName(name, table, "ESRI"), where "geodetic_crs" means geographic 2D
    let table = if table == "geodetic_crs" {
        "geographic_2D_crs"
    } else {
        table
    };
    lookup(&esri_names().alias_by_official_name, table, name)
}

pub(super) struct OfficialName {
    pub(super) name: &'static str,
    pub(super) identifier: Identifier,
}

pub(super) fn official_name_from_esri_alias(alias: &str, table: &str) -> Option<OfficialName> {
    // AuthorityFactory::getOfficialNameFromAlias(alias, table, "ESRI", false, ...)
    let (name, code) = lookup(&esri_names().official_by_alias, table, alias)?;
    let (code_space, code) = code.split_once(':')?;
    Some(OfficialName {
        name,
        identifier: Identifier::new(code_space, code),
    })
}

pub(super) fn is_single_esri_object_name(name: &str, table: &str) -> bool {
    // AuthorityFactory("ESRI")::createObjectsFromName(name, {type}, false) finding one object
    esri_names()
        .esri_object_names
        .get(table)
        .is_some_and(|names| names.contains(name.to_ascii_lowercase().as_str()))
}

pub(super) fn esri_alias_through_epsg_old_name(
    identifier: &Identifier,
    table: &str,
) -> Option<&'static str> {
    // the EPSG_OLD fallback in GeodeticCRS::_exportToWKT and GeodeticReferenceFrame::_exportToWKT
    if identifier.code_space != "EPSG" {
        return None;
    }
    lookup(&esri_names().epsg_old_aliases, table, &code_key(identifier))
}

pub(super) fn is_known_datum_name(name: &str) -> bool {
    // DatabaseContext::isKnownName(name, "geodetic_datum"), kept for names ending in a parenthesis
    esri_names().datum_names.contains(name)
}

pub(super) fn esri_projected_definition(esri_name: &str) -> Option<&'static str> {
    // DatabaseContext::getTextDefinition("projected_crs", "ESRI", code) of the one ESRI CRS with this name
    esri_names()
        .esri_projected_definitions
        .get(esri_name)
        .copied()
}

pub(super) struct DatumWithEsriVerticalCrs {
    pub(super) datum: Identifier,
    pub(super) vertical_crs_name: Option<&'static str>,
}

pub(super) fn datum_with_esri_vertical_crs(datum_name: &str) -> Option<DatumWithEsriVerticalCrs> {
    // createObjectsFromName(name, {GEODETIC_REFERENCE_FRAME}, false).front() and the one vertical CRS createVerticalCRSFromDatum("ESRI", "from_geogdatum_...") finds
    let ((code_space, code), vertical_crs_name) =
        *esri_names().esri_vertical_by_datum_name.get(datum_name)?;
    Some(DatumWithEsriVerticalCrs {
        datum: Identifier::new(code_space, code),
        vertical_crs_name: (!vertical_crs_name.is_empty()).then_some(vertical_crs_name),
    })
}

#[cfg(feature = "registry")]
mod registry_objects {
    use crate::iso19111::authority_factory::{
        create_coordinate_reference_system, create_geodetic_datum,
    };
    use crate::iso19111::coordinate_system::{CoordinateSystemAxis, EllipsoidalCs};
    use crate::iso19111::crs::{Crs, GeographicCrs, ProjectedCrs};
    use crate::iso19111::datum::{GeodeticDatum, GeodeticReferenceFrame};
    use crate::iso19111::metadata::{EPSG, Identifier};
    use crate::registry::{self, CrsDefinitionKind, GeodeticCrsType};

    fn epsg_code(identifier: &Identifier) -> Option<u32> {
        if identifier.code_space != EPSG {
            return None;
        }
        identifier.code.parse().ok()
    }

    pub(in super::super) fn geographic_crs_coordinate_system(
        identifier: &Identifier,
    ) -> Option<EllipsoidalCs> {
        // AuthorityFactory::createGeographicCRS(code)->coordinateSystem()
        match create_coordinate_reference_system(epsg_code(identifier)?) {
            Ok(Crs::Geographic(geographic)) => Some(geographic.coordinate_system),
            _ => None,
        }
    }

    pub(in super::super) fn registered_geodetic_datum(
        identifier: &Identifier,
    ) -> Option<GeodeticReferenceFrame> {
        // AuthorityFactory::createGeodeticDatum(code)
        create_geodetic_datum(epsg_code(identifier)?).ok()
    }

    pub(in super::super) fn registered_geographic_crs_with_axes(
        crs: &GeographicCrs,
        axes: &[CoordinateSystemAxis],
    ) -> Option<GeographicCrs> {
        // the same-named CRS that GeographicCRS::demoteTo2D and promoteTo3D take, searched on the datum is2DPartOf3D requires
        let geodetic_type = match axes.len() {
            2 => GeodeticCrsType::Geographic2D,
            3 => GeodeticCrsType::Geographic3D,
            _ => return None,
        };
        let datum_identifier = match &crs.datum {
            GeodeticDatum::ReferenceFrame(frame) => frame.identifiers.first(),
            GeodeticDatum::Ensemble(ensemble) => ensemble.identifiers.first(),
        }?;
        let mut candidates: Vec<(bool, &str, u32)> =
            registry::geodetic_crs_with_datum(epsg_code(datum_identifier)?)
                .into_iter()
                .filter_map(|code| {
                    let same_type = matches!(
                        registry::crs_definition(code)?.kind,
                        CrsDefinitionKind::Geodetic { geodetic_type: candidate_type, .. }
                            if candidate_type == geodetic_type
                    );
                    let metadata = registry::metadata(code)?;
                    (same_type && metadata.name.eq_ignore_ascii_case(&crs.name)).then_some((
                        metadata.deprecated,
                        metadata.name,
                        code,
                    ))
                })
                .collect();
        candidates.sort();
        let (_, _, code) = *candidates.first()?;
        let Ok(Crs::Geographic(candidate)) = create_coordinate_reference_system(code) else {
            return None;
        };
        let expected = EllipsoidalCs {
            axes: axes.to_vec(),
        };
        candidate
            .coordinate_system
            .is_equivalent_to(&expected)
            .then_some(candidate)
    }

    pub(in super::super) fn projected_crs_named(name: &str) -> Option<ProjectedCrs> {
        // AuthorityFactory("EPSG")::createObjectsFromNameEx(name, {PROJECTED_CRS}, false, 0, true), matching the ESRI aliases
        let official = super::official_name_from_esri_alias(name, "projected_crs")?;
        match create_coordinate_reference_system(epsg_code(&official.identifier)?) {
            Ok(Crs::Projected(projected)) => Some(projected),
            _ => None,
        }
    }

    pub(in super::super) fn geodetic_datum_aliases(identifier: &Identifier) -> Vec<&'static str> {
        // DatabaseContext::getAliases(code, "geodetic_datum", "not EPSG_OLD") with the official name
        let Some(code) = epsg_code(identifier) else {
            return Vec::new();
        };
        registry::datum_aliases(code)
            .iter()
            .map(|alias| alias.name)
            .collect()
    }
}

#[cfg(not(feature = "registry"))]
mod registry_objects {
    use crate::iso19111::coordinate_system::{CoordinateSystemAxis, EllipsoidalCs};
    use crate::iso19111::crs::{GeographicCrs, ProjectedCrs};
    use crate::iso19111::datum::GeodeticReferenceFrame;
    use crate::iso19111::metadata::Identifier;

    pub(in super::super) fn geographic_crs_coordinate_system(
        _identifier: &Identifier,
    ) -> Option<EllipsoidalCs> {
        None
    }

    pub(in super::super) fn registered_geodetic_datum(
        _identifier: &Identifier,
    ) -> Option<GeodeticReferenceFrame> {
        None
    }

    pub(in super::super) fn registered_geographic_crs_with_axes(
        _crs: &GeographicCrs,
        _axes: &[CoordinateSystemAxis],
    ) -> Option<GeographicCrs> {
        None
    }

    pub(in super::super) fn geodetic_datum_aliases(_identifier: &Identifier) -> Vec<&'static str> {
        Vec::new()
    }

    pub(in super::super) fn projected_crs_named(_name: &str) -> Option<ProjectedCrs> {
        None
    }
}

pub(super) use registry_objects::{
    geodetic_datum_aliases, geographic_crs_coordinate_system, projected_crs_named,
    registered_geodetic_datum, registered_geographic_crs_with_axes,
};
