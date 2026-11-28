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

    #[error("invalid WKT: {0}")]
    InvalidWkt(String),

    #[error("cannot write WKT: {0}")]
    UnsupportedWkt(String),

    #[error("invalid coordinate: {0}")]
    InvalidCoordinate(String),

    #[error("projection error: {0}")]
    ProjectionError(String),

    #[error("grid error: {0}")]
    GridError(String),

    #[error("invalid PROJJSON: {0}")]
    InvalidProjJson(String),

    #[error("no coordinate operation from {source_crs} to {target_crs} covers the point")]
    NoOperation {
        source_crs: String,
        target_crs: String,
    },

    #[error("cannot run {operation}: {reason}")]
    OperationNotExecutable { operation: String, reason: String },
}
