//! proj4rs fallback engine for EPSG codes projicio has no native projection for.
//!
//! Definitions come from the `crs-definitions` crate, which embeds a proj4 string
//! per EPSG code at compile time. Nothing is read from disk at runtime.
//!
//! proj4rs uses radians for geographic CRS while projicio uses degrees everywhere,
//! so angles are converted on the way in and out.

use crate::Error;
use crate::epsg::Support;
use proj4rs::Proj;

/// Largest EPSG code the embedded table can hold, since it is keyed by `u16`.
const MAX_TABLE_CODE: u32 = u16::MAX as u32;

/// What a caller asked to transform between.
pub enum Spec {
    /// An EPSG code, resolved through the embedded definition table.
    Epsg(u32),
    /// A proj4 projstring, used as given. This is how a caller names a datum shift
    /// grid the embedded definition does not mention, with `+nadgrids=`.
    Proj4(String),
}

impl Spec {
    /// The proj4 definition this spec resolves to.
    fn definition(&self) -> Result<&str, Error> {
        match self {
            Spec::Epsg(code) => {
                proj4_definition(*code).ok_or_else(|| Error::UnsupportedCrs(format!("EPSG:{code}")))
            }
            Spec::Proj4(s) => Ok(s),
        }
    }

    fn label(&self) -> String {
        match self {
            Spec::Epsg(code) => format!("EPSG:{code}"),
            Spec::Proj4(s) => s.clone(),
        }
    }
}

/// The proj4 definition string for an EPSG code, if the embedded table has one.
///
/// A string here does not guarantee a working transform: proj4rs implements a
/// subset of proj's projections, and some definitions need a datum shift grid that
/// has not been registered. `crate::epsg::support` reports which case applies.
pub fn proj4_definition(code: u32) -> Option<&'static str> {
    if code > MAX_TABLE_CODE {
        return None;
    }
    crs_definitions::from_code(code as u16).map(|def| def.proj4)
}

/// Build a proj4rs projection for a spec.
pub fn build(spec: &Spec) -> Result<Proj, Error> {
    let def = spec.definition()?;
    Proj::from_proj_string(def).map_err(|e| match e {
        // The definition names a grid and no registered grid answered to the name.
        proj4rs::errors::Error::NadGridNotAvailable => Error::GridError(format!(
            "{} needs a datum shift grid that is not registered, its definition is {def:?}",
            spec.label()
        )),
        e => Error::UnsupportedCrs(format!(
            "{} ({def}) is not supported by proj4rs: {e}",
            spec.label()
        )),
    })
}

/// Which engine, if any, can handle an EPSG code through the fallback path.
pub fn classify(code: u32) -> Support {
    match build(&Spec::Epsg(code)) {
        Ok(_) => Support::Fallback,
        Err(Error::GridError(_)) => Support::NeedsGrid,
        Err(_) => Support::Unsupported,
    }
}

/// A transform between two CRS resolved through proj4rs.
pub struct Proj4Transform {
    src: Proj,
    dst: Proj,
    src_is_latlong: bool,
    dst_is_latlong: bool,
}

impl Proj4Transform {
    pub fn new(from: &Spec, to: &Spec) -> Result<Self, Error> {
        let src = build(from)?;
        let dst = build(to)?;
        if !src.has_inverse() {
            return Err(Error::UnsupportedCrs(format!(
                "{} has no inverse projection",
                from.label()
            )));
        }
        if !dst.has_forward() {
            return Err(Error::UnsupportedCrs(format!(
                "{} has no forward projection",
                to.label()
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
        let err = build(&Spec::Epsg(65000)).unwrap_err();
        assert!(matches!(err, Error::UnsupportedCrs(_)));
    }

    #[test]
    fn test_build_proj4_string() {
        assert!(build(&Spec::Proj4("+proj=merc +ellps=WGS84".into())).is_ok());
        assert!(build(&Spec::Proj4("+proj=nonsuch".into())).is_err());
    }

    #[test]
    fn test_grid_backed_definition_reports_needs_grid() {
        // NAD27 resolves to a nadgrids list, so without a registered grid this must be
        // distinguishable from a projection projicio simply cannot do.
        let err = build(&Spec::Epsg(4267)).unwrap_err();
        assert!(matches!(err, Error::GridError(_)), "{err}");
    }
}
