// adapted from PROJ src/iso19111/operation/coordinateoperationfactory.cpp (FilterResults, SortFunction, filterAndSort) and src/iso19111/factory.cpp (lookForGridInfo) (MIT), see LICENSES/PROJ-MIT.txt

use std::cmp::Ordering;
use std::collections::HashSet;

use super::candidate::{
    BALLPARK_GEOGRAPHIC_OFFSET, CandidateOperation, candidates_equivalent, is_null_transformation,
};
use super::context::{GridAvailabilityUse, OperationContext};
use super::execution::{pipeline_key, proj_exportable};
use super::extent::{AreaOfUse, pseudo_area};
use crate::iso19111::CoordinateOperation;

struct GridInfo {
    available: bool,
    known: bool,
}

fn grid_info(file_name: &str, consider_known_grids_as_available: bool) -> GridInfo {
    // proj's lookForGridInfo with no grid file readable and the network off
    if file_name == "null" {
        return GridInfo {
            available: true,
            known: true,
        };
    }
    let alternative = crate::registry::grid_alternative_for_file(file_name).filter(|alternative| {
        alternative.proj_grid_name == Some(file_name)
            || alternative.old_proj_grid_name == Some(file_name)
    });
    let downloadable = alternative
        .is_some_and(|alternative| alternative.url.is_some() && alternative.open_license);
    let available = consider_known_grids_as_available && downloadable;
    GridInfo {
        available,
        known: downloadable || available,
    }
}

pub(crate) fn rejected_for_missing_grid(
    operation: &CandidateOperation,
    consider_known_grids_as_available: bool,
) -> bool {
    // proj's rejectOpDueToMissingGrid
    operation
        .grid_file_names()
        .iter()
        .any(|file_name| !grid_info(file_name, consider_known_grids_as_available).available)
}

pub(crate) struct Extents<'a> {
    pub(crate) source: Option<&'a AreaOfUse>,
    pub(crate) target: Option<&'a AreaOfUse>,
}

pub(crate) struct Filtered {
    pub(crate) operations: Vec<CandidateOperation>,
    has_operation_containing_area_of_interest_without_grid: bool,
    area_of_interest: Option<AreaOfUse>,
}

fn area_of_interest(context: &OperationContext, extents: &Extents) -> Option<AreaOfUse> {
    // proj's computeAreaOfInterest under SourceTargetCRSExtentUse::SMALLEST
    if let Some(area) = &context.area_of_interest {
        return Some(area.clone());
    }
    match (extents.source, extents.target) {
        (Some(source), Some(target)) => {
            if pseudo_area(Some(source)) < pseudo_area(Some(target)) {
                Some(source.clone())
            } else {
                Some(target.clone())
            }
        }
        (Some(source), None) => Some(source.clone()),
        (None, target) => target.cloned(),
    }
}

fn within_desired_accuracy(operation: &CandidateOperation, context: &OperationContext) -> bool {
    let Some(desired_accuracy) = context.desired_accuracy else {
        return true;
    };
    (0.0..=desired_accuracy).contains(&operation.accuracy_or_unknown())
}

pub(crate) fn filter_results(
    operations: &[CandidateOperation],
    context: &OperationContext,
    extents: &Extents,
    force_strict_containment: bool,
) -> Filtered {
    // proj's FilterResults::filterOut, with ballpark operations always allowed
    let strict_containment = force_strict_containment
        || context.spatial_criterion == super::context::SpatialCriterion::StrictContainment;
    let area_of_interest = area_of_interest(context, extents);
    let mut has_only_ballpark = true;
    let mut has_non_ballpark_without_extent = false;
    let mut has_non_ballpark_with_extent = false;
    let mut has_operation_containing_area_of_interest_without_grid = false;
    let expected_description = context
        .area_of_interest
        .as_ref()
        .and_then(|area| area.description.clone())
        .filter(|description| {
            operations.iter().any(|operation| {
                operation
                    .area_of_use()
                    .is_some_and(|area| area.description.as_ref() == Some(description))
            })
        });

    let mut kept = Vec::new();
    for operation in operations {
        if !within_desired_accuracy(operation, context) {
            continue;
        }
        if let Some(area_of_interest) = &area_of_interest {
            let Some(extent) = operation.area_of_use() else {
                if !operation.has_ballpark_transformation {
                    has_non_ballpark_without_extent = true;
                }
                continue;
            };
            if let Some(description) = &expected_description {
                if extent.description.as_ref() != Some(description) {
                    continue;
                }
            }
            if !operation.has_ballpark_transformation {
                has_non_ballpark_with_extent = true;
            }
            let extent_contains = extent.contains(area_of_interest);
            if !has_operation_containing_area_of_interest_without_grid
                && extent_contains
                && !operation.has_ballpark_transformation
                && operation.grid_file_names().is_empty()
            {
                has_operation_containing_area_of_interest_without_grid = true;
            }
            if strict_containment && !extent_contains {
                continue;
            }
            if !strict_containment && !extent.intersects(area_of_interest) {
                continue;
            }
        }
        if !operation.has_ballpark_transformation {
            has_only_ballpark = false;
        }
        kept.push(operation.clone());
    }

    if (kept.is_empty() && !has_non_ballpark_with_extent)
        || (has_only_ballpark && has_non_ballpark_without_extent)
    {
        kept.extend(
            operations
                .iter()
                .filter(|operation| within_desired_accuracy(operation, context))
                .cloned(),
        );
    }
    Filtered {
        operations: kept,
        has_operation_containing_area_of_interest_without_grid,
        area_of_interest,
    }
}

struct Characteristics {
    area: f64,
    accuracy: f64,
    proj_exportable: bool,
    has_grids: bool,
    grids_available: bool,
    grids_known: bool,
    step_count: usize,
    ballpark: bool,
    null_transformation: bool,
    etrs89_to_etrs89: bool,
    uses_etrf2000: bool,
    uses_generic_etrs89: bool,
}

fn characteristics(
    operation: &CandidateOperation,
    context: &OperationContext,
    area_of_interest: Option<&AreaOfUse>,
    database: bool,
) -> Characteristics {
    // proj's PrecomputedOpCharacteristics; projStepCount is left at 0, which turns its criterion off
    let area = match (operation.area_of_use(), area_of_interest) {
        (Some(extent), Some(area_of_interest)) => {
            pseudo_area(extent.intersection(area_of_interest).as_ref())
        }
        (Some(extent), None) => pseudo_area(Some(&extent)),
        (None, _) => 0.0,
    };
    let mut has_grids = false;
    let mut grids_available = true;
    let mut grids_known = true;
    if database {
        for file_name in operation.grid_file_names() {
            has_grids = true;
            let info = grid_info(&file_name, context.consider_known_grids_as_available());
            if context.grid_availability == GridAvailabilityUse::UseForSorting && !info.available {
                grids_available = false;
            }
            if !info.known {
                grids_known = false;
            }
        }
    }
    let name = operation.name();
    Characteristics {
        area,
        accuracy: operation.accuracy_or_unknown(),
        proj_exportable: proj_exportable(operation),
        has_grids,
        grids_available,
        grids_known,
        step_count: operation.steps().len(),
        ballpark: operation.has_ballpark_transformation,
        null_transformation: is_null_transformation(name),
        etrs89_to_etrs89: operation.source_crs().name().starts_with("ETRS89-")
            && operation.target_crs().name().starts_with("ETRS89-"),
        uses_etrf2000: name.contains("ETRF2000"),
        uses_generic_etrs89: name.contains("ETRS89 to ETRS89-"),
    }
}

fn first_is_true(a: bool, b: bool) -> Option<bool> {
    (a != b).then_some(a)
}

fn ballpark_offset_names_similar_crs(name: &str) -> Option<bool> {
    let from = format!("{BALLPARK_GEOGRAPHIC_OFFSET} from ");
    let start = name.find(&from)? + from.len();
    let to_offset = name[start..].find(" to ")? + start;
    let source = &name[start..to_offset];
    let rest = &name[to_offset + " to ".len()..];
    let target = match rest.find(" + ") {
        Some(end) => &rest[..end],
        None => rest,
    };
    Some(source.starts_with(target) || target.starts_with(source))
}

fn sort_before(
    a: &CandidateOperation,
    a_info: &Characteristics,
    b: &CandidateOperation,
    b_info: &Characteristics,
) -> bool {
    // proj's SortFunction::compare, true when a goes before b
    if let Some(a_first) = first_is_true(a_info.proj_exportable, b_info.proj_exportable) {
        return a_first;
    }
    if let Some(a_first) = first_is_true(!a_info.ballpark, !b_info.ballpark) {
        return a_first;
    }
    if let Some(a_first) = first_is_true(!a_info.null_transformation, !b_info.null_transformation) {
        return a_first;
    }
    if let Some(a_first) = first_is_true(a_info.grids_available, b_info.grids_available) {
        return a_first;
    }
    if let Some(a_first) = first_is_true(a_info.grids_known, b_info.grids_known) {
        return a_first;
    }
    let (accuracy_a, accuracy_b) = (a_info.accuracy, b_info.accuracy);
    if accuracy_a >= 0.0 && accuracy_b < 0.0 {
        return true;
    }
    if accuracy_b >= 0.0 && accuracy_a < 0.0 {
        return false;
    }
    if accuracy_a < 0.0 && accuracy_b < 0.0 {
        if let Some(a_first) = first_is_true(a_info.has_grids, b_info.has_grids) {
            return a_first;
        }
    }
    let (area_a, area_b) = (a_info.area, b_info.area);
    if area_a > 0.0 {
        if area_a > area_b {
            return true;
        }
        if area_a < area_b {
            return false;
        }
    } else if area_b > 0.0 {
        return false;
    }
    if accuracy_a >= 0.0 && accuracy_a < accuracy_b {
        return true;
    }
    if accuracy_b >= 0.0 && accuracy_b < accuracy_a {
        return false;
    }
    if accuracy_a >= 0.0 && accuracy_a == accuracy_b {
        if let Some(a_first) = first_is_true(!a_info.has_grids, !b_info.has_grids) {
            return a_first;
        }
    }
    // proj's IOGP 373-07-7 check, whose first test can never hold as proj writes it
    if a_info.etrs89_to_etrs89
        && b_info.etrs89_to_etrs89
        && b_info.uses_etrf2000
        && !b_info.uses_generic_etrs89
        && !a_info.uses_etrf2000
        && a_info.uses_generic_etrs89
    {
        return false;
    }
    if a_info.step_count < b_info.step_count {
        return true;
    }
    if b_info.step_count < a_info.step_count {
        return false;
    }
    let (a_name, b_name) = (a.name(), b.name());
    if let (Some(similar_a), Some(similar_b)) = (
        ballpark_offset_names_similar_crs(a_name),
        ballpark_offset_names_similar_crs(b_name),
    ) {
        if let Some(a_first) = first_is_true(similar_a, similar_b) {
            return a_first;
        }
    }
    if a_name.len() != b_name.len() {
        return a_name.len() < b_name.len();
    }
    for (first, second) in [
        ("NTF (Paris) to NTF (1)", "NTF (Paris) to NTF (2)"),
        ("NTF (Paris) to RGF93 v1 (1)", "NTF (Paris) to RGF93 v1 (2)"),
    ] {
        if a_name.contains(first) && b_name.contains(second) {
            return true;
        }
        if a_name.contains(second) && b_name.contains(first) {
            return false;
        }
    }
    a_name > b_name
}

fn transformation_step_count(operation: &CandidateOperation) -> usize {
    match &operation.operation {
        CoordinateOperation::Concatenated(concatenated) => concatenated
            .steps
            .iter()
            .filter(|step| !matches!(step, CoordinateOperation::Conversion { .. }))
            .count(),
        _ => 1,
    }
}

fn same_extent(extent: Option<&AreaOfUse>, other: Option<&AreaOfUse>) -> bool {
    match (extent, other) {
        (None, None) => true,
        (Some(extent), Some(other)) => extent.contains(other) && other.contains(extent),
        _ => false,
    }
}

pub(crate) fn filter_and_sort(
    operations: &[CandidateOperation],
    context: &OperationContext,
    extents: &Extents,
    database: bool,
) -> Vec<CandidateOperation> {
    let filtered = filter_results(operations, context, extents, false);
    let infos: Vec<Characteristics> = filtered
        .operations
        .iter()
        .map(|operation| {
            characteristics(
                operation,
                context,
                filtered.area_of_interest.as_ref(),
                database,
            )
        })
        .collect();
    let mut order: Vec<usize> = (0..filtered.operations.len()).collect();
    order.sort_by(|&a, &b| {
        let (a_operation, b_operation) = (&filtered.operations[a], &filtered.operations[b]);
        if sort_before(a_operation, &infos[a], b_operation, &infos[b]) {
            Ordering::Less
        } else if sort_before(b_operation, &infos[b], a_operation, &infos[a]) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });
    let mut sorted: Vec<CandidateOperation> = order
        .into_iter()
        .map(|index| filtered.operations[index].clone())
        .collect();

    let remove_synthetic_null_transforms = |sorted: &mut Vec<CandidateOperation>| {
        if filtered.has_operation_containing_area_of_interest_without_grid && sorted.len() > 1 {
            let last = &sorted[sorted.len() - 1];
            if last.has_ballpark_transformation || is_null_transformation(last.name()) {
                sorted.pop();
            }
        }
    };
    remove_synthetic_null_transforms(&mut sorted);
    let mut interesting = remove_uninteresting_operations(sorted);
    interesting = remove_duplicate_operations(interesting);
    remove_synthetic_null_transforms(&mut interesting);
    interesting
}

fn remove_uninteresting_operations(sorted: Vec<CandidateOperation>) -> Vec<CandidateOperation> {
    // proj's removeUninterestingOps
    let mut kept: Vec<CandidateOperation> = Vec::new();
    let mut last_extent: Option<AreaOfUse> = None;
    let mut last_accuracy = -1.0;
    let mut last_step_count = 0;
    for operation in sorted {
        let accuracy = operation.accuracy_or_unknown();
        let extent = operation.area_of_use();
        // proj counts a concatenated operation with an identifier as one step, which keeps EPSG:8537
        let step_count = if operation.operation.identifiers().is_empty() {
            transformation_step_count(&operation)
        } else {
            1
        };
        if let Some(last) = kept.last() {
            if candidates_equivalent(last, &operation) {
                continue;
            }
            let worse_or_equal = (accuracy >= last_accuracy && last_accuracy >= 0.0)
                || (accuracy < 0.0 && last_accuracy >= 0.0);
            if worse_or_equal
                && same_extent(extent.as_ref(), last_extent.as_ref())
                && step_count > last_step_count
            {
                continue;
            }
        }
        kept.push(operation);
        last_step_count = step_count;
        last_extent = extent;
        last_accuracy = accuracy;
    }
    kept
}

fn remove_duplicate_operations(operations: Vec<CandidateOperation>) -> Vec<CandidateOperation> {
    // proj's removeDuplicateOps, keyed on the execution steps where proj keys on the PROJ string
    if operations.len() <= 1 {
        return operations;
    }
    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    for operation in operations {
        let Some(mut key) = pipeline_key(&operation) else {
            kept.push(operation);
            continue;
        };
        if let Some(area) = operation.area_of_use() {
            let bounding_box = area.bounding_box;
            key.push_str(&format!(
                "-{}-{}-{}-{}",
                bounding_box.west, bounding_box.south, bounding_box.east, bounding_box.north
            ));
        }
        if seen.insert(key) {
            kept.push(operation);
        }
    }
    kept
}
