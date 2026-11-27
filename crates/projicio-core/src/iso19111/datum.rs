// adapted from PROJ src/iso19111/datum.cpp (MIT), see LICENSES/PROJ-MIT.txt

use super::common::{
    DEFAULT_MAX_RELATIVE_ERROR, Measure, ObjectDomain, UnitOfMeasure, names_are_equivalent,
};
use super::metadata::{EPSG, Identifier, is_equivalent_name};

#[derive(Debug, Clone, PartialEq)]
pub enum EllipsoidShape {
    Sphere,
    InverseFlattening(f64),
    SemiMinorAxis(Measure),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ellipsoid {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub semi_major_axis: Measure,
    pub shape: EllipsoidShape,
}

impl Ellipsoid {
    pub fn is_sphere(&self) -> bool {
        match &self.shape {
            EllipsoidShape::Sphere => true,
            EllipsoidShape::InverseFlattening(inverse_flattening) => *inverse_flattening == 0.0,
            EllipsoidShape::SemiMinorAxis(semi_minor_axis) => {
                self.semi_major_axis.is_identical_to(semi_minor_axis)
            }
        }
    }

    pub fn computed_inverse_flattening(&self) -> f64 {
        match &self.shape {
            EllipsoidShape::Sphere => 0.0,
            EllipsoidShape::InverseFlattening(inverse_flattening) => *inverse_flattening,
            EllipsoidShape::SemiMinorAxis(semi_minor_axis) => {
                let a = self.semi_major_axis.si_value();
                let b = semi_minor_axis.si_value();
                if a == b { 0.0 } else { a / (a - b) }
            }
        }
    }

    pub fn compute_semi_minor_axis(&self) -> Measure {
        match &self.shape {
            EllipsoidShape::Sphere => self.semi_major_axis.clone(),
            EllipsoidShape::InverseFlattening(inverse_flattening) => Measure::new(
                (1.0 - 1.0 / inverse_flattening) * self.semi_major_axis.value,
                self.semi_major_axis.unit.clone(),
            ),
            EllipsoidShape::SemiMinorAxis(semi_minor_axis) => semi_minor_axis.clone(),
        }
    }

    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        // PROJ: "clrk80" is named "Clarke 1880 mod." and GDAL exports several Clarke 1880 variants to it
        if self.name == "Clarke 1880 mod." || other.name == "Clarke 1880 mod." {
            let a = self.semi_major_axis.si_value();
            let inverse_flattening = self.computed_inverse_flattening();
            return (a - other.semi_major_axis.si_value()).abs() < 1e-8 * a
                && (inverse_flattening - other.computed_inverse_flattening()).abs()
                    < 1e-5 * inverse_flattening;
        }
        if !self
            .semi_major_axis
            .is_equivalent_to(&other.semi_major_axis, DEFAULT_MAX_RELATIVE_ERROR)
        {
            return false;
        }
        match (&self.shape, &other.shape) {
            (EllipsoidShape::SemiMinorAxis(b), EllipsoidShape::SemiMinorAxis(other_b))
                if !b.is_equivalent_to(other_b, DEFAULT_MAX_RELATIVE_ERROR) =>
            {
                return false;
            }
            (
                EllipsoidShape::InverseFlattening(inverse_flattening),
                EllipsoidShape::InverseFlattening(other_inverse_flattening),
            ) if (inverse_flattening - other_inverse_flattening).abs()
                > DEFAULT_MAX_RELATIVE_ERROR * inverse_flattening.abs() =>
            {
                return false;
            }
            _ => {}
        }
        self.compute_semi_minor_axis()
            .is_equivalent_to(&other.compute_semi_minor_axis(), DEFAULT_MAX_RELATIVE_ERROR)
    }
}

impl From<&Ellipsoid> for crate::Ellipsoid {
    fn from(ellipsoid: &Ellipsoid) -> Self {
        let inverse_flattening = ellipsoid.computed_inverse_flattening();
        let flattening = if inverse_flattening == 0.0 {
            0.0
        } else {
            1.0 / inverse_flattening
        };
        crate::Ellipsoid::new(ellipsoid.semi_major_axis.si_value(), flattening)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrimeMeridian {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub longitude: Measure,
}

impl PrimeMeridian {
    pub fn greenwich() -> Self {
        Self {
            name: "Greenwich".to_string(),
            identifiers: vec![Identifier::new(EPSG, "8901")],
            longitude: Measure::new(0.0, UnitOfMeasure::degree()),
        }
    }

    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        // PROJ: MapInfo writes the Paris meridian as 2.3372291666667 degrees, a relative error near 1e-9
        names_are_equivalent(&self.name, &other.name)
            && self.longitude.is_equivalent_to(&other.longitude, 1e-8)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DynamicFrame {
    pub frame_reference_epoch: f64,
    pub deformation_model_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeodeticReferenceFrame {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub remarks: String,
    pub domains: Vec<ObjectDomain>,
    pub anchor: Option<String>,
    pub anchor_epoch: Option<f64>,
    pub ellipsoid: Ellipsoid,
    pub prime_meridian: PrimeMeridian,
    pub dynamic: Option<DynamicFrame>,
}

fn starts_with_ignoring_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn frame_names_match_without_aliases(name: &str, other_name: &str) -> bool {
    // TODO: PROJ also matches datum names through the alias table in proj.db
    let names_match = |this_name: &str, that_name: &str| {
        this_name == that_name || this_name == "unknown" || that_name == "unknown"
    };
    // PROJ: Esri prefixes datum names with "D_", PROJ issue 4514
    let this_has_prefix = starts_with_ignoring_case(name, "D_");
    let other_has_prefix = starts_with_ignoring_case(other_name, "D_");
    if this_has_prefix && !other_has_prefix {
        return is_equivalent_name(&name[2..], other_name) || names_match(&name[2..], other_name);
    }
    if !this_has_prefix && other_has_prefix {
        return is_equivalent_name(name, &other_name[2..]) || names_match(name, &other_name[2..]);
    }
    names_match(name, other_name)
}

impl GeodeticReferenceFrame {
    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        let names_equivalent = is_equivalent_name(&self.name, &other.name)
            || frame_names_match_without_aliases(&self.name, &other.name)
            || names_are_equivalent(&self.name, &other.name);
        if !names_equivalent
            || !self.prime_meridian.is_equivalent_to(&other.prime_meridian)
            || !self.ellipsoid.is_equivalent_to(&other.ellipsoid)
        {
            return false;
        }
        match (&self.dynamic, &other.dynamic) {
            (Some(dynamic), Some(other_dynamic)) => {
                let epoch = dynamic.frame_reference_epoch;
                (epoch - other_dynamic.frame_reference_epoch).abs()
                    <= DEFAULT_MAX_RELATIVE_ERROR * epoch.abs()
                    && is_equivalent_name(
                        dynamic.deformation_model_name.as_deref().unwrap_or(""),
                        other_dynamic
                            .deformation_model_name
                            .as_deref()
                            .unwrap_or(""),
                    )
            }
            _ => true,
        }
    }
}

pub(crate) fn ensemble_name_to_non_ensemble_name(name: &str) -> Option<&'static str> {
    match name {
        "World Geodetic System 1984 ensemble" => Some("World Geodetic System 1984"),
        "European Terrestrial Reference System 1989 ensemble" => {
            Some("European Terrestrial Reference System 1989")
        }
        "Greenland Reference 1996 ensemble" => Some("Greenland 1996"),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DatumEnsemble {
    pub name: String,
    pub identifiers: Vec<Identifier>,
    pub members: Vec<GeodeticReferenceFrame>,
    pub accuracy: String,
}

impl DatumEnsemble {
    pub fn as_datum(&self) -> GeodeticReferenceFrame {
        // TODO: PROJ builds the datum from the registry when the ensemble has an identifier
        let first_member = &self.members[0];
        let name = ensemble_name_to_non_ensemble_name(&self.name).unwrap_or(self.name.as_str());
        GeodeticReferenceFrame {
            name: name.to_string(),
            identifiers: self.identifiers.iter().take(1).cloned().collect(),
            remarks: String::new(),
            domains: Vec::new(),
            anchor: None,
            anchor_epoch: None,
            ellipsoid: first_member.ellipsoid.clone(),
            prime_meridian: first_member.prime_meridian.clone(),
            dynamic: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GeodeticDatum {
    ReferenceFrame(Box<GeodeticReferenceFrame>),
    Ensemble(DatumEnsemble),
}

impl GeodeticDatum {
    pub fn ellipsoid(&self) -> &Ellipsoid {
        match self {
            Self::ReferenceFrame(frame) => &frame.ellipsoid,
            Self::Ensemble(ensemble) => &ensemble.members[0].ellipsoid,
        }
    }

    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        // PROJ compares a datum ensemble through the datum it stands for
        match (self, other) {
            (Self::ReferenceFrame(frame), Self::ReferenceFrame(other_frame)) => {
                frame.is_equivalent_to(other_frame)
            }
            (Self::Ensemble(ensemble), Self::Ensemble(other_ensemble)) => {
                names_are_equivalent(&ensemble.name, &other_ensemble.name)
                    && ensemble
                        .as_datum()
                        .is_equivalent_to(&other_ensemble.as_datum())
            }
            (Self::ReferenceFrame(frame), Self::Ensemble(ensemble))
            | (Self::Ensemble(ensemble), Self::ReferenceFrame(frame)) => {
                frame.is_equivalent_to(&ensemble.as_datum())
            }
        }
    }
}
