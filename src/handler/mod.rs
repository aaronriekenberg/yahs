use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, Response, StatusCode, body::Incoming};

use crate::error::AppError;
use crate::server::error_files::ErrorFileEntry;
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

/// Build a streaming `BoxBody` from any async reader (e.g. `tokio::fs::File`
/// or `tokio::io::Take<tokio::fs::File>`).  Data is read and forwarded in
/// chunks, so the full file contents never need to reside in memory at once.
pub fn stream_body<R>(reader: R) -> BoxBody
where
    R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
{
    use futures::StreamExt as _;
    use hyper::body::Frame;
    http_body_util::BodyExt::boxed(http_body_util::StreamBody::new(
        tokio_util::io::ReaderStream::new(reader).map(|r| r.map(Frame::data)),
    ))
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

/// Build an HTML error-file response, preserving the original status code.
pub fn error_file_response(status: StatusCode, entry: &ErrorFileEntry) -> HandlerResponse {
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(hyper::header::CACHE_CONTROL, entry.cache_control.as_str())
        .body(full_body(entry.body.clone()))
        .unwrap()
}
