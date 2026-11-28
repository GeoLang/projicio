// adapted from PROJ src/crs_to_crs.cpp (proj_create_crs_to_crs_from_pj, pj_create_prepared_operations), src/coord_operation.cpp and src/trans.cpp (proj_trans, pj_get_suggested_operation) (MIT), see LICENSES/PROJ-MIT.txt

use super::candidate::{BALLPARK_GEOCENTRIC_TRANSLATION, CandidateOperation, operation_domains};
use super::context::{GridAvailabilityUse, OperationContext};
use super::execution::{ExecutionPlan, GeographicToCrs, NotExecutable};
use super::extent::AreaOfUse;
use super::search::create_operations;
use crate::Error;
use crate::iso19111::Crs;
use crate::iso19111::authority_factory::create_coordinate_reference_system;

// proj_trans tries the best operation and two more before it falls back
const ATTEMPTS: usize = 3;
// reproject_bbox in crs_to_crs.cpp samples each edge of an area of use at 21 points
const EDGE_STEPS: usize = 20;

struct PreparedCandidate {
    operation: CandidateOperation,
    plan: Result<ExecutionPlan, NotExecutable>,
}

impl PreparedCandidate {
    fn new(operation: CandidateOperation) -> Self {
        let plan = ExecutionPlan::new(&operation);
        Self { operation, plan }
    }

    fn run(&self, point: [f64; 3], epoch: Option<f64>) -> Result<[f64; 3], Error> {
        match &self.plan {
            Ok(plan) => plan.run(point, epoch),
            Err(not_executable) => Err(not_executable.clone().into_error(self.operation.name())),
        }
    }

    fn is_ballpark(&self) -> bool {
        // proj leaves the ballpark flag off a ballpark geocentric translation, its name tells it
        self.operation.has_ballpark_transformation
            || self
                .operation
                .name()
                .contains(BALLPARK_GEOCENTRIC_TRANSLATION)
    }
}

#[derive(Debug, Clone, Copy)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds {
    fn contains(&self, x: f64, y: f64) -> bool {
        (self.min_x..=self.max_x).contains(&x) && (self.min_y..=self.max_y).contains(&y)
    }

    fn is_world(&self) -> bool {
        self.min_x == -180.0 && self.min_y == -90.0 && self.max_x == 180.0 && self.max_y == 90.0
    }
}

struct PreparedArea {
    candidate: usize,
    source_bounds: Bounds,
    accuracy: f64,
    pseudo_area: f64,
    is_offshore: bool,
    is_unknown_area_name: bool,
    is_priority_operation: bool,
}

fn is_priority_operation(name: &str) -> bool {
    // coord_operation.cpp prefers NADCON5 NAD83 to NAD83(HARN) and the null GDA94 and GDA2020 transformations
    [
        "NAD83 to NAD83(HARN) (47)",
        "NAD83 to NAD83(HARN) (48)",
        "NAD83 to NAD83(HARN) (49)",
        "NAD83 to NAD83(HARN) (50)",
        "GDA94 to WGS 84 (1)",
        "GDA2020 to WGS 84 (2)",
    ]
    .iter()
    .any(|special| name.contains(special))
}

fn normalize_longitude(longitude: f64) -> f64 {
    // pj_get_suggested_operation's normalizeLongitude
    if longitude > 180.0 {
        let shifted = longitude - 360.0;
        if shifted > 180.0 {
            return (shifted + 180.0) % 360.0 - 180.0;
        }
        return shifted;
    }
    if longitude < -180.0 {
        let shifted = longitude + 360.0;
        if shifted < -180.0 {
            return (shifted + 180.0) % 360.0 - 180.0;
        }
        return shifted;
    }
    longitude
}

fn reproject_bounds(
    to_crs: &GeographicToCrs,
    west: f64,
    south: f64,
    east: f64,
    north: f64,
) -> Bounds {
    // crs_to_crs.cpp's reproject_bbox, and the longitude and latitude box kept for a geocentric CRS
    if to_crs.is_geocentric() {
        return Bounds {
            min_x: west,
            min_y: south,
            max_x: east,
            max_y: north,
        };
    }
    if west == -180.0 && east == 180.0 && south == -90.0 && north == 90.0 {
        return Bounds {
            min_x: -f64::MAX,
            min_y: -f64::MAX,
            max_x: f64::MAX,
            max_y: f64::MAX,
        };
    }
    let mut bounds = Bounds {
        min_x: f64::MAX,
        min_y: f64::MAX,
        max_x: -f64::MAX,
        max_y: -f64::MAX,
    };
    let step_longitude = (east - west) / EDGE_STEPS as f64;
    let step_latitude = (north - south) / EDGE_STEPS as f64;
    for step in 0..=EDGE_STEPS {
        let longitude = west + step as f64 * step_longitude;
        let latitude = south + step as f64 * step_latitude;
        for (sample_longitude, sample_latitude) in [
            (longitude, south),
            (longitude, north),
            (west, latitude),
            (east, latitude),
        ] {
            if let Some([x, y]) = to_crs.convert(sample_longitude, sample_latitude) {
                bounds.min_x = bounds.min_x.min(x);
                bounds.min_y = bounds.min_y.min(y);
                bounds.max_x = bounds.max_x.max(x);
                bounds.max_y = bounds.max_y.max(y);
            }
        }
    }
    bounds
}

pub struct CandidateOperations {
    source: Crs,
    target: Crs,
    epoch: Option<f64>,
    candidates: Vec<PreparedCandidate>,
    areas: Vec<PreparedArea>,
    // proj_create_crs_to_crs hands back the operation itself when only one is left
    single: bool,
    source_to_bounds: GeographicToCrs,
    source_is_longitude_first_degree: bool,
    source_is_latitude_first_degree: bool,
}

struct ConvertedPoint {
    point: [f64; 3],
    candidate: usize,
    through_fallback: bool,
}

impl CandidateOperations {
    pub fn new(source: &Crs, target: &Crs) -> Result<Self, Error> {
        Self::build(source, target, None)
    }

    pub fn at_epoch(source: &Crs, target: &Crs, epoch: f64) -> Result<Self, Error> {
        Self::build(source, target, Some(epoch))
    }

    pub fn from_epsg(source_code: u32, target_code: u32) -> Result<Self, Error> {
        Self::new(
            &create_coordinate_reference_system(source_code)?,
            &create_coordinate_reference_system(target_code)?,
        )
    }

    pub fn source_crs(&self) -> &Crs {
        &self.source
    }

    pub fn target_crs(&self) -> &Crs {
        &self.target
    }

    pub fn operations(&self) -> impl Iterator<Item = &CandidateOperation> {
        self.candidates.iter().map(|candidate| &candidate.operation)
    }

    pub fn convert(&self, point: [f64; 3]) -> Result<[f64; 3], Error> {
        let conversion = self.convert_as_proj_does(point)?;
        let candidate = &self.candidates[conversion.candidate];
        let outside_its_area = (self.single || conversion.through_fallback)
            && candidate.operation.has_transformation_step()
            && !self.covers(conversion.candidate, point);
        if candidate.is_ballpark() || outside_its_area {
            return Err(self.no_operation());
        }
        Ok(conversion.point)
    }

    pub fn convert_allowing_ballpark(&self, point: [f64; 3]) -> Result<[f64; 3], Error> {
        Ok(self.convert_as_proj_does(point)?.point)
    }

    fn no_operation(&self) -> Error {
        Error::NoOperation {
            source_crs: self.source.name().to_string(),
            target_crs: self.target.name().to_string(),
        }
    }

    fn build(source: &Crs, target: &Crs, epoch: Option<f64>) -> Result<Self, Error> {
        // proj_create_crs_to_crs_from_pj with no options: known grids count as available, and with no grid readable proj retries discarding them
        let source_to_bounds =
            GeographicToCrs::new(source).map_err(|error| error.into_error(source.name()))?;
        let target_to_bounds =
            GeographicToCrs::new(target).map_err(|error| error.into_error(target.name()))?;
        let candidates_with = |grid_availability| -> Result<Vec<PreparedCandidate>, Error> {
            let context = OperationContext::crs_to_crs(grid_availability);
            Ok(create_operations(source, target, &context)?
                .into_iter()
                .filter(|operation| epoch.is_some() || !operation.requires_coordinate_epoch())
                .map(PreparedCandidate::new)
                .collect())
        };
        let mut candidates = candidates_with(GridAvailabilityUse::KnownAvailable)?;
        if candidates.is_empty() {
            return Err(Error::NoOperation {
                source_crs: source.name().to_string(),
                target_crs: target.name().to_string(),
            });
        }
        let first_is_executable = candidates.len() == 1 && candidates[0].plan.is_ok();
        let mut areas = prepare_areas(&candidates, &source_to_bounds, &target_to_bounds);
        let found_executable_non_ballpark = candidates.iter().any(|candidate| {
            !candidate.operation.has_ballpark_transformation && candidate.plan.is_ok()
        });
        if !first_is_executable && !found_executable_non_ballpark {
            let discarding = candidates_with(GridAvailabilityUse::DiscardOperationIfMissingGrid)?;
            if !discarding.is_empty() {
                candidates.retain(|candidate| !candidate.operation.has_ballpark_transformation);
                candidates.extend(discarding);
                areas = prepare_areas(&candidates, &source_to_bounds, &target_to_bounds);
            }
        }
        if !first_is_executable && areas.is_empty() {
            return Err(Error::NoOperation {
                source_crs: source.name().to_string(),
                target_crs: target.name().to_string(),
            });
        }
        let single = first_is_executable || areas.len() == 1;
        if single && !first_is_executable {
            let only = areas[0].candidate;
            candidates = vec![candidates.swap_remove(only)];
            areas = prepare_areas(&candidates, &source_to_bounds, &target_to_bounds);
        }
        Ok(Self {
            source: source.clone(),
            target: target.clone(),
            epoch,
            candidates,
            areas,
            single,
            source_is_longitude_first_degree: source_to_bounds.is_longitude_first_degree(),
            source_is_latitude_first_degree: source_to_bounds.is_latitude_first_degree(),
            source_to_bounds,
        })
    }

    fn area_contains(&self, area: &PreparedArea, point: [f64; 3]) -> bool {
        let bounds = &area.source_bounds;
        if self.source_to_bounds.is_geocentric() {
            if bounds.is_world() {
                return true;
            }
            let [longitude, latitude] = self.source_to_bounds.bounds_coordinates(point);
            return bounds.contains(longitude, latitude);
        }
        let [x, y, _] = point;
        if bounds.contains(x, y) {
            return true;
        }
        let x_range = bounds.min_x..=bounds.max_x;
        let y_range = bounds.min_y..=bounds.max_y;
        if self.source_is_longitude_first_degree && y_range.contains(&y) {
            return x_range.contains(&normalize_longitude(x));
        }
        if self.source_is_latitude_first_degree && x_range.contains(&x) {
            return y_range.contains(&normalize_longitude(y));
        }
        false
    }

    fn covers(&self, candidate: usize, point: [f64; 3]) -> bool {
        self.areas
            .iter()
            .any(|area| area.candidate == candidate && self.area_contains(area, point))
    }

    fn suggested_area(
        &self,
        point: [f64; 3],
        excluded: &[Option<usize>],
        skip_non_executable: bool,
    ) -> Option<usize> {
        // pj_get_suggested_operation in the forward direction
        let mut best: Option<usize> = None;
        let mut best_accuracy = f64::MAX;
        for (index, area) in self.areas.iter().enumerate() {
            if excluded.contains(&Some(index)) || !self.area_contains(area, point) {
                continue;
            }
            let better = match best {
                None => true,
                Some(best_index) => {
                    let best_area = &self.areas[best_index];
                    let more_accurate = area.accuracy >= 0.0 && area.accuracy < best_accuracy;
                    // proj prefers the smaller of two areas of equal accuracy, and an onshore one over an offshore one
                    let smaller_with_same_accuracy = area.accuracy == best_accuracy
                        && area.pseudo_area < best_area.pseudo_area
                        && !(area.is_unknown_area_name && !best_area.is_unknown_area_name)
                        && !best_area.is_priority_operation;
                    (more_accurate || smaller_with_same_accuracy) && !area.is_offshore
                }
            };
            if !better {
                continue;
            }
            if skip_non_executable && self.candidates[area.candidate].plan.is_err() {
                continue;
            }
            best = Some(index);
            best_accuracy = area.accuracy;
        }
        best
    }

    fn convert_as_proj_does(&self, point: [f64; 3]) -> Result<ConvertedPoint, Error> {
        // proj_trans on a fresh transformation object, whose first failure turns on skipping operations that cannot run
        if self.single {
            return Ok(ConvertedPoint {
                point: self.candidates[0].run(point, self.epoch)?,
                candidate: 0,
                through_fallback: false,
            });
        }
        let mut excluded: [Option<usize>; ATTEMPTS - 1] = [None; ATTEMPTS - 1];
        let mut skip_non_executable = false;
        let mut last_error = None;
        for attempt in 0..ATTEMPTS {
            let Some(best) = self.suggested_area(point, &excluded, skip_non_executable) else {
                break;
            };
            let candidate = self.areas[best].candidate;
            match self.candidates[candidate].run(point, self.epoch) {
                Ok(converted) => {
                    return Ok(ConvertedPoint {
                        point: converted,
                        candidate,
                        through_fallback: false,
                    });
                }
                Err(error) => {
                    last_error = Some(error);
                    skip_non_executable = true;
                }
            }
            if attempt == ATTEMPTS - 1 {
                break;
            }
            excluded[attempt] = Some(best);
        }
        // proj_trans falls back on the first operation that needs no grid, wherever its area of use is
        let fallback = self
            .areas
            .iter()
            .map(|area| area.candidate)
            .find(|candidate| {
                self.candidates[*candidate]
                    .operation
                    .grid_file_names()
                    .is_empty()
            });
        if let Some(candidate) = fallback {
            return Ok(ConvertedPoint {
                point: self.candidates[candidate].run(point, self.epoch)?,
                candidate,
                through_fallback: true,
            });
        }
        Err(last_error.unwrap_or_else(|| self.no_operation()))
    }
}

fn accuracy_as_proj_reports(operation: &CandidateOperation) -> f64 {
    // proj_coordoperation_get_accuracy gives -1 for an operation with no accuracy or one it cannot read
    operation
        .operation
        .accuracies()
        .first()
        .and_then(|accuracy| accuracy.parse().ok())
        .unwrap_or(-1.0)
}

fn prepare_areas(
    candidates: &[PreparedCandidate],
    source_to_bounds: &GeographicToCrs,
    target_to_bounds: &GeographicToCrs,
) -> Vec<PreparedArea> {
    // pj_create_prepared_operations, which splits an area crossing the antimeridian in two
    let mut areas = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let operation = &candidate.operation;
        // proj_get_area_of_use reads the operation's first usage and counts an operation without one as valid over the world
        let usage = AreaOfUse::from_domains(operation_domains(&operation.operation));
        let (west, south, east, north) =
            usage.as_ref().map_or((-180.0, -90.0, 180.0, 90.0), |area| {
                let bounding_box = area.bounding_box;
                (
                    bounding_box.west,
                    bounding_box.south,
                    bounding_box.east,
                    bounding_box.north,
                )
            });
        let area_name = usage
            .as_ref()
            .and_then(|area| area.description.clone())
            .unwrap_or_default();
        let parts = if west <= east {
            vec![(west, east)]
        } else {
            vec![(west, 180.0), (-180.0, east)]
        };
        for (part_west, part_east) in parts {
            let source_bounds =
                reproject_bounds(source_to_bounds, part_west, south, part_east, north);
            let target_bounds =
                reproject_bounds(target_to_bounds, part_west, south, part_east, north);
            if source_bounds.min_x > source_bounds.max_x
                || target_bounds.min_x > target_bounds.max_x
            {
                continue;
            }
            let (radians_west, radians_east) = (part_west.to_radians(), part_east.to_radians());
            let radians_east = if radians_west > radians_east {
                radians_east + 2.0 * std::f64::consts::PI
            } else {
                radians_east
            };
            areas.push(PreparedArea {
                candidate: index,
                source_bounds,
                accuracy: accuracy_as_proj_reports(operation),
                pseudo_area: (radians_east - radians_west)
                    * (north.to_radians().sin() - south.to_radians().sin()),
                is_offshore: area_name.contains("- offshore"),
                is_unknown_area_name: area_name.is_empty() || area_name == "unknown",
                is_priority_operation: is_priority_operation(operation.name()),
            });
        }
    }
    areas
}
