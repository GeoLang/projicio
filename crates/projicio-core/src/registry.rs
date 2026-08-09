//! Embedded EPSG metadata registry.
//!
//! Reads the blob generated from PROJ's proj.db by `tools/gen-epsg-registry`.
//! It carries names, kind, deprecation, datum links, areas of use, and datum
//! aliases. Nothing here takes part in transforming a coordinate.
//!
//! The blob is parsed once on first use. The provenance file next to it
//! records the dataset it came from and the checksum CI reproduces.

use projicio_epsg_format::{
    CRS_KIND_GEOGRAPHIC, CRS_KIND_OTHER, CRS_KIND_PROJECTED, CRS_RECORD_BASE_SIZE,
    DATUM_ALIAS_RECORD_BASE_SIZE, EXTENT_RECORD_BASE_SIZE, FLAG_DEPRECATED, HEADER_SIZE, MAGIC,
    NO_CODE, SECTION_CRS, SECTION_DATUM_ALIAS, SECTION_ENTRY_SIZE, SECTION_EXTENT, VERSION, read,
};
use std::sync::OnceLock;
use thiserror::Error;

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

struct Registry {
    crs: Vec<CrsRecord>,
    extents: Vec<Extent>,
    datum_aliases: Vec<DatumAlias>,
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

    Ok(Registry {
        crs,
        extents,
        datum_aliases,
    })
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
        assert_eq!(provenance_field("epsg_version"), "v12.013");
        assert_eq!(provenance_field("proj_version"), "9.6.2");
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
        assert_eq!(
            parse_error(&REGISTRY_BYTES[..REGISTRY_BYTES.len() / 2]),
            RegistryError::MalformedSection { tag: SECTION_CRS }
        );
    }

    #[test]
    fn foreign_blob_is_rejected() {
        static NOT_A_REGISTRY: &[u8] = b"this is not an epsg registry blob at all";
        assert_eq!(parse_error(NOT_A_REGISTRY), RegistryError::BadMagic);
    }
}
