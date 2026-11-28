// adapted from PROJ src/iso19111/operation/coordinateoperationfactory.cpp and src/crs_to_crs.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::extent::AreaOfUse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationAuthority {
    // proj's empty authority: every authority, in the order of proj.db's authority_to_authority_preference
    Any,
    Epsg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialCriterion {
    StrictContainment,
    PartialIntersection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridAvailabilityUse {
    UseForSorting,
    DiscardOperationIfMissingGrid,
    KnownAvailable,
    IgnoreGridAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntermediateCrsUse {
    IfNoDirectTransformation,
    Never,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationContext {
    pub authority: OperationAuthority,
    pub area_of_interest: Option<AreaOfUse>,
    pub desired_accuracy: Option<f64>,
    pub spatial_criterion: SpatialCriterion,
    pub grid_availability: GridAvailabilityUse,
    pub intermediate_crs_use: IntermediateCrsUse,
}

impl OperationContext {
    pub fn new(authority: OperationAuthority) -> Self {
        // proj's CoordinateOperationContext::create defaults
        Self {
            authority,
            area_of_interest: None,
            desired_accuracy: None,
            spatial_criterion: SpatialCriterion::StrictContainment,
            grid_availability: GridAvailabilityUse::UseForSorting,
            intermediate_crs_use: IntermediateCrsUse::IfNoDirectTransformation,
        }
    }

    pub fn crs_to_crs(grid_availability: GridAvailabilityUse) -> Self {
        // proj_create_crs_to_crs_from_pj with no options
        Self {
            spatial_criterion: SpatialCriterion::PartialIntersection,
            grid_availability,
            ..Self::new(OperationAuthority::Any)
        }
    }

    pub(crate) fn discard_if_missing_grid(&self) -> bool {
        matches!(
            self.grid_availability,
            GridAvailabilityUse::DiscardOperationIfMissingGrid
                | GridAvailabilityUse::KnownAvailable
        )
    }

    pub(crate) fn consider_known_grids_as_available(&self) -> bool {
        self.grid_availability == GridAvailabilityUse::KnownAvailable
    }
}
