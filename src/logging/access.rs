use serde::Serialize;
use std::{collections::BTreeMap, time::Duration};
use tracing::info;

/// A single structured access log entry.
#[derive(Debug, Serialize)]
pub struct AccessLogRecord {
    /// ISO-8601 timestamp when the request was received.
    pub timestamp: String,

    /// Unique request ID.
    pub request_id: String,

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

    /// Response body size in bytes.
    pub response_bytes: u64,

    /// Total request processing time in milliseconds.
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

    /// Value of the `User-Agent` request header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,

    /// Value of the `Referer` request header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,
}

impl AccessLogRecord {
    /// Emit this record as a structured JSON log line.
    pub fn emit(&self) {
        match serde_json::to_string(self) {
            Ok(json) => info!(target: "access", "{}", json),
            Err(e) => tracing::warn!("Failed to serialize access log record: {}", e),
        }
    }

    /// Format a `Duration` in nanoseconds.
    pub fn duration_ns_from(duration: Duration) -> u128 {
        duration.as_nanos()
    }
}
