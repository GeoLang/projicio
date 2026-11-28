# Projicio

[![CI](https://github.com/GeoLang/projicio/actions/workflows/ci.yml/badge.svg)](https://github.com/GeoLang/projicio/actions)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL--2.0-blue.svg)](LICENSE)

Pure-Rust coordinate reference system and map projection engine.

Transforms coordinates between 5935 EPSG codes without linking C PROJ or GDAL.

[Documentation](https://geolang.github.io/projicio/) · [GitHub](https://github.com/GeoLang/projicio)

## Features

- **EPSG codes, proj4 strings and WKT**: `Transform::new("EPSG:4326", "EPSG:3857")` resolves
  5935 EPSG codes, and either side can also be a proj4 string or a `.prj` file's WKT
- **Projections**: Web Mercator, Transverse Mercator and UTM, Mercator, Lambert Conformal
  Conic, Lambert Azimuthal Equal Area, Albers, Polar Stereographic, Cassini-Soldner, Hotine
  Oblique Mercator, American Polyconic, Equal Earth and Laborde in projicio's own code, the
  rest through proj4rs
- **Datum shifts**: the Helmert parameters in each definition, and NTv2 grids you register
  at runtime (OSTN15, NAD27 and others)
- **EPSG metadata**: names, deprecation, datums and areas of use, plus the definitions and
  coordinate operations behind every current EPSG CRS, generated from PROJ 9.8.1's proj.db
- **ISO 19111 model ported from PROJ**: PROJJSON, WKT1 and WKT2 read and write, an
  authority factory, operation selection as `proj_create_crs_to_crs` does it, and datum and
  grid transformations by EPSG method. `Transform` does not use these yet
- **CLI and WebAssembly bindings**, grid registration included
- **Pure Rust**: no C dependencies and no data files to install

## Quick Start

```bash
# Transform a coordinate
projicio transform --from EPSG:4326 --to EPSG:27700 -- -0.1275 51.50722

# Ask whether a CRS is supported and by which engine
projicio info EPSG:27700

# Supply a datum shift grid, as NAME=PATH
projicio --grid conus=/data/proj/conus transform --from EPSG:4267 --to EPSG:4326 -- -100 35

# Library usage, from git since the crates are not on crates.io
cargo add projicio-core --git https://github.com/GeoLang/projicio
```

```rust
use projicio_core::Transform;

let t = Transform::new("EPSG:4326", "EPSG:3857").unwrap();
let (x, y) = t.convert(-74.006, 40.7128).unwrap();
println!("NYC in Web Mercator: {x}, {y}");
```

## Accuracy

Projections follow the published formulas and are tested against IOGP Guidance Note 7-2,
NOAA NGS datasheets and IGN's Lambert-93 constants. A parity suite compares transforms with
PROJ 9.8.1's `cs2cs` weekly in CI. `tools/proj-agreement` measures agreement with `cs2cs`
over every current EPSG CRS and operation. The current scores are in the
[guide](docs/guide.md#accuracy).

## Documentation

[docs/guide.md](docs/guide.md) covers each part in detail:

- [CRS coverage](docs/guide.md#crs-coverage): which codes resolve and through which engine
- [EPSG metadata](docs/guide.md#epsg-metadata): the embedded registry and authority factory
- [Datum shift grids](docs/guide.md#datum-shift-grids): registering grids, in the browser
  too, and OSTN15
- [Accuracy](docs/guide.md#accuracy): parity tests, proj-agreement and PROJ's gie tests
- [Architecture](docs/guide.md#architecture) and [supported CRSs](docs/guide.md#supported-crs)
- [WKT in the ISO 19111 model](docs/guide.md#wkt-in-the-iso-19111-model)
- [Datum transforms](docs/guide.md#datum-transforms) and
  [choosing an operation between two CRSs](docs/guide.md#choosing-an-operation-between-two-crss)

The API reference is at [geolang.github.io/projicio/api](https://geolang.github.io/projicio/api/).

## Credits

The Cassini-Soldner, Hotine Oblique Mercator, American Polyconic, Equal Earth and Laborde
implementations are ported from [proj-rust](https://github.com/roteiro-gis/proj-rust), used under
its MIT OR Apache-2.0 license.

The projections behind `from_epsg_method`, the datum transformations behind
`transformation_from_epsg_method`, the operation search behind `CandidateOperations` and the
grid readers behind `horizontal_shift_grid` are
ported from [PROJ](https://github.com/OSGeo/PROJ),
used under its MIT license, kept in [LICENSES/PROJ-MIT.txt](LICENSES/PROJ-MIT.txt).

## License

MPL-2.0, see [LICENSE](LICENSE).

Copyright (C) 2026 Grok Image Compression Inc.
