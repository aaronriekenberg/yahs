use std::sync::Arc;

use crate::config::Config;
use crate::server::error_files::ErrorFileStore;

/// Shared, read-only application state threaded through all handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub error_files: Arc<ErrorFileStore>,
}

impl AppState {
    pub fn new(config: Config, error_files: ErrorFileStore) -> Self {
        Self {
            config: Arc::new(config),
            error_files: Arc::new(error_files),
        }
    }
}
