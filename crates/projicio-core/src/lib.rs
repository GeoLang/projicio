//! Projicio — Pure-Rust coordinate reference system and map projection engine.
//!
//! Provides coordinate transformations between geographic (longitude/latitude)
//! and projected coordinate systems with no C dependencies.
//!
//! Codes projicio implements itself are transformed by its own projection math.
//! Anything else resolves through an embedded proj4 definition table covering the
//! wider EPSG registry, transformed by the pure-Rust proj4rs engine. Ask
//! [`epsg::support`] which path a code takes.
//!
//! No grid data is embedded. Codes whose definition names a datum shift grid report
//! [`Support::NeedsGrid`] until the caller supplies the `.gsb` file through
//! [`grids::register_file`].

mod datum;
mod ellipsoid;
pub mod epsg;
mod error;
mod fallback;
pub mod grids;
mod ntv2;
mod projection;
mod transform;

pub use datum::{
    GeocentricCoord, HelmertTransform, geocentric_to_geodetic, geodetic_to_geocentric,
    transform_datum,
};
pub use ellipsoid::Ellipsoid;
pub use epsg::Support;
pub use error::Error;
pub use ntv2::{NTv2Grid, SubGrid};
pub use projection::{
    AlbersEqualArea, LambertConformalConic, Mercator, PolarStereographic, Projection,
    TransverseMercator, WebMercator,
};
pub use transform::Transform;

/// A 2D coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coord {
    pub x: f64,
    pub y: f64,
}

impl Coord {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A geographic coordinate in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geographic {
    pub lon: f64,
    pub lat: f64,
}

impl Geographic {
    pub fn new(lon: f64, lat: f64) -> Self {
        Self { lon, lat }
    }
}
