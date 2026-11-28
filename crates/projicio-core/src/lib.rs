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
//!
//! The `registry` feature, on by default, embeds EPSG data generated from
//! PROJ's proj.db: names, kind, deprecation, datum links, and areas of use
//! (see `epsg::metadata`), and the CRS definitions and coordinate operations
//! the `registry` module looks up by code.

mod datum;
mod datum_transformation;
mod ellipsoid;
pub mod epsg;
mod error;
mod fallback;
pub mod grids;
#[cfg(feature = "registry")]
pub mod horizontal_shift_grid;
pub mod iso19111;
#[cfg(feature = "registry")]
pub mod operation_factory;
mod projection;
pub mod projstring;
#[cfg(feature = "registry")]
pub mod registry;
mod transform;

pub use datum::{
    GeocentricCoord, HelmertTransform, geocentric_to_geodetic, geodetic_to_geocentric,
    transform_datum,
};
pub use datum_transformation::{
    DatumTransformation, GeocentricConversion, GeodeticCoord, Helmert,
    geocentric_transformation_from_epsg_method, transformation_from_epsg_method,
};
pub use ellipsoid::Ellipsoid;
pub use epsg::Support;
pub use error::Error;
#[cfg(feature = "registry")]
pub use horizontal_shift_grid::grid_transformation_from_epsg_method;
pub use projection::{
    AlbersEqualArea, AmericanPolyconic, CassiniSoldner, EqualEarth, HotineObliqueMercator, Laborde,
    LambertAzimuthalEqualArea, LambertConformalConic, Mercator, PolarStereographic, Projection,
    TransverseMercator, WebMercator, from_epsg_method,
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
