use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use http_body_util::BodyExt;
use hyper::{
    Request, Response, Uri,
    body::Incoming,
    header::{HeaderName, HeaderValue},
};
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use hyper_util::rt::TokioExecutor;
use tracing::debug;

use crate::config::{BackendConfig, LoadBalancingStrategy, ReverseProxyConfig, UpstreamConfig};
use crate::error::AppError;
use crate::handler::{BoxBody, Handler, HandlerResponse};
use crate::server::state::AppState;

/// Headers that must not be forwarded as-is.
static HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
];

/// Reverse proxy handler.
pub struct ReverseProxyHandler {
    config: ReverseProxyConfig,
    location_prefix: String,
    upstream: Arc<UpstreamRuntime>,
}

impl ReverseProxyHandler {
    pub fn new(
        config: ReverseProxyConfig,
        location_prefix: String,
        upstream_config: &UpstreamConfig,
    ) -> Self {
        let upstream = Arc::new(UpstreamRuntime::new(upstream_config));

        Self {
            config,
            location_prefix,
            upstream,
        }
    }
}

#[async_trait]
impl Handler for ReverseProxyHandler {
    async fn handle(
        &self,
        req: Request<Incoming>,
        _state: &AppState,
    ) -> Result<HandlerResponse, AppError> {
        let backend_url = self.upstream.next_backend();
        debug!("Proxying {} {} → {}", req.method(), req.uri(), backend_url);

        // Build the forwarded URI.
        let forwarded_path = if self.config.strip_prefix {
            let stripped = req
                .uri()
                .path()
                .strip_prefix(&self.location_prefix)
                .unwrap_or(req.uri().path());
            if stripped.is_empty() { "/" } else { stripped }
        } else {
            req.uri().path()
        };

        let query = req
            .uri()
            .query()
            .map(|q| format!("?{}", q))
            .unwrap_or_default();
        let target_uri: Uri = format!("{}{}{}", backend_url, forwarded_path, query)
            .parse()
            .map_err(|e: hyper::http::uri::InvalidUri| AppError::upstream(e.to_string()))?;

        // Build forwarded request.
        let (parts, body) = req.into_parts();

        let mut builder = Request::builder().method(parts.method).uri(target_uri);

        // Copy headers, removing hop-by-hop and applying removes/overrides.
        for (name, value) in &parts.headers {
            let name_lower = name.as_str().to_lowercase();
            if HOP_BY_HOP.contains(&name_lower.as_str()) {
                continue;
            }
            if self
                .config
                .remove_request_headers
                .iter()
                .any(|r| r.to_lowercase() == name_lower)
            {
                continue;
            }
            builder = builder.header(name, value);
        }

        // Inject X-Forwarded-For using the actual client IP from the request extension.
        let client_ip = parts
            .extensions
            .get::<String>()
            .map(|s| s.split(':').next().unwrap_or(s).to_owned())
            .unwrap_or_else(|| "unknown".to_owned());
        builder = builder.header("x-forwarded-for", client_ip);

        // Extra headers from config.
        for (k, v) in &self.config.extra_request_headers {
            if let (Ok(name), Ok(val)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                builder = builder.header(name, val);
            }
        }

        let forward_req = builder
            .body(body.map_err(std::io::Error::other).boxed())
            .map_err(|e| AppError::upstream(e.to_string()))?;

        let request_timeout = Duration::from_millis(self.upstream.request_timeout_ms);

        let response =
            tokio::time::timeout(request_timeout, self.upstream.client.request(forward_req))
                .await
                .map_err(|_| AppError::upstream("Request to upstream timed out"))?
                .map_err(|e| AppError::upstream(e.to_string()))?;

        // Convert the upstream response.
        let (resp_parts, resp_body) = response.into_parts();

        let mut resp_builder = Response::builder().status(resp_parts.status);

        for (name, value) in &resp_parts.headers {
            let name_lower = name.as_str().to_lowercase();
            if HOP_BY_HOP.contains(&name_lower.as_str()) {
                continue;
            }
            resp_builder = resp_builder.header(name, value);
        }
        let cache_control = if self.config.cache_max_age_secs == 0 {
            "no-store".to_string()
        } else {
            format!("public, max-age={}", self.config.cache_max_age_secs)
        };
        resp_builder = resp_builder.header(hyper::header::CACHE_CONTROL, cache_control);

        // Stream the upstream body directly to the client without buffering it
        // in memory.  Map the hyper body error to std::io::Error to satisfy
        // BoxBody's error type.
        let streaming_body = resp_body.map_err(std::io::Error::other).boxed();

        let final_resp = resp_builder
            .body(streaming_body)
            .map_err(|e| AppError::upstream(e.to_string()))?;

        Ok(final_resp)
    }
}

// ── upstream load balancing ───────────────────────────────────────────────

struct UpstreamRuntime {
    backends: Vec<String>,
    strategy: LoadBalancingStrategy,
    counter: AtomicUsize,
    client: Client<HttpConnector, BoxBody>,
    request_timeout_ms: u64,
}

impl UpstreamRuntime {
    fn new(config: &UpstreamConfig) -> Self {
        let backends: Vec<String> = config
            .backends
            .iter()
            .flat_map(|b: &BackendConfig| {
                std::iter::repeat_n(b.url.trim_end_matches('/').to_owned(), b.weight as usize)
            })
            .collect();

        let mut connector = HttpConnector::new();
        connector.set_nodelay(config.tcp_nodelay);
        if config.tcp_keepalive_enabled {
            let ka = Duration::from_secs(config.tcp_keepalive_secs);
            connector.set_keepalive(Some(ka));
            connector.set_keepalive_interval(Some(ka));
            connector.set_keepalive_retries(Some(9));
        } else {
            connector.set_keepalive(None);
        }

        let mut client_builder = Client::builder(TokioExecutor::new());
        client_builder
            .pool_max_idle_per_host(config.max_idle_connections_per_host)
            .pool_idle_timeout(Duration::from_secs(config.max_idle_connection_timeout_secs))
            .timer(hyper_util::rt::TokioTimer::new());
        if config.http2_prior_knowledge {
            client_builder.http2_only(true);
        }
        if let Some(interval_secs) = config.http2_keepalive_interval_secs {
            let interval = Duration::from_secs(interval_secs);
            let timeout = Duration::from_secs(config.http2_keepalive_timeout_secs);
            client_builder.http2_keep_alive_interval(Some(interval));
            client_builder.http2_keep_alive_timeout(timeout);
            client_builder.http2_keep_alive_while_idle(true);
        }
        let client = client_builder.build(connector);

        Self {
            backends,
            strategy: config.strategy,
            counter: AtomicUsize::new(0),
            client,
            request_timeout_ms: config.request_timeout_ms,
        }
    }

    fn next_backend(&self) -> String {
        use LoadBalancingStrategy::*;
        match self.strategy {
            RoundRobin => {
                let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.backends.len();
                self.backends[idx].clone()
            }
            Random => {
                let idx = (next_counter()) % self.backends.len();
                self.backends[idx].clone()
            }
            IpHash => {
                // Fallback to round-robin when no IP is available at this layer.
                let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.backends.len();
                self.backends[idx].clone()
            }
        }
    }
}

/// Monotonically increasing counter used for pseudo-random backend selection.
fn next_counter() -> usize {
    static COUNTER: AtomicUsize = AtomicUsize::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}
