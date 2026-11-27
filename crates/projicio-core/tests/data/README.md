`proj-gie/` is PROJ's `test/gie/` and `gigs/` is its `test/gigs/` (the IOGP GIGS 5100 and 5200 series), copied unchanged from PROJ master 8cf4fde under the MIT licence in `LICENSES/PROJ-MIT.txt`.

`crs_axes.tsv` is the first axis of each EPSG CRS those files name, from projinfo as its header says. `gie_passing.txt` lists cases that pass today, and `tests/proj_gie.rs` fails when one of them stops passing.
