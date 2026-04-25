use async_trait::async_trait;
use bytes::Bytes;
use hyper::{Request, Response, StatusCode, body::Incoming};
use serde_json::json;

use crate::error::AppError;
use crate::handler::{Handler, HandlerResponse, full_body};
use crate::server::state::AppState;

pub struct HealthHandler;

#[async_trait]
impl Handler for HealthHandler {
    async fn handle(
        &self,
        _req: Request<Incoming>,
        _state: &AppState,
    ) -> Result<HandlerResponse, AppError> {
        let body_json = json!({
            "status": "ok",
        });
        let body_bytes =
            serde_json::to_vec(&body_json).map_err(|e| AppError::internal(e.to_string()))?;

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .body(full_body(Bytes::from(body_bytes)))
            .unwrap())
    }
}
