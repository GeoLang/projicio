//! Embedded EPSG registry.
//!
//! Reads the blob generated from PROJ's proj.db by `tools/gen-epsg-registry`.
//! It carries names, kind, deprecation, datum links, areas of use, and datum
//! aliases, plus the definitions behind EPSG's geodetic and projected CRSs,
//! every current EPSG coordinate operation between geodetic CRSs and PROJ's
//! own operations between EPSG geodetic CRSs, looked up by code. Nothing here
//! takes part in transforming a coordinate.
//!
//! The blob is parsed once on first use. The provenance file next to it
//! records the dataset it came from and the checksum CI reproduces.

use projicio_epsg_format::read::Reader;
use projicio_epsg_format::records::{StringTable, decode_records};
use projicio_epsg_format::{
    CRS_KIND_GEOGRAPHIC, CRS_KIND_OTHER, CRS_KIND_PROJECTED, CRS_RECORD_BASE_SIZE,
    DATUM_ALIAS_RECORD_BASE_SIZE, EXTENT_RECORD_BASE_SIZE, FLAG_DEPRECATED, HEADER_SIZE, MAGIC,
    NO_CODE, SECTION_CONVERSION, SECTION_COORDINATE_OPERATION, SECTION_COORDINATE_SYSTEM,
    SECTION_CRS, SECTION_CRS_DEFINITION, SECTION_DATUM, SECTION_DATUM_ALIAS, SECTION_ELLIPSOID,
    SECTION_ENTRY_SIZE, SECTION_EXTENT, SECTION_GRID_RESOURCE, SECTION_PRIME_MERIDIAN,
    SECTION_STRING, SECTION_SUPERSESSION, SECTION_UNIT_OF_MEASURE, VERSION, read,
};
use std::sync::OnceLock;
use thiserror::Error;

pub use projicio_epsg_format::records::{
    Axis, ConversionRecord, CoordinateSystemKind, CoordinateSystemRecord, CrsDefinitionKind,
    CrsDefinitionRecord, DatumEnsemble, DatumRecord, EllipsoidRecord, EllipsoidShape,
    GeodeticCrsType, GridAlternativeRecord, GridFileReference, HelmertParameters, Measure,
    MeasuredVector, OperationCode, OperationKind, OperationMethod, OperationRecord, OperationStep,
    ParameterValue, PrimeMeridianRecord, StepDirection, StepReference, SupersessionRecord,
    TransformationParameters, UnitKind, UnitRecord,
};

static REGISTRY_BYTES: &[u8] = include_bytes!("../data/epsg.bin");
static PROVENANCE_JSON: &str = include_str!("../data/epsg.provenance.json");

/// What a CRS measures in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrsKind {
    /// Longitude and latitude.
    Geographic,
    /// Projected plane coordinates.
    Projected,
    /// Anything else, including vertical, compound, geocentric, and
    /// engineering CRS.
    Other,
}

/// An EPSG area of use, in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extent {
    pub code: u32,
    pub name: &'static str,
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

impl Extent {
    /// True when the coordinate falls inside the area of use.
    ///
    /// An extent whose west bound is east of its east bound wraps the
    /// antimeridian, so it covers both sides of the seam.
    pub fn contains(&self, longitude: f64, latitude: f64) -> bool {
        let within_latitude = latitude >= self.south && latitude <= self.north;
        let within_longitude = if self.west <= self.east {
            longitude >= self.west && longitude <= self.east
        } else {
            longitude >= self.west || longitude <= self.east
        };
        within_latitude && within_longitude
    }
}

/// What the EPSG dataset records about a CRS, transform behavior aside.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrsMetadata {
    pub code: u32,
    pub name: &'static str,
    pub kind: CrsKind,
    /// EPSG has retired this code. It still resolves, but new data should not
    /// use it.
    pub deprecated: bool,
    /// The geodetic datum this CRS is referenced to, where EPSG records one.
    /// Vertical and engineering CRS reference datums the registry does not
    /// carry, so they report `None`.
    pub datum_code: Option<u32>,
    pub extent: Option<Extent>,
}

/// A name or alternative name EPSG records for a geodetic datum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatumAlias {
    pub datum_code: u32,
    pub name: &'static str,
}

/// Look up what the EPSG dataset records about a code.
pub fn metadata(code: u32) -> Option<CrsMetadata> {
    let registry = registry();
    let record = registry.find_crs(code)?;
    Some(CrsMetadata {
        code: record.code,
        name: record.name,
        kind: record.kind,
        deprecated: record.flags & FLAG_DEPRECATED != 0,
        datum_code: optional_code(record.datum_code),
        extent: optional_code(record.extent_code)
            .and_then(|extent_code| registry.find_extent(extent_code).copied()),
    })
}

pub fn extent(code: u32) -> Option<&'static Extent> {
    registry().find_extent(code)
}

/// Whether a CRS is meant to be used at this coordinate.
///
/// `None` when the code is unknown or EPSG records no area of use for it.
/// A coordinate outside the area of use still transforms, just with accuracy
/// EPSG makes no claim about.
pub fn applies_at(code: u32, longitude: f64, latitude: f64) -> Option<bool> {
    let extent = metadata(code)?.extent?;
    Some(extent.contains(longitude, latitude))
}

/// Every name EPSG records for a geodetic datum, sorted, including the
/// alternative names other authorities use for it.
pub fn datum_aliases(datum_code: u32) -> &'static [DatumAlias] {
    let aliases = &registry().datum_aliases;
    let start = aliases.partition_point(|alias| alias.datum_code < datum_code);
    let end = aliases.partition_point(|alias| alias.datum_code <= datum_code);
    &aliases[start..end]
}

/// Provenance of the embedded registry: the EPSG and PROJ versions it was
/// generated from, its checksum, and its record counts.
pub fn provenance_json() -> &'static str {
    PROVENANCE_JSON
}

pub fn unit_of_measure(code: u32) -> Option<&'static UnitRecord<'static>> {
    find_by_code(&registry().units, code, |unit| unit.code)
}

pub fn ellipsoid(code: u32) -> Option<&'static EllipsoidRecord<'static>> {
    find_by_code(&registry().ellipsoids, code, |ellipsoid| ellipsoid.code)
}

pub fn prime_meridian(code: u32) -> Option<&'static PrimeMeridianRecord<'static>> {
    find_by_code(&registry().prime_meridians, code, |meridian| meridian.code)
}

pub fn geodetic_datum(code: u32) -> Option<&'static DatumRecord<'static>> {
    find_by_code(&registry().datums, code, |datum| datum.code)
}

pub fn coordinate_system(code: u32) -> Option<&'static CoordinateSystemRecord<'static>> {
    find_by_code(&registry().coordinate_systems, code, |system| system.code)
}

pub fn crs_definition(code: u32) -> Option<&'static CrsDefinitionRecord> {
    find_by_code(&registry().crs_definitions, code, |definition| {
        definition.code
    })
}

pub fn conversion(code: u32) -> Option<&'static ConversionRecord<'static>> {
    find_by_code(&registry().conversions, code, |conversion| conversion.code)
}

pub fn coordinate_operation(code: OperationCode<'_>) -> Option<&'static OperationRecord<'static>> {
    let operations = &registry().operations;
    let index = operations
        .binary_search_by(|operation| operation.code.cmp(&code))
        .ok()?;
    Some(&operations[index])
}

pub fn grid_alternative(original_name: &str) -> Option<&'static GridAlternativeRecord<'static>> {
    let alternatives = &registry().grid_alternatives;
    let index = alternatives
        .binary_search_by(|alternative| alternative.original_name.cmp(original_name))
        .ok()?;
    Some(&alternatives[index])
}

pub fn grid_alternative_for_file(
    file_name: &str,
) -> Option<&'static GridAlternativeRecord<'static>> {
    // proj's factory writes proj's file name in place of epsg's
    grid_alternative(file_name).or_else(|| {
        registry().grid_alternatives.iter().find(|alternative| {
            alternative.proj_grid_name == Some(file_name)
                || alternative.old_proj_grid_name == Some(file_name)
        })
    })
}

pub fn supersessions(superseded_code: u32) -> &'static [SupersessionRecord] {
    equal_range(&registry().supersessions, superseded_code, |supersession| {
        supersession.superseded_code
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectedOperation {
    pub operation: &'static OperationRecord<'static>,
    // true when the request runs from the operation's target crs to its source crs
    pub inverse: bool,
}

pub fn operations_between(source_crs_code: u32, target_crs_code: u32) -> Vec<DirectedOperation> {
    let registry = registry();
    equal_range(
        &registry.operation_endpoints,
        (source_crs_code, target_crs_code),
        |endpoint| (endpoint.crs_code, endpoint.other_crs_code),
    )
    .iter()
    .map(|endpoint| registry.directed(endpoint))
    .collect()
}

pub fn operations_from(crs_code: u32) -> Vec<DirectedOperation> {
    let registry = registry();
    equal_range(&registry.operation_endpoints, crs_code, |endpoint| {
        endpoint.crs_code
    })
    .iter()
    .map(|endpoint| registry.directed(endpoint))
    .collect()
}

fn find_by_code<T>(records: &[T], code: u32, code_of: impl Fn(&T) -> u32) -> Option<&T> {
    let index = records.binary_search_by_key(&code, code_of).ok()?;
    Some(&records[index])
}

fn equal_range<T, K: Ord>(records: &[T], key: K, key_of: impl Fn(&T) -> K) -> &[T] {
    let start = records.partition_point(|record| key_of(record) < key);
    let end = records.partition_point(|record| key_of(record) <= key);
    &records[start..end]
}

fn optional_code(code: u32) -> Option<u32> {
    (code != NO_CODE).then_some(code)
}

fn registry() -> &'static Registry {
    static PARSED: OnceLock<Registry> = OnceLock::new();
    PARSED.get_or_init(|| {
        parse(REGISTRY_BYTES)
            .unwrap_or_else(|error| panic!("embedded EPSG registry is corrupt: {error}"))
    })
}

struct CrsRecord {
    code: u32,
    kind: CrsKind,
    flags: u8,
    datum_code: u32,
    extent_code: u32,
    name: &'static str,
}

struct OperationEndpoint {
    crs_code: u32,
    other_crs_code: u32,
    operation_index: usize,
    inverse: bool,
}

struct Registry {
    crs: Vec<CrsRecord>,
    extents: Vec<Extent>,
    datum_aliases: Vec<DatumAlias>,
    units: Vec<UnitRecord<'static>>,
    ellipsoids: Vec<EllipsoidRecord<'static>>,
    prime_meridians: Vec<PrimeMeridianRecord<'static>>,
    datums: Vec<DatumRecord<'static>>,
    coordinate_systems: Vec<CoordinateSystemRecord<'static>>,
    crs_definitions: Vec<CrsDefinitionRecord>,
    conversions: Vec<ConversionRecord<'static>>,
    operations: Vec<OperationRecord<'static>>,
    grid_alternatives: Vec<GridAlternativeRecord<'static>>,
    supersessions: Vec<SupersessionRecord>,
    operation_endpoints: Vec<OperationEndpoint>,
}

impl Registry {
    fn find_crs(&self, code: u32) -> Option<&CrsRecord> {
        let index = self
            .crs
            .binary_search_by_key(&code, |record| record.code)
            .ok()?;
        Some(&self.crs[index])
    }

    fn find_extent(&self, code: u32) -> Option<&Extent> {
        let index = self
            .extents
            .binary_search_by_key(&code, |extent| extent.code)
            .ok()?;
        Some(&self.extents[index])
    }

    fn directed(&'static self, endpoint: &OperationEndpoint) -> DirectedOperation {
        DirectedOperation {
            operation: &self.operations[endpoint.operation_index],
            inverse: endpoint.inverse,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
enum RegistryError {
    #[error("registry is {0} bytes, too short to hold a header")]
    TooShort(usize),

    #[error("registry does not start with the format magic")]
    BadMagic,

    #[error("registry format version is {found}, expected {VERSION}")]
    UnsupportedVersion { found: u16 },

    #[error("registry section table is truncated")]
    MalformedSectionTable,

    #[error("registry has no section {tag}")]
    MissingSection { tag: u32 },

    #[error("registry section {tag} is truncated or misencoded")]
    MalformedSection { tag: u32 },

    #[error("registry section {tag} is not sorted by code")]
    UnsortedSection { tag: u32 },
}

/// Parse a registry blob.
///
/// Takes `&'static [u8]` so records can borrow their names straight out of the
/// blob. The only caller outside tests is the embedded-blob initializer.
fn parse(bytes: &'static [u8]) -> Result<Registry, RegistryError> {
    let magic = read::bytes(bytes, 0, MAGIC.len()).ok_or(RegistryError::TooShort(bytes.len()))?;
    if magic != MAGIC.as_slice() {
        return Err(RegistryError::BadMagic);
    }
    let version = read::u16(bytes, MAGIC.len()).ok_or(RegistryError::TooShort(bytes.len()))?;
    if version != VERSION {
        return Err(RegistryError::UnsupportedVersion { found: version });
    }
    let section_count =
        usize::from(read::u16(bytes, MAGIC.len() + 2).ok_or(RegistryError::TooShort(bytes.len()))?);
    let table = read_section_table(bytes, section_count)?;

    let crs = parse_crs_records(find_section(&table, SECTION_CRS)?)?;
    let extents = parse_extents(find_section(&table, SECTION_EXTENT)?)?;
    let datum_aliases = parse_datum_aliases(find_section(&table, SECTION_DATUM_ALIAS)?)?;

    require_sorted(crs.iter().map(|record| record.code), SECTION_CRS)?;
    require_sorted(extents.iter().map(|extent| extent.code), SECTION_EXTENT)?;

    let string_section = find_section(&table, SECTION_STRING)?;
    let strings = StringTable::decode(string_section.payload, string_section.record_count).ok_or(
        RegistryError::MalformedSection {
            tag: SECTION_STRING,
        },
    )?;
    let units = decode_section(&table, SECTION_UNIT_OF_MEASURE, |reader| {
        UnitRecord::decode(reader, &strings)
    })?;
    let ellipsoids = decode_section(&table, SECTION_ELLIPSOID, |reader| {
        EllipsoidRecord::decode(reader, &strings)
    })?;
    let prime_meridians = decode_section(&table, SECTION_PRIME_MERIDIAN, |reader| {
        PrimeMeridianRecord::decode(reader, &strings)
    })?;
    let datums = decode_section(&table, SECTION_DATUM, |reader| {
        DatumRecord::decode(reader, &strings)
    })?;
    let coordinate_systems = decode_section(&table, SECTION_COORDINATE_SYSTEM, |reader| {
        CoordinateSystemRecord::decode(reader, &strings)
    })?;
    let crs_definitions =
        decode_section(&table, SECTION_CRS_DEFINITION, CrsDefinitionRecord::decode)?;
    let conversions = decode_section(&table, SECTION_CONVERSION, |reader| {
        ConversionRecord::decode(reader, &strings)
    })?;
    let operations = decode_section(&table, SECTION_COORDINATE_OPERATION, |reader| {
        OperationRecord::decode(reader, &strings)
    })?;
    let grid_alternatives = decode_section(&table, SECTION_GRID_RESOURCE, |reader| {
        GridAlternativeRecord::decode(reader, &strings)
    })?;
    let supersessions = decode_section(&table, SECTION_SUPERSESSION, SupersessionRecord::decode)?;

    require_sorted(units.iter().map(|unit| unit.code), SECTION_UNIT_OF_MEASURE)?;
    require_sorted(
        ellipsoids.iter().map(|ellipsoid| ellipsoid.code),
        SECTION_ELLIPSOID,
    )?;
    require_sorted(
        prime_meridians.iter().map(|meridian| meridian.code),
        SECTION_PRIME_MERIDIAN,
    )?;
    require_sorted(datums.iter().map(|datum| datum.code), SECTION_DATUM)?;
    require_sorted(
        coordinate_systems.iter().map(|system| system.code),
        SECTION_COORDINATE_SYSTEM,
    )?;
    require_sorted(
        crs_definitions.iter().map(|definition| definition.code),
        SECTION_CRS_DEFINITION,
    )?;
    require_sorted(
        conversions.iter().map(|conversion| conversion.code),
        SECTION_CONVERSION,
    )?;
    if operations
        .windows(2)
        .any(|pair| pair[0].code >= pair[1].code)
    {
        return Err(RegistryError::UnsortedSection {
            tag: SECTION_COORDINATE_OPERATION,
        });
    }
    if grid_alternatives
        .windows(2)
        .any(|pair| pair[0].original_name >= pair[1].original_name)
    {
        return Err(RegistryError::UnsortedSection {
            tag: SECTION_GRID_RESOURCE,
        });
    }
    if supersessions.windows(2).any(|pair| {
        (pair[0].superseded_code, pair[0].replacement_code)
            > (pair[1].superseded_code, pair[1].replacement_code)
    }) {
        return Err(RegistryError::UnsortedSection {
            tag: SECTION_SUPERSESSION,
        });
    }
    let operation_endpoints = index_operation_endpoints(&operations);

    Ok(Registry {
        crs,
        extents,
        datum_aliases,
        units,
        ellipsoids,
        prime_meridians,
        datums,
        coordinate_systems,
        crs_definitions,
        conversions,
        operations,
        grid_alternatives,
        supersessions,
        operation_endpoints,
    })
}

fn decode_section<T>(
    table: &[SectionEntry],
    tag: u32,
    decode: impl FnMut(&mut Reader<'static>) -> Option<T>,
) -> Result<Vec<T>, RegistryError> {
    let section = find_section(table, tag)?;
    decode_records(section.payload, section.record_count, decode)
        .ok_or(RegistryError::MalformedSection { tag })
}

fn index_operation_endpoints(operations: &[OperationRecord<'static>]) -> Vec<OperationEndpoint> {
    let mut endpoints: Vec<OperationEndpoint> = operations
        .iter()
        .enumerate()
        .flat_map(|(operation_index, operation)| {
            [
                OperationEndpoint {
                    crs_code: operation.source_crs_code,
                    other_crs_code: operation.target_crs_code,
                    operation_index,
                    inverse: false,
                },
                OperationEndpoint {
                    crs_code: operation.target_crs_code,
                    other_crs_code: operation.source_crs_code,
                    operation_index,
                    inverse: true,
                },
            ]
        })
        .collect();
    endpoints.sort_by_key(|endpoint| {
        (
            endpoint.crs_code,
            endpoint.other_crs_code,
            endpoint.operation_index,
            endpoint.inverse,
        )
    });
    endpoints
}

struct SectionEntry {
    tag: u32,
    record_count: usize,
    payload: &'static [u8],
}

fn read_section_table(
    bytes: &'static [u8],
    section_count: usize,
) -> Result<Vec<SectionEntry>, RegistryError> {
    let mut entries = Vec::with_capacity(section_count);
    for index in 0..section_count {
        let entry = HEADER_SIZE + index * SECTION_ENTRY_SIZE;
        let tag = read::u32(bytes, entry).ok_or(RegistryError::MalformedSectionTable)?;
        let record_count =
            read::u32(bytes, entry + 4).ok_or(RegistryError::MalformedSectionTable)?;
        let offset = read::u32(bytes, entry + 8).ok_or(RegistryError::MalformedSectionTable)?;
        let byte_len = read::u32(bytes, entry + 12).ok_or(RegistryError::MalformedSectionTable)?;
        let payload = read::bytes(bytes, offset as usize, byte_len as usize)
            .ok_or(RegistryError::MalformedSection { tag })?;
        entries.push(SectionEntry {
            tag,
            record_count: record_count as usize,
            payload,
        });
    }
    Ok(entries)
}

/// Sections the reader does not know are skipped, so the generator can add one
/// without breaking older readers.
fn find_section(table: &[SectionEntry], tag: u32) -> Result<&SectionEntry, RegistryError> {
    table
        .iter()
        .find(|entry| entry.tag == tag)
        .ok_or(RegistryError::MissingSection { tag })
}

fn parse_crs_records(section: &SectionEntry) -> Result<Vec<CrsRecord>, RegistryError> {
    let malformed = || RegistryError::MalformedSection { tag: section.tag };
    let payload = section.payload;
    let mut records = Vec::new();
    let mut offset = 0;
    for _ in 0..section.record_count {
        let code = read::u32(payload, offset).ok_or_else(malformed)?;
        let kind = read::u8(payload, offset + 4).ok_or_else(malformed)?;
        let flags = read::u8(payload, offset + 5).ok_or_else(malformed)?;
        let datum_code = read::u32(payload, offset + 6).ok_or_else(malformed)?;
        let extent_code = read::u32(payload, offset + 10).ok_or_else(malformed)?;
        let (name, name_len) =
            read::string_u16(payload, offset + CRS_RECORD_BASE_SIZE).ok_or_else(malformed)?;
        offset += CRS_RECORD_BASE_SIZE + name_len;
        records.push(CrsRecord {
            code,
            kind: decode_kind(kind).ok_or_else(malformed)?,
            flags,
            datum_code,
            extent_code,
            name,
        });
    }
    require_exhausted(offset, section)?;
    Ok(records)
}

fn parse_extents(section: &SectionEntry) -> Result<Vec<Extent>, RegistryError> {
    let malformed = || RegistryError::MalformedSection { tag: section.tag };
    let payload = section.payload;
    let mut extents = Vec::new();
    let mut offset = 0;
    for _ in 0..section.record_count {
        let code = read::u32(payload, offset).ok_or_else(malformed)?;
        let west = read::f64(payload, offset + 4).ok_or_else(malformed)?;
        let south = read::f64(payload, offset + 12).ok_or_else(malformed)?;
        let east = read::f64(payload, offset + 20).ok_or_else(malformed)?;
        let north = read::f64(payload, offset + 28).ok_or_else(malformed)?;
        let (name, name_len) =
            read::string_u16(payload, offset + EXTENT_RECORD_BASE_SIZE).ok_or_else(malformed)?;
        offset += EXTENT_RECORD_BASE_SIZE + name_len;
        extents.push(Extent {
            code,
            name,
            west,
            south,
            east,
            north,
        });
    }
    require_exhausted(offset, section)?;
    Ok(extents)
}

fn parse_datum_aliases(section: &SectionEntry) -> Result<Vec<DatumAlias>, RegistryError> {
    let malformed = || RegistryError::MalformedSection { tag: section.tag };
    let payload = section.payload;
    let mut aliases = Vec::new();
    let mut offset = 0;
    for _ in 0..section.record_count {
        let datum_code = read::u32(payload, offset).ok_or_else(malformed)?;
        let (name, name_len) = read::string_u16(payload, offset + DATUM_ALIAS_RECORD_BASE_SIZE)
            .ok_or_else(malformed)?;
        offset += DATUM_ALIAS_RECORD_BASE_SIZE + name_len;
        aliases.push(DatumAlias { datum_code, name });
    }
    require_exhausted(offset, section)?;
    if aliases
        .windows(2)
        .any(|pair| pair[0].datum_code > pair[1].datum_code)
    {
        return Err(RegistryError::UnsortedSection { tag: section.tag });
    }
    Ok(aliases)
}

fn decode_kind(kind: u8) -> Option<CrsKind> {
    match kind {
        CRS_KIND_GEOGRAPHIC => Some(CrsKind::Geographic),
        CRS_KIND_PROJECTED => Some(CrsKind::Projected),
        CRS_KIND_OTHER => Some(CrsKind::Other),
        _ => None,
    }
}

/// Records must fill the section exactly, so a record count that disagrees
/// with the payload cannot slip through.
fn require_exhausted(offset: usize, section: &SectionEntry) -> Result<(), RegistryError> {
    if offset == section.payload.len() {
        return Ok(());
    }
    Err(RegistryError::MalformedSection { tag: section.tag })
}

/// Lookups binary search, so a section out of order would answer wrongly
/// rather than fail.
fn require_sorted(codes: impl Iterator<Item = u32>, tag: u32) -> Result<(), RegistryError> {
    let mut previous = None;
    for code in codes {
        if previous.is_some_and(|earlier| earlier >= code) {
            return Err(RegistryError::UnsortedSection { tag });
        }
        previous = Some(code);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WGS84_DATUM_ENSEMBLE: u32 = 6326;
    const UTM_ZONE_18N: u32 = 32618;
    const DEPRECATED_SCOPQ_ZONE_2: u32 = 2008;

    fn provenance_field(key: &str) -> &'static str {
        let needle = format!("\"{key}\":");
        let start = PROVENANCE_JSON
            .find(&needle)
            .unwrap_or_else(|| panic!("provenance has no {key}"))
            + needle.len();
        let rest = PROVENANCE_JSON[start..].trim_start();
        let end = rest.find([',', '\n', '}']).expect("provenance field ends");
        rest[..end].trim().trim_matches('"')
    }

    fn provenance_count(key: &str) -> usize {
        provenance_field(key).parse().expect("count is a number")
    }

    #[test]
    fn provenance_pins_the_committed_blob() {
        assert_eq!(
            provenance_field("sha256"),
            projicio_epsg_format::checksum::sha256_hex(REGISTRY_BYTES)
        );
        assert_eq!(provenance_count("byte_len"), REGISTRY_BYTES.len());
        assert_eq!(provenance_field("magic"), "EPSGMETA");
        assert_eq!(provenance_field("epsg_version"), "v12.029");
        assert_eq!(provenance_field("proj_version"), "9.8.1");
    }

    #[test]
    fn section_counts_match_the_provenance() {
        let registry = registry();

        assert!(!registry.crs.is_empty());
        assert!(!registry.extents.is_empty());
        assert!(!registry.datum_aliases.is_empty());
        assert_eq!(registry.crs.len(), provenance_count("crs"));
        assert_eq!(registry.extents.len(), provenance_count("extents"));
        assert_eq!(
            registry.datum_aliases.len(),
            provenance_count("datum_aliases")
        );
        assert_eq!(registry.units.len(), provenance_count("units_of_measure"));
        assert_eq!(registry.ellipsoids.len(), provenance_count("ellipsoids"));
        assert_eq!(
            registry.prime_meridians.len(),
            provenance_count("prime_meridians")
        );
        assert_eq!(registry.datums.len(), provenance_count("datums"));
        assert_eq!(
            registry.coordinate_systems.len(),
            provenance_count("coordinate_systems")
        );
        assert_eq!(
            registry.crs_definitions.len(),
            provenance_count("crs_definitions")
        );
        assert_eq!(registry.conversions.len(), provenance_count("conversions"));
        assert_eq!(
            registry.operations.len(),
            provenance_count("coordinate_operations")
        );
        assert_eq!(
            registry.grid_alternatives.len(),
            provenance_count("grid_alternatives")
        );
        assert_eq!(
            registry.supersessions.len(),
            provenance_count("supersessions")
        );
    }

    #[test]
    fn wgs84_is_geographic_and_current() {
        let wgs84 = metadata(4326).unwrap();

        assert_eq!(wgs84.name, "WGS 84");
        assert_eq!(wgs84.kind, CrsKind::Geographic);
        assert!(!wgs84.deprecated);
        assert_eq!(wgs84.datum_code, Some(WGS84_DATUM_ENSEMBLE));
    }

    #[test]
    fn utm_zone_18n_is_projected() {
        let zone = metadata(UTM_ZONE_18N).unwrap();

        assert_eq!(zone.name, "WGS 84 / UTM zone 18N");
        assert_eq!(zone.kind, CrsKind::Projected);
        assert_eq!(zone.extent.unwrap().west, -78.0);
    }

    #[test]
    fn deprecated_code_is_flagged() {
        let retired = metadata(DEPRECATED_SCOPQ_ZONE_2).unwrap();

        assert!(retired.deprecated);
        assert!(!metadata(4326).unwrap().deprecated);
    }

    #[test]
    fn unknown_code_has_no_metadata() {
        assert!(metadata(999_999).is_none());
        assert!(applies_at(999_999, 0.0, 0.0).is_none());
    }

    #[test]
    fn utm_zone_18n_applies_over_new_york_only() {
        assert_eq!(applies_at(UTM_ZONE_18N, -74.0, 40.7), Some(true));
        assert_eq!(applies_at(UTM_ZONE_18N, 2.35, 48.85), Some(false));
    }

    #[test]
    fn extent_wrapping_the_antimeridian_covers_both_sides() {
        let wrapping = Extent {
            code: 1,
            name: "test",
            west: 170.0,
            south: -10.0,
            east: -170.0,
            north: 10.0,
        };

        assert!(wrapping.contains(179.0, 0.0));
        assert!(wrapping.contains(-179.0, 0.0));
        assert!(!wrapping.contains(0.0, 0.0));
        assert!(!wrapping.contains(179.0, 20.0));
    }

    #[test]
    fn datum_aliases_cover_every_recorded_name() {
        let names: Vec<&str> = datum_aliases(WGS84_DATUM_ENSEMBLE)
            .iter()
            .map(|alias| alias.name)
            .collect();

        assert!(names.contains(&"World Geodetic System 1984 ensemble"));
        assert!(names.contains(&"D_WGS_1984"));
        assert!(datum_aliases(999_999).is_empty());
    }

    fn parse_error(bytes: &'static [u8]) -> RegistryError {
        match parse(bytes) {
            Ok(_) => panic!("expected the blob to be rejected"),
            Err(error) => error,
        }
    }

    #[test]
    fn truncated_blob_is_rejected() {
        assert_eq!(
            parse_error(&REGISTRY_BYTES[..4]),
            RegistryError::TooShort(4)
        );
        assert_eq!(
            parse_error(&REGISTRY_BYTES[..HEADER_SIZE + 8]),
            RegistryError::MalformedSectionTable
        );
        assert_eq!(read::u32(REGISTRY_BYTES, HEADER_SIZE), Some(SECTION_CRS));
        let crs_payload_offset = read::u32(REGISTRY_BYTES, HEADER_SIZE + 8).unwrap() as usize;
        assert_eq!(
            parse_error(&REGISTRY_BYTES[..crs_payload_offset + 1]),
            RegistryError::MalformedSection { tag: SECTION_CRS }
        );
    }

    #[test]
    fn foreign_blob_is_rejected() {
        static NOT_A_REGISTRY: &[u8] = b"this is not an epsg registry blob at all";
        assert_eq!(parse_error(NOT_A_REGISTRY), RegistryError::BadMagic);
    }

    fn helmert(code: u32) -> &'static HelmertParameters<'static> {
        match &coordinate_operation(OperationCode::Epsg(code))
            .unwrap()
            .kind
        {
            OperationKind::HelmertTransformation(parameters) => parameters.as_ref(),
            other => panic!("EPSG:{code} is not a Helmert transformation: {other:?}"),
        }
    }

    fn metres(values: [f64; 3]) -> MeasuredVector {
        MeasuredVector {
            values,
            unit_code: 9001,
        }
    }

    #[test]
    fn nad83_is_geographic_2d_on_grs_1980() {
        let nad83 = crs_definition(4269).unwrap();
        assert_eq!(
            nad83.kind,
            CrsDefinitionKind::Geodetic {
                geodetic_type: GeodeticCrsType::Geographic2D,
                datum_code: 6269,
            }
        );
        assert_eq!(nad83.coordinate_system_code, 6422);
        assert_eq!(nad83.extent_codes, [1350]);

        let datum = geodetic_datum(6269).unwrap();
        assert_eq!(datum.name, "North American Datum 1983");
        assert_eq!(datum.ellipsoid_code, 7019);
        assert_eq!(datum.prime_meridian_code, 8901);
        assert_eq!(datum.frame_reference_epoch, None);
        assert_eq!(datum.ensemble, None);

        let grs_1980 = ellipsoid(7019).unwrap();
        assert_eq!(grs_1980.name, "GRS 1980");
        assert_eq!(grs_1980.semi_major_axis, 6_378_137.0);
        assert_eq!(grs_1980.unit_code, 9001);
        assert_eq!(
            grs_1980.shape,
            EllipsoidShape::InverseFlattening(298.257_222_101)
        );

        let greenwich = prime_meridian(8901).unwrap();
        assert_eq!(greenwich.name, "Greenwich");
        assert_eq!(greenwich.longitude, 0.0);
        assert_eq!(greenwich.unit_code, 9102);
    }

    #[test]
    fn geographic_axes_are_latitude_then_longitude() {
        let axes = &coordinate_system(6422).unwrap().axes;

        assert_eq!(
            coordinate_system(6422).unwrap().kind,
            CoordinateSystemKind::Ellipsoidal
        );
        assert_eq!(
            axes,
            &[
                Axis {
                    name: "Geodetic latitude",
                    abbreviation: "Lat",
                    orientation: "north",
                    unit_code: 9122,
                },
                Axis {
                    name: "Geodetic longitude",
                    abbreviation: "Lon",
                    orientation: "east",
                    unit_code: 9122,
                },
            ]
        );
    }

    #[test]
    fn lambert_93_is_projected_with_easting_first() {
        let lambert_93 = crs_definition(2154).unwrap();
        assert_eq!(
            lambert_93.kind,
            CrsDefinitionKind::Projected {
                base_crs_code: 4171,
                conversion_code: 18085,
            }
        );
        assert_eq!(lambert_93.coordinate_system_code, 4499);

        let system = coordinate_system(4499).unwrap();
        assert_eq!(system.kind, CoordinateSystemKind::Cartesian);
        assert_eq!(
            system.axes,
            [
                Axis {
                    name: "Easting",
                    abbreviation: "X",
                    orientation: "east",
                    unit_code: 9001,
                },
                Axis {
                    name: "Northing",
                    abbreviation: "Y",
                    orientation: "north",
                    unit_code: 9001,
                },
            ]
        );
        let lambert = conversion(18085).unwrap();
        assert_eq!(lambert.method.code, 9802);
        assert_eq!(lambert.parameters[0].unit_code, 9110);

        let sexagesimal = unit_of_measure(9110).unwrap();
        assert_eq!(sexagesimal.name, "sexagesimal DMS");
        assert_eq!(sexagesimal.kind, UnitKind::Angle);
        assert_eq!(sexagesimal.to_si_factor, None);
        assert_eq!(unit_of_measure(9001).unwrap().to_si_factor, Some(1.0));
    }

    #[test]
    fn utm_zone_32n_is_transverse_mercator_with_five_parameters() {
        let zone = crs_definition(32632).unwrap();
        assert_eq!(
            zone.kind,
            CrsDefinitionKind::Projected {
                base_crs_code: 4326,
                conversion_code: 16032,
            }
        );
        assert_eq!(zone.coordinate_system_code, 4400);
        assert_eq!(zone.extent_codes, [2062]);

        let utm = conversion(16032).unwrap();
        assert_eq!(utm.name, "UTM zone 32N");
        assert_eq!(
            utm.method,
            OperationMethod {
                code: 9807,
                name: "Transverse Mercator",
            }
        );
        let parameter = |code, name, value, unit_code| ParameterValue {
            code,
            name,
            value,
            unit_code,
        };
        assert_eq!(
            utm.parameters,
            [
                parameter(8801, "Latitude of natural origin", 0.0, 9102),
                parameter(8802, "Longitude of natural origin", 9.0, 9102),
                parameter(8805, "Scale factor at natural origin", 0.9996, 9201),
                parameter(8806, "False easting", 500_000.0, 9001),
                parameter(8807, "False northing", 0.0, 9001),
            ]
        );
    }

    #[test]
    fn wgs84_ensemble_lists_its_realizations_in_order() {
        let ensemble = geodetic_datum(WGS84_DATUM_ENSEMBLE).unwrap();

        assert_eq!(ensemble.name, "World Geodetic System 1984 ensemble");
        assert_eq!(ensemble.ellipsoid_code, 7030);
        assert_eq!(
            ensemble.ensemble,
            Some(DatumEnsemble {
                accuracy: 2.0,
                member_codes: vec![1166, 1152, 1153, 1154, 1155, 1156, 1309, 1383],
            })
        );
        let g2139 = geodetic_datum(1309).unwrap();
        assert_eq!(g2139.frame_reference_epoch, Some(2016.0));
        assert_eq!(g2139.ensemble, None);
    }

    #[test]
    fn nad83_to_wgs84_1_is_a_zero_translation_good_to_4_m() {
        let operation = coordinate_operation(OperationCode::Epsg(1188)).unwrap();
        assert_eq!(operation.name, "NAD83 to WGS 84 (1)");
        assert_eq!(
            (operation.source_crs_code, operation.target_crs_code),
            (4269, 4326)
        );
        assert_eq!(operation.accuracy, Some(4.0));
        assert_eq!(operation.extent_codes, [1325]);

        assert_eq!(
            *helmert(1188),
            HelmertParameters {
                method: OperationMethod {
                    code: 9603,
                    name: "Geocentric translations (geog2D domain)",
                },
                translation: metres([0.0, 0.0, 0.0]),
                rotation: None,
                scale_difference: None,
                rate_translation: None,
                rate_rotation: None,
                rate_scale_difference: None,
                epoch: None,
                pivot: None,
            }
        );
    }

    #[test]
    fn etrs89_to_wgs84_1_is_a_zero_translation_good_to_1_m() {
        let operation = coordinate_operation(OperationCode::Epsg(1149)).unwrap();
        assert_eq!(operation.name, "ETRS89 to WGS 84 (1)");
        assert_eq!(
            (operation.source_crs_code, operation.target_crs_code),
            (4258, 4326)
        );
        assert_eq!(operation.accuracy, Some(1.0));
        assert_eq!(operation.extent_codes, [4755]);
        assert_eq!(helmert(1149).method.code, 9603);
        assert_eq!(helmert(1149).translation, metres([0.0, 0.0, 0.0]));
        assert_eq!(helmert(1149).epoch, None);
    }

    #[test]
    fn itrf2014_to_etrf2014_carries_rates_and_epoch() {
        let operation = coordinate_operation(OperationCode::Epsg(8366)).unwrap();
        assert_eq!(operation.name, "ITRF2014 to ETRF2014 (1)");
        assert_eq!(
            (operation.source_crs_code, operation.target_crs_code),
            (7789, 8401)
        );
        assert_eq!(operation.extent_codes, [1298]);

        let zero = |unit_code| MeasuredVector {
            values: [0.0, 0.0, 0.0],
            unit_code,
        };
        let parameters = helmert(8366);
        assert_eq!(
            parameters.method,
            OperationMethod {
                code: 1053,
                name: "Time-dependent Position Vector tfm (geocentric)",
            }
        );
        assert_eq!(parameters.translation, zero(1025));
        assert_eq!(parameters.rotation, Some(zero(1031)));
        assert_eq!(
            parameters.scale_difference,
            Some(Measure {
                value: 0.0,
                unit_code: 1028,
            })
        );
        assert_eq!(parameters.rate_translation, Some(zero(1027)));
        assert_eq!(
            parameters.rate_rotation,
            Some(MeasuredVector {
                values: [0.085, 0.531, -0.77],
                unit_code: 1032,
            })
        );
        assert_eq!(
            parameters.epoch,
            Some(Measure {
                value: 1989.0,
                unit_code: 1029,
            })
        );
        assert_eq!(parameters.pivot, None);
        assert_eq!(unit_of_measure(1029).unwrap().name, "year");
    }

    #[test]
    fn operations_keep_every_area_of_use_in_proj_order() {
        assert_eq!(
            coordinate_operation(OperationCode::Epsg(1240))
                .unwrap()
                .extent_codes,
            [1262, 4780]
        );
    }

    fn undirected(reference: StepReference<'static>) -> OperationStep<'static> {
        OperationStep {
            reference,
            direction: None,
        }
    }

    #[test]
    fn concatenated_operation_lists_its_steps() {
        let operation = coordinate_operation(OperationCode::Epsg(10778)).unwrap();
        assert_eq!(operation.name, "KKJ to EUREF-FIN (3)");
        assert_eq!(
            (operation.source_crs_code, operation.target_crs_code),
            (4123, 10690)
        );
        assert_eq!(operation.accuracy, Some(0.03));
        assert_eq!(
            operation.kind,
            OperationKind::ConcatenatedOperation(vec![
                undirected(StepReference::Conversion(18193)),
                undirected(StepReference::Operation(OperationCode::Epsg(10703))),
                undirected(StepReference::Conversion(16065)),
            ])
        );

        let tin = coordinate_operation(OperationCode::Epsg(10703)).unwrap();
        assert_eq!((tin.source_crs_code, tin.target_crs_code), (2393, 3067));
        assert_eq!(
            conversion(18193).unwrap().name,
            "Finland Uniform Coordinate System"
        );
        assert_eq!(conversion(16065).unwrap().name, "TM35FIN");
    }

    #[test]
    fn ntv2_operation_names_its_grid_and_proj_cdn_file() {
        let operation = coordinate_operation(OperationCode::Epsg(1313)).unwrap();
        assert_eq!(operation.name, "NAD27 to NAD83 (4)");
        assert_eq!(operation.accuracy, Some(1.5));
        assert_eq!(
            operation.kind,
            OperationKind::GridTransformation(TransformationParameters {
                method: OperationMethod {
                    code: 9615,
                    name: "NTv2",
                },
                parameters: Vec::new(),
                grids: vec![GridFileReference {
                    parameter_code: 8656,
                    parameter_name: "Latitude and longitude difference file",
                    file_name: "NTv2_0.gsb",
                }],
                interpolation_crs_code: None,
            })
        );

        assert_eq!(
            grid_alternative("NTv2_0.gsb"),
            Some(&GridAlternativeRecord {
                original_name: "NTv2_0.gsb",
                proj_grid_name: Some("ca_nrc_ntv2_0.tif"),
                old_proj_grid_name: Some("ntv2_0.gsb"),
                format: "GTiff",
                proj_method: "hgridshift",
                inverse_direction: false,
                url: Some("https://cdn.proj.org/ca_nrc_ntv2_0.tif"),
                open_license: true,
                direct_download: true,
            })
        );
        assert_eq!(grid_alternative("no such grid"), None);
    }

    #[test]
    fn operations_between_two_crs_answer_both_directions() {
        let expected: Vec<u32> = [1188, 1251, 1252, 1696, 1697, 1702]
            .into_iter()
            .chain(1708..=1750)
            .chain([15835, 15837, 15839])
            .collect();
        let codes_and_directions = |source, target| -> Vec<(OperationCode<'static>, bool)> {
            operations_between(source, target)
                .iter()
                .map(|directed| (directed.operation.code, directed.inverse))
                .collect()
        };

        assert_eq!(
            codes_and_directions(4269, 4326),
            expected
                .iter()
                .map(|code| (OperationCode::Epsg(*code), false))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            codes_and_directions(4326, 4269),
            expected
                .iter()
                .map(|code| (OperationCode::Epsg(*code), true))
                .collect::<Vec<_>>()
        );
        assert!(operations_from(4269).iter().any(|directed| {
            directed.operation.code == OperationCode::Epsg(1313) && directed.inverse
        }));
        assert!(operations_between(4269, 999_999).is_empty());
    }

    #[test]
    fn supersession_names_the_replacement() {
        assert_eq!(
            supersessions(1112),
            [SupersessionRecord {
                superseded_code: 1112,
                replacement_code: 1672,
                same_source_target_crs: true,
            }]
        );
        assert!(supersessions(1188).is_empty());
    }
    #[test]
    fn proj_authority_concatenated_operation_keeps_step_directions() {
        let operation = coordinate_operation(OperationCode::Proj("KKJ_TO_EUREF_FIN")).unwrap();
        assert_eq!(operation.name, "KKJ to EUREF-FIN (using EPSG:10703)");
        assert_eq!(
            (operation.source_crs_code, operation.target_crs_code),
            (4123, 10690)
        );
        assert_eq!(operation.accuracy, None);
        assert_eq!(operation.extent_codes, [3333]);
        let step = |reference, direction| OperationStep {
            reference,
            direction: Some(direction),
        };
        assert_eq!(
            operation.kind,
            OperationKind::ConcatenatedOperation(vec![
                step(StepReference::Conversion(18193), StepDirection::Forward),
                step(
                    StepReference::Operation(OperationCode::Epsg(10703)),
                    StepDirection::Forward
                ),
                step(StepReference::Conversion(16065), StepDirection::Reverse),
            ])
        );
    }

    #[test]
    fn proj_authority_helmert_sits_after_every_epsg_operation() {
        let operation = coordinate_operation(OperationCode::Proj("WGS84_TO_WGS84_G1762")).unwrap();
        assert_eq!(operation.name, "WGS 84 to WGS 84 (G1762)");
        assert_eq!(
            (operation.source_crs_code, operation.target_crs_code),
            (4326, 9057)
        );
        assert_eq!(operation.accuracy, Some(2.0));
        assert_eq!(operation.extent_codes, [1262]);
        assert!(
            operations_between(4326, 9057)
                .iter()
                .any(|directed| directed.operation.code == operation.code && !directed.inverse)
        );
        let last = registry().operations.last().unwrap();
        assert!(matches!(last.code, OperationCode::Proj(_)));
        assert_eq!(
            coordinate_operation(OperationCode::Proj("S_JTSK_GEOG_TO_S_JTSK05_GEOG")),
            None
        );
    }

    #[test]
    fn nad83_2011_datum_carries_its_anchor_epoch() {
        let datum = geodetic_datum(1116).unwrap();
        assert_eq!(datum.name, "NAD83 (National Spatial Reference System 2011)");
        assert_eq!(datum.anchor_epoch, Some(2010.0));
        assert_eq!(geodetic_datum(6269).unwrap().anchor_epoch, None);
    }

    #[test]
    fn grid_file_resolves_by_epsg_or_proj_name() {
        let by_epsg_name = grid_alternative_for_file("NTv2_0.gsb").unwrap();
        let by_proj_name = grid_alternative_for_file("ca_nrc_ntv2_0.tif").unwrap();
        assert_eq!(by_epsg_name, by_proj_name);
        assert_eq!(by_proj_name.original_name, "NTv2_0.gsb");
        assert_eq!(grid_alternative_for_file("no such grid"), None);
    }
}
