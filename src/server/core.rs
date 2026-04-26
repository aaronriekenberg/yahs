use std::collections::HashMap;
use std::io::BufReader;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Result;
use hyper::body::Incoming;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::Semaphore;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info, warn};

use crate::config::{Config, HandlerConfig, LocationConfig, TlsConfig};
use crate::error::AppError;
use crate::handler::health::HealthHandler;
use crate::handler::proxy::ReverseProxyHandler;
use crate::handler::static_files::StaticFilesHandler;
use crate::handler::{BoxBody, Handler, HandlerResponse, error_file_response, error_response};
use crate::middleware::logging::RequestContext;
use crate::server::error_files::ErrorFileStore;
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
                let h = StaticFilesHandler::new(
                    sf_config.clone(),
                    location.path.clone(),
                    &config.root,
                )?;
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

/// Replace a 4xx/5xx response with an error-file body when one is configured.
fn maybe_replace_with_error_file(resp: HandlerResponse, store: &ErrorFileStore) -> HandlerResponse {
    let status = resp.status();
    if status.is_client_error() {
        if let Some(entry) = &store.client_error {
            return error_file_response(status, entry);
        }
    } else if status.is_server_error()
        && let Some(entry) = &store.server_error
    {
        return error_file_response(status, entry);
    }
    resp
}

/// Route a single request to the appropriate handler.
async fn route_request(
    mut req: Request<Incoming>,
    state: AppState,
    handlers: Arc<HashMap<String, Arc<dyn Handler>>>,
    locations: Arc<Vec<LocationConfig>>,
    remote_addr: String,
    connection_id: u64,
    access_log_enabled: bool,
) -> Response<BoxBody> {
    let ctx = RequestContext::from_request(&req, &remote_addr, connection_id);

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

    // Substitute custom error-file body for 4xx/5xx responses when configured.
    let response = maybe_replace_with_error_file(response, &state.error_files);

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

// ── TLS helpers ─────────────────────────────────────────────────────────────

/// Build a `TlsAcceptor` from the supplied `TlsConfig`.
fn build_tls_acceptor(tls: &TlsConfig) -> Result<TlsAcceptor> {
    use tokio_rustls::rustls::ServerConfig as RustlsServerConfig;
    use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

    // Load certificate chain.
    let cert_file = std::fs::File::open(&tls.cert_path)
        .map_err(|e| anyhow::anyhow!("Failed to open cert file '{}': {}", tls.cert_path, e))?;
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("Failed to parse certificates: {}", e))?;
    if certs.is_empty() {
        anyhow::bail!("No certificates found in '{}'", tls.cert_path);
    }

    // Load private key (RSA or PKCS#8 or EC).
    let key_file = std::fs::File::open(&tls.key_path)
        .map_err(|e| anyhow::anyhow!("Failed to open key file '{}': {}", tls.key_path, e))?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))
        .map_err(|e| anyhow::anyhow!("Failed to parse private key: {}", e))?
        .ok_or_else(|| anyhow::anyhow!("No private key found in '{}'", tls.key_path))?;
    let key = PrivateKeyDer::from(key);

    // Build the rustls server config.
    let mut rustls_config = RustlsServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| anyhow::anyhow!("TLS configuration error: {}", e))?;

    // Advertise protocols via ALPN.
    if tls.http2 {
        rustls_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    } else {
        rustls_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    }

    Ok(TlsAcceptor::from(Arc::new(rustls_config)))
}

// ── Shared connection parameters ─────────────────────────────────────────────

#[derive(Clone)]
struct ConnParams {
    http1_keepalive_enabled: bool,
    http2_keepalive_interval: Option<Duration>,
    http2_keepalive_timeout: Duration,
    tcp_keepalive_enabled: bool,
    tcp_keepalive: Duration,
    tcp_nodelay: bool,
    max_connections: u32,
    access_log_enabled: bool,
}

// ── Per-listener accept loops ────────────────────────────────────────────────

/// Accept loop for plain HTTP connections.
async fn accept_http(
    listener: TcpListener,
    state: AppState,
    handlers: Arc<HashMap<String, Arc<dyn Handler>>>,
    locations: Arc<Vec<LocationConfig>>,
    params: ConnParams,
    connection_limiter: Option<Arc<Semaphore>>,
) {
    static HTTP_COUNTER: AtomicU64 = AtomicU64::new(0);

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, peer_addr) = match result {
                    Ok(v) => v,
                    Err(e) => {
                        error!("HTTP accept error: {}", e);
                        continue;
                    }
                };

                let state = state.clone();
                let handlers = handlers.clone();
                let locations = locations.clone();
                let params = params.clone();
                let remote_addr = peer_addr.to_string();
                let connection_id = HTTP_COUNTER.fetch_add(1, Ordering::Relaxed);
                let connection_permit = if let Some(limiter) = &connection_limiter {
                    match limiter.clone().try_acquire_owned() {
                        Ok(permit) => Some(permit),
                        Err(_) => {
                            error!(
                                "Max connections reached ({}), closing incoming connection from {}",
                                params.max_connections, remote_addr
                            );
                            continue;
                        }
                    }
                } else {
                    None
                };

                if params.tcp_keepalive_enabled
                    && let Err(e) = set_tcp_keepalive(&stream, params.tcp_keepalive)
                {
                    warn!("Failed to set TCP keepalive for {}: {}", remote_addr, e);
                }
                if params.tcp_nodelay
                    && let Err(e) = stream.set_nodelay(true)
                {
                    warn!("Failed to set TCP_NODELAY for {}: {}", remote_addr, e);
                }

                tokio::spawn(async move {
                    let _connection_permit = connection_permit;
                    let io = TokioIo::new(stream);
                    serve_conn(io, state, handlers, locations, remote_addr, connection_id, &params).await;
                });
            }
            _ = shutdown_signal() => { break; }
        }
    }
}

/// Accept loop for HTTPS (TLS) connections.
async fn accept_https(
    listener: TcpListener,
    tls_acceptor: TlsAcceptor,
    state: AppState,
    handlers: Arc<HashMap<String, Arc<dyn Handler>>>,
    locations: Arc<Vec<LocationConfig>>,
    params: ConnParams,
    connection_limiter: Option<Arc<Semaphore>>,
) {
    static HTTPS_COUNTER: AtomicU64 = AtomicU64::new(0);

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, peer_addr) = match result {
                    Ok(v) => v,
                    Err(e) => {
                        error!("HTTPS accept error: {}", e);
                        continue;
                    }
                };

                let state = state.clone();
                let handlers = handlers.clone();
                let locations = locations.clone();
                let params = params.clone();
                let tls_acceptor = tls_acceptor.clone();
                let remote_addr = peer_addr.to_string();
                let connection_id = HTTPS_COUNTER.fetch_add(1, Ordering::Relaxed);
                let connection_permit = if let Some(limiter) = &connection_limiter {
                    match limiter.clone().try_acquire_owned() {
                        Ok(permit) => Some(permit),
                        Err(_) => {
                            error!(
                                "Max connections reached ({}), closing incoming connection from {}",
                                params.max_connections, remote_addr
                            );
                            continue;
                        }
                    }
                } else {
                    None
                };

                if params.tcp_keepalive_enabled
                    && let Err(e) = set_tcp_keepalive(&stream, params.tcp_keepalive)
                {
                    warn!("Failed to set TCP keepalive for {}: {}", remote_addr, e);
                }
                if params.tcp_nodelay
                    && let Err(e) = stream.set_nodelay(true)
                {
                    warn!("Failed to set TCP_NODELAY for {}: {}", remote_addr, e);
                }

                tokio::spawn(async move {
                    let _connection_permit = connection_permit;
                    let tls_stream = match tls_acceptor.accept(stream).await {
                        Ok(s) => s,
                        Err(e) => {
                            debug!("TLS handshake failed for {}: {}", remote_addr, e);
                            return;
                        }
                    };
                    let io = TokioIo::new(tls_stream);
                    serve_conn(io, state, handlers, locations, remote_addr, connection_id, &params).await;
                });
            }
            _ = shutdown_signal() => { break; }
        }
    }
}

/// Drive a single accepted connection through hyper's auto HTTP/1.1+2 builder.
async fn serve_conn<I>(
    io: TokioIo<I>,
    state: AppState,
    handlers: Arc<HashMap<String, Arc<dyn Handler>>>,
    locations: Arc<Vec<LocationConfig>>,
    remote_addr: String,
    connection_id: u64,
    params: &ConnParams,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let access_log_enabled = params.access_log_enabled;
    let http1_keepalive_enabled = params.http1_keepalive_enabled;
    let http2_keepalive_interval = params.http2_keepalive_interval;
    let http2_keepalive_timeout = params.http2_keepalive_timeout;

    let mut conn_builder =
        hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());
    conn_builder
        .http1()
        .timer(hyper_util::rt::TokioTimer::new())
        .keep_alive(http1_keepalive_enabled);
    conn_builder
        .http2()
        .timer(hyper_util::rt::TokioTimer::new())
        .keep_alive_interval(http2_keepalive_interval)
        .keep_alive_timeout(http2_keepalive_timeout);

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
                connection_id,
                access_log_enabled,
            )
            .await;
            Ok::<_, std::convert::Infallible>(resp)
        }
    });

    if let Err(e) = conn_builder.serve_connection(io, service).await {
        let is_timeout = e
            .downcast_ref::<hyper::Error>()
            .is_some_and(|he| he.is_timeout());
        if is_timeout {
            debug!("Connection {}: header read timeout", connection_id);
        } else {
            warn!("Connection {} error: {}", connection_id, e);
        }
    }
}

// ── Public entry point ───────────────────────────────────────────────────────

/// Run the HTTP (and optionally HTTPS) server until a shutdown signal is received.
pub async fn run_server(config: Config) -> Result<()> {
    let params = ConnParams {
        http1_keepalive_enabled: config.server.http1_keepalive_enabled,
        http2_keepalive_interval: config
            .server
            .http2_keepalive_interval_secs
            .map(Duration::from_secs),
        http2_keepalive_timeout: Duration::from_secs(config.server.http2_keepalive_timeout_secs),
        tcp_keepalive_enabled: config.server.tcp_keepalive_enabled,
        tcp_keepalive: Duration::from_secs(config.server.tcp_keepalive_secs),
        tcp_nodelay: config.server.tcp_nodelay,
        max_connections: config.server.max_connections,
        access_log_enabled: config.logging.access_log,
    };

    let error_files =
        ErrorFileStore::from_config(config.error_files.as_ref(), &config.root).await?;
    let state = AppState::new(config.clone(), error_files);
    let handlers = Arc::new(build_handlers(&config)?);
    let locations = Arc::new(config.locations.clone());

    let connection_limiter: Option<Arc<Semaphore>> = if params.max_connections > 0 {
        Some(Arc::new(Semaphore::new(params.max_connections as usize)))
    } else {
        None
    };

    // Bind the plain-HTTP listener.
    let http_addr = config.server.bind.clone();
    let http_listener = TcpListener::bind(&http_addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind HTTP listener on {}: {}", http_addr, e))?;
    info!("yahs listening on {} (HTTP)", http_addr);

    // Optionally bind the HTTPS listener.
    let tls_task: tokio::task::JoinHandle<()> = match &config.server.tls {
        Some(tls_config) => {
            let bind_tls = config.server.bind_tls.clone().ok_or_else(|| {
                anyhow::anyhow!("`server.bind_tls` must be set when `[server.tls]` is configured")
            })?;
            let tls_acceptor = build_tls_acceptor(tls_config)?;
            let https_listener = TcpListener::bind(&bind_tls).await.map_err(|e| {
                anyhow::anyhow!("Failed to bind HTTPS listener on {}: {}", bind_tls, e)
            })?;
            info!("yahs listening on {} (HTTPS)", bind_tls);

            let state = state.clone();
            let handlers = handlers.clone();
            let locations = locations.clone();
            let params = params.clone();
            let limiter = connection_limiter.clone();
            tokio::spawn(accept_https(
                https_listener,
                tls_acceptor,
                state,
                handlers,
                locations,
                params,
                limiter,
            ))
        }
        None => tokio::spawn(std::future::ready(())),
    };

    let http_task = tokio::spawn(accept_http(
        http_listener,
        state,
        handlers,
        locations,
        params,
        connection_limiter,
    ));

    // Wait for both tasks; either will stop on the shutdown signal.
    let _ = tokio::join!(http_task, tls_task);

    info!("Shutdown signal received, stopping server.");
    Ok(())
}

fn set_tcp_keepalive(stream: &tokio::net::TcpStream, idle: Duration) -> std::io::Result<()> {
    let keepalive = socket2::TcpKeepalive::new()
        .with_time(idle)
        .with_interval(idle)
        .with_retries(9);
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
