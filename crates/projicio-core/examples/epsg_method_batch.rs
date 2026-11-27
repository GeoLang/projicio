use std::io::{BufRead, BufWriter, Write};

use projicio_core::{Ellipsoid, Geographic, from_epsg_method};

fn main() -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let numbers: Vec<f64> = line
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        let [method, a, f, lon, lat, rest @ ..] = &numbers[..] else {
            panic!("want `method a f lon lat (code value)*`, got {line:?}");
        };
        let parameters: Vec<(u32, f64)> =
            rest.chunks_exact(2).map(|p| (p[0] as u32, p[1])).collect();
        let projected = from_epsg_method(*method as u32, &parameters, &Ellipsoid::new(*a, *f))
            .and_then(|projection| projection.forward(Geographic::new(*lon, *lat)));
        match projected {
            Ok(coord) => writeln!(out, "{:.12} {:.12}", coord.x, coord.y)?,
            Err(e) => writeln!(out, "error {}", e.to_string().replace('\n', " "))?,
        }
    }
    out.flush()
}
