// adapted from PROJ src/iso19111/operation/coordinateoperationfactory.cpp (createOperations and the functions it calls) and src/iso19111/factory.cpp (createFromCoordinateReferenceSystemCodes, createFromCRSCodesWithIntermediates, createBetweenGeodeticCRSWithDatumBasedIntermediates, createGeodeticCRSFromDatum) (MIT), see LICENSES/PROJ-MIT.txt

use std::collections::{BTreeMap, HashMap, HashSet};

use super::candidate::{CandidateOperation, concatenate, crs_equivalent, is_null_transformation};
use super::context::{IntermediateCrsUse, OperationAuthority, OperationContext};
use super::execution::proj_exportable;
use super::extent::{AreaOfUse, BoundingBox, crs_area_of_use, pseudo_area, registry_area};
use super::filter::{Extents, filter_and_sort, filter_results, rejected_for_missing_grid};
use super::synthesized::{
    axis_count, axis_order_reversal, ballpark_geocentric_translation, ballpark_geographic_offset,
    geodetic_datum, geographic_geocentric_conversion, intermediate_geographic_crs,
    is_same_geodetic_datum, longitude_rotation, null_geocentric_translation, prime_meridian,
    transformation_name, unnamed_datum,
};
use crate::Error;
use crate::iso19111::authority_factory::{
    create_coordinate_operation, create_coordinate_reference_system,
};
use crate::iso19111::common::{Measure, UnitOfMeasure};
use crate::iso19111::coordinate_system::{AxisOrder, EllipsoidalCs};
use crate::iso19111::datum::GeodeticDatum;
use crate::iso19111::{BoundCrs, CoordinateOperation, Criterion, Crs, GeographicCrs, ProjectedCrs};
use crate::registry::{
    self, CrsDefinitionKind, DirectedOperation, GeodeticCrsType, OperationCode, OperationKind,
};

// proj's createOperations stops at this recursion depth
const MAX_RECURSION_DEPTH: usize = 10;
const UNKNOWN_BASED_ON: &str = "Unknown based on ";

pub fn create_operations(
    source: &Crs,
    target: &Crs,
    context: &OperationContext,
) -> Result<Vec<CandidateOperation>, Error> {
    // proj's CoordinateOperationFactory::createOperations, without getResolvedCRS's identification by name
    let mut search = Search::new(source, target, context, true);
    let operations = search.operations(source, target)?;
    Ok(filter_and_sort(
        &operations,
        context,
        &search.extents(),
        true,
    ))
}

fn create_operation_without_database(
    source: &Crs,
    target: &Crs,
) -> Result<CandidateOperation, Error> {
    // proj's CoordinateOperationFactory::createOperation, which has no authority factory
    let context = OperationContext::new(OperationAuthority::Any);
    let mut search = Search::new(source, target, &context, false);
    let operations = search.operations(source, target)?;
    filter_and_sort(&operations, &context, &search.extents(), false)
        .into_iter()
        .next()
        .ok_or_else(|| Error::NoOperation {
            source_crs: source.name().to_string(),
            target_crs: target.name().to_string(),
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegistryAuthority {
    Proj,
    Epsg,
}

impl RegistryAuthority {
    fn issued(self, code: OperationCode<'_>) -> bool {
        matches!(
            (self, code),
            (RegistryAuthority::Proj, OperationCode::Proj(_))
                | (RegistryAuthority::Epsg, OperationCode::Epsg(_))
        )
    }
}

fn issued_by_any(authorities: &[RegistryAuthority], code: OperationCode<'_>) -> bool {
    authorities.iter().any(|authority| authority.issued(code))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Situation {
    SourceIsTarget,
    SourceIsSource,
    TargetIsTarget,
    TargetIsSource,
}

#[derive(Debug, Clone)]
struct DatumPivotStep {
    situation: Situation,
    code: OperationCode<'static>,
    bounding_box: BoundingBox,
}

struct Search<'a> {
    context: &'a OperationContext,
    database: bool,
    source_extent: Option<AreaOfUse>,
    target_extent: Option<AreaOfUse>,
    in_datum_pivot: bool,
    depth: usize,
    crs_by_code: HashMap<u32, Crs>,
    operation_by_code: HashMap<OperationCode<'static>, CandidateOperation>,
}

fn inverses(operations: Vec<CandidateOperation>) -> Result<Vec<CandidateOperation>, Error> {
    // proj's applyInverse
    operations.iter().map(CandidateOperation::inverse).collect()
}

fn base_is_equivalent(derived: &Crs, other: &Crs) -> bool {
    match derived {
        Crs::Projected(projected) => {
            crs_equivalent(&Crs::Geographic((*projected.base_crs).clone()), other)
        }
        Crs::Geographic(_) | Crs::Geodetic(_) | Crs::Bound(_) => false,
    }
}

fn crs_identifiers(crs: &Crs) -> &[crate::iso19111::metadata::Identifier] {
    match crs {
        Crs::Geographic(geographic) => &geographic.identifiers,
        Crs::Geodetic(geodetic) => &geodetic.identifiers,
        Crs::Projected(projected) => &projected.identifiers,
        Crs::Bound(bound) => &bound.identifiers,
    }
}

fn datum_identifiers(datum: &GeodeticDatum) -> &[crate::iso19111::metadata::Identifier] {
    match datum {
        GeodeticDatum::ReferenceFrame(frame) => &frame.identifiers,
        GeodeticDatum::Ensemble(ensemble) => &ensemble.identifiers,
    }
}

fn datum_is_unknown(datum: &GeodeticDatum) -> bool {
    let name = match datum {
        GeodeticDatum::ReferenceFrame(frame) => frame.name.clone(),
        GeodeticDatum::Ensemble(ensemble) => ensemble.as_datum().name,
    };
    datum_identifiers(datum).is_empty()
        && (name.eq_ignore_ascii_case("unknown")
            || name
                .to_ascii_lowercase()
                .starts_with(&UNKNOWN_BASED_ON.to_ascii_lowercase()))
}

fn epsg_datum_code(datum: &GeodeticDatum) -> Option<u32> {
    datum_identifiers(datum)
        .iter()
        .find(|identifier| identifier.code_space.eq_ignore_ascii_case("EPSG"))
        .and_then(|identifier| identifier.code.parse().ok())
}

fn is_dynamic(datum: &GeodeticDatum) -> bool {
    matches!(datum, GeodeticDatum::ReferenceFrame(frame) if frame.dynamic.is_some())
}

fn geodetic_kind(crs: &Crs) -> Option<usize> {
    // proj's getType in createOperationsWithDatumPivot
    match crs {
        Crs::Geographic(geographic) if geographic.coordinate_system.axes.len() == 3 => Some(1),
        Crs::Geographic(_) => Some(0),
        Crs::Geodetic(_) => Some(2),
        Crs::Projected(_) | Crs::Bound(_) => None,
    }
}

fn is_axis_order_reversal(operation: &CandidateOperation) -> bool {
    match &operation.operation {
        CoordinateOperation::Conversion { conversion, .. } => conversion
            .method
            .identifiers
            .iter()
            .any(|identifier| identifier.code == "9843" || identifier.code == "9844"),
        _ => false,
    }
}

fn has_only_proj_authority_steps(operations: &[CandidateOperation]) -> bool {
    // proj's hasResultSetOnlyResultsWithPROJStep
    operations.iter().all(|operation| {
        matches!(operation.operation, CoordinateOperation::Concatenated(_))
            && operation.steps().iter().any(|step| {
                step.identifiers().first().is_some_and(|identifier| {
                    matches!(
                        identifier.code_space.as_str(),
                        "PROJ" | "INVERSE(PROJ)" | "DERIVED_FROM(PROJ)"
                    )
                })
            })
    })
}

fn is_datum_in_ensemble(datum: &GeodeticDatum, ensemble: &GeodeticDatum) -> bool {
    let (GeodeticDatum::ReferenceFrame(frame), GeodeticDatum::Ensemble(ensemble)) =
        (datum, ensemble)
    else {
        return false;
    };
    ensemble
        .members
        .iter()
        .any(|member| frame.is_equivalent_to(member))
}

fn uses_only_ensemble_null_operations(operations: &[CandidateOperation]) -> bool {
    // proj's useOnlyIntermediateCRSThroughDatumEnsembleNullOp
    operations.iter().all(|operation| {
        let CoordinateOperation::Concatenated(concatenated) = &operation.operation else {
            return false;
        };
        let mut found = false;
        for step in &concatenated.steps {
            let identifiers = step.identifiers();
            if identifiers.len() != 1 || !identifiers[0].code_space.contains("PROJ") {
                continue;
            }
            let (Some(source), Some(target)) = (
                geodetic_datum(step.source_crs()),
                geodetic_datum(step.target_crs()),
            ) else {
                continue;
            };
            found = is_datum_in_ensemble(source, target) || is_datum_in_ensemble(target, source);
        }
        found
    })
}

fn irrelevant_pivot(operation: &CandidateOperation, source_code: u32, target_code: u32) -> bool {
    // proj's useIrrelevantPivot
    let CoordinateOperation::Concatenated(concatenated) = &operation.operation else {
        return false;
    };
    let steps = &concatenated.steps;
    steps[..steps.len().saturating_sub(1)].iter().any(|step| {
        let identifiers = crs_identifiers(step.target_crs());
        identifiers.len() == 1
            && identifiers[0].code_space == "EPSG"
            && (identifiers[0].code == source_code.to_string()
                || identifiers[0].code == target_code.to_string())
    })
}

fn leading_year(name: &str, prefix: &str) -> i32 {
    // atoi on the text after the prefix, with 80 to 99 read as 1980 to 1999
    let digits: String = name[prefix.len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let value = digits.parse().unwrap_or(0);
    if (80..=99).contains(&value) {
        value + 1900
    } else {
        value
    }
}

struct PivotCheck {
    etrf_to_etrf: bool,
    nad83_csrs_to_nad83_csrs: bool,
    source_name: String,
    target_name: String,
}

impl PivotCheck {
    fn new(source: &Crs, target: &Crs) -> Self {
        Self {
            etrf_to_etrf: source.name().starts_with("ETRF") && target.name().starts_with("ETRF"),
            nad83_csrs_to_nad83_csrs: source.name().starts_with("NAD83(CSRS)")
                && target.name().starts_with("NAD83(CSRS)"),
            source_name: source.name().to_string(),
            target_name: target.name().to_string(),
        }
    }

    fn accepts(&self, intermediate: &Crs) -> bool {
        // proj's checkPivot in createFromCRSCodesWithIntermediates
        let name = intermediate.name();
        if self.etrf_to_etrf && name.starts_with("ITRF") {
            let source_year = leading_year(&self.source_name, "ETRF");
            let target_year = leading_year(&self.target_name, "ETRF");
            let intermediate_year = leading_year(name, "ITRF");
            if source_year > 0
                && target_year > 0
                && intermediate_year > 0
                && (intermediate_year < source_year.min(target_year)
                    || intermediate_year > source_year.max(target_year))
            {
                return false;
            }
        }
        !(self.nad83_csrs_to_nad83_csrs && (name == "NAD83" || name == "WGS 84"))
    }
}

fn replacement_uses_unknown_grid(replacement_code: u32) -> bool {
    let Some(replacement) = registry::coordinate_operation(OperationCode::Epsg(replacement_code))
    else {
        return false;
    };
    let OperationKind::GridTransformation(parameters) = &replacement.kind else {
        return false;
    };
    parameters.grids.first().is_some_and(|grid| {
        registry::grid_alternative(grid.file_name)
            .is_none_or(|alternative| alternative.proj_grid_name.is_none())
    })
}

fn replacement_codes(superseded_code: u32) -> impl Iterator<Item = u32> {
    // proj's supersession joins require one source and target CRS and one proj.db table, which the registry keeps as the operation kind
    let superseded_kind = registry::coordinate_operation(OperationCode::Epsg(superseded_code))
        .map(|operation| std::mem::discriminant(&operation.kind));
    registry::supersessions(superseded_code)
        .iter()
        .filter(move |supersession| {
            let replacement_kind =
                registry::coordinate_operation(OperationCode::Epsg(supersession.replacement_code))
                    .map(|operation| std::mem::discriminant(&operation.kind));
            supersession.same_source_target_crs
                && superseded_kind.is_some()
                && replacement_kind == superseded_kind
        })
        .map(|supersession| supersession.replacement_code)
}

fn superseded_by(code: OperationCode<'_>, kept: &HashSet<OperationCode<'static>>) -> bool {
    let OperationCode::Epsg(code) = code else {
        return false;
    };
    replacement_codes(code).any(|replacement| kept.contains(&OperationCode::Epsg(replacement)))
}

fn accuracy_key(accuracy: Option<f64>) -> (usize, f64) {
    accuracy.map_or((1, f64::NEG_INFINITY), |accuracy| (0, accuracy))
}

#[derive(Clone, Copy)]
struct PivotPair {
    first: DirectedOperation,
    second: DirectedOperation,
    intermediate_code: u32,
}

impl PivotPair {
    fn sort_key(&self) -> (usize, f64) {
        // sqlite orders a sum with a NULL in it first
        let (first_nulls, first) = accuracy_key(self.first.operation.accuracy);
        let (second_nulls, second) = accuracy_key(self.second.operation.accuracy);
        (first_nulls + second_nulls, first + second)
    }
}

fn geographic_crs_code(code: u32) -> bool {
    matches!(
        registry::crs_definition(code).map(|definition| definition.kind),
        Some(CrsDefinitionKind::Geodetic {
            geodetic_type: GeodeticCrsType::Geographic2D | GeodeticCrsType::Geographic3D,
            ..
        })
    )
}

impl<'a> Search<'a> {
    fn new(source: &Crs, target: &Crs, context: &'a OperationContext, database: bool) -> Self {
        Self {
            context,
            database,
            source_extent: crs_area_of_use(source),
            target_extent: crs_area_of_use(target),
            in_datum_pivot: false,
            depth: 0,
            crs_by_code: HashMap::new(),
            operation_by_code: HashMap::new(),
        }
    }

    fn extents(&self) -> Extents<'_> {
        Extents {
            source: self.source_extent.as_ref(),
            target: self.target_extent.as_ref(),
        }
    }

    fn registry_authorities(&self) -> &'static [RegistryAuthority] {
        // proj.db's authority_to_authority_preference gives PROJ,EPSG,NKG from EPSG to EPSG, and the registry holds no NKG operation
        match self.context.authority {
            OperationAuthority::Any => &[RegistryAuthority::Proj, RegistryAuthority::Epsg],
            OperationAuthority::Epsg => &[RegistryAuthority::Epsg],
        }
    }

    fn registry_crs(&mut self, code: u32) -> Result<Crs, Error> {
        if let Some(crs) = self.crs_by_code.get(&code) {
            return Ok(crs.clone());
        }
        let crs = create_coordinate_reference_system(code)?;
        self.crs_by_code.insert(code, crs.clone());
        Ok(crs)
    }

    fn registry_operation(
        &mut self,
        code: OperationCode<'static>,
    ) -> Result<CandidateOperation, Error> {
        if let Some(operation) = self.operation_by_code.get(&code) {
            return Ok(operation.clone());
        }
        let operation = CandidateOperation::new(create_coordinate_operation(code)?);
        self.operation_by_code.insert(code, operation.clone());
        Ok(operation)
    }

    fn directed_operation(
        &mut self,
        directed: &DirectedOperation,
    ) -> Result<CandidateOperation, Error> {
        let operation = self.registry_operation(directed.operation.code)?;
        if directed.inverse {
            return operation.inverse();
        }
        Ok(operation)
    }

    fn crs_codes(&mut self, crs: &Crs) -> Vec<u32> {
        // proj's buildCRSIds, without the lookup by name for a CRS that has no identifier
        let mut codes = Vec::new();
        for identifier in crs_identifiers(crs) {
            if !identifier.code_space.eq_ignore_ascii_case("EPSG") {
                continue;
            }
            let Ok(code) = identifier.code.parse::<u32>() else {
                continue;
            };
            // proj's buildCRSIds skips an identifier its factory cannot build
            let consistent = self.registry_crs(code).is_ok_and(|registered| {
                registered.is_equivalent_to(crs, Criterion::EquivalentExceptAxisOrderGeogcrs)
            });
            if consistent {
                codes.push(code);
            }
        }
        codes
    }

    fn operations(&mut self, source: &Crs, target: &Crs) -> Result<Vec<CandidateOperation>, Error> {
        if self.depth == MAX_RECURSION_DEPTH {
            return Err(Error::Factory {
                kind: "coordinate operations",
                code: format!("{} to {}", source.name(), target.name()),
                reason: "Too deep recursion in createOperations()".to_string(),
            });
        }
        self.depth += 1;
        let operations = self.operations_at_depth(source, target);
        self.depth -= 1;
        operations
    }

    fn operations_at_depth(
        &mut self,
        source: &Crs,
        target: &Crs,
    ) -> Result<Vec<CandidateOperation>, Error> {
        let mut found = Vec::new();
        if self.database
            && !base_is_equivalent(source, target)
            && !base_is_equivalent(target, source)
            && self.create_operations_from_database(source, target, &mut found)?
        {
            return Ok(found);
        }
        if geodetic_datum(source).is_some() && geodetic_datum(target).is_some() {
            self.geodetic_to_geodetic(source, target, &mut found, false)?;
            return Ok(found);
        }
        if let Crs::Projected(projected) = source {
            self.derived_to(projected, source, target, &mut found)?;
            return Ok(found);
        }
        if matches!(target, Crs::Projected(_)) {
            return inverses(self.operations(target, source)?);
        }
        match (source, target) {
            (Crs::Bound(bound), Crs::Geographic(_)) => {
                self.bound_to_geographic(source, bound, target, &mut found)?;
            }
            (Crs::Bound(_), Crs::Geodetic(_)) => {
                self.create_operations_to_geocentric(source, target, &mut found)?;
            }
            (Crs::Geographic(_) | Crs::Geodetic(_), Crs::Bound(_)) => {
                return inverses(self.operations(target, source)?);
            }
            (Crs::Bound(bound), Crs::Bound(other)) => {
                self.bound_to_bound(bound, other, &mut found)?;
            }
            _ => {}
        }
        Ok(found)
    }

    fn has_perfect_accuracy_result(&self, operations: &[CandidateOperation]) -> bool {
        filter_results(operations, self.context, &self.extents(), true)
            .operations
            .iter()
            .any(|operation| operation.accuracy_or_unknown() == 0.0)
    }

    fn has_too_small_areas(&self, operations: &[CandidateOperation]) -> bool {
        // proj's hasTooSmallAreas, whose limit of 10 keeps the search short between WGS 84 and NAD83(CSRS)v2
        if operations.is_empty() || operations.len() >= 10 {
            return false;
        }
        let area_of_interest = self.context.area_of_interest.as_ref();
        let target_area = match (area_of_interest, &self.source_extent, &self.target_extent) {
            (Some(area), _, _) => pseudo_area(Some(area)),
            (None, Some(source), Some(target)) => pseudo_area(source.intersection(target).as_ref()),
            (None, Some(source), None) => pseudo_area(Some(source)),
            (None, None, target) => pseudo_area(target.as_ref()),
        };
        if target_area <= 0.0 {
            return false;
        }
        !operations.iter().any(|operation| {
            let Some(extent) = operation.area_of_use() else {
                return false;
            };
            let area = match (area_of_interest, &self.source_extent, &self.target_extent) {
                (Some(area_of_interest), _, _) => {
                    pseudo_area(extent.intersection(area_of_interest).as_ref())
                }
                (None, Some(source), Some(target)) => {
                    let x = extent.intersection(source);
                    let y = extent.intersection(target);
                    let both = match (&x, &y) {
                        (Some(x), Some(y)) => pseudo_area(x.intersection(y).as_ref()),
                        _ => 0.0,
                    };
                    pseudo_area(x.as_ref()) + pseudo_area(y.as_ref()) - both
                }
                (None, Some(source), None) => pseudo_area(extent.intersection(source).as_ref()),
                (None, None, Some(target)) => pseudo_area(extent.intersection(target).as_ref()),
                (None, None, None) => pseudo_area(Some(&extent)),
            };
            area > 0.5 * target_area
        })
    }

    fn create_operations_from_database(
        &mut self,
        source: &Crs,
        target: &Crs,
        found: &mut Vec<CandidateOperation>,
    ) -> Result<bool, Error> {
        // proj's createOperationsFromDatabase, without vertical CRSs and point motion between epochs
        let direct = self.find_ops_in_registry_direct(source, target)?;
        *found = direct.operations;
        let mut direct_non_empty_before_filtering = direct.non_empty_before_filtering;
        if self.has_perfect_accuracy_result(found) {
            return Ok(true);
        }
        let mut filter_and_check_perfect = false;
        let geodetic = match (geodetic_datum(source), geodetic_datum(target)) {
            (Some(source_datum), Some(target_datum)) => Some((source_datum, target_datum)),
            _ => None,
        };
        let same_geodetic_datum = geodetic.is_some_and(|(source_datum, target_datum)| {
            is_same_geodetic_datum(source_datum, target_datum)
        });
        if geodetic.is_some() && found.is_empty() && !same_geodetic_datum && !self.in_datum_pivot {
            self.with_datum_pivot(found, source, target)?;
            filter_and_check_perfect = !found.is_empty();
        }

        let mut found_instantiable = false;
        if found.len() == 1 {
            found_instantiable = proj_exportable(&found[0]);
            if !found_instantiable {
                direct_non_empty_before_filtering = false;
            }
        } else if found.len() > 1 {
            found_instantiable = true;
        }

        let intermediate_allowed =
            self.context.intermediate_crs_use == IntermediateCrsUse::IfNoDirectTransformation;
        if !same_geodetic_datum
            && (found.is_empty() || !found_instantiable)
            && !direct_non_empty_before_filtering
            && intermediate_allowed
        {
            let with_intermediate = self.find_ops_with_intermediate(source, target, false)?;
            found.extend(with_intermediate);
            filter_and_check_perfect = !found.is_empty();
        }

        // proj also takes this branch when every operation found is "Replaced by" another in its remarks, which the registry does not carry
        if !self.in_datum_pivot
            && !same_geodetic_datum
            && intermediate_allowed
            && !direct_non_empty_before_filtering
        {
            if let Some((source_datum, target_datum)) = geodetic {
                let too_small_areas = self.has_too_small_areas(found);
                let all_need_epoch = found
                    .iter()
                    .all(CandidateOperation::requires_coordinate_epoch);
                if found.is_empty()
                    || too_small_areas
                    || (all_need_epoch && !is_dynamic(source_datum) && !is_dynamic(target_datum))
                    || uses_only_ensemble_null_operations(found)
                {
                    let names: HashSet<String> = if too_small_areas {
                        found
                            .iter()
                            .map(|operation| operation.name().to_string())
                            .collect()
                    } else {
                        HashSet::new()
                    };
                    let with_intermediate =
                        self.find_ops_with_intermediate(source, target, true)?;
                    if too_small_areas && !found.is_empty() {
                        found.extend(
                            with_intermediate
                                .into_iter()
                                .filter(|operation| !names.contains(operation.name())),
                        );
                    } else {
                        found.extend(with_intermediate);
                    }
                    filter_and_check_perfect = !found.is_empty();
                }
            }
        }

        Ok(filter_and_check_perfect && self.has_perfect_accuracy_result(found))
    }

    fn find_ops_in_registry_direct(
        &mut self,
        source: &Crs,
        target: &Crs,
    ) -> Result<DirectOperations, Error> {
        // proj's findOpsInRegistryDirect
        let unknown = |crs: &Crs| {
            crs_identifiers(crs).is_empty() && crs.name().eq_ignore_ascii_case("unknown")
        };
        if unknown(source) || unknown(target) {
            return Ok(DirectOperations::default());
        }
        let source_codes = self.crs_codes(source);
        let target_codes = self.crs_codes(target);
        for source_code in &source_codes {
            for target_code in &target_codes {
                let mut found = Vec::new();
                for authority in self.registry_authorities() {
                    found.extend(self.registry_operations_between(
                        *source_code,
                        *target_code,
                        *authority,
                    )?);
                    if *authority == RegistryAuthority::Proj {
                        continue;
                    }
                    if !found.is_empty() {
                        let filtered = filter_results(&found, self.context, &self.extents(), false);
                        return Ok(DirectOperations {
                            operations: filtered.operations,
                            non_empty_before_filtering: true,
                        });
                    }
                }
            }
        }
        Ok(DirectOperations::default())
    }

    fn projected_conversion(&mut self, projected_code: u32) -> Result<CandidateOperation, Error> {
        let projected = create_coordinate_reference_system(projected_code)?;
        let Crs::Projected(projected_crs) = &projected else {
            return Err(Error::Factory {
                kind: "CRS",
                code: format!("EPSG:{projected_code}"),
                reason: "it is not a projected CRS".to_string(),
            });
        };
        let base = Crs::Geographic((*projected_crs.base_crs).clone());
        Ok(CandidateOperation::new(CoordinateOperation::Conversion {
            conversion: projected_crs.conversion.clone(),
            source_crs: Box::new(base),
            target_crs: Box::new(projected),
        }))
    }

    fn registry_operations_between(
        &mut self,
        source_code: u32,
        target_code: u32,
        authority: RegistryAuthority,
    ) -> Result<Vec<CandidateOperation>, Error> {
        // proj's createFromCoordinateReferenceSystemCodes with tryReverseOrder, restricted to one authority
        if let Some(CrsDefinitionKind::Projected { base_crs_code, .. }) =
            registry::crs_definition(target_code).map(|definition| definition.kind)
        {
            if base_crs_code == source_code && authority == RegistryAuthority::Epsg {
                return Ok(vec![self.projected_conversion(target_code)?]);
            }
        }

        struct Row {
            directed: DirectedOperation,
            area: AreaOfUse,
        }
        let mut rows = Vec::new();
        for directed in registry::operations_between(source_code, target_code) {
            if !authority.issued(directed.operation.code) {
                continue;
            }
            for extent_code in &directed.operation.extent_codes {
                rows.push(Row {
                    directed,
                    area: registry_area(*extent_code)?,
                });
            }
        }
        rows.sort_by(|a, b| {
            b.area
                .bounding_box
                .pseudo_area()
                .total_cmp(&a.area.bounding_box.pseudo_area())
                .then_with(|| {
                    let (a_nulls, a_accuracy) = accuracy_key(a.directed.operation.accuracy);
                    let (b_nulls, b_accuracy) = accuracy_key(b.directed.operation.accuracy);
                    a_nulls
                        .cmp(&b_nulls)
                        .then(a_accuracy.total_cmp(&b_accuracy))
                })
        });
        let codes: HashSet<OperationCode<'static>> =
            rows.iter().map(|row| row.directed.operation.code).collect();
        rows.retain(|row| {
            let OperationCode::Epsg(code) = row.directed.operation.code else {
                return true;
            };
            !replacement_codes(code).any(|replacement| {
                codes.contains(&OperationCode::Epsg(replacement))
                    && !replacement_uses_unknown_grid(replacement)
            })
        });
        let intersecting: Vec<bool> = rows
            .iter()
            .map(|row| {
                [&self.source_extent, &self.target_extent]
                    .into_iter()
                    .flatten()
                    .all(|extent| row.area.intersects(extent))
            })
            .collect();
        let any_intersecting = intersecting.iter().any(|&intersects| intersects);

        let mut seen = HashSet::new();
        let mut operations = Vec::new();
        for (row, intersects) in rows.iter().zip(intersecting) {
            if any_intersecting && !intersects {
                continue;
            }
            if !seen.insert((row.directed.operation.code, row.directed.inverse)) {
                continue;
            }
            let operation = self.directed_operation(&row.directed)?;
            let rejected = self.context.discard_if_missing_grid()
                && rejected_for_missing_grid(
                    &operation,
                    self.context.consider_known_grids_as_available(),
                );
            if !rejected {
                operations.push(operation);
            }
        }
        Ok(operations)
    }

    fn candidates_for_datum(&mut self, datum: &GeodeticDatum) -> Result<Vec<Crs>, Error> {
        // proj's createGeodeticCRSFromDatum, ordered by code, without the lookup by name for a datum with no identifier
        let Some(datum_code) = epsg_datum_code(datum) else {
            return Ok(Vec::new());
        };
        registry::geodetic_crs_with_datum(datum_code)
            .into_iter()
            .map(|code| self.registry_crs(code))
            .collect()
    }

    fn find_ops_with_intermediate(
        &mut self,
        source: &Crs,
        target: &Crs,
        datum_based: bool,
    ) -> Result<Vec<CandidateOperation>, Error> {
        // proj's findsOpsInRegistryWithIntermediate
        let source_datum = geodetic_datum(source);
        if source_datum.is_some_and(datum_is_unknown) {
            return Ok(Vec::new());
        }
        if source_datum.is_none() && matches!(source, Crs::Bound(_)) {
            return Ok(Vec::new());
        }
        let target_datum = geodetic_datum(target);
        if target_datum.is_some_and(datum_is_unknown) {
            return Ok(Vec::new());
        }
        if target_datum.is_none() && matches!(target, Crs::Bound(_)) {
            return Ok(Vec::new());
        }

        let source_codes = self.crs_codes(source);
        if source_codes.is_empty() {
            let Some(source_datum) = source_datum else {
                return Ok(Vec::new());
            };
            return self.intermediate_through_datum_candidates(
                source,
                source_datum,
                target,
                datum_based,
            );
        }
        let target_codes = self.crs_codes(target);
        if target_codes.is_empty() {
            return inverses(self.find_ops_with_intermediate(target, source, datum_based)?);
        }

        let allowed = self.registry_authorities();
        for source_code in &source_codes {
            for target_code in &target_codes {
                let found = if datum_based {
                    self.between_geodetic_with_datum_based_intermediates(
                        source,
                        *source_code,
                        target,
                        *target_code,
                        allowed,
                    )?
                } else {
                    let geographic_intermediate = matches!(source, Crs::Geographic(_))
                        && matches!(target, Crs::Geographic(_));
                    self.with_intermediates(
                        *source_code,
                        *target_code,
                        geographic_intermediate,
                        allowed,
                    )?
                };
                if !found.is_empty() {
                    return Ok(
                        filter_results(&found, self.context, &self.extents(), false).operations
                    );
                }
            }
        }
        Ok(Vec::new())
    }

    fn intermediate_through_datum_candidates(
        &mut self,
        source: &Crs,
        source_datum: &GeodeticDatum,
        target: &Crs,
        datum_based: bool,
    ) -> Result<Vec<CandidateOperation>, Error> {
        let originating_authority = crs_identifiers(source)
            .first()
            .map(|identifier| identifier.code_space.clone());
        let mut found = Vec::new();
        for candidate in self.candidates_for_datum(source_datum)? {
            let candidate_identifiers = crs_identifiers(&candidate);
            let same_authority = originating_authority.is_none()
                || candidate_identifiers.len() > 1
                || (candidate_identifiers.len() == 1
                    && Some(&candidate_identifiers[0].code_space)
                        == originating_authority.as_ref());
            let same_shape = axis_count(&candidate) == axis_count(source)
                && matches!(candidate, Crs::Geographic(_)) == matches!(source, Crs::Geographic(_));
            if !same_authority || !same_shape {
                continue;
            }
            if let (Some(candidate_datum), Some(target_datum)) =
                (geodetic_datum(&candidate), geodetic_datum(target))
            {
                if is_same_geodetic_datum(candidate_datum, target_datum) {
                    continue;
                }
            }
            let with_intermediate =
                self.find_ops_with_intermediate(&candidate, target, datum_based)?;
            if with_intermediate.is_empty() {
                continue;
            }
            for first in self.operations(source, &candidate)? {
                for second in &with_intermediate {
                    let concatenated = concatenate(vec![first.clone(), second.clone()], true)?;
                    found.extend(concatenated);
                }
            }
            if !found.is_empty() {
                return Ok(found);
            }
        }
        Ok(found)
    }

    fn pivot_pairs(&self, source_code: u32, target_code: u32) -> [Vec<PivotPair>; 4] {
        let from_source = registry::operations_from(source_code);
        let forward = |directed: &&DirectedOperation| !directed.inverse;
        let pairs = |first_inverse: bool, second_from_target: bool| -> Vec<PivotPair> {
            let mut pairs = Vec::new();
            for first in from_source
                .iter()
                .filter(|directed| directed.inverse == first_inverse)
            {
                let intermediate_code = if first_inverse {
                    first.operation.source_crs_code
                } else {
                    first.operation.target_crs_code
                };
                let seconds = if second_from_target {
                    registry::operations_between(target_code, intermediate_code)
                } else {
                    registry::operations_between(intermediate_code, target_code)
                };
                for second in seconds.iter().filter(forward) {
                    pairs.push(PivotPair {
                        first: *first,
                        second: DirectedOperation {
                            operation: second.operation,
                            inverse: second_from_target,
                        },
                        intermediate_code,
                    });
                }
            }
            pairs
        };
        [
            pairs(false, false),
            pairs(false, true),
            pairs(true, false),
            pairs(true, true),
        ]
    }

    fn usages_intersect(&self, pair: &PivotPair) -> Result<bool, Error> {
        let extra: Vec<&AreaOfUse> = [&self.source_extent, &self.target_extent]
            .into_iter()
            .flatten()
            .filter(|extent| !extent.bounding_box.is_world())
            .collect();
        for first_code in &pair.first.operation.extent_codes {
            let first = registry_area(*first_code)?;
            for second_code in &pair.second.operation.extent_codes {
                let second = registry_area(*second_code)?;
                let intersects = first.intersects(&second)
                    && extra
                        .iter()
                        .all(|extent| first.intersects(extent) && second.intersects(extent));
                if intersects {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn with_intermediates(
        &mut self,
        source_code: u32,
        target_code: u32,
        geographic_intermediate: bool,
        allowed: &[RegistryAuthority],
    ) -> Result<Vec<CandidateOperation>, Error> {
        // proj's createFromCRSCodesWithIntermediates, without its datum publication date criterion
        if source_code == target_code
            || registry::operations_from(source_code).is_empty()
            || registry::operations_from(target_code).is_empty()
        {
            return Ok(Vec::new());
        }
        let source = self.registry_crs(source_code)?;
        let target = self.registry_crs(target_code)?;
        let pivot_check = PivotCheck::new(&source, &target);
        let mut found = Vec::new();
        for mut pairs in self.pivot_pairs(source_code, target_code) {
            let mut kept = Vec::new();
            let mut seen = HashSet::new();
            for pair in pairs.drain(..) {
                let codes = (pair.first.operation.code, pair.second.operation.code);
                let acceptable = (!geographic_intermediate
                    || geographic_crs_code(pair.intermediate_code))
                    && issued_by_any(allowed, codes.0)
                    && issued_by_any(allowed, codes.1)
                    && seen.insert(codes)
                    && self.usages_intersect(&pair)?;
                if acceptable {
                    kept.push(pair);
                }
            }
            kept.sort_by(|a, b| {
                let (a_nulls, a_sum) = a.sort_key();
                let (b_nulls, b_sum) = b.sort_key();
                a_nulls.cmp(&b_nulls).then(a_sum.total_cmp(&b_sum))
            });
            let first_codes: HashSet<OperationCode<'static>> =
                kept.iter().map(|pair| pair.first.operation.code).collect();
            let second_codes: HashSet<OperationCode<'static>> =
                kept.iter().map(|pair| pair.second.operation.code).collect();
            kept.retain(|pair| {
                !superseded_by(pair.first.operation.code, &first_codes)
                    && !superseded_by(pair.second.operation.code, &second_codes)
            });
            for pair in kept {
                let first = self.registry_operation(pair.first.operation.code)?;
                if irrelevant_pivot(&first, source_code, target_code) {
                    continue;
                }
                let intermediate = if pair.first.inverse {
                    first.source_crs()
                } else {
                    first.target_crs()
                };
                if !pivot_check.accepts(intermediate) {
                    continue;
                }
                let second = self.registry_operation(pair.second.operation.code)?;
                if irrelevant_pivot(&second, source_code, target_code) {
                    continue;
                }
                let first = if pair.first.inverse {
                    first.inverse()?
                } else {
                    first
                };
                let second = if pair.second.inverse {
                    second.inverse()?
                } else {
                    second
                };
                found.extend(concatenate(vec![first, second], false)?);
            }
        }
        Ok(self.without_missing_grids(found))
    }

    fn without_missing_grids(
        &self,
        operations: Vec<CandidateOperation>,
    ) -> Vec<CandidateOperation> {
        if !self.context.discard_if_missing_grid() {
            return operations;
        }
        let consider_known = self.context.consider_known_grids_as_available();
        operations
            .into_iter()
            .filter(|operation| !rejected_for_missing_grid(operation, consider_known))
            .collect()
    }

    fn datum_pivot_steps(
        &self,
        crs_codes: &[u32],
        source_side: bool,
        nad83_csrs_to_nad83_csrs: bool,
        allowed: &[RegistryAuthority],
    ) -> Result<BTreeMap<String, Vec<DatumPivotStep>>, Error> {
        let mut steps: BTreeMap<String, Vec<DatumPivotStep>> = BTreeMap::new();
        // proj's UNION ALL takes the operations ending in the list before those starting from it
        for select_on_target in [true, false] {
            let situation = match (source_side, select_on_target) {
                (true, true) => Situation::SourceIsTarget,
                (true, false) => Situation::SourceIsSource,
                (false, true) => Situation::TargetIsTarget,
                (false, false) => Situation::TargetIsSource,
            };
            for crs_code in crs_codes {
                for directed in registry::operations_from(*crs_code) {
                    if directed.inverse != select_on_target {
                        continue;
                    }
                    let operation = directed.operation;
                    if !issued_by_any(allowed, operation.code) {
                        continue;
                    }
                    let other_code = if select_on_target {
                        operation.source_crs_code
                    } else {
                        operation.target_crs_code
                    };
                    let Some(CrsDefinitionKind::Geodetic { datum_code, .. }) =
                        registry::crs_definition(other_code).map(|definition| definition.kind)
                    else {
                        continue;
                    };
                    let other_name = registry::metadata(other_code).map(|metadata| metadata.name);
                    if nad83_csrs_to_nad83_csrs
                        && matches!(other_name, Some("NAD83") | Some("WGS 84"))
                    {
                        continue;
                    }
                    for extent_code in &operation.extent_codes {
                        steps.entry(format!("EPSG:{datum_code}")).or_default().push(
                            DatumPivotStep {
                                situation,
                                code: operation.code,
                                bounding_box: registry_area(*extent_code)?.bounding_box,
                            },
                        );
                    }
                }
            }
        }
        Ok(steps)
    }

    fn crs_codes_sharing_datum(&self, crs: &Crs, crs_code: u32) -> Vec<u32> {
        let datum_code =
            geodetic_datum(crs).and_then(epsg_datum_code).or_else(
                || match registry::crs_definition(crs_code).map(|definition| definition.kind) {
                    Some(CrsDefinitionKind::Geodetic { datum_code, .. }) => Some(datum_code),
                    _ => None,
                },
            );
        let codes = datum_code
            .map(registry::geodetic_crs_with_datum)
            .unwrap_or_default();
        if codes.is_empty() {
            return vec![crs_code];
        }
        codes
    }

    fn pivot_step_operation(
        &mut self,
        step: &DatumPivotStep,
        source_code: u32,
        target_code: u32,
        cache: &mut HashMap<OperationCode<'static>, Option<CandidateOperation>>,
    ) -> Result<Option<CandidateOperation>, Error> {
        if let Some(operation) = cache.get(&step.code) {
            return Ok(operation.clone());
        }
        let operation = self.registry_operation(step.code)?;
        let usable = (!irrelevant_pivot(&operation, source_code, target_code)).then_some(operation);
        cache.insert(step.code, usable.clone());
        Ok(usable)
    }

    fn between_geodetic_with_datum_based_intermediates(
        &mut self,
        source: &Crs,
        source_code: u32,
        target: &Crs,
        target_code: u32,
        allowed: &[RegistryAuthority],
    ) -> Result<Vec<CandidateOperation>, Error> {
        // proj's createBetweenGeodeticCRSWithDatumBasedIntermediates, without its 1900 cut-off on datum publication dates
        if source_code == target_code
            || geodetic_datum(source).is_none()
            || geodetic_datum(target).is_none()
        {
            return Ok(Vec::new());
        }
        let nad83_csrs_to_nad83_csrs =
            source.name().starts_with("NAD83(CSRS)") && target.name().starts_with("NAD83(CSRS)");
        let source_list = self.crs_codes_sharing_datum(source, source_code);
        let target_list = self.crs_codes_sharing_datum(target, target_code);
        let source_steps =
            self.datum_pivot_steps(&source_list, true, nad83_csrs_to_nad83_csrs, allowed)?;
        let target_steps =
            self.datum_pivot_steps(&target_list, false, nad83_csrs_to_nad83_csrs, allowed)?;
        let extra: Vec<BoundingBox> = [&self.source_extent, &self.target_extent]
            .into_iter()
            .flatten()
            .map(|extent| extent.bounding_box)
            .filter(|bounding_box| !bounding_box.is_world())
            .collect();

        let mut cache = HashMap::new();
        let mut candidates: Vec<(DatumPivotStep, DatumPivotStep)> = Vec::new();
        let mut seen = HashSet::new();
        let mut used_codes: HashSet<OperationCode<'static>> = HashSet::new();
        for (datum_key, source_side) in &source_steps {
            let Some(target_side) = target_steps.get(datum_key) else {
                continue;
            };
            for source_step in source_side {
                if !extra
                    .iter()
                    .all(|bounding_box| bounding_box.intersects(&source_step.bounding_box))
                {
                    continue;
                }
                for target_step in target_side {
                    let intersects = source_step
                        .bounding_box
                        .intersects(&target_step.bounding_box)
                        && extra
                            .iter()
                            .all(|bounding_box| bounding_box.intersects(&target_step.bounding_box));
                    if !intersects {
                        continue;
                    }
                    if self
                        .pivot_step_operation(source_step, source_code, target_code, &mut cache)?
                        .is_none()
                    {
                        continue;
                    }
                    if self
                        .pivot_step_operation(target_step, source_code, target_code, &mut cache)?
                        .is_none()
                    {
                        continue;
                    }
                    let key = (
                        source_step.code,
                        source_step.situation,
                        target_step.code,
                        target_step.situation,
                    );
                    if seen.insert(key) {
                        candidates.push((source_step.clone(), target_step.clone()));
                        used_codes.insert(source_step.code);
                        used_codes.insert(target_step.code);
                    }
                }
            }
        }

        let superseded: HashSet<OperationCode<'static>> = used_codes
            .iter()
            .filter(|code| superseded_by(**code, &used_codes))
            .copied()
            .collect();

        let mut found = Vec::new();
        for (source_step, target_step) in candidates {
            if superseded.contains(&source_step.code) || superseded.contains(&target_step.code) {
                continue;
            }
            let first = cache[&source_step.code]
                .clone()
                .expect("candidates keep usable operations only");
            let second = cache[&target_step.code]
                .clone()
                .expect("candidates keep usable operations only");
            let first = if source_step.situation == Situation::SourceIsTarget {
                first.inverse()?
            } else {
                first
            };
            let second = if target_step.situation == Situation::TargetIsSource {
                second.inverse()?
            } else {
                second
            };
            let mut steps = Vec::new();
            if !crs_equivalent(source, first.source_crs()) {
                steps.push(create_operation_without_database(
                    source,
                    first.source_crs(),
                )?);
            }
            let middle = (!crs_equivalent(first.target_crs(), second.source_crs()))
                .then(|| create_operation_without_database(first.target_crs(), second.source_crs()))
                .transpose()?;
            let last = (!crs_equivalent(second.target_crs(), target))
                .then(|| create_operation_without_database(second.target_crs(), target))
                .transpose()?;
            steps.push(first);
            steps.extend(middle);
            steps.push(second);
            steps.extend(last);
            found.extend(concatenate(steps, false)?);
        }
        Ok(self.without_missing_grids(found))
    }

    fn with_datum_pivot(
        &mut self,
        found: &mut Vec<CandidateOperation>,
        source: &Crs,
        target: &Crs,
    ) -> Result<(), Error> {
        // proj's createOperationsWithDatumPivot, without promotion to 3D and epoch changes
        self.in_datum_pivot = true;
        let result = self.with_datum_pivot_guarded(found, source, target);
        self.in_datum_pivot = false;
        result
    }

    fn with_datum_pivot_guarded(
        &mut self,
        found: &mut Vec<CandidateOperation>,
        source: &Crs,
        target: &Crs,
    ) -> Result<(), Error> {
        let (Some(source_datum), Some(target_datum)) =
            (geodetic_datum(source), geodetic_datum(target))
        else {
            return Ok(());
        };
        if datum_is_unknown(source_datum) || datum_is_unknown(target_datum) {
            return Ok(());
        }
        let source_candidates = self.candidates_for_datum(source_datum)?;
        let target_candidates = self.candidates_for_datum(target_datum)?;

        for iteration in 0..3 {
            let only_direct = iteration == 0;
            for source_candidate in &source_candidates {
                if source_candidate.name() != source.name() {
                    continue;
                }
                let source_kind = if iteration >= 1 {
                    geodetic_kind(source_candidate)
                } else {
                    None
                };
                for target_candidate in &target_candidates {
                    if target_candidate.name() != target.name() {
                        continue;
                    }
                    if iteration == 1 && source_kind != geodetic_kind(target_candidate) {
                        continue;
                    }
                    if iteration == 2 && source_kind == geodetic_kind(target_candidate) {
                        continue;
                    }
                    let firsts = self.operations(source, source_candidate)?;
                    let Some(first) = firsts.first() else {
                        continue;
                    };
                    let transformations = self.pivot_transformations(
                        source,
                        target,
                        source_candidate,
                        target_candidate,
                        first,
                        only_direct,
                    )?;
                    found.extend(transformations);
                    if !found.is_empty() {
                        if has_only_proj_authority_steps(found) {
                            continue;
                        }
                        return Ok(());
                    }
                }
            }

            for source_candidate in &source_candidates {
                let same_source_name = source_candidate.name() == source.name();
                let firsts = self.operations(source, source_candidate)?;
                let Some(first) = firsts.first() else {
                    continue;
                };
                for target_candidate in &target_candidates {
                    if same_source_name && target_candidate.name() == target.name() {
                        continue;
                    }
                    let transformations = self.pivot_transformations(
                        source,
                        target,
                        source_candidate,
                        target_candidate,
                        first,
                        only_direct,
                    )?;
                    found.extend(transformations);
                    if !found.is_empty() && !has_only_proj_authority_steps(found) {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    fn pivot_transformations(
        &mut self,
        source: &Crs,
        target: &Crs,
        source_candidate: &Crs,
        target_candidate: &Crs,
        first: &CandidateOperation,
        only_direct: bool,
    ) -> Result<Vec<CandidateOperation>, Error> {
        // proj's createTransformations lambda in createOperationsWithDatumPivot
        let mut found = Vec::new();
        let seconds = if only_direct {
            self.find_ops_in_registry_direct(source_candidate, target_candidate)?
                .operations
        } else {
            self.operations(source_candidate, target_candidate)?
        };
        let thirds = self.operations(target_candidate, target)?;
        let Some(third) = thirds.first() else {
            return Ok(found);
        };
        let first_is_null = is_null_transformation(first.name());
        let third_is_null = is_null_transformation(third.name());
        for second in seconds {
            if !second.has_identifiers() || is_axis_order_reversal(&second) {
                continue;
            }
            let derived = (first_is_null || third_is_null)
                && second.operation.identifiers().len() == 1
                && !second.operation.identifiers()[0]
                    .code_space
                    .contains("DERIVED_FROM");
            let second = if derived {
                second.with_derived_identifiers()
            } else {
                second
            };
            let mut steps = Vec::new();
            if !first_is_null {
                steps.push(first.clone());
            }
            steps.push(second);
            if !third_is_null {
                steps.push(third.clone());
            }
            let head = steps.remove(0);
            let head_target = head.target_crs().clone();
            steps.insert(0, head.with_crs(source, &head_target));
            let tail = steps.pop().expect("a second operation is always present");
            let tail_source = tail.source_crs().clone();
            steps.push(tail.with_crs(&tail_source, target));
            found.extend(concatenate(steps, true)?);
        }
        Ok(found)
    }

    fn geodetic_to_geodetic(
        &mut self,
        source: &Crs,
        target: &Crs,
        found: &mut Vec<CandidateOperation>,
        force_ballpark: bool,
    ) -> Result<(), Error> {
        // proj's createOperationsGeodToGeod, without its celestial body check
        if let (Crs::Geographic(source_geographic), Crs::Geographic(target_geographic)) =
            (source, target)
        {
            found.push(geographic_to_geographic(
                source,
                source_geographic,
                target,
                target_geographic,
                force_ballpark,
            )?);
            return Ok(());
        }
        let (Some(source_datum), Some(target_datum)) =
            (geodetic_datum(source), geodetic_datum(target))
        else {
            return Ok(());
        };
        let same_datum = source_datum.is_equivalent_to(target_datum);
        match (source, target) {
            (Crs::Geodetic(_), Crs::Geographic(_)) | (Crs::Geographic(_), Crs::Geodetic(_)) => {
                if same_datum {
                    if force_ballpark {
                        return Err(Error::Factory {
                            kind: "coordinate operation",
                            code: format!("{} to {}", source.name(), target.name()),
                            reason: "proj builds a PROJ-based operation here, which is not ported"
                                .to_string(),
                        });
                    }
                    found.push(geographic_geocentric_conversion(source, target));
                } else if let Crs::Geodetic(geodetic) = source {
                    let intermediate = intermediate_geographic_crs(
                        format!("{} (geographic)", geodetic.name),
                        geodetic.domains.clone(),
                        geodetic.datum.clone(),
                        EllipsoidalCs::longitude_latitude_ellipsoidal_height(
                            &UnitOfMeasure::degree(),
                            &UnitOfMeasure::metre(),
                        ),
                    );
                    let first = geographic_geocentric_conversion(source, &intermediate);
                    for second in self.operations(&intermediate, target)? {
                        found.extend(concatenate(vec![first.clone(), second], true)?);
                    }
                } else {
                    let mut reversed = Vec::new();
                    self.geodetic_to_geodetic(target, source, &mut reversed, force_ballpark)?;
                    found.extend(inverses(reversed)?);
                }
            }
            (Crs::Geodetic(_), Crs::Geodetic(_)) => {
                if !force_ballpark && (crs_equivalent(source, target) || same_datum) {
                    found.push(null_geocentric_translation(source, target));
                } else {
                    found.push(ballpark_geocentric_translation(source, target));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn derived_to(
        &mut self,
        projected: &ProjectedCrs,
        source: &Crs,
        target: &Crs,
        found: &mut Vec<CandidateOperation>,
    ) -> Result<(), Error> {
        // proj's createOperationsDerivedTo, without its vertical CRS shortcut
        let base = Crs::Geographic((*projected.base_crs).clone());
        let first = CandidateOperation::new(CoordinateOperation::Conversion {
            conversion: projected.conversion.clone(),
            source_crs: Box::new(base.clone()),
            target_crs: Box::new(source.clone()),
        })
        .inverse()?;
        if crs_equivalent(&base, target) {
            found.push(first);
            return Ok(());
        }
        for second in self.operations(&base, target)? {
            found.extend(concatenate(vec![first.clone(), second], true)?);
        }
        Ok(())
    }

    fn bound_to_geographic(
        &mut self,
        source: &Crs,
        bound: &BoundCrs,
        target: &Crs,
        found: &mut Vec<CandidateOperation>,
    ) -> Result<(), Error> {
        // proj's createOperationsBoundToGeog, without its vertical CRS and Clarke 1866 grid cases
        let hub = &*bound.target_crs;
        let hub_is_geographic = matches!(hub, Crs::Geographic(_));
        let base = &*bound.source_crs;
        let base_geographic = base.extract_geographic_crs().cloned().map(Crs::Geographic);
        let target_datum =
            geodetic_datum(target).expect("bound_to_geographic takes a geographic target");
        let transformation = CandidateOperation::new(CoordinateOperation::Transformation(
            bound.transformation.clone(),
        ));

        if let Some(base_geographic) = &base_geographic {
            let base_datum = geodetic_datum(base_geographic).expect("a geographic CRS has a datum");
            if base_datum.is_equivalent_to(target_datum) {
                *found = self.operations(base, target)?;
                return Ok(());
            }
        }

        let mut tried_hub_as_target = false;
        if let Some(base_geographic) = base_geographic.as_ref().filter(|_| hub_is_geographic) {
            if crs_equivalent(hub, target) {
                tried_hub_as_target = true;
                let intermediate = if crs_equivalent(base_geographic, transformation.source_crs()) {
                    None
                } else {
                    self.operations(base_geographic, transformation.source_crs())?
                        .into_iter()
                        .next()
                };
                if matches!(base, Crs::Geographic(_)) {
                    match intermediate {
                        Some(intermediate) => {
                            found.extend(concatenate(vec![intermediate, transformation], true)?)
                        }
                        None => found.push(transformation),
                    }
                    return Ok(());
                }
                for first in self.operations(base, base_geographic)? {
                    let mut steps = vec![first];
                    steps.extend(intermediate.clone());
                    steps.push(transformation.clone());
                    found.extend(concatenate(steps, true)?);
                }
                if !found.is_empty() {
                    return Ok(());
                }
            } else if geodetic_datum(hub)
                .is_some_and(|hub_datum| hub_datum.is_equivalent_to(target_datum))
            {
                let firsts = self.operations(base, base_geographic)?;
                let lasts = self.operations(hub, target)?;
                if !firsts.is_empty() && !lasts.is_empty() {
                    let intermediate =
                        if crs_equivalent(base_geographic, transformation.source_crs()) {
                            None
                        } else {
                            self.operations(base_geographic, transformation.source_crs())?
                                .into_iter()
                                .next()
                        };
                    for first in &firsts {
                        for last in &lasts {
                            let mut steps = vec![first.clone()];
                            steps.extend(intermediate.clone());
                            steps.push(transformation.clone());
                            steps.push(last.clone());
                            found.extend(concatenate(steps, true)?);
                        }
                    }
                    if !found.is_empty() {
                        return Ok(());
                    }
                }
            }
        }

        if !tried_hub_as_target && hub_is_geographic && base_geographic.is_some() {
            let firsts = self.operations(source, hub)?;
            let lasts = self.operations(hub, target)?;
            for first in &firsts {
                for last in &lasts {
                    if lasts.len() > 1 || !last.has_ballpark_transformation {
                        found.extend(concatenate(vec![first.clone(), last.clone()], true)?);
                    }
                }
            }
            if !found.is_empty() {
                return Ok(());
            }
        }

        *found = self.operations(base, target)?;
        Ok(())
    }

    fn create_operations_to_geocentric(
        &mut self,
        source: &Crs,
        target: &Crs,
        found: &mut Vec<CandidateOperation>,
    ) -> Result<(), Error> {
        // proj's createOperationsToGeod
        let Crs::Geodetic(geodetic) = target else {
            return Ok(());
        };
        let intermediate = intermediate_geographic_crs(
            geodetic.name.clone(),
            vec![AreaOfUse::world().to_domain()],
            geodetic.datum.clone(),
            EllipsoidalCs::latitude_longitude_ellipsoidal_height(
                &UnitOfMeasure::degree(),
                &UnitOfMeasure::metre(),
            ),
        );
        let to_intermediate = self.operations(source, &intermediate)?;
        let from_intermediate = self.operations(&intermediate, target)?;
        let Some(last) = from_intermediate.first() else {
            return Ok(());
        };
        for first in to_intermediate {
            let first = first.with_crs(source, &intermediate);
            found.extend(concatenate(vec![first, last.clone()], true)?);
        }
        Ok(())
    }

    fn bound_to_bound(
        &mut self,
        bound: &BoundCrs,
        other: &BoundCrs,
        found: &mut Vec<CandidateOperation>,
    ) -> Result<(), Error> {
        // proj's createOperationsBoundToBound, without its vertical CRS cases
        let (hub, other_hub) = (&*bound.target_crs, &*other.target_crs);
        let source = Crs::Bound(bound.clone());
        let target = Crs::Bound(other.clone());
        if matches!((hub, other_hub), (Crs::Geographic(_), Crs::Geographic(_)))
            && crs_equivalent(hub, other_hub)
        {
            let firsts = self.operations(&source, hub)?;
            let lasts = self.operations(hub, &target)?;
            for first in &firsts {
                for last in &lasts {
                    found.extend(concatenate(vec![first.clone(), last.clone()], true)?);
                }
            }
            if !found.is_empty() {
                return Ok(());
            }
        }
        *found = self.operations(&bound.source_crs, &other.source_crs)?;
        Ok(())
    }
}

#[derive(Default)]
struct DirectOperations {
    operations: Vec<CandidateOperation>,
    non_empty_before_filtering: bool,
}

fn geographic_to_geographic(
    source: &Crs,
    source_geographic: &GeographicCrs,
    target: &Crs,
    target_geographic: &GeographicCrs,
    force_ballpark: bool,
) -> Result<CandidateOperation, Error> {
    // proj's createOperationsGeogToGeog
    let source_prime_meridian = prime_meridian(&source_geographic.datum);
    let target_prime_meridian = prime_meridian(&target_geographic.datum);
    let (source_meridian, target_meridian) = (
        &source_prime_meridian.longitude,
        &target_prime_meridian.longitude,
    );
    let meridian_offset = if source_meridian.unit.has_same_name(&target_meridian.unit) {
        Measure::new(
            source_meridian.value - target_meridian.value,
            source_meridian.unit.clone(),
        )
    } else {
        let degree = UnitOfMeasure::degree();
        Measure::new(
            source_meridian.convert_to_unit(&degree) - target_meridian.convert_to_unit(&degree),
            degree,
        )
    };
    let source_axes = &source_geographic.coordinate_system;
    let target_axes = &target_geographic.coordinate_system;
    let height_unit = |axes: &EllipsoidalCs| {
        axes.axes
            .get(2)
            .map_or(1.0, |axis| axis.unit.conversion_to_si)
    };
    let name = transformation_name(source, target);
    let same_datum = !force_ballpark
        && is_same_geodetic_datum(&source_geographic.datum, &target_geographic.datum);

    let mut axis_reversal_2d = false;
    let mut axis_reversal_3d = false;
    if !source_axes.is_equivalent_to(target_axes) {
        let latitude_first = |order: AxisOrder| {
            matches!(
                order,
                AxisOrder::LatNorthLongEast | AxisOrder::LatNorthLongEastHeightUp
            )
        };
        let longitude_first = |order: AxisOrder| {
            matches!(
                order,
                AxisOrder::LongEastLatNorth | AxisOrder::LongEastLatNorthHeightUp
            )
        };
        let (source_order, target_order) = (source_axes.axis_order(), target_axes.axis_order());
        let reversed = (latitude_first(source_order) && longitude_first(target_order))
            || (longitude_first(source_order) && latitude_first(target_order));
        if reversed {
            if source_axes.axes.len() == 3 || target_axes.axes.len() == 3 {
                axis_reversal_3d = true;
            } else {
                axis_reversal_2d = true;
            }
        }
    }

    let source_ellipsoid = source_geographic.datum.ellipsoid();
    let target_ellipsoid = target_geographic.datum.ellipsoid();
    let same_ellipsoid = source_ellipsoid.is_equivalent_to(target_ellipsoid);
    if height_unit(source_axes) != height_unit(target_axes) && same_ellipsoid {
        return Err(Error::Factory {
            kind: "coordinate operation",
            code: format!("{} to {}", source.name(), target.name()),
            reason: "a change of vertical unit is not ported".to_string(),
        });
    }
    if same_datum && (axis_reversal_2d || axis_reversal_3d) {
        return Ok(axis_order_reversal(axis_reversal_3d, source, target));
    }

    let source_si = source_meridian.si_value();
    let target_si = target_meridian.si_value();
    let mut steps = Vec::new();
    if same_ellipsoid && source_si != target_si {
        steps.push(longitude_rotation(name, source, target, meridian_offset));
    } else if source_si == 0.0 && target_si != 0.0 {
        let intermediate = intermediate_geographic_crs(
            format!(
                "{} altered to use prime meridian of {}",
                target.name(),
                source.name()
            ),
            vec![AreaOfUse::world().to_domain()],
            unnamed_datum(target_ellipsoid, source_prime_meridian),
            target_axes.clone(),
        );
        steps.push(ballpark_geographic_offset(
            source,
            &intermediate,
            force_ballpark,
        ));
        steps.push(longitude_rotation(
            transformation_name(source, &intermediate),
            &intermediate,
            target,
            meridian_offset,
        ));
    } else if source_si != target_si {
        let intermediate = intermediate_geographic_crs(
            format!(
                "{} altered to use prime meridian of {}",
                source.name(),
                target.name()
            ),
            Vec::new(),
            unnamed_datum(source_ellipsoid, target_prime_meridian),
            source_axes.clone(),
        );
        steps.push(longitude_rotation(
            transformation_name(source, &intermediate),
            source,
            &intermediate,
            meridian_offset,
        ));
        steps.push(ballpark_geographic_offset(
            &intermediate,
            target,
            force_ballpark,
        ));
    } else {
        steps.push(ballpark_geographic_offset(source, target, force_ballpark));
    }
    let operation = concatenate(steps, true)?.ok_or_else(|| Error::Factory {
        kind: "coordinate operation",
        code: format!("{} to {}", source.name(), target.name()),
        reason: "empty intersection of area of validity of concatenated operations".to_string(),
    })?;
    Ok(operation.with_ballpark(!same_datum))
}
