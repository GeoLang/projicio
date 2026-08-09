//! Parse an embedded proj4 definition into parameters projicio builds its own
//! projection from.
//!
//! Every token in a definition is either consumed by name or the whole
//! definition is rejected, so an accepted definition never carries meaning that
//! was silently dropped. A rejected definition leaves the EPSG code with the
//! classification it already had.

use crate::projection::{
    AmericanPolyconic, CassiniSoldner, EqualEarth, HotineObliqueMercator, Laborde, Projection,
};
use crate::{
    Coord, Ellipsoid, Error, GeocentricCoord, Geographic, HelmertTransform, geocentric_to_geodetic,
    geodetic_to_geocentric,
};

/// Angles are degrees and lengths are meters, matching the rest of projicio.
#[derive(Debug, Clone)]
pub struct Definition {
    pub method: Method,
    pub ellipsoid: Ellipsoid,
    pub datum_shift: DatumShift,
    pub false_easting: f64,
    pub false_northing: f64,
    /// Size of the projected unit in meters, from `+units` or `+to_meter`.
    pub to_meter: f64,
}

/// The projection method and the parameters it takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Method {
    CassiniSoldner {
        lat_0: f64,
        lon_0: f64,
    },
    HotineObliqueMercator {
        lat_c: f64,
        lon_c: f64,
        azimuth: f64,
        rectified_grid_angle: f64,
        k_0: f64,
        /// False easting and northing are at the projection centre (EPSG 9815)
        /// rather than at the natural origin (EPSG 9812, proj4's `+no_uoff`).
        offsets_at_centre: bool,
    },
    AmericanPolyconic {
        lat_0: f64,
        lon_0: f64,
    },
    EqualEarth {
        lon_0: f64,
    },
    Laborde {
        lat_0: f64,
        lon_0: f64,
        azimuth: f64,
        k_0: f64,
    },
}

/// How geographic coordinates on this CRS reach WGS84.
#[derive(Debug, Clone)]
pub enum DatumShift {
    /// The definition names no datum, so proj skips the shift entirely and
    /// longitude and latitude pass through untouched. projicio matches that.
    None,
    Helmert(HelmertTransform),
}

impl DatumShift {
    pub fn is_none(&self) -> bool {
        matches!(self, DatumShift::None)
    }
}

/// Ellipsoids named by the definitions projicio builds natively, from proj's table.
const ELLIPSOIDS: [(&str, f64, Flattening); 9] = [
    (
        "WGS84",
        6_378_137.0,
        Flattening::InverseFlattening(298.257_223_563),
    ),
    (
        "GRS80",
        6_378_137.0,
        Flattening::InverseFlattening(298.257_222_101),
    ),
    (
        "GRS67",
        6_378_160.0,
        Flattening::InverseFlattening(298.247_167_427),
    ),
    (
        "aust_SA",
        6_378_160.0,
        Flattening::InverseFlattening(298.25),
    ),
    (
        "bessel",
        6_377_397.155,
        Flattening::InverseFlattening(299.152_812_8),
    ),
    (
        "clrk66",
        6_378_206.4,
        Flattening::SemiMinorAxis(6_356_583.8),
    ),
    (
        "evrstSS",
        6_377_298.556,
        Flattening::InverseFlattening(300.801_7),
    ),
    ("helmert", 6_378_200.0, Flattening::InverseFlattening(298.3)),
    ("intl", 6_378_388.0, Flattening::InverseFlattening(297.0)),
];

#[derive(Debug, Clone, Copy)]
enum Flattening {
    InverseFlattening(f64),
    SemiMinorAxis(f64),
}

/// Datums named by the definitions projicio builds natively, from proj's table.
/// Only datums that resolve to a Helmert shift are listed: the grid-backed ones
/// stay with the fallback engine so they keep reporting that they need a grid.
const DATUMS: [(&str, &str, [f64; 7]); 3] = [
    ("WGS84", "WGS84", [0.0; 7]),
    ("NAD83", "GRS80", [0.0; 7]),
    (
        "potsdam",
        "bessel",
        [598.1, 73.7, 418.2, 0.202, 0.045, -2.455, 6.7],
    ),
];

/// Projected units named by the definitions projicio builds natively.
const UNITS: [(&str, f64); 3] = [("m", 1.0), ("ft", 0.3048), ("link", 0.201_168)];

const GREENWICH: &str = "greenwich";
const EAST_NORTH_UP: &str = "enu";

/// A CRS built from a proj4 definition with projicio's own projection math.
///
/// Projected coordinates are in the definition's own unit. Geographic
/// coordinates are degrees on the definition's own datum until
/// [`Self::shift_to_wgs84`] moves them.
pub struct NativeCrs {
    projection: Box<dyn Projection + Send + Sync>,
    datum_shift: DatumShift,
    to_meter: f64,
}

impl NativeCrs {
    /// Build from a proj4 definition, or say why projicio cannot.
    pub fn from_definition(definition: &str) -> Result<Self, Error> {
        Self::build(&parse(definition)?)
    }

    pub fn build(definition: &Definition) -> Result<Self, Error> {
        let ellipsoid = definition.ellipsoid;
        let false_easting = definition.false_easting;
        let false_northing = definition.false_northing;
        let projection: Box<dyn Projection + Send + Sync> = match definition.method {
            Method::CassiniSoldner { lat_0, lon_0 } => Box::new(CassiniSoldner::new(
                ellipsoid,
                lat_0,
                lon_0,
                false_easting,
                false_northing,
            )),
            Method::HotineObliqueMercator {
                lat_c,
                lon_c,
                azimuth,
                rectified_grid_angle,
                k_0,
                offsets_at_centre,
            } => Box::new(HotineObliqueMercator::new(
                ellipsoid,
                lat_c,
                lon_c,
                azimuth,
                rectified_grid_angle,
                k_0,
                false_easting,
                false_northing,
                offsets_at_centre,
            )?),
            Method::AmericanPolyconic { lat_0, lon_0 } => Box::new(AmericanPolyconic::new(
                ellipsoid,
                lat_0,
                lon_0,
                false_easting,
                false_northing,
            )),
            Method::EqualEarth { lon_0 } => Box::new(EqualEarth::new(
                ellipsoid,
                lon_0,
                false_easting,
                false_northing,
            )),
            Method::Laborde {
                lat_0,
                lon_0,
                azimuth,
                k_0,
            } => Box::new(Laborde::new(
                ellipsoid,
                lat_0,
                lon_0,
                azimuth,
                k_0,
                false_easting,
                false_northing,
            )?),
        };
        Ok(Self {
            projection,
            datum_shift: definition.datum_shift.clone(),
            to_meter: definition.to_meter,
        })
    }

    /// True when the definition names a datum. proj skips the datum shift for a
    /// pair where either side does not, and projicio follows it.
    pub fn names_a_datum(&self) -> bool {
        !self.datum_shift.is_none()
    }

    pub fn to_geographic(&self, x: f64, y: f64) -> Result<Geographic, Error> {
        self.projection
            .inverse(Coord::new(x * self.to_meter, y * self.to_meter))
    }

    pub fn from_geographic(&self, geo: Geographic) -> Result<(f64, f64), Error> {
        let projected = self.projection.forward(geo)?;
        Ok((projected.x / self.to_meter, projected.y / self.to_meter))
    }

    /// A shift moves the ellipsoidal height too, so the caller has to carry the
    /// height it returns into whatever shifts next.
    pub fn shift_to_wgs84(&self, geo: Geographic, height: f64) -> (Geographic, f64) {
        let DatumShift::Helmert(helmert) = &self.datum_shift else {
            return (geo, height);
        };
        let geocentric = geodetic_to_geocentric(
            geo.lat.to_radians(),
            geo.lon.to_radians(),
            height,
            self.projection.ellipsoid(),
        );
        let (lat, lon, height) =
            geocentric_to_geodetic(&helmert.forward(&geocentric), &Ellipsoid::WGS84);
        (Geographic::new(lon.to_degrees(), lat.to_degrees()), height)
    }

    pub fn shift_from_wgs84(&self, geo: Geographic, height: f64) -> Geographic {
        let DatumShift::Helmert(helmert) = &self.datum_shift else {
            return geo;
        };
        let geocentric: GeocentricCoord = geodetic_to_geocentric(
            geo.lat.to_radians(),
            geo.lon.to_radians(),
            height,
            &Ellipsoid::WGS84,
        );
        let (lat, lon, _) =
            geocentric_to_geodetic(&helmert.inverse(&geocentric), self.projection.ellipsoid());
        Geographic::new(lon.to_degrees(), lat.to_degrees())
    }
}

impl std::fmt::Debug for NativeCrs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeCrs")
            .field("to_meter", &self.to_meter)
            .field("datum_shift", &self.datum_shift)
            .finish()
    }
}

/// True when a proj4 definition names a datum, by the same three parameters
/// proj reads it from.
pub fn names_a_datum(definition: &str) -> bool {
    ["+towgs84=", "+datum=", "+nadgrids="]
        .iter()
        .any(|parameter| definition.contains(parameter))
}

/// Parse a proj4 definition, or say why projicio cannot build it natively.
pub fn parse(definition: &str) -> Result<Definition, Error> {
    let mut params = Params::split(definition)?;

    let proj = params.take_value("proj")?;
    params.take_flag("no_defs");
    params.take_flag("wktext");

    if let Some(pm) = params.take_value_opt("pm")? {
        if !pm.eq_ignore_ascii_case(GREENWICH) {
            return Err(reject(format!("+pm={pm} is not the greenwich meridian")));
        }
    }
    if let Some(axis) = params.take_value_opt("axis")? {
        if !axis.eq_ignore_ascii_case(EAST_NORTH_UP) {
            return Err(reject(format!("+axis={axis} is not east-north-up")));
        }
    }

    let (ellipsoid, datum_shift) = parse_datum(&mut params)?;
    let to_meter = parse_to_meter(&mut params)?;
    let false_easting = params.take_number("x_0")?.unwrap_or(0.0);
    let false_northing = params.take_number("y_0")?.unwrap_or(0.0);

    let method = parse_method(proj, &mut params)?;
    params.reject_leftovers(proj)?;

    Ok(Definition {
        method,
        ellipsoid,
        datum_shift,
        false_easting,
        false_northing,
        to_meter,
    })
}

fn parse_method(proj: &str, params: &mut Params) -> Result<Method, Error> {
    match proj {
        "cass" => Ok(Method::CassiniSoldner {
            lat_0: params.take_number("lat_0")?.unwrap_or(0.0),
            lon_0: params.take_number("lon_0")?.unwrap_or(0.0),
        }),
        "omerc" => {
            let azimuth = params
                .take_number("alpha")?
                .ok_or_else(|| reject("+proj=omerc needs +alpha".to_string()))?;
            Ok(Method::HotineObliqueMercator {
                lat_c: params.take_number("lat_0")?.unwrap_or(0.0),
                lon_c: params
                    .take_number("lonc")?
                    .ok_or_else(|| reject("+proj=omerc needs +lonc".to_string()))?,
                azimuth,
                rectified_grid_angle: params.take_number("gamma")?.unwrap_or(azimuth),
                k_0: take_scale(params)?,
                offsets_at_centre: !params.take_flag("no_uoff"),
            })
        }
        "poly" => Ok(Method::AmericanPolyconic {
            lat_0: params.take_number("lat_0")?.unwrap_or(0.0),
            lon_0: params.take_number("lon_0")?.unwrap_or(0.0),
        }),
        "eqearth" => Ok(Method::EqualEarth {
            lon_0: params.take_number("lon_0")?.unwrap_or(0.0),
        }),
        "labrd" => Ok(Method::Laborde {
            lat_0: params.take_number("lat_0")?.unwrap_or(0.0),
            lon_0: params.take_number("lon_0")?.unwrap_or(0.0),
            azimuth: params
                .take_number("azi")?
                .ok_or_else(|| reject("+proj=labrd needs +azi".to_string()))?,
            k_0: take_scale(params)?,
        }),
        other => Err(reject(format!(
            "+proj={other} has no projicio implementation"
        ))),
    }
}

fn take_scale(params: &mut Params) -> Result<f64, Error> {
    let k = params.take_number("k")?;
    let k_0 = params.take_number("k_0")?;
    match (k, k_0) {
        (Some(_), Some(_)) => Err(reject("+k and +k_0 disagree".to_string())),
        (Some(value), None) | (None, Some(value)) => Ok(value),
        (None, None) => Ok(1.0),
    }
}

fn parse_datum(params: &mut Params) -> Result<(Ellipsoid, DatumShift), Error> {
    if let Some(grids) = params.take_value_opt("nadgrids")? {
        return Err(reject(format!("+nadgrids={grids} needs grid data")));
    }

    let ellps = params.take_value_opt("ellps")?;
    let a = params.take_number("a")?;
    let b = params.take_number("b")?;
    let rf = params.take_number("rf")?;
    let datum = params.take_value_opt("datum")?;
    let towgs84 = params.take_value_opt("towgs84")?;

    if datum.is_some() && towgs84.is_some() {
        return Err(reject("+datum and +towgs84 disagree".to_string()));
    }

    let named_ellipsoid = match (ellps, datum) {
        (Some(_), Some(_)) => return Err(reject("+ellps and +datum disagree".to_string())),
        (Some(name), None) => Some(name),
        (None, Some(name)) => Some(named_datum(name)?.0),
        (None, None) => None,
    };

    let ellipsoid = match (named_ellipsoid, a) {
        (Some(_), Some(_)) => return Err(reject("+ellps and +a disagree".to_string())),
        (Some(name), None) => named_ellipsoid_shape(name)?,
        (None, Some(a)) => axis_ellipsoid(a, b, rf)?,
        (None, None) => {
            return Err(reject(
                "no +ellps, +a or +datum names an ellipsoid".to_string(),
            ));
        }
    };

    if named_ellipsoid.is_some() && (b.is_some() || rf.is_some()) {
        return Err(reject("+ellps and +b or +rf disagree".to_string()));
    }

    let shift = match (datum, towgs84) {
        (Some(name), _) => DatumShift::Helmert(helmert(named_datum(name)?.1)),
        (None, Some(values)) => DatumShift::Helmert(parse_towgs84(values)?),
        (None, None) => DatumShift::None,
    };

    Ok((ellipsoid, shift))
}

fn axis_ellipsoid(a: f64, b: Option<f64>, rf: Option<f64>) -> Result<Ellipsoid, Error> {
    if a <= 0.0 {
        return Err(reject(format!("+a={a} is not a positive radius")));
    }
    match (b, rf) {
        (Some(_), Some(_)) => Err(reject("+b and +rf disagree".to_string())),
        (Some(b), None) if b > 0.0 && b <= a => Ok(Ellipsoid::new(a, 1.0 - b / a)),
        (Some(b), None) => Err(reject(format!("+b={b} is not a semi-minor axis of {a}"))),
        (None, Some(rf)) if rf > 0.0 => Ok(Ellipsoid::new(a, 1.0 / rf)),
        (None, Some(rf)) => Err(reject(format!("+rf={rf} is not a reverse flattening"))),
        // proj reads a semi-major axis with no flattening as a sphere
        (None, None) => Ok(Ellipsoid::new(a, 0.0)),
    }
}

fn named_ellipsoid_shape(name: &str) -> Result<Ellipsoid, Error> {
    let (_, a, flattening) = ELLIPSOIDS
        .iter()
        .find(|(id, _, _)| id.eq_ignore_ascii_case(name))
        .ok_or_else(|| reject(format!("+ellps={name} is not in projicio's table")))?;
    Ok(match flattening {
        Flattening::InverseFlattening(rf) => Ellipsoid::new(*a, 1.0 / rf),
        Flattening::SemiMinorAxis(b) => Ellipsoid::new(*a, 1.0 - b / a),
    })
}

fn named_datum(name: &str) -> Result<(&'static str, [f64; 7]), Error> {
    DATUMS
        .iter()
        .find(|(id, _, _)| id.eq_ignore_ascii_case(name))
        .map(|(_, ellipsoid, params)| (*ellipsoid, *params))
        .ok_or_else(|| reject(format!("+datum={name} is not in projicio's table")))
}

fn parse_towgs84(values: &str) -> Result<HelmertTransform, Error> {
    let mut parsed = [0.0; 7];
    let mut count = 0;
    for field in values.split(',') {
        if count == parsed.len() {
            return Err(reject(format!("+towgs84={values} has too many values")));
        }
        parsed[count] = field
            .trim()
            .parse::<f64>()
            .map_err(|_| reject(format!("+towgs84={values} is not a list of numbers")))?;
        count += 1;
    }
    if count != 3 && count != 7 {
        return Err(reject(format!(
            "+towgs84={values} is neither 3 nor 7 parameters"
        )));
    }
    Ok(helmert(parsed))
}

fn helmert(p: [f64; 7]) -> HelmertTransform {
    HelmertTransform::new(p[0], p[1], p[2], p[3], p[4], p[5], p[6])
}

fn parse_to_meter(params: &mut Params) -> Result<f64, Error> {
    let units = params.take_value_opt("units")?;
    let to_meter = params.take_number("to_meter")?;
    match (units, to_meter) {
        (Some(_), Some(_)) => Err(reject("+units and +to_meter disagree".to_string())),
        (Some(name), None) => UNITS
            .iter()
            .find(|(id, _)| id.eq_ignore_ascii_case(name))
            .map(|(_, to_meter)| *to_meter)
            .ok_or_else(|| reject(format!("+units={name} is not in projicio's table"))),
        (None, Some(to_meter)) if to_meter > 0.0 => Ok(to_meter),
        (None, Some(to_meter)) => Err(reject(format!("+to_meter={to_meter} is not positive"))),
        (None, None) => Ok(1.0),
    }
}

fn reject(reason: String) -> Error {
    Error::UnsupportedCrs(reason)
}

/// The `+key=value` and `+flag` tokens of a definition, consumed by name.
struct Params<'a> {
    tokens: Vec<(&'a str, Option<&'a str>)>,
}

impl<'a> Params<'a> {
    fn split(definition: &'a str) -> Result<Self, Error> {
        let mut tokens: Vec<(&str, Option<&str>)> = Vec::new();
        for token in definition.split_whitespace() {
            let token = token
                .strip_prefix('+')
                .ok_or_else(|| reject(format!("{token} is not a +parameter")))?;
            let (key, value) = match token.split_once('=') {
                Some((key, value)) => (key, Some(value)),
                None => (token, None),
            };
            if tokens.iter().any(|(seen, _)| *seen == key) {
                return Err(reject(format!("+{key} is given more than once")));
            }
            tokens.push((key, value));
        }
        Ok(Self { tokens })
    }

    fn take(&mut self, key: &str) -> Option<Option<&'a str>> {
        let index = self.tokens.iter().position(|(seen, _)| *seen == key)?;
        Some(self.tokens.remove(index).1)
    }

    fn take_flag(&mut self, key: &str) -> bool {
        self.take(key).is_some()
    }

    fn take_value(&mut self, key: &str) -> Result<&'a str, Error> {
        self.take_value_opt(key)?
            .ok_or_else(|| reject(format!("+{key} is missing")))
    }

    fn take_value_opt(&mut self, key: &str) -> Result<Option<&'a str>, Error> {
        match self.take(key) {
            None => Ok(None),
            Some(Some(value)) => Ok(Some(value)),
            Some(None) => Err(reject(format!("+{key} has no value"))),
        }
    }

    fn take_number(&mut self, key: &str) -> Result<Option<f64>, Error> {
        let Some(value) = self.take_value_opt(key)? else {
            return Ok(None);
        };
        let number = value
            .parse::<f64>()
            .map_err(|_| reject(format!("+{key}={value} is not a number")))?;
        if !number.is_finite() {
            return Err(reject(format!("+{key}={value} is not finite")));
        }
        Ok(Some(number))
    }

    fn reject_leftovers(&self, proj: &str) -> Result<(), Error> {
        match self.tokens.first() {
            None => Ok(()),
            Some((key, _)) => Err(reject(format!("+{key} means nothing to +proj={proj}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parses_cassini_soldner() {
        let def = parse(
            "+proj=cass +lat_0=5.421517541666667 +lon_0=100.3443769638889 +x_0=-23.414 \
             +y_0=62.283 +ellps=GRS80 +units=m +no_defs",
        )
        .unwrap();
        assert!(matches!(def.method, Method::CassiniSoldner { .. }));
        assert_eq!(def.false_easting, -23.414);
        assert_eq!(def.to_meter, 1.0);
        assert!(def.datum_shift.is_none());
        assert_eq!(def.ellipsoid, Ellipsoid::GRS80);
    }

    #[test]
    fn test_parses_hotine_variants() {
        let with_offset = parse("+proj=omerc +lonc=115 +alpha=53.3 +gamma=53.1 +ellps=GRS80")
            .unwrap()
            .method;
        let no_offset =
            parse("+proj=omerc +lonc=115 +alpha=53.3 +gamma=53.1 +no_uoff +ellps=GRS80")
                .unwrap()
                .method;
        let Method::HotineObliqueMercator {
            offsets_at_centre, ..
        } = with_offset
        else {
            panic!("expected omerc, got {with_offset:?}");
        };
        assert!(offsets_at_centre);
        let Method::HotineObliqueMercator {
            offsets_at_centre, ..
        } = no_offset
        else {
            panic!("expected omerc, got {no_offset:?}");
        };
        assert!(!offsets_at_centre);
    }

    #[test]
    fn test_ellipsoid_from_axes() {
        let def = parse("+proj=cass +a=6378293.645208759 +b=6356617.987679838").unwrap();
        assert_eq!(def.ellipsoid.a, 6_378_293.645_208_759);
        assert!((def.ellipsoid.b() - 6_356_617.987_679_838).abs() < 1e-6);
    }

    #[test]
    fn test_towgs84_seven_parameters() {
        let def = parse(
            "+proj=cass +ellps=intl +towgs84=-275.7224,94.7824,340.8944,-8.001,-4.42,-11.821,1",
        )
        .unwrap();
        let DatumShift::Helmert(helmert) = def.datum_shift else {
            panic!("expected a helmert shift");
        };
        assert_eq!(helmert.dx, -275.7224);
        assert_eq!(helmert.rz, -11.821);
        assert_eq!(helmert.ds, 1.0);
    }

    #[test]
    fn test_towgs84_three_parameters() {
        let def = parse("+proj=cass +ellps=intl +towgs84=-133.63,-157.5,-158.62").unwrap();
        let DatumShift::Helmert(helmert) = def.datum_shift else {
            panic!("expected a helmert shift");
        };
        assert_eq!(helmert.dz, -158.62);
        assert_eq!(helmert.rx, 0.0);
    }

    #[test]
    fn test_units_and_to_meter() {
        assert_eq!(
            parse("+proj=cass +a=6378137 +units=link").unwrap().to_meter,
            0.201_168
        );
        assert_eq!(
            parse("+proj=cass +a=6378137 +to_meter=0.3047972654")
                .unwrap()
                .to_meter,
            0.3047972654
        );
        assert!(parse("+proj=cass +a=6378137 +units=us-ft").is_err());
        assert!(parse("+proj=cass +a=6378137 +units=link +to_meter=0.5").is_err());
    }

    #[test]
    fn test_rejects_unknown_parameter() {
        let err = parse("+proj=cass +a=6378137 +hyperbolic").unwrap_err();
        assert!(err.to_string().contains("hyperbolic"), "{err}");
    }

    #[test]
    fn test_rejects_parameter_belonging_to_another_method() {
        // lat_1 is a conic parameter, so cassini-soldner must not swallow it
        assert!(parse("+proj=cass +a=6378137 +lat_1=30").is_err());
    }

    #[test]
    fn test_rejects_grids_prime_meridian_and_axis() {
        assert!(parse("+proj=cass +ellps=GRS80 +nadgrids=alaska").is_err());
        assert!(parse("+proj=cass +ellps=bessel +pm=ferro").is_err());
        assert!(parse("+proj=cass +ellps=GRS80 +axis=neu").is_err());
        assert!(parse("+proj=cass +ellps=GRS80 +pm=greenwich +axis=enu").is_ok());
    }

    #[test]
    fn test_rejects_unimplemented_method() {
        let err = parse("+proj=cea +lat_ts=30 +ellps=GRS80").unwrap_err();
        assert!(err.to_string().contains("cea"), "{err}");
    }

    #[test]
    fn test_rejects_unknown_ellipsoid_and_datum() {
        assert!(parse("+proj=cass +ellps=krass").is_err());
        assert!(parse("+proj=cass +datum=NAD27").is_err());
        assert!(parse("+proj=cass +datum=NAD83").is_ok());
    }

    #[test]
    fn test_rejects_missing_ellipsoid() {
        assert!(parse("+proj=cass +lat_0=1 +units=m").is_err());
    }

    #[test]
    fn test_rejects_repeated_and_malformed_tokens() {
        assert!(parse("+proj=cass +ellps=GRS80 +x_0=1 +x_0=2").is_err());
        assert!(parse("+proj=cass +ellps=GRS80 +x_0=east").is_err());
        assert!(parse("proj=cass").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn test_datum_carries_its_ellipsoid() {
        let def = parse("+proj=eqearth +lon_0=150 +datum=WGS84 +units=m +no_defs").unwrap();
        assert_eq!(def.ellipsoid, Ellipsoid::WGS84);
        assert!(!def.datum_shift.is_none());
    }
}
