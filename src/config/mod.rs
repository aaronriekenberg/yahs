mod loader;
mod types;

pub use loader::load_config;
pub use types::{
    BackendConfig, CacheRule, Config, ErrorFilesConfig, HandlerConfig, LoadBalancingStrategy,
    LocationConfig, PrecompressedEncoding, ReverseProxyConfig, StaticFilesConfig, UpstreamConfig,
};
