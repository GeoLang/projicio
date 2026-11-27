`proj-gie/` is PROJ's `test/gie/` and `gigs/` is its `test/gigs/` (the IOGP GIGS 5100 and 5200 series), copied unchanged from PROJ master 8cf4fde under the MIT licence in `LICENSES/PROJ-MIT.txt`.

`crs_axes.tsv` is the first axis of each EPSG CRS those files name, from projinfo as its header says. `gie_passing.txt` lists cases that pass today, and `tests/proj_gie.rs` fails when one of them stops passing.

`datum_transformation/` holds one operation per EPSG transformation method for `tests/datum_transformation.rs`. `epsg_<code>.input.txt` are points inside the operation's area, written by hand as `latitude longitude height epoch` in the source CRS's units, or for a geocentric CRS converted to `x y z epoch` with `cct -d 4 +proj=pipeline +step +proj=axisswap +order=2,1 +step +proj=unitconvert +xy_in=deg +xy_out=rad +step +proj=cart +ellps=<source ellipsoid>`. The other two files are PROJ 9.8.1's output, with `-d 6` in place of `-d 12` for geocentric CRSs:

    pipeline=$(projinfo -k operation EPSG:<code> -o PROJ -q --single-line)
    cct -d 12 $pipeline < epsg_<code>.input.txt | tee epsg_<code>.cct_forward.txt | cct -I -d 12 $pipeline > epsg_<code>.cct_inverse.txt
