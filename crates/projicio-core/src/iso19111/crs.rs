// adapted from PROJ src/iso19111/crs.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::common::ObjectDomain;
use super::coordinate_system::{AxisOrder, CartesianCs, EllipsoidalCs};
use super::datum::GeodeticDatum;
use super::metadata::Identifier;
use super::operation::{Conversion, Transformation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Criterion {
    Equivalent,
    EquivalentExceptAxisOrderGeogcrs,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeographicCrs {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub remarks: String,
    pub domains: Vec<ObjectDomain>,
    pub datum: GeodeticDatum,
    pub coordinate_system: EllipsoidalCs,
    pub coordinate_system_is_implicit: bool,
}

impl GeographicCrs {
    pub fn is_equivalent_to(&self, other: &Self, criterion: Criterion) -> bool {
        if self.datum.is_equivalent_to(&other.datum)
            && self
                .coordinate_system
                .is_equivalent_to(&other.coordinate_system)
        {
            // PROJ: GeoPackage's "Undefined geographic SRS" must not match EPSG:4326
            let undefined = "Undefined geographic SRS";
            let either_undefined = self.name == undefined || other.name == undefined;
            return !either_undefined || self.name == other.name;
        }
        if criterion != Criterion::EquivalentExceptAxisOrderGeogcrs {
            return false;
        }
        let axes = &self.coordinate_system.axes;
        let swapped = match self.coordinate_system.axis_order() {
            AxisOrder::LongEastLatNorth => EllipsoidalCs::latitude_longitude(&axes[0].unit),
            AxisOrder::LatNorthLongEast => EllipsoidalCs::longitude_latitude(&axes[0].unit),
            AxisOrder::LongEastLatNorthHeightUp => {
                EllipsoidalCs::latitude_longitude_ellipsoidal_height(&axes[0].unit, &axes[2].unit)
            }
            AxisOrder::LatNorthLongEastHeightUp => {
                EllipsoidalCs::longitude_latitude_ellipsoidal_height(&axes[0].unit, &axes[2].unit)
            }
            AxisOrder::Other => return false,
        };
        self.datum.is_equivalent_to(&other.datum)
            && swapped.is_equivalent_to(&other.coordinate_system)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeodeticCrs {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub remarks: String,
    pub domains: Vec<ObjectDomain>,
    pub datum: GeodeticDatum,
    pub coordinate_system: CartesianCs,
}

impl GeodeticCrs {
    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        self.datum.is_equivalent_to(&other.datum)
            && self
                .coordinate_system
                .is_equivalent_to(&other.coordinate_system)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedCrs {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub remarks: String,
    pub domains: Vec<ObjectDomain>,
    pub base_crs: Box<GeographicCrs>,
    pub conversion: Conversion,
    pub coordinate_system: CartesianCs,
}

impl ProjectedCrs {
    pub fn is_equivalent_to(&self, other: &Self, criterion: Criterion) -> bool {
        // PROJ ignores the base axis order when either base CRS has an implicit coordinate system
        let base_criterion = if criterion == Criterion::Equivalent
            && (self.base_crs.coordinate_system_is_implicit
                || other.base_crs.coordinate_system_is_implicit)
        {
            Criterion::EquivalentExceptAxisOrderGeogcrs
        } else {
            criterion
        };
        self.coordinate_system
            .is_equivalent_to(&other.coordinate_system)
            && self
                .base_crs
                .is_equivalent_to(&other.base_crs, base_criterion)
            && self.conversion.is_equivalent_to(
                &other.conversion,
                self.base_crs.datum.ellipsoid(),
                other.base_crs.datum.ellipsoid(),
            )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundCrs {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub remarks: String,
    pub domains: Vec<ObjectDomain>,
    pub source_crs: Box<Crs>,
    pub target_crs: Box<Crs>,
    pub transformation: Transformation,
}

impl BoundCrs {
    pub fn is_equivalent_to(&self, other: &Self, criterion: Criterion) -> bool {
        self.source_crs
            .is_equivalent_to(&other.source_crs, criterion)
            && self
                .target_crs
                .is_equivalent_to(&other.target_crs, criterion)
            && self.transformation.is_equivalent_to(&other.transformation)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Crs {
    Geographic(GeographicCrs),
    Geodetic(GeodeticCrs),
    Projected(ProjectedCrs),
    Bound(BoundCrs),
}

impl Crs {
    pub fn name(&self) -> &str {
        match self {
            Self::Geographic(crs) => &crs.name,
            Self::Geodetic(crs) => &crs.name,
            Self::Projected(crs) => &crs.name,
            Self::Bound(crs) => &crs.name,
        }
    }

    pub fn is_equivalent_to(&self, other: &Self, criterion: Criterion) -> bool {
        match (self, other) {
            (Self::Geographic(crs), Self::Geographic(other_crs)) => {
                crs.is_equivalent_to(other_crs, criterion)
            }
            (Self::Geodetic(crs), Self::Geodetic(other_crs)) => crs.is_equivalent_to(other_crs),
            (Self::Projected(crs), Self::Projected(other_crs)) => {
                crs.is_equivalent_to(other_crs, criterion)
            }
            (Self::Bound(crs), Self::Bound(other_crs)) => {
                crs.is_equivalent_to(other_crs, criterion)
            }
            _ => false,
        }
    }

    pub fn extract_geographic_crs(&self) -> Option<&GeographicCrs> {
        match self {
            Self::Geographic(crs) => Some(crs),
            Self::Geodetic(_) => None,
            Self::Projected(crs) => Some(&*crs.base_crs),
            Self::Bound(crs) => crs.source_crs.extract_geographic_crs(),
        }
    }
}
