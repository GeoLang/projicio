# proj-agreement

Compares projicio against PROJ on two scores, each case at five points inside its area of use, the case's error being the largest distance between the two answers in metres:

- from EPSG:4326 to every current EPSG geographic 2D and projected CRS
- every current EPSG operation between two geographic 2D CRSs, from its source CRS to its target CRS

Needs PROJ's `cs2cs` and `projinfo` on the path, with the grids you want PROJ to use installed. Python standard library only.

```sh
cargo build --release --example transform_batch -p projicio-core
python3 tools/proj-agreement/compare.py --projicio-bin target/release/examples/transform_batch --out /tmp/proj-agreement
```

`summary.txt` reports, per score, the share of cases within 1 mm, 1 cm, 10 cm and 1 m, what projicio refuses, the kind of datum step PROJ ranks best for the disagreeing cases (Helmert, grid, ballpark and so on), how many of those use a time-dependent operation, and the datums, projection methods or operation kinds with the most disagreements. `from-wgs84.tsv` and `pairs.tsv` have one row per case.

Coordinates are compared with easting before northing, keeping each axis's sign as the CRS defines it. For an operation pair, the datum step column is the operation under test, not PROJ's choice. projicio uses no datum shift grid unless one is registered, so cases that need a grid show up as refused.
