use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
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

/// A connector that establishes TCP connections to a fixed backend address
/// regardless of the URI authority in the outgoing request.
///
/// This enables the HTTP client to send the original incoming `Host` /
/// HTTP-2 `:authority` in the request URI (so hyper derives the correct
/// `Host`/`:authority` header) while still physically connecting to the
/// configured upstream backend address.
#[derive(Clone)]
struct BackendOverrideConnector {
    /// URI whose authority is used for the actual TCP connection.
    backend_uri: Uri,
    inner: HttpConnector,
}

impl tower::Service<Uri> for BackendOverrideConnector {
    type Response = <HttpConnector as tower::Service<Uri>>::Response;
    type Error = <HttpConnector as tower::Service<Uri>>::Error;
    type Future = <HttpConnector as tower::Service<Uri>>::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, _uri: Uri) -> Self::Future {
        // Always route the TCP connection to the configured backend,
        // irrespective of whatever authority the request URI carries.
        self.inner.call(self.backend_uri.clone())
    }
}

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
        let client_ip_ext = req
            .extensions()
            .get::<String>()
            .map(|s| s.split(':').next().unwrap_or(s).to_owned())
            .unwrap_or_else(|| "unknown".to_owned());
        let backend = self.upstream.next_backend(&client_ip_ext);
        debug!(
            "Proxying {} {} → {}://{}",
            req.method(),
            req.uri(),
            backend.scheme,
            backend.authority
        );

        // Build the forwarded URI path.
        let forwarded_path = {
            let path = if self.config.strip_prefix {
                let stripped = req
                    .uri()
                    .path()
                    .strip_prefix(&self.location_prefix)
                    .unwrap_or(req.uri().path());
                if stripped.is_empty() { "/" } else { stripped }
            } else {
                req.uri().path()
            };
            path.to_owned()
        };

        let query = req
            .uri()
            .query()
            .map(|q| format!("?{}", q))
            .unwrap_or_default();

        // Build forwarded request.
        let (parts, body) = req.into_parts();

        // Use the incoming Host header (or :authority for HTTP/2) as the URI
        // authority so that hyper derives the correct Host (HTTP/1.1) or
        // :authority (HTTP/2) header for the upstream request.
        // Fall back to the backend's own address when the incoming request
        // carries no host information.
        let original_host = parts
            .uri
            .authority()
            .map(|a| a.as_str().to_owned())
            .or_else(|| {
                parts
                    .headers
                    .get(hyper::header::HOST)
                    .and_then(|v| v.to_str().ok())
                    .map(String::from)
            })
            .unwrap_or_else(|| backend.authority.clone());

        let target_uri: Uri = format!(
            "{}://{}{}{}",
            backend.scheme, original_host, forwarded_path, query
        )
        .parse()
        .map_err(|e: hyper::http::uri::InvalidUri| AppError::upstream(e.to_string()))?;

        let mut builder = Request::builder().method(parts.method).uri(target_uri);

        // Copy headers, removing hop-by-hop and applying removes/overrides.
        // Skip `host` explicitly: hyper derives the Host header (HTTP/1.1) or
        // :authority pseudo-header (HTTP/2) from the request URI set above,
        // which already carries the original incoming host value.
        for (name, value) in &parts.headers {
            let name_lower = name.as_str().to_lowercase();
            if HOP_BY_HOP.contains(&name_lower.as_str()) {
                continue;
            }
            if name_lower == "host" {
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
        builder = builder.header("x-forwarded-for", &client_ip_ext);

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

        let response = tokio::time::timeout(request_timeout, backend.client.request(forward_req))
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

/// A single backend with its own HTTP client that always physically connects
/// to this backend's address via `BackendOverrideConnector`.
struct BackendRuntime {
    /// URI scheme of the backend (e.g. "http").
    scheme: String,
    /// URI authority of the backend (e.g. "127.0.0.1:3000"), used as fallback
    /// when the incoming request carries no host information.
    authority: String,
    /// HTTP client bound to this backend.
    client: Client<BackendOverrideConnector, BoxBody>,
}

struct UpstreamRuntime {
    /// One entry per distinct backend; clients are never duplicated.
    backends: Vec<BackendRuntime>,
    /// Weighted index table: each backend index appears `weight` times so that
    /// round-robin / hash selection naturally honours configured weights without
    /// creating additional HTTP clients.
    weighted_index: Vec<usize>,
    strategy: LoadBalancingStrategy,
    counter: AtomicUsize,
    request_timeout_ms: u64,
}

impl UpstreamRuntime {
    fn new(config: &UpstreamConfig) -> Self {
        // Build exactly one BackendRuntime per configured backend.
        let backends: Vec<BackendRuntime> = config
            .backends
            .iter()
            .map(|b: &BackendConfig| {
                let url = b.url.trim_end_matches('/').to_owned();
                let backend_uri: Uri = url
                    .parse()
                    .unwrap_or_else(|e| panic!("invalid backend URL '{}': {}", url, e));
                let scheme = backend_uri.scheme_str().unwrap_or("http").to_owned();
                let authority = backend_uri
                    .authority()
                    .map(|a| a.as_str().to_owned())
                    .unwrap_or_default();

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

                let override_connector = BackendOverrideConnector {
                    backend_uri,
                    inner: connector,
                };

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
                let client = client_builder.build(override_connector);

                BackendRuntime {
                    scheme,
                    authority,
                    client,
                }
            })
            .collect();

        // Build the weighted index table: backend index i appears weight[i] times.
        let weighted_index: Vec<usize> = config
            .backends
            .iter()
            .enumerate()
            .flat_map(|(i, b)| std::iter::repeat_n(i, b.weight as usize))
            .collect();

        Self {
            backends,
            weighted_index,
            strategy: config.strategy,
            counter: AtomicUsize::new(0),
            request_timeout_ms: config.request_timeout_ms,
        }
    }

    fn next_backend(&self, client_ip: &str) -> &BackendRuntime {
        use LoadBalancingStrategy::*;
        let slot = match self.strategy {
            RoundRobin => self.counter.fetch_add(1, Ordering::Relaxed) % self.weighted_index.len(),
            Random => next_counter() % self.weighted_index.len(),
            IpHash => {
                // FNV-1a hash of the client IP for stable backend affinity.
                let hash = ip_fnv1a(client_ip);
                hash % self.weighted_index.len()
            }
        };
        &self.backends[self.weighted_index[slot]]
    }
}

/// Monotonically increasing counter used for pseudo-random backend selection.
fn next_counter() -> usize {
    static COUNTER: AtomicUsize = AtomicUsize::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// FNV-1a hash of a string, returned as a `usize` for index arithmetic.
fn ip_fnv1a(s: &str) -> usize {
    const FNV_OFFSET: usize = 14695981039346656037_u64 as usize;
    const FNV_PRIME: usize = 1099511628211;
    s.bytes().fold(FNV_OFFSET, |acc, b| {
        (acc ^ b as usize).wrapping_mul(FNV_PRIME)
    })
}
