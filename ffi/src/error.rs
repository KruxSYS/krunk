//! Errors crossing the FFI. Kotlin sees `FfiException.Database(message)` etc.
use std::fmt;

/// Failures of the store and of parsing (flat: Kotlin gets the variant and a message).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
#[uniffi(flat_error)]
pub enum FfiError {
    /// SQLite failed (corrupt file, disk full, locked).
    Database(String),
    /// The input was rejected (bad JSON, bad URL, too large).
    Invalid(String),
}

impl fmt::Display for FfiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FfiError::Database(m) => write!(f, "database: {m}"),
            FfiError::Invalid(m) => write!(f, "invalid: {m}"),
        }
    }
}

impl std::error::Error for FfiError {}

impl From<lb_core::storage::DbError> for FfiError {
    fn from(e: lb_core::storage::DbError) -> Self {
        FfiError::Database(e.to_string())
    }
}

impl From<serde_json::Error> for FfiError {
    fn from(e: serde_json::Error) -> Self {
        FfiError::Invalid(e.to_string())
    }
}

/// One rejected settings field (the settings screen marks the input).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SettingsFieldError {
    /// The camelCase JSON name, e.g. `customSearchUrl`.
    pub field: String,
    pub message: String,
}

/// Saving settings failed: validation (with the fields) or the database.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum SettingsError {
    Validation { errors: Vec<SettingsFieldError> },
    Database { message: String },
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingsError::Validation { errors } => {
                let list: Vec<String> = errors.iter().map(|e| format!("{}: {}", e.field, e.message)).collect();
                write!(f, "invalid settings: {}", list.join("; "))
            }
            SettingsError::Database { message } => write!(f, "database: {message}"),
        }
    }
}

impl std::error::Error for SettingsError {}

impl From<lb_core::storage::DbError> for SettingsError {
    fn from(e: lb_core::storage::DbError) -> Self {
        SettingsError::Database { message: e.to_string() }
    }
}
