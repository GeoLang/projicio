# Projicio

[![CI](https://github.com/GeoLang/projicio/actions/workflows/ci.yml/badge.svg)](https://github.com/GeoLang/projicio/actions)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)

**Pure-Rust coordinate reference system and map projection engine.**

Zero C dependencies. No PROJ, no GDAL. Just fast, correct coordinate transformations.

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
- **Batch transforms** — transform thousands of coordinates efficiently
- **Pure Rust** — no unsafe, no C dependencies, no build scripts

## Quick Start

```bash
# Transform a coordinate
projicio transform --from EPSG:4326 --to EPSG:3857 -- -74.006 40.7128

# Library usage
cargo add projicio-core
```

```rust
use projicio_core::{Transform, Coord};

let t = Transform::new("EPSG:4326", "EPSG:3857").unwrap();
let result = t.forward(&Coord::new(-74.006, 40.7128)).unwrap();
println!("NYC in Web Mercator: {}, {}", result.x, result.y);
```

## Architecture

```
projicio-core    — Projection math, ellipsoids, datum transforms, NTv2, CRS registry
projicio-cli     — Command-line interface
```

## Supported CRS

| Family | EPSG Codes | Status |
|--------|------------|--------|
| WGS84 Geographic | 4326 | ✅ |
| Web Mercator | 3857 | ✅ |
| UTM North | 32601–32660 | ✅ |
| UTM South | 32701–32760 | ✅ |
| Mercator | 3395 | ✅ |
| Lambert Conformal Conic | 2154, custom | ✅ |
| Albers Equal Area | custom | ✅ |
| Polar Stereographic | custom | ✅ |

## Datum Transforms

| Method | Description |
|--------|-------------|
| Helmert 7-parameter | 3 translations + 3 rotations + scale factor |
| NTv2 grid shift | Bilinear interpolation from binary grid files |
| Geocentric pipeline | Geodetic → ECEF → Helmert → ECEF → Geodetic |

## License

AGPL-3.0-or-later
