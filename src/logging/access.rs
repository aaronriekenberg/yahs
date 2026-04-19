use serde::Serialize;
use std::{collections::BTreeMap, time::Duration};
use tracing::{info, warn};

/// A single structured access log entry.
#[derive(Debug, Serialize)]
pub struct AccessLogRecord {
    /// ISO-8601 timestamp when the request was received.
    pub timestamp: String,

    /// Monotonically incrementing request ID (starts at 0 per process).
    pub request_id: u64,

    /// Monotonically incrementing connection ID (starts at 0 per process).
    pub connection_id: u64,

    /// Client IP address.
    pub remote_addr: String,

    /// HTTP method.
    pub method: String,

    /// Full request URI.
    pub uri: String,

    /// HTTP version.
    pub http_version: String,

    /// HTTP status code of the response.
    pub status: u16,

    /// Request body size in bytes (from `Content-Length`, 0 if absent).
    pub request_bytes: u64,

    /// Response body size in bytes (from `Content-Length`, 0 if absent).
    pub response_bytes: u64,

    /// Total request processing time in nanoseconds.
    pub duration_ns: u128,

    /// All request headers.
    pub request_headers: BTreeMap<String, Vec<String>>,

    /// All response headers.
    pub response_headers: BTreeMap<String, Vec<String>>,

    /// Name of the upstream used (None for static files or health).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,

    /// Matched location path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

impl AccessLogRecord {
    /// Convert this record into a single JSON line.
    pub fn to_json_line(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Emit this record as a structured JSON log line.
    pub fn emit(&self) {
        match self.to_json_line() {
            Ok(json) => info!(target: "access", "{}", json),
            Err(e) => warn!("Failed to serialize access log record: {}", e),
        }
    }

    /// Format a `Duration` in nanoseconds.
    pub fn duration_ns_from(duration: Duration) -> u128 {
        duration.as_nanos()
    }
}

#[cfg(test)]
mod tests {
    use super::AccessLogRecord;
    use std::collections::BTreeMap;

    #[test]
    fn test_to_json_line_contains_structured_fields() {
        let record = AccessLogRecord {
            timestamp: "2026-04-18T17:13:43.366967+00:00".to_string(),
            request_id: 42,
            connection_id: 7,
            remote_addr: "127.0.0.1:50003".to_string(),
            method: "GET".to_string(),
            uri: "/".to_string(),
            http_version: "HTTP/1.1".to_string(),
            status: 200,
            request_bytes: 0,
            response_bytes: 12,
            duration_ns: 734_459,
            request_headers: BTreeMap::from([("accept".to_string(), vec!["*/*".to_string()])]),
            response_headers: BTreeMap::from([(
                "content-type".to_string(),
                vec!["text/html".to_string()],
            )]),
            upstream: None,
            location: Some("/".to_string()),
        };

        let line = record.to_json_line().expect("serialization should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&line).expect("output should be valid JSON");

        assert_eq!(parsed["request_id"], 42);
        assert_eq!(parsed["connection_id"], 7);
        assert_eq!(parsed["request_bytes"], 0);
        assert_eq!(parsed["status"], 200);
        assert_eq!(parsed["request_headers"]["accept"][0], "*/*");
        assert_eq!(parsed["response_headers"]["content-type"][0], "text/html");
        assert_eq!(parsed["location"], "/");
    }
}
