use std::collections::HashMap;
use std::io::{BufRead, BufWriter, Write};

use projicio_core::iso19111::Crs;
use projicio_core::iso19111::coordinate_system::{AxisDirection, CoordinateSystemAxis};
use projicio_core::operation_factory::CandidateOperations;

fn axes(crs: &Crs) -> &[CoordinateSystemAxis] {
    match crs {
        Crs::Geographic(geographic) => &geographic.coordinate_system.axes,
        Crs::Geodetic(geodetic) => &geodetic.coordinate_system.axes,
        Crs::Projected(projected) => &projected.coordinate_system.axes,
        Crs::Bound(bound) => axes(&bound.source_crs),
    }
}

fn northing_first(crs: &Crs) -> bool {
    // compare.py reads proj.db's orientation, where a polar axis such as "North along 90°E" is neither north nor south
    let first = &axes(crs)[0];
    matches!(first.direction, AxisDirection::North | AxisDirection::South)
        && first.meridian.is_none()
}

fn units_per_degree(crs: &Crs) -> f64 {
    match crs {
        Crs::Geographic(geographic) => {
            1.0_f64.to_radians() / geographic.coordinate_system.axes[0].unit.conversion_to_si
        }
        Crs::Geodetic(_) | Crs::Projected(_) | Crs::Bound(_) => 1.0,
    }
}

fn convert(operations: &CandidateOperations, x: f64, y: f64) -> Result<(f64, f64), String> {
    // transform_batch.rs takes and prints longitude before latitude in degrees, and easting before northing
    let source = operations.source_crs();
    let target = operations.target_crs();
    let (x, y) = (x * units_per_degree(source), y * units_per_degree(source));
    let native = if northing_first(source) {
        [y, x, 0.0]
    } else {
        [x, y, 0.0]
    };
    let [first, second, _] = operations.convert(native).map_err(|e| e.to_string())?;
    let (x, y) = if northing_first(target) {
        (second, first)
    } else {
        (first, second)
    };
    Ok((x / units_per_degree(target), y / units_per_degree(target)))
}

fn epsg_code(text: &str) -> Result<u32, String> {
    text.strip_prefix("EPSG:")
        .unwrap_or(text)
        .parse()
        .map_err(|_| format!("{text} is not an EPSG code"))
}

fn main() -> std::io::Result<()> {
    let mut built: HashMap<(String, String), Result<CandidateOperations, String>> = HashMap::new();
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [from, to, x, y] = fields[..] else {
            panic!("want `from to x y`, got {line:?}");
        };
        let (x, y): (f64, f64) = (x.parse().unwrap(), y.parse().unwrap());
        let operations = built
            .entry((from.to_string(), to.to_string()))
            .or_insert_with(|| {
                CandidateOperations::from_epsg(epsg_code(from)?, epsg_code(to)?)
                    .map_err(|e| e.to_string())
            });
        let converted = match operations {
            Ok(operations) => convert(operations, x, y),
            Err(message) => Err(message.clone()),
        };
        match converted {
            Ok((rx, ry)) => writeln!(out, "{rx:.12} {ry:.12}")?,
            Err(message) => writeln!(out, "error {}", message.replace('\n', " "))?,
        }
    }
    out.flush()
}
