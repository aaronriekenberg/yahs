use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level server configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// Global server settings.
    pub server: ServerConfig,

    /// Named locations mapped to handlers.
    pub locations: Vec<LocationConfig>,

    /// Named upstream clusters for reverse proxy.
    #[serde(default)]
    pub upstreams: Vec<UpstreamConfig>,

    /// Logging configuration.
    #[serde(default)]
    pub logging: LoggingConfig,
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

    /// Keepalive timeout in seconds.
    #[serde(default = "default_keepalive_timeout")]
    pub keepalive_timeout_secs: u64,

    /// Enable TCP keepalive probes on accepted client connections.
    #[serde(default)]
    pub tcp_keepalive_enabled: bool,

    /// TCP keepalive probe idle time in seconds for accepted client connections.
    #[serde(default = "default_keepalive_timeout")]
    pub tcp_keepalive_secs: u64,

    /// Optional TLS configuration.
    pub tls: Option<TlsConfig>,
}

fn default_server_name() -> String {
    "yahs".to_string()
}

fn default_keepalive_timeout() -> u64 {
    75
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

/// Static file server settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StaticFilesConfig {
    /// Filesystem root directory.
    pub root: String,

    /// Index file names to try in order when a directory is requested.
    #[serde(default = "default_index_files")]
    pub index: Vec<String>,

    /// Strip the location prefix before joining with root.
    #[serde(default = "default_true")]
    pub strip_prefix: bool,

    /// Cache-Control max-age in seconds for served files (0 = no caching).
    #[serde(default = "default_cache_max_age")]
    pub cache_max_age_secs: u64,

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

    /// Connect timeout in milliseconds.
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,

    /// Request timeout in milliseconds (0 = no timeout).
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,

    /// Maximum number of idle pooled connections per upstream host.
    #[serde(default = "default_proxy_max_connection_pool_size")]
    pub max_connection_pool_size: usize,

    /// Enable TCP keepalive probes for upstream connections.
    #[serde(default)]
    pub tcp_keepalive_enabled: bool,

    /// TCP keepalive probe idle time in seconds for upstream connections.
    #[serde(default = "default_keepalive_timeout")]
    pub tcp_keepalive_secs: u64,

    /// Extra request headers to forward (or override).
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,

    /// Request headers to remove before forwarding.
    #[serde(default)]
    pub remove_headers: Vec<String>,
}

fn default_connect_timeout_ms() -> u64 {
    5000
}

fn default_request_timeout_ms() -> u64 {
    30000
}

fn default_proxy_max_connection_pool_size() -> usize {
    32
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
