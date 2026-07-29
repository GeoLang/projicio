use thiserror::Error;

/// Errors that can occur during coordinate transformations.
#[derive(Debug, Error)]
pub enum Error {
    #[error("unsupported CRS: {0}")]
    UnsupportedCrs(String),

    #[error("invalid coordinate: {0}")]
    InvalidCoordinate(String),

    #[error("projection error: {0}")]
    ProjectionError(String),

    #[error("grid error: {0}")]
    GridError(String),
}
