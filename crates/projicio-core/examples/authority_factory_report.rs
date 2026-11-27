use std::io::{BufRead, Write};

use projicio_core::iso19111::authority_factory::{
    create_coordinate_operation, create_coordinate_reference_system,
};
use projicio_core::registry::OperationCode;

fn projjson(line: &str) -> Result<String, String> {
    let (kind, code) = line
        .split_once(' ')
        .ok_or_else(|| format!("{line:?} is not `<kind> <code>`"))?;
    match kind {
        "crs" => {
            let code = code
                .parse()
                .map_err(|_| format!("{code:?} is not an EPSG code"))?;
            create_coordinate_reference_system(code)
                .map(|crs| crs.to_projjson())
                .map_err(|error| error.to_string())
        }
        "operation" => {
            let code = match code.strip_prefix("PROJ:") {
                Some(proj_code) => OperationCode::Proj(proj_code),
                None => OperationCode::Epsg(
                    code.parse()
                        .map_err(|_| format!("{code:?} is not an EPSG code"))?,
                ),
            };
            create_coordinate_operation(code)
                .map(|operation| operation.to_projjson())
                .map_err(|error| error.to_string())
        }
        other => Err(format!("unknown kind {other:?}, expected crs or operation")),
    }
}

fn main() -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        // records end in a nul byte since projjson spans many lines
        match projjson(line.trim()) {
            Ok(projjson) => write!(out, "{projjson}\0")?,
            Err(message) => write!(out, "error {message}\0")?,
        }
    }
    out.flush()
}
