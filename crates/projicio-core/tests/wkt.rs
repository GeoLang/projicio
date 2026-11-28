use projicio_core::Error;
use projicio_core::iso19111::authority_factory::create_coordinate_reference_system;
use projicio_core::iso19111::common::UnitOfMeasure;
use projicio_core::iso19111::coordinate_system::AxisDirection;
use projicio_core::iso19111::datum::{EllipsoidShape, GeodeticDatum};
use projicio_core::iso19111::operation::ParameterValue;
use projicio_core::iso19111::{Criterion, Crs, ProjectedCrs, WktDialect, WktLayout};

struct Fixture {
    code: &'static str,
    projjson: &'static str,
    wkt2_2019: &'static str,
    wkt2_2015: &'static str,
    wkt1_gdal: Option<&'static str>,
    wkt1_esri: Option<&'static str>,
}

// projinfo EPSG:<code> -o <dialect> -q, saved with a shell redirect
macro_rules! fixture {
    ($code:literal) => {
        fixture!(
            $code,
            Some(include_str!(concat!(
                "data/wkt/wkt1_gdal/epsg_",
                $code,
                ".wkt"
            ))),
            Some(include_str!(concat!(
                "data/wkt/wkt1_esri/epsg_",
                $code,
                ".wkt"
            )))
        )
    };
    ($code:literal, $wkt1_gdal:expr, $wkt1_esri:expr) => {
        Fixture {
            code: $code,
            projjson: include_str!(concat!("data/wkt/projjson/epsg_", $code, ".json")),
            wkt2_2019: include_str!(concat!("data/wkt/wkt2_2019/epsg_", $code, ".wkt")),
            wkt2_2015: include_str!(concat!("data/wkt/wkt2_2015/epsg_", $code, ".wkt")),
            wkt1_gdal: $wkt1_gdal,
            wkt1_esri: $wkt1_esri,
        }
    };
}

fn fixtures() -> Vec<Fixture> {
    vec![
        fixture!("4326"),
        fixture!("4269"),
        fixture!("4807"),
        // projinfo refuses WKT1:GDAL for a geographic 3D CRS
        fixture!(
            "4979",
            None,
            Some(include_str!("data/wkt/wkt1_esri/epsg_4979.wkt"))
        ),
        // projinfo refuses WKT1:ESRI for a geocentric CRS
        fixture!(
            "4978",
            Some(include_str!("data/wkt/wkt1_gdal/epsg_4978.wkt")),
            None
        ),
        fixture!("32632"),
        fixture!("2154"),
        fixture!("3857"),
        fixture!("27700"),
        fixture!("5514"),
        fixture!("3035"),
        fixture!("2193"),
        fixture!("2065"),
    ]
}

fn projinfo_text(text: &str) -> &str {
    // projinfo ends its output with an empty line
    text.trim_end_matches('\n')
}

fn parse_projjson(fixture: &Fixture) -> Crs {
    Crs::from_projjson(fixture.projjson).unwrap_or_else(|e| panic!("EPSG:{}: {e}", fixture.code))
}

fn parse_wkt(name: &str, wkt: &str) -> Crs {
    Crs::from_wkt(wkt).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn written(name: &str, crs: &Crs, dialect: WktDialect, layout: WktLayout) -> String {
    crs.to_wkt(dialect, layout)
        .unwrap_or_else(|e| panic!("{name} {dialect:?}: {e}"))
}

#[test]
fn wkt2_writer_matches_projinfo_byte_for_byte() {
    for fixture in fixtures() {
        let crs = parse_projjson(&fixture);
        for (dialect, expected) in [
            (WktDialect::Wkt2_2019, fixture.wkt2_2019),
            (WktDialect::Wkt2_2015, fixture.wkt2_2015),
        ] {
            let wkt = written(fixture.code, &crs, dialect, WktLayout::MultiLine);
            assert_eq!(
                wkt,
                projinfo_text(expected),
                "EPSG:{} {dialect:?}",
                fixture.code
            );
        }
    }
}

#[test]
fn wkt1_esri_writer_matches_projinfo_byte_for_byte() {
    for fixture in fixtures() {
        let Some(expected) = fixture.wkt1_esri else {
            continue;
        };
        let wkt = written(
            fixture.code,
            &parse_projjson(&fixture),
            WktDialect::Wkt1Esri,
            WktLayout::SingleLine,
        );
        let reread = projinfo_reread(fixture.code, WktDialect::Wkt1Esri).unwrap_or(expected);
        assert_eq!(wkt, projinfo_text(reread), "EPSG:{}", fixture.code);
    }
}

#[test]
fn wkt1_gdal_written_from_its_own_parse_matches_projinfo() {
    for fixture in fixtures() {
        let Some(expected) = fixture.wkt1_gdal else {
            continue;
        };
        let crs = parse_wkt(fixture.code, expected);
        let wkt = written(
            fixture.code,
            &crs,
            WktDialect::Wkt1Gdal,
            WktLayout::MultiLine,
        );
        let reread = projinfo_reread(fixture.code, WktDialect::Wkt1Gdal).unwrap_or(expected);
        assert_eq!(wkt, projinfo_text(reread), "EPSG:{}", fixture.code);
    }
}

#[test]
fn dialects_projinfo_refuses_are_refused() {
    let fixtures = fixtures();
    let crs_of = |code: &str| {
        let fixture = fixtures
            .iter()
            .find(|fixture| fixture.code == code)
            .expect("fixture");
        parse_projjson(fixture)
    };
    let geographic_3d = crs_of("4979").to_wkt(WktDialect::Wkt1Gdal, WktLayout::MultiLine);
    assert!(
        matches!(&geographic_3d, Err(Error::UnsupportedWkt(message)) if message == "WKT1 does not support Geographic 3D CRS."),
        "{geographic_3d:?}"
    );
    let geocentric = crs_of("4978").to_wkt(WktDialect::Wkt1Esri, WktLayout::SingleLine);
    assert!(
        matches!(&geocentric, Err(Error::UnsupportedWkt(message)) if message == "Geocentric CRS not supported in WKT1_ESRI"),
        "{geocentric:?}"
    );
}

#[test]
fn wkt2_reads_back_equivalent_to_projjson() {
    for fixture in fixtures() {
        let projjson = parse_projjson(&fixture);
        for (dialect, wkt) in [
            (WktDialect::Wkt2_2019, fixture.wkt2_2019),
            (WktDialect::Wkt2_2015, fixture.wkt2_2015),
        ] {
            let crs = parse_wkt(fixture.code, wkt);
            assert!(
                crs.is_equivalent_to(&projjson, Criterion::Equivalent),
                "EPSG:{} {dialect:?}: {crs:?}",
                fixture.code
            );
            let rewritten = written(fixture.code, &crs, dialect, WktLayout::MultiLine);
            let reread = projinfo_reread(fixture.code, dialect).unwrap_or(wkt);
            assert_eq!(
                rewritten,
                projinfo_text(reread),
                "EPSG:{} {dialect:?} round trip",
                fixture.code
            );
        }
    }
}

fn projinfo_reread(code: &str, dialect: WktDialect) -> Option<&'static str> {
    // projinfo given the PROJJSON fixture for ESRI, or the same dialect's fixture, writes these instead
    match (code, dialect) {
        // proj.db keeps the grad to 16 digits, the PROJJSON and WKT1 grad to 15, so the Paris meridian rounds differently
        ("4807", WktDialect::Wkt1Esri) => {
            Some(include_str!("data/wkt/wkt1_esri/epsg_4807_reread.wkt"))
        }
        ("4807", WktDialect::Wkt1Gdal) => {
            Some(include_str!("data/wkt/wkt1_gdal/epsg_4807_reread.wkt"))
        }
        // WKT1 GDAL leaves out axes that are not east, north, and PROJ reads them back as easting, northing
        ("3035", WktDialect::Wkt1Gdal) => {
            Some(include_str!("data/wkt/wkt1_gdal/epsg_3035_reread.wkt"))
        }
        ("2193", WktDialect::Wkt1Gdal) => {
            Some(include_str!("data/wkt/wkt1_gdal/epsg_2193_reread.wkt"))
        }
        ("2065", WktDialect::Wkt1Gdal) => {
            Some(include_str!("data/wkt/wkt1_gdal/epsg_2065_reread.wkt"))
        }
        // PROJ reads a projected CRS named like "WGS 84 / Pseudo-Mercator" as its built-in web mercator on the WGS 84 datum
        ("3857", WktDialect::Wkt2_2019) => {
            Some(include_str!("data/wkt/wkt2_2019/epsg_3857_reread.wkt"))
        }
        ("3857", WktDialect::Wkt2_2015) => {
            Some(include_str!("data/wkt/wkt2_2015/epsg_3857_reread.wkt"))
        }
        _ => None,
    }
}

#[test]
fn wkt1_reads_back_equivalent_to_projjson_up_to_geographic_axis_order() {
    for fixture in fixtures() {
        let projjson = parse_projjson(&fixture);
        let dialects = [
            (WktDialect::Wkt1Gdal, fixture.wkt1_gdal),
            (WktDialect::Wkt1Esri, fixture.wkt1_esri),
        ];
        for (dialect, wkt) in dialects {
            let Some(wkt) = wkt else {
                continue;
            };
            let crs = parse_wkt(fixture.code, wkt);
            if fixture.code == "2065" && dialect == WktDialect::Wkt1Gdal {
                // WKT1 writes both Krovak variants as PROJECTION["Krovak"] and PROJ reads it back as the north oriented one
                let Crs::Projected(projected) = &crs else {
                    panic!("EPSG:2065 is projected");
                };
                assert_eq!(projected.conversion.method.identifiers[0].code, "1041");
                continue;
            }
            let expected = with_axes_wkt1_carries(&projjson, &crs);
            assert!(
                crs.is_equivalent_to(&expected, Criterion::EquivalentExceptAxisOrderGeogcrs),
                "EPSG:{} {dialect:?}: {crs:?}",
                fixture.code
            );
        }
    }
}

fn with_axes_wkt1_carries(projjson: &Crs, wkt1: &Crs) -> Crs {
    // WKT1 GDAL writes projected axes only when they are easting, northing, and WKT1 ESRI never
    let (Crs::Projected(projected), Crs::Projected(from_wkt1)) = (projjson, wkt1) else {
        return projjson.clone();
    };
    let directions: Vec<AxisDirection> = projected
        .coordinate_system
        .axes
        .iter()
        .map(|axis| axis.direction)
        .collect();
    if directions == [AxisDirection::East, AxisDirection::North] {
        return projjson.clone();
    }
    Crs::Projected(ProjectedCrs {
        coordinate_system: from_wkt1.coordinate_system.clone(),
        ..projected.clone()
    })
}

#[test]
fn wkt1_gdal_geographic_crs_takes_its_axes_from_the_registry() {
    let fixture = &fixtures()[0];
    let crs = parse_wkt("4326", fixture.wkt1_gdal.expect("WKT1 of 4326"));
    let Crs::Geographic(geographic) = &crs else {
        panic!("EPSG:4326 is geographic");
    };
    assert_eq!(geographic.identifiers[0].code, "4326");
    let axes = &geographic.coordinate_system.axes;
    assert_eq!(axes[0].name, "Geodetic latitude");
    assert_eq!(axes[0].abbreviation, "Lat");
    assert_eq!(axes[0].direction, AxisDirection::North);
    assert!(geographic.coordinate_system_is_implicit);
    assert!(crs.is_equivalent_to(&parse_projjson(fixture), Criterion::Equivalent));
}

#[test]
fn wkt2_2019_base_crs_takes_the_registry_coordinate_system_as_projjson_gives_it() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.code == "27700")
        .expect("27700");
    let (Crs::Projected(read), Crs::Projected(projjson)) = (
        parse_wkt("27700", fixture.wkt2_2019),
        parse_projjson(&fixture),
    ) else {
        panic!("EPSG:27700 is projected");
    };
    // proj.db names unit 9122 "degree (supplier to define representation)" and PROJ's createUnitOfMeasure calls it "degree"
    assert_eq!(
        read.base_crs.coordinate_system,
        projjson.base_crs.coordinate_system
    );
}

#[test]
fn projected_crs_ignores_the_axis_order_of_an_implicit_base() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.code == "2154")
        .expect("2154");
    let projjson = parse_projjson(&fixture);
    let Crs::Projected(mut from_esri) = parse_wkt("2154", fixture.wkt1_esri.expect("ESRI of 2154"))
    else {
        panic!("EPSG:2154 is projected");
    };
    assert!(from_esri.base_crs.coordinate_system_is_implicit);
    assert_eq!(
        from_esri.base_crs.coordinate_system.axes[0].direction,
        AxisDirection::East
    );
    let implicit = Crs::Projected(from_esri.clone());
    assert!(implicit.is_equivalent_to(&projjson, Criterion::Equivalent));

    from_esri.base_crs.coordinate_system_is_implicit = false;
    assert!(!Crs::Projected(from_esri).is_equivalent_to(&projjson, Criterion::Equivalent));
}

#[test]
fn esri_names_read_back_to_official_names() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.code == "2154")
        .expect("2154");
    let Crs::Projected(projected) = parse_wkt("2154", fixture.wkt1_esri.expect("ESRI of 2154"))
    else {
        panic!("EPSG:2154 is projected");
    };
    assert_eq!(projected.name, "RGF93 v1 / Lambert-93");
    assert_eq!(projected.base_crs.name, "RGF93 v1");
    let GeodeticDatum::ReferenceFrame(datum) = &projected.base_crs.datum else {
        panic!("RGF93 v1 has one datum");
    };
    assert_eq!(datum.name, "Reseau Geodesique Francais 1993 v1");
    assert_eq!(datum.ellipsoid.name, "GRS 1980");
}

// projinfo ESRI:102003 -o WKT1:ESRI -q and -o PROJJSON -q
const ESRI_102003_WKT: &str = include_str!("data/wkt/wkt1_esri/esri_102003.wkt");
const ESRI_102003_PROJJSON: &str = include_str!("data/wkt/projjson/esri_102003.json");

#[test]
fn esri_only_names_read_and_write_back() {
    let crs = parse_wkt("ESRI:102003", ESRI_102003_WKT);
    let projjson = Crs::from_projjson(ESRI_102003_PROJJSON).expect("PROJJSON of ESRI:102003");
    assert_eq!(crs.name(), "USA_Contiguous_Albers_Equal_Area_Conic");
    assert!(crs.is_equivalent_to(&projjson, Criterion::EquivalentExceptAxisOrderGeogcrs));
    let wkt = written(
        "ESRI:102003",
        &crs,
        WktDialect::Wkt1Esri,
        WktLayout::SingleLine,
    );
    assert_eq!(wkt, projinfo_text(ESRI_102003_WKT));
}

// projinfo "+proj=longlat +ellps=intl +towgs84=-87,-98,-121 +type=crs" -o WKT1:GDAL -q, then that WKT -o WKT2:2019 -q
const TOWGS84_WKT1: &str = include_str!("data/wkt/wkt1_gdal/longlat_intl_towgs84.wkt");
const TOWGS84_WKT2: &str = include_str!("data/wkt/wkt2_2019/longlat_intl_towgs84.wkt");

#[test]
fn wkt1_towgs84_becomes_a_bound_crs_to_wgs84() {
    let crs = parse_wkt("TOWGS84", TOWGS84_WKT1);
    let Crs::Bound(bound) = &crs else {
        panic!("TOWGS84 reads as a BoundCRS");
    };
    assert_eq!(bound.target_crs.name(), "WGS 84");
    assert_eq!(bound.transformation.method.identifiers[0].code, "9606");
    let values: Vec<f64> = bound
        .transformation
        .parameter_values
        .iter()
        .map(|parameter| match &parameter.value {
            ParameterValue::Measure(measure) => measure.value,
            other => panic!("Helmert parameters are measures, got {other:?}"),
        })
        .collect();
    assert_eq!(values, [-87.0, -98.0, -121.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(
        written("TOWGS84", &crs, WktDialect::Wkt1Gdal, WktLayout::MultiLine),
        projinfo_text(TOWGS84_WKT1)
    );
    assert_eq!(
        written("TOWGS84", &crs, WktDialect::Wkt2_2019, WktLayout::MultiLine),
        projinfo_text(TOWGS84_WKT2)
    );
    let reread = parse_wkt("BOUNDCRS", TOWGS84_WKT2);
    assert!(reread.is_equivalent_to(&crs, Criterion::Equivalent));
}

// projinfo "+proj=lcc +lat_1=46.5194302239869 +lat_0=46.5194302239869 +lon_0=3 +k_0=0.999051028637469
// +x_0=700000 +y_0=6602157.83881034 +ellps=GRS80 +units=m +no_defs +type=crs" -o WKT2:2019 -q
const LCC_1SP_LAMBERT_93: &str = include_str!("data/wkt/wkt2_2019/lcc_1sp_lambert_93.wkt");

#[test]
fn lcc_1sp_and_2sp_forms_of_lambert_93_are_equivalent() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.code == "2154")
        .expect("2154");
    let Crs::Projected(lambert_93) = parse_projjson(&fixture) else {
        panic!("EPSG:2154 is projected");
    };
    let Crs::Projected(one_standard_parallel) = parse_wkt("LCC 1SP", LCC_1SP_LAMBERT_93) else {
        panic!("LCC 1SP is projected");
    };
    let with_1sp_conversion = ProjectedCrs {
        conversion: one_standard_parallel.conversion.clone(),
        ..lambert_93.clone()
    };
    let lambert_93 = Crs::Projected(lambert_93);
    let with_1sp_conversion = Crs::Projected(with_1sp_conversion);
    assert!(lambert_93.is_equivalent_to(&with_1sp_conversion, Criterion::Equivalent));
    assert!(with_1sp_conversion.is_equivalent_to(&lambert_93, Criterion::Equivalent));

    let Crs::Projected(mut shifted) = with_1sp_conversion else {
        unreachable!();
    };
    if let ParameterValue::Measure(false_northing) =
        &mut shifted.conversion.parameter_values[4].value
    {
        false_northing.value += 1.0;
    }
    assert!(!lambert_93.is_equivalent_to(&Crs::Projected(shifted), Criterion::Equivalent));
}

// PROJ test/unit/test_io.cpp wkt_parse.wkt1_EPSG_4267
const WKT1_EPSG_4267: &str = concat!(
    "GEOGCS[\"NAD27\",",
    "    DATUM[\"North_American_Datum_1927\",",
    "        SPHEROID[\"Clarke 1866\",6378206.4,294.978698213898,",
    "            AUTHORITY[\"EPSG\",\"7008\"]],",
    "        AUTHORITY[\"EPSG\",\"6267\"]],",
    "    PRIMEM[\"Greenwich\",0,",
    "        AUTHORITY[\"EPSG\",\"8901\"]],",
    "    UNIT[\"degree\",0.0174532925199433,",
    "        AUTHORITY[\"EPSG\",\"9122\"]],",
    "    AUTHORITY[\"EPSG\",\"4267\"]]",
);

#[test]
fn proj_wkt1_epsg_4267_takes_the_official_datum_name() {
    let Crs::Geographic(crs) = parse_wkt("4267", WKT1_EPSG_4267) else {
        panic!("NAD27 is geographic");
    };
    let GeodeticDatum::ReferenceFrame(datum) = &crs.datum else {
        panic!("NAD27 has one datum");
    };
    assert_eq!(datum.identifiers.len(), 1);
    assert_eq!(datum.identifiers[0].code, "6267");
    assert_eq!(datum.identifiers[0].code_space, "EPSG");
    assert_eq!(datum.name, "North American Datum 1927");
}

// PROJ test/unit/test_io.cpp wkt_parse.wkt1_EPSG_4807_grad_mess
const WKT1_EPSG_4807_GRAD_MESS: &str = concat!(
    "GEOGCS[\"NTF (Paris)\",\n",
    "    DATUM[\"Nouvelle_Triangulation_Francaise_Paris\",\n",
    "        SPHEROID[\"Clarke 1880 (IGN)\",6378249.2,293.466021293627,\n",
    "            AUTHORITY[\"EPSG\",\"6807\"]],\n",
    "        AUTHORITY[\"EPSG\",\"6807\"]],\n",
    "    PRIMEM[\"Paris\",2.33722917,\n",
    "        AUTHORITY[\"EPSG\",\"8903\"]],\n",
    "    UNIT[\"grad\",0.015707963267949,\n",
    "        AUTHORITY[\"EPSG\",\"9105\"]],\n",
    "    AXIS[\"latitude\",NORTH],\n",
    "    AXIS[\"longitude\",EAST],\n",
    "    AUTHORITY[\"EPSG\",\"4807\"]]",
);

#[test]
fn proj_wkt1_paris_meridian_in_degrees_under_grads_is_corrected() {
    let Crs::Geographic(crs) = parse_wkt("4807", WKT1_EPSG_4807_GRAD_MESS) else {
        panic!("NTF (Paris) is geographic");
    };
    let GeodeticDatum::ReferenceFrame(datum) = &crs.datum else {
        panic!("NTF (Paris) has one datum");
    };
    assert_eq!(datum.prime_meridian.longitude.unit.name, "grad");
    assert_eq!(datum.prime_meridian.longitude.value, 2.5969213);
}

// PROJ test/unit/test_io.cpp wkt_parse.wkt1_esri_EPSG_4901_grad
const WKT1_ESRI_EPSG_4901_GRAD: &str = concat!(
    "GEOGCS[\"GCS_ATF_Paris\",DATUM[\"D_ATF\",",
    "SPHEROID[\"Plessis_1817\",6376523.0,308.64]],",
    "PRIMEM[\"Paris_RGS\",2.33720833333333],",
    "UNIT[\"Grad\",0.0157079632679489]]",
);

#[test]
fn proj_wkt1_esri_paris_rgs_meridian_is_in_degrees() {
    let Crs::Geographic(crs) = parse_wkt("4901", WKT1_ESRI_EPSG_4901_GRAD) else {
        panic!("ATF (Paris) is geographic");
    };
    let GeodeticDatum::ReferenceFrame(datum) = &crs.datum else {
        panic!("ATF (Paris) has one datum");
    };
    assert_eq!(datum.prime_meridian.name, "Paris RGS");
    assert_eq!(datum.prime_meridian.longitude.unit, UnitOfMeasure::degree());
    assert!((datum.prime_meridian.longitude.value - 2.33720833333333).abs() < 1e-14);
}

// PROJ test/unit/test_io.cpp wkt_parse.wkt1_esri_LINUNIT
const WKT1_ESRI_LINUNIT: &str = concat!(
    "GEOGCS[\"WGS_1984_3D\",DATUM[\"D_WGS_1984\",",
    "SPHEROID[\"WGS_1984\",6378137.0,298.257223563]],",
    "PRIMEM[\"Greenwich\",0.0],",
    "UNIT[\"Degree\",0.0174532925199433],",
    "LINUNIT[\"Meter\",1.0]]",
);

#[test]
fn proj_wkt1_esri_linunit_adds_an_ellipsoidal_height() {
    let Crs::Geographic(crs) = parse_wkt("LINUNIT", WKT1_ESRI_LINUNIT) else {
        panic!("WGS_1984_3D is geographic");
    };
    let axes = &crs.coordinate_system.axes;
    assert_eq!(axes.len(), 3);
    assert!((axes[0].unit.conversion_to_si - 0.0174532925199433).abs() < 1e-15);
    assert!((axes[1].unit.conversion_to_si - 0.0174532925199433).abs() < 1e-15);
    assert_eq!(axes[2].unit, UnitOfMeasure::metre());
}

// PROJ test/unit/test_io.cpp wkt_parse.wkt1_non_conformant_inf_inverse_flattening
const WKT1_INF_INVERSE_FLATTENING: &str = concat!(
    "GEOGCS[\"GCS_sphere\",DATUM[\"D_unknown\",",
    "SPHEROID[\"Spherical_Earth\",6370997,\"inf\"]],",
    "PRIMEM[\"Greenwich\",0],UNIT[\"Degree\",0.017453292519943295]]",
);

#[test]
fn proj_wkt1_inf_inverse_flattening_is_a_sphere() {
    let Crs::Geographic(crs) = parse_wkt("inf", WKT1_INF_INVERSE_FLATTENING) else {
        panic!("GCS_sphere is geographic");
    };
    assert_eq!(crs.datum.ellipsoid().shape, EllipsoidShape::Sphere);
    assert_eq!(crs.datum.ellipsoid().semi_major_axis.value, 6370997.0);
}

// PROJ test/unit/test_io.cpp wkt_parse.wkt1_geocentric_with_z_OTHER
const WKT1_GEOCENTRIC_WITH_Z_OTHER: &str = concat!(
    "GEOCCS[\"WGS 84 (geocentric)\",\n",
    "    DATUM[\"WGS_1984\",\n",
    "        SPHEROID[\"WGS 84\",6378137,298.257223563,\n",
    "            AUTHORITY[\"EPSG\",\"7030\"]],\n",
    "        AUTHORITY[\"EPSG\",\"6326\"]],\n",
    "    PRIMEM[\"Greenwich\",0,\n",
    "        AUTHORITY[\"EPSG\",\"8901\"]],\n",
    "    UNIT[\"metre\",1,\n",
    "        AUTHORITY[\"EPSG\",\"9001\"]],\n",
    "    AXIS[\"Geocentric X\",OTHER],\n",
    "    AXIS[\"Geocentric Y\",OTHER],\n",
    "    AXIS[\"Geocentric Z\",OTHER],\n",
    "    AUTHORITY[\"EPSG\",\"4328\"]]",
);

#[test]
fn proj_wkt1_geocentric_axes_take_geocentric_directions() {
    let Crs::Geodetic(crs) = parse_wkt("4328", WKT1_GEOCENTRIC_WITH_Z_OTHER) else {
        panic!("GEOCCS is geocentric");
    };
    assert_eq!(crs.name, "WGS 84 (geocentric)");
    let names: Vec<(&str, &str, AxisDirection)> = crs
        .coordinate_system
        .axes
        .iter()
        .map(|axis| {
            (
                axis.name.as_str(),
                axis.abbreviation.as_str(),
                axis.direction,
            )
        })
        .collect();
    assert_eq!(
        names,
        [
            ("Geocentric X", "X", AxisDirection::GeocentricX),
            ("Geocentric Y", "Y", AxisDirection::GeocentricY),
            ("Geocentric Z", "Z", AxisDirection::GeocentricZ),
        ]
    );
    let GeodeticDatum::ReferenceFrame(datum) = &crs.datum else {
        panic!("one datum");
    };
    assert_eq!(datum.name, "World Geodetic System 1984");
    assert_eq!(
        datum.ellipsoid.shape,
        EllipsoidShape::InverseFlattening(298.257223563)
    );
}

// PROJ test/unit/test_io.cpp wkt_parse.wkt1_projected_no_axis
const WKT1_PROJECTED_NO_AXIS: &str = concat!(
    "PROJCS[\"WGS 84 / UTM zone 31N\",\n",
    "    GEOGCS[\"WGS 84\",\n",
    "        DATUM[\"WGS_1984\",\n",
    "            SPHEROID[\"WGS 84\",6378137,298.257223563,\n",
    "                AUTHORITY[\"EPSG\",\"7030\"]],\n",
    "            AUTHORITY[\"EPSG\",\"6326\"]],\n",
    "        PRIMEM[\"Greenwich\",0,\n",
    "            AUTHORITY[\"EPSG\",\"8901\"]],\n",
    "        UNIT[\"degree\",0.0174532925199433,\n",
    "            AUTHORITY[\"EPSG\",\"9122\"]],\n",
    "        AXIS[\"latitude\",NORTH],\n",
    "        AXIS[\"longitude\",EAST],\n",
    "        AUTHORITY[\"EPSG\",\"4326\"]],\n",
    "    PROJECTION[\"Transverse_Mercator\"],\n",
    "    PARAMETER[\"latitude_of_origin\",0],\n",
    "    PARAMETER[\"central_meridian\",3],\n",
    "    PARAMETER[\"scale_factor\",0.9996],\n",
    "    PARAMETER[\"false_easting\",500000],\n",
    "    PARAMETER[\"false_northing\",0],\n",
    "    UNIT[\"metre\",1,\n",
    "        AUTHORITY[\"EPSG\",\"9001\"]],\n",
    "    AUTHORITY[\"EPSG\",\"32631\"]]",
);

#[test]
fn proj_wkt1_projected_without_axis_maps_wkt1_parameters_to_epsg() {
    let Crs::Projected(crs) = parse_wkt("32631", WKT1_PROJECTED_NO_AXIS) else {
        panic!("UTM 31N is projected");
    };
    assert_eq!(crs.name, "WGS 84 / UTM zone 31N");
    assert_eq!(crs.identifiers[0].code, "32631");
    assert_eq!(crs.conversion.method.name, "Transverse Mercator");
    let parameters: Vec<(&str, f64)> = crs
        .conversion
        .parameter_values
        .iter()
        .map(|parameter| match &parameter.value {
            ParameterValue::Measure(measure) => (parameter.parameter.name.as_str(), measure.value),
            other => panic!("measure expected, got {other:?}"),
        })
        .collect();
    assert_eq!(
        parameters,
        [
            ("Latitude of natural origin", 0.0),
            ("Longitude of natural origin", 3.0),
            ("Scale factor at natural origin", 0.9996),
            ("False easting", 500000.0),
            ("False northing", 0.0),
        ]
    );
    let axes = &crs.coordinate_system.axes;
    assert_eq!(
        (axes[0].name.as_str(), axes[0].direction),
        ("Easting", AxisDirection::East)
    );
    assert_eq!(
        (axes[1].name.as_str(), axes[1].direction),
        ("Northing", AxisDirection::North)
    );
}

// PROJ test/unit/test_io.cpp wkt_parse.wkt1_polar_stereographic_latitude_of_origin_70
const WKT1_POLAR_STEREOGRAPHIC_70: &str = concat!(
    "PROJCS[\"unknown\",\n",
    "    GEOGCS[\"unknown\",\n",
    "        DATUM[\"WGS_1984\",\n",
    "            SPHEROID[\"WGS 84\",6378137,298.257223563,\n",
    "                AUTHORITY[\"EPSG\",\"7030\"]],\n",
    "            AUTHORITY[\"EPSG\",\"6326\"]],\n",
    "        PRIMEM[\"Greenwich\",0,\n",
    "            AUTHORITY[\"EPSG\",\"8901\"]],\n",
    "        UNIT[\"degree\",0.0174532925199433,\n",
    "            AUTHORITY[\"EPSG\",\"9122\"]]],\n",
    "    PROJECTION[\"Polar_Stereographic\"],\n",
    "    PARAMETER[\"latitude_of_origin\",70],\n",
    "    PARAMETER[\"central_meridian\",2],\n",
    "    PARAMETER[\"false_easting\",3],\n",
    "    PARAMETER[\"false_northing\",4],\n",
    "    UNIT[\"metre\",1,\n",
    "        AUTHORITY[\"EPSG\",\"9001\"]]]",
);

#[test]
fn proj_wkt1_polar_stereographic_70_is_variant_b_with_south_axes() {
    let Crs::Projected(crs) = parse_wkt("polar 70", WKT1_POLAR_STEREOGRAPHIC_70) else {
        panic!("polar stereographic is projected");
    };
    assert_eq!(crs.conversion.method.identifiers[0].code, "9829");
    let axes = &crs.coordinate_system.axes;
    assert_eq!(
        (axes[0].name.as_str(), axes[0].direction),
        ("Easting", AxisDirection::South)
    );
    assert_eq!(
        (axes[1].name.as_str(), axes[1].direction),
        ("Northing", AxisDirection::South)
    );
}

// PROJ test/unit/test_io.cpp wkt_parse.wkt1_hotine_oblique_mercator_without_rectified_grid_angle
const WKT1_HOTINE_WITHOUT_RECTIFIED_GRID_ANGLE: &str = concat!(
    "PROJCS[\"NAD_1983_Michigan_GeoRef_Meters\",",
    "GEOGCS[\"NAD83(1986)\",",
    "DATUM[\"North_American_Datum_1983\",",
    "SPHEROID[\"GRS_1980\",6378137,298.257222101]],",
    "PRIMEM[\"Greenwich\",0],",
    "UNIT[\"Degree\",0.017453292519943295]],",
    "PROJECTION[\"Hotine_Oblique_Mercator\"],",
    "PARAMETER[\"false_easting\",2546731.496],",
    "PARAMETER[\"false_northing\",-4354009.816],",
    "PARAMETER[\"latitude_of_center\",45.30916666666666],",
    "PARAMETER[\"longitude_of_center\",-86],",
    "PARAMETER[\"azimuth\",-22.74444],",
    "PARAMETER[\"scale_factor\",0.9996],",
    "UNIT[\"Meter\",1]]",
);

#[test]
fn proj_wkt1_hotine_gains_the_rectified_grid_angle_from_the_azimuth() {
    let crs = parse_wkt("Hotine", WKT1_HOTINE_WITHOUT_RECTIFIED_GRID_ANGLE);
    let wkt = written("Hotine", &crs, WktDialect::Wkt1Gdal, WktLayout::MultiLine);
    assert!(
        wkt.contains("PARAMETER[\"rectified_grid_angle\",-22.74444]"),
        "{wkt}"
    );
}

// PROJ test/unit/test_io.cpp wkt_parse.wkt1_geographic_with_PROJ4_extension
const WKT1_GEOGRAPHIC_WITH_PROJ4_EXTENSION: &str = concat!(
    "GEOGCS[\"WGS 84\",\n",
    "    DATUM[\"unknown\",\n",
    "        SPHEROID[\"WGS84\",6378137,298.257223563]],\n",
    "    PRIMEM[\"Greenwich\",0],\n",
    "    UNIT[\"degree\",0.0174532925199433],\n",
    "    EXTENSION[\"PROJ4\",\"+proj=longlat +foo=bar +wktext\"]]",
);

#[test]
fn proj4_extension_the_model_cannot_hold_is_refused() {
    let err = Crs::from_wkt(WKT1_GEOGRAPHIC_WITH_PROJ4_EXTENSION).unwrap_err();
    assert!(matches!(err, Error::UnsupportedCrs(_)), "{err}");
    assert!(err.to_string().contains("EXTENSION PROJ4"), "{err}");
}

// PROJ test/unit/test_io.cpp wkt_parse.dynamic_geodetic_reference_frame_with_model
const WKT2_DYNAMIC_FRAME_WITH_MODEL: &str = concat!(
    "GEOGCRS[\"WGS 84 (G1762)\",",
    "DYNAMIC[FRAMEEPOCH[2005.0],MODEL[\"my_model\"]],",
    "TRF[\"World Geodetic System 1984 (G1762)\",",
    "    ELLIPSOID[\"WGS 84\",6378137,298.257223563],",
    "    ANCHOR[\"My anchor\"]],",
    "CS[ellipsoidal,3],",
    "    AXIS[\"(lat)\",north,ANGLEUNIT[\"degree\",0.0174532925199433]],",
    "    AXIS[\"(lon)\",east,ANGLEUNIT[\"degree\",0.0174532925199433]],",
    "    AXIS[\"ellipsoidal height (h)\",up,LENGTHUNIT[\"metre\",1.0]]",
    "]",
);

#[test]
fn proj_wkt2_dynamic_frame_keeps_epoch_model_and_anchor() {
    let Crs::Geographic(crs) = parse_wkt("G1762", WKT2_DYNAMIC_FRAME_WITH_MODEL) else {
        panic!("WGS 84 (G1762) is geographic");
    };
    let GeodeticDatum::ReferenceFrame(datum) = &crs.datum else {
        panic!("one datum");
    };
    let dynamic = datum.dynamic.as_ref().expect("a dynamic frame");
    assert_eq!(dynamic.frame_reference_epoch, 2005.0);
    assert_eq!(dynamic.deformation_model_name.as_deref(), Some("my_model"));
    assert_eq!(datum.anchor.as_deref(), Some("My anchor"));
    assert_eq!(crs.coordinate_system.axes[0].name, "Latitude");
    assert_eq!(crs.coordinate_system.axes[2].name, "Ellipsoidal height");
}

#[test]
fn crs_kinds_outside_the_model_raise_naming_the_kind() {
    let cases = [
        (
            "VERTCRS[\"EGM96 height\",VDATUM[\"EGM96 geoid\"],CS[vertical,1],AXIS[\"gravity-related height (H)\",up,LENGTHUNIT[\"metre\",1]]]",
            "VerticalCRS",
        ),
        (
            "COMPOUNDCRS[\"x\",GEOGCRS[\"a\"],VERTCRS[\"b\"]]",
            "CompoundCRS",
        ),
        (
            "ENGCRS[\"site\",EDATUM[\"site\"],CS[Cartesian,2],AXIS[\"x\",east],AXIS[\"y\",north],LENGTHUNIT[\"metre\",1]]",
            "EngineeringCRS",
        ),
        ("LOCAL_CS[\"site\"]", "EngineeringCRS"),
        (
            "DERIVEDPROJCRS[\"x\",BASEPROJCRS[\"y\"]]",
            "DerivedProjectedCRS",
        ),
    ];
    for (wkt, kind) in cases {
        let err = Crs::from_wkt(wkt).unwrap_err();
        assert!(
            matches!(&err, Error::UnsupportedCrs(message) if message == kind),
            "{wkt}: {err}"
        );
    }
}

#[test]
fn malformed_and_oversized_wkt_raise() {
    let deep = format!("GEOGCS[{}{}]", "A[".repeat(20), "]".repeat(20));
    assert!(matches!(Crs::from_wkt(&deep), Err(Error::InvalidWkt(_))));
    let huge = format!("GEOGCS[\"{}\"]", "x".repeat(2 << 20));
    assert!(matches!(Crs::from_wkt(&huge), Err(Error::InvalidWkt(_))));
    assert!(matches!(
        Crs::from_wkt("GEOGCS[\"x\""),
        Err(Error::InvalidWkt(_))
    ));
    assert!(matches!(
        Crs::from_wkt("DATUM[\"x\",ELLIPSOID[\"y\",6378137,298]]"),
        Err(Error::InvalidWkt(_))
    ));
}

#[test]
fn single_line_layout_joins_the_multi_line_output() {
    let fixture = &fixtures()[0];
    let crs = parse_projjson(fixture);
    let single = written("4326", &crs, WktDialect::Wkt1Gdal, WktLayout::SingleLine);
    let multi = written("4326", &crs, WktDialect::Wkt1Gdal, WktLayout::MultiLine);
    let joined: String = multi.lines().map(str::trim_start).collect();
    assert_eq!(single, joined);
    assert!(!single.contains('\n'));
}

// projinfo EPSG:5105 -o WKT1:ESRI -q and -o PROJJSON -q
const EPSG_5105_ESRI: &str = include_str!("data/wkt/wkt1_esri/epsg_5105.wkt");
const EPSG_5105_PROJJSON: &str = include_str!("data/wkt/projjson/epsg_5105.json");

#[test]
fn esri_etrs89_projected_crs_takes_the_norwegian_base_epsg_moved_it_to() {
    let crs = parse_wkt("5105", EPSG_5105_ESRI);
    let Crs::Projected(projected) = &crs else {
        panic!("EPSG:5105 is projected");
    };
    assert_eq!(projected.name, "ETRS89-NOR [EUREF89] / NTM zone 5");
    assert_eq!(projected.base_crs.name, "ETRS89-NOR [EUREF89]");
    let projjson = Crs::from_projjson(EPSG_5105_PROJJSON).expect("PROJJSON of EPSG:5105");
    let expected = with_axes_wkt1_carries(&projjson, &crs);
    assert!(crs.is_equivalent_to(&expected, Criterion::EquivalentExceptAxisOrderGeogcrs));
}

// projinfo EPSG:10875 -o WKT1:GDAL -q
const EPSG_10875_GDAL: &str = include_str!("data/wkt/wkt1_gdal/epsg_10875.wkt");

#[test]
fn wkt1_datum_name_no_datum_resembles_takes_the_authority_name() {
    let Crs::Geographic(crs) = parse_wkt("10875", EPSG_10875_GDAL) else {
        panic!("EPSG:10875 is geographic");
    };
    let GeodeticDatum::ReferenceFrame(datum) = &crs.datum else {
        panic!("one datum");
    };
    assert_eq!(datum.name, "ETRS89-NOR [EUREF89]");
}

#[test]
fn wkt1_datum_name_of_one_character_takes_the_authority_name() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.code == "4269")
        .expect("4269");
    let wkt = fixture
        .wkt1_gdal
        .expect("WKT1 of 4269")
        .replace("North_American_Datum_1983", "A_");
    let Crs::Geographic(crs) = parse_wkt("A_", &wkt) else {
        panic!("geographic");
    };
    let GeodeticDatum::ReferenceFrame(datum) = &crs.datum else {
        panic!("one datum");
    };
    // projinfo given this WKT names the datum "North American Datum 1983"
    assert_eq!(datum.name, "North American Datum 1983");
}

#[test]
fn wkt1_datum_name_a_datum_alias_resembles_stays_as_written() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.code == "4269")
        .expect("4269");
    let wkt = fixture
        .wkt1_gdal
        .expect("WKT1 of 4269")
        .replace("North_American_Datum_1983", "M_poraloko")
        .replace("\"6269\"", "\"6266\"");
    let Crs::Geographic(crs) = parse_wkt("M'poraloko", &wkt) else {
        panic!("geographic");
    };
    let GeodeticDatum::ReferenceFrame(datum) = &crs.datum else {
        panic!("one datum");
    };
    // PROJ's approximate search finds M'poraloko through its ESRI alias D_Mporaloko, whose name is not equivalent
    assert_eq!(datum.name, "M_poraloko");
}

#[test]
fn wkt1_written_from_the_registry_keeps_proj_db_precision() {
    // projinfo EPSG:<code> -o <dialect> -q: the Paris meridian in grads and the Everest 1830 axes in Indian feet
    let cases = [
        (
            4807,
            WktDialect::Wkt1Gdal,
            include_str!("data/wkt/wkt1_gdal/epsg_4807.wkt"),
        ),
        (
            4807,
            WktDialect::Wkt1Esri,
            include_str!("data/wkt/wkt1_esri/epsg_4807.wkt"),
        ),
        (
            4243,
            WktDialect::Wkt1Gdal,
            include_str!("data/wkt/wkt1_gdal/epsg_4243.wkt"),
        ),
        (
            4243,
            WktDialect::Wkt1Esri,
            include_str!("data/wkt/wkt1_esri/epsg_4243.wkt"),
        ),
    ];
    for (code, dialect, expected) in cases {
        let crs =
            create_coordinate_reference_system(code).unwrap_or_else(|e| panic!("EPSG:{code}: {e}"));
        let layout = if dialect == WktDialect::Wkt1Esri {
            WktLayout::SingleLine
        } else {
            WktLayout::MultiLine
        };
        assert_eq!(
            written(&code.to_string(), &crs, dialect, layout),
            projinfo_text(expected),
            "EPSG:{code} {dialect:?}"
        );
    }
}

// projinfo EPSG:9895 -o WKT1:ESRI -q, and projinfo given that WKT -o PROJJSON -q
const EPSG_9895_ESRI: &str = include_str!("data/wkt/wkt1_esri/epsg_9895.wkt");
const EPSG_9895_FROM_ESRI_PROJJSON: &str =
    include_str!("data/wkt/projjson/epsg_9895_from_wkt1_esri.json");

#[test]
fn esri_projected_3d_crs_is_written_as_projcs_and_vertcs() {
    let crs = create_coordinate_reference_system(9895).expect("EPSG:9895");
    let wkt = written("9895", &crs, WktDialect::Wkt1Esri, WktLayout::SingleLine);
    assert_eq!(wkt, projinfo_text(EPSG_9895_ESRI));
}

#[test]
fn esri_projcs_and_vertcs_read_as_a_projected_3d_crs() {
    let crs = parse_wkt("9895", EPSG_9895_ESRI);
    let Crs::Projected(projected) = &crs else {
        panic!("EPSG:9895 is projected");
    };
    assert_eq!(projected.coordinate_system.axes.len(), 3);
    assert_eq!(projected.base_crs.coordinate_system.axes.len(), 3);
    // PROJ's _exportToJSON writes measures with 15 significant digits and leaves out CRS::hasImplicitCS
    let read = Crs::from_projjson(&crs.to_projjson()).expect("PROJJSON of the read CRS");
    let expected =
        Crs::from_projjson(EPSG_9895_FROM_ESRI_PROJJSON).expect("projinfo's PROJJSON of EPSG:9895");
    assert_eq!(read, expected);
}

fn assert_reads_as_projinfo(name: &str, wkt: &str, projinfo_projjson: &str) -> Crs {
    let crs = parse_wkt(name, wkt);
    // PROJ's _exportToJSON writes measures with 15 significant digits and leaves out CRS::hasImplicitCS
    let read = Crs::from_projjson(&crs.to_projjson()).expect("PROJJSON of the read CRS");
    let expected = Crs::from_projjson(projinfo_projjson)
        .unwrap_or_else(|e| panic!("projinfo's PROJJSON of {name}: {e}"));
    assert_eq!(read, expected, "{name}");
    crs
}

#[test]
fn wkt1_utm_conversion_takes_the_epsg_utm_name_and_code() {
    // projinfo given the WKT1 ESRI of EPSG:32632 -o PROJJSON -q
    let crs = assert_reads_as_projinfo(
        "EPSG:32632 WKT1 ESRI",
        include_str!("data/wkt/wkt1_esri/epsg_32632.wkt"),
        include_str!("data/wkt/projjson/epsg_32632_from_wkt1_esri.json"),
    );
    let Crs::Projected(projected) = &crs else {
        panic!("EPSG:32632 is projected");
    };
    assert_eq!(projected.conversion.name, "UTM zone 32N");
    assert_eq!(projected.conversion.identifiers[0].code, "16032");
}

fn datum_name(crs: &Crs) -> &str {
    let datum = match crs {
        Crs::Geographic(geographic) => &geographic.datum,
        Crs::Projected(projected) => &projected.base_crs.datum,
        _ => panic!("geographic or projected"),
    };
    let GeodeticDatum::ReferenceFrame(frame) = datum else {
        panic!("one datum");
    };
    &frame.name
}

#[test]
fn wkt1_datum_takes_the_name_of_the_first_datum_proj_db_finds() {
    // projinfo EPSG:<code> -o <dialect> -q, and projinfo given that WKT -o PROJJSON -q
    let cases = [
        (
            "EPSG:4475 WKT1 GDAL",
            include_str!("data/wkt/wkt1_gdal/epsg_4475.wkt"),
            include_str!("data/wkt/projjson/epsg_4475_from_wkt1_gdal.json"),
            // IGNF's CADASTRE 1997 sorts before EPSG's Cadastre 1997
            "CADASTRE 1997",
        ),
        (
            "EPSG:10807 WKT1 ESRI",
            include_str!("data/wkt/wkt1_esri/epsg_10807.wkt"),
            include_str!("data/wkt/projjson/epsg_10807_from_wkt1_esri.json"),
            // D_Nordic_Geodetic_Commission_ETRF14 is no ESRI alias and the WKT has no AUTHORITY
            "Nordic Geodetic Commission ETRF14",
        ),
    ];
    for (name, wkt, projjson, expected_datum_name) in cases {
        let crs = assert_reads_as_projinfo(name, wkt, projjson);
        assert_eq!(datum_name(&crs), expected_datum_name, "{name}");
    }
}

#[test]
fn esri_etrs89_projected_crs_takes_the_remarks_of_its_new_base() {
    // projinfo given the WKT1 ESRI of EPSG:5105 -o PROJJSON -q
    let crs = assert_reads_as_projinfo(
        "EPSG:5105 WKT1 ESRI",
        EPSG_5105_ESRI,
        include_str!("data/wkt/projjson/epsg_5105_from_wkt1_esri.json"),
    );
    let Crs::Projected(projected) = &crs else {
        panic!("EPSG:5105 is projected");
    };
    assert_eq!(
        projected.base_crs.remarks,
        "Horizontal component of the national realization of ETRS89 in Norway."
    );
}

#[test]
fn etrf2000_pl_cs92_takes_the_base_crs_of_epsg_2180() {
    // projinfo EPSG:2180 -o <dialect> -q, and projinfo given that WKT -o PROJJSON -q
    let cases = [
        (
            "EPSG:2180 WKT2 2015",
            include_str!("data/wkt/wkt2_2015/epsg_2180.wkt"),
            include_str!("data/wkt/projjson/epsg_2180_from_wkt2_2015.json"),
        ),
        (
            "EPSG:2180 WKT1 ESRI",
            include_str!("data/wkt/wkt1_esri/epsg_2180.wkt"),
            include_str!("data/wkt/projjson/epsg_2180_from_wkt1_esri.json"),
        ),
    ];
    for (name, wkt, projjson) in cases {
        let crs = assert_reads_as_projinfo(name, wkt, projjson);
        let Crs::Projected(projected) = &crs else {
            panic!("{name} is projected");
        };
        assert_eq!(projected.name, "ETRF2000-PL / CS92", "{name}");
        assert_eq!(projected.base_crs.identifiers[0].code, "9702", "{name}");
    }
}
