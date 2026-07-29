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
- **NTv2 grid shifts** — register a `.gsb` at runtime and transforms use it (OSTN15, NAD27, etc.)
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

# Supply a datum shift grid, as NAME=PATH
projicio --grid conus=/data/proj/conus transform --from EPSG:4267 --to EPSG:4326 -- -100 35

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
assert_eq!(epsg::support(4267), Support::NeedsGrid);   // NAD27, see Datum shift grids
assert_eq!(epsg::support(99999), Support::Unsupported);
```

| | Codes | With common grids registered |
|---|---|---|
| Native | 122 | 122 |
| Fallback | 5747 | 5951 |
| **Total resolvable** | **5869** | **6073** |
| Needs a datum-shift grid | 237 | 33 |
| No usable projection method | 78 | 78 |

`epsg::support` builds the definition before answering, so it never promises a transform
that fails later. The 78 permanent gaps are projection methods proj4rs does not implement
(Cassini-Soldner, oblique Mercator, polyconic, Equal Earth and a few others). The rest
need a grid file you supply.

## Datum shift grids

projicio embeds no grid data. A definition that names a grid transforms only once you
register the matching NTv2 `.gsb` file, which you download yourself:

```rust
use projicio_core::{Support, Transform, epsg, grids};

// 204 NAD27 codes, including most 1927 State Plane zones, are behind this one grid.
grids::register_file("conus", "/data/proj/conus").unwrap();
assert_eq!(epsg::support(4267), Support::Fallback);
let t = Transform::new("EPSG:4267", "EPSG:4326").unwrap();
```

`register_bytes` takes the file contents instead, for embedding or fetching your own way.
The name is the one the definition uses, which is not always a file name: `+datum=NAD27`
names `conus`, `alaska`, `ntv2_0.gsb` and `ntv1_can.dat`, while `+nadgrids=` names
whatever you write. Registration is process wide, and a name can be registered once.

Grids the embedded table asks for, by number of codes waiting on them:

| Grid | Codes | Source |
|---|---|---|
| `conus` and friends (NAD27) | 204 | NOAA NGS / the PROJ data package |
| `nzgd2kgrid0005.gsb` (NZGD49) | 33 | Land Information New Zealand |

### OSTN15 for the British National Grid

EPSG:27700's embedded definition carries the classic 7-parameter OSGB36 Helmert shift,
which is good to a few metres. For the OSTN15 accuracy that Ordnance Survey publishes,
register the OS grid and name it in a projstring, which `Transform::new` accepts wherever
an EPSG code goes:

```rust
use projicio_core::{Transform, grids};

grids::register_file("OSTN15_NTv2_OSGBtoETRS.gsb", "/data/OSTN15_NTv2_OSGBtoETRS.gsb").unwrap();

let bng = "+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +x_0=400000 +y_0=-100000 \
           +ellps=airy +nadgrids=OSTN15_NTv2_OSGBtoETRS.gsb +units=m +no_defs";
let t = Transform::new("EPSG:4258", bng).unwrap();
```

The file is `OSTN15_NTv2_OSGBtoETRS.gsb`, from the "NTv2 format files" zip on the
[OS coordinate transformations resources page](https://www.ordnancesurvey.co.uk/geodesy-positioning/coordinate-transformations/resources).
Pick the OSTN15 links, not the superseded OSTN02 ones still listed there. OS distributes
the grid free of charge, and publishes it under the Open Government Licence 3.0 in their
own [os-transform repository](https://github.com/OrdnanceSurvey/os-transform). Download it
from OS rather than relying on a redistributed copy. OSTN15 covers the horizontal
transformation, which is all that 2D easting and northing work needs; the separate OSGM15
geoid model is for heights.

No OS data ships with this crate, so projicio's own tests drive the same code path with a
synthetic grid of the same shape. To check the real file, point the ignored OSTN15 test at
your download:

```bash
PROJICIO_OSTN15_GSB=/data/OSTN15_NTv2_OSGBtoETRS.gsb \
  cargo test -p projicio-core --test grids -- --ignored ostn15_real
```

That asserts the OS published test point TP01, ETRS89 49.92226393730 N 6.29977752014 W
giving easting 91492.146 and northing 11318.804. The full set of TP01 to TP40 points is in
the OS developer pack as `OSTN15_OSGM15_TestInput_ETRStoOSGB.txt` and the matching output
file.

## Accuracy

Projections within one datum are exact to the published formulas. The test suite checks
that against IOGP Guidance Note 7-2 worked examples, NOAA NGS datasheets and IGN's
published Lambert-93 constants.

Between two different datums the accuracy depends on what the definition carries: a
registered grid where one is named, otherwise the 7-parameter Helmert shift in the
embedded definition, which is metre-level. A definition with neither applies no shift at
all, and `epsg::support` will still report `Fallback` for it, since that is what the
definition itself asks for.

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
