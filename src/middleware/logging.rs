use std::time::Instant;

use chrono::Utc;
use hyper::{Request, Response, body::Incoming};

use crate::handler::BoxBody;
use crate::logging::AccessLogRecord;
use crate::middleware::RequestId;

/// Context collected before request dispatch, used to build the access log.
pub struct RequestContext {
    pub request_id: RequestId,
    pub start: Instant,
    pub remote_addr: String,
    pub method: String,
    pub uri: String,
    pub http_version: String,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
}

impl RequestContext {
    pub fn from_request(req: &Request<Incoming>, remote_addr: &str) -> Self {
        let request_id = RequestId::new();
        let start = Instant::now();

        let method = req.method().to_string();
        let uri = req.uri().to_string();
        let http_version = format!("{:?}", req.version());

        let user_agent = req
            .headers()
            .get(hyper::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());

        let referer = req
            .headers()
            .get(hyper::header::REFERER)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());

        Self {
            request_id,
            start,
            remote_addr: remote_addr.to_owned(),
            method,
            uri,
            http_version,
            user_agent,
            referer,
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
            request_id: self.request_id.to_string(),
            remote_addr: self.remote_addr.clone(),
            method: self.method.clone(),
            uri: self.uri.clone(),
            http_version: self.http_version.clone(),
            status: response.status().as_u16(),
            response_bytes,
            duration_ms: AccessLogRecord::duration_ms_from(duration),
            upstream: upstream.map(|s| s.to_owned()),
            location: location.map(|s| s.to_owned()),
            user_agent: self.user_agent.clone(),
            referer: self.referer.clone(),
        };

        record.emit();
    }
}
