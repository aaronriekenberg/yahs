use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, Response, body::Incoming};

use crate::error::AppError;
use crate::server::state::AppState;

pub mod health;
pub mod proxy;
pub mod static_files;

pub type BoxBody = http_body_util::combinators::BoxBody<Bytes, std::io::Error>;
pub type HandlerResponse = Response<BoxBody>;

/// Core request handler trait.  Each location handler implements this.
#[async_trait]
pub trait Handler: Send + Sync + 'static {
    async fn handle(
        &self,
        req: Request<Incoming>,
        state: &AppState,
    ) -> Result<HandlerResponse, AppError>;
}

/// Build a full `BoxBody` from static bytes.
pub fn full_body(data: impl Into<Bytes>) -> BoxBody {
    use http_body_util::BodyExt;
    Full::new(data.into()).map_err(|e| match e {}).boxed()
}

/// Build an empty `BoxBody`.
pub fn empty_body() -> BoxBody {
    use http_body_util::BodyExt;
    http_body_util::Empty::<Bytes>::new()
        .map_err(|e| match e {})
        .boxed()
}

/// Build a plain-text error response.
pub fn error_response(error: &AppError) -> HandlerResponse {
    let status = error.status_code();
    let body = full_body(format!("{}\n", error));
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(body)
        .unwrap()
}
