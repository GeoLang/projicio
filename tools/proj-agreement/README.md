# proj-agreement

Measures projicio against PROJ's `cs2cs` over everything current in proj.db's EPSG data. Each case runs at five points inside its area of use.

Needs PROJ's `cs2cs` and `projinfo` on the path, with the grids you want PROJ to use installed. Python standard library only.

```sh
cargo build --release --example transform_batch -p projicio-core
python3 tools/proj-agreement/compare.py --projicio-bin target/release/examples/transform_batch --out /tmp/proj-agreement
```

## Scores

The full scope has two parts:

- from EPSG:4326 to every current EPSG CRS of every kind (geographic 2D and 3D, geocentric, projected, vertical, compound, engineering), from EPSG:4979 where the target has a height
- every current EPSG operation, from its source CRS to its target CRS

A case passes when, at every point, projicio's answer is within 1 cm of cs2cs's, or both refuse. The distance counts the height where the target has one, is the 3D distance for a geocentric target and the height alone for a vertical target. projicio answering where PROJ refuses fails, and so does refusing where PROJ answers.

Two sub-scores are computed the same way as before the full scope, so their history stays comparable. They report the share of cases within 1 mm, 1 cm, 10 cm and 1 m:

- from EPSG:4326 to every current EPSG geographic 2D and projected CRS
- every current EPSG operation between two geographic 2D CRSs

`summary.txt` has the full scope, both parts broken down by CRS kind, operation method, datum, the datum step PROJ ranks best, and failure reason, then the sub-scores. `crs.tsv` and `operations.tsv` have one row per full-scope case, `from-wgs84.tsv` and `pairs.tsv` one per sub-score case.

## Inputs

A case with a dynamic CRS (a datum with a frame reference epoch) or a time-dependent operation carries the coordinate epoch 2025.0, passed to cs2cs as the fourth input column. Time-dependent operations are the time-dependent and time-specific Helmert methods, velocity grids, point motion, deformation models, and concatenated operations with one of these as a step.

The source height is 100 in the source CRS's height unit, or 0 where only the target has a height. Points in a projected source CRS come from cs2cs between its base geographic CRS and the CRS, points in a geocentric CRS from its ellipsoid, and points in an engineering CRS from cs2cs from the operation's target CRS.

## Batch contract

`transform_batch` reads one line per point and answers one line per point:

```
from to c1 c2 [c3 [epoch]]
```

The answer is `error <reason>` or as many values as coordinates came in. `from` and `to` are CRS strings such as `EPSG:4326`. c1 and c2 are easting before northing, longitude before latitude in degrees, each axis signed as the CRS defines it. c3 is the height in the CRS's height unit, and for a geocentric CRS c1 c2 c3 are X Y Z. For a vertical CRS c1 c2 are the point's longitude and latitude in degrees, and only c3 is compared. A case with an epoch always sends c3, 0 where the source has no height, and the third value back is compared only where the target has a height. Lines with c3 or an epoch get `error`, a refusal, until `Transform` has heights and epochs.

## Refusals

PROJ refuses a point when cs2cs exits non-zero, which happens when PROJ builds no operation for the pair, or prints `*` or `inf` for it. projicio refuses a point with an `error` line.

cs2cs 9.8.1 refuses a vertical source or target CRS outright when PROJ has more than one candidate operation for the pair, so many cases touching a vertical CRS pass only when projicio refuses too. An operation case measures the operation cs2cs picks between the two CRSs, which is not always the operation under test. A point motion operation has the same source and target CRS, and with one epoch cs2cs answers the input unchanged.
