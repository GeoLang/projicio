//! WebAssembly bindings for projicio coordinate transforms.
//!
//! One call covers the browser need: transform a flat `[x0, y0, x1, y1, ...]`
//! batch between two CRS, each named by an EPSG code, a proj4 projstring or a
//! WKT definition (the content of a `.prj` sidecar).
//!
//! Datum shift grids come in as bytes, since there is no file system to read them
//! from. Registration lasts as long as the wasm instance, so a page fetches a `.gsb`
//! once and every later transform sees it.

use projicio_core::{Transform, grids};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn transform_coordinates(
    from: &str,
    to: &str,
    coordinates: &[f64],
) -> Result<Vec<f64>, JsError> {
    if coordinates.len() % 2 != 0 {
        return Err(JsError::new("coordinates must be a flat list of x,y pairs"));
    }
    let transform = Transform::new(from, to).map_err(|e| JsError::new(&e.to_string()))?;
    let mut result = Vec::with_capacity(coordinates.len());
    for pair in coordinates.chunks_exact(2) {
        let (x, y) = transform
            .convert(pair[0], pair[1])
            .map_err(|e| JsError::new(&e.to_string()))?;
        result.push(x);
        result.push(y);
    }
    Ok(result)
}

/// Register an NTv2 grid, under the name definitions refer to it by.
///
/// The bytes are the contents of a `.gsb` file, parsed here so a bad one is reported
/// now rather than at the first transform. A name can be registered once.
#[wasm_bindgen]
pub fn register_grid(name: &str, bytes: &[u8]) -> Result<(), JsError> {
    grids::register_bytes(name, bytes.to_vec()).map_err(|e| JsError::new(&e.to_string()))
}

/// The names of every grid registered so far, sorted.
#[wasm_bindgen]
pub fn registered_grids() -> Vec<String> {
    grids::registered()
}

/// The grid names a CRS needs registering before it can transform, sorted.
///
/// Empty means registration is not what stands in the way. The spec is what
/// `transform_coordinates` takes on either side.
#[wasm_bindgen]
pub fn missing_grids(spec: &str) -> Vec<String> {
    grids::missing_grids(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_utm_to_lonlat() {
        let result =
            transform_coordinates("EPSG:32618", "EPSG:4326", &[585_000.0, 4_510_000.0]).unwrap();
        assert!((result[0] - (-73.994)).abs() < 0.01, "{}", result[0]);
        assert!((result[1] - 40.729).abs() < 0.01, "{}", result[1]);
    }
}
