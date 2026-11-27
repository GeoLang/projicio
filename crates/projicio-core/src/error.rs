use thiserror::Error;

/// Errors that can occur during coordinate transformations.
#[derive(Debug, Error)]
pub enum Error {
    #[error("{kind} {code} is not in the registry")]
    NoSuchAuthorityCode { kind: &'static str, code: String },

    #[error("cannot build {kind} {code}: {reason}")]
    Factory {
        kind: &'static str,
        code: String,
        reason: String,
    },

    #[error("unsupported CRS: {0}")]
    UnsupportedCrs(String),

    #[error("invalid coordinate: {0}")]
    InvalidCoordinate(String),

    #[error("projection error: {0}")]
    ProjectionError(String),

    #[error("grid error: {0}")]
    GridError(String),

    #[error("invalid PROJJSON: {0}")]
    InvalidProjJson(String),
}
