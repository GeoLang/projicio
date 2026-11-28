use std::io::{BufRead, BufWriter, Write};
use std::path::Path;

use projicio_core::{
    Error, GeodeticCoord, grid_transformation_from_epsg_method, horizontal_shift_grid,
};

struct Case<'a> {
    inverse: bool,
    method_code: u32,
    grid_path: &'a Path,
    longitude: f64,
    latitude: f64,
    grid_files: Vec<(u32, &'a str)>,
}

fn number(text: &str) -> f64 {
    text.parse().unwrap()
}

fn parse(line: &str) -> Case<'_> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let [direction, method, grid_path, longitude, latitude, rest @ ..] = &fields[..] else {
        panic!(
            "want `forward|inverse method grid_path longitude latitude (parameter_code file_name)*`, got {line:?}"
        );
    };
    let inverse = match *direction {
        "forward" => false,
        "inverse" => true,
        other => panic!("unknown direction {other:?} in {line:?}"),
    };
    let grid_files = rest
        .chunks_exact(2)
        .map(|pair| (pair[0].parse().unwrap(), pair[1]))
        .collect();
    Case {
        inverse,
        method_code: method.parse().unwrap(),
        grid_path: Path::new(*grid_path),
        longitude: number(longitude),
        latitude: number(latitude),
        grid_files,
    }
}

fn run(case: &Case) -> Result<String, Error> {
    let name = case
        .grid_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap();
    if !horizontal_shift_grid::is_registered(name) {
        horizontal_shift_grid::register_file(name, case.grid_path)?;
    }
    let transformation = grid_transformation_from_epsg_method(case.method_code, &case.grid_files)?;
    let point = GeodeticCoord {
        longitude: case.longitude.to_radians(),
        latitude: case.latitude.to_radians(),
        height: 0.0,
    };
    let result = if case.inverse {
        transformation.inverse(point, None)?
    } else {
        transformation.forward(point, None)?
    };
    Ok(format!(
        "{:.12} {:.12}",
        result.longitude.to_degrees(),
        result.latitude.to_degrees()
    ))
}

fn main() -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        match run(&parse(&line)) {
            Ok(text) => writeln!(out, "{text}")?,
            Err(error) => writeln!(out, "error {}", error.to_string().replace('\n', " "))?,
        }
    }
    out.flush()
}
