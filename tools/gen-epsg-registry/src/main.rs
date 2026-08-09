//! Generate projicio's embedded EPSG metadata registry from PROJ's proj.db.
//!
//! Write: `cargo run --manifest-path tools/gen-epsg-registry/Cargo.toml`
//! Check: `cargo run --manifest-path tools/gen-epsg-registry/Cargo.toml -- --check`
//!
//! Outputs `crates/projicio-core/data/epsg.bin` and `epsg.provenance.json`.
//!
//! This package is deliberately outside the projicio workspace so that
//! `cargo test --all` never builds PROJ or SQLite. It depends on `proj-sys`
//! with a bundled PROJ build purely to pin the proj.db it reads: the registry
//! is generated from that build, not from whatever PROJ the machine has
//! installed.

use projicio_epsg_format::checksum::sha256_hex;
use projicio_epsg_format::{
    CRS_KIND_GEOGRAPHIC, CRS_KIND_OTHER, CRS_KIND_PROJECTED, FLAG_DEPRECATED, HEADER_SIZE, MAGIC,
    NO_CODE, SECTION_CRS, SECTION_DATUM_ALIAS, SECTION_ENTRY_SIZE, SECTION_EXTENT, VERSION, write,
};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const REGISTRY_FILE: &str = "epsg.bin";
const PROVENANCE_FILE: &str = "epsg.provenance.json";
const PROVENANCE_SCHEMA_VERSION: u16 = 1;
const GENERATOR_NAME: &str = "gen-epsg-registry";
const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Decimal places used to round every float before it reaches the blob, so a
/// value that differs only in the last bits of its decimal expansion cannot
/// change the output.
const CANONICAL_FLOAT_DECIMAL_PLACES: usize = 13;

/// proj.db metadata keys recorded in the provenance file. Both must be
/// present: without them the artifact cannot be traced to a dataset release.
const EPSG_VERSION_KEY: &str = "EPSG.VERSION";
const PROJ_VERSION_KEY: &str = "PROJ.VERSION";

fn main() {
    let args = Args::parse();

    let proj_db = args
        .proj_db
        .clone()
        .unwrap_or_else(|| find_pinned_proj_db().unwrap_or_else(|message| fatal(message)));
    eprintln!("Reading {}", proj_db.display());

    let connection = open_read_only(&proj_db);
    let source_metadata = read_source_metadata(&connection);
    let source_digest = normalized_proj_db_sha256(&connection);

    let extent_codes_by_crs = load_extent_codes_by_crs(&connection);
    let datum_codes_by_crs = load_datum_codes_by_crs(&connection);
    let extents = load_extents(&connection);
    let crs_records = load_crs_records(
        &connection,
        &datum_codes_by_crs,
        &extent_codes_by_crs,
        &extents,
    );

    let referenced_extents = referenced_extents(&crs_records, &extents);
    let used_datum_codes: BTreeSet<u32> = crs_records
        .iter()
        .map(|record| record.datum_code)
        .filter(|code| *code != NO_CODE)
        .collect();
    let datum_aliases = load_datum_aliases(&connection, &used_datum_codes);

    let sections = [
        Section {
            tag: SECTION_CRS,
            record_count: crs_records.len(),
            payload: encode_crs_records(&crs_records),
        },
        Section {
            tag: SECTION_EXTENT,
            record_count: referenced_extents.len(),
            payload: encode_extent_records(&referenced_extents),
        },
        Section {
            tag: SECTION_DATUM_ALIAS,
            record_count: datum_aliases.len(),
            payload: encode_datum_alias_records(&datum_aliases),
        },
    ];
    let blob = assemble(&sections);

    let counts = Counts {
        crs: crs_records.len(),
        extents: referenced_extents.len(),
        datum_aliases: datum_aliases.len(),
    };
    eprintln!(
        "CRS: {}, extents: {}, datum aliases: {}",
        counts.crs, counts.extents, counts.datum_aliases
    );

    let provenance = Provenance {
        schema_version: PROVENANCE_SCHEMA_VERSION,
        generator: GeneratorProvenance {
            name: GENERATOR_NAME,
            version: GENERATOR_VERSION,
        },
        registry_format: FormatProvenance {
            magic: std::str::from_utf8(&MAGIC).expect("magic is ascii"),
            version: VERSION,
        },
        source_database: SourceProvenance {
            kind: "PROJ proj.db",
            file_name: "proj.db",
            epsg_version: required_metadata(&source_metadata, EPSG_VERSION_KEY),
            proj_version: required_metadata(&source_metadata, PROJ_VERSION_KEY),
            normalized_content_sha256: source_digest,
        },
        output: OutputProvenance {
            file_name: REGISTRY_FILE,
            byte_len: blob.len(),
            sha256: sha256_hex(&blob),
        },
        counts,
    };
    let mut provenance_json = serde_json::to_vec_pretty(&provenance)
        .unwrap_or_else(|error| fatal(format!("failed to serialize provenance: {error}")));
    provenance_json.push(b'\n');

    let registry_path = args.out_dir.join(REGISTRY_FILE);
    let provenance_path = args.out_dir.join(PROVENANCE_FILE);
    match args.mode {
        Mode::Write => {
            fs::create_dir_all(&args.out_dir).unwrap_or_else(|error| {
                fatal(format!(
                    "failed to create {}: {error}",
                    args.out_dir.display()
                ))
            });
            write_file(&registry_path, &blob);
            write_file(&provenance_path, &provenance_json);
            eprintln!(
                "Wrote {} ({:.1} KiB) and {}",
                registry_path.display(),
                blob.len() as f64 / 1024.0,
                provenance_path.display()
            );
        }
        Mode::Check => {
            assert_reproducible(&registry_path, &blob);
            assert_reproducible(&provenance_path, &provenance_json);
            eprintln!(
                "Reproducible from the pinned proj.db: {}, {}",
                registry_path.display(),
                provenance_path.display()
            );
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Write,
    Check,
}

struct Args {
    mode: Mode,
    proj_db: Option<PathBuf>,
    out_dir: PathBuf,
}

impl Args {
    fn parse() -> Self {
        Self::parse_from(std::env::args().skip(1)).unwrap_or_else(|message| {
            eprintln!("{message}");
            std::process::exit(2);
        })
    }

    fn parse_from<I: IntoIterator<Item = String>>(args: I) -> Result<Self, String> {
        let mut mode = Mode::Write;
        let mut proj_db = None;
        let mut out_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/projicio-core/data");

        let mut remaining = args.into_iter();
        while let Some(arg) = remaining.next() {
            match arg.as_str() {
                "--write" => mode = Mode::Write,
                "--check" => mode = Mode::Check,
                "--proj-db" => {
                    proj_db = Some(PathBuf::from(
                        remaining
                            .next()
                            .ok_or_else(|| "--proj-db needs a path".to_string())?,
                    ));
                }
                "--out-dir" => {
                    out_dir = PathBuf::from(
                        remaining
                            .next()
                            .ok_or_else(|| "--out-dir needs a path".to_string())?,
                    );
                }
                "--help" | "-h" => {
                    return Err(
                        "usage: gen-epsg-registry [--write|--check] [--proj-db PATH] [--out-dir DIR]"
                            .to_string(),
                    );
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }

        Ok(Self {
            mode,
            proj_db,
            out_dir,
        })
    }
}

/// Directory PROJ's build puts its own cut-down test fixture database in.
const PROJ_TEST_FIXTURE_DIR: &str = "for_tests";

/// Locate the proj.db that this package's pinned bundled PROJ build produced.
///
/// The bundled build drops copies under the package's own target directory, so
/// several identical files are normal. Distinct contents mean the caller has
/// to say which one they meant.
fn find_pinned_proj_db() -> Result<PathBuf, String> {
    let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    let mut candidates: Vec<PathBuf> = find_files_named(&target_dir, "proj.db")
        .into_iter()
        .filter(|path| {
            !path
                .components()
                .any(|component| component.as_os_str() == PROJ_TEST_FIXTURE_DIR)
        })
        .collect();
    candidates.sort();
    if candidates.is_empty() {
        return Err(format!(
            "no proj.db below {}. Build this package first so the bundled PROJ build produces one.",
            target_dir.display()
        ));
    }

    let mut by_digest = BTreeMap::<String, PathBuf>::new();
    for candidate in &candidates {
        let connection = open_read_only(candidate);
        by_digest
            .entry(normalized_proj_db_sha256(&connection))
            .or_insert_with(|| candidate.clone());
    }
    if by_digest.len() > 1 {
        let listing = by_digest
            .into_iter()
            .map(|(digest, path)| format!("{digest} {}", path.display()))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "found proj.db files with different contents, pass --proj-db:\n{listing}"
        ));
    }

    Ok(candidates.remove(0))
}

fn find_files_named(dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(find_files_named(&path, name));
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            found.push(path);
        }
    }
    found
}

fn open_read_only(path: &Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap_or_else(|error| fatal(format!("failed to open {}: {error}", path.display())))
}

// ---------------------------------------------------------------------------
// proj.db reading
// ---------------------------------------------------------------------------

struct CrsRecord {
    code: u32,
    kind: u8,
    flags: u8,
    datum_code: u32,
    extent_code: u32,
    name: String,
}

#[derive(Clone)]
struct ExtentRecord {
    code: u32,
    west: f64,
    south: f64,
    east: f64,
    north: f64,
    name: String,
}

fn load_crs_records(
    connection: &Connection,
    datum_codes: &BTreeMap<u32, u32>,
    extent_codes: &BTreeMap<u32, u32>,
    extents: &BTreeMap<u32, ExtentRecord>,
) -> Vec<CrsRecord> {
    let mut statement = prepare(
        connection,
        "SELECT CAST(code AS TEXT), name, type, deprecated
         FROM crs_view
         WHERE auth_name = 'EPSG'",
    );
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .unwrap_or_else(|error| fatal(format!("failed to query crs_view: {error}")));

    let mut records = BTreeMap::<u32, CrsRecord>::new();
    for row in rows {
        let (code_text, name, kind_text, deprecated) =
            row.unwrap_or_else(|error| fatal(format!("failed to read a crs_view row: {error}")));
        let code = parse_code(&code_text, "CRS");
        let datum_code = datum_codes.get(&code).copied().unwrap_or(NO_CODE);
        let extent_code = extent_codes
            .get(&code)
            .copied()
            .filter(|extent_code| extents.contains_key(extent_code))
            .unwrap_or(NO_CODE);
        let record = CrsRecord {
            code,
            kind: crs_kind(&kind_text),
            flags: if deprecated == 0 { 0 } else { FLAG_DEPRECATED },
            datum_code,
            extent_code,
            name,
        };
        if records.insert(code, record).is_some() {
            fatal(format!(
                "EPSG CRS code {code} appears in two crs_view tables"
            ));
        }
    }

    records.into_values().collect()
}

fn crs_kind(kind_text: &str) -> u8 {
    match kind_text {
        "geographic 2D" | "geographic 3D" => CRS_KIND_GEOGRAPHIC,
        "projected" => CRS_KIND_PROJECTED,
        _ => CRS_KIND_OTHER,
    }
}

/// Map each EPSG CRS to its geodetic datum, following projected CRS through
/// their base geographic CRS.
///
/// Vertical and engineering CRS reference datums in other tables, so they get
/// no link: the registry's datum codes all name entries of `geodetic_datum`.
fn load_datum_codes_by_crs(connection: &Connection) -> BTreeMap<u32, u32> {
    let pairs = query_code_pairs(
        connection,
        "SELECT CAST(code AS TEXT), CAST(datum_code AS TEXT)
         FROM geodetic_crs
         WHERE auth_name = 'EPSG' AND datum_auth_name = 'EPSG'
         UNION ALL
         SELECT CAST(projected.code AS TEXT), CAST(base.datum_code AS TEXT)
         FROM projected_crs projected
         JOIN geodetic_crs base
           ON base.auth_name = projected.geodetic_crs_auth_name
          AND base.code = projected.geodetic_crs_code
         WHERE projected.auth_name = 'EPSG'
           AND base.auth_name = 'EPSG'
           AND base.datum_auth_name = 'EPSG'",
    );

    let mut datum_codes = BTreeMap::new();
    for (crs_text, datum_text) in pairs {
        let crs_code = parse_code(&crs_text, "CRS");
        let datum_code = parse_code(&datum_text, "geodetic datum");
        datum_codes.insert(crs_code, datum_code);
    }
    datum_codes
}

/// Map each EPSG CRS to its area of use.
///
/// A handful of CRS record more than one usage. The lowest extent code wins so
/// the choice does not depend on row order.
fn load_extent_codes_by_crs(connection: &Connection) -> BTreeMap<u32, u32> {
    let pairs = query_code_pairs(
        connection,
        "SELECT CAST(usage.object_code AS TEXT), CAST(usage.extent_code AS TEXT)
         FROM usage
         JOIN crs_view
           ON crs_view.table_name = usage.object_table_name
          AND crs_view.auth_name = usage.object_auth_name
          AND crs_view.code = usage.object_code
         WHERE usage.object_auth_name = 'EPSG' AND usage.extent_auth_name = 'EPSG'",
    );

    let mut extent_codes = BTreeMap::<u32, u32>::new();
    for (crs_text, extent_text) in pairs {
        let crs_code = parse_code(&crs_text, "CRS");
        let extent_code = parse_code(&extent_text, "extent");
        extent_codes
            .entry(crs_code)
            .and_modify(|current| *current = (*current).min(extent_code))
            .or_insert(extent_code);
    }
    extent_codes
}

/// Load every EPSG extent that has a complete bounding box.
///
/// An extent missing a bound cannot answer whether a coordinate falls inside
/// it, so it is dropped and the CRS that referenced it records no area of use.
fn load_extents(connection: &Connection) -> BTreeMap<u32, ExtentRecord> {
    let mut statement = prepare(
        connection,
        "SELECT CAST(code AS TEXT), name, west_lon, south_lat, east_lon, north_lat
         FROM extent
         WHERE auth_name = 'EPSG'
           AND west_lon IS NOT NULL
           AND south_lat IS NOT NULL
           AND east_lon IS NOT NULL
           AND north_lat IS NOT NULL",
    );
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, f64>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, f64>(5)?,
            ))
        })
        .unwrap_or_else(|error| fatal(format!("failed to query extent: {error}")));

    let mut extents = BTreeMap::new();
    for row in rows {
        let (code_text, name, west, south, east, north) =
            row.unwrap_or_else(|error| fatal(format!("failed to read an extent row: {error}")));
        let code = parse_code(&code_text, "extent");
        extents.insert(
            code,
            ExtentRecord {
                code,
                west,
                south,
                east,
                north,
                name,
            },
        );
    }
    extents
}

fn referenced_extents(
    crs_records: &[CrsRecord],
    extents: &BTreeMap<u32, ExtentRecord>,
) -> Vec<ExtentRecord> {
    let referenced: BTreeSet<u32> = crs_records
        .iter()
        .map(|record| record.extent_code)
        .filter(|code| *code != NO_CODE)
        .collect();
    referenced
        .into_iter()
        .map(|code| extents[&code].clone())
        .collect()
}

/// Names and alternative names for the geodetic datums the CRS records link
/// to, deduplicated and sorted.
fn load_datum_aliases(
    connection: &Connection,
    used_datum_codes: &BTreeSet<u32>,
) -> Vec<(u32, String)> {
    let pairs = query_code_pairs(
        connection,
        "SELECT CAST(code AS TEXT), name
         FROM geodetic_datum
         WHERE auth_name = 'EPSG'
         UNION ALL
         SELECT CAST(code AS TEXT), alt_name
         FROM alias_name
         WHERE table_name = 'geodetic_datum' AND auth_name = 'EPSG'",
    );

    let mut aliases = BTreeSet::<(u32, String)>::new();
    for (code_text, name) in pairs {
        let code = parse_code(&code_text, "geodetic datum");
        if used_datum_codes.contains(&code) && !name.is_empty() {
            aliases.insert((code, name));
        }
    }
    aliases.into_iter().collect()
}

fn read_source_metadata(connection: &Connection) -> BTreeMap<String, String> {
    query_code_pairs(connection, "SELECT key, value FROM metadata")
        .into_iter()
        .collect()
}

fn required_metadata(metadata: &BTreeMap<String, String>, key: &str) -> String {
    metadata
        .get(key)
        .cloned()
        .unwrap_or_else(|| fatal(format!("proj.db metadata has no {key}")))
}

/// Digest the database contents rather than the file, so that a rebuild that
/// only changes SQLite page layout or free-page noise still compares equal.
fn normalized_proj_db_sha256(connection: &Connection) -> String {
    let mut payload = Vec::new();
    let tables = query_single_column(
        connection,
        "SELECT name
         FROM sqlite_schema
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
    );

    for table in tables {
        append_tagged(&mut payload, b't', table.as_bytes());
        let columns = query_single_column(
            connection,
            &format!(
                "SELECT name FROM pragma_table_info({})",
                quote_string(&table)
            ),
        );
        for column in &columns {
            append_tagged(&mut payload, b'c', column.as_bytes());
        }

        let column_list = columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {column_list} FROM {} ORDER BY {column_list}",
            quote_identifier(&table)
        );
        let mut statement = prepare(connection, &sql);
        let mut rows = statement
            .query([])
            .unwrap_or_else(|error| fatal(format!("failed to scan {table}: {error}")));
        while let Some(row) = rows
            .next()
            .unwrap_or_else(|error| fatal(format!("failed to read a {table} row: {error}")))
        {
            payload.push(b'r');
            for index in 0..columns.len() {
                let value = row.get_ref(index).unwrap_or_else(|error| {
                    fatal(format!("failed to read {table} value: {error}"))
                });
                match value {
                    ValueRef::Null => payload.push(b'n'),
                    ValueRef::Integer(value) => {
                        payload.push(b'i');
                        payload.extend_from_slice(&value.to_le_bytes());
                    }
                    ValueRef::Real(value) => {
                        payload.push(b'f');
                        payload.extend_from_slice(&canonical_f64(value).to_le_bytes());
                    }
                    ValueRef::Text(value) => append_tagged(&mut payload, b's', value),
                    ValueRef::Blob(value) => append_tagged(&mut payload, b'b', value),
                }
            }
        }
    }

    sha256_hex(&payload)
}

fn append_tagged(payload: &mut Vec<u8>, tag: u8, bytes: &[u8]) {
    payload.push(tag);
    payload.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    payload.extend_from_slice(bytes);
}

fn prepare<'a>(connection: &'a Connection, sql: &str) -> rusqlite::Statement<'a> {
    connection
        .prepare(sql)
        .unwrap_or_else(|error| fatal(format!("failed to prepare `{sql}`: {error}")))
}

fn query_code_pairs(connection: &Connection, sql: &str) -> Vec<(String, String)> {
    let mut statement = prepare(connection, sql);
    statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap_or_else(|error| fatal(format!("failed to run `{sql}`: {error}")))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| fatal(format!("failed to read `{sql}`: {error}")))
}

fn query_single_column(connection: &Connection, sql: &str) -> Vec<String> {
    let mut statement = prepare(connection, sql);
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap_or_else(|error| fatal(format!("failed to run `{sql}`: {error}")))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| fatal(format!("failed to read `{sql}`: {error}")))
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn quote_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn parse_code(text: &str, kind: &str) -> u32 {
    let code = text
        .parse::<u32>()
        .unwrap_or_else(|_| fatal(format!("EPSG {kind} code {text:?} is not a number")));
    if code == NO_CODE {
        fatal(format!(
            "EPSG {kind} code 0 collides with the absent marker"
        ));
    }
    code
}

// ---------------------------------------------------------------------------
// blob assembly
// ---------------------------------------------------------------------------

struct Section {
    tag: u32,
    record_count: usize,
    payload: Vec<u8>,
}

fn assemble(sections: &[Section]) -> Vec<u8> {
    let mut blob = Vec::new();
    blob.extend_from_slice(&MAGIC);
    write::u16(&mut blob, VERSION);
    write::u16(&mut blob, to_u16(sections.len(), "section count"));

    let mut offset = HEADER_SIZE + sections.len() * SECTION_ENTRY_SIZE;
    for section in sections {
        write::u32(&mut blob, section.tag);
        write::u32(&mut blob, to_u32(section.record_count, "record count"));
        write::u32(&mut blob, to_u32(offset, "section offset"));
        write::u32(&mut blob, to_u32(section.payload.len(), "section length"));
        offset += section.payload.len();
    }
    for section in sections {
        blob.extend_from_slice(&section.payload);
    }
    blob
}

fn encode_crs_records(records: &[CrsRecord]) -> Vec<u8> {
    let mut payload = Vec::new();
    for record in records {
        write::u32(&mut payload, record.code);
        write::u8(&mut payload, record.kind);
        write::u8(&mut payload, record.flags);
        write::u32(&mut payload, record.datum_code);
        write::u32(&mut payload, record.extent_code);
        write_string(&mut payload, &record.name);
    }
    payload
}

fn encode_extent_records(records: &[ExtentRecord]) -> Vec<u8> {
    let mut payload = Vec::new();
    for record in records {
        write::u32(&mut payload, record.code);
        write::f64(&mut payload, canonical_f64(record.west));
        write::f64(&mut payload, canonical_f64(record.south));
        write::f64(&mut payload, canonical_f64(record.east));
        write::f64(&mut payload, canonical_f64(record.north));
        write_string(&mut payload, &record.name);
    }
    payload
}

fn encode_datum_alias_records(records: &[(u32, String)]) -> Vec<u8> {
    let mut payload = Vec::new();
    for (code, name) in records {
        write::u32(&mut payload, *code);
        write_string(&mut payload, name);
    }
    payload
}

fn write_string(payload: &mut Vec<u8>, value: &str) {
    write::string_u16(payload, value)
        .unwrap_or_else(|error| fatal(format!("cannot encode {value:?}: {error}")));
}

fn canonical_f64(value: f64) -> f64 {
    if !value.is_finite() {
        fatal(format!("registry value {value} is not finite"));
    }
    if value == 0.0 {
        return 0.0;
    }
    format!("{value:.CANONICAL_FLOAT_DECIMAL_PLACES$e}")
        .parse()
        .unwrap_or_else(|error| fatal(format!("cannot canonicalize {value}: {error}")))
}

fn to_u16(value: usize, label: &str) -> u16 {
    u16::try_from(value).unwrap_or_else(|_| fatal(format!("{label} {value} does not fit in u16")))
}

fn to_u32(value: usize, label: &str) -> u32 {
    u32::try_from(value).unwrap_or_else(|_| fatal(format!("{label} {value} does not fit in u32")))
}

// ---------------------------------------------------------------------------
// provenance and output
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct Provenance {
    schema_version: u16,
    generator: GeneratorProvenance,
    registry_format: FormatProvenance,
    source_database: SourceProvenance,
    output: OutputProvenance,
    counts: Counts,
}

#[derive(Serialize)]
struct GeneratorProvenance {
    name: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct FormatProvenance {
    magic: &'static str,
    version: u16,
}

#[derive(Serialize)]
struct SourceProvenance {
    kind: &'static str,
    file_name: &'static str,
    epsg_version: String,
    proj_version: String,
    normalized_content_sha256: String,
}

#[derive(Serialize)]
struct OutputProvenance {
    file_name: &'static str,
    byte_len: usize,
    sha256: String,
}

#[derive(Clone, Copy, Serialize)]
struct Counts {
    crs: usize,
    extents: usize,
    datum_aliases: usize,
}

fn write_file(path: &Path, contents: &[u8]) {
    fs::write(path, contents)
        .unwrap_or_else(|error| fatal(format!("failed to write {}: {error}", path.display())));
}

fn assert_reproducible(path: &Path, expected: &[u8]) {
    let existing = fs::read(path).unwrap_or_else(|error| {
        fatal(format!(
            "{} is missing or unreadable: {error}",
            path.display()
        ))
    });
    if existing != expected {
        fatal(format!(
            "{} does not match the pinned proj.db\n  expected {} ({} bytes)\n  found    {} ({} bytes)\nregenerate with: cargo run --manifest-path tools/gen-epsg-registry/Cargo.toml",
            path.display(),
            sha256_hex(expected),
            expected.len(),
            sha256_hex(&existing),
            existing.len()
        ));
    }
}

fn fatal(message: impl AsRef<str>) -> ! {
    eprintln!("error: {}", message.as_ref());
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_float_rounds_to_a_stable_decimal_expansion() {
        assert_eq!(canonical_f64(0.0), 0.0);
        assert_eq!(canonical_f64(-180.0), -180.0);
        assert_eq!(canonical_f64(1.0 / 3.0), 0.33333333333333);
        assert_eq!(canonical_f64(49.000_000_000_000_01), 49.0);
    }

    #[test]
    fn crs_kind_maps_geographic_and_projected() {
        assert_eq!(crs_kind("geographic 2D"), CRS_KIND_GEOGRAPHIC);
        assert_eq!(crs_kind("geographic 3D"), CRS_KIND_GEOGRAPHIC);
        assert_eq!(crs_kind("projected"), CRS_KIND_PROJECTED);
        assert_eq!(crs_kind("geocentric"), CRS_KIND_OTHER);
        assert_eq!(crs_kind("vertical"), CRS_KIND_OTHER);
        assert_eq!(crs_kind("compound"), CRS_KIND_OTHER);
    }

    #[test]
    fn section_offsets_follow_the_header_and_table() {
        let sections = [
            Section {
                tag: SECTION_CRS,
                record_count: 1,
                payload: vec![1, 2, 3],
            },
            Section {
                tag: SECTION_EXTENT,
                record_count: 2,
                payload: vec![4, 5],
            },
        ];

        let blob = assemble(&sections);

        let first_payload_offset = HEADER_SIZE + 2 * SECTION_ENTRY_SIZE;
        let offset_field = HEADER_SIZE + 8;
        assert_eq!(&blob[..MAGIC.len()], MAGIC);
        assert_eq!(
            &blob[offset_field..offset_field + 4],
            &(first_payload_offset as u32).to_le_bytes()
        );
        assert_eq!(
            &blob[offset_field + SECTION_ENTRY_SIZE..offset_field + SECTION_ENTRY_SIZE + 4],
            &(first_payload_offset as u32 + 3).to_le_bytes()
        );
        assert_eq!(&blob[first_payload_offset..], &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn identifiers_and_strings_are_quoted() {
        assert_eq!(quote_identifier(r#"od"d"#), r#""od""d""#);
        assert_eq!(quote_string("it's"), "'it''s'");
    }

    #[test]
    fn args_default_to_writing() {
        let args = Args::parse_from(Vec::new()).unwrap();
        assert!(args.mode == Mode::Write && args.proj_db.is_none());

        let args = Args::parse_from(["--check".to_string()]).unwrap();
        assert!(args.mode == Mode::Check);

        assert!(Args::parse_from(["--proj-db".to_string()]).is_err());
        assert!(Args::parse_from(["--nope".to_string()]).is_err());
    }
}
