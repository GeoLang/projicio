use crate::read::{self, Reader};
use crate::write::{self, StringTooLongError};
use std::collections::BTreeMap;

pub struct StringTable<'a> {
    strings: Vec<&'a str>,
}

impl<'a> StringTable<'a> {
    pub fn decode(payload: &'a [u8], record_count: usize) -> Option<Self> {
        let mut strings = Vec::new();
        let mut offset = 0;
        for _ in 0..record_count {
            let (text, len) = read::string_u16(payload, offset)?;
            strings.push(text);
            offset += len;
        }
        (offset == payload.len()).then_some(Self { strings })
    }

    pub fn get(&self, index: u32) -> Option<&'a str> {
        self.strings.get(index as usize).copied()
    }

    fn string(&self, reader: &mut Reader<'a>) -> Option<&'a str> {
        self.get(reader.code()?)
    }

    fn optional_string(&self, reader: &mut Reader<'a>) -> Option<Option<&'a str>> {
        match reader.code()? {
            0 => Some(None),
            reference => self.get(reference - 1).map(Some),
        }
    }
}

pub struct StringIndex<'a> {
    strings: Vec<&'a str>,
    indexes: BTreeMap<&'a str, u32>,
}

impl<'a> StringIndex<'a> {
    pub fn from_use_counts(use_counts: &BTreeMap<&'a str, usize>) -> Self {
        let mut counted: Vec<(&'a str, usize)> = use_counts
            .iter()
            .map(|(text, count)| (*text, *count))
            .collect();
        // the most used strings get the shortest varint indexes
        counted.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(right.0)));
        let strings: Vec<&'a str> = counted.into_iter().map(|(text, _)| text).collect();
        let indexes = strings
            .iter()
            .zip(0u32..)
            .map(|(text, index)| (*text, index))
            .collect();
        Self { strings, indexes }
    }

    pub fn string_count(&self) -> usize {
        self.strings.len()
    }

    pub fn index(&self, text: &str) -> u32 {
        self.indexes[text]
    }

    pub fn encode(&self) -> Result<Vec<u8>, StringTooLongError> {
        let mut payload = Vec::new();
        for text in &self.strings {
            write::string_u16(&mut payload, text)?;
        }
        Ok(payload)
    }
}

pub fn decode_records<'a, T>(
    payload: &'a [u8],
    record_count: usize,
    mut decode: impl FnMut(&mut Reader<'a>) -> Option<T>,
) -> Option<Vec<T>> {
    let mut reader = Reader::new(payload);
    let records = (0..record_count)
        .map(|_| decode(&mut reader))
        .collect::<Option<Vec<T>>>()?;
    reader.is_exhausted().then_some(records)
}

fn write_code(out: &mut Vec<u8>, code: u32) {
    write::varint(out, u64::from(code));
}

fn write_string<'a>(out: &mut Vec<u8>, text: &'a str, strings: &mut impl FnMut(&'a str) -> u32) {
    write_code(out, strings(text));
}

fn write_optional_string<'a>(
    out: &mut Vec<u8>,
    text: Option<&'a str>,
    strings: &mut impl FnMut(&'a str) -> u32,
) {
    write::varint(out, text.map_or(0, |text| u64::from(strings(text)) + 1));
}

fn write_codes(out: &mut Vec<u8>, codes: &[u32]) {
    write::varint(out, codes.len() as u64);
    for code in codes {
        write_code(out, *code);
    }
}

fn read_codes(reader: &mut Reader<'_>) -> Option<Vec<u32>> {
    let count = reader.varint()?;
    (0..count).map(|_| reader.code()).collect()
}

fn read_if<T>(present: bool, decode: impl FnOnce() -> Option<T>) -> Option<Option<T>> {
    if present {
        decode().map(Some)
    } else {
        Some(None)
    }
}

fn find_tagged<T: Copy>(values: &[T], tag: u8, tag_of: impl Fn(T) -> u8) -> Option<T> {
    values.iter().copied().find(|value| tag_of(*value) == tag)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UnitKind {
    Length = 1,
    Angle = 2,
    Scale = 3,
    Time = 4,
}

const UNIT_KINDS: [UnitKind; 4] = [
    UnitKind::Length,
    UnitKind::Angle,
    UnitKind::Scale,
    UnitKind::Time,
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitRecord<'a> {
    pub code: u32,
    pub name: &'a str,
    pub kind: UnitKind,
    // epsg gives sexagesimal angle units such as 9110 no factor
    pub to_si_factor: Option<f64>,
}

impl<'a> UnitRecord<'a> {
    pub fn encode(&self, out: &mut Vec<u8>, strings: &mut impl FnMut(&'a str) -> u32) {
        write_code(out, self.code);
        write_string(out, self.name, strings);
        write::u8(out, self.kind as u8);
        write::optional_decimal(out, self.to_si_factor);
    }

    pub fn decode(reader: &mut Reader<'a>, strings: &StringTable<'a>) -> Option<Self> {
        Some(Self {
            code: reader.code()?,
            name: strings.string(reader)?,
            kind: find_tagged(&UNIT_KINDS, reader.u8()?, |kind| kind as u8)?,
            to_si_factor: reader.optional_decimal()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EllipsoidShape {
    InverseFlattening(f64),
    SemiMinorAxis(f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EllipsoidRecord<'a> {
    pub code: u32,
    pub name: &'a str,
    pub semi_major_axis: f64,
    pub unit_code: u32,
    pub shape: EllipsoidShape,
}

impl<'a> EllipsoidRecord<'a> {
    pub fn encode(&self, out: &mut Vec<u8>, strings: &mut impl FnMut(&'a str) -> u32) {
        write_code(out, self.code);
        write_string(out, self.name, strings);
        write::decimal(out, self.semi_major_axis);
        write_code(out, self.unit_code);
        let (inverse_flattening, semi_minor_axis) = match self.shape {
            EllipsoidShape::InverseFlattening(value) => (Some(value), None),
            EllipsoidShape::SemiMinorAxis(value) => (None, Some(value)),
        };
        write::optional_decimal(out, inverse_flattening);
        write::optional_decimal(out, semi_minor_axis);
    }

    pub fn decode(reader: &mut Reader<'a>, strings: &StringTable<'a>) -> Option<Self> {
        let code = reader.code()?;
        let name = strings.string(reader)?;
        let semi_major_axis = reader.decimal()?;
        let unit_code = reader.code()?;
        let shape = match (reader.optional_decimal()?, reader.optional_decimal()?) {
            (Some(inverse_flattening), None) => {
                EllipsoidShape::InverseFlattening(inverse_flattening)
            }
            (None, Some(semi_minor_axis)) => EllipsoidShape::SemiMinorAxis(semi_minor_axis),
            _ => return None,
        };
        Some(Self {
            code,
            name,
            semi_major_axis,
            unit_code,
            shape,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrimeMeridianRecord<'a> {
    pub code: u32,
    pub name: &'a str,
    pub longitude: f64,
    pub unit_code: u32,
}

impl<'a> PrimeMeridianRecord<'a> {
    pub fn encode(&self, out: &mut Vec<u8>, strings: &mut impl FnMut(&'a str) -> u32) {
        write_code(out, self.code);
        write_string(out, self.name, strings);
        write::decimal(out, self.longitude);
        write_code(out, self.unit_code);
    }

    pub fn decode(reader: &mut Reader<'a>, strings: &StringTable<'a>) -> Option<Self> {
        Some(Self {
            code: reader.code()?,
            name: strings.string(reader)?,
            longitude: reader.decimal()?,
            unit_code: reader.code()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DatumEnsemble {
    pub accuracy: f64,
    pub member_codes: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DatumRecord<'a> {
    pub code: u32,
    pub name: &'a str,
    pub ellipsoid_code: u32,
    pub prime_meridian_code: u32,
    // set only for dynamic datums
    pub frame_reference_epoch: Option<f64>,
    pub ensemble: Option<DatumEnsemble>,
}

impl<'a> DatumRecord<'a> {
    pub fn encode(&self, out: &mut Vec<u8>, strings: &mut impl FnMut(&'a str) -> u32) {
        write_code(out, self.code);
        write_string(out, self.name, strings);
        write_code(out, self.ellipsoid_code);
        write_code(out, self.prime_meridian_code);
        write::optional_decimal(out, self.frame_reference_epoch);
        write::optional_decimal(
            out,
            self.ensemble.as_ref().map(|ensemble| ensemble.accuracy),
        );
        let member_codes = self
            .ensemble
            .as_ref()
            .map_or(&[][..], |ensemble| ensemble.member_codes.as_slice());
        write_codes(out, member_codes);
    }

    pub fn decode(reader: &mut Reader<'a>, strings: &StringTable<'a>) -> Option<Self> {
        let code = reader.code()?;
        let name = strings.string(reader)?;
        let ellipsoid_code = reader.code()?;
        let prime_meridian_code = reader.code()?;
        let frame_reference_epoch = reader.optional_decimal()?;
        let ensemble_accuracy = reader.optional_decimal()?;
        let member_codes = read_codes(reader)?;
        let ensemble = match ensemble_accuracy {
            Some(accuracy) if !member_codes.is_empty() => Some(DatumEnsemble {
                accuracy,
                member_codes,
            }),
            None if member_codes.is_empty() => None,
            _ => return None,
        };
        Some(Self {
            code,
            name,
            ellipsoid_code,
            prime_meridian_code,
            frame_reference_epoch,
            ensemble,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CoordinateSystemKind {
    Cartesian = 1,
    Ellipsoidal = 2,
}

const COORDINATE_SYSTEM_KINDS: [CoordinateSystemKind; 2] = [
    CoordinateSystemKind::Cartesian,
    CoordinateSystemKind::Ellipsoidal,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Axis<'a> {
    pub name: &'a str,
    pub abbreviation: &'a str,
    // epsg orientation text, such as "east" or "North along 90°E"
    pub orientation: &'a str,
    pub unit_code: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinateSystemRecord<'a> {
    pub code: u32,
    pub kind: CoordinateSystemKind,
    pub axes: Vec<Axis<'a>>,
}

impl<'a> CoordinateSystemRecord<'a> {
    pub fn encode(&self, out: &mut Vec<u8>, strings: &mut impl FnMut(&'a str) -> u32) {
        write_code(out, self.code);
        write::u8(out, self.kind as u8);
        write::varint(out, self.axes.len() as u64);
        for axis in &self.axes {
            write_string(out, axis.name, strings);
            write_string(out, axis.abbreviation, strings);
            write_string(out, axis.orientation, strings);
            write_code(out, axis.unit_code);
        }
    }

    pub fn decode(reader: &mut Reader<'a>, strings: &StringTable<'a>) -> Option<Self> {
        let code = reader.code()?;
        let kind = find_tagged(&COORDINATE_SYSTEM_KINDS, reader.u8()?, |kind| kind as u8)?;
        let axis_count = reader.varint()?;
        let axes = (0..axis_count)
            .map(|_| {
                Some(Axis {
                    name: strings.string(reader)?,
                    abbreviation: strings.string(reader)?,
                    orientation: strings.string(reader)?,
                    unit_code: reader.code()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { code, kind, axes })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GeodeticCrsType {
    Geographic2D = 1,
    Geographic3D = 2,
    Geocentric = 3,
}

const GEODETIC_CRS_TYPES: [GeodeticCrsType; 3] = [
    GeodeticCrsType::Geographic2D,
    GeodeticCrsType::Geographic3D,
    GeodeticCrsType::Geocentric,
];

const PROJECTED_CRS_TAG: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrsDefinitionKind {
    Geodetic {
        geodetic_type: GeodeticCrsType,
        datum_code: u32,
    },
    Projected {
        base_crs_code: u32,
        conversion_code: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrsDefinitionRecord {
    pub code: u32,
    pub kind: CrsDefinitionKind,
    pub coordinate_system_code: u32,
    // in the order proj gives the crs domains
    pub extent_codes: Vec<u32>,
}

impl CrsDefinitionRecord {
    pub fn encode(&self, out: &mut Vec<u8>) {
        write_code(out, self.code);
        match self.kind {
            CrsDefinitionKind::Geodetic {
                geodetic_type,
                datum_code,
            } => {
                write::u8(out, geodetic_type as u8);
                write_code(out, datum_code);
            }
            CrsDefinitionKind::Projected {
                base_crs_code,
                conversion_code,
            } => {
                write::u8(out, PROJECTED_CRS_TAG);
                write_code(out, base_crs_code);
                write_code(out, conversion_code);
            }
        }
        write_code(out, self.coordinate_system_code);
        write_codes(out, &self.extent_codes);
    }

    pub fn decode(reader: &mut Reader<'_>) -> Option<Self> {
        let code = reader.code()?;
        let tag = reader.u8()?;
        let kind = if tag == PROJECTED_CRS_TAG {
            CrsDefinitionKind::Projected {
                base_crs_code: reader.code()?,
                conversion_code: reader.code()?,
            }
        } else {
            CrsDefinitionKind::Geodetic {
                geodetic_type: find_tagged(&GEODETIC_CRS_TYPES, tag, |kind| kind as u8)?,
                datum_code: reader.code()?,
            }
        };
        Some(Self {
            code,
            kind,
            coordinate_system_code: reader.code()?,
            extent_codes: read_codes(reader)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationMethod<'a> {
    pub code: u32,
    pub name: &'a str,
}

impl<'a> OperationMethod<'a> {
    fn encode(&self, out: &mut Vec<u8>, strings: &mut impl FnMut(&'a str) -> u32) {
        write_code(out, self.code);
        write_string(out, self.name, strings);
    }

    fn decode(reader: &mut Reader<'a>, strings: &StringTable<'a>) -> Option<Self> {
        Some(Self {
            code: reader.code()?,
            name: strings.string(reader)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParameterValue<'a> {
    pub code: u32,
    pub name: &'a str,
    pub value: f64,
    pub unit_code: u32,
}

fn write_parameters<'a>(
    out: &mut Vec<u8>,
    parameters: &[ParameterValue<'a>],
    strings: &mut impl FnMut(&'a str) -> u32,
) {
    write::varint(out, parameters.len() as u64);
    for parameter in parameters {
        write_code(out, parameter.code);
        write_string(out, parameter.name, strings);
        write::decimal(out, parameter.value);
        write_code(out, parameter.unit_code);
    }
}

fn read_parameters<'a>(
    reader: &mut Reader<'a>,
    strings: &StringTable<'a>,
) -> Option<Vec<ParameterValue<'a>>> {
    let count = reader.varint()?;
    (0..count)
        .map(|_| {
            Some(ParameterValue {
                code: reader.code()?,
                name: strings.string(reader)?,
                value: reader.decimal()?,
                unit_code: reader.code()?,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversionRecord<'a> {
    pub code: u32,
    pub name: &'a str,
    pub method: OperationMethod<'a>,
    pub parameters: Vec<ParameterValue<'a>>,
}

impl<'a> ConversionRecord<'a> {
    pub fn encode(&self, out: &mut Vec<u8>, strings: &mut impl FnMut(&'a str) -> u32) {
        write_code(out, self.code);
        write_string(out, self.name, strings);
        self.method.encode(out, strings);
        write_parameters(out, &self.parameters, strings);
    }

    pub fn decode(reader: &mut Reader<'a>, strings: &StringTable<'a>) -> Option<Self> {
        Some(Self {
            code: reader.code()?,
            name: strings.string(reader)?,
            method: OperationMethod::decode(reader, strings)?,
            parameters: read_parameters(reader, strings)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measure {
    pub value: f64,
    pub unit_code: u32,
}

impl Measure {
    fn encode(&self, out: &mut Vec<u8>) {
        write::decimal(out, self.value);
        write_code(out, self.unit_code);
    }

    fn decode(reader: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            value: reader.decimal()?,
            unit_code: reader.code()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasuredVector {
    pub values: [f64; 3],
    pub unit_code: u32,
}

impl MeasuredVector {
    fn encode(&self, out: &mut Vec<u8>) {
        for value in self.values {
            write::decimal(out, value);
        }
        write_code(out, self.unit_code);
    }

    fn decode(reader: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            values: [reader.decimal()?, reader.decimal()?, reader.decimal()?],
            unit_code: reader.code()?,
        })
    }
}

const HELMERT_ROTATION: u8 = 1 << 0;
const HELMERT_SCALE_DIFFERENCE: u8 = 1 << 1;
const HELMERT_RATE_TRANSLATION: u8 = 1 << 2;
const HELMERT_RATE_ROTATION: u8 = 1 << 3;
const HELMERT_RATE_SCALE_DIFFERENCE: u8 = 1 << 4;
const HELMERT_EPOCH: u8 = 1 << 5;
const HELMERT_PIVOT: u8 = 1 << 6;
const HELMERT_KNOWN_GROUPS: u8 = (1 << 7) - 1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HelmertParameters<'a> {
    pub method: OperationMethod<'a>,
    pub translation: MeasuredVector,
    pub rotation: Option<MeasuredVector>,
    pub scale_difference: Option<Measure>,
    pub rate_translation: Option<MeasuredVector>,
    pub rate_rotation: Option<MeasuredVector>,
    pub rate_scale_difference: Option<Measure>,
    pub epoch: Option<Measure>,
    // the evaluation point of molodensky-badekas methods
    pub pivot: Option<MeasuredVector>,
}

impl<'a> HelmertParameters<'a> {
    fn encode(&self, out: &mut Vec<u8>, strings: &mut impl FnMut(&'a str) -> u32) {
        self.method.encode(out, strings);
        let groups = [
            (HELMERT_ROTATION, self.rotation.is_some()),
            (HELMERT_SCALE_DIFFERENCE, self.scale_difference.is_some()),
            (HELMERT_RATE_TRANSLATION, self.rate_translation.is_some()),
            (HELMERT_RATE_ROTATION, self.rate_rotation.is_some()),
            (
                HELMERT_RATE_SCALE_DIFFERENCE,
                self.rate_scale_difference.is_some(),
            ),
            (HELMERT_EPOCH, self.epoch.is_some()),
            (HELMERT_PIVOT, self.pivot.is_some()),
        ];
        let present = groups
            .iter()
            .filter(|(_, is_present)| *is_present)
            .fold(0, |mask, (bit, _)| mask | bit);
        write::u8(out, present);
        self.translation.encode(out);
        if let Some(rotation) = self.rotation {
            rotation.encode(out);
        }
        if let Some(scale_difference) = self.scale_difference {
            scale_difference.encode(out);
        }
        if let Some(rate_translation) = self.rate_translation {
            rate_translation.encode(out);
        }
        if let Some(rate_rotation) = self.rate_rotation {
            rate_rotation.encode(out);
        }
        if let Some(rate_scale_difference) = self.rate_scale_difference {
            rate_scale_difference.encode(out);
        }
        if let Some(epoch) = self.epoch {
            epoch.encode(out);
        }
        if let Some(pivot) = self.pivot {
            pivot.encode(out);
        }
    }

    fn decode(reader: &mut Reader<'a>, strings: &StringTable<'a>) -> Option<Self> {
        let method = OperationMethod::decode(reader, strings)?;
        let present = reader.u8()?;
        if present & !HELMERT_KNOWN_GROUPS != 0 {
            return None;
        }
        let has = |bit: u8| present & bit != 0;
        Some(Self {
            method,
            translation: MeasuredVector::decode(reader)?,
            rotation: read_if(has(HELMERT_ROTATION), || MeasuredVector::decode(reader))?,
            scale_difference: read_if(has(HELMERT_SCALE_DIFFERENCE), || Measure::decode(reader))?,
            rate_translation: read_if(has(HELMERT_RATE_TRANSLATION), || {
                MeasuredVector::decode(reader)
            })?,
            rate_rotation: read_if(has(HELMERT_RATE_ROTATION), || {
                MeasuredVector::decode(reader)
            })?,
            rate_scale_difference: read_if(has(HELMERT_RATE_SCALE_DIFFERENCE), || {
                Measure::decode(reader)
            })?,
            epoch: read_if(has(HELMERT_EPOCH), || Measure::decode(reader))?,
            pivot: read_if(has(HELMERT_PIVOT), || MeasuredVector::decode(reader))?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridFileReference<'a> {
    pub parameter_code: u32,
    pub parameter_name: &'a str,
    // epsg's file name, which GridAlternativeRecord maps to proj's file
    pub file_name: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransformationParameters<'a> {
    pub method: OperationMethod<'a>,
    pub parameters: Vec<ParameterValue<'a>>,
    pub grids: Vec<GridFileReference<'a>>,
    pub interpolation_crs_code: Option<u32>,
}

impl<'a> TransformationParameters<'a> {
    fn encode(&self, out: &mut Vec<u8>, strings: &mut impl FnMut(&'a str) -> u32) {
        self.method.encode(out, strings);
        write_parameters(out, &self.parameters, strings);
        write::varint(out, self.grids.len() as u64);
        for grid in &self.grids {
            write_code(out, grid.parameter_code);
            write_string(out, grid.parameter_name, strings);
            write_string(out, grid.file_name, strings);
        }
        write_code(out, self.interpolation_crs_code.unwrap_or(crate::NO_CODE));
    }

    fn decode(reader: &mut Reader<'a>, strings: &StringTable<'a>) -> Option<Self> {
        let method = OperationMethod::decode(reader, strings)?;
        let parameters = read_parameters(reader, strings)?;
        let grid_count = reader.varint()?;
        let grids = (0..grid_count)
            .map(|_| {
                Some(GridFileReference {
                    parameter_code: reader.code()?,
                    parameter_name: strings.string(reader)?,
                    file_name: strings.string(reader)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let interpolation_crs_code = reader.code()?;
        Some(Self {
            method,
            parameters,
            grids,
            interpolation_crs_code: (interpolation_crs_code != crate::NO_CODE)
                .then_some(interpolation_crs_code),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStep {
    Operation(u32),
    Conversion(u32),
}

impl OperationStep {
    fn encode(&self, out: &mut Vec<u8>) {
        let (code, is_conversion) = match *self {
            OperationStep::Operation(code) => (code, 0),
            OperationStep::Conversion(code) => (code, 1),
        };
        write::varint(out, (u64::from(code) << 1) | is_conversion);
    }

    fn decode(reader: &mut Reader<'_>) -> Option<Self> {
        let value = reader.varint()?;
        let code = u32::try_from(value >> 1).ok()?;
        Some(if value & 1 == 0 {
            OperationStep::Operation(code)
        } else {
            OperationStep::Conversion(code)
        })
    }
}

const OPERATION_HELMERT: u8 = 1;
const OPERATION_GRID: u8 = 2;
const OPERATION_OTHER: u8 = 3;
const OPERATION_CONCATENATED: u8 = 4;

#[derive(Debug, Clone, PartialEq)]
pub enum OperationKind<'a> {
    HelmertTransformation(Box<HelmertParameters<'a>>),
    GridTransformation(TransformationParameters<'a>),
    OtherTransformation(TransformationParameters<'a>),
    ConcatenatedOperation(Vec<OperationStep>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationRecord<'a> {
    pub code: u32,
    pub name: &'a str,
    pub source_crs_code: u32,
    pub target_crs_code: u32,
    pub accuracy: Option<f64>,
    // in the order proj gives the operation domains
    pub extent_codes: Vec<u32>,
    pub kind: OperationKind<'a>,
}

impl<'a> OperationRecord<'a> {
    pub fn encode(&self, out: &mut Vec<u8>, strings: &mut impl FnMut(&'a str) -> u32) {
        write_code(out, self.code);
        write_string(out, self.name, strings);
        write_code(out, self.source_crs_code);
        write_code(out, self.target_crs_code);
        write::optional_decimal(out, self.accuracy);
        write_codes(out, &self.extent_codes);
        match &self.kind {
            OperationKind::HelmertTransformation(parameters) => {
                write::u8(out, OPERATION_HELMERT);
                parameters.encode(out, strings);
            }
            OperationKind::GridTransformation(parameters) => {
                write::u8(out, OPERATION_GRID);
                parameters.encode(out, strings);
            }
            OperationKind::OtherTransformation(parameters) => {
                write::u8(out, OPERATION_OTHER);
                parameters.encode(out, strings);
            }
            OperationKind::ConcatenatedOperation(steps) => {
                write::u8(out, OPERATION_CONCATENATED);
                write::varint(out, steps.len() as u64);
                for step in steps {
                    step.encode(out);
                }
            }
        }
    }

    pub fn decode(reader: &mut Reader<'a>, strings: &StringTable<'a>) -> Option<Self> {
        let code = reader.code()?;
        let name = strings.string(reader)?;
        let source_crs_code = reader.code()?;
        let target_crs_code = reader.code()?;
        let accuracy = reader.optional_decimal()?;
        let extent_codes = read_codes(reader)?;
        let kind = match reader.u8()? {
            OPERATION_HELMERT => OperationKind::HelmertTransformation(Box::new(
                HelmertParameters::decode(reader, strings)?,
            )),
            OPERATION_GRID => OperationKind::GridTransformation(TransformationParameters::decode(
                reader, strings,
            )?),
            OPERATION_OTHER => OperationKind::OtherTransformation(
                TransformationParameters::decode(reader, strings)?,
            ),
            OPERATION_CONCATENATED => {
                let step_count = reader.varint()?;
                let steps = (0..step_count)
                    .map(|_| OperationStep::decode(reader))
                    .collect::<Option<Vec<_>>>()?;
                OperationKind::ConcatenatedOperation(steps)
            }
            _ => return None,
        };
        Some(Self {
            code,
            name,
            source_crs_code,
            target_crs_code,
            accuracy,
            extent_codes,
            kind,
        })
    }
}

const GRID_INVERSE_DIRECTION: u8 = 1 << 0;
const GRID_OPEN_LICENSE: u8 = 1 << 1;
const GRID_DIRECT_DOWNLOAD: u8 = 1 << 2;
const GRID_KNOWN_FLAGS: u8 = (1 << 3) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridAlternativeRecord<'a> {
    pub original_name: &'a str,
    // file name on cdn.proj.org, absent when the cdn does not carry the grid
    pub proj_grid_name: Option<&'a str>,
    pub old_proj_grid_name: Option<&'a str>,
    // "GTiff", "GTX", "NTv2" or "JSON"
    pub format: &'a str,
    // proj's operation for the file, such as "hgridshift" or "geocentricoffset"
    pub proj_method: &'a str,
    // proj's file applies the shift in the opposite direction to the epsg operation
    pub inverse_direction: bool,
    pub url: Option<&'a str>,
    pub open_license: bool,
    pub direct_download: bool,
}

impl<'a> GridAlternativeRecord<'a> {
    pub fn encode(&self, out: &mut Vec<u8>, strings: &mut impl FnMut(&'a str) -> u32) {
        write_string(out, self.original_name, strings);
        write_optional_string(out, self.proj_grid_name, strings);
        write_optional_string(out, self.old_proj_grid_name, strings);
        write_string(out, self.format, strings);
        write_string(out, self.proj_method, strings);
        write_optional_string(out, self.url, strings);
        let flags = [
            (GRID_INVERSE_DIRECTION, self.inverse_direction),
            (GRID_OPEN_LICENSE, self.open_license),
            (GRID_DIRECT_DOWNLOAD, self.direct_download),
        ];
        let flags = flags
            .iter()
            .filter(|(_, is_set)| *is_set)
            .fold(0, |mask, (bit, _)| mask | bit);
        write::u8(out, flags);
    }

    pub fn decode(reader: &mut Reader<'a>, strings: &StringTable<'a>) -> Option<Self> {
        let original_name = strings.string(reader)?;
        let proj_grid_name = strings.optional_string(reader)?;
        let old_proj_grid_name = strings.optional_string(reader)?;
        let format = strings.string(reader)?;
        let proj_method = strings.string(reader)?;
        let url = strings.optional_string(reader)?;
        let flags = reader.u8()?;
        if flags & !GRID_KNOWN_FLAGS != 0 {
            return None;
        }
        Some(Self {
            original_name,
            proj_grid_name,
            old_proj_grid_name,
            format,
            proj_method,
            inverse_direction: flags & GRID_INVERSE_DIRECTION != 0,
            url,
            open_license: flags & GRID_OPEN_LICENSE != 0,
            direct_download: flags & GRID_DIRECT_DOWNLOAD != 0,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupersessionRecord {
    pub superseded_code: u32,
    pub replacement_code: u32,
    // proj hides a superseded operation only through rows with this set
    pub same_source_target_crs: bool,
}

impl SupersessionRecord {
    pub fn encode(&self, out: &mut Vec<u8>) {
        write_code(out, self.superseded_code);
        write_code(out, self.replacement_code);
        write::u8(out, u8::from(self.same_source_target_crs));
    }

    pub fn decode(reader: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            superseded_code: reader.code()?,
            replacement_code: reader.code()?,
            same_source_target_crs: match reader.u8()? {
                0 => false,
                1 => true,
                _ => return None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Encoded {
        strings: Vec<u8>,
        string_count: usize,
        payload: Vec<u8>,
    }

    fn encode(
        encode_record: impl Fn(&mut Vec<u8>, &mut dyn FnMut(&'static str) -> u32),
    ) -> Encoded {
        let mut use_counts = BTreeMap::new();
        encode_record(&mut Vec::new(), &mut |text| {
            *use_counts.entry(text).or_insert(0) += 1;
            0
        });
        let index = StringIndex::from_use_counts(&use_counts);
        let mut payload = Vec::new();
        encode_record(&mut payload, &mut |text| index.index(text));
        Encoded {
            strings: index.encode().unwrap(),
            string_count: index.string_count(),
            payload,
        }
    }

    fn method(code: u32, name: &'static str) -> OperationMethod<'static> {
        OperationMethod { code, name }
    }

    #[test]
    fn time_dependent_helmert_roundtrips() {
        let record = OperationRecord {
            code: 8366,
            name: "ITRF2014 to ETRF2014 (1)",
            source_crs_code: 7789,
            target_crs_code: 8401,
            accuracy: Some(0.0),
            extent_codes: vec![1298],
            kind: OperationKind::HelmertTransformation(Box::new(HelmertParameters {
                method: method(1053, "Time-dependent Position Vector tfm (geocentric)"),
                translation: MeasuredVector {
                    values: [0.0, 0.0, 0.0],
                    unit_code: 1025,
                },
                rotation: Some(MeasuredVector {
                    values: [0.0, 0.0, 0.0],
                    unit_code: 1031,
                }),
                scale_difference: Some(Measure {
                    value: 0.0,
                    unit_code: 1028,
                }),
                rate_translation: Some(MeasuredVector {
                    values: [0.0, 0.0, 0.0],
                    unit_code: 1027,
                }),
                rate_rotation: Some(MeasuredVector {
                    values: [0.085, 0.531, -0.77],
                    unit_code: 1032,
                }),
                rate_scale_difference: Some(Measure {
                    value: 0.0,
                    unit_code: 1030,
                }),
                epoch: Some(Measure {
                    value: 1989.0,
                    unit_code: 1029,
                }),
                pivot: None,
            })),
        };

        let encoded = encode(|out, strings| record.encode(out, &mut |text| strings(text)));
        let strings = StringTable::decode(&encoded.strings, encoded.string_count).unwrap();
        let decoded = decode_records(&encoded.payload, 1, |reader| {
            OperationRecord::decode(reader, &strings)
        })
        .unwrap();

        assert_eq!(decoded, [record]);
    }

    #[test]
    fn grid_and_concatenated_operations_roundtrip() {
        let grid = OperationRecord {
            code: 10811,
            name: "NKG_ETRF14 to ETRS89-NOR [EUREF89] (1)",
            source_crs_code: 10805,
            target_crs_code: 10873,
            accuracy: Some(0.005),
            extent_codes: vec![1352],
            kind: OperationKind::GridTransformation(TransformationParameters {
                method: method(
                    1142,
                    "Geocen translations by grid (gtg) & Geocen translations NEU velocities (gtg)",
                ),
                parameters: vec![
                    ParameterValue {
                        code: 1068,
                        name: "Source epoch",
                        value: 2000.0,
                        unit_code: 1029,
                    },
                    ParameterValue {
                        code: 1069,
                        name: "Target epoch",
                        value: 1995.0,
                        unit_code: 1029,
                    },
                ],
                grids: vec![
                    GridFileReference {
                        parameter_code: 8727,
                        parameter_name: "Geocentric translation file",
                        file_name: "no_kv_NKGETRF14_EPSG7922_2000.tif",
                    },
                    GridFileReference {
                        parameter_code: 1050,
                        parameter_name: "Point motion velocity grid file",
                        file_name: "NKG_RF17vel.tif",
                    },
                ],
                interpolation_crs_code: Some(10807),
            }),
        };
        let concatenated = OperationRecord {
            code: 10778,
            name: "KKJ to EUREF-FIN (3)",
            source_crs_code: 4123,
            target_crs_code: 10690,
            accuracy: Some(0.03),
            extent_codes: vec![3333],
            kind: OperationKind::ConcatenatedOperation(vec![
                OperationStep::Conversion(18193),
                OperationStep::Operation(10703),
                OperationStep::Conversion(16065),
            ]),
        };

        let encoded = encode(|out, strings| {
            grid.encode(out, &mut |text| strings(text));
            concatenated.encode(out, &mut |text| strings(text));
        });
        let strings = StringTable::decode(&encoded.strings, encoded.string_count).unwrap();
        let decoded = decode_records(&encoded.payload, 2, |reader| {
            OperationRecord::decode(reader, &strings)
        })
        .unwrap();

        assert_eq!(decoded, [grid, concatenated]);
    }

    #[test]
    fn datum_ensemble_and_coordinate_system_roundtrip() {
        let ensemble = DatumRecord {
            code: 6326,
            name: "World Geodetic System 1984 ensemble",
            ellipsoid_code: 7030,
            prime_meridian_code: 8901,
            frame_reference_epoch: None,
            ensemble: Some(DatumEnsemble {
                accuracy: 2.0,
                member_codes: vec![1166, 1152, 1153],
            }),
        };
        let coordinate_system = CoordinateSystemRecord {
            code: 4499,
            kind: CoordinateSystemKind::Cartesian,
            axes: vec![
                Axis {
                    name: "Easting",
                    abbreviation: "X",
                    orientation: "east",
                    unit_code: 9001,
                },
                Axis {
                    name: "Northing",
                    abbreviation: "Y",
                    orientation: "north",
                    unit_code: 9001,
                },
            ],
        };

        let encoded = encode(|out, strings| {
            ensemble.encode(out, &mut |text| strings(text));
            coordinate_system.encode(out, &mut |text| strings(text));
        });
        let strings = StringTable::decode(&encoded.strings, encoded.string_count).unwrap();
        let mut reader = Reader::new(&encoded.payload);

        assert_eq!(DatumRecord::decode(&mut reader, &strings), Some(ensemble));
        assert_eq!(
            CoordinateSystemRecord::decode(&mut reader, &strings),
            Some(coordinate_system)
        );
        assert!(reader.is_exhausted());
    }

    #[test]
    fn crs_definitions_and_grid_alternatives_roundtrip() {
        let projected = CrsDefinitionRecord {
            code: 2154,
            kind: CrsDefinitionKind::Projected {
                base_crs_code: 4171,
                conversion_code: 18085,
            },
            coordinate_system_code: 4499,
            extent_codes: vec![1096],
        };
        let geocentric = CrsDefinitionRecord {
            code: 4978,
            kind: CrsDefinitionKind::Geodetic {
                geodetic_type: GeodeticCrsType::Geocentric,
                datum_code: 6326,
            },
            coordinate_system_code: 6500,
            extent_codes: vec![1262],
        };
        let alternative = GridAlternativeRecord {
            original_name: "rdtrans2008",
            proj_grid_name: None,
            old_proj_grid_name: Some("rdtrans2008.gsb"),
            format: "NTv2",
            proj_method: "hgridshift",
            inverse_direction: false,
            url: Some(
                "https://salsa.debian.org/debian-gis-team/proj-rdnap/raw/upstream/2008/rdtrans2008.gsb",
            ),
            open_license: false,
            direct_download: true,
        };

        let mut payload = Vec::new();
        projected.encode(&mut payload);
        geocentric.encode(&mut payload);
        let encoded = encode(|out, strings| alternative.encode(out, &mut |text| strings(text)));
        let strings = StringTable::decode(&encoded.strings, encoded.string_count).unwrap();

        assert_eq!(
            decode_records(&payload, 2, CrsDefinitionRecord::decode),
            Some(vec![projected, geocentric])
        );
        assert_eq!(
            decode_records(&encoded.payload, 1, |reader| {
                GridAlternativeRecord::decode(reader, &strings)
            }),
            Some(vec![alternative])
        );
    }

    #[test]
    fn record_count_must_fill_the_payload() {
        let mut payload = Vec::new();
        let supersession = SupersessionRecord {
            superseded_code: 1112,
            replacement_code: 1672,
            same_source_target_crs: true,
        };
        supersession.encode(&mut payload);
        supersession.encode(&mut payload);

        assert_eq!(
            decode_records(&payload, 2, SupersessionRecord::decode),
            Some(vec![supersession, supersession])
        );
        assert_eq!(
            decode_records(&payload, 1, SupersessionRecord::decode),
            None
        );
        assert_eq!(
            decode_records(&payload, 3, SupersessionRecord::decode),
            None
        );
    }

    #[test]
    fn frequent_strings_get_the_smallest_indexes() {
        let use_counts = BTreeMap::from([("metre", 9), ("Easting", 4), ("Afgooye", 1)]);
        let index = StringIndex::from_use_counts(&use_counts);

        assert_eq!(index.index("metre"), 0);
        assert_eq!(index.index("Easting"), 1);
        assert_eq!(index.index("Afgooye"), 2);
        let encoded = index.encode().unwrap();
        let table = StringTable::decode(&encoded, index.string_count()).unwrap();
        assert_eq!(table.get(1), Some("Easting"));
        assert_eq!(table.get(3), None);
    }
}
