use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level server configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// Global server settings.
    pub server: ServerConfig,

    /// Root directory used as the base for static file serving and error file
    /// paths.  Defaults to `"."` (the working directory).
    #[serde(default = "default_root")]
    pub root: String,

    /// Named locations mapped to handlers.
    pub locations: Vec<LocationConfig>,

    /// Named upstream clusters for reverse proxy.
    #[serde(default)]
    pub upstreams: Vec<UpstreamConfig>,

    /// Logging configuration.
    #[serde(default)]
    pub logging: LoggingConfig,

    /// Optional custom error file configuration.
    #[serde(default)]
    pub error_files: Option<ErrorFilesConfig>,
}

/// Configuration for optional custom error-page files.
///
/// When a request returns a 4xx or 5xx response the server can substitute
/// the body of a pre-configured HTML file in place of the default plain-text
/// error message.  Both entries are fully optional; if omitted the default
/// error response is returned unchanged.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ErrorFilesConfig {
    /// Path to the file served for 4xx (client error) responses.
    pub client_error_file: Option<String>,

    /// Path to the file served for 5xx (server error) responses.
    pub server_error_file: Option<String>,

    /// `Cache-Control: max-age` in seconds for the client-error file.
    /// Defaults to `0` (i.e. `no-store`).
    #[serde(default)]
    pub client_error_cache_max_age_secs: u64,

    /// `Cache-Control: max-age` in seconds for the server-error file.
    /// Defaults to `0` (i.e. `no-store`).
    #[serde(default)]
    pub server_error_cache_max_age_secs: u64,
}

/// Core server socket and limits configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    /// Bind address, e.g. "0.0.0.0:8080"
    pub bind: String,

    /// Optional server name emitted in the `Server` response header.
    #[serde(default = "default_server_name")]
    pub server_name: String,

    /// Maximum number of concurrent connections (0 = unlimited).
    #[serde(default)]
    pub max_connections: u32,

    /// HTTP keepalive timeout in seconds.
    #[serde(default = "default_keepalive_timeout")]
    pub http_keepalive_timeout_secs: u64,

    /// Disable Nagle's algorithm on accepted client connections (recommended
    /// for low-latency HTTP/1.1 and HTTP/2 workloads).
    #[serde(default = "default_true")]
    pub tcp_nodelay: bool,

    /// Enable TCP keepalive probes on accepted client connections.
    #[serde(default = "default_true")]
    pub tcp_keepalive_enabled: bool,

    /// TCP keepalive probe idle time in seconds for accepted client connections.
    #[serde(default = "default_tcp_keepalive_secs")]
    pub tcp_keepalive_secs: u64,

    /// Optional TLS configuration.
    pub tls: Option<TlsConfig>,
}

fn default_server_name() -> String {
    "yahs".to_string()
}

fn default_root() -> String {
    ".".to_string()
}

fn default_keepalive_timeout() -> u64 {
    20
}

fn default_tcp_keepalive_secs() -> u64 {
    15
}

/// TLS configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
}

/// A single URL-prefix location block.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocationConfig {
    /// URL path prefix this location matches, e.g. "/static"
    pub path: String,

    /// Handler type and settings.
    pub handler: HandlerConfig,

    /// Extra response headers injected for this location.
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
}

/// Handler variant for a location.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HandlerConfig {
    /// Serve static files from a root directory.
    StaticFiles(StaticFilesConfig),

    /// Reverse-proxy to an upstream cluster.
    ReverseProxy(ReverseProxyConfig),

    /// Built-in health/readiness endpoint.
    Health,
}

/// A single cache rule: files whose path matches `pattern` (glob) get
/// `max_age_secs` as their `Cache-Control: max-age`.  Rules are evaluated in
/// order; the first match wins.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CacheRule {
    /// Glob pattern matched against the request path relative to the location
    /// prefix, e.g. `"images/*.png"` or `"**/*.js"`.
    pub pattern: String,

    /// Cache-Control max-age in seconds for matching files.
    pub max_age_secs: u64,
}

/// Static file server settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StaticFilesConfig {
    /// Index file names to try in order when a directory is requested.
    #[serde(default = "default_index_files")]
    pub index: Vec<String>,

    /// Strip the location prefix before joining with root.
    #[serde(default = "default_true")]
    pub strip_prefix: bool,

    /// Cache-Control max-age in seconds for served files (0 = no caching).
    /// Used as the fallback when no `cache_rules` entry matches.
    #[serde(default = "default_cache_max_age")]
    pub cache_max_age_secs: u64,

    /// Ordered list of per-path cache rules (glob pattern → max-age).
    /// The first matching rule wins; unmatched files use `cache_max_age_secs`.
    #[serde(default)]
    pub cache_rules: Vec<CacheRule>,

    /// Glob patterns for paths that should always return 404.
    /// Matched against the request path relative to the location prefix.
    /// Example: `[".git", "**/.env"]`
    #[serde(default)]
    pub blocked_paths: Vec<String>,

    /// Attempt to serve precompressed variants before the raw file.
    #[serde(default = "default_true")]
    pub precompressed: bool,

    /// List of encodings to try in preference order.
    #[serde(default = "default_encodings")]
    pub encodings: Vec<PrecompressedEncoding>,
}

fn default_index_files() -> Vec<String> {
    vec!["index.html".to_string()]
}

fn default_true() -> bool {
    true
}

fn default_cache_max_age() -> u64 {
    3600
}

fn default_encodings() -> Vec<PrecompressedEncoding> {
    vec![
        PrecompressedEncoding::Zstd,
        PrecompressedEncoding::Brotli,
        PrecompressedEncoding::Gzip,
    ]
}

/// Compression encoding variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PrecompressedEncoding {
    Zstd,
    Brotli,
    Gzip,
}

impl PrecompressedEncoding {
    /// File extension appended to locate precompressed files.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Zstd => ".zst",
            Self::Brotli => ".br",
            Self::Gzip => ".gz",
        }
    }

    /// Value for the `Content-Encoding` response header.
    pub fn content_encoding(self) -> &'static str {
        match self {
            Self::Zstd => "zstd",
            Self::Brotli => "br",
            Self::Gzip => "gzip",
        }
    }

    /// Token used in the client `Accept-Encoding` header.
    #[allow(dead_code)]
    pub fn accept_encoding_token(self) -> &'static str {
        match self {
            Self::Zstd => "zstd",
            Self::Brotli => "br",
            Self::Gzip => "gzip",
        }
    }
}

/// Reverse proxy handler settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReverseProxyConfig {
    /// Name of the upstream cluster to proxy to (references `upstreams`).
    pub upstream: String,

    /// Strip the location prefix when forwarding the request.
    #[serde(default = "default_true")]
    pub strip_prefix: bool,

    /// Cache-Control max-age in seconds for proxied responses (0 = `no-store`).
    #[serde(default = "default_proxy_cache_max_age")]
    pub cache_max_age_secs: u64,

    /// Extra request headers to forward (or override).
    #[serde(default, alias = "extra_headers")]
    pub extra_request_headers: HashMap<String, String>,

    /// Request headers to remove before forwarding.
    #[serde(default, alias = "remove_headers")]
    pub remove_request_headers: Vec<String>,
}

fn default_proxy_cache_max_age() -> u64 {
    0
}

/// An upstream cluster with one or more backends.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpstreamConfig {
    /// Unique name referenced by proxy locations.
    pub name: String,

    /// Load balancing strategy.
    #[serde(default)]
    pub strategy: LoadBalancingStrategy,

    /// Backend server list.
    pub backends: Vec<BackendConfig>,

    /// Request timeout in milliseconds for proxy requests to this upstream (0 = no timeout).
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,

    /// Maximum number of idle pooled connections per upstream host.
    #[serde(
        default = "default_proxy_max_connection_pool_size",
        alias = "max_connection_pool_size"
    )]
    pub max_idle_connections_per_host: usize,

    /// Use HTTP/2 cleartext (h2c / prior knowledge) for upstream connections.
    /// All upstream backends must support HTTP/2 without TLS negotiation.
    #[serde(default)]
    pub http2_prior_knowledge: bool,

    /// Disable Nagle's algorithm on upstream connections (recommended for
    /// low-latency proxy workloads).
    #[serde(default = "default_true")]
    pub tcp_nodelay: bool,

    /// Enable TCP keepalive probes for upstream connections.
    #[serde(default = "default_true")]
    pub tcp_keepalive_enabled: bool,

    /// TCP keepalive probe idle time in seconds for upstream connections.
    #[serde(default = "default_tcp_keepalive_secs")]
    pub tcp_keepalive_secs: u64,

    /// HTTP/2 keepalive PING interval in seconds for upstream connections
    /// (0 = disabled).  Only meaningful when `http2_prior_knowledge` is true
    /// or the upstream negotiates HTTP/2 via ALPN.
    #[serde(default, alias = "http2_keepalive_interval_secs")]
    pub http_keepalive_timeout_secs: u64,
}

fn default_request_timeout_ms() -> u64 {
    30000
}

fn default_proxy_max_connection_pool_size() -> usize {
    32
}

/// Supported load balancing strategies.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadBalancingStrategy {
    /// Round-robin (default).
    #[default]
    RoundRobin,
    /// Hash by client IP.
    IpHash,
    /// Random selection.
    Random,
}

/// A single backend server.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BackendConfig {
    /// Full URL, e.g. "http://127.0.0.1:3000"
    pub url: String,

    /// Relative weight for weighted round-robin (default 1).
    #[serde(default = "default_weight")]
    pub weight: u32,
}

fn default_weight() -> u32 {
    1
}

/// Logging configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    /// Logging level filter: trace, debug, info, warn, error.
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Emit structured JSON access logs for every request.
    #[serde(default = "default_true")]
    pub access_log: bool,

    /// Include request/response bodies in debug logs.
    #[serde(default)]
    pub log_bodies: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            access_log: true,
            log_bodies: false,
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}
