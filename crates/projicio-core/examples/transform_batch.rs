use std::collections::HashMap;
use std::io::{BufRead, BufWriter, Write};

use projicio_core::Transform;

fn main() -> std::io::Result<()> {
    let mut transforms: HashMap<(String, String), Result<Transform, String>> = HashMap::new();
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [from, to, values @ ..] = &fields[..] else {
            panic!("want `from to c1 c2 [c3 [epoch]]`, got {line:?}");
        };
        let values: Vec<f64> = values.iter().map(|v| v.parse().unwrap()).collect();
        let (x, y) = match values[..] {
            [x, y] => (x, y),
            [_, _, _] => {
                writeln!(
                    out,
                    "error heights are not supported, Transform converts two coordinates"
                )?;
                continue;
            }
            [_, _, _, _] => {
                writeln!(
                    out,
                    "error coordinate epochs are not supported, Transform has no epoch"
                )?;
                continue;
            }
            _ => panic!("want `from to c1 c2 [c3 [epoch]]`, got {line:?}"),
        };
        let transform = transforms
            .entry((from.to_string(), to.to_string()))
            .or_insert_with(|| Transform::new(from, to).map_err(|e| e.to_string()));
        let converted = match transform {
            Ok(t) => t.convert(x, y).map_err(|e| e.to_string()),
            Err(message) => Err(message.clone()),
        };
        match converted {
            Ok((rx, ry)) => writeln!(out, "{rx:.12} {ry:.12}")?,
            Err(message) => writeln!(out, "error {}", message.replace('\n', " "))?,
        }
    }
    out.flush()
}
