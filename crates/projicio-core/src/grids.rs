//! Runtime registration of NTv2 datum shift grids.
//!
//! projicio embeds no grid data. A definition that names a grid, such as the nadgrids
//! list behind `+datum=NAD27`, only transforms once the matching `.gsb` file has been
//! registered under the name the definition uses. Registering `conus` is enough to make
//! the NAD27 codes work, and a custom projstring can name any grid it likes with
//! `+nadgrids=`.
//!
//! Parsing and interpolation are proj4rs's, so a registered grid runs through the same
//! code as the rest of the fallback engine.
//!
//! Two properties follow from proj4rs keeping loaded grids on the heap for the lifetime
//! of the process: registration is global rather than per-transform, and a name can be
//! registered only once.

use crate::Error;
use proj4rs::nadgrids::{Catalog, NadGrids, catalog, files};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// Grids waiting for proj4rs to ask for them, and the names it already holds.
///
/// Never hold this lock while calling into proj4rs. The builder callback runs with
/// proj4rs's own catalog lock held and then takes this one, so the order is always
/// catalog first.
struct Registry {
    pending: BTreeMap<String, Vec<u8>>,
    /// Every name ever passed to a register function, successful or not.
    seen: BTreeSet<String>,
    /// Names proj4rs parsed and accepted.
    loaded: BTreeSet<String>,
    /// Set by the builder so a parse failure can be reported by the register call
    /// that triggered it, since proj4rs only tells us the grid was unavailable.
    last_error: Option<String>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    pending: BTreeMap::new(),
    seen: BTreeSet::new(),
    loaded: BTreeSet::new(),
    last_error: None,
});

static BUILDER: OnceLock<()> = OnceLock::new();

/// Hand proj4rs a loader that only ever looks in this registry.
///
/// Deliberately not proj4rs's own file loader: that one searches `PROJ_NADGRIDS` and
/// `PROJ_DATA`, and reading grids from the environment behind the caller's back is not
/// something projicio should do on its own.
fn install_builder() {
    BUILDER.get_or_init(|| {
        catalog::set_builder(builder);
    });
}

fn builder(catalog: &Catalog, key: &str) -> Result<(), proj4rs::errors::Error> {
    let bytes = REGISTRY.lock().unwrap().pending.remove(key);
    let Some(bytes) = bytes else {
        return Err(proj4rs::errors::Error::GridFileNotFound(key.into()));
    };

    match files::read(catalog, key, &mut Cursor::new(bytes)) {
        Ok(()) => {
            REGISTRY.lock().unwrap().loaded.insert(key.to_string());
            Ok(())
        }
        Err(e) => {
            REGISTRY.lock().unwrap().last_error = Some(e.to_string());
            Err(e)
        }
    }
}

/// Register an NTv2 grid from memory, under the name definitions refer to it by.
///
/// The grid is parsed straight away so a bad file is reported here rather than at the
/// first transform that needs it. A name can only be registered once per process.
///
/// ```
/// # let gsb: Vec<u8> = Vec::new();
/// # if !gsb.is_empty() {
/// projicio_core::grids::register_bytes("conus", gsb).unwrap();
/// # }
/// ```
pub fn register_bytes(name: &str, bytes: Vec<u8>) -> Result<(), Error> {
    if name.trim().is_empty() {
        return Err(Error::GridError("grid name must not be empty".into()));
    }
    install_builder();

    {
        let mut reg = REGISTRY.lock().unwrap();
        if !reg.seen.insert(name.to_string()) {
            return Err(Error::GridError(format!(
                "grid {name:?} was already registered, and loaded grids cannot be replaced for the life of the process"
            )));
        }
        reg.pending.insert(name.to_string(), bytes);
        reg.last_error = None;
    }

    // Ask proj4rs for the grid by name, which is what calls the builder above.
    if NadGrids::new_grid_transform(name).is_err() {
        let detail = REGISTRY.lock().unwrap().last_error.take();
        return Err(Error::GridError(match detail {
            Some(d) => format!("could not load NTv2 grid {name:?}: {d}"),
            None => format!("could not load NTv2 grid {name:?}"),
        }));
    }

    Ok(())
}

/// Register an NTv2 grid from a file, under the name definitions refer to it by.
///
/// The name is separate from the path because definitions do not agree on the two:
/// `+datum=NAD27` names `conus`, while `+nadgrids=` usually names a full file name.
pub fn register_file(name: &str, path: impl AsRef<Path>) -> Result<(), Error> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|e| {
        Error::GridError(format!("could not read grid file {}: {e}", path.display()))
    })?;
    register_bytes(name, bytes)
}

/// True when a grid of this name has been registered and parsed.
pub fn is_registered(name: &str) -> bool {
    REGISTRY.lock().unwrap().loaded.contains(name)
}

/// The names of every grid registered so far, sorted.
pub fn registered() -> Vec<String> {
    REGISTRY.lock().unwrap().loaded.iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_name_rejected() {
        assert!(register_bytes("", vec![0; 200]).is_err());
        assert!(register_bytes("   ", vec![0; 200]).is_err());
    }

    #[test]
    fn test_garbage_bytes_rejected() {
        // A name no CRS definition mentions, so this cannot disturb other tests.
        let err = register_bytes("projicio-unit-garbage", vec![0u8; 400]).unwrap_err();
        assert!(matches!(err, Error::GridError(_)));
        assert!(!is_registered("projicio-unit-garbage"));
    }

    #[test]
    fn test_name_cannot_be_registered_twice() {
        let name = "projicio-unit-duplicate";
        // The first call fails to parse, but the name is still spent.
        let _ = register_bytes(name, vec![0u8; 400]);
        let err = register_bytes(name, vec![0u8; 400]).unwrap_err();
        assert!(err.to_string().contains("already registered"), "{err}");
    }

    #[test]
    fn test_missing_file_reports_the_path() {
        let err = register_file("projicio-unit-missing", "/nonexistent/grid.gsb").unwrap_err();
        assert!(err.to_string().contains("/nonexistent/grid.gsb"), "{err}");
    }
}
