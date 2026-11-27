use projicio_core::{Coord, Ellipsoid, Geographic, Projection, from_epsg_method};

const LATITUDE_OF_NATURAL_ORIGIN: u32 = 8801;
const LONGITUDE_OF_NATURAL_ORIGIN: u32 = 8802;
const SCALE_FACTOR_AT_NATURAL_ORIGIN: u32 = 8805;
const FALSE_EASTING: u32 = 8806;
const FALSE_NORTHING: u32 = 8807;
const LATITUDE_OF_PROJECTION_CENTRE: u32 = 8811;
const LONGITUDE_OF_PROJECTION_CENTRE: u32 = 8812;
const AZIMUTH_AT_PROJECTION_CENTRE: u32 = 8813;
const ANGLE_FROM_RECTIFIED_TO_SKEW_GRID: u32 = 8814;
const SCALE_FACTOR_AT_PROJECTION_CENTRE: u32 = 8815;
const EASTING_AT_PROJECTION_CENTRE: u32 = 8816;
const NORTHING_AT_PROJECTION_CENTRE: u32 = 8817;
const LATITUDE_OF_PSEUDO_STANDARD_PARALLEL: u32 = 8818;
const SCALE_FACTOR_ON_PSEUDO_STANDARD_PARALLEL: u32 = 8819;
const LATITUDE_OF_FALSE_ORIGIN: u32 = 8821;
const LONGITUDE_OF_FALSE_ORIGIN: u32 = 8822;
const LATITUDE_OF_FIRST_STANDARD_PARALLEL: u32 = 8823;
const LATITUDE_OF_SECOND_STANDARD_PARALLEL: u32 = 8824;
const EASTING_AT_FALSE_ORIGIN: u32 = 8826;
const NORTHING_AT_FALSE_ORIGIN: u32 = 8827;
const INITIAL_LONGITUDE: u32 = 8830;
const ZONE_WIDTH: u32 = 8831;
const LATITUDE_OF_STANDARD_PARALLEL: u32 = 8832;
const LONGITUDE_OF_ORIGIN: u32 = 8833;
const COLATITUDE_OF_CONE_AXIS: u32 = 1036;
const ELLIPSOID_SCALING_FACTOR: u32 = 1038;
const PROJECTION_PLANE_ORIGIN_HEIGHT: u32 = 1039;

// PROJ's us-ft and link units
const US_SURVEY_FOOT: f64 = 1200.0 / 3937.0;
const LINK: f64 = 0.201168;

const WGS84: Ellipsoid = Ellipsoid::WGS84;
const GRS80: Ellipsoid = Ellipsoid::GRS80;
const INTERNATIONAL_1924: Ellipsoid = Ellipsoid::INTERNATIONAL_1924;
const BESSEL_1841: Ellipsoid = Ellipsoid::new(6_377_397.155, 1.0 / 299.152_812_8);
const CLARKE_1866: Ellipsoid =
    Ellipsoid::new(6_378_206.4, (6_378_206.4 - 6_356_583.8) / 6_378_206.4);

#[derive(Clone, Copy)]
enum AxisOrder {
    EastNorth,
    NorthEast,
    WestSouth,
    SouthWest,
}

#[derive(Clone, Copy)]
struct PrintedAxes {
    order: AxisOrder,
    metres_per_unit: f64,
}

const EAST_NORTH: PrintedAxes = PrintedAxes {
    order: AxisOrder::EastNorth,
    metres_per_unit: 1.0,
};
const NORTH_EAST: PrintedAxes = PrintedAxes {
    order: AxisOrder::NorthEast,
    metres_per_unit: 1.0,
};

fn printed_to_natural(axes: PrintedAxes, first: f64, second: f64) -> Coord {
    let (easting, northing) = match axes.order {
        AxisOrder::EastNorth => (first, second),
        AxisOrder::NorthEast => (second, first),
        AxisOrder::WestSouth => (-first, -second),
        AxisOrder::SouthWest => (-second, -first),
    };
    Coord::new(
        easting * axes.metres_per_unit,
        northing * axes.metres_per_unit,
    )
}

fn check(
    method_code: u32,
    parameters: &[(u32, f64)],
    ellipsoid: Ellipsoid,
    axes: PrintedAxes,
    forward: &[[f64; 4]],
    inverse: &[[f64; 4]],
) {
    let projection: Box<dyn Projection + Send + Sync> =
        from_epsg_method(method_code, parameters, &ellipsoid).unwrap();
    // cs2cs rows hold its input then its output, latitude first on the geographic side
    for (row, &[latitude, longitude, first, second]) in forward.iter().enumerate() {
        let expected = printed_to_natural(axes, first, second);
        let projected = projection
            .forward(Geographic::new(longitude, latitude))
            .unwrap();
        assert!(
            (projected.x - expected.x).abs() < 1e-3 && (projected.y - expected.y).abs() < 1e-3,
            "method {method_code} forward row {row}: got {projected:?}, expected {expected:?}"
        );
    }
    for (row, &[first, second, latitude, longitude]) in inverse.iter().enumerate() {
        let geographic = projection
            .inverse(printed_to_natural(axes, first, second))
            .unwrap();
        assert!(
            (geographic.lat - latitude).abs() < 1e-9 && (geographic.lon - longitude).abs() < 1e-9,
            "method {method_code} inverse row {row}: got {geographic:?}, expected lat {latitude} lon {longitude}"
        );
    }
}

fn degrees(code: u32, value: f64) -> (u32, f64) {
    (code, value.to_radians())
}

#[test]
fn transverse_mercator_utm_33n_with_zone_edge() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 0.0),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 15.0),
        (SCALE_FACTOR_AT_NATURAL_ORIGIN, 0.9996),
        (FALSE_EASTING, 500000.0),
        (FALSE_NORTHING, 0.0),
    ];
    // printf '52 15\n60 18\n45 12.5\n' | cs2cs -f %.6f EPSG:4326 EPSG:32633
    let forward = [
        [52.0, 15.0, 500000.000000, 5761038.212590],
        [60.0, 18.0, 667294.821124, 6655205.483635],
        [45.0, 12.5, 302961.671843, 4985991.017362],
    ];
    // printf '500000.000000 5761038.212590\n667294.821124 6655205.483635\n302961.671843 4985991.017362\n' | cs2cs -f %.12f EPSG:32633 EPSG:4326
    let inverse = [
        [
            500000.000000,
            5761038.212590,
            51.999999999996,
            15.000000000000,
        ],
        [
            667294.821124,
            6655205.483635,
            60.000000000004,
            17.999999999992,
        ],
        [
            302961.671843,
            4985991.017362,
            45.000000000003,
            12.500000000001,
        ],
    ];
    check(9807, &parameters, WGS84, EAST_NORTH, &forward, &inverse);
}

#[test]
fn transverse_mercator_3d_luxembourg() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 49.8333333333333),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 6.16666666666667),
        (SCALE_FACTOR_AT_NATURAL_ORIGIN, 1.0),
        (FALSE_EASTING, 80000.0),
        (FALSE_NORTHING, 100000.0),
    ];
    // printf '49.6 6.1 0\n50.1 5.9 0\n' | cs2cs -f %.6f EPSG:9893 EPSG:9895
    let forward = [
        [49.6, 6.1, 74049.036712, 75180.514907],
        [50.1, 5.9, 129696.038873, 60919.920417],
    ];
    // printf '74049.036712 75180.514907 0\n129696.038873 60919.920417 0\n' | cs2cs -f %.12f EPSG:9895 EPSG:9893
    let inverse = [
        [74049.036712, 75180.514907, 49.600000000000, 6.100000000004],
        [129696.038873, 60919.920417, 50.100000000001, 5.899999999994],
    ];
    check(
        1111,
        &parameters,
        INTERNATIONAL_1924,
        NORTH_EAST,
        &forward,
        &inverse,
    );
}

#[test]
fn transverse_mercator_south_orientated_lo15() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 0.0),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 15.0),
        (SCALE_FACTOR_AT_NATURAL_ORIGIN, 1.0),
        (FALSE_EASTING, 0.0),
        (FALSE_NORTHING, 0.0),
    ];
    let west_south = PrintedAxes {
        order: AxisOrder::WestSouth,
        metres_per_unit: 1.0,
    };
    // printf -- '-22.9 14.5\n-23.1 14.4\n' | cs2cs -f %.6f EPSG:4148 EPSG:2046
    let forward = [
        [-22.9, 14.5, 51299.408307, 2533550.348941],
        [-23.1, 14.4, 61468.897676, 2555738.326495],
    ];
    // printf '51303.105826 2533412.627290\n61487.913932 2555578.226633\n' | cs2cs -f %.12f EPSG:2046 EPSG:4148
    let inverse = [
        [
            51303.105826,
            2533412.627290,
            -22.898756314816,
            14.499968520822,
        ],
        [
            61487.913932,
            2555578.226633,
            -23.098553717108,
            14.399820811828,
        ],
    ];
    check(9808, &parameters, WGS84, west_south, &forward, &inverse);
}

fn utm_grid_system_parameters(false_northing: f64) -> [(u32, f64); 6] {
    [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 0.0),
        degrees(INITIAL_LONGITUDE, -180.0),
        degrees(ZONE_WIDTH, 6.0),
        (SCALE_FACTOR_AT_NATURAL_ORIGIN, 0.9996),
        (FALSE_EASTING, 500000.0),
        (FALSE_NORTHING, false_northing),
    ]
}

#[test]
fn transverse_mercator_zoned_grid_system_north() {
    // PROJ 9.8.1 has no tmerczoned and cs2cs prints `* * inf` for EPSG:32600 and EPSG:32700.
    // These references are tmerc at the zone's central meridian with x_0 = 500000 + zone * 1000000.
    // cct takes longitude then latitude, the rows below keep latitude first.
    // printf '15 52 0 0\n' | cct -d 6 +proj=tmerc +lat_0=0 +lon_0=15 +k=0.9996 +x_0=33500000 +y_0=0 +ellps=WGS84
    // printf -- '-74 40 0 0\n' | cct -d 6 +proj=tmerc +lat_0=0 +lon_0=-75 +k=0.9996 +x_0=18500000 +y_0=0 +ellps=WGS84
    // printf '179 10 0 0\n' | cct -d 6 +proj=tmerc +lat_0=0 +lon_0=177 +k=0.9996 +x_0=60500000 +y_0=0 +ellps=WGS84
    let forward = [
        [52.0, 15.0, 33500000.000000, 5761038.212590],
        [40.0, -74.0, 18585360.461843, 4428236.064633],
        [10.0, 179.0, 60719233.138834, 1106077.129706],
    ];
    // printf '18585360.461843 4428236.064633 0 0\n' | cct -I -d 12 +proj=tmerc +lat_0=0 +lon_0=-75 +k=0.9996 +x_0=18500000 +y_0=0 +ellps=WGS84
    // printf '60719233.138834 1106077.129706 0 0\n' | cct -I -d 12 +proj=tmerc +lat_0=0 +lon_0=177 +k=0.9996 +x_0=60500000 +y_0=0 +ellps=WGS84
    let inverse = [
        [
            18585360.461843,
            4428236.064633,
            39.999999999999,
            -73.999999999997,
        ],
        [
            60719233.138834,
            1106077.129706,
            10.000000000003,
            179.000000000002,
        ],
    ];
    check(
        9824,
        &utm_grid_system_parameters(0.0),
        WGS84,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn transverse_mercator_zoned_grid_system_south() {
    // references are tmerc at the zone's central meridian, as in the northern test
    // printf '25 -30 0 0\n' | cct -d 6 +proj=tmerc +lat_0=0 +lon_0=27 +k=0.9996 +x_0=35500000 +y_0=10000000 +ellps=WGS84
    // printf -- '-56 -20 0 0\n' | cct -d 6 +proj=tmerc +lat_0=0 +lon_0=-57 +k=0.9996 +x_0=21500000 +y_0=10000000 +ellps=WGS84
    let forward = [
        [-30.0, 25.0, 35307084.894821, 6679530.713502],
        [-20.0, -56.0, 21604609.323832, 7788206.443835],
    ];
    // printf '35307084.894821 6679530.713502 0 0\n' | cct -I -d 12 +proj=tmerc +lat_0=0 +lon_0=27 +k=0.9996 +x_0=35500000 +y_0=10000000 +ellps=WGS84
    // printf '21604609.323832 7788206.443835 0 0\n' | cct -I -d 12 +proj=tmerc +lat_0=0 +lon_0=-57 +k=0.9996 +x_0=21500000 +y_0=10000000 +ellps=WGS84
    let inverse = [
        [
            35307084.894821,
            6679530.713502,
            -30.000000000001,
            24.999999999996,
        ],
        [
            21604609.323832,
            7788206.443835,
            -19.999999999997,
            -55.999999999998,
        ],
    ];
    check(
        9824,
        &utm_grid_system_parameters(10_000_000.0),
        WGS84,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn lambert_conic_conformal_1sp_jamaica() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 18.0),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, -77.0),
        (SCALE_FACTOR_AT_NATURAL_ORIGIN, 1.0),
        (FALSE_EASTING, 750000.0),
        (FALSE_NORTHING, 650000.0),
    ];
    // printf '18.2 -76.8\n17.9 -78.2\n' | cs2cs -f %.6f EPSG:4758 EPSG:3448
    let forward = [
        [18.2, -76.8, 771157.114565, 672147.762126],
        [17.9, -78.2, 622843.276001, 639343.495445],
    ];
    // printf '771157.114565 672147.762126\n622843.276001 639343.495445\n' | cs2cs -f %.12f EPSG:3448 EPSG:4758
    let inverse = [
        [
            771157.114565,
            672147.762126,
            18.200000000001,
            -76.800000000002,
        ],
        [
            622843.276001,
            639343.495445,
            17.899999999997,
            -78.200000000004,
        ],
    ];
    check(9801, &parameters, WGS84, EAST_NORTH, &forward, &inverse);
}

#[test]
fn lambert_conic_conformal_1sp_variant_b_ltf2004() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 44.3791666666667),
        (SCALE_FACTOR_AT_NATURAL_ORIGIN, 1.0),
        degrees(LATITUDE_OF_FALSE_ORIGIN, 45.1833333333333),
        degrees(LONGITUDE_OF_FALSE_ORIGIN, 6.81666666666667),
        (EASTING_AT_FALSE_ORIGIN, 150000.0),
        (NORTHING_AT_FALSE_ORIGIN, 50000.0),
    ];
    // printf '45.2 6.5\n45.8 5.0\n' | cs2cs -f %.6f EPSG:9547 EPSG:9549
    let forward = [
        [45.2, 6.5, 125116.356151, 51900.539348],
        [45.8, 5.0, 8736.773763, 120116.866564],
    ];
    // printf '125130.744998 51832.474924\n9286.738926 120938.014183\n' | cs2cs -f %.12f EPSG:9549 EPSG:9547
    let inverse = [
        [125130.744998, 51832.474924, 45.199388123341, 6.500186455391],
        [9286.738926, 120938.014183, 45.807493284453, 5.006837071084],
    ];
    check(1102, &parameters, GRS80, EAST_NORTH, &forward, &inverse);
}

#[test]
fn lambert_conic_conformal_2sp_europe() {
    let parameters = [
        degrees(LATITUDE_OF_FALSE_ORIGIN, 52.0),
        degrees(LONGITUDE_OF_FALSE_ORIGIN, 10.0),
        degrees(LATITUDE_OF_FIRST_STANDARD_PARALLEL, 35.0),
        degrees(LATITUDE_OF_SECOND_STANDARD_PARALLEL, 65.0),
        (EASTING_AT_FALSE_ORIGIN, 4000000.0),
        (NORTHING_AT_FALSE_ORIGIN, 2800000.0),
    ];
    // printf '48 2\n60 25\n40 -5\n' | cs2cs -f %.6f EPSG:4258 EPSG:3034
    let forward = [
        [48.0, 2.0, 2401460.791955, 3423954.579643],
        [60.0, 25.0, 3747440.114387, 4813845.120686],
        [40.0, -5.0, 1632636.775834, 2750866.152544],
    ];
    // printf '2349362.051637 3402633.312049\n3683286.312089 4705419.096278\n1773210.689307 2646006.627071\n' | cs2cs -f %.12f EPSG:3034 EPSG:4258
    let inverse = [
        [
            2349362.051637,
            3402633.312049,
            47.496633658801,
            1.786398263678,
        ],
        [
            3683286.312089,
            4705419.096278,
            59.608126600217,
            22.841826385656,
        ],
        [
            1773210.689307,
            2646006.627071,
            41.057553219909,
            -6.592579236320,
        ],
    ];
    check(9802, &parameters, GRS80, NORTH_EAST, &forward, &inverse);
}

#[test]
fn lambert_conic_conformal_2sp_belgium() {
    let parameters = [
        degrees(LATITUDE_OF_FALSE_ORIGIN, 90.0),
        degrees(LONGITUDE_OF_FALSE_ORIGIN, 4.35693972222222),
        degrees(LATITUDE_OF_FIRST_STANDARD_PARALLEL, 49.8333333333333),
        degrees(LATITUDE_OF_SECOND_STANDARD_PARALLEL, 51.1666666666667),
        (EASTING_AT_FALSE_ORIGIN, 150000.01256),
        (NORTHING_AT_FALSE_ORIGIN, 5400088.4378),
    ];
    // printf '50.8 4.4\n51.2 3.0\n' | cs2cs -f %.6f EPSG:4313 EPSG:31300
    let forward = [
        [50.8, 4.4, 153035.701947, 165444.743034],
        [51.2, 3.0, 55156.158762, 210810.226993],
    ];
    // printf '153056.151839 168806.708225\n55289.302237 214049.587632\n' | cs2cs -f %.12f EPSG:31300 EPSG:4313
    let inverse = [
        [
            153056.151839,
            168806.708225,
            50.830221715685,
            4.400317934850,
        ],
        [55289.302237, 214049.587632, 51.229132862134, 3.001058263773],
    ];
    check(
        9803,
        &parameters,
        INTERNATIONAL_1924,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn lambert_conic_conformal_2sp_michigan() {
    let parameters = [
        degrees(LATITUDE_OF_FALSE_ORIGIN, 43.3166666666667),
        degrees(LONGITUDE_OF_FALSE_ORIGIN, -84.3333333333333),
        degrees(LATITUDE_OF_FIRST_STANDARD_PARALLEL, 44.1833333333333),
        degrees(LATITUDE_OF_SECOND_STANDARD_PARALLEL, 45.7),
        (EASTING_AT_FALSE_ORIGIN, 2000000.0 * US_SURVEY_FOOT),
        (NORTHING_AT_FALSE_ORIGIN, 0.0),
        (ELLIPSOID_SCALING_FACTOR, 1.0000382),
    ];
    let us_feet = PrintedAxes {
        order: AxisOrder::EastNorth,
        metres_per_unit: US_SURVEY_FOOT,
    };
    // printf '44.5 -84.0\n45.5 -86.5\n' | cs2cs -f %.6f EPSG:4267 EPSG:6201
    let forward = [
        [44.5, -84.0, 2086975.024540, 431596.682961],
        [45.5, -86.5, 1444466.132328, 803427.242752],
    ];
    // printf '2087314.106131 431282.733312\n1424770.087998 802932.708082\n' | cs2cs -f %.12f EPSG:6201 EPSG:4267
    let inverse = [
        [
            2087314.106131,
            431282.733312,
            44.499135009072,
            -83.998705442525,
        ],
        [
            1424770.087998,
            802932.708082,
            45.497175945248,
            -86.576725734487,
        ],
    ];
    check(1051, &parameters, CLARKE_1866, us_feet, &forward, &inverse);
}

#[test]
fn albers_equal_area_british_columbia() {
    let parameters = [
        degrees(LATITUDE_OF_FALSE_ORIGIN, 45.0),
        degrees(LONGITUDE_OF_FALSE_ORIGIN, -126.0),
        degrees(LATITUDE_OF_FIRST_STANDARD_PARALLEL, 50.0),
        degrees(LATITUDE_OF_SECOND_STANDARD_PARALLEL, 58.5),
        (EASTING_AT_FALSE_ORIGIN, 1000000.0),
        (NORTHING_AT_FALSE_ORIGIN, 0.0),
    ];
    // printf '49.5 -123.0\n58.0 -130.0\n' | cs2cs -f %.6f EPSG:4269 EPSG:3005
    let forward = [
        [49.5, -123.0, 1217378.188146, 502795.385272],
        [58.0, -130.0, 763752.609888, 1452611.788326],
    ];
    // printf '1219105.590622 431565.484014\n763612.648109 1388659.373183\n' | cs2cs -f %.12f EPSG:3005 EPSG:4269
    let inverse = [
        [
            1219105.590622,
            431565.484014,
            48.858767389148,
            -123.017578654796,
        ],
        [
            763612.648109,
            1388659.373183,
            57.427186335964,
            -129.942130734217,
        ],
    ];
    check(9822, &parameters, GRS80, EAST_NORTH, &forward, &inverse);
}

#[test]
fn azimuthal_equidistant_equi7_africa() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 8.5),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 21.5),
        (FALSE_EASTING, 5621452.02),
        (FALSE_NORTHING, 5990638.423),
    ];
    // printf '0 20\n-30 25\n30 10\n' | cs2cs -f %.6f EPSG:4326 EPSG:27701
    let forward = [
        [0.0, 20.0, 5453862.908919, 5050904.957317],
        [-30.0, 25.0, 5985790.964314, 1729675.400393],
        [30.0, 10.0, 4486422.670060, 8401991.274390],
    ];
    // printf '5455287.928339 5050592.733081\n5986278.637981 1813111.983766\n4374911.547880 8484155.312493\n' | cs2cs -f %.12f EPSG:27701 EPSG:4326
    let inverse = [
        [
            5455287.928339,
            5050592.733081,
            -0.002790135244,
            20.012758161593,
        ],
        [
            5986278.637981,
            1813111.983766,
            -29.247765883114,
            24.989141822442,
        ],
        [
            4374911.547880,
            8484155.312493,
            30.675951519208,
            8.799626915060,
        ],
    ];
    check(1125, &parameters, WGS84, EAST_NORTH, &forward, &inverse);
}

#[test]
fn modified_azimuthal_equidistant_yap() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 9.54670833333333),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 138.168744444444),
        (FALSE_EASTING, 40000.0),
        (FALSE_NORTHING, 60000.0),
    ];
    // printf '9.5 138.1\n9.6 138.2\n' | cs2cs -f %.6f EPSG:4675 EPSG:3295
    let forward = [
        [9.5, 138.1, 32451.575369, 54834.904063],
        [9.6, 138.2, 43430.988575, 65894.121492],
    ];
    // printf '32446.761209 54831.063149\n43420.728734 65885.436700\n' | cs2cs -f %.12f EPSG:3295 EPSG:4675
    let inverse = [
        [32446.761209, 54831.063149, 9.499965262696, 138.099956163790],
        [43420.728734, 65885.436700, 9.599921482947, 138.199906527931],
    ];
    check(
        9832,
        &parameters,
        CLARKE_1866,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn guam_projection() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 13.4724663527778),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 144.748750705556),
        (FALSE_EASTING, 50000.0),
        (FALSE_NORTHING, 50000.0),
    ];
    // printf '13.4 144.75\n13.6 144.9\n' | cs2cs -f %.6f EPSG:4675 EPSG:3993
    let forward = [
        [13.4, 144.75, 50135.310809, 41983.203572],
        [13.6, 144.9, 66368.143215, 64113.966687],
    ];
    // printf '50135.536508 41977.766212\n66356.226102 64171.035954\n' | cs2cs -f %.12f EPSG:3993 EPSG:4675
    let inverse = [
        [
            50135.536508,
            41977.766212,
            13.399950849838,
            144.750002083574,
        ],
        [
            66356.226102,
            64171.035954,
            13.600515925310,
            144.899890207455,
        ],
    ];
    check(
        9831,
        &parameters,
        CLARKE_1866,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn cassini_soldner_berlin() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 52.4186482777778),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 13.6272036666667),
        (FALSE_EASTING, 40000.0),
        (FALSE_NORTHING, 10000.0),
    ];
    // printf '52.5 13.4\n52.4 13.7\n' | cs2cs -f %.6f EPSG:4314 EPSG:3068
    let forward = [
        [52.5, 13.4, 19075.727769, 24572.458733],
        [52.4, 13.7, 7927.642424, 44954.218077],
    ];
    // printf '19055.143845 24585.402577\n7926.744434 44960.713013\n' | cs2cs -f %.12f EPSG:3068 EPSG:4314
    let inverse = [
        [19055.143845, 24585.402577, 52.499815366624, 13.400191577516],
        [7926.744434, 44960.713013, 52.399991870273, 13.700095421979],
    ];
    check(
        9806,
        &parameters,
        BESSEL_1841,
        NORTH_EAST,
        &forward,
        &inverse,
    );
}

#[test]
fn hyperbolic_cassini_soldner_vanua_levu_across_the_antimeridian() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, -16.25),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 179.333333333333),
        (FALSE_EASTING, 1251331.8 * LINK),
        (FALSE_NORTHING, 1662888.5 * LINK),
    ];
    let clarke_1880_international_foot = Ellipsoid::new(
        20_926_202.0 * 0.3048,
        (20_926_202.0 - 20_854_895.0) / 20_926_202.0,
    );
    let links = PrintedAxes {
        order: AxisOrder::NorthEast,
        metres_per_unit: LINK,
    };
    // printf -- '-16.5 179.0\n-16.8 -179.9\n' | cs2cs -f %.6f EPSG:4748 EPSG:3139
    let forward = [
        [-16.5, 179.0, 1525227.351352, 1074419.283350],
        [-16.8, -179.9, 1359569.146049, 1657597.072636],
    ];
    // printf '1525227.351352 1074419.283350\n1359569.146049 1657597.072636\n' | cs2cs -f %.12f EPSG:3139 EPSG:4748
    let inverse = [
        [
            1525227.351352,
            1074419.283350,
            -16.500000000000,
            179.000000000000,
        ],
        [
            1359569.146049,
            1657597.072636,
            -16.800000000000,
            -179.900000000000,
        ],
    ];
    check(
        9833,
        &parameters,
        clarke_1880_international_foot,
        links,
        &forward,
        &inverse,
    );
}

#[test]
fn oblique_stereographic_rd_new() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 52.1561605555556),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 5.38763888888889),
        (SCALE_FACTOR_AT_NATURAL_ORIGIN, 0.9999079),
        (FALSE_EASTING, 155000.0),
        (FALSE_NORTHING, 463000.0),
    ];
    // printf '52.0 5.0\n53.2 6.5\n' | cs2cs -f %.6f EPSG:4289 EPSG:28992
    let forward = [
        [52.0, 5.0, 128383.474527, 445698.804306],
        [53.2, 6.5, 229323.394170, 579711.597578],
    ];
    // printf '128410.084589 445806.502864\n230633.405547 579425.627044\n' | cs2cs -f %.12f EPSG:28992 EPSG:4289
    let inverse = [
        [
            128410.084589,
            445806.502864,
            52.000969382190,
            5.000379173402,
        ],
        [
            230633.405547,
            579425.627044,
            53.197247135266,
            6.519535386180,
        ],
    ];
    check(
        9809,
        &parameters,
        BESSEL_1841,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn polar_stereographic_variant_a_ups_north() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 90.0),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 0.0),
        (SCALE_FACTOR_AT_NATURAL_ORIGIN, 0.994),
        (FALSE_EASTING, 2000000.0),
        (FALSE_NORTHING, 2000000.0),
    ];
    // printf '80 10\n70 -120\n' | cs2cs -f %.6f EPSG:4326 EPSG:5041
    let forward = [
        [80.0, 10.0, 2193261.936765, 903957.091603],
        [70.0, -120.0, 58033.494847, 3121194.884508],
    ];
    // printf '2193379.045343 903053.969553\n803683.012937 2690231.693271\n' | cs2cs -f %.12f EPSG:5041 EPSG:4326
    let inverse = [
        [
            2193379.045343,
            903053.969553,
            79.991865726181,
            9.997865488155,
        ],
        [
            803683.012937,
            2690231.693271,
            77.606412727846,
            -119.983393466829,
        ],
    ];
    check(9810, &parameters, WGS84, EAST_NORTH, &forward, &inverse);
}

#[test]
fn polar_stereographic_variant_b_antarctic() {
    let parameters = [
        degrees(LATITUDE_OF_STANDARD_PARALLEL, -71.0),
        degrees(LONGITUDE_OF_ORIGIN, 0.0),
        (FALSE_EASTING, 0.0),
        (FALSE_NORTHING, 0.0),
    ];
    // printf -- '-75 0\n-80 120\n-65 -60\n' | cs2cs -f %.6f EPSG:4326 EPSG:3031
    let forward = [
        [-75.0, 0.0, 0.000000, 1638783.238407],
        [-80.0, 120.0, 943257.077852, -544589.727813],
        [-65.0, -60.0, -2388932.766882, 1379250.976035],
    ];
    // printf '0.000000 1638783.238203\n943738.814032 544867.856034\n-2328283.418426 1344230.316003\n' | cs2cs -f %.12f EPSG:3031 EPSG:4326
    let inverse = [
        [0.000000, 1638783.238203, -75.000000001849, 0.000000000000],
        [
            943738.814032,
            544867.856034,
            -79.994917729347,
            60.000000104366,
        ],
        [
            -2328283.418426,
            1344230.316003,
            -65.616204629354,
            -60.000087527460,
        ],
    ];
    check(9829, &parameters, WGS84, EAST_NORTH, &forward, &inverse);
}

#[test]
fn hotine_oblique_mercator_variant_a_michigan() {
    let parameters = [
        degrees(LATITUDE_OF_PROJECTION_CENTRE, 45.3091666666667),
        degrees(LONGITUDE_OF_PROJECTION_CENTRE, -86.0),
        degrees(AZIMUTH_AT_PROJECTION_CENTRE, 337.25556),
        degrees(ANGLE_FROM_RECTIFIED_TO_SKEW_GRID, 337.25556),
        (SCALE_FACTOR_AT_PROJECTION_CENTRE, 0.9996),
        (FALSE_EASTING, 2546731.496),
        (FALSE_NORTHING, -4354009.816),
    ];
    // printf '44 -85\n46.5 -88\n' | cs2cs -f %.6f EPSG:4269 EPSG:3078
    let forward = [
        [44.0, -85.0, 580017.658975, 383669.837336],
        [46.5, -88.0, 346385.522415, 662839.905094],
    ];
    // printf '625283.981580 205043.548934\n432233.453437 488004.474045\n' | cs2cs -f %.12f EPSG:3078 EPSG:4269
    let inverse = [
        [
            625283.981580,
            205043.548934,
            42.385434036618,
            -84.476696642252,
        ],
        [
            432233.453437,
            488004.474045,
            44.940516961689,
            -86.856892240771,
        ],
    ];
    check(9812, &parameters, GRS80, EAST_NORTH, &forward, &inverse);
}

#[test]
fn hotine_oblique_mercator_variant_b_swiss_lv95() {
    // PROJ runs EPSG:2056 through somerc because its azimuth and skew angle are both 90 degrees
    let parameters = [
        degrees(LATITUDE_OF_PROJECTION_CENTRE, 46.9524055555556),
        degrees(LONGITUDE_OF_PROJECTION_CENTRE, 7.43958333333333),
        degrees(AZIMUTH_AT_PROJECTION_CENTRE, 90.0),
        degrees(ANGLE_FROM_RECTIFIED_TO_SKEW_GRID, 90.0),
        (SCALE_FACTOR_AT_PROJECTION_CENTRE, 1.0),
        (EASTING_AT_PROJECTION_CENTRE, 2600000.0),
        (NORTHING_AT_PROJECTION_CENTRE, 1200000.0),
    ];
    // printf '46.9 7.4\n47.5 9.0\n' | cs2cs -f %.6f EPSG:4150 EPSG:2056
    let forward = [
        [46.9, 7.4, 2596984.202522, 1194175.518290],
        [47.5, 9.0, 2717550.739557, 1262042.889912],
    ];
    // printf '2596967.034007 1194126.024834\n2717668.234766 1261082.617577\n' | cs2cs -f %.12f EPSG:2056 EPSG:4150
    let inverse = [
        [
            2596967.034007,
            1194126.024834,
            46.899554662613,
            7.399774987477,
        ],
        [
            2717668.234766,
            1261082.617577,
            47.491343046589,
            9.001305391061,
        ],
    ];
    check(
        9815,
        &parameters,
        BESSEL_1841,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn hotine_oblique_mercator_variant_b_rso_borneo() {
    let parameters = [
        degrees(LATITUDE_OF_PROJECTION_CENTRE, 4.0),
        degrees(LONGITUDE_OF_PROJECTION_CENTRE, 115.0),
        degrees(AZIMUTH_AT_PROJECTION_CENTRE, 53.3158204722222),
        degrees(ANGLE_FROM_RECTIFIED_TO_SKEW_GRID, 53.1301023611111),
        (SCALE_FACTOR_AT_PROJECTION_CENTRE, 0.99984),
        (EASTING_AT_PROJECTION_CENTRE, 590476.87),
        (NORTHING_AT_PROJECTION_CENTRE, 442857.65),
    ];
    let everest_1830_1967 = Ellipsoid::new(6_377_298.556, 1.0 / 300.8017);
    // printf '5.0 116.0\n2.0 111.0\n' | cs2cs -f %.6f EPSG:4298 EPSG:29873
    let forward = [
        [5.0, 116.0, 700991.444560, 553843.345144],
        [2.0, 111.0, 146250.659021, 221129.929746],
    ];
    // printf '701455.430134 552870.163003\n144930.553637 220740.993519\n' | cs2cs -f %.12f EPSG:29873 EPSG:4298
    let inverse = [
        [
            701455.430134,
            552870.163003,
            4.991178399056,
            116.004144869347,
        ],
        [
            144930.553637,
            220740.993519,
            1.996483610060,
            110.988131409902,
        ],
    ];
    check(
        9815,
        &parameters,
        everest_1830_1967,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn lambert_azimuthal_equal_area_europe() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 52.0),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 10.0),
        (FALSE_EASTING, 4321000.0),
        (FALSE_NORTHING, 3210000.0),
    ];
    // printf '50 5\n60 25\n40 -5\n' | cs2cs -f %.6f EPSG:4258 EPSG:3035
    let forward = [
        [50.0, 5.0, 2999718.853160, 3962799.450955],
        [60.0, 25.0, 4188383.611105, 5152827.253890],
        [40.0, -5.0, 2005271.675257, 3042765.835795],
    ];
    // printf '2995549.074930 3963383.373040\n4106913.815612 5160926.372069\n2004785.312049 2927706.155380\n' | cs2cs -f %.12f EPSG:3035 EPSG:4258
    let inverse = [
        [
            2995549.074930,
            3963383.373040,
            49.962964209787,
            5.012029266923,
        ],
        [
            4106913.815612,
            5160926.372069,
            59.270725973689,
            24.823713461561,
        ],
        [
            2004785.312049,
            2927706.155380,
            39.782009346902,
            -6.317101367845,
        ],
    ];
    check(9820, &parameters, GRS80, NORTH_EAST, &forward, &inverse);
}

#[test]
fn lambert_azimuthal_equal_area_spherical_us_national_atlas() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 45.0),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, -100.0),
        (FALSE_EASTING, 0.0),
        (FALSE_NORTHING, 0.0),
    ];
    // printf '40 -100\n60 -150\n25 -80\n' | cs2cs -f %.6f EPSG:4267 EPSG:9311
    let forward = [
        [40.0, -100.0, 0.000000, -555797.993100],
        [60.0, -150.0, -2544373.638912, 2558260.431759],
        [25.0, -80.0, 2025600.044040, -1982446.488926],
    ];
    // printf '0.000000 -556597.453966\n-2366458.006137 1767007.208937\n1976315.584613 -2006925.393617\n' | cs2cs -f %.12f EPSG:9311 EPSG:4267
    let inverse = [
        [0.000000, -556597.453966, 39.992803413172, -100.000000000000],
        [
            -2366458.006137,
            1767007.208937,
            55.276153447232,
            -139.370169634692,
        ],
        [
            1976315.584613,
            -2006925.393617,
            24.883005711828,
            -80.519299651661,
        ],
    ];
    check(
        1027,
        &parameters,
        CLARKE_1866,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn mercator_variant_a_segara() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 0.0),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 110.0),
        (SCALE_FACTOR_AT_NATURAL_ORIGIN, 0.997),
        (FALSE_EASTING, 3900000.0),
        (FALSE_NORTHING, 900000.0),
    ];
    // printf '0 116\n-3 118.5\n' | cs2cs -f %.6f EPSG:4613 EPSG:3000
    let forward = [
        [0.0, 116.0, 4565835.949967, 900000.000000],
        [-3.0, 118.5, 4843267.595787, 569150.818614],
    ];
    // printf '4566175.902542 900000.000000\n4843576.002468 567418.627931\n' | cs2cs -f %.12f EPSG:3000 EPSG:4613
    let inverse = [
        [
            4566175.902542,
            900000.000000,
            0.000000000000,
            116.003063390387,
        ],
        [
            4843576.002468,
            567418.627931,
            -3.015692110988,
            118.502779123125,
        ],
    ];
    check(
        9804,
        &parameters,
        BESSEL_1841,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn mercator_variant_b_across_the_antimeridian() {
    let parameters = [
        degrees(LATITUDE_OF_FIRST_STANDARD_PARALLEL, -41.0),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 100.0),
        (FALSE_EASTING, 0.0),
        (FALSE_NORTHING, 0.0),
    ];
    // printf -- '-45 170\n-35 -175\n' | cs2cs -f %.6f EPSG:4326 EPSG:3994
    let forward = [
        [-45.0, 170.0, 5889462.957985, -4225897.134300],
        [-35.0, -175.0, 7151490.734696, -3128534.734807],
    ];
    // printf '5891866.412530 -4232783.785937\n7154409.143428 -3122547.290152\n' | cs2cs -f %.12f EPSG:3994 EPSG:4326
    let inverse = [
        [
            5891866.412530,
            -4232783.785937,
            -45.058043666526,
            170.028566580578,
        ],
        [
            7154409.143428,
            -3122547.290152,
            -34.941420632522,
            -174.965312862533,
        ],
    ];
    check(9805, &parameters, WGS84, EAST_NORTH, &forward, &inverse);
}

#[test]
fn popular_visualisation_pseudo_mercator() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 0.0),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 0.0),
        (FALSE_EASTING, 0.0),
        (FALSE_NORTHING, 0.0),
    ];
    // printf '40.7128 -74.006\n-33.8688 151.2093\n85 179\n' | cs2cs -f %.6f EPSG:4326 EPSG:3857
    let forward = [
        [40.7128, -74.006, -8238310.235647, 4970071.579142],
        [-33.8688, 151.2093, 16832542.279207, -4011198.647308],
        [85.0, 179.0, 19926188.851996, 19971868.880409],
    ];
    // printf -- '-8238310.235647 4970071.579142\n16832437.059072 -4011652.425360\n19926188.851996 19971868.880409\n' | cs2cs -f %.12f EPSG:3857 EPSG:4326
    let inverse = [
        [
            -8238310.235647,
            4970071.579142,
            40.712799999997,
            -74.006000000000,
        ],
        [
            16832437.059072,
            -4011652.425360,
            -33.872184597340,
            151.208354791442,
        ],
        [
            19926188.851996,
            19971868.880409,
            85.000000000000,
            179.000000000000,
        ],
    ];
    check(1024, &parameters, WGS84, EAST_NORTH, &forward, &inverse);
}

fn krovak_parameters(colatitude_of_cone_axis: f64, false_origin: f64) -> [(u32, f64); 7] {
    [
        degrees(LATITUDE_OF_PROJECTION_CENTRE, 49.5),
        degrees(LONGITUDE_OF_ORIGIN, 24.8333333333333),
        degrees(COLATITUDE_OF_CONE_AXIS, colatitude_of_cone_axis),
        degrees(LATITUDE_OF_PSEUDO_STANDARD_PARALLEL, 78.5),
        (SCALE_FACTOR_ON_PSEUDO_STANDARD_PARALLEL, 0.9999),
        (FALSE_EASTING, false_origin),
        (FALSE_NORTHING, false_origin),
    ]
}

// PROJ writes +axis=swu into the krovak step for the south-west CRSs
const SOUTH_WEST: PrintedAxes = PrintedAxes {
    order: AxisOrder::SouthWest,
    metres_per_unit: 1.0,
};

#[test]
fn krovak_s_jtsk() {
    // printf '50.0 14.4\n49.2 16.6\n48.9 21.2\n' | cs2cs -f %.6f EPSG:4156 EPSG:5513
    let forward = [
        [50.0, 14.4, 1052516.530041, 745733.864622],
        [49.2, 16.6, 1160206.215911, 598786.141107],
        [48.9, 21.2, 1219470.494491, 266244.967010],
    ];
    // printf '1047225.937143 745311.022143\n1152896.432225 590264.302722\n1214283.040098 256961.226574\n' | cs2cs -f %.12f EPSG:5513 EPSG:4156
    let inverse = [
        [
            1047225.937143,
            745311.022143,
            50.047648187244,
            14.395759368756,
        ],
        [
            1152896.432225,
            590264.302722,
            49.273554899520,
            16.705645762999,
        ],
        [
            1214283.040098,
            256961.226574,
            48.950489148249,
            21.323278387090,
        ],
    ];
    check(
        9819,
        &krovak_parameters(30.2881397527778, 0.0),
        BESSEL_1841,
        SOUTH_WEST,
        &forward,
        &inverse,
    );
}

#[test]
fn krovak_north_orientated_s_jtsk() {
    // printf '50.0 14.4\n49.2 16.6\n48.9 21.2\n' | cs2cs -f %.6f EPSG:4156 EPSG:5514
    let forward = [
        [50.0, 14.4, -745733.864622, -1052516.530041],
        [49.2, 16.6, -598786.141107, -1160206.215911],
        [48.9, 21.2, -266244.967010, -1219470.494491],
    ];
    // printf -- '-745311.022143 -1047225.937143\n-590264.302722 -1152896.432225\n-256961.226574 -1214283.040098\n' | cs2cs -f %.12f EPSG:5514 EPSG:4156
    let inverse = [
        [
            -745311.022143,
            -1047225.937143,
            50.047648187244,
            14.395759368756,
        ],
        [
            -590264.302722,
            -1152896.432225,
            49.273554899520,
            16.705645762999,
        ],
        [
            -256961.226574,
            -1214283.040098,
            48.950489148249,
            21.323278387090,
        ],
    ];
    check(
        1041,
        &krovak_parameters(30.2881397527778, 0.0),
        BESSEL_1841,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn krovak_modified_s_jtsk_05() {
    // printf '50.0 14.4\n49.2 16.6\n' | cs2cs -f %.6f EPSG:5228 EPSG:5515
    let forward = [
        [50.0, 14.4, 6052516.514864, 5745733.880619],
        [49.2, 16.6, 6160206.170124, 5598786.227362],
    ];
    // printf '6047225.900000 5745310.990000\n6152896.400000 5590264.270000\n' | cs2cs -f %.12f EPSG:5515 EPSG:5228
    let inverse = [
        [
            6047225.900000,
            5745310.990000,
            50.047648405082,
            14.395760066035,
        ],
        [
            6152896.400000,
            5590264.270000,
            49.273555111706,
            16.705647425785,
        ],
    ];
    check(
        1042,
        &krovak_parameters(30.2881397222222, 5000000.0),
        BESSEL_1841,
        SOUTH_WEST,
        &forward,
        &inverse,
    );
}

#[test]
fn krovak_modified_north_orientated_s_jtsk_05() {
    // printf '50.0 14.4\n49.2 16.6\n' | cs2cs -f %.6f EPSG:5228 EPSG:5516
    let forward = [
        [50.0, 14.4, -5745733.880619, -6052516.514864],
        [49.2, 16.6, -5598786.227362, -6160206.170124],
    ];
    // printf -- '-5745310.990000 -6047225.900000\n-5590264.270000 -6152896.400000\n' | cs2cs -f %.12f EPSG:5516 EPSG:5228
    let inverse = [
        [
            -5745310.990000,
            -6047225.900000,
            50.047648405082,
            14.395760066035,
        ],
        [
            -5590264.270000,
            -6152896.400000,
            49.273555111706,
            16.705647425785,
        ],
    ];
    check(
        1043,
        &krovak_parameters(30.2881397222222, 5000000.0),
        BESSEL_1841,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn american_polyconic_brazil() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 0.0),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, -54.0),
        (FALSE_EASTING, 5000000.0),
        (FALSE_NORTHING, 10000000.0),
    ];
    let grs_1967_modified = Ellipsoid::new(6_378_160.0, 1.0 / 298.25);
    // printf -- '-15 -47.9\n-30 -51.2\n0 -60\n' | cs2cs -f %.6f EPSG:5527 EPSG:5530
    let forward = [
        [-15.0, -47.9, 5655977.330484, 8331966.327057],
        [-30.0, -51.2, 5270135.681769, 6676574.606519],
        [0.0, -60.0, 4332080.646686, 10000000.000000],
    ];
    // printf '5655696.016519 8331395.087911\n5277170.990000 6669637.040000\n4332155.270000 10000000.000000\n' | cs2cs -f %.12f EPSG:5530 EPSG:5527
    let inverse = [
        [
            5655696.016519,
            8331395.087911,
            -15.005203032394,
            -47.902468603012,
        ],
        [
            5277170.990000,
            6669637.040000,
            -30.060935064257,
            -51.125303449868,
        ],
        [
            4332155.270000,
            10000000.000000,
            0.000000000000,
            -59.999329649782,
        ],
    ];
    check(
        9818,
        &parameters,
        grs_1967_modified,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn laborde_oblique_mercator_madagascar() {
    let parameters = [
        degrees(LATITUDE_OF_PROJECTION_CENTRE, -18.9),
        degrees(LONGITUDE_OF_PROJECTION_CENTRE, 46.4372291666667),
        degrees(AZIMUTH_AT_PROJECTION_CENTRE, 18.9),
        (SCALE_FACTOR_AT_PROJECTION_CENTRE, 0.9995),
        (FALSE_EASTING, 400000.0),
        (FALSE_NORTHING, 800000.0),
    ];
    // printf -- '-18.9 47.5\n-23.0 44.0\n-12.5 49.3\n' | cs2cs -f %.6f EPSG:4297 EPSG:8441
    let forward = [
        [-18.9, 47.5, 799665.520468, 511921.054153],
        [-23.0, 44.0, 344377.008373, 150118.617892],
        [-12.5, 49.3, 1505688.222066, 711526.506321],
    ];
    // printf '800005.170000 511908.170000\n347178.930000 150316.550000\n1508541.380000 694036.120000\n' | cs2cs -f %.12f EPSG:8441 EPSG:4297
    let inverse = [
        [
            800005.170000,
            511908.170000,
            -18.896931235693,
            47.499858449238,
        ],
        [
            347178.930000,
            150316.550000,
            -22.974720707729,
            44.002362044882,
        ],
        [
            1508541.380000,
            694036.120000,
            -12.476195455045,
            49.138713418378,
        ],
    ];
    check(
        9813,
        &parameters,
        INTERNATIONAL_1924,
        NORTH_EAST,
        &forward,
        &inverse,
    );
}

#[test]
fn new_zealand_map_grid() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, -41.0),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 173.0),
        (FALSE_EASTING, 2510000.0),
        (FALSE_NORTHING, 6023150.0),
    ];
    // printf -- '-41.3 174.8\n-45.9 170.5\n-36.8 174.7\n' | cs2cs -f %.6f EPSG:4272 EPSG:27200
    let forward = [
        [-41.3, 174.8, 2660739.633250, 5988279.997099],
        [-45.9, 170.5, 2315861.472202, 5475852.724375],
        [-36.8, 174.7, 2662127.980096, 6487875.334036],
    ];
    // printf '2660739.633250 5988279.997099\n2315861.472202 5475852.724375\n2662127.980096 6487875.334036\n' | cs2cs -f %.12f EPSG:27200 EPSG:4272
    let inverse = [
        [
            2660739.633250,
            5988279.997099,
            -41.300000000029,
            174.800000000001,
        ],
        [
            2315861.472202,
            5475852.724375,
            -45.900000000433,
            170.500000000003,
        ],
        [
            2662127.980096,
            6487875.334036,
            -36.799999999485,
            174.700000000003,
        ],
    ];
    check(
        9811,
        &parameters,
        INTERNATIONAL_1924,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

fn cylindrical_parameters(standard_parallel: f64) -> [(u32, f64); 4] {
    [
        degrees(LATITUDE_OF_FIRST_STANDARD_PARALLEL, standard_parallel),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 0.0),
        (FALSE_EASTING, 0.0),
        (FALSE_NORTHING, 0.0),
    ]
}

#[test]
fn lambert_cylindrical_equal_area_ease_grid_2() {
    // printf '45 10\n-60 -100\n' | cs2cs -f %.6f EPSG:4326 EPSG:6933
    let forward = [
        [45.0, 10.0, 964862.802509, 5180102.328839],
        [-60.0, -100.0, -9648628.025090, -6351419.997326],
    ];
    // printf '964862.802509 5269327.162169\n-9648628.025090 -5845361.931090\n' | cs2cs -f %.12f EPSG:6933 EPSG:4326
    let inverse = [
        [
            964862.802509,
            5269327.162169,
            45.990997081605,
            10.000000000000,
        ],
        [
            -9648628.025090,
            -5845361.931090,
            -52.885549048822,
            -100.000000000004,
        ],
    ];
    check(
        9835,
        &cylindrical_parameters(30.0),
        WGS84,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn lambert_cylindrical_equal_area_spherical_ease_grid() {
    let authalic_sphere = Ellipsoid::new(6_371_228.0, 0.0);
    // printf '45 10\n-60 -100\n' | cs2cs -f %.6f EPSG:10346 EPSG:3410
    let forward = [
        [45.0, 10.0, 963010.774649, 5202085.878311],
        [-60.0, -100.0, -9630107.746493, -6371228.000000],
    ];
    // printf '963010.774649 5202085.878311\n-9630107.746493 -6371228.000000\n' | cs2cs -f %.12f EPSG:3410 EPSG:10346
    let inverse = [
        [
            963010.774649,
            5202085.878311,
            45.000000000000,
            9.999999999997,
        ],
        [
            -9630107.746493,
            -6371228.000000,
            -60.000000000000,
            -100.000000000003,
        ],
    ];
    check(
        9834,
        &cylindrical_parameters(30.0),
        authalic_sphere,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn equidistant_cylindrical_world() {
    // printf '45 10\n-60 -100\n' | cs2cs -f %.6f EPSG:4326 EPSG:4087
    let forward = [
        [45.0, 10.0, 1113194.907933, 4984944.377978],
        [-60.0, -100.0, -11131949.079327, -6654072.819491],
    ];
    // printf '1113194.907933 5009377.085697\n-11131949.079327 -6679169.447597\n' | cs2cs -f %.12f EPSG:4087 EPSG:4326
    let inverse = [
        [
            1113194.907933,
            5009377.085697,
            45.219849227708,
            10.000000000002,
        ],
        [
            -11131949.079327,
            -6679169.447597,
            -60.225255204392,
            -99.999999999997,
        ],
    ];
    check(
        1028,
        &cylindrical_parameters(0.0),
        WGS84,
        EAST_NORTH,
        &forward,
        &inverse,
    );
}

#[test]
fn equal_earth_greenwich() {
    let parameters = [
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 0.0),
        (FALSE_EASTING, 0.0),
        (FALSE_NORTHING, 0.0),
    ];
    // printf '45 10\n-60 -100\n80 170\n' | cs2cs -f %.6f EPSG:4326 EPSG:8857
    let forward = [
        [45.0, 10.0, 821804.152722, 5466867.760214],
        [-60.0, -100.0, -7225128.360577, -6924035.137553],
        [80.0, 170.0, 10053524.739331, 8205603.096488],
    ];
    // printf '845944.196133 5224454.938541\n-7672524.155766 -6713985.497155\n' | cs2cs -f %.12f EPSG:8857 EPSG:4326
    let inverse = [
        [
            845944.196133,
            5224454.938541,
            42.751319408262,
            10.134300831973,
        ],
        [
            -7672524.155766,
            -6713985.497155,
            -57.614477653875,
            -103.749483201896,
        ],
    ];
    check(1078, &parameters, WGS84, EAST_NORTH, &forward, &inverse);
}

#[test]
fn local_orthographic_san_francisco_airport() {
    let parameters = [
        degrees(LATITUDE_OF_PROJECTION_CENTRE, 37.6289686531),
        degrees(LONGITUDE_OF_PROJECTION_CENTRE, -122.3939412704),
        degrees(AZIMUTH_AT_PROJECTION_CENTRE, 27.7928209333),
        (SCALE_FACTOR_AT_PROJECTION_CENTRE, 0.9999968),
        (EASTING_AT_PROJECTION_CENTRE, 0.0),
        (NORTHING_AT_PROJECTION_CENTRE, 0.0),
    ];
    let us_feet = PrintedAxes {
        order: AxisOrder::EastNorth,
        metres_per_unit: US_SURVEY_FOOT,
    };
    // printf '37.62 -122.38\n37.60 -122.42\n' | cs2cs -f %.6f EPSG:6318 EPSG:10622
    let forward = [
        [37.62, -122.38, 5094.800353, -1005.982658],
        [37.60, -122.42, -1760.780033, -12850.983637],
    ];
    // printf '1300.000000 -4500.000000\n-14000.000000 -1500.000000\n' | cs2cs -f %.12f EPSG:10622 EPSG:6318
    let inverse = [
        [
            1300.000000,
            -4500.000000,
            37.616371580754,
            -122.397214826993,
        ],
        [
            -14000.000000,
            -1500.000000,
            37.643242810254,
            -122.439129408911,
        ],
    ];
    check(1130, &parameters, GRS80, us_feet, &forward, &inverse);
}

#[test]
fn colombia_urban_arauca() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 7.08760639166667),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, -70.7583096555555),
        (FALSE_EASTING, 1035263.443),
        (FALSE_NORTHING, 1275526.621),
        (PROJECTION_PLANE_ORIGIN_HEIGHT, 100.0),
    ];
    // printf '7.08 -70.75\n7.06 -70.72\n' | cs2cs -f %.6f EPSG:4686 EPSG:6244
    let forward = [
        [7.08, -70.75, 1274685.416092, 1036181.477393],
        [7.06, -70.72, 1272473.723933, 1039496.000382],
    ];
    // printf '1274667.880000 1036178.880000\n1272468.110000 1039490.920000\n' | cs2cs -f %.12f EPSG:6244 EPSG:4686
    let inverse = [
        [
            1274667.880000,
            1036178.880000,
            7.079841436117,
            -70.750023513327,
        ],
        [
            1272468.110000,
            1039490.920000,
            7.059949241722,
            -70.720045987670,
        ],
    ];
    check(1052, &parameters, GRS80, NORTH_EAST, &forward, &inverse);
}

#[test]
fn unknown_method_names_the_method_code() {
    let error = from_epsg_method(9999, &[], &WGS84)
        .err()
        .expect("method 9999 does not exist");
    assert!(error.to_string().contains("9999"), "{error}");
}

#[test]
fn missing_parameter_names_the_parameter_code() {
    let parameters = [
        degrees(LATITUDE_OF_NATURAL_ORIGIN, 0.0),
        degrees(LONGITUDE_OF_NATURAL_ORIGIN, 15.0),
        (FALSE_EASTING, 500000.0),
        (FALSE_NORTHING, 0.0),
    ];
    let error = from_epsg_method(9807, &parameters, &WGS84)
        .err()
        .expect("transverse mercator needs a scale factor");
    assert!(error.to_string().contains("8805"), "{error}");
}
