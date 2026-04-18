mod access;

pub use access::AccessLogRecord;

use tracing_subscriber::{EnvFilter, filter::filter_fn, fmt, prelude::*};

/// Initialise the global tracing subscriber.
pub fn init_logging(level: &str) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    let app_logs = fmt::layer()
        .json()
        .with_target(false)
        .with_filter(filter_fn(|metadata| metadata.target() != "access"));

    let access_logs = fmt::layer()
        .with_ansi(false)
        .without_time()
        .with_level(false)
        .with_target(false)
        .with_filter(filter_fn(|metadata| metadata.target() == "access"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(app_logs)
        .with(access_logs)
        .init();
}
