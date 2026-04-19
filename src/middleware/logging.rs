use std::collections::BTreeMap;
use std::time::Instant;

use chrono::Utc;
use hyper::{
    HeaderMap, Request, Response,
    body::Incoming,
    header::{HeaderName, HeaderValue},
};

use crate::handler::BoxBody;
use crate::logging::AccessLogRecord;
use crate::middleware::RequestId;

/// Context collected before request dispatch, used to build the access log.
pub struct RequestContext {
    pub request_id: RequestId,
    pub connection_id: u64,
    pub start: Instant,
    pub remote_addr: String,
    pub method: String,
    pub uri: String,
    pub http_version: String,
    pub request_headers: BTreeMap<String, Vec<String>>,
    pub request_bytes: u64,
}

impl RequestContext {
    pub fn from_request(req: &Request<Incoming>, remote_addr: &str, connection_id: u64) -> Self {
        let request_id = RequestId::new();
        let start = Instant::now();

        let method = req.method().to_string();
        let uri = req.uri().to_string();
        let http_version = format!("{:?}", req.version());
        let request_headers = collect_headers(req.headers());
        let request_bytes = req
            .headers()
            .get(hyper::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        Self {
            request_id,
            connection_id,
            start,
            remote_addr: remote_addr.to_owned(),
            method,
            uri,
            http_version,
            request_headers,
            request_bytes,
        }
    }

    /// Emit a structured access log entry after the response is ready.
    pub fn log_response(
        &self,
        response: &Response<BoxBody>,
        location: Option<&str>,
        upstream: Option<&str>,
    ) {
        let duration = self.start.elapsed();

        let response_bytes = response
            .headers()
            .get(hyper::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let record = AccessLogRecord {
            timestamp: Utc::now().to_rfc3339(),
            request_id: self.request_id.0,
            connection_id: self.connection_id,
            remote_addr: self.remote_addr.clone(),
            method: self.method.clone(),
            uri: self.uri.clone(),
            http_version: self.http_version.clone(),
            status: response.status().as_u16(),
            request_bytes: self.request_bytes,
            response_bytes,
            duration_ns: AccessLogRecord::duration_ns_from(duration),
            request_headers: self.request_headers.clone(),
            response_headers: collect_headers(response.headers()),
            upstream: upstream.map(|s| s.to_owned()),
            location: location.map(|s| s.to_owned()),
        };

        record.emit();
    }
}

fn collect_headers(headers: &HeaderMap<HeaderValue>) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for (name, value) in headers {
        let key = header_name_to_string(name);
        let val = value
            .to_str()
            .map(|s| s.to_owned())
            .unwrap_or_else(|_| String::from_utf8_lossy(value.as_bytes()).to_string());
        out.entry(key).or_insert_with(Vec::new).push(val);
    }
    out
}

fn header_name_to_string(name: &HeaderName) -> String {
    name.as_str().to_owned()
}
