// adapted from PROJ src/iso19111/operation/concatenatedoperation.cpp, oputils.cpp, coordinateoperation.cpp and coordinateoperationfactory.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::extent::AreaOfUse;
use crate::Error;
use crate::iso19111::coordinate_operation::{build_operation_name, modified_identifiers};
use crate::iso19111::metadata::Identifier;
use crate::iso19111::operation::Conversion;
use crate::iso19111::{ConcatenatedOperation, CoordinateOperation, Criterion, Crs};

pub(crate) const BALLPARK_GEOCENTRIC_TRANSLATION: &str = "Ballpark geocentric translation";
pub(crate) const NULL_GEOGRAPHIC_OFFSET: &str = "Null geographic offset";
pub(crate) const NULL_GEOCENTRIC_TRANSLATION: &str = "Null geocentric translation";
pub(crate) const BALLPARK_GEOGRAPHIC_OFFSET: &str = "Ballpark geographic offset";
const INVERSE_OF: &str = "Inverse of ";
const AXIS_ORDER_CHANGE_2D_NAME: &str = "axis order change (2D)";
const AXIS_ORDER_CHANGE_3D_NAME: &str = "axis order change (geographic3D horizontal)";
// proj's getAccuracy gives -1 for an operation of unknown accuracy
pub(crate) const UNKNOWN_ACCURACY: f64 = -1.0;

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateOperation {
    pub(crate) operation: CoordinateOperation,
    pub(crate) has_ballpark_transformation: bool,
    // proj's computedName_: the name joins the step names and is rebuilt on inversion
    computed_name: bool,
    // proj's InverseTransformation keeps the operation it inverts and gives it back when inverted, one entry per step
    inverted_steps: Vec<Option<CoordinateOperation>>,
}

fn retag(operation: &mut CoordinateOperation, source: &Crs, target: &Crs) {
    match operation {
        CoordinateOperation::Conversion {
            source_crs,
            target_crs,
            ..
        } => {
            **source_crs = source.clone();
            **target_crs = target.clone();
        }
        CoordinateOperation::Transformation(transformation) => {
            *transformation.source_crs = source.clone();
            *transformation.target_crs = target.clone();
        }
        CoordinateOperation::Concatenated(concatenated) => {
            *concatenated.source_crs = source.clone();
            *concatenated.target_crs = target.clone();
        }
        CoordinateOperation::PointMotion(point_motion) => {
            *point_motion.crs = source.clone();
        }
    }
}

fn derive_identifiers(operation: &mut CoordinateOperation) {
    let identifiers = modified_identifiers(operation.identifiers(), false, true);
    match operation {
        CoordinateOperation::Conversion { conversion, .. } => {
            conversion.identifiers = identifiers;
        }
        CoordinateOperation::Transformation(transformation) => {
            transformation.identifiers = identifiers;
        }
        CoordinateOperation::Concatenated(concatenated) => {
            concatenated.identifiers = identifiers;
        }
        CoordinateOperation::PointMotion(point_motion) => {
            point_motion.identifiers = identifiers;
        }
    }
}

fn step_inverse(
    step: &CoordinateOperation,
    inverted: &Option<CoordinateOperation>,
) -> Result<CoordinateOperation, Error> {
    match inverted {
        Some(original) => Ok(original.clone()),
        None => inverse_step(step),
    }
}

impl CandidateOperation {
    pub(crate) fn new(operation: CoordinateOperation) -> Self {
        let inverted_steps = vec![None; steps_of(&operation).len()];
        Self {
            operation,
            has_ballpark_transformation: false,
            computed_name: false,
            inverted_steps,
        }
    }

    pub(crate) fn with_ballpark(mut self, has_ballpark_transformation: bool) -> Self {
        self.has_ballpark_transformation = has_ballpark_transformation;
        self
    }

    pub fn operation(&self) -> &CoordinateOperation {
        &self.operation
    }

    pub fn has_ballpark_transformation(&self) -> bool {
        self.has_ballpark_transformation
    }

    pub fn name(&self) -> &str {
        self.operation.name()
    }

    pub fn source_crs(&self) -> &Crs {
        self.operation.source_crs()
    }

    pub fn target_crs(&self) -> &Crs {
        self.operation.target_crs()
    }

    pub fn accuracy(&self) -> Option<f64> {
        let accuracy = accuracy_of(&self.operation);
        (accuracy >= 0.0).then_some(accuracy)
    }

    pub fn area_of_use(&self) -> Option<AreaOfUse> {
        extent_of(&self.operation).area
    }

    pub fn steps(&self) -> &[CoordinateOperation] {
        steps_of(&self.operation)
    }

    pub(crate) fn accuracy_or_unknown(&self) -> f64 {
        accuracy_of(&self.operation)
    }

    pub(crate) fn has_identifiers(&self) -> bool {
        !self.operation.identifiers().is_empty()
            || self
                .steps()
                .iter()
                .any(|step| !step.identifiers().is_empty())
    }

    pub(crate) fn has_transformation_step(&self) -> bool {
        self.steps()
            .iter()
            .any(|step| !matches!(step, CoordinateOperation::Conversion { .. }))
    }

    pub(crate) fn requires_coordinate_epoch(&self) -> bool {
        // proj's requiresPerCoordinateInputTime
        self.steps().iter().any(|step| match step {
            CoordinateOperation::Transformation(transformation) => {
                is_time_dependent(&transformation.method.name)
            }
            CoordinateOperation::PointMotion(_) => true,
            CoordinateOperation::Conversion { .. } | CoordinateOperation::Concatenated(_) => false,
        })
    }

    pub(crate) fn grid_file_names(&self) -> Vec<String> {
        self.operation
            .grids_needed()
            .iter()
            .map(|grid| grid.file_name.to_string())
            .collect()
    }

    pub(crate) fn with_crs(mut self, source: &Crs, target: &Crs) -> Self {
        // proj's setCRSsUpdateInverse, which on a concatenated operation moves only its own endpoints
        retag(&mut self.operation, source, target);
        let single = !matches!(self.operation, CoordinateOperation::Concatenated(_));
        if let (true, Some(Some(inverted))) = (single, self.inverted_steps.first_mut()) {
            retag(inverted, target, source);
        }
        self
    }

    pub(crate) fn with_derived_identifiers(mut self) -> Self {
        derive_identifiers(&mut self.operation);
        let single = !matches!(self.operation, CoordinateOperation::Concatenated(_));
        if let (true, Some(Some(inverted))) = (single, self.inverted_steps.first_mut()) {
            derive_identifiers(inverted);
        }
        self
    }

    pub fn inverse(&self) -> Result<Self, Error> {
        let mut steps = Vec::new();
        let mut inverted_steps = Vec::new();
        for (step, inverted) in self.steps().iter().zip(&self.inverted_steps).rev() {
            steps.push(step_inverse(step, inverted)?);
            inverted_steps.push(Some(step.clone()));
        }
        let operation = match &self.operation {
            CoordinateOperation::Concatenated(concatenated) => {
                let name = if self.computed_name {
                    concatenated_name(&steps)
                } else {
                    inverse_name(&self.operation)
                };
                CoordinateOperation::Concatenated(ConcatenatedOperation {
                    name,
                    identifiers: modified_identifiers(&concatenated.identifiers, true, false),
                    remarks: concatenated.remarks.clone(),
                    domains: concatenated.domains.clone(),
                    source_crs: concatenated.target_crs.clone(),
                    target_crs: concatenated.source_crs.clone(),
                    steps,
                    accuracies: concatenated.accuracies.clone(),
                })
            }
            _ => steps.pop().expect("a single operation has one step"),
        };
        Ok(Self {
            operation,
            has_ballpark_transformation: self.has_ballpark_transformation,
            computed_name: self.computed_name,
            inverted_steps,
        })
    }
}

pub(crate) fn steps_of(operation: &CoordinateOperation) -> &[CoordinateOperation] {
    match operation {
        CoordinateOperation::Concatenated(concatenated) => &concatenated.steps,
        single => std::slice::from_ref(single),
    }
}

pub(crate) fn is_time_dependent(method_name: &str) -> bool {
    let lowercase = method_name.to_ascii_lowercase();
    lowercase.contains("time dependent") || lowercase.contains("time-dependent")
}

pub(crate) fn is_null_transformation(name: &str) -> bool {
    if name.contains(" + ") {
        return false;
    }
    [
        BALLPARK_GEOCENTRIC_TRANSLATION,
        BALLPARK_GEOGRAPHIC_OFFSET,
        NULL_GEOGRAPHIC_OFFSET,
        NULL_GEOCENTRIC_TRANSLATION,
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

fn concatenated_name(steps: &[CoordinateOperation]) -> String {
    // proj's computeConcatenatedName
    steps
        .iter()
        .map(|step| match step.name() {
            "" => "unnamed",
            name => name,
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

fn split_on_plus_outside_quotes(name: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut characters = name.char_indices();
    while let Some((index, character)) = characters.next() {
        if in_string {
            current.push(character);
            in_string = character != '\'';
        } else if index + 3 < name.len() && name[index..].starts_with(" + ") {
            tokens.push(std::mem::take(&mut current));
            characters.next();
            characters.next();
        } else {
            in_string = character == '\'';
            current.push(character);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn inverse_name(operation: &CoordinateOperation) -> String {
    // proj's createPropertiesForInverse
    let forward_name = operation.name();
    let is_transformation = matches!(operation, CoordinateOperation::Transformation(_));
    let is_concatenated = matches!(operation, CoordinateOperation::Concatenated(_));
    let operation_type = [
        BALLPARK_GEOCENTRIC_TRANSLATION,
        BALLPARK_GEOGRAPHIC_OFFSET,
        NULL_GEOGRAPHIC_OFFSET,
        NULL_GEOCENTRIC_TRANSLATION,
    ]
    .into_iter()
    .find(|prefix| forward_name.starts_with(prefix))
    .unwrap_or(
        if is_transformation || forward_name.starts_with("Transformation from ") {
            "Transformation"
        } else if matches!(operation, CoordinateOperation::Conversion { .. }) {
            "Conversion"
        } else {
            "Operation"
        },
    );
    let source = operation.source_crs();
    let target = operation.target_crs();
    if !forward_name.is_empty() {
        let tokenized = !is_transformation
            && !is_concatenated
            && (forward_name.starts_with(INVERSE_OF) || forward_name.contains(" + "));
        if tokenized {
            let tokens = split_on_plus_outside_quotes(forward_name);
            return tokens
                .iter()
                .rev()
                .map(|token| match token.strip_prefix(INVERSE_OF) {
                    Some(forward_token) => forward_token.to_string(),
                    None if token == AXIS_ORDER_CHANGE_2D_NAME
                        || token == AXIS_ORDER_CHANGE_3D_NAME =>
                    {
                        token.clone()
                    }
                    None => format!("{INVERSE_OF}{token}"),
                })
                .collect::<Vec<_>>()
                .join(" + ");
        }
        if forward_name != build_operation_name(operation_type, source, target) {
            if forward_name.contains(" + ") {
                return format!("{INVERSE_OF}'{forward_name}'");
            }
            return format!("{INVERSE_OF}{forward_name}");
        }
    }
    build_operation_name(operation_type, target, source)
}

fn inverse_step(step: &CoordinateOperation) -> Result<CoordinateOperation, Error> {
    let name = inverse_name(step);
    match step {
        CoordinateOperation::Conversion {
            conversion,
            source_crs,
            target_crs,
        } => Ok(CoordinateOperation::Conversion {
            conversion: Conversion {
                name,
                ..conversion.inverse()
            },
            source_crs: target_crs.clone(),
            target_crs: source_crs.clone(),
        }),
        CoordinateOperation::Transformation(transformation) => {
            let mut inverse = transformation.inverse()?;
            inverse.name = name;
            Ok(CoordinateOperation::Transformation(inverse))
        }
        CoordinateOperation::Concatenated(_) => {
            Ok(CandidateOperation::new(step.clone()).inverse()?.operation)
        }
        // PointMotionOperation::inverse in proj returns a plain copy when the operation has no source epoch
        CoordinateOperation::PointMotion(_) => Ok(step.clone()),
    }
}

pub(crate) fn accuracy_of(operation: &CoordinateOperation) -> f64 {
    // proj's getAccuracy
    if matches!(operation, CoordinateOperation::Conversion { .. }) {
        return 0.0;
    }
    match operation.accuracies().first() {
        Some(accuracy) => accuracy.parse().unwrap_or(UNKNOWN_ACCURACY),
        None => match operation {
            CoordinateOperation::Concatenated(concatenated) => {
                accuracy_of_steps(&concatenated.steps)
            }
            _ => UNKNOWN_ACCURACY,
        },
    }
}

fn accuracy_of_steps(steps: &[CoordinateOperation]) -> f64 {
    // proj's getAccuracy over a step list, without its ETRS89 no-op special case
    let mut accuracy = UNKNOWN_ACCURACY;
    for step in steps {
        let step_accuracy = accuracy_of(step);
        if step_accuracy < 0.0 {
            return UNKNOWN_ACCURACY;
        }
        if accuracy < 0.0 {
            accuracy = 0.0;
        }
        accuracy += step_accuracy;
    }
    accuracy
}

fn accuracy_text(value: f64) -> String {
    // proj's toString(double) prints 15 significant digits
    let rounded: f64 = format!("{value:.14e}")
        .parse()
        .expect("scientific notation parses back");
    format!("{rounded}")
}

pub(crate) struct OperationExtent {
    pub(crate) area: Option<AreaOfUse>,
    pub(crate) empty_intersection: bool,
}

pub(crate) fn extent_of(operation: &CoordinateOperation) -> OperationExtent {
    // proj's getExtent, where a conversion counts as valid over the world
    if matches!(operation, CoordinateOperation::Conversion { .. }) {
        return OperationExtent {
            area: Some(AreaOfUse::world()),
            empty_intersection: false,
        };
    }
    if let Some(area) = AreaOfUse::from_domains(operation_domains(operation)) {
        return OperationExtent {
            area: Some(area),
            empty_intersection: false,
        };
    }
    match operation {
        CoordinateOperation::Concatenated(concatenated) => extent_of_steps(&concatenated.steps),
        _ => OperationExtent {
            area: None,
            empty_intersection: false,
        },
    }
}

pub(crate) fn operation_domains(
    operation: &CoordinateOperation,
) -> &[crate::iso19111::common::ObjectDomain] {
    match operation {
        CoordinateOperation::Conversion { .. } => &[],
        CoordinateOperation::Transformation(transformation) => &transformation.domains,
        CoordinateOperation::PointMotion(point_motion) => &point_motion.domains,
        CoordinateOperation::Concatenated(concatenated) => &concatenated.domains,
    }
}

fn extent_of_steps(steps: &[CoordinateOperation]) -> OperationExtent {
    let mut result: Option<AreaOfUse> = None;
    for step in steps {
        let step_extent = extent_of(step);
        let Some(step_area) = step_extent.area else {
            if step_extent.empty_intersection {
                return step_extent;
            }
            continue;
        };
        result = match result {
            None => Some(step_area),
            Some(area) => match area.intersection(&step_area) {
                Some(intersection) => Some(intersection),
                None => {
                    return OperationExtent {
                        area: None,
                        empty_intersection: true,
                    };
                }
            },
        };
    }
    OperationExtent {
        area: result,
        empty_intersection: false,
    }
}

pub(crate) fn crs_equivalent(crs: &Crs, other: &Crs) -> bool {
    crs.is_equivalent_to(other, Criterion::Equivalent)
}

fn is_sphere_based(crs: &Crs) -> bool {
    crs.extract_geographic_crs()
        .is_some_and(|geographic| geographic.datum.ellipsoid().is_sphere())
}

fn conversions_equivalent(
    conversion: &Conversion,
    source: &Crs,
    other: &Conversion,
    other_source: &Crs,
) -> bool {
    conversion.is_equivalent_to(
        other,
        is_sphere_based(source) && is_sphere_based(other_source),
    )
}

pub(crate) fn steps_equivalent(step: &CoordinateOperation, other: &CoordinateOperation) -> bool {
    match (step, other) {
        (CoordinateOperation::Transformation(a), CoordinateOperation::Transformation(b)) => {
            a.is_equivalent_to(b)
                && crs_equivalent(&a.source_crs, &b.source_crs)
                && crs_equivalent(&a.target_crs, &b.target_crs)
        }
        (
            CoordinateOperation::Conversion {
                conversion: a,
                source_crs: a_source,
                ..
            },
            CoordinateOperation::Conversion {
                conversion: b,
                source_crs: b_source,
                ..
            },
        ) => conversions_equivalent(a, a_source, b, b_source),
        _ => false,
    }
}

pub(crate) fn candidates_equivalent(a: &CandidateOperation, b: &CandidateOperation) -> bool {
    let (a_steps, b_steps) = (a.steps(), b.steps());
    a_steps.len() == b_steps.len()
        && a_steps
            .iter()
            .zip(b_steps)
            .all(|(step, other)| steps_equivalent(step, other))
}

pub(crate) fn concatenate(
    operations: Vec<CandidateOperation>,
    check_extent: bool,
) -> Result<Option<CandidateOperation>, Error> {
    // proj's ConcatenatedOperation::createComputeMetadata, None where it throws InvalidOperationEmptyIntersection
    if operations.len() == 1 {
        return Ok(operations.into_iter().next());
    }
    let has_ballpark_transformation = operations
        .iter()
        .any(|operation| operation.has_ballpark_transformation);
    let mut steps: Vec<CoordinateOperation> = Vec::new();
    let mut inverted_steps: Vec<Option<CoordinateOperation>> = Vec::new();
    for operation in &operations {
        steps.extend(operation.steps().iter().cloned());
        inverted_steps.extend(operation.inverted_steps.iter().cloned());
    }

    if steps.len() > 2 {
        loop {
            let mut changed = false;
            for index in 0..steps.len() - 1 {
                let (step, next) = (&steps[index], &steps[index + 1]);
                let cancels =
                    steps_equivalent(step, &step_inverse(next, &inverted_steps[index + 1])?)
                        && crs_equivalent(step.source_crs(), next.target_crs());
                if cancels {
                    steps.drain(index..index + 2);
                    inverted_steps.drain(index..index + 2);
                    changed = true;
                    break;
                }
            }
            if !changed || steps.len() <= 2 {
                break;
            }
        }
    }
    if steps.len() == 1 {
        return Ok(Some(CandidateOperation {
            operation: steps.pop().expect("one step is left"),
            has_ballpark_transformation,
            computed_name: false,
            inverted_steps,
        }));
    }

    let extent = extent_of_steps(&steps);
    if check_extent && extent.empty_intersection {
        return Ok(None);
    }
    let accuracy = accuracy_of_steps(&steps);
    let accuracies = if accuracy >= 0.0 {
        vec![accuracy_text(accuracy)]
    } else {
        Vec::new()
    };
    let source_crs = Box::new(steps[0].source_crs().clone());
    let target_crs = Box::new(steps[steps.len() - 1].target_crs().clone());
    Ok(Some(CandidateOperation {
        operation: CoordinateOperation::Concatenated(ConcatenatedOperation {
            name: concatenated_name(&steps),
            identifiers: Vec::<Identifier>::new(),
            remarks: String::new(),
            domains: extent.area.iter().map(AreaOfUse::to_domain).collect(),
            source_crs,
            target_crs,
            steps,
            accuracies,
        }),
        has_ballpark_transformation,
        computed_name: true,
        inverted_steps,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_of_a_conversion_chain_reverses_its_tokens() {
        assert_eq!(
            split_on_plus_outside_quotes("Inverse of A + 'B + C' + D"),
            ["Inverse of A", "'B + C'", "D"]
        );
    }

    #[test]
    fn accuracy_text_keeps_fifteen_significant_digits() {
        assert_eq!(accuracy_text(0.1 + 0.2), "0.3");
        assert_eq!(accuracy_text(1.0), "1");
        assert_eq!(accuracy_text(7.5), "7.5");
    }

    #[test]
    fn null_transformation_names_have_no_step_separator() {
        assert!(is_null_transformation(
            "Ballpark geographic offset from NAD27 to WGS 84"
        ));
        assert!(!is_null_transformation(
            "Ballpark geographic offset from A to B + C"
        ));
        assert!(!is_null_transformation("NAD27 to WGS 84 (4)"));
    }
}
