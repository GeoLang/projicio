use std::io::{Read, Write};

use projicio_core::iso19111::authority_factory::create_coordinate_reference_system;
use projicio_core::iso19111::{Crs, WktDialect, WktLayout};

// one record per CRS, every field ended by a nul byte: its EPSG code, projinfo's PROJJSON, then per dialect
// projinfo's WKT of EPSG:<code>, projinfo's WKT written from that PROJJSON, and projinfo's PROJJSON read from the first
const DIALECTS: [(&str, WktDialect); 4] = [
    ("WKT2_2019", WktDialect::Wkt2_2019),
    ("WKT2_2015", WktDialect::Wkt2_2015),
    ("WKT1_GDAL", WktDialect::Wkt1Gdal),
    ("WKT1_ESRI", WktDialect::Wkt1Esri),
];
const FIELDS_PER_RECORD: usize = 2 + 3 * DIALECTS.len();

fn one_line(text: &str) -> String {
    text.replace(['\n', '\t'], " ")
}

fn write_verdict(crs: Result<&Crs, String>, dialect: WktDialect, expected: &str) -> String {
    let layout = if dialect == WktDialect::Wkt1Esri {
        WktLayout::SingleLine
    } else {
        WktLayout::MultiLine
    };
    let written = crs.and_then(|crs| crs.to_wkt(dialect, layout).map_err(|e| e.to_string()));
    match (written, expected.trim_end_matches('\n')) {
        (Err(e), "") => format!("both refused ({})", one_line(&e)),
        (Err(e), _) => format!("refused: {}", one_line(&e)),
        (Ok(_), "") => "written where projinfo refused".to_string(),
        (Ok(text), expected) if text == expected => "identical".to_string(),
        (Ok(text), _) => format!("differs: {}", one_line(&text)),
    }
}

fn read_verdict(wkt: &str, projjson_of_wkt: &str) -> String {
    if wkt.is_empty() {
        return "no input".to_string();
    }
    let read = Crs::from_wkt(wkt);
    if projjson_of_wkt.is_empty() {
        return match read {
            Err(e) => format!("both refused ({})", one_line(&e.to_string())),
            Ok(_) => "read where projinfo refused".to_string(),
        };
    }
    let read = match read {
        Ok(crs) => crs,
        Err(e) => return format!("refused: {}", one_line(&e.to_string())),
    };
    let expected = match Crs::from_projjson(projjson_of_wkt) {
        Ok(crs) => crs,
        Err(e) => return format!("refused: projinfo's PROJJSON: {}", one_line(&e.to_string())),
    };
    // PROJ's _exportToJSON writes measures with 15 significant digits and leaves out CRS::hasImplicitCS and the datum and ellipsoid identifiers inside an identified CRS
    let read = match Crs::from_projjson(&read.to_projjson()) {
        Ok(crs) => crs,
        Err(e) => return format!("refused: projicio's PROJJSON: {}", one_line(&e.to_string())),
    };
    if read == expected {
        "equal".to_string()
    } else {
        format!("differs: {}", first_difference(&read, &expected))
    }
}

fn first_difference(read: &Crs, expected: &Crs) -> String {
    let read = format!("{read:?}");
    let expected = format!("{expected:?}");
    let start = read
        .char_indices()
        .zip(expected.chars())
        .find(|((_, a), b)| a != b)
        .map_or(read.len().min(expected.len()), |((index, _), _)| index);
    let context = |text: &str| {
        let mut from = start.saturating_sub(120).min(text.len());
        while !text.is_char_boundary(from) {
            from -= 1;
        }
        let mut to = (start + 120).min(text.len());
        while !text.is_char_boundary(to) {
            to += 1;
        }
        one_line(&text[from..to])
    };
    format!(
        "read [{}] expected [{}]",
        context(&read),
        context(&expected)
    )
}

fn main() -> std::io::Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let fields: Vec<&str> = input.split('\0').collect();
    let mut out = std::io::stdout().lock();
    for record in fields.chunks_exact(FIELDS_PER_RECORD) {
        let code: u32 = record[0].parse().expect("record starts with an EPSG code");
        let projjson = Crs::from_projjson(record[1]).map_err(|e| e.to_string());
        let from_registry = create_coordinate_reference_system(code).map_err(|e| e.to_string());
        for ((name, dialect), references) in DIALECTS.iter().zip(record[2..].chunks_exact(3)) {
            let (wkt_of_code, wkt_of_projjson, projjson_of_wkt) =
                (references[0], references[1], references[2]);
            // PROJJSON drops the datum identifiers WKT1 uses for AUTHORITY and ESRI names, and the registry has no scope or remarks for WKT2
            let (source, expected) = match dialect {
                WktDialect::Wkt1Gdal | WktDialect::Wkt1Esri => {
                    (from_registry.as_ref(), wkt_of_code)
                }
                WktDialect::Wkt2_2019 | WktDialect::Wkt2_2015 => {
                    (projjson.as_ref(), wkt_of_projjson)
                }
            };
            let written = write_verdict(source.map_err(Clone::clone), *dialect, expected);
            let read = read_verdict(wkt_of_code, projjson_of_wkt);
            writeln!(out, "{name}\t{written}\t{read}")?;
        }
    }
    out.flush()
}
