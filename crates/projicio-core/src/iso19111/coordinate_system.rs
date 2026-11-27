// adapted from PROJ src/iso19111/coordinatesystem.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::common::{Measure, UnitOfMeasure};
use super::metadata::Identifier;

macro_rules! axis_directions {
    ($($variant:ident => $text:literal,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum AxisDirection {
            $($variant,)*
        }

        impl AxisDirection {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)*
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                [$(Self::$variant,)*]
                    .into_iter()
                    .find(|direction| direction.as_str().eq_ignore_ascii_case(name))
            }
        }
    };
}

axis_directions! {
    North => "north",
    NorthNorthEast => "northNorthEast",
    NorthEast => "northEast",
    EastNorthEast => "eastNorthEast",
    East => "east",
    EastSouthEast => "eastSouthEast",
    SouthEast => "southEast",
    SouthSouthEast => "southSouthEast",
    South => "south",
    SouthSouthWest => "southSouthWest",
    SouthWest => "southWest",
    WestSouthWest => "westSouthWest",
    West => "west",
    WestNorthWest => "westNorthWest",
    NorthWest => "northWest",
    NorthNorthWest => "northNorthWest",
    Up => "up",
    Down => "down",
    GeocentricX => "geocentricX",
    GeocentricY => "geocentricY",
    GeocentricZ => "geocentricZ",
    ColumnPositive => "columnPositive",
    ColumnNegative => "columnNegative",
    RowPositive => "rowPositive",
    RowNegative => "rowNegative",
    DisplayRight => "displayRight",
    DisplayLeft => "displayLeft",
    DisplayUp => "displayUp",
    DisplayDown => "displayDown",
    Forward => "forward",
    Aft => "aft",
    Port => "port",
    Starboard => "starboard",
    Clockwise => "clockwise",
    CounterClockwise => "counterClockwise",
    Towards => "towards",
    AwayFrom => "awayFrom",
    Future => "future",
    Past => "past",
    Unspecified => "unspecified",
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeMeaning {
    Exact,
    Wraparound,
}

impl RangeMeaning {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Wraparound => "wraparound",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Exact, Self::Wraparound]
            .into_iter()
            .find(|meaning| meaning.as_str().eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Meridian {
    pub longitude: Measure,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinateSystemAxis {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub abbreviation: String,
    pub direction: AxisDirection,
    pub unit: UnitOfMeasure,
    pub minimum_value: Option<f64>,
    pub maximum_value: Option<f64>,
    pub range_meaning: Option<RangeMeaning>,
    pub meridian: Option<Meridian>,
}

impl CoordinateSystemAxis {
    fn new(name: &str, abbreviation: &str, direction: AxisDirection, unit: &UnitOfMeasure) -> Self {
        Self {
            name: name.to_string(),
            identifiers: Vec::new(),
            abbreviation: abbreviation.to_string(),
            direction,
            unit: unit.clone(),
            minimum_value: None,
            maximum_value: None,
            range_meaning: None,
            meridian: None,
        }
    }

    fn latitude_north(unit: &UnitOfMeasure) -> Self {
        Self::new("Latitude", "lat", AxisDirection::North, unit)
    }

    fn longitude_east(unit: &UnitOfMeasure) -> Self {
        Self::new("Longitude", "lon", AxisDirection::East, unit)
    }

    fn ellipsoidal_height_up(unit: &UnitOfMeasure) -> Self {
        Self::new("Ellipsoidal height", "h", AxisDirection::Up, unit)
    }

    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        // PROJ compares names and abbreviations only under its STRICT criterion
        self.direction == other.direction && self.unit.is_equivalent_to(&other.unit)
    }
}

fn axes_are_equivalent(axes: &[CoordinateSystemAxis], other_axes: &[CoordinateSystemAxis]) -> bool {
    axes.len() == other_axes.len()
        && axes
            .iter()
            .zip(other_axes)
            .all(|(axis, other_axis)| axis.is_equivalent_to(other_axis))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisOrder {
    LatNorthLongEast,
    LatNorthLongEastHeightUp,
    LongEastLatNorth,
    LongEastLatNorthHeightUp,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EllipsoidalCs {
    pub axes: Vec<CoordinateSystemAxis>,
}

impl EllipsoidalCs {
    pub fn latitude_longitude(unit: &UnitOfMeasure) -> Self {
        Self {
            axes: vec![
                CoordinateSystemAxis::latitude_north(unit),
                CoordinateSystemAxis::longitude_east(unit),
            ],
        }
    }

    pub fn longitude_latitude(unit: &UnitOfMeasure) -> Self {
        Self {
            axes: vec![
                CoordinateSystemAxis::longitude_east(unit),
                CoordinateSystemAxis::latitude_north(unit),
            ],
        }
    }

    pub fn latitude_longitude_ellipsoidal_height(
        angular_unit: &UnitOfMeasure,
        linear_unit: &UnitOfMeasure,
    ) -> Self {
        Self {
            axes: vec![
                CoordinateSystemAxis::latitude_north(angular_unit),
                CoordinateSystemAxis::longitude_east(angular_unit),
                CoordinateSystemAxis::ellipsoidal_height_up(linear_unit),
            ],
        }
    }

    pub fn longitude_latitude_ellipsoidal_height(
        angular_unit: &UnitOfMeasure,
        linear_unit: &UnitOfMeasure,
    ) -> Self {
        Self {
            axes: vec![
                CoordinateSystemAxis::longitude_east(angular_unit),
                CoordinateSystemAxis::latitude_north(angular_unit),
                CoordinateSystemAxis::ellipsoidal_height_up(linear_unit),
            ],
        }
    }

    pub fn axis_order(&self) -> AxisOrder {
        let directions: Vec<AxisDirection> = self.axes.iter().map(|axis| axis.direction).collect();
        match directions.as_slice() {
            [AxisDirection::North, AxisDirection::East] => AxisOrder::LatNorthLongEast,
            [AxisDirection::North, AxisDirection::East, AxisDirection::Up] => {
                AxisOrder::LatNorthLongEastHeightUp
            }
            [AxisDirection::East, AxisDirection::North] => AxisOrder::LongEastLatNorth,
            [AxisDirection::East, AxisDirection::North, AxisDirection::Up] => {
                AxisOrder::LongEastLatNorthHeightUp
            }
            _ => AxisOrder::Other,
        }
    }

    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        axes_are_equivalent(&self.axes, &other.axes)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CartesianCs {
    pub axes: Vec<CoordinateSystemAxis>,
}

impl CartesianCs {
    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        axes_are_equivalent(&self.axes, &other.axes)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoordinateSystem {
    Ellipsoidal(EllipsoidalCs),
    Cartesian(CartesianCs),
}
