use crate::epsg::{self, Support};
use crate::fallback::{Proj4Transform, Spec};
use crate::{Coord, Error, Geographic, projection::*};

type InverseFn = Box<dyn Fn(Coord) -> Result<Geographic, Error> + Send + Sync>;
type ForwardFn = Box<dyn Fn(Geographic) -> Result<Coord, Error> + Send + Sync>;

/// The engine doing the work for a given pair of codes.
enum Engine {
    /// projicio's own projection math, pivoting through WGS84 geographic.
    Native {
        source_to_geo: InverseFn,
        geo_to_target: ForwardFn,
    },
    /// proj4rs over the embedded proj4 definition table, which also carries the
    /// datum shift so there is no WGS84 pivot here.
    ///
    /// Boxed because a pair of proj4rs projections is over a kilobyte, which would
    /// otherwise set the size of every `Transform`.
    Fallback(Box<Proj4Transform>),
}

/// High-level transform between two CRS identified by EPSG codes.
pub struct Transform {
    engine: Engine,
}

impl Transform {
    /// Create a transform between two CRS.
    ///
    /// Each side is one of: an EPSG code, as `"EPSG:4326"` or `"4326"`; a proj4
    /// projstring starting with `+`; or a WKT CRS definition such as the content
    /// of a `.prj` sidecar. A projstring is the way to name a datum shift
    /// grid the embedded definition does not mention, with `+nadgrids=`.
    ///
    /// Both sides take projicio's native path when it covers them, otherwise the
    /// whole transform is handed to the embedded proj4 fallback so datum shifts
    /// stay consistent across the pair.
    pub fn new(from: &str, to: &str) -> Result<Self, Error> {
        let source = parse_spec(from)?;
        let target = parse_spec(to)?;

        let engine = match (&source, &target) {
            (Spec::Epsg(s), Spec::Epsg(t)) if epsg::is_native(*s) && epsg::is_native(*t) => {
                Engine::Native {
                    source_to_geo: make_inverse(*s)?,
                    geo_to_target: make_forward(*t)?,
                }
            }
            _ => Engine::Fallback(Box::new(Proj4Transform::new(&source, &target)?)),
        };

        Ok(Self { engine })
    }

    /// Which engine this transform resolved to.
    ///
    /// Never returns [`Support::Unsupported`]: an unsupported pair fails in [`Self::new`].
    pub fn path(&self) -> Support {
        match self.engine {
            Engine::Native { .. } => Support::Native,
            Engine::Fallback(_) => Support::Fallback,
        }
    }

    /// Transform a single coordinate.
    pub fn convert(&self, x: f64, y: f64) -> Result<(f64, f64), Error> {
        match &self.engine {
            Engine::Native {
                source_to_geo,
                geo_to_target,
            } => {
                let geo = source_to_geo(Coord::new(x, y))?;
                let result = geo_to_target(geo)?;
                Ok((result.x, result.y))
            }
            Engine::Fallback(t) => t.convert(x, y),
        }
    }

    /// Transform a batch of coordinates.
    pub fn convert_batch(&self, coords: &[(f64, f64)]) -> Result<Vec<(f64, f64)>, Error> {
        coords.iter().map(|&(x, y)| self.convert(x, y)).collect()
    }
}

impl std::fmt::Debug for Transform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transform")
            .field("path", &self.path())
            .finish()
    }
}

fn parse_spec(s: &str) -> Result<Spec, Error> {
    let trimmed = s.trim();
    if trimmed.starts_with('+') {
        Ok(Spec::Proj4(trimmed.to_string()))
    } else if is_wkt(trimmed) {
        // a WKT naming its EPSG code resolves by code, which keeps the native
        // path and the embedded datum handling; codeless WKT (typical for an
        // ESRI .prj) converts to a projstring for the fallback engine
        if let Some(code) = epsg::parse_wkt_epsg(trimmed) {
            return Ok(Spec::Epsg(code));
        }
        let projstring = proj4wkt::wkt_to_projstring(trimmed)
            .map_err(|e| Error::UnsupportedCrs(format!("WKT: {e}")))?;
        Ok(Spec::Proj4(projstring))
    } else {
        parse_epsg(trimmed).map(Spec::Epsg)
    }
}

/// A WKT CRS definition starts with a keyword followed by a bracketed body,
/// as in `PROJCS["..."]`, which no EPSG code or projstring does.
fn is_wkt(s: &str) -> bool {
    match s.find('[') {
        Some(bracket) if bracket > 0 => s[..bracket]
            .trim_end()
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '_'),
        _ => false,
    }
}

fn parse_epsg(code: &str) -> Result<u32, Error> {
    let num_str = code
        .strip_prefix("EPSG:")
        .or_else(|| code.strip_prefix("epsg:"))
        .unwrap_or(code);
    num_str
        .parse::<u32>()
        .map_err(|_| Error::UnsupportedCrs(code.to_string()))
}

fn make_inverse(epsg: u32) -> Result<InverseFn, Error> {
    match epsg {
        // WGS84 geographic — identity (input is lon/lat degrees)
        4326 => Ok(Box::new(|c: Coord| Ok(Geographic::new(c.x, c.y)))),
        // Web Mercator
        3857 => {
            let proj = WebMercator::new();
            Ok(Box::new(move |c: Coord| proj.inverse(c)))
        }
        // UTM zones 1-60 North
        32601..=32660 => {
            let zone = (epsg - 32600) as u8;
            let proj = TransverseMercator::utm(zone, true);
            Ok(Box::new(move |c: Coord| proj.inverse(c)))
        }
        // UTM zones 1-60 South
        32701..=32760 => {
            let zone = (epsg - 32700) as u8;
            let proj = TransverseMercator::utm(zone, false);
            Ok(Box::new(move |c: Coord| proj.inverse(c)))
        }
        _ => Err(Error::UnsupportedCrs(format!("EPSG:{epsg}"))),
    }
}

fn make_forward(epsg: u32) -> Result<ForwardFn, Error> {
    match epsg {
        // WGS84 geographic — identity (output is lon/lat)
        4326 => Ok(Box::new(|g: Geographic| Ok(Coord::new(g.lon, g.lat)))),
        // Web Mercator
        3857 => {
            let proj = WebMercator::new();
            Ok(Box::new(move |g: Geographic| proj.forward(g)))
        }
        // UTM zones 1-60 North
        32601..=32660 => {
            let zone = (epsg - 32600) as u8;
            let proj = TransverseMercator::utm(zone, true);
            Ok(Box::new(move |g: Geographic| proj.forward(g)))
        }
        // UTM zones 1-60 South
        32701..=32760 => {
            let zone = (epsg - 32700) as u8;
            let proj = TransverseMercator::utm(zone, false);
            Ok(Box::new(move |g: Geographic| proj.forward(g)))
        }
        _ => Err(Error::UnsupportedCrs(format!("EPSG:{epsg}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_4326_to_3857() {
        let t = Transform::new("EPSG:4326", "EPSG:3857").unwrap();
        let (x, y) = t.convert(-74.006, 40.7128).unwrap();
        // Known approximate values for NYC in Web Mercator
        assert!((x - (-8_238_310.0)).abs() < 100.0);
        assert!((y - 4_970_072.0).abs() < 100.0);
    }

    // the content of a typical ESRI .prj: NAD83 UTM zone 18N
    const UTM_18N_WKT: &str = r#"PROJCS["NAD_1983_UTM_Zone_18N",GEOGCS["GCS_North_American_1983",DATUM["D_North_American_1983",SPHEROID["GRS_1980",6378137.0,298.257222101]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["False_Easting",500000.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",-75.0],PARAMETER["Scale_Factor",0.9996],PARAMETER["Latitude_Of_Origin",0.0],UNIT["Meter",1.0]]"#;

    #[test]
    fn test_transform_from_wkt_matches_epsg() {
        let from_wkt = Transform::new(UTM_18N_WKT, "EPSG:4326").unwrap();
        let from_code = Transform::new("EPSG:26918", "EPSG:4326").unwrap();
        let (wkt_lon, wkt_lat) = from_wkt.convert(585_000.0, 4_510_000.0).unwrap();
        let (code_lon, code_lat) = from_code.convert(585_000.0, 4_510_000.0).unwrap();
        // same projection either way; only the datum handling may differ at the meter level
        assert!((wkt_lon - code_lon).abs() < 1e-5, "{wkt_lon} vs {code_lon}");
        assert!((wkt_lat - code_lat).abs() < 1e-5, "{wkt_lat} vs {code_lat}");
    }

    #[test]
    fn test_transform_wkt_roundtrip() {
        let forward = Transform::new("EPSG:4326", UTM_18N_WKT).unwrap();
        let inverse = Transform::new(UTM_18N_WKT, "EPSG:4326").unwrap();
        let (x, y) = forward.convert(-74.006, 40.7128).unwrap();
        let (lon, lat) = inverse.convert(x, y).unwrap();
        assert!((lon - (-74.006)).abs() < 1e-6);
        assert!((lat - 40.7128).abs() < 1e-6);
    }

    #[test]
    fn test_transform_geographic_wkt_is_identity() {
        let wkt = r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#;
        let t = Transform::new(wkt, "EPSG:4326").unwrap();
        let (lon, lat) = t.convert(-74.006, 40.7128).unwrap();
        assert!((lon - (-74.006)).abs() < 1e-9);
        assert!((lat - 40.7128).abs() < 1e-9);
    }

    #[test]
    fn test_transform_bad_wkt_errors() {
        let err = Transform::new("PROJCS[\"broken\"", "EPSG:4326").unwrap_err();
        assert!(matches!(err, Error::UnsupportedCrs(_)), "{err}");
    }

    #[test]
    fn test_transform_roundtrip() {
        let t1 = Transform::new("EPSG:4326", "EPSG:32618").unwrap();
        let t2 = Transform::new("EPSG:32618", "EPSG:4326").unwrap();
        let (x, y) = t1.convert(-74.006, 40.7128).unwrap();
        let (lon, lat) = t2.convert(x, y).unwrap();
        assert!((lon - (-74.006)).abs() < 1e-6);
        assert!((lat - 40.7128).abs() < 1e-6);
    }
}
