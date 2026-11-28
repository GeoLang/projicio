// adapted from PROJ src/grids.cpp, src/fwd.cpp and src/iso19111/operation/singleoperation.cpp (MIT), see LICENSES/PROJ-MIT.txt

mod grid;
mod reader;

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::projection::epsg_method::LambdaPhi;
use crate::projection::epsg_method::math::prepare_angular_input;
use crate::{DatumTransformation, Error, GeodeticCoord, registry};
use grid::HorizontalShiftGridSet;

static GRIDS: RwLock<BTreeMap<String, Arc<HorizontalShiftGridSet>>> = RwLock::new(BTreeMap::new());

pub fn register_bytes(name: &str, bytes: &[u8]) -> Result<(), Error> {
    if name.trim().is_empty() {
        return Err(Error::GridError("grid name must not be empty".into()));
    }
    let grid_set = reader::read_horizontal_shift_grids(name, bytes)
        .map_err(|reason| Error::GridError(format!("could not read grid {name:?}: {reason}")))?;
    match GRIDS.write().unwrap().entry(name.to_string()) {
        Entry::Occupied(_) => Err(Error::GridError(format!(
            "grid {name:?} was already registered, and a registered grid stays for the life of the process"
        ))),
        Entry::Vacant(slot) => {
            slot.insert(Arc::new(grid_set));
            Ok(())
        }
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn register_file(name: &str, path: impl AsRef<Path>) -> Result<(), Error> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|error| {
        Error::GridError(format!(
            "could not read grid file {}: {error}",
            path.display()
        ))
    })?;
    register_bytes(name, &bytes)
}

pub fn is_registered(name: &str) -> bool {
    GRIDS.read().unwrap().contains_key(name)
}

pub fn registered() -> Vec<String> {
    GRIDS.read().unwrap().keys().cloned().collect()
}

struct AcceptedName<'a> {
    name: &'a str,
    // proj.db's grid_alternatives.inverse_direction, which covers proj's file names and not epsg's
    file_applies_in_reverse: bool,
}

fn accepted(file_name: &str) -> Vec<AcceptedName<'_>> {
    let Some(alternative) = registry::grid_alternative_for_file(file_name) else {
        return vec![AcceptedName {
            name: file_name,
            file_applies_in_reverse: false,
        }];
    };
    // factory.cpp DatabaseContext::lookForGridAlternative matches only epsg's original_grid_name
    let epsg_name_given = file_name == alternative.original_name;
    let mut names: Vec<AcceptedName<'_>> = Vec::new();
    let candidates = [
        alternative.proj_grid_name,
        alternative.old_proj_grid_name,
        epsg_name_given.then_some(alternative.original_name),
    ];
    for name in candidates.into_iter().flatten() {
        if names.iter().all(|accepted| accepted.name != name) {
            names.push(AcceptedName {
                name,
                file_applies_in_reverse: epsg_name_given
                    && alternative.inverse_direction
                    && name != alternative.original_name,
            });
        }
    }
    names
}

pub fn accepted_names(file_name: &str) -> Vec<String> {
    accepted(file_name)
        .into_iter()
        .map(|accepted| accepted.name.to_string())
        .collect()
}

struct HorizontalGridShift {
    grids: Arc<HorizontalShiftGridSet>,
    file_applies_in_reverse: bool,
}

fn registered_grid_shift(file_name: &str) -> Option<HorizontalGridShift> {
    let grids = GRIDS.read().unwrap();
    accepted(file_name).into_iter().find_map(|accepted| {
        grids
            .get(accepted.name)
            .map(|grid_set| HorizontalGridShift {
                grids: Arc::clone(grid_set),
                file_applies_in_reverse: accepted.file_applies_in_reverse,
            })
    })
}

pub fn is_available(file_name: &str) -> bool {
    registered_grid_shift(file_name).is_some()
}

impl HorizontalGridShift {
    fn file_forward(&self, point: GeodeticCoord) -> Result<GeodeticCoord, Error> {
        // fwd.cpp fwd_prepare checks and wraps the input of every hgridshift step
        let prepared = prepare_angular_input(point.longitude, point.latitude)?;
        let shifted = self.grids.apply_forward(prepared)?;
        Ok(GeodeticCoord {
            longitude: shifted.lambda,
            latitude: shifted.phi,
            height: point.height,
        })
    }

    fn file_inverse(&self, point: GeodeticCoord) -> Result<GeodeticCoord, Error> {
        // inv.cpp inv_prepare rejects HUGE_VAL input
        if !(point.longitude.is_finite() && point.latitude.is_finite()) {
            return Err(Error::InvalidCoordinate(format!(
                "longitude {} latitude {} is not finite",
                point.longitude.to_degrees(),
                point.latitude.to_degrees()
            )));
        }
        let shifted = self.grids.apply_inverse(LambdaPhi {
            lambda: point.longitude,
            phi: point.latitude,
        })?;
        Ok(GeodeticCoord {
            longitude: shifted.lambda,
            latitude: shifted.phi,
            height: point.height,
        })
    }
}

impl DatumTransformation for HorizontalGridShift {
    fn forward(&self, point: GeodeticCoord, _epoch: Option<f64>) -> Result<GeodeticCoord, Error> {
        if self.file_applies_in_reverse {
            return self.file_inverse(point);
        }
        self.file_forward(point)
    }

    fn inverse(&self, point: GeodeticCoord, _epoch: Option<f64>) -> Result<GeodeticCoord, Error> {
        if self.file_applies_in_reverse {
            return self.file_forward(point);
        }
        self.file_inverse(point)
    }
}

const NADCON: u32 = 9613;
const NTV1: u32 = 9614;
const NTV2: u32 = 9615;
const NADCON5_2D: u32 = 1074;
const GEOCENTRIC_TRANSLATIONS_BY_GRID_IGN: u32 = 1087;
const LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE: u32 = 8656;
const LATITUDE_DIFFERENCE_FILE: u32 = 8657;

pub fn grid_transformation_from_epsg_method(
    method_code: u32,
    grid_files: &[(u32, &str)],
) -> Result<Box<dyn DatumTransformation + Send + Sync>, Error> {
    let parameter_code = match method_code {
        NTV1 | NTV2 => LATITUDE_AND_LONGITUDE_DIFFERENCE_FILE,
        // proj.db's NADCON alternatives are keyed by the latitude file
        NADCON => LATITUDE_DIFFERENCE_FILE,
        // Transformation::_exportToPROJString writes gridshift and xyzgridshift steps for these, and proj.db lists only GeoTIFF files for them
        NADCON5_2D | GEOCENTRIC_TRANSLATIONS_BY_GRID_IGN => {
            return Err(Error::ProjectionError(format!(
                "EPSG method {method_code} needs a GeoTIFF grid, and projicio reads no GeoTIFF grids yet"
            )));
        }
        _ => {
            return Err(Error::ProjectionError(format!(
                "EPSG method {method_code} is not a horizontal grid method"
            )));
        }
    };
    let file_name = grid_files
        .iter()
        .find(|(code, _)| *code == parameter_code)
        .map(|(_, file_name)| *file_name)
        .ok_or_else(|| {
            Error::ProjectionError(format!(
                "EPSG method {method_code}: missing parameter {parameter_code}"
            ))
        })?;
    let shift = registered_grid_shift(file_name).ok_or_else(|| {
        Error::GridError(format!(
            "grid {file_name} is not registered, register it under any of {}",
            accepted_names(file_name).join(", ")
        ))
    })?;
    Ok(Box::new(shift))
}
