# Projicio

[![CI](https://github.com/GeoLang/projicio/actions/workflows/ci.yml/badge.svg)](https://github.com/GeoLang/projicio/actions)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)

**Pure-Rust coordinate reference system and map projection engine.**

No C PROJ, no GDAL. Pure Rust throughout, with 5869 EPSG codes embedded at compile time.

[Documentation](https://geolang.github.io/projicio/) · [GitHub](https://github.com/GeoLang/projicio)

## Features

- **Web Mercator** (EPSG:3857) — forward and inverse
- **Transverse Mercator / UTM** (EPSG:32601–32660, 32701–32760) — all 120 zones
- **Mercator** (EPSG:3395) — ellipsoidal
- **Lambert Conformal Conic** — 2SP variant
- **Albers Equal Area** — conic equal-area projection
- **Polar Stereographic** — for polar regions
- **Helmert 7-parameter datum transforms** — translation, rotation, scale (geocentric)
- **NTv2 grid shifts** — binary grid file loading, bilinear interpolation (NAD27→NAD83, etc.)
- **Datum transforms** — geodetic ↔ geocentric conversion pipeline
- **Ellipsoids** — WGS84, GRS80, Clarke 1866, International 1924, unit sphere
- **EPSG code dispatch** — `Transform::new("EPSG:4326", "EPSG:3857")`
- **5869 EPSG codes** — national grids, State Plane, UTM on any datum, via a fallback engine
- **Batch transforms** — transform thousands of coordinates efficiently
- **Pure Rust** — no unsafe in projicio, no C dependencies, no build scripts, no runtime data files

## Quick Start

```bash
# Transform a coordinate
projicio transform --from EPSG:4326 --to EPSG:27700 -- -0.1275 51.50722

# Ask whether a CRS is supported and by which engine
projicio info EPSG:27700

# Library usage
cargo add projicio-core
```

```rust
use projicio_core::Transform;

let t = Transform::new("EPSG:4326", "EPSG:3857").unwrap();
let (x, y) = t.convert(-74.006, 40.7128).unwrap();
println!("NYC in Web Mercator: {x}, {y}");
```

## CRS Coverage

Two engines sit behind the same API. Codes projicio implements itself take its own
projection math. Everything else resolves through an embedded proj4 definition table
([`crs-definitions`](https://crates.io/crates/crs-definitions)) transformed by
[`proj4rs`](https://crates.io/crates/proj4rs), a pure-Rust port of proj4. Both crates
embed their data at compile time, so there is still nothing to install and nothing to
read from disk.

```rust
use projicio_core::{Support, epsg};

assert_eq!(epsg::support(4326), Support::Native);
assert_eq!(epsg::support(27700), Support::Fallback);
assert_eq!(epsg::support(99999), Support::Unsupported);
```

| | Codes |
|---|---|
| Native | 122 |
| Fallback | 5747 |
| **Total resolvable** | **5869** |
| In the table but not resolvable | 315 |

The 315 gaps are codes whose definition needs a datum-shift grid file (206 of them are
NAD27-based, including most 1927 State Plane zones) plus projection methods proj4rs does
not implement (Cassini-Soldner, oblique Mercator, polyconic, Equal Earth and a few
others). `epsg::support` builds the definition before answering, so it reports these as
unsupported rather than promising a transform that fails later.

Accuracy note: a transform between two different datums uses the 7-parameter Helmert
shift carried in the embedded definition, which is metre-level. Projections within one
datum are exact to the published formulas, and the test suite checks that against IOGP
Guidance Note 7-2, NOAA NGS datasheets and IGN's Lambert-93 constants.

## Architecture

```
projicio-core    — Projection math, ellipsoids, datum transforms, NTv2, CRS registry, fallback engine
projicio-cli     — Command-line interface
```

## Supported CRS

Native path, projicio's own projection math:

| Family | EPSG Codes |
|--------|------------|
| WGS84 Geographic | 4326 |
| Web Mercator | 3857 |
| UTM North | 32601–32660 |
| UTM South | 32701–32760 |

The projection types below are implemented natively and usable directly, but
`Transform` routes their EPSG codes through the fallback engine, which handles the
axis units and datum shift those codes carry:

| Family | Example codes |
|--------|---------------|
| Mercator | 3395 |
| Transverse Mercator | 27700, 25832, 31370 |
| Lambert Conformal Conic | 2154, 26985, 2229 |
| Lambert Azimuthal Equal Area | 3035 |
| Albers Equal Area | 5070 |
| Polar Stereographic | 3031, 3413 |

Ask for any other code with `projicio info EPSG:<code>` or `epsg::support(code)`.

## Datum Transforms

| Method | Description |
|--------|-------------|
| Helmert 7-parameter | 3 translations + 3 rotations + scale factor |
| NTv2 grid shift | Bilinear interpolation from binary grid files |
| Geocentric pipeline | Geodetic → ECEF → Helmert → ECEF → Geodetic |

## License

AGPL-3.0-or-later, see [LICENSE](LICENSE).

Copyright (C) 2026 Grok Image Compression Inc.
