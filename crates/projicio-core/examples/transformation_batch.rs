use std::io::{BufRead, BufWriter, Write};

use projicio_core::{
    Ellipsoid, Error, GeocentricCoord, GeodeticCoord, geocentric_transformation_from_epsg_method,
    transformation_from_epsg_method,
};

struct Case {
    coordinates: [f64; 3],
    epoch: Option<f64>,
    method_code: u32,
    source: Ellipsoid,
    target: Ellipsoid,
    parameters: Vec<(u32, f64)>,
}

fn number(text: &str) -> f64 {
    text.parse().unwrap()
}

fn parse(line: &str) -> (String, Case) {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let [
        kind,
        method,
        source_a,
        source_f,
        target_a,
        target_f,
        first,
        second,
        third,
        epoch,
        rest @ ..,
    ] = &fields[..]
    else {
        panic!(
            "want `geographic|geocentric method source_a source_f target_a target_f c1 c2 c3 epoch|- (code value)*`, got {line:?}"
        );
    };
    let epoch = match *epoch {
        "-" => None,
        text => Some(number(text)),
    };
    let parameters = rest
        .chunks_exact(2)
        .map(|pair| (pair[0].parse().unwrap(), number(pair[1])))
        .collect();
    let case = Case {
        coordinates: [number(first), number(second), number(third)],
        epoch,
        method_code: method.parse().unwrap(),
        source: Ellipsoid::new(number(source_a), number(source_f)),
        target: Ellipsoid::new(number(target_a), number(target_f)),
        parameters,
    };
    (kind.to_string(), case)
}

fn geographic(case: &Case) -> Result<String, Error> {
    let transformation = transformation_from_epsg_method(
        case.method_code,
        &case.parameters,
        &case.source,
        &case.target,
    )?;
    let [longitude, latitude, height] = case.coordinates;
    let result = transformation.forward(
        GeodeticCoord {
            longitude: longitude.to_radians(),
            latitude: latitude.to_radians(),
            height,
        },
        case.epoch,
    )?;
    Ok(format!(
        "{:.12} {:.12} {:.6}",
        result.longitude.to_degrees(),
        result.latitude.to_degrees(),
        result.height
    ))
}

fn geocentric(case: &Case) -> Result<String, Error> {
    let helmert = geocentric_transformation_from_epsg_method(case.method_code, &case.parameters)?;
    let [x, y, z] = case.coordinates;
    let result = helmert.forward(GeocentricCoord { x, y, z }, case.epoch)?;
    Ok(format!("{:.6} {:.6} {:.6}", result.x, result.y, result.z))
}

fn main() -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let (kind, case) = parse(&line);
        let result = match kind.as_str() {
            "geographic" => geographic(&case),
            "geocentric" => geocentric(&case),
            other => panic!("unknown coordinate kind {other:?} in {line:?}"),
        };
        match result {
            Ok(text) => writeln!(out, "{text}")?,
            Err(error) => writeln!(out, "error {}", error.to_string().replace('\n', " "))?,
        }
    }
    out.flush()
}
