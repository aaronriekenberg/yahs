//! Integration tests verifying that the reverse proxy correctly forwards the
//! incoming `Host` header (HTTP/1.1) / `:authority` pseudo-header (HTTP/2) to
//! the upstream backend for all four protocol combinations:
//!
//! * HTTP/1.1 client → HTTP/1.1 upstream
//! * HTTP/1.1 client → HTTP/2 upstream  (prior-knowledge h2c)
//! * HTTP/2 client   → HTTP/1.1 upstream
//! * HTTP/2 client   → HTTP/2 upstream  (prior-knowledge h2c)
//!
//! Each test starts a lightweight echo-backend that returns the Host/:authority
//! value it received in the request, then starts a yahs proxy pointing at that
//! backend, makes a request through the proxy with a synthetic host header, and
//! asserts that the backend saw the original host rather than the backend's own
//! address.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use yahs::config::{BackendConfig, ReverseProxyConfig, ServerConfig, UpstreamConfig};
use yahs::handler::proxy::ReverseProxyHandler;
use yahs::handler::{Handler, error_response};
use yahs::server::error_files::ErrorFileStore;
use yahs::server::state::AppState;

// ── Echo backend helpers ──────────────────────────────────────────────────────

/// Start a simple HTTP/1.1 or HTTP/2 echo backend that responds with the
/// value of the request's `host` header / `:authority` pseudo-header.
///
/// Returns `(bound_addr, task_handle)`.
async fn start_echo_backend(http2: bool) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let io = TokioIo::new(stream);

            tokio::spawn(async move {
                let svc = hyper::service::service_fn(echo_host_handler);
                if http2 {
                    let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                        .serve_connection(io, svc)
                        .await;
                } else {
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await;
                }
            });
        }
    });

    (addr, handle)
}

/// Service function: reply with the request's host/:authority as the body.
async fn echo_host_handler(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    // For HTTP/2, :authority is exposed via req.uri().authority().
    // For HTTP/1.1, the Host header is in req.headers().
    let host = req
        .uri()
        .authority()
        .map(|a| a.as_str().to_owned())
        .or_else(|| {
            req.headers()
                .get(hyper::header::HOST)
                .and_then(|v| v.to_str().ok())
                .map(String::from)
        })
        .unwrap_or_default();

    Ok(Response::new(Full::new(Bytes::from(host))))
}

// ── Proxy server helper ───────────────────────────────────────────────────────

/// Build a minimal `AppState` for testing (no error files, minimal config).
fn make_app_state() -> AppState {
    use yahs::config::Config;
    let config = Config {
        server: ServerConfig {
            bind: "127.0.0.1:0".to_string(),
            server_name: "yahs-test".to_string(),
            max_connections: 0,
            http1_keepalive_enabled: true,
            http2_keepalive_interval_secs: None,
            http2_keepalive_timeout_secs: 20,
            tcp_nodelay: true,
            tcp_keepalive_enabled: false,
            tcp_keepalive_secs: 15,
            tls: None,
        },
        root: ".".to_string(),
        locations: vec![],
        upstreams: vec![],
        logging: Default::default(),
        error_files: None,
    };
    let store = ErrorFileStore {
        client_error: None,
        server_error: None,
    };
    AppState::new(config, store)
}

/// Start a yahs proxy server backed by `backend_addr` using HTTP/2
/// prior-knowledge when `http2_upstream` is true.
///
/// Returns `(proxy_addr, task_handle)`.
async fn start_proxy(
    backend_addr: SocketAddr,
    http2_upstream: bool,
) -> (SocketAddr, JoinHandle<()>) {
    let upstream_config = UpstreamConfig {
        name: "test".to_string(),
        strategy: Default::default(),
        backends: vec![BackendConfig {
            url: format!("http://{}", backend_addr),
            weight: 1,
        }],
        request_timeout_ms: 5_000,
        max_idle_connections_per_host: 4,
        max_idle_connection_timeout_secs: 10,
        http2_prior_knowledge: http2_upstream,
        tcp_nodelay: true,
        tcp_keepalive_enabled: false,
        tcp_keepalive_secs: 15,
        http2_keepalive_interval_secs: None,
        http2_keepalive_timeout_secs: 20,
    };

    let proxy_config = ReverseProxyConfig {
        upstream: "test".to_string(),
        strip_prefix: false,
        cache_max_age_secs: 0,
        extra_request_headers: Default::default(),
        remove_request_headers: vec![],
    };

    let handler = Arc::new(ReverseProxyHandler::new(
        proxy_config,
        "/".to_string(),
        &upstream_config,
    )) as Arc<dyn Handler>;

    let state = make_app_state();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        loop {
            let Ok((stream, peer_addr)) = listener.accept().await else {
                break;
            };
            let io = TokioIo::new(stream);
            let handler = handler.clone();
            let state = state.clone();
            let remote = peer_addr.to_string();

            tokio::spawn(async move {
                let svc = hyper::service::service_fn(move |mut req: Request<Incoming>| {
                    let handler = handler.clone();
                    let state = state.clone();
                    let remote = remote.clone();
                    async move {
                        req.extensions_mut().insert(remote);
                        let resp = match handler.handle(req, &state).await {
                            Ok(r) => r,
                            Err(e) => error_response(&e),
                        };
                        Ok::<_, Infallible>(resp)
                    }
                });

                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(io, svc)
                    .await;
            });
        }
    });

    (proxy_addr, handle)
}

// ── HTTP client helpers ───────────────────────────────────────────────────────

/// Make a single HTTP/1.1 GET request to `proxy_addr` with a synthetic
/// `Host` header and return the response body as a String.
async fn http1_request(proxy_addr: SocketAddr, host: &str) -> String {
    let stream = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();
    let io = TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(conn);

    let req = Request::builder()
        .method("GET")
        .uri("/")
        .header(hyper::header::HOST, host)
        .body(Full::<Bytes>::new(Bytes::new()))
        .unwrap();

    let resp = sender.send_request(req).await.unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(body.to_vec()).unwrap()
}

/// Make a single HTTP/2 (h2c / prior-knowledge) GET request to `proxy_addr`
/// with a synthetic `:authority` and return the response body as a String.
async fn http2_request(proxy_addr: SocketAddr, host: &str) -> String {
    let stream = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();
    let io = TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http2::handshake(TokioExecutor::new(), io)
        .await
        .unwrap();
    tokio::spawn(conn);

    // Build a fully-qualified URI so that hyper sets :authority from it.
    let uri = format!("http://{}/", host);
    let req = Request::builder()
        .method("GET")
        .uri(&uri)
        .body(Full::<Bytes>::new(Bytes::new()))
        .unwrap();

    let resp = sender.send_request(req).await.unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(body.to_vec()).unwrap()
}

// ── Test cases ────────────────────────────────────────────────────────────────

/// Helper that abstracts the common test body.
///
/// * `http2_client`   – whether the incoming request uses HTTP/2
/// * `http2_upstream` – whether the proxy-to-backend connection uses HTTP/2
async fn assert_host_forwarded(http2_client: bool, http2_upstream: bool) {
    // The synthetic host the client sends.  It deliberately differs from the
    // backend address so we can tell whether it was actually forwarded.
    let original_host = "myservice.example.com";

    let (backend_addr, _backend) = start_echo_backend(http2_upstream).await;
    let (proxy_addr, _proxy) = start_proxy(backend_addr, http2_upstream).await;

    // Wait until the proxy is accepting connections (poll for up to 1 second).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        if tokio::net::TcpStream::connect(proxy_addr).await.is_ok() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!("proxy at {} did not become ready within 1s", proxy_addr);
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    let received_host = if http2_client {
        http2_request(proxy_addr, original_host).await
    } else {
        http1_request(proxy_addr, original_host).await
    };

    assert_eq!(
        received_host, original_host,
        "Expected backend to receive host '{}', but got '{}'. \
         (http2_client={}, http2_upstream={})",
        original_host, received_host, http2_client, http2_upstream
    );
}

#[tokio::test]
async fn test_host_forwarded_h1_client_h1_upstream() {
    assert_host_forwarded(false, false).await;
}

#[tokio::test]
async fn test_host_forwarded_h1_client_h2_upstream() {
    assert_host_forwarded(false, true).await;
}

#[tokio::test]
async fn test_host_forwarded_h2_client_h1_upstream() {
    assert_host_forwarded(true, false).await;
}

#[tokio::test]
async fn test_host_forwarded_h2_client_h2_upstream() {
    assert_host_forwarded(true, true).await;
}
