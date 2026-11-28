use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use projicio_core::operation_factory::CandidateOperations;
use projicio_core::{Ellipsoid, Error, Transform, geodetic_to_geocentric, grids};

pub const SUITES: [(&str, &str); 2] = [("GIGS", "gigs"), ("test/gie", "proj-gie")];

const TAGS: [&str; 19] = [
    "<gie>",
    "operation",
    "crs_src",
    "crs_dst",
    "use_proj4_init_rules",
    "accept",
    "expect",
    "roundtrip",
    "banner",
    "verbose",
    "direction",
    "tolerance",
    "ignore",
    "require_grid",
    "echo",
    "skip",
    "</gie>",
    "<gie-strict>",
    "</gie-strict>",
];

const DEFAULT_TOLERANCE: f64 = 0.0005;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Setup {
    CrsPair { source: u32, target: u32 },
    InitPair { source: u32, target: u32 },
    ProjString,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Engine {
    Transform,
    CandidateOperations,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    Forward,
    Inverse,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinate {
    pub values: [f64; 4],
    pub dimensions: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    Coordinate(Coordinate),
    Failure { errno: Option<String> },
    Roundtrip { trips: u32, tolerance: f64 },
}

#[derive(Debug)]
pub struct Case {
    pub line: usize,
    pub setup: Setup,
    pub direction: Direction,
    pub tolerance: f64,
    pub accept: Coordinate,
    pub check: Check,
    pub required_grids: Vec<String>,
}

#[derive(Debug)]
pub enum Outcome {
    Passed,
    Failed(Failure),
    Skipped(SkipReason),
}

#[derive(Debug)]
pub enum Failure {
    Outside { error: f64 },
    Raised(Error),
    DidNotFail,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Outside { error } => write!(f, "off by {error:.3e}"),
            Failure::Raised(error) => write!(f, "raised {error}"),
            Failure::DidNotFail => write!(f, "returned a coordinate where failure is expected"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkipReason {
    ProjString,
    RequiredGridNotRegistered,
    DefinitionGridNotRegistered,
    ThirdCoordinate,
    Time,
    ErrorCode,
}

impl SkipReason {
    pub fn label(self) -> &'static str {
        match self {
            SkipReason::ProjString => "PROJ string operation",
            SkipReason::RequiredGridNotRegistered => "require_grid not registered",
            SkipReason::DefinitionGridNotRegistered => "unregistered grid in projicio's definition",
            SkipReason::ThirdCoordinate => "height or geocentric coordinate",
            SkipReason::Time => "time coordinate",
            SkipReason::ErrorCode => "PROJ error code expected",
        }
    }
}

pub fn gie_files(directory: &Path) -> Result<Vec<String>, String> {
    let entries =
        std::fs::read_dir(directory).map_err(|e| format!("{}: {e}", directory.display()))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", directory.display()))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|name| format!("{}: file name {name:?} is not UTF-8", directory.display()))?;
        if name.contains(".gie") {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn chomp(line: &str) -> &str {
    let uncommented = line.split_once('#').map_or(line, |(before, _)| before);
    uncommented.trim_matches(|c: char| c.is_ascii_whitespace() || c == ';')
}

fn shrink(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    let mut after_space = false;
    let mut in_string = false;
    let mut chars = chomp(text).chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            collapsed.push(c);
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    collapsed.push('"');
                    chars.next();
                } else {
                    in_string = false;
                }
            }
            continue;
        }
        let c = if c == '+' && (collapsed.is_empty() || after_space) {
            ' '
        } else {
            c
        };
        if c == '"' && collapsed.ends_with('=') {
            in_string = true;
            after_space = false;
            collapsed.push(c);
        } else if c.is_ascii_whitespace() || c == ';' {
            if !after_space && !collapsed.is_empty() {
                collapsed.push(' ');
            }
            after_space = true;
        } else {
            after_space = false;
            collapsed.push(c);
        }
    }

    let mut shrunk = String::with_capacity(collapsed.len());
    for c in collapsed.chars() {
        if (c == '=' || c == ',') && shrunk.ends_with(' ') {
            shrunk.pop();
        }
        if c == ' ' && (shrunk.ends_with('=') || shrunk.ends_with(',')) {
            continue;
        }
        shrunk.push(c);
    }
    shrunk
}

fn tag_of(line: &str) -> Option<&'static str> {
    TAGS.iter().copied().find(|tag| line.starts_with(tag))
}

fn is_decoration(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.len() >= 5 && bytes[1..5].iter().all(|&byte| byte == bytes[0])
}

struct Command {
    tag: &'static str,
    args: String,
    line: usize,
}

fn read_block(lines: &[&str], mut index: usize, commands: &mut Vec<Command>) -> usize {
    loop {
        let Some((start, tag)) =
            (index..lines.len()).find_map(|i| tag_of(lines[i]).map(|tag| (i, tag)))
        else {
            return lines.len();
        };
        if tag == "</gie>" {
            return start + 1;
        }
        let end = (start + 1..lines.len())
            .find(|&i| is_decoration(lines[i]) || tag_of(lines[i]).is_some());
        // gie drops a command that is still open at the end of the file
        let Some(end) = end else {
            return lines.len();
        };
        let mut args = lines[start][tag.len()..].to_string();
        for line in &lines[start + 1..end] {
            args.push(' ');
            args.push_str(line);
        }
        commands.push(Command {
            tag,
            args: shrink(&args),
            line: start + 1,
        });
        index = end;
    }
}

fn read_strict_block(
    lines: &[&str],
    mut index: usize,
    commands: &mut Vec<Command>,
) -> Result<usize, String> {
    while index < lines.len() {
        let line = lines[index];
        let number = index + 1;
        index += 1;
        if line.is_empty() || is_decoration(line) {
            continue;
        }
        let tag = tag_of(line)
            .ok_or_else(|| format!("line {number}: unknown command in <gie-strict>: {line}"))?;
        let mut args = shrink(&line[tag.len()..]);
        while args.ends_with('\\') {
            args.pop();
            // gie drops a continued command that is still open at the end of the file
            let Some(next) = lines.get(index) else {
                return Ok(lines.len());
            };
            index += 1;
            let next = tag_of(next).map_or(*next, |next_tag| &next[next_tag.len()..]);
            args = shrink(&format!("{args} {next}"));
        }
        if tag == "</gie-strict>" {
            return Ok(index);
        }
        commands.push(Command {
            tag,
            args,
            line: number,
        });
    }
    Ok(index)
}

fn read_commands(text: &str) -> Result<Vec<Command>, String> {
    // gie reads lines with fgets and drops a last line that has no newline
    let lines: Vec<&str> = text
        .split_inclusive('\n')
        .filter(|line| line.ends_with('\n'))
        .map(chomp)
        .collect();
    let mut commands = Vec::new();
    let mut index = 0;
    while let Some(start) = (index..lines.len())
        .find(|&i| lines[i].starts_with("<gie>") || lines[i].starts_with("<gie-strict>"))
    {
        index = if lines[start].starts_with("<gie-strict>") {
            read_strict_block(&lines, start + 1, &mut commands)?
        } else {
            read_block(&lines, start + 1, &mut commands)
        };
    }
    Ok(commands)
}

fn leading_number(word: &str) -> Option<f64> {
    if word
        .get(..3)
        .is_some_and(|start| start.eq_ignore_ascii_case("nan"))
    {
        return Some(f64::NAN);
    }
    let candidate: String = word
        .chars()
        .take_while(|c| c.is_ascii_digit() || "+-._eE".contains(*c))
        .filter(|&c| c != '_')
        .collect();
    (1..=candidate.len())
        .rev()
        .find_map(|end| candidate[..end].parse::<f64>().ok())
}

fn scaled_length(text: &str) -> Option<f64> {
    // gie's strtod_scaled turns angles into metres along GRS80's equator
    const GRS80_METRES_PER_DEGREE: f64 = 111319.4908;
    let text = text.trim_start();
    let (number, unit) = text.split_once(' ').unwrap_or((text, ""));
    let value = leading_number(number)?;
    Some(match unit {
        "km" => value * 1000.0,
        "m" => value,
        "dm" => value / 10.0,
        "cm" => value / 100.0,
        "mm" => value / 1000.0,
        "um" => value / 1e6,
        "nm" => value / 1e9,
        "rad" => GRS80_METRES_PER_DEGREE * value.to_degrees(),
        "deg" => GRS80_METRES_PER_DEGREE * value,
        _ => value,
    })
}

const DEGREE_MINUTE_SECOND_DIVISORS: [f64; 3] = [1.0, 60.0, 3600.0];

fn dms_degrees(word: &str) -> Option<f64> {
    let (mut sign, mut rest) = match word.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, word.strip_prefix('+').unwrap_or(word)),
    };
    let mut degrees = 0.0;
    let mut next_unit = 0;
    while next_unit < DEGREE_MINUTE_SECOND_DIVISORS.len() {
        let digits = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        if digits == 0 {
            break;
        }
        let value: f64 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let mut symbols = rest.chars();
        let unit = match symbols.next() {
            Some('d' | 'D' | '°') => 0,
            Some('\'') => 1,
            Some('"') => 2,
            _ => {
                degrees += value / DEGREE_MINUTE_SECOND_DIVISORS[next_unit];
                break;
            }
        };
        if unit < next_unit {
            return None;
        }
        degrees += value / DEGREE_MINUTE_SECOND_DIVISORS[unit];
        rest = symbols.as_str();
        next_unit = unit + 1;
    }
    let mut symbols = rest.chars();
    match symbols.next() {
        Some('N' | 'n' | 'E' | 'e') => {
            sign = 1.0;
            rest = symbols.as_str();
        }
        Some('S' | 's' | 'W' | 'w') => {
            sign = -1.0;
            rest = symbols.as_str();
        }
        _ => {}
    }
    rest.is_empty().then_some(sign * degrees)
}

fn coordinate_value(word: &str) -> Option<f64> {
    if word.starts_with("HUGE_VAL") {
        return Some(f64::INFINITY);
    }
    word.replace('_', "")
        .parse::<f64>()
        .ok()
        .or_else(|| dms_degrees(word))
}

fn parse_coordinate(args: &str) -> Option<Coordinate> {
    let mut values = [0.0; 4];
    let mut dimensions = 0;
    for (index, word) in args.split_whitespace().take(4).enumerate() {
        let Some(value) = coordinate_value(word) else {
            break;
        };
        values[index] = value;
        dimensions = index + 1;
    }
    (dimensions >= 2).then_some(Coordinate { values, dimensions })
}

fn epsg_code(spec: &str) -> Option<u32> {
    let (authority, code) = spec.split_once(':')?;
    if !authority.eq_ignore_ascii_case("epsg") {
        return None;
    }
    code.parse().ok()
}

fn operation_setup(operation: &str) -> Setup {
    let words: Vec<&str> = operation.split(' ').collect();
    let ["proj=pipeline", "step", source, "inv", "step", target] = words[..] else {
        return Setup::ProjString;
    };
    let init_code = |word: &str| epsg_code(word.strip_prefix("init=")?);
    match (init_code(source), init_code(target)) {
        (Some(source), Some(target)) => Setup::InitPair { source, target },
        _ => Setup::ProjString,
    }
}

fn expectation(args: &str) -> Option<Check> {
    if args.starts_with("failure") {
        let mut words = args.split_whitespace().skip(1);
        let errno = match words.next() {
            Some(word) if word.starts_with("errno") => words.next().map(str::to_string),
            _ => None,
        };
        return Some(Check::Failure { errno });
    }
    parse_coordinate(args).map(Check::Coordinate)
}

fn roundtrip(args: &str, tolerance: f64) -> Result<Check, String> {
    let (count, rest) = args.split_once(' ').unwrap_or((args, ""));
    let trips = match leading_number(count) {
        None => 100,
        Some(count) if (1.0..=1_000_000.0).contains(&count) => count as u32,
        Some(count) => return Err(format!("roundtrip count {count} is outside 1 to 1000000")),
    };
    Ok(Check::Roundtrip {
        trips,
        tolerance: scaled_length(rest).unwrap_or(tolerance),
    })
}

struct Settings {
    direction: Direction,
    tolerance: f64,
    required_grids: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            direction: Direction::Forward,
            tolerance: DEFAULT_TOLERANCE,
            required_grids: Vec::new(),
        }
    }
}

pub fn parse(text: &str) -> Result<Vec<Case>, String> {
    let mut cases = Vec::new();
    let mut setup = None;
    let mut crs_source = None;
    let mut crs_target = None;
    let mut settings = Settings::default();
    let mut accept = Coordinate {
        values: [0.0; 4],
        dimensions: 0,
    };
    for Command { tag, args, line } in read_commands(text)? {
        match tag {
            "operation" => {
                setup = Some(operation_setup(&args));
                settings = Settings::default();
            }
            "crs_src" | "crs_dst" => {
                let code = epsg_code(&args)
                    .ok_or_else(|| format!("line {line}: {tag} {args} is not an EPSG code"))?;
                if tag == "crs_src" {
                    crs_source = Some(code);
                } else {
                    crs_target = Some(code);
                }
                if let (Some(source), Some(target)) = (crs_source, crs_target) {
                    setup = Some(Setup::CrsPair { source, target });
                    settings = Settings::default();
                    crs_source = None;
                    crs_target = None;
                }
            }
            "accept" => {
                accept = parse_coordinate(&args)
                    .ok_or_else(|| format!("line {line}: accept {args} is not a coordinate"))?;
            }
            "expect" | "roundtrip" => {
                let check = if tag == "expect" {
                    expectation(&args)
                        .ok_or_else(|| format!("line {line}: expect {args} is not a coordinate"))?
                } else {
                    roundtrip(&args, settings.tolerance).map_err(|e| format!("line {line}: {e}"))?
                };
                let setup =
                    setup.ok_or_else(|| format!("line {line}: {tag} before any operation"))?;
                cases.push(Case {
                    line,
                    setup,
                    direction: settings.direction,
                    tolerance: settings.tolerance,
                    accept,
                    check,
                    required_grids: settings.required_grids.clone(),
                });
            }
            "direction" => match args.chars().next() {
                Some('f' | 'F') => settings.direction = Direction::Forward,
                Some('i' | 'I' | 'r' | 'R') => settings.direction = Direction::Inverse,
                _ => {}
            },
            "tolerance" => settings.tolerance = scaled_length(&args).unwrap_or(DEFAULT_TOLERANCE),
            "require_grid" => {
                if let Some(grid) = args.split_whitespace().next() {
                    settings.required_grids.push(grid.to_string());
                }
            }
            "skip" => break,
            _ => {}
        }
    }
    Ok(cases)
}

pub struct CrsAxis {
    kind: String,
    unit: String,
    first_axis: String,
}

struct Side {
    code: u32,
    swap_axes: bool,
    angular: bool,
    geocentric: bool,
}

impl Side {
    fn spec(&self) -> String {
        format!("EPSG:{}", self.code)
    }

    fn in_projicio_order(&self, coordinate: &Coordinate) -> (f64, f64) {
        let [x, y, ..] = coordinate.values;
        if self.swap_axes { (y, x) } else { (x, y) }
    }

    fn distance(&self, a: (f64, f64), b: (f64, f64)) -> f64 {
        // gie counts a NaN result as a match when NaN is expected
        if a.0.is_nan() && b.0.is_nan() {
            return 0.0;
        }
        if !self.angular {
            return (a.0 - b.0).hypot(a.1 - b.1);
        }
        // the chord on GRS80 is within a micrometre of gie's geodesic distance below 1 km
        let geocentric = |(longitude, latitude): (f64, f64)| {
            geodetic_to_geocentric(
                latitude.to_radians(),
                longitude.to_radians(),
                0.0,
                &Ellipsoid::GRS80,
            )
        };
        let (p, q) = (geocentric(a), geocentric(b));
        ((p.x - q.x).powi(2) + (p.y - q.y).powi(2) + (p.z - q.z).powi(2)).sqrt()
    }
}

pub struct AxisTable(HashMap<u32, CrsAxis>);

impl AxisTable {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut axes = HashMap::new();
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            let [code, kind, unit, first_axis] = fields[..] else {
                return Err(format!(
                    "{}: want four tab-separated fields in {line:?}",
                    path.display()
                ));
            };
            let code = code
                .parse::<u32>()
                .map_err(|e| format!("{}: code {code:?}: {e}", path.display()))?;
            axes.insert(
                code,
                CrsAxis {
                    kind: kind.to_string(),
                    unit: unit.to_string(),
                    first_axis: first_axis.to_string(),
                },
            );
        }
        Ok(Self(axes))
    }

    pub fn get(&self, code: u32) -> Option<&CrsAxis> {
        self.0.get(&code)
    }

    fn axis(&self, code: u32) -> &CrsAxis {
        self.get(code)
            .unwrap_or_else(|| panic!("EPSG:{code} is missing from crs_axes.tsv"))
    }

    fn crs_side(&self, code: u32) -> Side {
        let axis = self.axis(code);
        let first_axis = axis.first_axis.to_ascii_lowercase();
        // gie swaps the axes when the first axis name contains latitude or northing
        let swap_axes = first_axis.contains("latitude") || first_axis.contains("northing");
        Side {
            code,
            swap_axes,
            angular: axis.unit == "degree",
            geocentric: axis.kind == "GeodeticCRS",
        }
    }

    fn init_side(&self, code: u32) -> Side {
        let axis = self.axis(code);
        // proj4 init definitions have longitude or easting first, with longitude in degrees
        Side {
            code,
            swap_axes: false,
            angular: axis.kind == "GeographicCRS",
            geocentric: axis.kind == "GeodeticCRS",
        }
    }
}

fn unusable_dimensions(case: &Case, source: &Side, target: &Side) -> Option<SkipReason> {
    let expected = match &case.check {
        Check::Coordinate(expected) => Some(expected),
        _ => None,
    };
    let given = || std::iter::once(&case.accept).chain(expected);
    let height_given = given().any(|c| c.dimensions >= 3 && c.values[2] != 0.0);
    if source.geocentric || target.geocentric || height_given {
        return Some(SkipReason::ThirdCoordinate);
    }
    given()
        .any(|c| c.dimensions >= 4 && c.values[3] != 0.0)
        .then_some(SkipReason::Time)
}

fn within(error: f64, tolerance: f64) -> Outcome {
    if error <= tolerance {
        Outcome::Passed
    } else {
        Outcome::Failed(Failure::Outside { error })
    }
}

enum Converter {
    Transform(Transform),
    CandidateOperations {
        operations: Box<CandidateOperations>,
        source_swap_axes: bool,
        target_swap_axes: bool,
    },
}

impl Converter {
    fn new(engine: Engine, crs_pair: bool, source: &Side, target: &Side) -> Result<Self, Error> {
        if engine == Engine::CandidateOperations && crs_pair {
            return Ok(Converter::CandidateOperations {
                operations: Box::new(CandidateOperations::from_epsg(source.code, target.code)?),
                source_swap_axes: source.swap_axes,
                target_swap_axes: target.swap_axes,
            });
        }
        Ok(Converter::Transform(Transform::new(
            &source.spec(),
            &target.spec(),
        )?))
    }

    fn convert(&self, (x, y): (f64, f64)) -> Result<(f64, f64), Error> {
        match self {
            Converter::Transform(transform) => transform.convert(x, y),
            Converter::CandidateOperations {
                operations,
                source_swap_axes,
                target_swap_axes,
            } => {
                // CandidateOperations takes and gives coordinates in each CRS's own axis order
                let native = if *source_swap_axes {
                    [y, x, 0.0]
                } else {
                    [x, y, 0.0]
                };
                let [first, second, _] = operations.convert(native)?;
                Ok(if *target_swap_axes {
                    (second, first)
                } else {
                    (first, second)
                })
            }
        }
    }
}

fn roundtrip_distance(
    forward: &Converter,
    inverse: &Converter,
    source: &Side,
    start: (f64, f64),
    trips: u32,
) -> Result<f64, Error> {
    let there = |point: (f64, f64)| forward.convert(point);
    let back = |point: (f64, f64)| inverse.convert(point);
    // proj_roundtrip takes half a step forward, trips - 1 full steps, then half a step back
    let mut point = there(start)?;
    for _ in 1..trips {
        point = there(back(point)?)?;
    }
    Ok(source.distance(start, back(point)?))
}

pub fn run(case: &Case, axes: &AxisTable, engine: Engine) -> Outcome {
    let crs_pair = matches!(case.setup, Setup::CrsPair { .. });
    let (source, target) = match case.setup {
        Setup::CrsPair { source, target } => (axes.crs_side(source), axes.crs_side(target)),
        Setup::InitPair { source, target } => (axes.init_side(source), axes.init_side(target)),
        Setup::ProjString => return Outcome::Skipped(SkipReason::ProjString),
    };
    if case
        .required_grids
        .iter()
        .any(|grid| !grids::is_registered(grid))
    {
        return Outcome::Skipped(SkipReason::RequiredGridNotRegistered);
    }
    let (source, target) = match case.direction {
        Direction::Forward => (source, target),
        Direction::Inverse => (target, source),
    };
    if let Some(reason) = unusable_dimensions(case, &source, &target) {
        return Outcome::Skipped(reason);
    }
    if matches!(case.check, Check::Failure { errno: Some(_) }) {
        return Outcome::Skipped(SkipReason::ErrorCode);
    }

    let forward = match Converter::new(engine, crs_pair, &source, &target) {
        Ok(converter) => converter,
        Err(_) if matches!(case.check, Check::Failure { .. }) => return Outcome::Passed,
        Err(Error::GridError(_)) => {
            return Outcome::Skipped(SkipReason::DefinitionGridNotRegistered);
        }
        Err(error) => return Outcome::Failed(Failure::Raised(error)),
    };
    let (x, y) = source.in_projicio_order(&case.accept);
    match &case.check {
        Check::Coordinate(expected) => match forward.convert((x, y)) {
            Ok(output) => within(
                target.distance(output, target.in_projicio_order(expected)),
                case.tolerance,
            ),
            Err(error) => Outcome::Failed(Failure::Raised(error)),
        },
        Check::Failure { .. } => match forward.convert((x, y)) {
            Ok(_) => Outcome::Failed(Failure::DidNotFail),
            Err(_) => Outcome::Passed,
        },
        Check::Roundtrip { trips, tolerance } => {
            let distance = Converter::new(engine, crs_pair, &target, &source).and_then(|inverse| {
                roundtrip_distance(&forward, &inverse, &source, (x, y), *trips)
            });
            match distance {
                Ok(distance) => within(distance, *tolerance),
                Err(error) => Outcome::Failed(Failure::Raised(error)),
            }
        }
    }
}
