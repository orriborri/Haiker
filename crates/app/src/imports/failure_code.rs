//! Domain failure codes for imports.
//!
//! Provides machine-readable, sanitized failure codes that can be safely
//! exposed in API responses without leaking internal implementation details.

use std::fmt;

/// Machine-readable failure code indicating why an import failed.
///
/// These codes are safe to expose externally and provide clients with
/// actionable information about the failure category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailureCode {
    /// The GPX file could not be parsed (malformed XML, missing required elements).
    ParseError,
    /// The file checksum did not match the expected value.
    ChecksumMismatch,
    /// The uploaded file exceeds the maximum allowed size.
    FileTooLarge,
    /// The file format is not valid or not supported.
    InvalidFormat,
    /// Object storage was unavailable during processing.
    StorageUnavailable,
    /// The import processing exceeded the allowed time limit.
    Timeout,
    /// An internal error occurred that does not fit other categories.
    InternalError,
}

impl FailureCode {
    /// Parse a failure code from its UPPER_SNAKE_CASE string representation.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "PARSE_ERROR" => Some(Self::ParseError),
            "CHECKSUM_MISMATCH" => Some(Self::ChecksumMismatch),
            "FILE_TOO_LARGE" => Some(Self::FileTooLarge),
            "INVALID_FORMAT" => Some(Self::InvalidFormat),
            "STORAGE_UNAVAILABLE" => Some(Self::StorageUnavailable),
            "TIMEOUT" => Some(Self::Timeout),
            "INTERNAL_ERROR" => Some(Self::InternalError),
            _ => None,
        }
    }

    /// Returns the UPPER_SNAKE_CASE string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ParseError => "PARSE_ERROR",
            Self::ChecksumMismatch => "CHECKSUM_MISMATCH",
            Self::FileTooLarge => "FILE_TOO_LARGE",
            Self::InvalidFormat => "INVALID_FORMAT",
            Self::StorageUnavailable => "STORAGE_UNAVAILABLE",
            Self::Timeout => "TIMEOUT",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }
}

impl fmt::Display for FailureCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_produces_upper_snake_case() {
        assert_eq!(FailureCode::ParseError.to_string(), "PARSE_ERROR");
        assert_eq!(
            FailureCode::ChecksumMismatch.to_string(),
            "CHECKSUM_MISMATCH"
        );
        assert_eq!(FailureCode::FileTooLarge.to_string(), "FILE_TOO_LARGE");
        assert_eq!(FailureCode::InvalidFormat.to_string(), "INVALID_FORMAT");
        assert_eq!(
            FailureCode::StorageUnavailable.to_string(),
            "STORAGE_UNAVAILABLE"
        );
        assert_eq!(FailureCode::Timeout.to_string(), "TIMEOUT");
        assert_eq!(FailureCode::InternalError.to_string(), "INTERNAL_ERROR");
    }

    #[test]
    fn parse_parses_all_variants() {
        assert_eq!(
            FailureCode::parse("PARSE_ERROR"),
            Some(FailureCode::ParseError)
        );
        assert_eq!(
            FailureCode::parse("CHECKSUM_MISMATCH"),
            Some(FailureCode::ChecksumMismatch)
        );
        assert_eq!(
            FailureCode::parse("FILE_TOO_LARGE"),
            Some(FailureCode::FileTooLarge)
        );
        assert_eq!(
            FailureCode::parse("INVALID_FORMAT"),
            Some(FailureCode::InvalidFormat)
        );
        assert_eq!(
            FailureCode::parse("STORAGE_UNAVAILABLE"),
            Some(FailureCode::StorageUnavailable)
        );
        assert_eq!(FailureCode::parse("TIMEOUT"), Some(FailureCode::Timeout));
        assert_eq!(
            FailureCode::parse("INTERNAL_ERROR"),
            Some(FailureCode::InternalError)
        );
    }

    #[test]
    fn parse_returns_none_for_unknown() {
        assert_eq!(FailureCode::parse("UNKNOWN_CODE"), None);
        assert_eq!(FailureCode::parse(""), None);
        assert_eq!(FailureCode::parse("parse_error"), None);
    }

    #[test]
    fn as_str_roundtrips_with_parse() {
        let codes = [
            FailureCode::ParseError,
            FailureCode::ChecksumMismatch,
            FailureCode::FileTooLarge,
            FailureCode::InvalidFormat,
            FailureCode::StorageUnavailable,
            FailureCode::Timeout,
            FailureCode::InternalError,
        ];

        for code in codes {
            let s = code.as_str();
            let parsed = FailureCode::parse(s).unwrap();
            assert_eq!(parsed, code);
        }
    }
}
