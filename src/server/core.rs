use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use hyper::body::Incoming;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::Semaphore;
use tracing::{error, info, warn};

use crate::config::{Config, HandlerConfig, LocationConfig};
use crate::error::AppError;
use crate::handler::health::HealthHandler;
use crate::handler::proxy::ReverseProxyHandler;
use crate::handler::static_files::StaticFilesHandler;
use crate::handler::{BoxBody, Handler, HandlerResponse, error_response};
use crate::middleware::logging::RequestContext;
use crate::server::state::AppState;

/// Build the handler map from config.
fn build_handlers(config: &Config) -> Result<HashMap<String, Arc<dyn Handler>>> {
    let mut handlers: HashMap<String, Arc<dyn Handler>> = HashMap::new();

    // Build a lookup of upstream configs by name.
    let upstream_map: HashMap<&str, _> = config
        .upstreams
        .iter()
        .map(|u| (u.name.as_str(), u))
        .collect();

    for location in &config.locations {
        let handler: Arc<dyn Handler> = match &location.handler {
            HandlerConfig::StaticFiles(sf_config) => {
                let h = StaticFilesHandler::new(sf_config.clone(), location.path.clone())?;
                Arc::new(h)
            }
            HandlerConfig::ReverseProxy(rp_config) => {
                let upstream = upstream_map
                    .get(rp_config.upstream.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Unknown upstream: {}", rp_config.upstream))?;
                let h =
                    ReverseProxyHandler::new(rp_config.clone(), location.path.clone(), upstream);
                Arc::new(h)
            }
            HandlerConfig::Health => Arc::new(HealthHandler),
        };
        handlers.insert(location.path.clone(), handler);
    }

    Ok(handlers)
}

/// Match a request URI path to the best (longest prefix) location.
fn match_location<'a>(
    uri_path: &str,
    locations: &'a [LocationConfig],
) -> Option<&'a LocationConfig> {
    locations
        .iter()
        .filter(|loc| {
            uri_path == loc.path
                || uri_path.starts_with(&format!("{}/", loc.path))
                || loc.path == "/"
        })
        .max_by_key(|loc| loc.path.len())
}

/// Route a single request to the appropriate handler.
async fn route_request(
    mut req: Request<Incoming>,
    state: AppState,
    handlers: Arc<HashMap<String, Arc<dyn Handler>>>,
    locations: Arc<Vec<LocationConfig>>,
    remote_addr: String,
    access_log_enabled: bool,
) -> Response<BoxBody> {
    let ctx = RequestContext::from_request(&req, &remote_addr);

    let uri_path = req.uri().path().to_owned();

    let matched_location = match_location(&uri_path, &locations);

    let (location_path, handler) = match matched_location {
        Some(loc) => {
            let h = handlers.get(&loc.path).cloned();
            (Some(loc.path.clone()), h)
        }
        None => (None, None),
    };

    // Attach the remote addr as a request extension so handlers (e.g. proxy) can read it.
    req.extensions_mut().insert(remote_addr.clone());

    let response = match handler {
        Some(h) => match h.handle(req, &state).await {
            Ok(resp) => resp,
            Err(e) => {
                if !matches!(e, AppError::NotFound) {
                    warn!("Handler error for {}: {}", uri_path, e);
                }
                error_response(&e)
            }
        },
        None => error_response(&AppError::NotFound),
    };

    // Inject extra headers from the matched location.
    let response = inject_extra_headers(response, matched_location, &locations);

    // Inject Server header.
    let response = inject_server_header(response, &state.config.server.server_name);

    if access_log_enabled {
        ctx.log_response(&response, location_path.as_deref(), None);
    }

    response
}

fn inject_extra_headers(
    mut resp: HandlerResponse,
    location: Option<&LocationConfig>,
    _all_locations: &[LocationConfig],
) -> HandlerResponse {
    if let Some(loc) = location {
        for (k, v) in &loc.extra_headers {
            if let (Ok(name), Ok(val)) = (
                hyper::header::HeaderName::from_bytes(k.as_bytes()),
                hyper::header::HeaderValue::from_str(v),
            ) {
                resp.headers_mut().insert(name, val);
            }
        }
    }
    resp
}

fn inject_server_header(mut resp: HandlerResponse, server_name: &str) -> HandlerResponse {
    if let Ok(val) = hyper::header::HeaderValue::from_str(server_name) {
        resp.headers_mut().insert(hyper::header::SERVER, val);
    }
    resp
}

/// Run the HTTP server until a shutdown signal is received.
pub async fn run_server(config: Config) -> Result<()> {
    let bind_addr = config.server.bind.clone();
    let access_log_enabled = config.logging.access_log;
    let max_connections = config.server.max_connections;
    let http_keepalive_timeout = Duration::from_secs(config.server.http_keepalive_timeout_secs);
    let server_tcp_keepalive_enabled = config.server.tcp_keepalive_enabled;
    let server_tcp_keepalive = Duration::from_secs(config.server.tcp_keepalive_secs);

    let state = AppState::new(config.clone());
    let handlers = Arc::new(build_handlers(&config)?);
    let locations = Arc::new(config.locations.clone());
    let connection_limiter = if max_connections > 0 {
        Some(Arc::new(Semaphore::new(max_connections as usize)))
    } else {
        None
    };

    let listener = TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind to {}: {}", bind_addr, e))?;

    info!("yahs listening on {}", bind_addr);

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, peer_addr) = match result {
                    Ok(v) => v,
                    Err(e) => {
                        error!("Accept error: {}", e);
                        continue;
                    }
                };

                let state = state.clone();
                let handlers = handlers.clone();
                let locations = locations.clone();
                let remote_addr = peer_addr.to_string();
                let connection_permit = if let Some(limiter) = &connection_limiter {
                    match limiter.clone().try_acquire_owned() {
                        Ok(permit) => Some(permit),
                        Err(_) => {
                            error!(
                                "Max connections reached ({}), closing incoming connection from {}",
                                max_connections, remote_addr
                            );
                            continue;
                        }
                    }
                } else {
                    None
                };

                if server_tcp_keepalive_enabled
                    && let Err(e) = set_tcp_keepalive(&stream, server_tcp_keepalive)
                {
                    warn!("Failed to set TCP keepalive for {}: {}", remote_addr, e);
                }

                tokio::spawn(async move {
                    let _connection_permit = connection_permit;
                    let io = TokioIo::new(stream);
                    let mut conn_builder = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    );
                    conn_builder
                        .http1()
                        .timer(hyper_util::rt::TokioTimer::new())
                        .keep_alive(http_keepalive_timeout.as_secs() > 0);
                    if http_keepalive_timeout.as_secs() > 0 {
                        conn_builder
                            .http2()
                            .timer(hyper_util::rt::TokioTimer::new())
                            .keep_alive_interval(Some(http_keepalive_timeout))
                            .keep_alive_timeout(http_keepalive_timeout);
                    } else {
                        conn_builder
                            .http2()
                            .timer(hyper_util::rt::TokioTimer::new())
                            .keep_alive_interval(None);
                    }

                    let service = hyper::service::service_fn(move |req| {
                        let state = state.clone();
                        let handlers = handlers.clone();
                        let locations = locations.clone();
                        let remote_addr = remote_addr.clone();
                        async move {
                            let resp = route_request(
                                req,
                                state,
                                handlers,
                                locations,
                                remote_addr,
                                access_log_enabled,
                            )
                            .await;
                            Ok::<_, std::convert::Infallible>(resp)
                        }
                    });

                    if let Err(e) = conn_builder.serve_connection(io, service).await
                    {
                        // Ignore normal connection close errors.
                        if !e.to_string().contains("connection closed") {
                            warn!("Connection error: {}", e);
                        }
                    }
                });
            }

            _ = shutdown_signal() => {
                info!("Shutdown signal received, stopping server.");
                break;
            }
        }
    }

    Ok(())
}

fn set_tcp_keepalive(stream: &tokio::net::TcpStream, duration: Duration) -> std::io::Result<()> {
    let keepalive = socket2::TcpKeepalive::new().with_time(duration);
    socket2::SockRef::from(stream).set_tcp_keepalive(&keepalive)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
