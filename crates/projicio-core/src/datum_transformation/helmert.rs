// adapted from PROJ src/transformations/helmert.cpp and src/iso19111/operation/singleoperation.cpp (MIT), see LICENSES/PROJ-MIT.txt

use crate::projection::epsg_method::Parameters;
use crate::{Error, GeocentricCoord};

const X_AXIS_TRANSLATION: u32 = 8605;
const Y_AXIS_TRANSLATION: u32 = 8606;
const Z_AXIS_TRANSLATION: u32 = 8607;
const X_AXIS_ROTATION: u32 = 8608;
const Y_AXIS_ROTATION: u32 = 8609;
const Z_AXIS_ROTATION: u32 = 8610;
const SCALE_DIFFERENCE: u32 = 8611;
const RATE_X_AXIS_TRANSLATION: u32 = 1040;
const RATE_Y_AXIS_TRANSLATION: u32 = 1041;
const RATE_Z_AXIS_TRANSLATION: u32 = 1042;
const RATE_X_AXIS_ROTATION: u32 = 1043;
const RATE_Y_AXIS_ROTATION: u32 = 1044;
const RATE_Z_AXIS_ROTATION: u32 = 1045;
const RATE_SCALE_DIFFERENCE: u32 = 1046;
const REFERENCE_EPOCH: u32 = 1047;
const ORDINATE_1_OF_EVALUATION_POINT: u32 = 8617;
const ORDINATE_2_OF_EVALUATION_POINT: u32 = 8618;
const ORDINATE_3_OF_EVALUATION_POINT: u32 = 8667;

// EPSG unit 1029, the year, converts reference epochs and rates to seconds with this factor
const SECONDS_PER_YEAR: f64 = 31_556_925.445;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RotationConvention {
    PositionVector,
    CoordinateFrame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RotationForm {
    SmallAngle,
    Exact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HelmertTerms {
    Translation,
    SevenParameter(RotationConvention, RotationForm),
    TimeDependent(RotationConvention),
    MolodenskyBadekas(RotationConvention),
}

#[derive(Debug, Clone, Copy)]
struct HelmertParameters {
    translation: [f64; 3],
    rotation: [f64; 3],
    scale_difference: f64,
}

#[derive(Debug, Clone, Copy)]
struct HelmertRates {
    per_year: HelmertParameters,
    reference_epoch: f64,
}

#[derive(Debug, Clone)]
pub struct Helmert {
    method_code: u32,
    // helmert.cpp keeps the Molodensky-Badekas evaluation point added into this translation
    reference: HelmertParameters,
    rates: Option<HelmertRates>,
    evaluation_point: [f64; 3],
    convention: RotationConvention,
    rotation_form: RotationForm,
}

fn read_vector(parameters: &Parameters, codes: [u32; 3]) -> Result<[f64; 3], Error> {
    Ok([
        parameters.get(codes[0])?,
        parameters.get(codes[1])?,
        parameters.get(codes[2])?,
    ])
}

fn read_reference(
    parameters: &Parameters,
    terms: HelmertTerms,
) -> Result<HelmertParameters, Error> {
    let translation = read_vector(
        parameters,
        [X_AXIS_TRANSLATION, Y_AXIS_TRANSLATION, Z_AXIS_TRANSLATION],
    )?;
    if terms == HelmertTerms::Translation {
        return Ok(HelmertParameters {
            translation,
            rotation: [0.0; 3],
            scale_difference: 0.0,
        });
    }
    Ok(HelmertParameters {
        translation,
        rotation: read_vector(
            parameters,
            [X_AXIS_ROTATION, Y_AXIS_ROTATION, Z_AXIS_ROTATION],
        )?,
        scale_difference: parameters.get(SCALE_DIFFERENCE)?,
    })
}

fn read_rates(parameters: &Parameters) -> Result<HelmertRates, Error> {
    let translation_per_second = read_vector(
        parameters,
        [
            RATE_X_AXIS_TRANSLATION,
            RATE_Y_AXIS_TRANSLATION,
            RATE_Z_AXIS_TRANSLATION,
        ],
    )?;
    let rotation_per_second = read_vector(
        parameters,
        [
            RATE_X_AXIS_ROTATION,
            RATE_Y_AXIS_ROTATION,
            RATE_Z_AXIS_ROTATION,
        ],
    )?;
    Ok(HelmertRates {
        per_year: HelmertParameters {
            translation: translation_per_second.map(|rate| rate * SECONDS_PER_YEAR),
            rotation: rotation_per_second.map(|rate| rate * SECONDS_PER_YEAR),
            scale_difference: parameters.get(RATE_SCALE_DIFFERENCE)? * SECONDS_PER_YEAR,
        },
        reference_epoch: parameters.get(REFERENCE_EPOCH)? / SECONDS_PER_YEAR,
    })
}

fn transpose(matrix: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|row| std::array::from_fn(|column| matrix[column][row]))
}

impl Helmert {
    pub(super) fn from_epsg_parameters(
        parameters: &Parameters,
        terms: HelmertTerms,
    ) -> Result<Self, Error> {
        let reference = read_reference(parameters, terms)?;
        let rates = match terms {
            HelmertTerms::TimeDependent(_) => Some(read_rates(parameters)?),
            _ => None,
        };
        let evaluation_point = match terms {
            HelmertTerms::MolodenskyBadekas(_) => read_vector(
                parameters,
                [
                    ORDINATE_1_OF_EVALUATION_POINT,
                    ORDINATE_2_OF_EVALUATION_POINT,
                    ORDINATE_3_OF_EVALUATION_POINT,
                ],
            )?,
            _ => [0.0; 3],
        };
        // helmert.cpp refuses a scale difference of minus one million ppm or less, molobadekas does not check it
        let refused_scale = reference.scale_difference <= -1.0
            && !matches!(terms, HelmertTerms::MolodenskyBadekas(_));
        if refused_scale {
            return Err(parameters.unsupported_value(SCALE_DIFFERENCE, reference.scale_difference));
        }
        let (convention, rotation_form) = match terms {
            HelmertTerms::Translation => (
                RotationConvention::CoordinateFrame,
                RotationForm::SmallAngle,
            ),
            HelmertTerms::SevenParameter(convention, rotation_form) => (convention, rotation_form),
            HelmertTerms::TimeDependent(convention)
            | HelmertTerms::MolodenskyBadekas(convention) => (convention, RotationForm::SmallAngle),
        };
        Ok(Self {
            method_code: parameters.method_code,
            reference: HelmertParameters {
                translation: std::array::from_fn(|axis| {
                    reference.translation[axis] + evaluation_point[axis]
                }),
                ..reference
            },
            rates,
            evaluation_point,
            convention,
            rotation_form,
        })
    }

    fn parameters_at(&self, epoch: Option<f64>) -> Result<HelmertParameters, Error> {
        let Some(rates) = &self.rates else {
            return Ok(self.reference);
        };
        let epoch = epoch.ok_or_else(|| {
            Error::InvalidCoordinate(format!(
                "EPSG method {} is time-dependent and needs a coordinate epoch",
                self.method_code
            ))
        })?;
        let elapsed_years = epoch - rates.reference_epoch;
        Ok(HelmertParameters {
            translation: std::array::from_fn(|axis| {
                self.reference.translation[axis] + rates.per_year.translation[axis] * elapsed_years
            }),
            rotation: std::array::from_fn(|axis| {
                self.reference.rotation[axis] + rates.per_year.rotation[axis] * elapsed_years
            }),
            scale_difference: self.reference.scale_difference
                + rates.per_year.scale_difference * elapsed_years,
        })
    }

    fn rotation_matrix(&self, rotation: [f64; 3]) -> [[f64; 3]; 3] {
        let [omega, phi, kappa] = rotation;
        let coordinate_frame = match self.rotation_form {
            RotationForm::Exact => {
                let (sin_omega, cos_omega) = omega.sin_cos();
                let (sin_phi, cos_phi) = phi.sin_cos();
                let (sin_kappa, cos_kappa) = kappa.sin_cos();
                [
                    [
                        cos_phi * cos_kappa,
                        cos_omega * sin_kappa + sin_omega * sin_phi * cos_kappa,
                        sin_omega * sin_kappa - cos_omega * sin_phi * cos_kappa,
                    ],
                    [
                        -cos_phi * sin_kappa,
                        cos_omega * cos_kappa - sin_omega * sin_phi * sin_kappa,
                        sin_omega * cos_kappa + cos_omega * sin_phi * sin_kappa,
                    ],
                    [sin_phi, -sin_omega * cos_phi, cos_omega * cos_phi],
                ]
            }
            RotationForm::SmallAngle => {
                [[1.0, kappa, -phi], [-kappa, 1.0, omega], [phi, -omega, 1.0]]
            }
        };
        match self.convention {
            RotationConvention::CoordinateFrame => coordinate_frame,
            RotationConvention::PositionVector => transpose(coordinate_frame),
        }
    }

    pub fn forward(
        &self,
        point: GeocentricCoord,
        epoch: Option<f64>,
    ) -> Result<GeocentricCoord, Error> {
        let parameters = self.parameters_at(epoch)?;
        let matrix = self.rotation_matrix(parameters.rotation);
        let scale = 1.0 + parameters.scale_difference;
        let [translation_x, translation_y, translation_z] = parameters.translation;
        let x = point.x - self.evaluation_point[0];
        let y = point.y - self.evaluation_point[1];
        let z = point.z - self.evaluation_point[2];
        Ok(GeocentricCoord {
            x: scale * (matrix[0][0] * x + matrix[0][1] * y + matrix[0][2] * z) + translation_x,
            y: scale * (matrix[1][0] * x + matrix[1][1] * y + matrix[1][2] * z) + translation_y,
            z: scale * (matrix[2][0] * x + matrix[2][1] * y + matrix[2][2] * z) + translation_z,
        })
    }

    pub fn inverse(
        &self,
        point: GeocentricCoord,
        epoch: Option<f64>,
    ) -> Result<GeocentricCoord, Error> {
        let parameters = self.parameters_at(epoch)?;
        let matrix = self.rotation_matrix(parameters.rotation);
        let scale = 1.0 + parameters.scale_difference;
        let [translation_x, translation_y, translation_z] = parameters.translation;
        let x = (point.x - translation_x) / scale;
        let y = (point.y - translation_y) / scale;
        let z = (point.z - translation_z) / scale;
        Ok(GeocentricCoord {
            x: (matrix[0][0] * x + matrix[1][0] * y + matrix[2][0] * z) + self.evaluation_point[0],
            y: (matrix[0][1] * x + matrix[1][1] * y + matrix[2][1] * z) + self.evaluation_point[1],
            z: (matrix[0][2] * x + matrix[1][2] * y + matrix[2][2] * z) + self.evaluation_point[2],
        })
    }
}
