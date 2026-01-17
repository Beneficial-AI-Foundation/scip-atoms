//! Error types for probe-verus.
//!
//! This module provides a unified error type hierarchy for the probe-verus library.
//! Using `thiserror` for derive macros makes error handling more ergonomic.

use std::path::PathBuf;
use thiserror::Error;

/// Main error type for probe-verus operations.
#[derive(Debug, Error)]
pub enum ProbeError {
    /// Error parsing SCIP index data
    #[error("SCIP parsing error: {0}")]
    ScipParse(String),

    /// Error with SCIP symbol format
    #[error("Invalid SCIP symbol format: {message}")]
    InvalidSymbol { message: String, symbol: String },

    /// File I/O error
    #[error("File I/O error for {path}: {source}")]
    FileIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Source file parsing error
    #[error("Failed to parse source file {path}: {message}")]
    SourceParse { path: PathBuf, message: String },

    /// Project validation error
    #[error("Project validation error: {0}")]
    ProjectValidation(String),

    /// Duplicate code-names detected (fatal for atomize)
    #[error("Found {count} duplicate code-name(s): {names:?}")]
    DuplicateCodeNames { count: usize, names: Vec<String> },

    /// External tool error
    #[error("External tool '{tool}' error: {message}")]
    ExternalTool { tool: String, message: String },

    /// Verification error
    #[error("Verification error: {0}")]
    Verification(String),
}

impl ProbeError {
    /// Create a file I/O error with path context.
    pub fn file_io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        ProbeError::FileIo {
            path: path.into(),
            source,
        }
    }

    /// Create an invalid symbol error.
    pub fn invalid_symbol(message: impl Into<String>, symbol: impl Into<String>) -> Self {
        ProbeError::InvalidSymbol {
            message: message.into(),
            symbol: symbol.into(),
        }
    }

    /// Create a source parsing error.
    pub fn source_parse(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        ProbeError::SourceParse {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Create an external tool error.
    pub fn external_tool(tool: impl Into<String>, message: impl Into<String>) -> Self {
        ProbeError::ExternalTool {
            tool: tool.into(),
            message: message.into(),
        }
    }
}

/// Result type alias for probe-verus operations.
pub type ProbeResult<T> = Result<T, ProbeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ProbeError::ScipParse("invalid format".to_string());
        assert_eq!(err.to_string(), "SCIP parsing error: invalid format");

        let err = ProbeError::invalid_symbol("missing prefix", "bad_symbol");
        assert!(err.to_string().contains("Invalid SCIP symbol format"));

        let err = ProbeError::ProjectValidation("Cargo.toml not found".to_string());
        assert!(err.to_string().contains("Cargo.toml not found"));
    }

    #[test]
    fn test_error_from_json() {
        let json_err: Result<String, serde_json::Error> =
            serde_json::from_str::<String>("invalid json");
        let probe_err: ProbeError = json_err.unwrap_err().into();
        assert!(matches!(probe_err, ProbeError::Json(_)));
    }
}
