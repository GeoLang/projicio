//! proj4rs fallback engine for EPSG codes projicio has no native projection for.
//!
//! Definitions come from the `crs-definitions` crate, which embeds a proj4 string
//! per EPSG code at compile time. Nothing is read from disk at runtime.
//!
//! proj4rs uses radians for geographic CRS while projicio uses degrees everywhere,
//! so angles are converted on the way in and out.

use crate::Error;
use proj4rs::Proj;

/// Largest EPSG code the embedded table can hold, since it is keyed by `u16`.
const MAX_TABLE_CODE: u32 = u16::MAX as u32;

/// The proj4 definition string for an EPSG code, if the embedded table has one.
///
/// A string here does not guarantee a working transform: proj4rs implements a
/// subset of proj's projections, so [`build`] can still fail.
pub fn proj4_definition(code: u32) -> Option<&'static str> {
    if code > MAX_TABLE_CODE {
        return None;
    }
    crs_definitions::from_code(code as u16).map(|def| def.proj4)
}

/// Build a proj4rs projection for an EPSG code.
pub fn build(code: u32) -> Result<Proj, Error> {
    let def =
        proj4_definition(code).ok_or_else(|| Error::UnsupportedCrs(format!("EPSG:{code}")))?;
    Proj::from_proj_string(def).map_err(|e| {
        Error::UnsupportedCrs(format!(
            "EPSG:{code} ({def}) is not supported by proj4rs: {e}"
        ))
    })
}

/// A transform between two EPSG codes resolved through the embedded proj4 table.
pub struct Proj4Transform {
    src: Proj,
    dst: Proj,
    src_is_latlong: bool,
    dst_is_latlong: bool,
}

impl Proj4Transform {
    pub fn new(from: u32, to: u32) -> Result<Self, Error> {
        let src = build(from)?;
        let dst = build(to)?;
        if !src.has_inverse() {
            return Err(Error::UnsupportedCrs(format!(
                "EPSG:{from} has no inverse projection"
            )));
        }
        if !dst.has_forward() {
            return Err(Error::UnsupportedCrs(format!(
                "EPSG:{to} has no forward projection"
            )));
        }
        Ok(Self {
            src_is_latlong: src.is_latlong(),
            dst_is_latlong: dst.is_latlong(),
            src,
            dst,
        })
    }

    pub fn convert(&self, x: f64, y: f64) -> Result<(f64, f64), Error> {
        let mut point = if self.src_is_latlong {
            (x.to_radians(), y.to_radians(), 0.0)
        } else {
            (x, y, 0.0)
        };

        proj4rs::transform::transform(&self.src, &self.dst, &mut point)
            .map_err(|e| Error::ProjectionError(e.to_string()))?;

        if self.dst_is_latlong {
            Ok((point.0.to_degrees(), point.1.to_degrees()))
        } else {
            Ok((point.0, point.1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Transform is documented as Send + Sync, so the fallback engine must not
    // silently take that away.
    #[test]
    fn test_proj_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Proj4Transform>();
    }

    #[test]
    fn test_definition_present() {
        assert!(proj4_definition(27700).unwrap().contains("+proj=tmerc"));
    }

    #[test]
    fn test_definition_out_of_table_range() {
        assert!(proj4_definition(900913).is_none());
        assert!(proj4_definition(99999).is_none());
    }

    #[test]
    fn test_build_unknown_code_errors() {
        let err = build(65000).unwrap_err();
        assert!(matches!(err, Error::UnsupportedCrs(_)));
    }
}
