// adapted from PROJ src/iso19111/factory.cpp DatabaseContext and AuthorityFactory (MIT), see LICENSES/PROJ-MIT.txt

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use crate::iso19111::crs::ProjectedCrs;
use crate::iso19111::datum::ensemble_name_to_non_ensemble_name;
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
    // name, authority and code, official name, deprecated
    datum_search_lines: Vec<[&'static str; 4]>,
    geodetic_crs_remarks: HashMap<&'static str, &'static str>,
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
                ["datum_search", name, code_key, official_name, deprecated] => {
                    names
                        .datum_search_lines
                        .push([*name, *code_key, *official_name, *deprecated]);
                }
                ["crs_remarks", _, code_key, remarks] => {
                    names.geodetic_crs_remarks.insert(*code_key, *remarks);
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

// metadata::Identifier map_utf8_to_lower
const ACCENTED_LETTERS: [(&str, u8); 10] = [
    ("\u{e1}", b'a'),
    ("\u{e4}", b'a'),
    ("\u{11b}", b'e'),
    ("\u{e8}", b'e'),
    ("\u{e9}", b'e'),
    ("\u{eb}", b'e'),
    ("\u{ed}", b'i'),
    ("\u{f4}", b'o'),
    ("\u{f6}", b'o'),
    ("\u{e7}", b'c'),
];

fn canonicalize_name(name: &str) -> String {
    // metadata::Identifier::canonicalizeName with bigger differences allowed
    let bytes = name.as_bytes();
    let byte_at = |index: usize| bytes.get(index).copied().unwrap_or(0);
    let is_ignored = |byte: u8| {
        matches!(
            byte,
            b' ' | b'_' | b'-' | b'/' | b'(' | b')' | b'.' | b'&' | b','
        )
    };
    let matches_lowercase = |index: usize, word: &str| {
        bytes
            .get(index..index + word.len())
            .is_some_and(|slice| slice.eq_ignore_ascii_case(word.as_bytes()))
    };
    let mut canonical: Vec<u8> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index].to_ascii_lowercase();
        if byte == b' ' && byte_at(index + 1) == b'+' && byte_at(index + 2) == b' ' {
            index += 3;
            continue;
        }
        // "19dd" canonicalizes as "dd"
        if byte == b'1'
            && canonical.last().is_some_and(|last| !last.is_ascii_digit())
            && byte_at(index + 1) == b'9'
            && byte_at(index + 2).is_ascii_digit()
            && byte_at(index + 3).is_ascii_digit()
        {
            index += 2;
            continue;
        }
        let after_ignored = index > 0 && is_ignored(bytes[index - 1]);
        let skipped = ["zone", "height"]
            .into_iter()
            .find(|word| after_ignored && matches_lowercase(index, word));
        if let Some(word) = skipped {
            index += word.len();
            continue;
        }
        let after_ignored_or_digit =
            index > 0 && (is_ignored(bytes[index - 1]) || bytes[index - 1].is_ascii_digit());
        let shortened = ["north", "south"]
            .into_iter()
            .find(|word| after_ignored_or_digit && matches_lowercase(index, word));
        if let Some(word) = shortened {
            canonical.push(byte);
            index += word.len();
            continue;
        }
        let accented = ACCENTED_LETTERS
            .iter()
            .find(|(letter, _)| bytes[index..].starts_with(letter.as_bytes()));
        if let Some((letter, ascii)) = accented {
            canonical.push(*ascii);
            index += letter.len();
            continue;
        }
        if matches_lowercase(index, "_IntlFeet") && index + "_IntlFeet".len() == bytes.len() {
            canonical.extend_from_slice(b"feet");
            break;
        }
        if !is_ignored(byte) {
            canonical.push(byte);
        }
        index += 1;
    }
    String::from_utf8_lossy(&canonical).into_owned()
}

struct DatumSearchRow {
    name: &'static str,
    code_key: &'static str,
    code_space: &'static str,
    code: &'static str,
    official_name: &'static str,
    deprecated: bool,
}

struct DatumSearch {
    rows: Vec<DatumSearchRow>,
    row_indices_by_canonical_name: BTreeMap<String, Vec<usize>>,
    names_by_code: HashMap<&'static str, Vec<&'static str>>,
}

fn datum_search() -> &'static DatumSearch {
    static SEARCH: OnceLock<DatumSearch> = OnceLock::new();
    SEARCH.get_or_init(|| {
        let rows: Vec<DatumSearchRow> = esri_names()
            .datum_search_lines
            .iter()
            .map(|&[name, code_key, official_name, deprecated]| {
                let (code_space, code) = code_key.split_once(':').unwrap_or_else(|| {
                    panic!("esri_names.tsv has a malformed datum_search code: {code_key}")
                });
                DatumSearchRow {
                    name,
                    code_key,
                    code_space,
                    code,
                    official_name,
                    deprecated: deprecated == "1",
                }
            })
            .collect();
        let mut row_indices_by_canonical_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut names_by_code: HashMap<&'static str, Vec<&'static str>> = HashMap::new();
        for (index, row) in rows.iter().enumerate() {
            names_by_code
                .entry(row.code_key)
                .or_default()
                .push(row.name);
            let indices = row_indices_by_canonical_name
                .entry(canonicalize_name(row.name))
                .or_default();
            // createObjectsFromNameEx keeps a deprecated datum only behind deprecated datums of the same name
            let first_is_deprecated = indices.first().is_none_or(|&first| rows[first].deprecated);
            if !row.deprecated || first_is_deprecated {
                indices.push(index);
            }
        }
        DatumSearch {
            rows,
            row_indices_by_canonical_name,
            names_by_code,
        }
    })
}

pub(super) struct NamedGeodeticDatum {
    pub(super) name: &'static str,
    pub(super) identifier: Identifier,
    pub(super) names_and_aliases: &'static [&'static str],
}

pub(super) fn first_geodetic_datum_named(name: &str) -> Option<NamedGeodeticDatum> {
    // AuthorityFactory("")::createObjectsFromName(name, {GEODETIC_REFERENCE_FRAME}, true, 1).front()
    let searched = name.strip_suffix(" (deprecated)").unwrap_or(name);
    let canonical = canonicalize_name(searched);
    // createObjectsFromNameEx returns nothing for a name that canonicalizes to one character or less
    if canonical.len() <= 1 {
        return None;
    }
    let search = datum_search();
    let index = match search.row_indices_by_canonical_name.get(&canonical) {
        Some(indices) => indices.first().copied(),
        None => {
            let lowercase = searched.to_ascii_lowercase();
            search
                .row_indices_by_canonical_name
                .iter()
                .find_map(|(canonical_name, indices)| {
                    indices.iter().copied().find(|&index| {
                        search.rows[index]
                            .name
                            .to_ascii_lowercase()
                            .contains(&lowercase)
                            || canonical_name.contains(&canonical)
                    })
                })
        }
    }?;
    let row = &search.rows[index];
    Some(NamedGeodeticDatum {
        // AuthorityFactory::createGeodeticDatum names a datum ensemble as its datum
        name: ensemble_name_to_non_ensemble_name(row.official_name).unwrap_or(row.official_name),
        identifier: Identifier::new(row.code_space, row.code),
        names_and_aliases: &search.names_by_code[row.code_key],
    })
}

pub(super) fn registered_projected_crs(identifier: &Identifier) -> Option<ProjectedCrs> {
    // AuthorityFactory::createProjectedCRS(code)
    let mut projected = registry_objects::projected_crs_without_remarks(identifier)?;
    // AuthorityFactory::createGeodeticCRS takes the remarks from the description in proj.db, which the registry leaves out
    let remarks = projected.base_crs.identifiers.first().and_then(|base| {
        esri_names()
            .geodetic_crs_remarks
            .get(code_key(base).as_str())
            .copied()
    });
    if let Some(remarks) = remarks {
        projected.base_crs.remarks = remarks.to_string();
    }
    Some(projected)
}

pub(super) fn projected_crs_named(name: &str) -> Option<ProjectedCrs> {
    // AuthorityFactory("EPSG")::createObjectsFromNameEx(name, {PROJECTED_CRS}, false, 0, true), matching the ESRI aliases
    let official = official_name_from_esri_alias(name, "projected_crs")?;
    registered_projected_crs(&official.identifier)
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

    pub(super) fn projected_crs_without_remarks(identifier: &Identifier) -> Option<ProjectedCrs> {
        match create_coordinate_reference_system(epsg_code(identifier)?) {
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

    pub(super) fn projected_crs_without_remarks(_identifier: &Identifier) -> Option<ProjectedCrs> {
        None
    }
}

pub(super) use registry_objects::{
    geodetic_datum_aliases, geographic_crs_coordinate_system, registered_geodetic_datum,
    registered_geographic_crs_with_axes,
};
