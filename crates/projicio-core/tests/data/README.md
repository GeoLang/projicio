`proj-gie/` is PROJ's `test/gie/` and `gigs/` is its `test/gigs/` (the IOGP GIGS 5100 and 5200 series), copied unchanged from PROJ master 8cf4fde under the MIT licence in `LICENSES/PROJ-MIT.txt`.

`crs_axes.tsv` is the first axis of each EPSG CRS those files name, from projinfo as its header says. `gie_passing.txt` lists cases that pass today, and `tests/proj_gie.rs` fails when one of them stops passing.

`datum_transformation/` holds one operation per EPSG transformation method for `tests/datum_transformation.rs`. `epsg_<code>.input.txt` are points inside the operation's area, written by hand as `latitude longitude height epoch` in the source CRS's units, or for a geocentric CRS converted to `x y z epoch` with `cct -d 4 +proj=pipeline +step +proj=axisswap +order=2,1 +step +proj=unitconvert +xy_in=deg +xy_out=rad +step +proj=cart +ellps=<source ellipsoid>`. The other two files are PROJ 9.8.1's output, with `-d 6` in place of `-d 12` for geocentric CRSs:

    pipeline=$(projinfo -k operation EPSG:<code> -o PROJ -q --single-line)
    cct -d 12 $pipeline < epsg_<code>.input.txt | tee epsg_<code>.cct_forward.txt | cct -I -d 12 $pipeline > epsg_<code>.cct_inverse.txt

`candidate_operations/` serves `tests/candidate_operations.rs`, written with `PROJ_DATA` set to a directory holding only proj.db and `PROJ_NETWORK=OFF`, so PROJ 9.8.1 reads no grid. `projinfo/epsg_<a>_to_epsg_<b>.txt` is `projinfo -s EPSG:<a> -t EPSG:<b> --spatial-test intersects --grid-check known_available --summary`. `cs2cs/<pair>.input` are points written by hand in the source CRS's axis order and units, with an epoch as fourth column where the pair needs one, and `<pair>.cs2cs` is `cs2cs -d 12 EPSG:<a> EPSG:<b> < <pair>.input`. `test_operationfactory/<test>.cpp` are tests cut with `sed -n` from PROJ 9.8.1's `test/unit/test_operationfactory.cpp`, under the MIT licence in `LICENSES/PROJ-MIT.txt`.

`horizontal_shift_grid/` holds the grids and cct output for `tests/horizontal_shift_grid.rs` and `tests/horizontal_shift_grid_proj_data.rs`. `test_hgrid_big_endian.gsb` (NTv2), `nkgrf03vel_realigned_xy_extract.ct2` (CTable2), `ntf_r93.gsb` (NTv2), `conus` (CTable2) and `ntv1_can.dat` (NTv1) are copied unchanged from PROJ 9.8.1's `data/tests`, under the MIT licence in `LICENSES/PROJ-MIT.txt`, which PROJ's `COPYING` extends to its data files. `nonlinear_with_subgrid.gsb` (NTv2 with one subgrid) and `nonlinear_ntv1.dat` are written by `python3 -I write_fixtures.py <directory>`. Each `<name>.input.txt` is hand-written as `latitude longitude height epoch` in degrees, and the cct files are PROJ 9.8.1's output, with no inverse file where the forward output leaves the grid:

    pipeline="+proj=pipeline +step +proj=axisswap +order=2,1 +step +proj=unitconvert +xy_in=deg +xy_out=rad +step +proj=hgridshift +grids=$PWD/<grid> +step +proj=unitconvert +xy_in=rad +xy_out=deg +step +proj=axisswap +order=2,1"
    cct -d 12 -o <name>.cct_forward.txt $pipeline <name>.input.txt
    cct -I -d 12 -o <name>.cct_inverse.txt $pipeline <name>.cct_forward.txt

`epsg_<code>.*` use the pipeline `projinfo -k operation EPSG:<code> -o PROJ -q --single-line` prints, which reads PROJ's GeoTIFF grids from proj-data. The test reads the same grid in its older form (`ntf_r93.gsb`, `conus`, `ntv1_can.dat`), and cct gives the same output for those files to 1e-10 degrees.
