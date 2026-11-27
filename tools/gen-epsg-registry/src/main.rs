//! Generate projicio's embedded EPSG registry from PROJ's proj.db.
//!
//! Write: `cargo run --manifest-path tools/gen-epsg-registry/Cargo.toml -- --proj-db PATH`
//! Check: `cargo run --manifest-path tools/gen-epsg-registry/Cargo.toml -- --check --proj-db PATH`
//!
//! Outputs `crates/projicio-core/data/epsg.bin` and `epsg.provenance.json`.
//!
//! This package is deliberately outside the projicio workspace so that
//! `cargo test --all` never builds SQLite. The proj.db it reads must come from
//! the pinned PROJ release, [`PINNED_PROJ_VERSION`].

use flate2::Compression;
use flate2::write::GzEncoder;
use projicio_epsg_format::checksum::sha256_hex;
use projicio_epsg_format::records::{
    Axis, ConversionRecord, CoordinateSystemKind, CoordinateSystemRecord, CrsDefinitionKind,
    CrsDefinitionRecord, DatumEnsemble, DatumRecord, EllipsoidRecord, EllipsoidShape,
    GeodeticCrsType, GridAlternativeRecord, GridFileReference, HelmertParameters, Measure,
    MeasuredVector, OperationCode, OperationKind, OperationMethod, OperationRecord, OperationStep,
    ParameterValue, PrimeMeridianRecord, StepDirection, StepReference, StringIndex,
    SupersessionRecord, TransformationParameters, UnitKind, UnitRecord,
};
use projicio_epsg_format::{
    CRS_KIND_GEOGRAPHIC, CRS_KIND_OTHER, CRS_KIND_PROJECTED, FLAG_DEPRECATED, HEADER_SIZE, MAGIC,
    NO_CODE, SECTION_CONVERSION, SECTION_COORDINATE_OPERATION, SECTION_COORDINATE_SYSTEM,
    SECTION_CRS, SECTION_CRS_DEFINITION, SECTION_DATUM, SECTION_DATUM_ALIAS, SECTION_ELLIPSOID,
    SECTION_ENTRY_SIZE, SECTION_EXTENT, SECTION_GRID_RESOURCE, SECTION_PRIME_MERIDIAN,
    SECTION_STRING, SECTION_SUPERSESSION, SECTION_UNIT_OF_MEASURE, VERSION, write,
};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, Row};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const REGISTRY_FILE: &str = "epsg.bin";
const PROVENANCE_FILE: &str = "epsg.provenance.json";
const PROVENANCE_SCHEMA_VERSION: u16 = 1;
const GENERATOR_NAME: &str = "gen-epsg-registry";
const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Decimal places used to round every float before it reaches the blob, so a
/// value that differs only in the last bits of its decimal expansion cannot
/// change the output.
const CANONICAL_FLOAT_DECIMAL_PLACES: usize = 14;

/// proj.db metadata keys recorded in the provenance file. Both must be
/// present: without them the artifact cannot be traced to a dataset release.
const EPSG_VERSION_KEY: &str = "EPSG.VERSION";
const PROJ_VERSION_KEY: &str = "PROJ.VERSION";

const PINNED_PROJ_VERSION: &str = "9.8.1";
const PINNED_EPSG_VERSION: &str = "v12.029";

fn main() {
    let args = Args::parse();
    eprintln!("Reading {}", args.proj_db.display());

    let connection = open_read_only(&args.proj_db);
    let source_metadata = read_source_metadata(&connection);
    let epsg_version = required_metadata(&source_metadata, EPSG_VERSION_KEY);
    let proj_version = required_metadata(&source_metadata, PROJ_VERSION_KEY);
    if proj_version != PINNED_PROJ_VERSION || epsg_version != PINNED_EPSG_VERSION {
        fatal(format!(
            "{} is from PROJ {proj_version} with EPSG {epsg_version}, the registry is pinned to PROJ {PINNED_PROJ_VERSION} with EPSG {PINNED_EPSG_VERSION}",
            args.proj_db.display()
        ));
    }
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

    let tables = load_epsg_tables(&connection, &extents);
    let selection = select_reachable(&tables);

    let referenced_extents = referenced_extents(&crs_records, &selection.extent_codes, &extents);
    let used_datum_codes: BTreeSet<u32> = crs_records
        .iter()
        .map(|record| record.datum_code)
        .filter(|code| *code != NO_CODE)
        .collect();
    let datum_aliases = load_datum_aliases(&connection, &used_datum_codes);

    let mut string_use_counts = BTreeMap::new();
    encode_string_referencing_sections(&selection, &mut |text| {
        *string_use_counts.entry(text).or_insert(0) += 1;
        0
    });
    let string_index = StringIndex::from_use_counts(&string_use_counts);
    let string_payload = string_index
        .encode()
        .unwrap_or_else(|error| fatal(format!("cannot encode the string table: {error}")));

    let mut sections = vec![
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
        Section {
            tag: SECTION_STRING,
            record_count: string_index.string_count(),
            payload: string_payload,
        },
    ];
    sections.extend(encode_string_referencing_sections(
        &selection,
        &mut |text| string_index.index(text),
    ));
    sections.extend([
        Section {
            tag: SECTION_CRS_DEFINITION,
            record_count: selection.crs_definitions.len(),
            payload: encode_each(&selection.crs_definitions, |record, out| record.encode(out)),
        },
        Section {
            tag: SECTION_SUPERSESSION,
            record_count: selection.supersessions.len(),
            payload: encode_each(&selection.supersessions, |record, out| record.encode(out)),
        },
    ]);
    let blob = assemble(&sections);

    for section in &sections {
        eprintln!(
            "section {:>2}: {:>6} records, {:>7} bytes",
            section.tag,
            section.record_count,
            section.payload.len()
        );
    }
    eprintln!(
        "{REGISTRY_FILE}: {} bytes, {} bytes after gzip -9",
        blob.len(),
        gzip_len(&blob)
    );

    let counts = Counts {
        crs: crs_records.len(),
        extents: referenced_extents.len(),
        datum_aliases: datum_aliases.len(),
        strings: string_index.string_count(),
        units_of_measure: selection.units.len(),
        ellipsoids: selection.ellipsoids.len(),
        prime_meridians: selection.prime_meridians.len(),
        datums: selection.datums.len(),
        coordinate_systems: selection.coordinate_systems.len(),
        crs_definitions: selection.crs_definitions.len(),
        conversions: selection.conversions.len(),
        coordinate_operations: selection.operations.len(),
        grid_alternatives: selection.grid_alternatives.len(),
        supersessions: selection.supersessions.len(),
    };

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
            epsg_version,
            proj_version,
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
                "Reproducible from PROJ {PINNED_PROJ_VERSION}'s proj.db: {}, {}",
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
    proj_db: PathBuf,
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
                        "usage: gen-epsg-registry [--write|--check] --proj-db PATH [--out-dir DIR]"
                            .to_string(),
                    );
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }

        Ok(Self {
            mode,
            proj_db: proj_db.ok_or_else(|| {
                format!("--proj-db is required, pass the proj.db of PROJ {PINNED_PROJ_VERSION}")
            })?,
            out_dir,
        })
    }
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
    selected_extent_codes: &BTreeSet<u32>,
    extents: &BTreeMap<u32, ExtentRecord>,
) -> Vec<ExtentRecord> {
    let mut referenced: BTreeSet<u32> = crs_records
        .iter()
        .map(|record| record.extent_code)
        .filter(|code| *code != NO_CODE)
        .collect();
    referenced.extend(selected_extent_codes);
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
// definitions and coordinate operations
// ---------------------------------------------------------------------------

struct EpsgTables {
    units: BTreeMap<u32, UnitRecord<'static>>,
    ellipsoids: BTreeMap<u32, EllipsoidRecord<'static>>,
    prime_meridians: BTreeMap<u32, PrimeMeridianRecord<'static>>,
    datums: BTreeMap<u32, DatumRecord<'static>>,
    coordinate_systems: BTreeMap<u32, CoordinateSystemRecord<'static>>,
    crs_definitions: BTreeMap<u32, CrsDefinitionRecord>,
    current_crs_codes: BTreeSet<u32>,
    conversions: BTreeMap<u32, ConversionRecord<'static>>,
    current_operations: BTreeMap<OperationCode<'static>, OperationRecord<'static>>,
    grid_alternatives: BTreeMap<&'static str, GridAlternativeRecord<'static>>,
    supersessions: Vec<SupersessionRecord>,
}

fn load_epsg_tables(connection: &Connection, extents: &BTreeMap<u32, ExtentRecord>) -> EpsgTables {
    let usages = |table: &str| load_usages(connection, table, EPSG, extents);
    let proj_usages = |table: &str| load_usages(connection, table, PROJ, extents);

    let mut crs_definitions = BTreeMap::new();
    let mut current_crs_codes = BTreeSet::new();
    let crs_rows = load_geodetic_crs(connection, &usages("geodetic_crs"))
        .into_iter()
        .chain(load_projected_crs(connection, &usages("projected_crs")));
    for crs_row in crs_rows {
        let code = crs_row.definition.code;
        if !crs_row.deprecated {
            current_crs_codes.insert(code);
        }
        insert_unique(&mut crs_definitions, code, crs_row.definition, "CRS");
    }

    let mut current_operations = BTreeMap::new();
    // proj.db's own grid and other transformations between epsg crs are all
    // vertical or use a PROJ string method, which the registry does not hold
    let operations = [
        load_helmert_transformations(connection, EPSG, &usages("helmert_transformation")),
        load_helmert_transformations(connection, PROJ, &proj_usages("helmert_transformation")),
        load_grid_transformations(connection, &usages("grid_transformation")),
        load_other_transformations(connection, &usages("other_transformation")),
        load_concatenated_operations(connection, EPSG, &usages("concatenated_operation")),
        load_concatenated_operations(connection, PROJ, &proj_usages("concatenated_operation")),
    ];
    for operation in operations.into_iter().flatten() {
        insert_unique(
            &mut current_operations,
            operation.code,
            operation,
            "coordinate operation",
        );
    }

    EpsgTables {
        units: load_units(connection),
        ellipsoids: load_ellipsoids(connection),
        prime_meridians: load_prime_meridians(connection),
        datums: load_datums(connection),
        coordinate_systems: load_coordinate_systems(connection),
        crs_definitions,
        current_crs_codes,
        conversions: load_conversions(connection),
        current_operations,
        grid_alternatives: load_grid_alternatives(connection),
        supersessions: load_supersessions(connection),
    }
}

const EPSG: &str = "EPSG";
const PROJ: &str = "PROJ";

// keyed by the object code as proj.db's text, since proj's own codes are not numbers
fn load_usages(
    connection: &Connection,
    table: &str,
    authority: &str,
    extents: &BTreeMap<u32, ExtentRecord>,
) -> BTreeMap<String, Vec<u32>> {
    // proj orders domains by scope score, then by usage code, which proj.db
    // leaves null, so ties keep row order
    let sql = format!(
        "SELECT CAST(usage.object_code AS TEXT), usage.extent_code
         FROM usage
         JOIN scope
           ON scope.auth_name = usage.scope_auth_name
          AND scope.code = usage.scope_code
         WHERE usage.object_table_name = {}
           AND usage.object_auth_name = {}
           AND usage.extent_auth_name = 'EPSG'
         ORDER BY usage.object_code,
                  CASE WHEN scope.scope LIKE '%large scale%' THEN 0 ELSE 1 END,
                  usage.rowid",
        quote_string(table),
        quote_string(authority)
    );
    let mut usages = BTreeMap::<String, Vec<u32>>::new();
    for_each_row(connection, &sql, |row| {
        let extent_code = column_code(row, 1);
        if extents.contains_key(&extent_code) {
            usages
                .entry(column_text(row, 0).to_string())
                .or_default()
                .push(extent_code);
        }
    });
    usages
}

fn extent_codes_of(usages: &BTreeMap<String, Vec<u32>>, code: &str) -> Vec<u32> {
    usages.get(code).cloned().unwrap_or_default()
}

const UNIT_KINDS_BY_NAME: [(&str, UnitKind); 4] = [
    ("length", UnitKind::Length),
    ("angle", UnitKind::Angle),
    ("scale", UnitKind::Scale),
    ("time", UnitKind::Time),
];

fn load_units(connection: &Connection) -> BTreeMap<u32, UnitRecord<'static>> {
    let mut units = BTreeMap::new();
    for_each_row(
        connection,
        "SELECT code, name, type, conv_factor
         FROM unit_of_measure
         WHERE auth_name = 'EPSG'",
        |row| {
            let unit = UnitRecord {
                code: column_code(row, 0),
                name: column_text(row, 1),
                kind: named(&UNIT_KINDS_BY_NAME, column_text(row, 2), "unit type"),
                to_si_factor: column_optional_number(row, 3),
            };
            insert_unique(&mut units, unit.code, unit, "unit of measure");
        },
    );
    units
}

fn load_ellipsoids(connection: &Connection) -> BTreeMap<u32, EllipsoidRecord<'static>> {
    let mut ellipsoids = BTreeMap::new();
    for_each_row(
        connection,
        "SELECT code, name, semi_major_axis, uom_auth_name, uom_code, inv_flattening,
                semi_minor_axis
         FROM ellipsoid
         WHERE auth_name = 'EPSG'",
        |row| {
            let code = column_code(row, 0);
            let shape = match (
                column_optional_number(row, 5),
                column_optional_number(row, 6),
            ) {
                (Some(inverse_flattening), None) => {
                    EllipsoidShape::InverseFlattening(inverse_flattening)
                }
                (None, Some(semi_minor_axis)) => EllipsoidShape::SemiMinorAxis(semi_minor_axis),
                _ => fatal(format!(
                    "EPSG ellipsoid {code} needs exactly one of inverse flattening and semi-minor axis"
                )),
            };
            let ellipsoid = EllipsoidRecord {
                code,
                name: column_text(row, 1),
                semi_major_axis: column_number(row, 2),
                unit_code: required_epsg_reference(row, 3),
                shape,
            };
            insert_unique(&mut ellipsoids, code, ellipsoid, "ellipsoid");
        },
    );
    ellipsoids
}

fn load_prime_meridians(connection: &Connection) -> BTreeMap<u32, PrimeMeridianRecord<'static>> {
    let mut prime_meridians = BTreeMap::new();
    for_each_row(
        connection,
        "SELECT code, name, longitude, uom_auth_name, uom_code
         FROM prime_meridian
         WHERE auth_name = 'EPSG'",
        |row| {
            let prime_meridian = PrimeMeridianRecord {
                code: column_code(row, 0),
                name: column_text(row, 1),
                longitude: column_number(row, 2),
                unit_code: required_epsg_reference(row, 3),
            };
            insert_unique(
                &mut prime_meridians,
                prime_meridian.code,
                prime_meridian,
                "prime meridian",
            );
        },
    );
    prime_meridians
}

fn load_datums(connection: &Connection) -> BTreeMap<u32, DatumRecord<'static>> {
    let mut members = BTreeMap::<u32, Vec<u32>>::new();
    for_each_row(
        connection,
        "SELECT ensemble_code, member_auth_name, member_code
         FROM geodetic_datum_ensemble_member
         WHERE ensemble_auth_name = 'EPSG'
         ORDER BY ensemble_code, sequence",
        |row| {
            members
                .entry(column_code(row, 0))
                .or_default()
                .push(required_epsg_reference(row, 1));
        },
    );

    let mut datums = BTreeMap::new();
    for_each_row(
        connection,
        "SELECT code, name, ellipsoid_auth_name, ellipsoid_code, prime_meridian_auth_name,
                prime_meridian_code, frame_reference_epoch, ensemble_accuracy, anchor_epoch
         FROM geodetic_datum
         WHERE auth_name = 'EPSG'",
        |row| {
            let code = column_code(row, 0);
            let ensemble = match (column_optional_number(row, 7), members.remove(&code)) {
                (Some(accuracy), Some(member_codes)) => Some(DatumEnsemble {
                    accuracy,
                    member_codes,
                }),
                (None, None) => None,
                _ => fatal(format!(
                    "EPSG datum {code} needs both an ensemble accuracy and ensemble members, or neither"
                )),
            };
            let datum = DatumRecord {
                code,
                name: column_text(row, 1),
                ellipsoid_code: required_epsg_reference(row, 2),
                prime_meridian_code: required_epsg_reference(row, 4),
                frame_reference_epoch: column_optional_number(row, 6),
                anchor_epoch: column_optional_number(row, 8),
                ensemble,
            };
            insert_unique(&mut datums, code, datum, "datum");
        },
    );
    datums
}

const COORDINATE_SYSTEM_KINDS_BY_NAME: [(&str, CoordinateSystemKind); 2] = [
    ("Cartesian", CoordinateSystemKind::Cartesian),
    ("ellipsoidal", CoordinateSystemKind::Ellipsoidal),
];

fn load_coordinate_systems(
    connection: &Connection,
) -> BTreeMap<u32, CoordinateSystemRecord<'static>> {
    let mut coordinate_systems = BTreeMap::<u32, CoordinateSystemRecord<'static>>::new();
    let mut dimensions = BTreeMap::new();
    for_each_row(
        connection,
        "SELECT coordinate_system.code, coordinate_system.type, coordinate_system.dimension,
                axis.coordinate_system_order, axis.name, axis.abbrev, axis.orientation,
                axis.uom_auth_name, axis.uom_code
         FROM coordinate_system
         JOIN axis
           ON axis.coordinate_system_auth_name = coordinate_system.auth_name
          AND axis.coordinate_system_code = coordinate_system.code
         WHERE coordinate_system.auth_name = 'EPSG'
           AND coordinate_system.type IN ('Cartesian', 'ellipsoidal')
         ORDER BY coordinate_system.code, axis.coordinate_system_order",
        |row| {
            let code = column_code(row, 0);
            dimensions.insert(code, column_usize(row, 2));
            let coordinate_system =
                coordinate_systems
                    .entry(code)
                    .or_insert_with(|| CoordinateSystemRecord {
                        code,
                        kind: named(
                            &COORDINATE_SYSTEM_KINDS_BY_NAME,
                            column_text(row, 1),
                            "coordinate system type",
                        ),
                        axes: Vec::new(),
                    });
            if column_usize(row, 3) != coordinate_system.axes.len() + 1 {
                fatal(format!("EPSG coordinate system {code} skips an axis order"));
            }
            coordinate_system.axes.push(Axis {
                name: column_text(row, 4),
                abbreviation: column_text(row, 5),
                orientation: column_text(row, 6),
                unit_code: required_epsg_reference(row, 7),
            });
        },
    );
    for (code, coordinate_system) in &coordinate_systems {
        if dimensions[code] != coordinate_system.axes.len() {
            fatal(format!(
                "EPSG coordinate system {code} has {} axes for dimension {}",
                coordinate_system.axes.len(),
                dimensions[code]
            ));
        }
    }
    coordinate_systems
}

struct CrsRow {
    definition: CrsDefinitionRecord,
    deprecated: bool,
}

const GEODETIC_CRS_TYPES_BY_NAME: [(&str, GeodeticCrsType); 3] = [
    ("geographic 2D", GeodeticCrsType::Geographic2D),
    ("geographic 3D", GeodeticCrsType::Geographic3D),
    ("geocentric", GeodeticCrsType::Geocentric),
];

fn load_geodetic_crs(connection: &Connection, usages: &BTreeMap<String, Vec<u32>>) -> Vec<CrsRow> {
    let mut rows = Vec::new();
    for_each_row(
        connection,
        "SELECT code, type, coordinate_system_auth_name, coordinate_system_code,
                datum_auth_name, datum_code, deprecated
         FROM geodetic_crs
         WHERE auth_name = 'EPSG'",
        |row| {
            let code = column_code(row, 0);
            let definition = CrsDefinitionRecord {
                code,
                kind: CrsDefinitionKind::Geodetic {
                    geodetic_type: named(
                        &GEODETIC_CRS_TYPES_BY_NAME,
                        column_text(row, 1),
                        "geodetic CRS type",
                    ),
                    datum_code: required_epsg_reference(row, 4),
                },
                coordinate_system_code: required_epsg_reference(row, 2),
                extent_codes: extent_codes_of(usages, &code.to_string()),
            };
            rows.push(CrsRow {
                definition,
                deprecated: column_flag(row, 6),
            });
        },
    );
    rows
}

fn load_projected_crs(connection: &Connection, usages: &BTreeMap<String, Vec<u32>>) -> Vec<CrsRow> {
    let mut rows = Vec::new();
    for_each_row(
        connection,
        "SELECT code, coordinate_system_auth_name, coordinate_system_code,
                geodetic_crs_auth_name, geodetic_crs_code, conversion_auth_name,
                conversion_code, deprecated
         FROM projected_crs
         WHERE auth_name = 'EPSG'",
        |row| {
            let code = column_code(row, 0);
            let definition = CrsDefinitionRecord {
                code,
                kind: CrsDefinitionKind::Projected {
                    base_crs_code: required_epsg_reference(row, 3),
                    conversion_code: required_epsg_reference(row, 5),
                },
                coordinate_system_code: required_epsg_reference(row, 1),
                extent_codes: extent_codes_of(usages, &code.to_string()),
            };
            rows.push(CrsRow {
                definition,
                deprecated: column_flag(row, 7),
            });
        },
    );
    rows
}

const PARAMETER_COLUMN_COUNT: usize = 6;
const CONVERSION_PARAMETER_SLOTS: usize = 7;
const GRID_TRANSFORMATION_PARAMETER_SLOTS: usize = 2;
const OTHER_TRANSFORMATION_PARAMETER_SLOTS: usize = 9;

fn parameter_columns(slot_count: usize) -> String {
    (1..=slot_count)
        .map(|slot| {
            format!(
                "param{slot}_auth_name, param{slot}_code, param{slot}_name, param{slot}_value, \
                 param{slot}_uom_auth_name, param{slot}_uom_code"
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn read_parameters(
    row: &Row<'_>,
    first_index: usize,
    slot_count: usize,
) -> Vec<ParameterValue<'static>> {
    (0..slot_count)
        .filter_map(|slot| {
            let index = first_index + slot * PARAMETER_COLUMN_COUNT;
            Some(ParameterValue {
                code: epsg_reference(row, index)?,
                name: column_text(row, index + 2),
                value: column_number(row, index + 3),
                unit_code: required_epsg_reference(row, index + 4),
            })
        })
        .collect()
}

fn read_method(row: &Row<'_>, authority_index: usize) -> OperationMethod<'static> {
    OperationMethod {
        code: required_epsg_reference(row, authority_index),
        name: column_text(row, authority_index + 2),
    }
}

fn load_conversions(connection: &Connection) -> BTreeMap<u32, ConversionRecord<'static>> {
    let sql = format!(
        "SELECT code, name, method_auth_name, method_code, method_name, {}
         FROM conversion
         WHERE auth_name = 'EPSG'",
        parameter_columns(CONVERSION_PARAMETER_SLOTS)
    );
    let mut conversions = BTreeMap::new();
    for_each_row(connection, &sql, |row| {
        let conversion = ConversionRecord {
            code: column_code(row, 0),
            name: column_text(row, 1),
            method: read_method(row, 2),
            parameters: read_parameters(row, 5, CONVERSION_PARAMETER_SLOTS),
        };
        insert_unique(&mut conversions, conversion.code, conversion, "conversion");
    });
    conversions
}

const OPERATION_COLUMNS: &str = "code, name, source_crs_auth_name, source_crs_code, \
     target_crs_auth_name, target_crs_code, accuracy";

// the registry keeps proj.db's own operations only between epsg crs
fn current_operations_of(authority: &str) -> String {
    let only_between_epsg_crs = if authority == EPSG {
        ""
    } else {
        " AND source_crs_auth_name = 'EPSG' AND target_crs_auth_name = 'EPSG'"
    };
    format!(
        "auth_name = {} AND deprecated = 0{only_between_epsg_crs}",
        quote_string(authority)
    )
}

fn operation_code(row: &Row<'_>, authority: &str) -> OperationCode<'static> {
    match authority {
        EPSG => OperationCode::Epsg(column_code(row, 0)),
        PROJ => OperationCode::Proj(column_text(row, 0)),
        other => fatal(format!("the registry holds no {other} operations")),
    }
}

fn read_operation(
    row: &Row<'_>,
    authority: &str,
    usages: &BTreeMap<String, Vec<u32>>,
    kind: OperationKind<'static>,
) -> OperationRecord<'static> {
    let code = operation_code(row, authority);
    OperationRecord {
        code,
        name: column_text(row, 1),
        source_crs_code: required_epsg_reference(row, 2),
        target_crs_code: required_epsg_reference(row, 4),
        accuracy: column_optional_number(row, 6),
        extent_codes: extent_codes_of(usages, &code.code_text()),
        kind,
    }
}

fn read_optional_vector(row: &Row<'_>, first_index: usize) -> Option<MeasuredVector> {
    let values = [
        column_optional_number(row, first_index),
        column_optional_number(row, first_index + 1),
        column_optional_number(row, first_index + 2),
    ];
    match (values, epsg_reference(row, first_index + 3)) {
        ([Some(x), Some(y), Some(z)], Some(unit_code)) => Some(MeasuredVector {
            values: [x, y, z],
            unit_code,
        }),
        ([None, None, None], None) => None,
        _ => fatal(format!(
            "proj.db {} is set for only some of its axes or has no unit",
            column_name(row, first_index)
        )),
    }
}

fn read_optional_measure(row: &Row<'_>, first_index: usize) -> Option<Measure> {
    match (
        column_optional_number(row, first_index),
        epsg_reference(row, first_index + 1),
    ) {
        (Some(value), Some(unit_code)) => Some(Measure { value, unit_code }),
        (None, None) => None,
        _ => fatal(format!(
            "proj.db {} and its unit are not both set",
            column_name(row, first_index)
        )),
    }
}

fn load_helmert_transformations(
    connection: &Connection,
    authority: &str,
    usages: &BTreeMap<String, Vec<u32>>,
) -> Vec<OperationRecord<'static>> {
    let sql = format!(
        "SELECT {OPERATION_COLUMNS}, method_auth_name, method_code, method_name,
                tx, ty, tz, translation_uom_auth_name, translation_uom_code,
                rx, ry, rz, rotation_uom_auth_name, rotation_uom_code,
                scale_difference, scale_difference_uom_auth_name, scale_difference_uom_code,
                rate_tx, rate_ty, rate_tz, rate_translation_uom_auth_name,
                rate_translation_uom_code,
                rate_rx, rate_ry, rate_rz, rate_rotation_uom_auth_name, rate_rotation_uom_code,
                rate_scale_difference, rate_scale_difference_uom_auth_name,
                rate_scale_difference_uom_code,
                epoch, epoch_uom_auth_name, epoch_uom_code,
                px, py, pz, pivot_uom_auth_name, pivot_uom_code
         FROM helmert_transformation
         WHERE {}",
        current_operations_of(authority)
    );
    let mut operations = Vec::new();
    for_each_row(connection, &sql, |row| {
        let translation = read_optional_vector(row, 10).unwrap_or_else(|| {
            fatal(format!(
                "Helmert transformation {} has no translation",
                operation_code(row, authority)
            ))
        });
        let parameters = HelmertParameters {
            method: read_method(row, 7),
            translation,
            rotation: read_optional_vector(row, 15),
            scale_difference: read_optional_measure(row, 20),
            rate_translation: read_optional_vector(row, 23),
            rate_rotation: read_optional_vector(row, 28),
            rate_scale_difference: read_optional_measure(row, 33),
            epoch: read_optional_measure(row, 36),
            pivot: read_optional_vector(row, 39),
        };
        operations.push(read_operation(
            row,
            authority,
            usages,
            OperationKind::HelmertTransformation(Box::new(parameters)),
        ));
    });
    operations
}

fn read_optional_grid(row: &Row<'_>, first_index: usize) -> Option<GridFileReference<'static>> {
    Some(GridFileReference {
        parameter_code: epsg_reference(row, first_index)?,
        parameter_name: column_text(row, first_index + 2),
        file_name: column_text(row, first_index + 3),
    })
}

fn load_grid_transformations(
    connection: &Connection,
    usages: &BTreeMap<String, Vec<u32>>,
) -> Vec<OperationRecord<'static>> {
    let sql = format!(
        "SELECT {OPERATION_COLUMNS}, method_auth_name, method_code, method_name,
                grid_param_auth_name, grid_param_code, grid_param_name, grid_name,
                grid2_param_auth_name, grid2_param_code, grid2_param_name, grid2_name,
                interpolation_crs_auth_name, interpolation_crs_code, {}
         FROM grid_transformation
         WHERE auth_name = 'EPSG' AND deprecated = 0",
        parameter_columns(GRID_TRANSFORMATION_PARAMETER_SLOTS)
    );
    let mut operations = Vec::new();
    for_each_row(connection, &sql, |row| {
        let parameters = TransformationParameters {
            method: read_method(row, 7),
            parameters: read_parameters(row, 20, GRID_TRANSFORMATION_PARAMETER_SLOTS),
            grids: [read_optional_grid(row, 10), read_optional_grid(row, 14)]
                .into_iter()
                .flatten()
                .collect(),
            interpolation_crs_code: epsg_reference(row, 18),
        };
        operations.push(read_operation(
            row,
            EPSG,
            usages,
            OperationKind::GridTransformation(parameters),
        ));
    });
    operations
}

fn load_other_transformations(
    connection: &Connection,
    usages: &BTreeMap<String, Vec<u32>>,
) -> Vec<OperationRecord<'static>> {
    let sql = format!(
        "SELECT {OPERATION_COLUMNS}, method_auth_name, method_code, method_name,
                grid_param_auth_name, grid_param_code, grid_param_name, grid_name,
                interpolation_crs_auth_name, interpolation_crs_code, {}
         FROM other_transformation
         WHERE auth_name = 'EPSG' AND deprecated = 0",
        parameter_columns(OTHER_TRANSFORMATION_PARAMETER_SLOTS)
    );
    let mut operations = Vec::new();
    for_each_row(connection, &sql, |row| {
        let parameters = TransformationParameters {
            method: read_method(row, 7),
            parameters: read_parameters(row, 16, OTHER_TRANSFORMATION_PARAMETER_SLOTS),
            grids: read_optional_grid(row, 10).into_iter().collect(),
            interpolation_crs_code: epsg_reference(row, 14),
        };
        operations.push(read_operation(
            row,
            EPSG,
            usages,
            OperationKind::OtherTransformation(parameters),
        ));
    });
    operations
}

fn load_concatenated_operations(
    connection: &Connection,
    authority: &str,
    usages: &BTreeMap<String, Vec<u32>>,
) -> Vec<OperationRecord<'static>> {
    let mut steps_by_operation = BTreeMap::<String, Vec<OperationStep<'static>>>::new();
    let steps_sql = format!(
        "SELECT CAST(step.operation_code AS TEXT), step.step_auth_name,
                CAST(step.step_code AS TEXT), step.step_direction, step_object.type
         FROM concatenated_operation_step step
         LEFT JOIN coordinate_operation_with_conversion_view step_object
           ON step_object.auth_name = step.step_auth_name
          AND step_object.code = step.step_code
         WHERE step.operation_auth_name = {}
         ORDER BY step.operation_code, step.step_number",
        quote_string(authority)
    );
    for_each_row(connection, &steps_sql, |row| {
        let operation_code = column_text(row, 0);
        let step_authority = column_text(row, 1);
        let step_code = column_text(row, 2);
        let direction = match column_optional_text(row, 3) {
            None => None,
            Some("forward") => Some(StepDirection::Forward),
            Some("reverse") => Some(StepDirection::Reverse),
            Some(other) => fatal(format!(
                "{authority} concatenated operation {operation_code} gives step {step_authority}:{step_code} the direction {other:?}"
            )),
        };
        let reference = match (column_optional_text(row, 4), step_authority) {
            (Some("conversion"), EPSG) => {
                StepReference::Conversion(parse_code(step_code, "conversion"))
            }
            (Some(_), EPSG) => {
                StepReference::Operation(OperationCode::Epsg(parse_code(step_code, "operation")))
            }
            (Some(step_type), PROJ) if step_type != "conversion" => {
                StepReference::Operation(OperationCode::Proj(step_code))
            }
            (Some(_), _) => fatal(format!(
                "{authority} concatenated operation {operation_code} names step {step_authority}:{step_code}, which the registry cannot hold"
            )),
            (None, _) => fatal(format!(
                "{authority} concatenated operation {operation_code} names step {step_authority}:{step_code}, which proj.db does not have"
            )),
        };
        steps_by_operation
            .entry(operation_code.to_string())
            .or_default()
            .push(OperationStep {
                reference,
                direction,
            });
    });

    let sql = format!(
        "SELECT {OPERATION_COLUMNS}
         FROM concatenated_operation
         WHERE {}",
        current_operations_of(authority)
    );
    let mut operations = Vec::new();
    for_each_row(connection, &sql, |row| {
        let code = operation_code(row, authority);
        let steps = steps_by_operation
            .remove(&code.code_text())
            .unwrap_or_else(|| fatal(format!("concatenated operation {code} has no steps")));
        operations.push(read_operation(
            row,
            authority,
            usages,
            OperationKind::ConcatenatedOperation(steps),
        ));
    });
    operations
}

fn load_grid_alternatives(
    connection: &Connection,
) -> BTreeMap<&'static str, GridAlternativeRecord<'static>> {
    let mut alternatives = BTreeMap::new();
    for_each_row(
        connection,
        "SELECT original_grid_name, proj_grid_name, old_proj_grid_name, proj_grid_format,
                proj_method, inverse_direction, url, open_license, direct_download
         FROM grid_alternatives",
        |row| {
            let alternative = GridAlternativeRecord {
                original_name: column_text(row, 0),
                // proj.db stores an empty name for grids the cdn does not carry
                proj_grid_name: Some(column_text(row, 1)).filter(|name| !name.is_empty()),
                old_proj_grid_name: column_optional_text(row, 2),
                format: column_text(row, 3),
                proj_method: column_text(row, 4),
                inverse_direction: column_flag(row, 5),
                url: column_optional_text(row, 6),
                open_license: column_optional_flag(row, 7) == Some(true),
                direct_download: column_optional_flag(row, 8) == Some(true),
            };
            insert_unique(
                &mut alternatives,
                alternative.original_name,
                alternative,
                "grid alternative",
            );
        },
    );
    alternatives
}

fn load_supersessions(connection: &Connection) -> Vec<SupersessionRecord> {
    let mut supersessions = Vec::new();
    for_each_row(
        connection,
        "SELECT superseded_code, replacement_code, same_source_target_crs
         FROM supersession
         WHERE superseded_auth_name = 'EPSG' AND replacement_auth_name = 'EPSG'",
        |row| {
            supersessions.push(SupersessionRecord {
                superseded_code: column_code(row, 0),
                replacement_code: column_code(row, 1),
                same_source_target_crs: column_flag(row, 2),
            });
        },
    );
    supersessions.sort_by_key(|supersession| {
        (
            supersession.superseded_code,
            supersession.replacement_code,
            supersession.same_source_target_crs,
        )
    });
    supersessions.dedup();
    supersessions
}

struct Selection<'t> {
    units: Vec<&'t UnitRecord<'static>>,
    ellipsoids: Vec<&'t EllipsoidRecord<'static>>,
    prime_meridians: Vec<&'t PrimeMeridianRecord<'static>>,
    datums: Vec<&'t DatumRecord<'static>>,
    coordinate_systems: Vec<&'t CoordinateSystemRecord<'static>>,
    crs_definitions: Vec<&'t CrsDefinitionRecord>,
    conversions: Vec<&'t ConversionRecord<'static>>,
    operations: Vec<&'t OperationRecord<'static>>,
    grid_alternatives: Vec<&'t GridAlternativeRecord<'static>>,
    supersessions: Vec<&'t SupersessionRecord>,
    extent_codes: BTreeSet<u32>,
}

fn select_reachable(tables: &EpsgTables) -> Selection<'_> {
    let is_geodetic = |code: u32| {
        matches!(
            tables
                .crs_definitions
                .get(&code)
                .map(|definition| definition.kind),
            Some(CrsDefinitionKind::Geodetic { .. })
        )
    };
    let steps_resolve = |operation: &OperationRecord<'static>| match &operation.kind {
        OperationKind::ConcatenatedOperation(steps) => {
            steps.iter().all(|step| match step.reference {
                StepReference::Operation(code) => tables.current_operations.contains_key(&code),
                StepReference::Conversion(code) => tables.conversions.contains_key(&code),
            })
        }
        _ => true,
    };
    let mut pending = Vec::new();
    for operation in tables.current_operations.values() {
        if !is_geodetic(operation.source_crs_code) || !is_geodetic(operation.target_crs_code) {
            continue;
        }
        if matches!(operation.code, OperationCode::Proj(_)) && !steps_resolve(operation) {
            eprintln!(
                "leaving out {}: a step is a proj.db operation the registry does not hold",
                operation.code
            );
            continue;
        }
        pending.push(operation.code);
    }
    let mut operation_codes = BTreeSet::new();
    let mut conversion_codes = BTreeSet::new();
    while let Some(code) = pending.pop() {
        if !operation_codes.insert(code) {
            continue;
        }
        let operation = lookup(
            &tables.current_operations,
            code,
            "current coordinate operation",
        );
        if let OperationKind::ConcatenatedOperation(steps) = &operation.kind {
            for step in steps {
                match step.reference {
                    StepReference::Operation(step_code) => pending.push(step_code),
                    StepReference::Conversion(step_code) => {
                        conversion_codes.insert(step_code);
                    }
                }
            }
        }
    }
    let operations = lookup_all(
        &tables.current_operations,
        &operation_codes,
        "current coordinate operation",
    );

    let mut crs_codes = tables.current_crs_codes.clone();
    for operation in &operations {
        crs_codes.extend([operation.source_crs_code, operation.target_crs_code]);
        crs_codes.extend(
            transformation_parameters(operation)
                .and_then(|parameters| parameters.interpolation_crs_code),
        );
    }
    let mut base_crs_codes = Vec::new();
    for code in &crs_codes {
        if let CrsDefinitionKind::Projected {
            base_crs_code,
            conversion_code,
        } = lookup(&tables.crs_definitions, *code, "CRS").kind
        {
            base_crs_codes.push(base_crs_code);
            conversion_codes.insert(conversion_code);
        }
    }
    crs_codes.extend(base_crs_codes);
    let crs_definitions = lookup_all(&tables.crs_definitions, &crs_codes, "CRS");

    let mut datum_codes: BTreeSet<u32> = crs_definitions
        .iter()
        .filter_map(|definition| match definition.kind {
            CrsDefinitionKind::Geodetic { datum_code, .. } => Some(datum_code),
            CrsDefinitionKind::Projected { .. } => None,
        })
        .collect();
    let member_codes: Vec<u32> = datum_codes
        .iter()
        .filter_map(|code| lookup(&tables.datums, *code, "datum").ensemble.as_ref())
        .flat_map(|ensemble| ensemble.member_codes.iter().copied())
        .collect();
    datum_codes.extend(member_codes);
    let datums = lookup_all(&tables.datums, &datum_codes, "datum");
    let ellipsoids = lookup_all(
        &tables.ellipsoids,
        &datums.iter().map(|datum| datum.ellipsoid_code).collect(),
        "ellipsoid",
    );
    let prime_meridians = lookup_all(
        &tables.prime_meridians,
        &datums
            .iter()
            .map(|datum| datum.prime_meridian_code)
            .collect(),
        "prime meridian",
    );
    let coordinate_systems = lookup_all(
        &tables.coordinate_systems,
        &crs_definitions
            .iter()
            .map(|definition| definition.coordinate_system_code)
            .collect(),
        "coordinate system",
    );
    let conversions = lookup_all(&tables.conversions, &conversion_codes, "conversion");

    let mut unit_codes = BTreeSet::new();
    unit_codes.extend(ellipsoids.iter().map(|ellipsoid| ellipsoid.unit_code));
    unit_codes.extend(prime_meridians.iter().map(|meridian| meridian.unit_code));
    unit_codes.extend(
        coordinate_systems
            .iter()
            .flat_map(|system| system.axes.iter().map(|axis| axis.unit_code)),
    );
    unit_codes.extend(conversions.iter().flat_map(|conversion| {
        conversion
            .parameters
            .iter()
            .map(|parameter| parameter.unit_code)
    }));
    unit_codes.extend(
        operations
            .iter()
            .flat_map(|operation| operation_unit_codes(operation)),
    );
    let units = lookup_all(&tables.units, &unit_codes, "unit of measure");

    let grid_file_names: BTreeSet<&str> = operations
        .iter()
        .filter_map(|operation| transformation_parameters(operation))
        .flat_map(|parameters| parameters.grids.iter().map(|grid| grid.file_name))
        .collect();
    let grid_alternatives = grid_file_names
        .into_iter()
        .filter_map(|name| tables.grid_alternatives.get(name))
        .collect();

    let extent_codes = crs_definitions
        .iter()
        .flat_map(|definition| definition.extent_codes.iter().copied())
        .chain(
            operations
                .iter()
                .flat_map(|operation| operation.extent_codes.iter().copied()),
        )
        .collect();

    let supersessions = tables
        .supersessions
        .iter()
        .filter(|supersession| {
            operation_codes.contains(&OperationCode::Epsg(supersession.superseded_code))
        })
        .collect();

    Selection {
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
        extent_codes,
    }
}

fn transformation_parameters<'r>(
    operation: &'r OperationRecord<'static>,
) -> Option<&'r TransformationParameters<'static>> {
    match &operation.kind {
        OperationKind::GridTransformation(parameters)
        | OperationKind::OtherTransformation(parameters) => Some(parameters),
        OperationKind::HelmertTransformation(_) | OperationKind::ConcatenatedOperation(_) => None,
    }
}

fn operation_unit_codes(operation: &OperationRecord<'static>) -> Vec<u32> {
    match &operation.kind {
        OperationKind::HelmertTransformation(helmert) => {
            let vectors = [
                Some(helmert.translation),
                helmert.rotation,
                helmert.rate_translation,
                helmert.rate_rotation,
                helmert.pivot,
            ];
            let measures = [
                helmert.scale_difference,
                helmert.rate_scale_difference,
                helmert.epoch,
            ];
            vectors
                .iter()
                .flatten()
                .map(|vector| vector.unit_code)
                .chain(measures.iter().flatten().map(|measure| measure.unit_code))
                .collect()
        }
        OperationKind::GridTransformation(parameters)
        | OperationKind::OtherTransformation(parameters) => parameters
            .parameters
            .iter()
            .map(|parameter| parameter.unit_code)
            .collect(),
        OperationKind::ConcatenatedOperation(_) => Vec::new(),
    }
}

fn lookup<'t, K: Ord + Copy + std::fmt::Display, T>(
    records: &'t BTreeMap<K, T>,
    key: K,
    kind: &str,
) -> &'t T {
    records.get(&key).unwrap_or_else(|| {
        fatal(format!(
            "EPSG {kind} {key} is referenced from the registry's scope but proj.db has none"
        ))
    })
}

fn lookup_all<'t, K: Ord + Copy + std::fmt::Display, T>(
    records: &'t BTreeMap<K, T>,
    codes: &BTreeSet<K>,
    kind: &str,
) -> Vec<&'t T> {
    codes
        .iter()
        .map(|code| lookup(records, *code, kind))
        .collect()
}

fn insert_unique<K: Ord + Copy + std::fmt::Display, T>(
    records: &mut BTreeMap<K, T>,
    key: K,
    record: T,
    kind: &str,
) {
    if records.insert(key, record).is_some() {
        fatal(format!("EPSG {kind} {key} appears twice in proj.db"));
    }
}

fn named<T: Copy>(values_by_name: &[(&str, T)], name: &str, kind: &str) -> T {
    values_by_name
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, value)| *value)
        .unwrap_or_else(|| fatal(format!("unknown {kind} {name:?} in proj.db")))
}

fn encode_each<T>(records: &[&T], mut encode: impl FnMut(&T, &mut Vec<u8>)) -> Vec<u8> {
    let mut payload = Vec::new();
    for record in records {
        encode(record, &mut payload);
    }
    payload
}

fn encode_string_referencing_sections(
    selection: &Selection<'_>,
    strings: &mut impl FnMut(&'static str) -> u32,
) -> Vec<Section> {
    vec![
        Section {
            tag: SECTION_UNIT_OF_MEASURE,
            record_count: selection.units.len(),
            payload: encode_each(&selection.units, |record, out| record.encode(out, strings)),
        },
        Section {
            tag: SECTION_ELLIPSOID,
            record_count: selection.ellipsoids.len(),
            payload: encode_each(&selection.ellipsoids, |record, out| {
                record.encode(out, strings)
            }),
        },
        Section {
            tag: SECTION_PRIME_MERIDIAN,
            record_count: selection.prime_meridians.len(),
            payload: encode_each(&selection.prime_meridians, |record, out| {
                record.encode(out, strings)
            }),
        },
        Section {
            tag: SECTION_DATUM,
            record_count: selection.datums.len(),
            payload: encode_each(&selection.datums, |record, out| record.encode(out, strings)),
        },
        Section {
            tag: SECTION_COORDINATE_SYSTEM,
            record_count: selection.coordinate_systems.len(),
            payload: encode_each(&selection.coordinate_systems, |record, out| {
                record.encode(out, strings)
            }),
        },
        Section {
            tag: SECTION_CONVERSION,
            record_count: selection.conversions.len(),
            payload: encode_each(&selection.conversions, |record, out| {
                record.encode(out, strings)
            }),
        },
        Section {
            tag: SECTION_COORDINATE_OPERATION,
            record_count: selection.operations.len(),
            payload: encode_each(&selection.operations, |record, out| {
                record.encode(out, strings)
            }),
        },
        Section {
            tag: SECTION_GRID_RESOURCE,
            record_count: selection.grid_alternatives.len(),
            payload: encode_each(&selection.grid_alternatives, |record, out| {
                record.encode(out, strings)
            }),
        },
    ]
}

fn gzip_len(blob: &[u8]) -> usize {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder
        .write_all(blob)
        .unwrap_or_else(|error| fatal(format!("gzip failed: {error}")));
    encoder
        .finish()
        .unwrap_or_else(|error| fatal(format!("gzip failed: {error}")))
        .len()
}

// ---------------------------------------------------------------------------
// proj.db columns
// ---------------------------------------------------------------------------

fn for_each_row(connection: &Connection, sql: &str, mut visit: impl FnMut(&Row<'_>)) {
    let mut statement = prepare(connection, sql);
    let mut rows = statement
        .query([])
        .unwrap_or_else(|error| fatal(format!("failed to run `{sql}`: {error}")));
    while let Some(row) = rows
        .next()
        .unwrap_or_else(|error| fatal(format!("failed to read `{sql}`: {error}")))
    {
        visit(row);
    }
}

fn column_name(row: &Row<'_>, index: usize) -> String {
    row.as_ref()
        .column_name(index)
        .map_or_else(|_| format!("column {index}"), str::to_string)
}

fn column_value<'r>(row: &'r Row<'_>, index: usize) -> ValueRef<'r> {
    row.get_ref(index).unwrap_or_else(|error| {
        fatal(format!(
            "failed to read {}: {error}",
            column_name(row, index)
        ))
    })
}

fn required<T>(value: Option<T>, row: &Row<'_>, index: usize) -> T {
    value.unwrap_or_else(|| fatal(format!("proj.db {} is null", column_name(row, index))))
}

fn column_optional_text(row: &Row<'_>, index: usize) -> Option<&'static str> {
    match column_value(row, index) {
        ValueRef::Null => None,
        ValueRef::Text(bytes) => {
            let text: &'static str = String::from_utf8(bytes.to_vec())
                .unwrap_or_else(|_| {
                    fatal(format!("proj.db {} is not UTF-8", column_name(row, index)))
                })
                .leak();
            Some(text)
        }
        _ => fatal(format!("proj.db {} is not text", column_name(row, index))),
    }
}

fn column_text(row: &Row<'_>, index: usize) -> &'static str {
    required(column_optional_text(row, index), row, index)
}

fn column_optional_number(row: &Row<'_>, index: usize) -> Option<f64> {
    match column_value(row, index) {
        ValueRef::Null => None,
        ValueRef::Integer(value) => Some(canonical_f64(value as f64)),
        ValueRef::Real(value) => Some(canonical_f64(value)),
        _ => fatal(format!(
            "proj.db {} is not a number",
            column_name(row, index)
        )),
    }
}

fn column_number(row: &Row<'_>, index: usize) -> f64 {
    required(column_optional_number(row, index), row, index)
}

fn column_optional_code(row: &Row<'_>, index: usize) -> Option<u32> {
    match column_value(row, index) {
        ValueRef::Null => None,
        ValueRef::Integer(value) => Some(
            u32::try_from(value)
                .ok()
                .filter(|code| *code != NO_CODE)
                .unwrap_or_else(|| {
                    fatal(format!(
                        "proj.db {} holds {value}, which is not an EPSG code",
                        column_name(row, index)
                    ))
                }),
        ),
        ValueRef::Text(bytes) => {
            let text = std::str::from_utf8(bytes).unwrap_or_else(|_| {
                fatal(format!("proj.db {} is not UTF-8", column_name(row, index)))
            });
            Some(parse_code(text, &column_name(row, index)))
        }
        _ => fatal(format!("proj.db {} is not a code", column_name(row, index))),
    }
}

fn column_code(row: &Row<'_>, index: usize) -> u32 {
    required(column_optional_code(row, index), row, index)
}

fn column_usize(row: &Row<'_>, index: usize) -> usize {
    match column_value(row, index) {
        ValueRef::Integer(value) => usize::try_from(value).unwrap_or_else(|_| {
            fatal(format!(
                "proj.db {} holds {value}, which is not a count",
                column_name(row, index)
            ))
        }),
        _ => fatal(format!(
            "proj.db {} is not an integer",
            column_name(row, index)
        )),
    }
}

fn column_optional_flag(row: &Row<'_>, index: usize) -> Option<bool> {
    match column_value(row, index) {
        ValueRef::Null => None,
        ValueRef::Integer(0) => Some(false),
        ValueRef::Integer(1) => Some(true),
        _ => fatal(format!("proj.db {} is not 0 or 1", column_name(row, index))),
    }
}

fn column_flag(row: &Row<'_>, index: usize) -> bool {
    required(column_optional_flag(row, index), row, index)
}

fn epsg_reference(row: &Row<'_>, authority_index: usize) -> Option<u32> {
    let is_epsg = match column_value(row, authority_index) {
        ValueRef::Null => None,
        ValueRef::Text(authority) => Some(authority == b"EPSG"),
        _ => fatal(format!(
            "proj.db {} is not text",
            column_name(row, authority_index)
        )),
    };
    match (is_epsg, column_optional_code(row, authority_index + 1)) {
        (None, None) => None,
        (Some(true), Some(code)) => Some(code),
        _ => fatal(format!(
            "proj.db {} is not an EPSG reference",
            column_name(row, authority_index)
        )),
    }
}

fn required_epsg_reference(row: &Row<'_>, authority_index: usize) -> u32 {
    required(epsg_reference(row, authority_index), row, authority_index)
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
    strings: usize,
    units_of_measure: usize,
    ellipsoids: usize,
    prime_meridians: usize,
    datums: usize,
    coordinate_systems: usize,
    crs_definitions: usize,
    conversions: usize,
    coordinate_operations: usize,
    grid_alternatives: usize,
    supersessions: usize,
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
            "{} does not match the pinned proj.db\n  expected {} ({} bytes)\n  found    {} ({} bytes)\nregenerate with: cargo run --manifest-path tools/gen-epsg-registry/Cargo.toml -- --proj-db PATH",
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
        assert_eq!(canonical_f64(1.0 / 3.0), 0.333333333333333);
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
    fn args_default_to_writing_and_require_a_proj_db() {
        let proj_db = || ["--proj-db".to_string(), "proj.db".to_string()];

        let args = Args::parse_from(proj_db()).unwrap();
        assert!(args.mode == Mode::Write);
        assert_eq!(args.proj_db, PathBuf::from("proj.db"));

        let args =
            Args::parse_from(std::iter::once("--check".to_string()).chain(proj_db())).unwrap();
        assert!(args.mode == Mode::Check);

        assert!(Args::parse_from(Vec::new()).is_err());
        assert!(Args::parse_from(["--check".to_string()]).is_err());
        assert!(Args::parse_from(["--proj-db".to_string()]).is_err());
        assert!(Args::parse_from(["--nope".to_string()]).is_err());
    }
}
