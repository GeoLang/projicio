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

### Changed

- `Transform::new` uses the native path when both codes are native, otherwise hands the
  whole transform to the fallback so the datum shift is applied once. Native results are
  unchanged.

## [0.1.0] - 2026-05-30

### Added

- Initial release.
