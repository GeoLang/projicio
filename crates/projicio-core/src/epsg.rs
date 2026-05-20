//! EPSG CRS registry — lookup projection parameters by EPSG code.
//!
//! Provides a built-in database of common coordinate reference systems
//! with their projection parameters, datum, and ellipsoid info.

use crate::ellipsoid::Ellipsoid;

/// A coordinate reference system definition from the EPSG registry.
#[derive(Debug, Clone)]
pub struct CrsDef {
    pub code: u32,
    pub name: &'static str,
    pub proj_type: ProjType,
    pub ellipsoid: Ellipsoid,
    pub params: ProjParams,
}

/// Projection type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjType {
    Geographic,
    TransverseMercator,
    LambertConformalConic,
    WebMercator,
    AlbersEqualArea,
    PolarStereographic,
}

/// Projection parameters.
#[derive(Debug, Clone)]
pub struct ProjParams {
    pub lat_0: f64,
    pub lon_0: f64,
    pub k_0: f64,
    pub x_0: f64,
    pub y_0: f64,
    pub lat_1: f64,
    pub lat_2: f64,
}

impl ProjParams {
    const fn zeroed() -> Self {
        Self {
            lat_0: 0.0,
            lon_0: 0.0,
            k_0: 1.0,
            x_0: 0.0,
            y_0: 0.0,
            lat_1: 0.0,
            lat_2: 0.0,
        }
    }
}

/// Look up a CRS definition by EPSG code.
pub fn lookup(code: u32) -> Option<CrsDef> {
    match code {
        4326 => Some(CrsDef {
            code: 4326,
            name: "WGS 84",
            proj_type: ProjType::Geographic,
            ellipsoid: Ellipsoid::WGS84,
            params: ProjParams::zeroed(),
        }),
        3857 => Some(CrsDef {
            code: 3857,
            name: "WGS 84 / Pseudo-Mercator",
            proj_type: ProjType::WebMercator,
            ellipsoid: Ellipsoid::WGS84,
            params: ProjParams {
                lat_0: 0.0,
                lon_0: 0.0,
                k_0: 1.0,
                x_0: 0.0,
                y_0: 0.0,
                lat_1: 0.0,
                lat_2: 0.0,
            },
        }),
        // UTM zones 1-60 North (326xx)
        32601..=32660 => {
            let zone = code - 32600;
            let lon_0 = (zone as f64 - 1.0) * 6.0 - 180.0 + 3.0;
            Some(CrsDef {
                code,
                name: "WGS 84 / UTM zone N",
                proj_type: ProjType::TransverseMercator,
                ellipsoid: Ellipsoid::WGS84,
                params: ProjParams {
                    lat_0: 0.0,
                    lon_0,
                    k_0: 0.9996,
                    x_0: 500000.0,
                    y_0: 0.0,
                    lat_1: 0.0,
                    lat_2: 0.0,
                },
            })
        }
        // UTM zones 1-60 South (327xx)
        32701..=32760 => {
            let zone = code - 32700;
            let lon_0 = (zone as f64 - 1.0) * 6.0 - 180.0 + 3.0;
            Some(CrsDef {
                code,
                name: "WGS 84 / UTM zone S",
                proj_type: ProjType::TransverseMercator,
                ellipsoid: Ellipsoid::WGS84,
                params: ProjParams {
                    lat_0: 0.0,
                    lon_0,
                    k_0: 0.9996,
                    x_0: 500000.0,
                    y_0: 10000000.0,
                    lat_1: 0.0,
                    lat_2: 0.0,
                },
            })
        }
        // NAD83 geographic
        4269 => Some(CrsDef {
            code: 4269,
            name: "NAD83",
            proj_type: ProjType::Geographic,
            ellipsoid: Ellipsoid::GRS80,
            params: ProjParams::zeroed(),
        }),
        // NAD83 / California zone 5
        2229 => Some(CrsDef {
            code: 2229,
            name: "NAD83 / California zone 5 (ftUS)",
            proj_type: ProjType::LambertConformalConic,
            ellipsoid: Ellipsoid::GRS80,
            params: ProjParams {
                lat_0: 33.5,
                lon_0: -118.0,
                k_0: 1.0,
                x_0: 2000000.0,
                y_0: 500000.0,
                lat_1: 34.0 + 2.0 / 60.0,
                lat_2: 35.0 + 28.0 / 60.0,
            },
        }),
        // ETRS89
        4258 => Some(CrsDef {
            code: 4258,
            name: "ETRS89",
            proj_type: ProjType::Geographic,
            ellipsoid: Ellipsoid::GRS80,
            params: ProjParams::zeroed(),
        }),
        // OSGB 1936 / British National Grid
        27700 => Some(CrsDef {
            code: 27700,
            name: "OSGB 1936 / British National Grid",
            proj_type: ProjType::TransverseMercator,
            ellipsoid: Ellipsoid::new(6377563.396, 1.0 / 299.3249646),
            params: ProjParams {
                lat_0: 49.0,
                lon_0: -2.0,
                k_0: 0.9996012717,
                x_0: 400000.0,
                y_0: -100000.0,
                lat_1: 0.0,
                lat_2: 0.0,
            },
        }),
        // NAD83 / Conus Albers
        5070 => Some(CrsDef {
            code: 5070,
            name: "NAD83 / Conus Albers",
            proj_type: ProjType::AlbersEqualArea,
            ellipsoid: Ellipsoid::GRS80,
            params: ProjParams {
                lat_0: 23.0,
                lon_0: -96.0,
                k_0: 1.0,
                x_0: 0.0,
                y_0: 0.0,
                lat_1: 29.5,
                lat_2: 45.5,
            },
        }),
        _ => None,
    }
}

/// Parse a simple WKT CRS string and extract the EPSG code.
///
/// Supports WKT1 and WKT2 patterns like:
/// - `AUTHORITY["EPSG","4326"]`
/// - `ID["EPSG",4326]`
pub fn parse_wkt_epsg(wkt: &str) -> Option<u32> {
    // Try WKT1: AUTHORITY["EPSG","CODE"]
    if let Some(idx) = wkt.find("AUTHORITY[\"EPSG\"") {
        let after = &wkt[idx..];
        if let Some(start) = after.find(",\"") {
            let rest = &after[start + 2..];
            if let Some(end) = rest.find('"') {
                return rest[..end].parse().ok();
            }
        }
    }

    // Try WKT2: ID["EPSG",CODE]
    if let Some(idx) = wkt.find("ID[\"EPSG\"") {
        let after = &wkt[idx..];
        if let Some(start) = after.find(',') {
            let rest = &after[start + 1..];
            if let Some(end) = rest.find(']') {
                return rest[..end].trim().parse().ok();
            }
        }
    }

    // Try EPSG:CODE pattern
    if let Some(idx) = wkt.find("EPSG:") {
        let rest = &wkt[idx + 5..];
        let code_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        return code_str.parse().ok();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_wgs84() {
        let crs = lookup(4326).unwrap();
        assert_eq!(crs.name, "WGS 84");
        assert_eq!(crs.proj_type, ProjType::Geographic);
    }

    #[test]
    fn test_lookup_utm_zone() {
        let crs = lookup(32632).unwrap();
        assert_eq!(crs.proj_type, ProjType::TransverseMercator);
        assert!((crs.params.lon_0 - 9.0).abs() < 1e-10); // Zone 32 central meridian
        assert_eq!(crs.params.k_0, 0.9996);
        assert_eq!(crs.params.x_0, 500000.0);
    }

    #[test]
    fn test_lookup_utm_south() {
        let crs = lookup(32755).unwrap();
        assert_eq!(crs.params.y_0, 10000000.0); // Southern hemisphere false northing
    }

    #[test]
    fn test_lookup_web_mercator() {
        let crs = lookup(3857).unwrap();
        assert_eq!(crs.proj_type, ProjType::WebMercator);
    }

    #[test]
    fn test_lookup_unknown() {
        assert!(lookup(99999).is_none());
    }

    #[test]
    fn test_parse_wkt1_epsg() {
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984"],AUTHORITY["EPSG","4326"]]"#;
        assert_eq!(parse_wkt_epsg(wkt), Some(4326));
    }

    #[test]
    fn test_parse_wkt2_epsg() {
        let wkt = r#"PROJCRS["WGS 84 / UTM zone 32N",ID["EPSG",32632]]"#;
        assert_eq!(parse_wkt_epsg(wkt), Some(32632));
    }

    #[test]
    fn test_parse_epsg_colon() {
        assert_eq!(parse_wkt_epsg("EPSG:3857"), Some(3857));
    }

    #[test]
    fn test_parse_no_epsg() {
        assert_eq!(parse_wkt_epsg("some random string"), None);
    }
}
