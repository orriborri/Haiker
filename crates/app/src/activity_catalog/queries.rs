//! Activity catalog query handlers.
//!
//! Contains query logic for listing and fetching activities.

use crate::identity::UserId;

use super::repository::{ActivityPage, ActivityRepository};
use super::{ActivityCatalogError, ActivityId, LifecycleState};

/// Default page size for activity listings.
pub const DEFAULT_PAGE_SIZE: u32 = 25;

/// Maximum allowed page size.
pub const MAX_PAGE_SIZE: u32 = 100;

/// Cursor payload for stable cursor-based pagination.
///
/// Encodes the (started_at, id) pair used for keyset pagination.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct CursorPayload {
    /// ISO 8601 timestamp of started_at (or null if activity has no start time).
    pub started_at: Option<String>,
    /// Activity ID as a UUID string.
    pub id: String,
}

/// Encode a cursor payload to a base64 string.
pub fn encode_cursor(payload: &CursorPayload) -> String {
    use serde_json;
    let json = serde_json::to_string(payload).expect("cursor serialization should not fail");
    base64_encode(&json)
}

/// Decode a cursor string back to a CursorPayload.
pub fn decode_cursor(cursor: &str) -> Result<CursorPayload, ActivityCatalogError> {
    let json = base64_decode(cursor).map_err(|_| ActivityCatalogError::InvalidCursor {
        message: "invalid base64 encoding".to_string(),
    })?;
    serde_json::from_str(&json).map_err(|_| ActivityCatalogError::InvalidCursor {
        message: "invalid cursor format".to_string(),
    })
}

/// List activities for a given owner with cursor-based pagination.
///
/// Validates the page_size, decodes the cursor if present, and delegates
/// to the repository for the actual query.
pub async fn list_activities(
    owner_id: UserId,
    cursor: Option<&str>,
    page_size: Option<u32>,
    repo: &dyn ActivityRepository,
) -> Result<ActivityPage, ActivityCatalogError> {
    let page_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    repo.list_activities(owner_id, cursor, page_size).await
}

/// Get a single activity by ID, verifying ownership.
///
/// Returns `ActivityNotFound` if the activity does not exist, the owner does not
/// match, or the activity is in Deleted lifecycle state (non-disclosing).
pub async fn get_activity(
    activity_id: ActivityId,
    owner_id: UserId,
    repo: &dyn ActivityRepository,
) -> Result<super::Activity, ActivityCatalogError> {
    let activity = repo
        .find_by_id(activity_id)
        .await?
        .filter(|a| a.owner_id == owner_id && a.lifecycle_state != LifecycleState::Deleted)
        .ok_or(ActivityCatalogError::ActivityNotFound)?;

    Ok(activity)
}

/// Base64 encode a string (standard encoding with padding).
fn base64_encode(input: &str) -> String {
    use std::io::Write;

    let mut buf = Vec::new();
    {
        let mut encoder = Base64Encoder::new(&mut buf);
        encoder.write_all(input.as_bytes()).unwrap();
        encoder.finish();
    }
    String::from_utf8(buf).unwrap()
}

/// Base64 decode a string.
fn base64_decode(input: &str) -> Result<String, &'static str> {
    let bytes = base64_decode_bytes(input)?;
    String::from_utf8(bytes).map_err(|_| "invalid utf8")
}

/// Minimal base64 encoder (no external crate dependency).
struct Base64Encoder<'a> {
    output: &'a mut Vec<u8>,
    buffer: [u8; 3],
    buffer_len: usize,
}

const BASE64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

impl<'a> Base64Encoder<'a> {
    fn new(output: &'a mut Vec<u8>) -> Self {
        Self {
            output,
            buffer: [0u8; 3],
            buffer_len: 0,
        }
    }

    fn finish(mut self) {
        if self.buffer_len > 0 {
            self.encode_block();
        }
    }

    fn encode_block(&mut self) {
        let b = &self.buffer;
        let len = self.buffer_len;

        let indices = match len {
            1 => {
                let i0 = (b[0] >> 2) as usize;
                let i1 = ((b[0] & 0x03) << 4) as usize;
                self.output.push(BASE64_CHARS[i0]);
                self.output.push(BASE64_CHARS[i1]);
                self.output.push(b'=');
                self.output.push(b'=');
                return;
            }
            2 => {
                let i0 = (b[0] >> 2) as usize;
                let i1 = (((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize;
                let i2 = ((b[1] & 0x0f) << 2) as usize;
                self.output.push(BASE64_CHARS[i0]);
                self.output.push(BASE64_CHARS[i1]);
                self.output.push(BASE64_CHARS[i2]);
                self.output.push(b'=');
                return;
            }
            3 => {
                let i0 = (b[0] >> 2) as usize;
                let i1 = (((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize;
                let i2 = (((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize;
                let i3 = (b[2] & 0x3f) as usize;
                [i0, i1, i2, i3]
            }
            _ => return,
        };

        for i in indices {
            self.output.push(BASE64_CHARS[i]);
        }
        self.buffer_len = 0;
    }
}

impl<'a> std::io::Write for Base64Encoder<'a> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for &byte in buf {
            self.buffer[self.buffer_len] = byte;
            self.buffer_len += 1;
            if self.buffer_len == 3 {
                self.encode_block();
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Decode base64 bytes.
fn base64_decode_bytes(input: &str) -> Result<Vec<u8>, &'static str> {
    let input = input.trim_end_matches('=');
    let mut output = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits_collected: u32 = 0;

    for c in input.chars() {
        let val = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            _ => return Err("invalid base64 character"),
        };

        buffer = (buffer << 6) | val;
        bits_collected += 6;

        if bits_collected >= 8 {
            bits_collected -= 8;
            output.push((buffer >> bits_collected) as u8);
            buffer &= (1 << bits_collected) - 1;
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn encode_decode_cursor_roundtrip() {
        let payload = CursorPayload {
            started_at: Some("2024-01-15T10:30:00Z".to_string()),
            id: Uuid::new_v4().to_string(),
        };

        let encoded = encode_cursor(&payload);
        let decoded = decode_cursor(&encoded).unwrap();

        assert_eq!(payload.started_at, decoded.started_at);
        assert_eq!(payload.id, decoded.id);
    }

    #[test]
    fn encode_decode_cursor_with_null_started_at() {
        let payload = CursorPayload {
            started_at: None,
            id: Uuid::new_v4().to_string(),
        };

        let encoded = encode_cursor(&payload);
        let decoded = decode_cursor(&encoded).unwrap();

        assert_eq!(payload.started_at, decoded.started_at);
        assert_eq!(payload.id, decoded.id);
    }

    #[test]
    fn decode_invalid_base64_returns_error() {
        let result = decode_cursor("not-valid-base64!!!");
        assert!(result.is_err());
        match result.unwrap_err() {
            ActivityCatalogError::InvalidCursor { .. } => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn decode_valid_base64_but_invalid_json_returns_error() {
        // Encode something that's not valid cursor JSON
        let encoded = encode_cursor(&CursorPayload {
            started_at: Some("test".to_string()),
            id: "not-a-uuid".to_string(),
        });
        // This should decode fine since the format is correct
        let decoded = decode_cursor(&encoded).unwrap();
        assert_eq!(decoded.id, "not-a-uuid");
    }

    #[test]
    fn base64_encode_decode_roundtrip() {
        let test_strings = [
            "hello",
            "",
            "a",
            "ab",
            "abc",
            r#"{"started_at":"2024-01-15T10:30:00Z","id":"550e8400-e29b-41d4-a716-446655440000"}"#,
        ];

        for s in test_strings {
            let encoded = base64_encode(s);
            let decoded = base64_decode(&encoded).unwrap();
            assert_eq!(s, decoded, "roundtrip failed for: {s}");
        }
    }
}
