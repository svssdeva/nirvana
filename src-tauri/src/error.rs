//! Core error type. The serializable `AppError` boundary + command mapping land
//! in plan Task 4; this is the internal `Result` error used across the core,
//! pulled forward because the `os/` trait seams (Task 2) return it.

/// Errors from the core: OS access, parsing, persistence, hardware queries.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("registry error: {0}")]
    Registry(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(String),
    #[error("gpu unavailable: {0}")]
    GpuUnavailable(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("operation cancelled")]
    Cancelled,
}

impl From<rusqlite::Error> for CoreError {
    fn from(e: rusqlite::Error) -> Self {
        CoreError::Db(e.to_string())
    }
}

/// Convenience alias for core fallible operations.
pub type CoreResult<T> = Result<T, CoreError>;

/// Machine-readable error category sent to the frontend. Serializes to its
/// variant name (`"NotFound"`, `"Db"`, …) — matches the `ErrorKind` union in
/// `docs/api-contract.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ErrorKind {
    NotFound,
    Parse,
    Registry,
    Io,
    Db,
    GpuUnavailable,
    Unsupported,
    Cancelled,
}

/// The serializable error the IPC boundary returns to the frontend:
/// `{ kind, message }`. Commands return `Result<T, AppError>`; core code uses
/// `CoreError` and converts via `?` (see `From<CoreError>`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
}

impl From<CoreError> for AppError {
    fn from(e: CoreError) -> Self {
        let kind = match &e {
            CoreError::NotFound(_) => ErrorKind::NotFound,
            CoreError::Parse(_) => ErrorKind::Parse,
            CoreError::Registry(_) => ErrorKind::Registry,
            CoreError::Io(_) => ErrorKind::Io,
            CoreError::Db(_) => ErrorKind::Db,
            CoreError::GpuUnavailable(_) => ErrorKind::GpuUnavailable,
            CoreError::Unsupported(_) => ErrorKind::Unsupported,
            CoreError::Cancelled => ErrorKind::Cancelled,
        };
        AppError {
            kind,
            message: e.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_error_serializes_to_kind_and_message() {
        let app: AppError = CoreError::NotFound("game 7".into()).into();
        let json = serde_json::to_value(&app).unwrap();
        assert_eq!(json["kind"], "NotFound");
        assert!(json["message"].as_str().unwrap().contains("game 7"));
    }

    #[test]
    fn app_error_maps_each_variant_kind() {
        let cases = [
            (CoreError::Parse("x".into()), ErrorKind::Parse),
            (CoreError::Registry("x".into()), ErrorKind::Registry),
            (CoreError::Db("x".into()), ErrorKind::Db),
            (
                CoreError::GpuUnavailable("x".into()),
                ErrorKind::GpuUnavailable,
            ),
            (CoreError::Unsupported("x".into()), ErrorKind::Unsupported),
            (CoreError::Cancelled, ErrorKind::Cancelled),
        ];
        for (core, expected) in cases {
            let app: AppError = core.into();
            assert_eq!(app.kind, expected);
        }
    }
}
