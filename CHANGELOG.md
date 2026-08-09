# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Native path built from an EPSG code's own proj4 definition, for the 64 codes proj4rs has
  no projection method for. `epsg::support` reports `Support::Native` for them and
  `Transform` projects them with projicio's own math. Cassini-Soldner, Hotine Oblique
  Mercator, American Polyconic, Equal Earth and Laborde are ported from
  [proj-rust](https://github.com/pka/proj-rust) under its MIT OR Apache-2.0 license and
  exposed as `CassiniSoldner`, `HotineObliqueMercator`, `AmericanPolyconic`, `EqualEarth`
  and `Laborde`.
- `projstring::parse` reads a proj4 definition into projection parameters, rejecting any
  definition that carries a parameter it does not fully implement. A rejected definition
  leaves its EPSG code with the classification it already had, so nothing that already
  worked changes engine.

- `Transform::new` accepts a WKT CRS definition (the content of a `.prj` sidecar) on
  either side, WKT1 or WKT2. A WKT naming its EPSG code resolves through that code; a
  codeless one (typical for ESRI `.prj`) is converted to a projstring by `proj4wkt` and
  runs on the fallback engine.

- Fallback transform engine covering 5869 EPSG codes, up from 122. Definitions come from
  the `crs-definitions` crate and are transformed by `proj4rs`, both pure Rust with their
  data embedded at compile time. National grids, State Plane zones and UTM on non-WGS84
  datums now work through the existing `Transform::new` API.
- `epsg::support` and `epsg::is_native` report whether an EPSG code is supported and
  which engine handles it, as `Support::Native`, `Support::Fallback` or
  `Support::Unsupported`.
- `epsg::proj4_definition` returns the embedded proj4 string for a code.
- `Transform::path` reports which engine a transform resolved to.
- `Debug` for `Transform`.
- `projicio info EPSG:<code>` prints support, name and proj4 definition for a code.
- `grids::register_file` and `grids::register_bytes` register an NTv2 datum shift grid at
  runtime, so definitions naming a grid transform accurately once the user supplies the
  `.gsb`. Registering `conus` alone resolves 204 NAD27 codes, including most 1927 State
  Plane zones. No grid data is embedded. Parsing and interpolation are proj4rs's, so a
  registered grid runs through the same code as the rest of the fallback engine.
- `grids::is_registered` and `grids::registered` report what has been registered.
- `Support::NeedsGrid` distinguishes a code whose definition names a datum shift grid
  that has not been registered from one projicio cannot do at all.
- `Error::GridError` for grid registration and missing grid failures.
- `Transform::new` accepts a proj4 projstring on either side, which is how a caller names
  a grid the embedded definition does not mention, as OSTN15 needs for EPSG:27700.
- `projicio --grid NAME=PATH` registers a grid from the CLI, repeatable.
- `projicio-wasm`: WebAssembly bindings exposing `transform_coordinates(from, to, [x0,
  y0, ...])`, built with `wasm-pack --target web`. The `grids` module is compiled out on
  wasm32 (proj4rs has no grid reader there), and the new default-on `aeqd` feature of
  `projicio-core` is off in the wasm build because `proj4rs-geodesic` does not compile
  on wasm32.

### Fixed

- `epsg::parse_wkt_epsg` on nested WKT returned the first `AUTHORITY`/`ID` code it saw,
  which is the datum's or spheroid's, not the CRS's. It now takes the last one.

### Removed

- The standalone `NTv2Grid` and `SubGrid` reader. Nothing used it, and grid shifts now
  run through the registration path, which parses with proj4rs.

### Changed

- `Transform::new` uses the native path when both codes are native, otherwise hands the
  whole transform to the fallback so the datum shift is applied once. Native results are
  unchanged.
- A pair where one side is built from its definition meets at WGS84 geographic, so each
  side reaches the hub on its own and the datum shift still happens once. Where the
  definition names no datum, neither side is shifted, matching what proj does for such a
  pair.
- `HelmertTransform::inverse` undoes `forward` exactly, transposing the rotation and
  dividing out the scale instead of negating the parameters. The old form left a
  centimetre-level residue at the rotations national grids use.

## [0.1.0] - 2026-05-30

### Added

- Initial release.
