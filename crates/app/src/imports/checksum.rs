//! Checksum value object for SHA-256 hex strings.

use serde::{Deserialize, Serialize};

use super::ImportError;

/// A validated SHA-256 checksum represented as a 64-character lowercase hex string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Checksum(String);

impl Checksum {
    /// Create a new Checksum from a hex string.
    ///
    /// Validates that the input is exactly 64 lowercase hexadecimal characters.
    pub fn new(value: impl Into<String>) -> Result<Self, ImportError> {
        let value = value.into();
        let value = value.to_lowercase();

        if value.len() != 64 {
            return Err(ImportError::ValidationFailed {
                message: format!(
                    "checksum must be exactly 64 hex characters, got {}",
                    value.len()
                ),
            });
        }

        if !value.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ImportError::ValidationFailed {
                message: "checksum must contain only hexadecimal characters".to_string(),
            });
        }

        Ok(Self(value))
    }

    /// Returns the hex string value of the checksum.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Checksum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_sha256_checksum() {
        let hex = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        let checksum = Checksum::new(hex).unwrap();
        assert_eq!(checksum.as_str(), hex);
    }

    #[test]
    fn valid_uppercase_is_lowercased() {
        let hex = "ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789";
        let checksum = Checksum::new(hex).unwrap();
        assert_eq!(checksum.as_str(), hex.to_lowercase());
    }

    #[test]
    fn rejects_too_short() {
        let result = Checksum::new("abcdef");
        assert!(result.is_err());
        if let Err(ImportError::ValidationFailed { message }) = result {
            assert!(message.contains("64 hex characters"));
        } else {
            panic!("Expected ValidationFailed");
        }
    }

    #[test]
    fn rejects_too_long() {
        let hex = "a".repeat(65);
        let result = Checksum::new(hex);
        assert!(result.is_err());
        if let Err(ImportError::ValidationFailed { message }) = result {
            assert!(message.contains("64 hex characters"));
        } else {
            panic!("Expected ValidationFailed");
        }
    }

    #[test]
    fn rejects_empty_string() {
        let result = Checksum::new("");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_non_hex_characters() {
        let hex = "ghijklmnopqrstuv0123456789abcdef0123456789abcdef0123456789abcdef";
        let result = Checksum::new(hex);
        assert!(result.is_err());
        if let Err(ImportError::ValidationFailed { message }) = result {
            assert!(message.contains("hexadecimal"));
        } else {
            panic!("Expected ValidationFailed");
        }
    }

    #[test]
    fn rejects_special_characters() {
        let hex = "!@#$%^&*()_+{}[]0123456789abcdef0123456789abcdef0123456789abcdef";
        let result = Checksum::new(hex);
        assert!(result.is_err());
    }

    #[test]
    fn display_shows_hex_value() {
        let hex = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        let checksum = Checksum::new(hex).unwrap();
        assert_eq!(checksum.to_string(), hex);
    }
}
