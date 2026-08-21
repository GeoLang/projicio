// Parity against C PROJ, so that a change to projicio's math shows up as drift from a
// reference implementation rather than only against the handful of published worked
// examples the other tests carry.
//
// Every test here is ignored by default and runs only when PROJICIO_CS2CS names a cs2cs
// binary:
//   PROJICIO_CS2CS=/usr/bin/cs2cs cargo test -p projicio-core --test proj_parity -- --ignored
//
// No expected coordinate is written down anywhere in this file. cs2cs is run live and its
// output is the expectation, so the corpus is points and tolerances only.

use projicio_core::{
    AlbersEqualArea, Coord, Ellipsoid, Geographic, LambertAzimuthalEqualArea,
    LambertConformalConic, Mercator, PolarStereographic, Projection, Support, Transform,
    TransverseMercator, WebMercator, epsg,
};
use std::io::Write;
use std::process::{Command, Stdio};

const CS2CS_ENV: &str = "PROJICIO_CS2CS";

/// Fixed point output, wide enough that printing never limits the comparison: 12 decimals
/// of a degree is a nanometre at the equator.
const COORDINATE_FORMAT: &str = "%.12f";

// ═══════════════════════════════════════════════════════════════════════════
// Tolerances
// ═══════════════════════════════════════════════════════════════════════════
//
// Each is sized above the worst disagreement its category actually shows against PROJ
// 9.6, with enough headroom that a rounding difference between PROJ releases or platforms
// cannot turn the suite red on its own. The observed worst is quoted with each.

/// projicio's own closed form projections against PROJ's, in metres. These evaluate the
/// same formulas in f64 and agree to about 1e-8 m.
const NATIVE_PROJECTED_METERS: f64 = 1e-3;

/// Transverse mercator, in metres. Its own constant because projicio sums a sixth order
/// series where PROJ solves exactly, which is the one native projection whose error is
/// visible: 0.8 mm at the edge of a UTM zone at 60 degrees north, growing with distance
/// from the central meridian.
const TRANSVERSE_MERCATOR_METERS: f64 = 5e-3;

/// projicio's own inverse projections against PROJ's, in degrees. 1e-8 degrees is a
/// millimetre of ground distance, and the worst observed is 2e-9, again transverse
/// mercator.
const NATIVE_GEOGRAPHIC_DEGREES: f64 = 1e-8;

/// The fallback engine is proj4rs over the same proj4 definition string, so only the
/// arithmetic can differ and these are tighter than the native tolerances rather than
/// looser. Worst observed is 2e-8 m projecting and 2e-11 degrees inverting.
const FALLBACK_PROJECTED_METERS: f64 = 1e-6;
const FALLBACK_GEOGRAPHIC_DEGREES: f64 = 1e-9;

/// Helmert datum shifts, same two engines and the same reasoning. Worst observed is
/// 1e-9 m and 5e-13 degrees, so the loose part of this category is not the arithmetic
/// but getting PROJ to apply the same parameters at all, see [`cs2cs_definition`].
const DATUM_PROJECTED_METERS: f64 = 1e-6;
const DATUM_GEOGRAPHIC_DEGREES: f64 = 1e-9;

/// Codes projicio builds natively from their embedded definition, in the CRS's own unit
/// rather than metres, since these grids are published in links, chains and feet as well
/// as metres and the comparison happens on the numbers a caller gets back.
///
/// Most of the corpus clears this by five orders of magnitude. What sets it is the
/// handful of grids whose false origin is millions of units: the two engines add that
/// offset at different points in the formula, and what survives the cancellation is a
/// tenth of a millimetre on the Michigan oblique mercator grid, which names 4.35
/// million. That is deterministic rather than version dependent, so ten times headroom
/// is enough here where the other categories get a hundred.
const DEFINITION_PROJECTED_UNITS: f64 = 1e-3;

/// Inverting back to WGS84, in degrees. Looser than the other categories because a 2D
/// transform across a Helmert shift is not symmetric: the return leg starts at height
/// zero on the other datum, where the outbound leg left a height of a few hundred
/// metres. Both engines carry that asymmetry and they do not resolve it identically.
/// Worst observed is 3.2e-9 degrees, a third of a millimetre, on Soldner Berlin.
const DEFINITION_GEOGRAPHIC_DEGREES: f64 = 5e-8;

// ═══════════════════════════════════════════════════════════════════════════
// Running cs2cs
// ═══════════════════════════════════════════════════════════════════════════

/// Transform points through cs2cs, one process for the whole batch.
///
/// Both CRS are given as proj4 strings so the axis order is always x then y. Handing
/// cs2cs an authority code instead would put it in the authority's own axis order, which
/// for every EPSG geographic CRS is latitude first.
fn cs2cs(from: &str, to: &str, points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let binary = std::env::var(CS2CS_ENV)
        .unwrap_or_else(|_| panic!("set {CS2CS_ENV} to the path of a cs2cs binary"));

    let mut child = Command::new(&binary)
        .arg("-f")
        .arg(COORDINATE_FORMAT)
        .args(from.split_whitespace())
        .arg("+to")
        .args(to.split_whitespace())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("could not run {binary}: {e}"));

    let mut stdin = child.stdin.take().expect("stdin was piped");
    for &(x, y) in points {
        writeln!(stdin, "{x:.12} {y:.12}").expect("cs2cs took the input");
    }
    drop(stdin);

    let output = child.wait_with_output().expect("cs2cs finished");
    let stdout = String::from_utf8(output.stdout).expect("cs2cs printed text");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "cs2cs failed for {from:?} -> {to:?}: {stderr}"
    );

    let results: Vec<(f64, f64)> = stdout
        .lines()
        .map(|line| {
            let mut fields = line.split_whitespace();
            let mut coordinate = || -> f64 {
                fields
                    .next()
                    .and_then(|field| field.parse().ok())
                    .unwrap_or_else(|| panic!("cs2cs could not transform a point: {line:?}"))
            };
            (coordinate(), coordinate())
        })
        .collect();

    assert_eq!(
        results.len(),
        points.len(),
        "cs2cs returned {} lines for {} points",
        results.len(),
        points.len()
    );
    results
}

/// Every mismatch found in a category, so one run reports all of them.
#[derive(Default)]
struct Mismatches(Vec<String>);

impl Mismatches {
    fn check(
        &mut self,
        label: &str,
        input: (f64, f64),
        got: (f64, f64),
        want: (f64, f64),
        tolerance: f64,
    ) {
        let (dx, dy) = ((got.0 - want.0).abs(), (got.1 - want.1).abs());
        if dx > tolerance || dy > tolerance {
            self.0.push(format!(
                "{label} at ({}, {}): projicio ({}, {}) cs2cs ({}, {}) off by ({dx:.3e}, {dy:.3e}) tolerance {tolerance:.0e}",
                input.0, input.1, got.0, got.1, want.0, want.1
            ));
        }
    }

    fn assert_empty(self, category: &str) {
        assert!(
            self.0.is_empty(),
            "{} {category} mismatches:\n{}",
            self.0.len(),
            self.0.join("\n")
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// The corpus
// ═══════════════════════════════════════════════════════════════════════════
//
// Four categories:
//   a) every projection projicio implements itself, driven through the `Projection`
//      trait against an equivalent proj4 string, with no datum in play on either side
//   b) EPSG codes the proj4rs fallback engine resolves, spread over projection methods,
//      each paired with a geographic CRS carrying its own datum so the shift cancels and
//      only the projection is under test
//   c) Helmert datum shifts, where the datum is the whole point
//   d) codes projicio builds natively from their embedded proj4 definition, covering
//      each of the projection methods that path added
//
// NTv2 grid shift parity is out of scope: projicio embeds no grid data, so there is
// nothing a runner could compare without fetching a national grid file first.

/// A projection projicio implements natively, and the proj4 string that names the same
/// projection to PROJ. `points` are longitude and latitude in degrees, chosen to include
/// the projection's origin, a point a hair off the central meridian, and a high latitude.
struct NativeCase {
    name: &'static str,
    projected: &'static str,
    geographic: &'static str,
    projection: Box<dyn Projection>,
    points: &'static [(f64, f64)],
    projected_tolerance: f64,
}

const WGS84_GEOGRAPHIC: &str = "+proj=longlat +ellps=WGS84 +no_defs";
const GRS80_GEOGRAPHIC: &str = "+proj=longlat +ellps=GRS80 +no_defs";

fn native_cases() -> Vec<NativeCase> {
    vec![
        NativeCase {
            name: "web mercator",
            // the sphere the projection is defined on is not the WGS84 ellipsoid the
            // latitudes come from, and @null is how a proj4 string says to leave the
            // coordinates alone rather than treat that as a datum change
            projected: "+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 \
                        +k=1 +units=m +nadgrids=@null +wktext +no_defs",
            geographic: "+proj=longlat +datum=WGS84 +no_defs",
            projection: Box::new(WebMercator::new()),
            points: &[
                (0.0, 0.0),
                (-74.006, 40.7128),
                (139.6917, 35.6895),
                (179.999, 84.9),
                (-179.999, -84.9),
                (0.000_000_001, 60.0),
                (18.0686, -33.9249),
            ],
            projected_tolerance: NATIVE_PROJECTED_METERS,
        },
        NativeCase {
            name: "utm zone 18 north",
            projected: "+proj=utm +zone=18 +ellps=WGS84 +units=m +no_defs",
            geographic: WGS84_GEOGRAPHIC,
            projection: Box::new(TransverseMercator::utm(18, true)),
            points: &[
                (-75.0, 0.0),
                (-75.0, 84.0),
                (-73.993322, 40.736553),
                (-77.5, 20.0),
                (-72.5, 60.0),
                (-75.000001, 45.0),
            ],
            projected_tolerance: TRANSVERSE_MERCATOR_METERS,
        },
        NativeCase {
            name: "utm zone 33 south",
            projected: "+proj=utm +zone=33 +south +ellps=WGS84 +units=m +no_defs",
            geographic: WGS84_GEOGRAPHIC,
            projection: Box::new(TransverseMercator::utm(33, false)),
            points: &[
                (15.0, -0.0001),
                (15.0, -80.0),
                (13.4, -34.0),
                (17.5, -60.0),
                (15.000001, -20.0),
            ],
            projected_tolerance: TRANSVERSE_MERCATOR_METERS,
        },
        NativeCase {
            name: "mercator",
            projected: "+proj=merc +lon_0=0 +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
            geographic: WGS84_GEOGRAPHIC,
            projection: Box::new(Mercator::new(Ellipsoid::WGS84, 0.0, 1.0)),
            points: &[
                (0.0, 0.0),
                (10.0, 50.0),
                (-120.0, -40.0),
                (0.000_000_001, 80.0),
                (100.0, 84.0),
                (-179.9, -84.0),
            ],
            projected_tolerance: NATIVE_PROJECTED_METERS,
        },
        NativeCase {
            name: "lambert conformal conic 2sp, california",
            projected: "+proj=lcc +lat_1=34.03333333333333 +lat_2=35.46666666666667 \
                        +lat_0=33.5 +lon_0=-118 +x_0=2000000 +y_0=500000 +ellps=GRS80 \
                        +units=m +no_defs",
            geographic: GRS80_GEOGRAPHIC,
            projection: Box::new(LambertConformalConic::new_2sp(
                Ellipsoid::GRS80,
                34.0 + 2.0 / 60.0,
                35.0 + 28.0 / 60.0,
                33.5,
                -118.0,
                2_000_000.0,
                500_000.0,
            )),
            points: &[
                (-118.0, 33.5),
                (-118.2437, 34.0522),
                (-120.5, 36.5),
                (-116.0, 32.6),
                (-118.000001, 35.0),
                (-121.0, 38.0),
            ],
            projected_tolerance: NATIVE_PROJECTED_METERS,
        },
        NativeCase {
            name: "lambert conformal conic 2sp, europe",
            projected: "+proj=lcc +lat_1=35 +lat_2=65 +lat_0=52 +lon_0=10 +x_0=4000000 \
                        +y_0=2800000 +ellps=GRS80 +units=m +no_defs",
            geographic: GRS80_GEOGRAPHIC,
            projection: Box::new(LambertConformalConic::new_2sp(
                Ellipsoid::GRS80,
                35.0,
                65.0,
                52.0,
                10.0,
                4_000_000.0,
                2_800_000.0,
            )),
            points: &[
                (10.0, 52.0),
                (10.0, 71.0),
                (-9.0, 40.0),
                (30.0, 65.0),
                (10.000001, 60.0),
                (25.0, 35.0),
            ],
            projected_tolerance: NATIVE_PROJECTED_METERS,
        },
        NativeCase {
            name: "lambert azimuthal equal area, europe",
            projected: "+proj=laea +lat_0=52 +lon_0=10 +x_0=4321000 +y_0=3210000 \
                        +ellps=GRS80 +units=m +no_defs",
            geographic: GRS80_GEOGRAPHIC,
            projection: Box::new(LambertAzimuthalEqualArea::new(
                Ellipsoid::GRS80,
                52.0,
                10.0,
                4_321_000.0,
                3_210_000.0,
            )),
            points: &[
                (10.0, 52.0),
                (5.0, 50.0),
                (-9.0, 40.0),
                (30.0, 65.0),
                (10.000001, 60.0),
                (25.0, 71.0),
            ],
            projected_tolerance: NATIVE_PROJECTED_METERS,
        },
        NativeCase {
            name: "albers equal area, conus",
            projected: "+proj=aea +lat_1=29.5 +lat_2=45.5 +lat_0=23 +lon_0=-96 +x_0=0 \
                        +y_0=0 +ellps=GRS80 +units=m +no_defs",
            geographic: GRS80_GEOGRAPHIC,
            projection: Box::new(AlbersEqualArea::new(
                Ellipsoid::GRS80,
                29.5,
                45.5,
                23.0,
                -96.0,
            )),
            points: &[
                (-96.0, 23.0),
                (-96.0, 60.0),
                (-120.0, 35.0),
                (-70.0, 45.0),
                (-96.000001, 49.0),
                (-100.0, 30.0),
            ],
            projected_tolerance: NATIVE_PROJECTED_METERS,
        },
        NativeCase {
            name: "polar stereographic, north",
            projected: "+proj=stere +lat_0=90 +lon_0=0 +k_0=0.994 +x_0=0 +y_0=0 \
                        +ellps=WGS84 +units=m +no_defs",
            geographic: WGS84_GEOGRAPHIC,
            projection: Box::new(PolarStereographic::north(Ellipsoid::WGS84)),
            points: &[
                (0.0, 89.999999),
                (0.0, 60.0),
                (44.0, 73.0),
                (-123.0, 66.0),
                (180.0, 75.0),
                (-45.0, 85.0),
            ],
            projected_tolerance: NATIVE_PROJECTED_METERS,
        },
        NativeCase {
            name: "polar stereographic, south",
            projected: "+proj=stere +lat_0=-90 +lon_0=0 +k_0=0.994 +x_0=0 +y_0=0 \
                        +ellps=WGS84 +units=m +no_defs",
            geographic: WGS84_GEOGRAPHIC,
            projection: Box::new(PolarStereographic::south(Ellipsoid::WGS84)),
            points: &[
                (0.0, -89.999999),
                (0.0, -60.0),
                (44.0, -73.0),
                (-123.0, -66.0),
                (180.0, -75.0),
                (-45.0, -85.0),
            ],
            projected_tolerance: NATIVE_PROJECTED_METERS,
        },
    ]
}

/// An EPSG code and points inside its area of use, longitude and latitude in degrees.
struct CodeCase {
    code: u32,
    points: &'static [(f64, f64)],
    /// The string to hand cs2cs in place of the embedded definition. Needed only where
    /// the definition names a datum rather than spelling out its shift, see
    /// [`cs2cs_definition`].
    cs2cs_override: Option<&'static str>,
}

/// Fallback engine codes, one per projection method proj4rs resolves, plus a unit
/// conversion (2263 is US survey feet).
const FALLBACK_CASES: &[CodeCase] = &[
    CodeCase {
        code: 3035, // laea, ETRS89 Europe
        points: &[(10.0, 52.0), (-9.0, 40.0), (30.0, 65.0), (25.0, 71.0)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 2154, // lcc, Lambert 93
        points: &[(3.0, 46.5), (2.3522, 48.8566), (-4.5, 48.4), (7.75, 43.7)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3395, // merc, World Mercator
        points: &[(0.0, 0.0), (100.0, 50.0), (-70.0, -40.0), (12.0, 80.0)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 5070, // aea, NAD83 Conus Albers
        points: &[(-96.0, 23.0), (-122.4, 37.8), (-71.0, 42.4), (-100.0, 45.0)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 32661, // stere, UPS North
        points: &[(0.0, 89.0), (30.0, 70.0), (-120.0, 75.0), (180.0, 65.0)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3031, // stere with a standard parallel, Antarctic
        points: &[(0.0, -80.0), (100.0, -70.0), (-60.0, -75.0), (0.0, -89.0)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 2193, // tmerc, New Zealand
        points: &[
            (173.0, -41.0),
            (174.7762, -41.2865),
            (170.5, -45.87),
            (176.5, -38.0),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 26918, // utm north, NAD83
        points: &[(-75.0, 0.0), (-73.99, 40.74), (-77.0, 20.0), (-72.5, 60.0)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 25832, // utm north, ETRS89
        points: &[(9.0, 0.0), (9.99, 53.55), (11.58, 48.14), (7.0, 60.0)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 28992, // sterea, oblique stereographic
        points: &[
            (5.3876, 52.1562),
            (4.8952, 52.3702),
            (6.5, 53.2),
            (5.7, 50.85),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 2263, // lcc in US survey feet
        points: &[
            (-74.0, 40.1667),
            (-73.9857, 40.7484),
            (-72.5, 41.0),
            (-74.5, 40.5),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3005, // aea, British Columbia
        points: &[
            (-126.0, 45.0),
            (-123.12, 49.28),
            (-120.0, 54.0),
            (-131.0, 54.0),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 21781, // somerc, oblique mercator on a Bessel ellipsoid
        points: &[
            (7.4396, 46.9524),
            (8.5417, 47.3769),
            (6.14, 46.2),
            (9.5, 46.5),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 27700, // tmerc on Airy
        points: &[
            (-2.0, 49.0),
            (-0.1275, 51.5072),
            (-3.2, 55.95),
            (-5.0, 58.0),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3006, // utm, SWEREF99 TM
        points: &[(15.0, 0.0), (18.07, 59.33), (13.0, 55.6), (20.0, 67.0)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 2039, // tmerc with a scale factor above one
        points: &[
            (35.2045, 31.7344),
            (34.78, 32.08),
            (35.2, 31.77),
            (34.9, 29.55),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 31370, // lcc with a pole as its latitude of origin
        points: &[(4.3675, 50.85), (3.72, 51.05), (5.57, 50.63), (4.4, 51.2)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 4087, // eqc, plate carree
        points: &[(0.0, 0.0), (100.0, 50.0), (-70.0, -40.0), (10.0, 80.0)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3577, // aea in the southern hemisphere
        points: &[
            (132.0, -25.0),
            (151.2, -33.87),
            (115.86, -31.95),
            (145.0, -38.0),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3416, // lcc, Austria
        points: &[(13.3333, 47.5), (16.37, 48.21), (11.5, 47.3), (15.0, 46.8)],
        cs2cs_override: None,
    },
];

/// Codes whose definition carries a Helmert shift, transformed against WGS84 so the
/// shift is what is being compared. Three and seven parameter forms, geographic and
/// projected, on four ellipsoids.
const DATUM_CASES: &[CodeCase] = &[
    CodeCase {
        code: 4230, // ED50, three parameter
        points: &[
            (2.3522, 48.8566),
            (12.4964, 41.9028),
            (-3.7038, 40.4168),
            (10.0, 60.0),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 23032, // ED50 / UTM zone 32N, the same shift under a projection
        points: &[(9.0, 50.0), (7.0, 52.0), (11.0, 45.0), (9.0, 60.0)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 4277, // OSGB 1936, seven parameter on Airy
        points: &[
            (-0.1275, 51.5072),
            (-3.2, 55.95),
            (-5.0, 58.0),
            (-2.0, 49.0),
        ],
        cs2cs_override: Some(
            "+proj=longlat +ellps=airy \
             +towgs84=446.448,-125.157,542.060,0.1502,0.2470,0.8421,-20.4894 +no_defs",
        ),
    },
    CodeCase {
        code: 27700, // British National Grid, the same shift under a projection
        points: &[
            (-0.1275, 51.5072),
            (-3.2, 55.95),
            (-5.0, 58.0),
            (-2.0, 49.0),
        ],
        cs2cs_override: Some(
            "+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +x_0=400000 +y_0=-100000 \
             +ellps=airy +towgs84=446.448,-125.157,542.060,0.1502,0.2470,0.8421,-20.4894 \
             +units=m +no_defs",
        ),
    },
    CodeCase {
        code: 4314, // DHDN, seven parameter on Bessel
        points: &[(13.4, 52.52), (11.5, 50.5), (9.0, 51.0), (12.0, 54.0)],
        cs2cs_override: Some(
            "+proj=longlat +ellps=bessel \
             +towgs84=598.1,73.7,418.2,0.202,0.045,-2.455,6.7 +no_defs",
        ),
    },
    CodeCase {
        code: 31467, // DHDN / Gauss-Kruger zone 3
        points: &[(9.0, 52.52), (11.5, 50.5), (9.0, 51.0), (10.0, 54.0)],
        cs2cs_override: Some(
            "+proj=tmerc +lat_0=0 +lon_0=9 +k=1 +x_0=3500000 +y_0=0 +ellps=bessel \
             +towgs84=598.1,73.7,418.2,0.202,0.045,-2.455,6.7 +units=m +no_defs",
        ),
    },
    CodeCase {
        code: 4289, // Amersfoort, seven parameter on Bessel
        points: &[(4.895, 52.37), (5.4, 51.9), (6.5, 53.2), (5.7, 50.85)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 28992, // Amersfoort / RD New, the same shift under a projection
        points: &[(4.895, 52.37), (5.4, 51.9), (6.5, 53.2), (5.7, 50.85)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 4149, // CH1903, three parameter on Bessel
        points: &[(7.44, 46.95), (8.54, 47.38), (6.14, 46.2), (9.5, 46.5)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 21781, // CH1903 / LV03, the same shift under a projection
        points: &[(7.44, 46.95), (8.54, 47.38), (6.14, 46.2), (9.5, 46.5)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 4313, // Belge 1972, seven parameter on International 1924
        points: &[(4.3675, 50.85), (3.72, 51.05), (5.57, 50.63), (4.4, 51.2)],
        cs2cs_override: None,
    },
    CodeCase {
        code: 4141, // Israel 1993, three parameter on GRS80
        points: &[(35.2, 31.77), (34.78, 32.08), (34.9, 29.55), (35.5, 33.0)],
        cs2cs_override: None,
    },
];

/// Codes projicio projects with its own math built from their embedded definition,
/// several per projection method the definition path implements, with points inside each
/// code's area of use. These are compared against WGS84 rather than against their own
/// datum, because that is the pair a caller writes and it puts the hub, the unit and the
/// datum shift in the comparison alongside the projection.
///
/// The full set of 64 codes is pinned by tests/native_from_definition.rs. This is a
/// spread over the five methods and over the shapes a definition comes in: three and
/// seven parameter shifts, no datum at all, and links, chains, feet and metres.
const DEFINITION_CASES: &[CodeCase] = &[
    // Cassini-Soldner
    CodeCase {
        code: 30200, // Trinidad 1903 / Trinidad Grid, in Clarke links
        points: &[
            (-61.3333, 10.4417),
            (-61.5189, 10.6518),
            (-61.0, 10.3),
            (-60.95, 10.6),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 24500, // Kertau 1968 / Singapore Grid
        points: &[
            (103.853, 1.2876),
            (103.8198, 1.3521),
            (103.95, 1.32),
            (103.7, 1.25),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3140, // Viti Levu 1912 / Viti Levu Grid, in links
        points: &[
            (178.4419, -18.1416),
            (177.4529, -17.6100),
            (177.4356, -17.7765),
            (178.0, -17.8),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 28191, // Palestine 1923 / Palestine Grid, seven parameter shift
        points: &[
            (35.2121, 31.7341),
            (34.7818, 32.0853),
            (35.0, 31.5),
            (35.3, 32.5),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 2099, // Qatar 1948 / Qatar Grid, on the Helmert 1906 ellipsoid, no shift
        points: &[
            (51.5310, 25.2854),
            (50.85, 25.5),
            (51.2, 24.9),
            (51.0, 26.0),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3407, // Hong Kong 1963 Grid System, in Clarke feet
        points: &[
            (114.1786, 22.3121),
            (114.0, 22.4),
            (114.25, 22.25),
            (113.95, 22.28),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3377, // GDM2000 / Johor Grid, a definition naming no datum
        points: &[
            (103.4279, 2.1217),
            (103.7618, 1.4927),
            (102.9, 2.3),
            (103.2, 1.9),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3068, // DHDN / Soldner Berlin, see cs2cs_definition for the override
        points: &[
            (13.6272, 52.4186),
            (13.4, 52.52),
            (13.2, 52.6),
            (13.75, 52.35),
        ],
        cs2cs_override: Some(
            "+proj=cass +lat_0=52.41864827777778 +lon_0=13.62720366666667 +x_0=40000 \
             +y_0=10000 +ellps=bessel +towgs84=598.1,73.7,418.2,0.202,0.045,-2.455,6.7 \
             +units=m +no_defs",
        ),
    },
    // Hotine oblique mercator
    CodeCase {
        code: 29873, // Timbalai 1948 / RSO Borneo, offsets at the projection centre
        points: &[
            (115.0, 4.0),
            (116.0724, 5.9804),
            (110.3592, 1.5533),
            (117.89, 4.25),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3376, // GDM2000 / East Malaysia BRSO, offsets at the natural origin
        points: &[
            (115.0, 4.0),
            (116.0724, 5.9804),
            (110.3592, 1.5533),
            (117.89, 4.25),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3375, // GDM2000 / Peninsula RSO
        points: &[
            (102.25, 4.0),
            (101.6869, 3.1390),
            (100.3327, 5.4141),
            (103.3333, 3.8077),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3167, // Kertau / RSO Malaya, in chains
        points: &[
            (102.25, 4.0),
            (101.6869, 3.1390),
            (100.3327, 5.4141),
            (103.3333, 3.8077),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 3078, // NAD83 / Michigan Oblique Mercator, see cs2cs_definition
        points: &[
            (-86.0, 45.3092),
            (-83.0458, 42.3314),
            (-87.3954, 46.5436),
            (-85.6, 44.8),
        ],
        cs2cs_override: Some(
            "+proj=omerc +lat_0=45.30916666666666 +lonc=-86 +alpha=337.25556 +k=0.9996 \
             +x_0=2546731.496 +y_0=-4354009.816 +no_uoff +gamma=337.25556 +ellps=GRS80 \
             +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
        ),
    },
    CodeCase {
        code: 3468, // NAD83(NSRS2007) / Alaska zone 1
        points: &[
            (-133.6667, 57.0),
            (-134.4197, 58.3019),
            (-131.6461, 55.3422),
            (-135.3139, 59.4583),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 6840, // NAD83(CORS96) / Oregon Coast zone, metres
        points: &[
            (-124.05, 44.75),
            (-124.0535, 44.6368),
            (-123.9, 46.0),
            (-124.2, 43.4),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 6841, // the same zone in international feet
        points: &[
            (-124.05, 44.75),
            (-124.0535, 44.6368),
            (-123.9, 46.0),
            (-124.2, 43.4),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 2057, // Rassadiran / Nakhl-e Taqi, a near zero azimuth on International 1924
        points: &[
            (52.6035, 27.5188),
            (52.75, 27.4),
            (52.5, 27.65),
            (52.6, 27.6),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 8065, // NAD83(2011) / PCCS zone 1, a scale factor above one, in feet
        points: &[
            (-111.4, 32.25),
            (-110.9747, 32.2226),
            (-111.8, 31.9),
            (-111.0, 32.6),
        ],
        cs2cs_override: None,
    },
    // American polyconic
    CodeCase {
        code: 29101, // SAD69 / Brazil Polyconic
        points: &[
            (-54.0, 0.0),
            (-47.8825, -15.7942),
            (-43.1729, -22.9068),
            (-60.0217, -3.1019),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 5880, // SIRGAS 2000 / Brazil Polyconic
        points: &[
            (-54.0, 0.0),
            (-47.8825, -15.7942),
            (-38.5014, -12.9777),
            (-60.0217, -3.1019),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 5530, // SAD69(96) / Brazil Polyconic, a different three parameter shift
        points: &[
            (-54.0, 0.0),
            (-47.8825, -15.7942),
            (-43.1729, -22.9068),
            (-51.2177, -30.0346),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 5472, // Panama-Colon 1911 / Panama Polyconic, in Panamanian feet
        points: &[
            (-81.0, 8.25),
            (-79.5199, 8.9824),
            (-82.4, 8.4),
            (-80.0, 9.0),
        ],
        cs2cs_override: None,
    },
    // Equal earth, all three of them since the method has only three codes
    CodeCase {
        code: 8857, // Equal Earth Greenwich
        points: &[
            (0.0, 0.0),
            (12.4964, 41.9028),
            (-58.3816, -34.6037),
            (100.0, 60.0),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 8858, // Equal Earth Americas
        points: &[
            (-90.0, 0.0),
            (-74.006, 40.7128),
            (-58.3816, -34.6037),
            (-122.4194, 37.7749),
        ],
        cs2cs_override: None,
    },
    CodeCase {
        code: 8859, // Equal Earth Asia Pacific
        points: &[
            (150.0, 0.0),
            (139.6917, 35.6895),
            (174.7762, -41.2865),
            (120.0, 30.0),
        ],
        cs2cs_override: None,
    },
    // Laborde, the one code the method has
    CodeCase {
        code: 8441, // Tananarive / Laborde Grid
        points: &[
            (46.4372, -18.9),
            (47.5162, -18.8792),
            (49.4023, -18.1492),
            (44.2833, -20.2833),
        ],
        cs2cs_override: None,
    },
];

/// The proj4 definition projicio would use for a code.
fn definition(code: u32) -> &'static str {
    epsg::proj4_definition(code).unwrap_or_else(|| panic!("no embedded definition for EPSG:{code}"))
}

/// What to hand cs2cs for a case, which is projicio's own definition unless the case
/// overrides it.
///
/// The override exists because a proj4 string carrying an explicit `+towgs84=` becomes a
/// bound CRS and PROJ applies exactly those parameters, while one naming a datum such as
/// `+datum=OSGB36` becomes a plain CRS and PROJ picks its own best operation from the
/// EPSG database, which for OSGB36, Potsdam and NAD83 is an NTv2 grid. Writing the shift
/// out puts both engines on the same parameters, which is what this file is comparing.
/// Where an override is present it names the same parameters projicio reads out of
/// `+datum=`, so if either datum table changes the test says so.
fn cs2cs_definition(case: &CodeCase) -> &'static str {
    case.cs2cs_override.unwrap_or_else(|| definition(case.code))
}

/// Parameters that decide where a coordinate sits rather than how it is projected.
const DATUM_PARAMETERS: &[&str] = &[
    "+ellps=",
    "+a=",
    "+b=",
    "+rf=",
    "+f=",
    "+datum=",
    "+towgs84=",
    "+nadgrids=",
    "+pm=",
];

/// The geographic CRS a definition projects from: same ellipsoid, same datum, no
/// projection. Pairing a code with this cancels the datum shift, leaving the projection
/// as the only thing that can differ.
fn geographic_base(definition: &str) -> String {
    let mut parts = vec!["+proj=longlat".to_string()];
    parts.extend(
        definition
            .split_whitespace()
            .filter(|token| DATUM_PARAMETERS.iter().any(|p| token.starts_with(p)))
            .map(str::to_string),
    );
    parts.push("+no_defs".to_string());
    parts.join(" ")
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

/// The invocation this whole file rests on, checked against a point that can be verified
/// without either engine: the UTM zone 18N coordinate of lower Manhattan.
#[test]
#[ignore = "needs a cs2cs binary, set PROJICIO_CS2CS"]
fn test_cs2cs_invocation_agrees_on_a_known_point() {
    let utm = "+proj=utm +zone=18 +ellps=WGS84 +units=m +no_defs";
    let [(lon, lat)] = cs2cs(utm, WGS84_GEOGRAPHIC, &[(585_000.0, 4_510_000.0)])[..] else {
        panic!("expected one point back");
    };
    assert!((lon - (-73.9933)).abs() < 1e-3, "longitude {lon}");
    assert!((lat - 40.7366).abs() < 1e-3, "latitude {lat}");
}

#[test]
#[ignore = "needs a cs2cs binary, set PROJICIO_CS2CS"]
fn test_native_projections_match_cs2cs() {
    let mut mismatches = Mismatches::default();

    for case in native_cases() {
        let expected = cs2cs(case.geographic, case.projected, case.points);
        for (&geographic, &want) in case.points.iter().zip(&expected) {
            let projected = case
                .projection
                .forward(Geographic::new(geographic.0, geographic.1))
                .unwrap_or_else(|e| panic!("{} forward failed: {e}", case.name));
            mismatches.check(
                &format!("{} forward", case.name),
                geographic,
                (projected.x, projected.y),
                want,
                case.projected_tolerance,
            );
        }

        // Invert what cs2cs produced, so both engines start from the same planar point.
        let recovered = cs2cs(case.projected, case.geographic, &expected);
        for (&planar, &want) in expected.iter().zip(&recovered) {
            let geographic = case
                .projection
                .inverse(Coord::new(planar.0, planar.1))
                .unwrap_or_else(|e| panic!("{} inverse failed: {e}", case.name));
            mismatches.check(
                &format!("{} inverse", case.name),
                planar,
                (geographic.lon, geographic.lat),
                want,
                NATIVE_GEOGRAPHIC_DEGREES,
            );
        }
    }

    mismatches.assert_empty("native projection");
}

#[test]
#[ignore = "needs a cs2cs binary, set PROJICIO_CS2CS"]
fn test_fallback_codes_match_cs2cs() {
    let mut mismatches = Mismatches::default();

    for case in FALLBACK_CASES {
        let code = case.code;
        let target = cs2cs_definition(case);
        let source = geographic_base(target);

        let forward = Transform::new(&source, &format!("EPSG:{code}"))
            .unwrap_or_else(|e| panic!("EPSG:{code} forward transform: {e}"));
        let expected = cs2cs(&source, target, case.points);
        for (&geographic, &want) in case.points.iter().zip(&expected) {
            let got = forward
                .convert(geographic.0, geographic.1)
                .unwrap_or_else(|e| panic!("EPSG:{code} forward: {e}"));
            mismatches.check(
                &format!("EPSG:{code} forward"),
                geographic,
                got,
                want,
                FALLBACK_PROJECTED_METERS,
            );
        }

        let inverse = Transform::new(&format!("EPSG:{code}"), &source)
            .unwrap_or_else(|e| panic!("EPSG:{code} inverse transform: {e}"));
        let recovered = cs2cs(target, &source, &expected);
        for (&planar, &want) in expected.iter().zip(&recovered) {
            let got = inverse
                .convert(planar.0, planar.1)
                .unwrap_or_else(|e| panic!("EPSG:{code} inverse: {e}"));
            mismatches.check(
                &format!("EPSG:{code} inverse"),
                planar,
                got,
                want,
                FALLBACK_GEOGRAPHIC_DEGREES,
            );
        }
    }

    mismatches.assert_empty("fallback code");
}

#[test]
#[ignore = "needs a cs2cs binary, set PROJICIO_CS2CS"]
fn test_datum_shifts_match_cs2cs() {
    let mut mismatches = Mismatches::default();
    let wgs84 = definition(4326);

    for case in DATUM_CASES {
        let code = case.code;
        let target = cs2cs_definition(case);
        let forward_tolerance = if target.contains("+proj=longlat") {
            DATUM_GEOGRAPHIC_DEGREES
        } else {
            DATUM_PROJECTED_METERS
        };

        let forward = Transform::new("EPSG:4326", &format!("EPSG:{code}"))
            .unwrap_or_else(|e| panic!("EPSG:{code} forward transform: {e}"));
        let expected = cs2cs(wgs84, target, case.points);
        for (&geographic, &want) in case.points.iter().zip(&expected) {
            let got = forward
                .convert(geographic.0, geographic.1)
                .unwrap_or_else(|e| panic!("EPSG:{code} forward: {e}"));
            mismatches.check(
                &format!("EPSG:{code} from WGS84"),
                geographic,
                got,
                want,
                forward_tolerance,
            );
        }

        let inverse = Transform::new(&format!("EPSG:{code}"), "EPSG:4326")
            .unwrap_or_else(|e| panic!("EPSG:{code} inverse transform: {e}"));
        let recovered = cs2cs(target, wgs84, &expected);
        for (&point, &want) in expected.iter().zip(&recovered) {
            let got = inverse
                .convert(point.0, point.1)
                .unwrap_or_else(|e| panic!("EPSG:{code} inverse: {e}"));
            mismatches.check(
                &format!("EPSG:{code} to WGS84"),
                point,
                got,
                want,
                DATUM_GEOGRAPHIC_DEGREES,
            );
        }
    }

    mismatches.assert_empty("datum shift");
}

#[test]
#[ignore = "needs a cs2cs binary, set PROJICIO_CS2CS"]
fn test_definition_built_codes_match_cs2cs() {
    let mut mismatches = Mismatches::default();
    let wgs84 = definition(4326);

    for case in DEFINITION_CASES {
        let code = case.code;
        let target = cs2cs_definition(case);

        let forward = Transform::new("EPSG:4326", &format!("EPSG:{code}"))
            .unwrap_or_else(|e| panic!("EPSG:{code} forward transform: {e}"));
        let inverse = Transform::new(&format!("EPSG:{code}"), "EPSG:4326")
            .unwrap_or_else(|e| panic!("EPSG:{code} inverse transform: {e}"));
        // The point of this category is projicio's own math, so a pair that quietly
        // dropped to proj4rs would still pass while testing nothing it claims to.
        for (direction, transform) in [("from", &forward), ("to", &inverse)] {
            assert_eq!(
                transform.path(),
                Support::Native,
                "EPSG:{code} {direction} WGS84 left the native path"
            );
        }

        let expected = cs2cs(wgs84, target, case.points);
        for (&geographic, &want) in case.points.iter().zip(&expected) {
            let got = forward
                .convert(geographic.0, geographic.1)
                .unwrap_or_else(|e| panic!("EPSG:{code} forward: {e}"));
            mismatches.check(
                &format!("EPSG:{code} from WGS84"),
                geographic,
                got,
                want,
                DEFINITION_PROJECTED_UNITS,
            );
        }

        let recovered = cs2cs(target, wgs84, &expected);
        for (&point, &want) in expected.iter().zip(&recovered) {
            let got = inverse
                .convert(point.0, point.1)
                .unwrap_or_else(|e| panic!("EPSG:{code} inverse: {e}"));
            mismatches.check(
                &format!("EPSG:{code} to WGS84"),
                point,
                got,
                want,
                DEFINITION_GEOGRAPHIC_DEGREES,
            );
        }
    }

    mismatches.assert_empty("definition built code");
}
