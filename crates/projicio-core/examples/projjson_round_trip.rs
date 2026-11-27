use std::io::{Read, Write};

use projicio_core::iso19111::Crs;

fn main() -> std::io::Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let mut out = std::io::stdout().lock();
    // records are separated by nul bytes since projjson spans many lines
    for record in input.split('\0').filter(|r| !r.trim().is_empty()) {
        match Crs::from_projjson(record) {
            Ok(crs) => write!(out, "{}\0", crs.to_projjson())?,
            Err(e) => write!(out, "error {e}\0")?,
        }
    }
    out.flush()
}
