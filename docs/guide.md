# Projicio in detail

How projicio resolves CRSs, what it embeds, how datum shifts and grids work, how it is measured against PROJ, and the ISO 19111 model ported from PROJ. The [README](../README.md) has the overview and quick start.

## CRS Coverage

`Transform` uses one of two engines, both reading the same embedded proj4 definition table
([`crs-definitions`](https://crates.io/crates/crs-definitions)). Codes projicio can build
itself take its own projection math, either from its hand-written table or by parsing the
definition. Everything else is transformed by [`proj4rs`](https://crates.io/crates/proj4rs),
a pure-Rust port of proj4. Both crates embed their data at compile time, so there is still
nothing to install and nothing to read from disk.

```rust
use projicio_core::{Support, epsg};

assert_eq!(epsg::support(4326), Support::Native);
assert_eq!(epsg::support(27700), Support::Fallback);
assert_eq!(epsg::support(4267), Support::NeedsGrid);   // NAD27, see Datum shift grids
assert_eq!(epsg::support(99999), Support::Unsupported);
```

| | Codes | With common grids registered |
|---|---|---|
| Native | 209 | 209 |
| Fallback | 5726 | 5930 |
| **Total resolvable** | **5935** | **6139** |
| Needs a datum-shift grid | 237 | 33 |
| No usable projection method | 12 | 12 |

`epsg::support` builds the definition before answering, so it never promises a transform
that fails later.

64 of the native codes are ones proj4rs cannot build. projicio parses their embedded
definition itself and projects them with its own math, which is how Cassini-Soldner,
Hotine Oblique Mercator, American Polyconic, Equal Earth and Laborde grids resolve. 23
more are Lambert Azimuthal Equal Area and Equidistant Cylindrical codes proj4rs can build
but with the formulas PROJ replaced in 9.7 and 9.8. projicio runs those through
`from_epsg_method`, ported from current PROJ. The parser rejects any definition carrying a
parameter it does not fully implement, so a code either transforms exactly as its
definition says or keeps reporting that it cannot.

The 12 remaining gaps break down as four for a projection method nobody here implements
(three `cea`, one `nzmg`), four for a prime meridian other than Greenwich (two on Ferro,
two on Paris), two empty definitions, and two `omerc` definitions projicio does implement
but rejects on a parameter: one carries `+vunits=m`, which nothing consumes and so the
projstring parser refuses outright, and the other is `+units=us-ft`, which is not in the
three-entry units table. The 237 codes outside that 12 need a grid file you supply.

## EPSG metadata

Separately from what it can transform, projicio embeds what the EPSG dataset says about
every EPSG CRS: its name, whether it is geographic or projected, whether EPSG has
deprecated it, the geodetic datum it is referenced to, and the area it is meant to be
used in.

```rust
use projicio_core::epsg;

let zone = epsg::metadata(32618).unwrap();
assert_eq!(zone.name, "WGS 84 / UTM zone 18N");
assert!(!zone.deprecated);

// New York is inside zone 18N, Paris is not
assert_eq!(epsg::applies_at(32618, -74.0, 40.7), Some(true));
assert_eq!(epsg::applies_at(32618, 2.35, 48.85), Some(false));
```

A coordinate outside a CRS's area of use still transforms. `applies_at` reports what EPSG
intends the code for, not what projicio will do with it, and answers `None` when EPSG
records no area of use.

`epsg::datum_aliases(datum_code)` lists every name EPSG records for a datum. This is
metadata only. Which engine transforms a code, and whether it can, is still
`epsg::support`.

The `projicio_core::registry` module also looks up, by EPSG code, the definitions behind
every current geodetic and projected CRS and every current EPSG coordinate operation
between geodetic CRSs: units of measure, ellipsoids, prime meridians, datums and datum
ensembles, coordinate systems with their axes, CRS definitions, conversions with their
method and parameters, Helmert, grid, other and concatenated operations with their
accuracy and areas of use, which operation supersedes which, and PROJ's CDN file for
each grid an operation names. It also holds proj.db's own operations between EPSG
geodetic CRSs (22 Helmert, 62 concatenated), coded `PROJ:<code>`. An operation is looked
up by `OperationCode::Epsg(code)` or `OperationCode::Proj(code)`.
`registry::operations_between(source, target)` lists the operations between two CRSs in
either direction. These are plain records, nothing in the transform path reads them yet.

```rust
use projicio_core::registry::{self, OperationCode};

let nad83_to_wgs84 = registry::coordinate_operation(OperationCode::Epsg(1188)).unwrap();
assert_eq!(nad83_to_wgs84.accuracy, Some(4.0));
```

`iso19111::authority_factory` turns these records into the ISO 19111 model, following
PROJ's `factory.cpp`: names, identifiers, units, accuracies, areas of use, ensemble
members, dynamic frame epochs, PROJ's grid file names in place of EPSG's, and the
direction of each step of a concatenated operation. Unknown codes raise
`Error::NoSuchAuthorityCode` naming the kind and code. The registry carries no scope text,
remarks or extent descriptions, so an object's area of use is named by the EPSG extent
name, where projinfo writes the extent description.

```rust
use projicio_core::iso19111::authority_factory::{
    create_coordinate_operation, create_coordinate_reference_system,
};
use projicio_core::registry::OperationCode;

let lambert_93 = create_coordinate_reference_system(2154).unwrap();
let nad27_to_nad83 = create_coordinate_operation(OperationCode::Epsg(1313)).unwrap();
println!("{}\n{}", lambert_93.to_projjson(), nad27_to_nad83.to_projjson());
```

`examples/authority_factory_report.rs` reads lines of `crs <EPSG code>`,
`operation <EPSG code>` or `operation PROJ:<code>` on stdin and prints the factory's
PROJJSON for each, or `error <message>`, each record ending in a nul byte. Its output
equals `projinfo [-k operation] <authority>:<code> -o PROJJSON -q` once the `scope`, `area`
and `remarks` members are dropped from both.

The data is a binary blob generated from PROJ 9.8.1's proj.db (EPSG v12.029) by
`tools/gen-epsg-registry`, a package outside the workspace so that building or testing
projicio never builds SQLite. The generator refuses a proj.db from any other PROJ
release:

```bash
cargo run --manifest-path tools/gen-epsg-registry/Cargo.toml -- --proj-db /usr/share/proj/proj.db
```

It prints each section's size and the blob's size after gzip.
`crates/projicio-core/data/epsg.provenance.json` records the EPSG and PROJ versions it
came from, its checksum and its record counts. CI builds proj.db from the PROJ 9.8.1
release tarball and regenerates the blob with `--check` to prove it still matches byte
for byte.

The WKT method and parameter tables (`src/iso19111/wkt/mappings.rs`) and the ESRI name
table (`data/esri_names.tsv`, 0.9 MB, embedded with the `registry` feature) come from a
PROJ 9.8.1 source checkout and its proj.db:

```bash
uv run tools/gen-wkt-tables/generate.py <PROJ 9.8.1 source> /usr/share/proj/proj.db
```

Turn the `registry` feature off to leave the blob out. The wasm bindings and the CLI
already do, so `projicio info` prints the proj4 definition but no EPSG metadata.

Contains data from the EPSG Dataset, © IOGP, at the version recorded in the provenance
file, obtained through PROJ's proj.db. Terms of use are at
[epsg.org](https://epsg.org/terms-of-use.html).

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
`grids::missing_grids("EPSG:4267")` names what a CRS is still waiting on, and returns an
empty list once it transforms.

Grids the embedded table asks for, by number of codes waiting on them:

| Grid | Codes | Source |
|---|---|---|
| `conus` and friends (NAD27) | 204 | NOAA NGS / the PROJ data package |
| `nzgd2kgrid0005.gsb` (NZGD49) | 33 | Land Information New Zealand |

### In the browser

The wasm build registers grids too, from bytes rather than a path, since there is no file
system to read them from. A grid stays registered for the life of the wasm instance, so
fetch it once per session and every later transform sees it.

```js
import init, { register_grid, missing_grids, transform_coordinates } from "projicio-wasm";

await init();
missing_grids("EPSG:4267"); // ["alaska", "conus", "ntv1_can.dat", "ntv2_0.gsb"]

const gsb = await fetch("/grids/conus").then((r) => r.arrayBuffer());
register_grid("conus", new Uint8Array(gsb));

missing_grids("EPSG:4267"); // []
transform_coordinates("EPSG:4267", "EPSG:4326", new Float64Array([-100, 35]));
```

`registered_grids()` lists what this instance holds. NTv2 parsing on wasm32 comes from a
patched proj4rs, pinned under `[patch.crates-io]` in the workspace `Cargo.toml` until
proj4rs 0.2.0 ships the same change.

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
transformation, which is all that 2D easting and northing work needs. The separate OSGM15
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

Projections within one datum follow the published formulas. The test suite checks them
against IOGP Guidance Note 7-2 worked examples, NOAA NGS datasheets and IGN's published
Lambert-93 constants. Transverse Mercator sums a sixth-order series where PROJ solves
exactly, which differs by 0.8 mm at the edge of a UTM zone at 60 degrees north and more
further out. `from_epsg_method` runs PROJ's own Poder/Engsager algorithm instead.

The parity suite compares transforms against C PROJ's `cs2cs`. CI runs it weekly against
PROJ 9.8.1 (`.github/workflows/parity.yml`), and locally it runs with:

```bash
PROJICIO_CS2CS=/usr/bin/cs2cs cargo test -p projicio-core --test proj_parity -- --ignored
```

`tools/proj-agreement` measures agreement with `cs2cs` over every current EPSG CRS and
operation, see [its README](../tools/proj-agreement/README.md). On its two original scores, `CandidateOperations` with
`convert_allowing_ballpark` lands within 1 cm of `cs2cs` for 86.4% of the CRSs reached from
EPSG:4326 and 68.3% of the operations between geographic 2D CRSs. Most of the remaining
cases need a GeoTIFF grid, or are Helmert and concatenated operations that still differ.

Between two different datums the accuracy depends on what the definition carries: a
registered grid where one is named, otherwise the 7-parameter Helmert shift in the
embedded definition, which is metre-level. A definition with neither applies no shift at
all, and `epsg::support` will still report `Fallback` for it, since that is what the
definition itself asks for.

### PROJ's gie tests

`crates/projicio-core/tests/data` has PROJ's declarative tests: `test/gie` and the IOGP
GIGS 5100 and 5200 series, whose expected values are IOGP's. Parsing matches PROJ's
`gie.cpp`, quirks included, and each case goes through `Transform::new` and `convert`.
Cases between two EPSG CRSs run: `crs_src` with `crs_dst`, in the CRSs' EPSG axis order,
and an `operation` of the form `proj=pipeline step init=epsg:A inv step init=epsg:B`, in
longitude, latitude order. These cases are skipped with a reason:

- any other `operation`, since projicio does not run PROJ strings
- a `require_grid` that is not registered with `grids`, or an EPSG pair whose projicio
  definition names an unregistered grid
- a nonzero height in the accepted or expected coordinate, or a geocentric CRS on either side
- a nonzero time coordinate
- `expect failure errno`, since projicio errors have no PROJ error code

```bash
cargo run --release -p projicio-core --example gie_report -- crates/projicio-core/tests/data
```

prints, per file and for each suite, the cases passed, failed and skipped, the largest
error with its tolerance, and the skip reasons. Errors and tolerances are metres for
geographic output, as the chord between the two points on GRS80, and the CRS's unit
otherwise. `--manifest` prints the passing cases instead, as `gie_passing.txt` lists them, and
`cargo test -p projicio-core --test proj_gie` fails when a listed case stops passing. The
transformation files from 5203 to 5207 state their tolerance as 2.78e-7 m where they mean
that many degrees, so their cases fail here unless the error is under 0.28 micrometres.

## Architecture

```
projicio-core:         projection math, ellipsoids, datum transforms, NTv2, CRS registry, fallback engine, ISO 19111 CRS model and PROJJSON
projicio-epsg-format:  binary layout of the embedded EPSG registry, with typed record decoding
projicio-cli:          command-line interface
projicio-wasm:         WebAssembly bindings, built with wasm-pack build --target web
```

## Supported CRS

Native path, projicio's own projection math:

| Family | EPSG Codes |
|--------|------------|
| WGS84 Geographic | 4326 |
| Web Mercator | 3857 |
| UTM North | 32601-32660 |
| UTM South | 32701-32760 |
| Cassini-Soldner | 30200, 3377-3385, 28191 |
| Hotine Oblique Mercator | 29873, 3376, 6808, 8065 |
| American Polyconic | 5880, 29101 |
| Equal Earth | 8857-8859 |
| Laborde | 8441 |

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

### Projections by EPSG method

`from_epsg_method(method_code, parameters, ellipsoid)` builds a projection from an EPSG
conversion method code and its parameters as `(EPSG parameter code, value)` pairs in
radians, metres and unity. It returns easting and northing in metres before any axis
swap or unit change, with longitudes relative to the base CRS's prime meridian.
`Transform` does not route through it yet.

The math is ported from PROJ 9.8's `src/projections` and follows the method mapping in
PROJ's `conversion.cpp`. It covers 38 of the 43 methods that current EPSG projected CRSs
use: Transverse Mercator (exact Poder/Engsager, also 3D, South Orientated and the
zoned grid system), Lambert Conic Conformal (1SP, 1SP variant B, 2SP, 2SP Belgium,
2SP Michigan), Albers, Cassini-Soldner and its hyperbolic form, Polar Stereographic
A and B, Oblique Stereographic, Hotine Oblique Mercator A and B, Lambert Azimuthal
Equal Area and its spherical form, Mercator A and B, Pseudo Mercator, Krovak and its
north-orientated and modified forms, American Polyconic, Laborde, New Zealand Map Grid,
Lambert Cylindrical Equal Area and its spherical form, Equidistant Cylindrical, Equal
Earth, Azimuthal Equidistant and its modified form, Guam, Local Orthographic and
Colombia Urban.

The other five have no PROJ implementation and return an error: Tunisia Mining Grid
(9816), Lambert Conic Near-Conformal (9817), Lambert Conic Conformal West Orientated
(9826), Bonne South Orientated (9828) and Polar Stereographic variant C (9830). The
`epsg_method` tests compare every implemented method against PROJ 9.8.1's `cs2cs` to
1 mm forward and 1e-9 degrees inverse.

## Datum Transforms

| Method | Description |
|--------|-------------|
| Helmert 7-parameter | 3 translations + 3 rotations + scale factor |
| NTv2 grid shift | Bilinear interpolation from binary grid files |
| Geocentric pipeline | Geodetic → ECEF → Helmert → ECEF → Geodetic |

`Transform` uses the table above. The functions below are not wired into it yet.

### Datum transformations by EPSG method

`transformation_from_epsg_method(method_code, parameters, source_ellipsoid,
target_ellipsoid)` builds a transformation between two geographic CRSs from an EPSG
transformation method code and its `(EPSG parameter code, value)` pairs in SI units:
metres, radians, unity for scale differences, metres, radians and unity per second for
rates, and seconds for the reference epoch (EPSG's year is 31556925.445 s). `forward` and
`inverse` take a `GeodeticCoord` (longitude and latitude in radians, ellipsoidal height in
metres) and a coordinate epoch in decimal years. A time-dependent method raises without
an epoch, the others ignore it. Between geographic 2D CRSs PROJ keeps the input height
and uses the computed one only for 3D CRSs, so a caller transforming 2D coordinates keeps
its own height.

`geocentric_transformation_from_epsg_method(method_code, parameters)` returns the
`Helmert` behind the Helmert and Molodensky-Badekas methods, with `forward` and `inverse`
on geocentric coordinates, for operations between geocentric CRSs.
`GeocentricConversion` converts geodetic to geocentric coordinates and back.

The math is ported from PROJ 9.8's `helmert.cpp`, `cart.cpp`, `affine.cpp` and
`latlong.cpp`, and the method mapping from `singleoperation.cpp`. It covers geocentric
translations (1031, 9603, 1035), position vector (1033, 9606, 1037), coordinate frame
(1032, 9607, 1038), coordinate frame full matrix (1132, 1133, 1140), their time-dependent
forms (1053 to 1058), Molodensky-Badekas (1061, 1062, 1063, 1034, 1039, 9636), longitude
rotation (9601) and geographic 2D offsets (9619). These are all the non-grid methods of
current EPSG operations between geodetic CRSs except the time-specific Helmert methods
1065 and 1066, which PROJ 9.8.1 cannot run either and which return an error. A longitude
rotation uses PROJ's value of a named prime meridian when the offset is within 1e-10
radians of it, as `cs2cs` does. The `datum_transformation` tests compare one operation per
method against `cct` running the pipeline `projinfo` prints, to 1 mm and 1e-9 degrees in
both directions.

### Horizontal grid transformations by EPSG method

`horizontal_shift_grid` is projicio's own grid store, separate from `grids` above, which
`Transform` still uses. `horizontal_shift_grid::register_file(name, path)` and
`register_bytes(name, bytes)` read an NTv2, NTv1 or CTable2 grid, subgrids included, and
raise at once on a file they cannot read. A name registers once and stays for the life of
the process. GeoTIFF grids, the form proj-data ships, raise for now.

`grid_transformation_from_epsg_method(method_code, grid_files)` takes an EPSG method code
and the operation's `(EPSG parameter code, file name)` pairs as the registry holds them,
and returns a transformation with the same `forward` and `inverse` as above. It covers
NTv2 (9615), NTv1 (9614) and NADCON (9613). A grid counts as present when it is registered
under any name `accepted_names(file_name)` lists: for an EPSG file name, PROJ's CDN name,
PROJ's older name and EPSG's own, so `conus` serves EPSG's `conus.las`, and for a PROJ file
name, PROJ's two names. A file proj.db marks as running in reverse, such as `ntf_r93.gsb`
for EPSG's `rgf93_ntf.gsb`, runs in reverse when the operation names EPSG's file, and
forward when it names PROJ's, as PROJ's factory does. A grid nobody
registered raises naming every accepted name, and a point outside every grid raises naming
the grid. NADCON5 (1074) and the IGN geocentric translations (1087) raise, since PROJ has
those grids only as GeoTIFF.

Reading, subgrid selection, bilinear interpolation and the iterative inverse are ported
from PROJ 9.8.1's `grids.cpp`. The `horizontal_shift_grid` tests compare small NTv2, NTv1
and CTable2 grids against `cct` in both directions, and
`horizontal_shift_grid_proj_data` compares EPSG:15940, 1241 and 1312 on copies of their
grids in NTv2, CTable2 and NTv1 form from PROJ 9.8.1's `data/tests`.

## WKT in the ISO 19111 model

`iso19111::Crs::from_wkt` reads WKT1 GDAL, WKT1 ESRI, WKT2 2015 and WKT2 2019, and
`Crs::to_wkt(dialect, layout)` writes them as projinfo 9.8.1 does. projinfo prints WKT1
ESRI on one line, which is `WktLayout::SingleLine`. A WKT1 TOWGS84 reads as a BoundCRS to
WGS 84. An ESRI projected 3D CRS writes as `PROJCS[...],VERTCS[...]` and reads back as a
projected 3D CRS. Vertical, compound, engineering and derived CRSs raise. With the
`registry` feature, EPSG identifiers give a WKT1 geographic CRS its EPSG axes, and ESRI
names map to and from EPSG names. `Transform` still reads WKT through proj4wkt.

## Choosing an operation between two CRSs

`operation_factory::CandidateOperations::new(source, target)` takes two `iso19111::Crs` and
`from_epsg(source_code, target_code)` two EPSG codes. It lists the candidate operations
PROJ 9.8.1's `proj_create_crs_to_crs` builds with no options, ported from
`coordinateoperationfactory.cpp`: registry operations in both directions, PROJ's own
operations between EPSG CRSs, concatenated operations, pivots through other CRSs, the
geographic offset PROJ falls back on, ordered as `filterAndSort` orders them. Known grids
count as available while the list is built. No grid is read yet, so an operation needing one
stays a candidate and is skipped when a point reaches it, as `cs2cs` does with a missing grid.

`convert([x, y, z])` takes and returns coordinates in each CRS's own axis order and units
(EPSG:4326 is latitude first, EPSG:4807 is in grads). It picks the operation `proj_trans`
picks at that point: the most accurate one whose area of use holds the point, the next one
when it fails, and an operation that needs no grid when none applies. `convert` raises
`Error::NoOperation` when that answer is a ballpark one or comes from an operation whose area
does not hold the point. `convert_allowing_ballpark` returns PROJ's answer there, which for
the geographic offset keeps latitude and longitude. A 2D CRS's third coordinate is a height
that a datum shift keeps, as PROJ's push and pop of the height does.

Time-dependent operations are candidates only with `at_epoch(source, target, epoch)`, the
epoch in decimal years. Without one they are left out, where PROJ uses the reference epoch.

`create_operations(source, target, &context)` returns the sorted list for an
`OperationContext`, the subset of PROJ's `CoordinateOperationContext` its tests use.
`examples/candidate_operations_batch.rs` reads `from to x y` lines as
`examples/transform_batch.rs` does and converts with `convert_allowing_ballpark`, since `cs2cs`
answers with a ballpark operation where no other exists, so `tools/proj-agreement/compare.py
--projicio-bin` scores it, and `examples/gie_report.rs --candidate-operations` runs the `crs_src`/`crs_dst` gie cases
on it.

Left out: vertical and compound CRSs, point motion between epochs, CRSs identified by name
only, NKG operations, datum publication dates in the pivot searches, and PROJ's count of
steps in its pipeline string as a sort criterion.
