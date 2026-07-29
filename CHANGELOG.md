# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

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

### Changed

- `Transform::new` uses the native path when both codes are native, otherwise hands the
  whole transform to the fallback so the datum shift is applied once. Native results are
  unchanged.

## [0.1.0] - 2026-05-30

### Added

- Initial release.
