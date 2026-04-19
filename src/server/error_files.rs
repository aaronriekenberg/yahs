use std::path::Path;

use anyhow::Result;
use bytes::Bytes;

use crate::config::ErrorFilesConfig;

/// A single pre-loaded error file with its computed `Cache-Control` header value.
pub struct ErrorFileEntry {
    pub body: Bytes,
    pub cache_control: String,
}

/// Holds pre-loaded bodies for the optional 4xx and 5xx error files.
///
/// Both entries are `None` when the corresponding file is not configured.
pub struct ErrorFileStore {
    /// Body served for 4xx (client error) responses.
    pub client_error: Option<ErrorFileEntry>,
    /// Body served for 5xx (server error) responses.
    pub server_error: Option<ErrorFileEntry>,
}

impl ErrorFileStore {
    /// Build an `ErrorFileStore` from the optional config section.
    ///
    /// `root` is the top-level `root` directory; relative paths in the config
    /// are resolved against it.
    ///
    /// Returns an error if a configured file cannot be read, so that the
    /// server fails fast at startup rather than silently serving the default
    /// error page.
    pub async fn from_config(config: Option<&ErrorFilesConfig>, root: &str) -> Result<Self> {
        let Some(cfg) = config else {
            return Ok(Self {
                client_error: None,
                server_error: None,
            });
        };

        let client_error = match &cfg.client_error_file {
            Some(path) => {
                let full_path = resolve_path(root, path);
                let body = tokio::fs::read(&full_path).await.map_err(|e| {
                    anyhow::anyhow!("Failed to read client_error_file '{}': {}", full_path.display(), e)
                })?;
                Some(ErrorFileEntry {
                    body: Bytes::from(body),
                    cache_control: cache_control_value(cfg.client_error_cache_max_age_secs),
                })
            }
            None => None,
        };

        let server_error = match &cfg.server_error_file {
            Some(path) => {
                let full_path = resolve_path(root, path);
                let body = tokio::fs::read(&full_path).await.map_err(|e| {
                    anyhow::anyhow!("Failed to read server_error_file '{}': {}", full_path.display(), e)
                })?;
                Some(ErrorFileEntry {
                    body: Bytes::from(body),
                    cache_control: cache_control_value(cfg.server_error_cache_max_age_secs),
                })
            }
            None => None,
        };

        Ok(Self {
            client_error,
            server_error,
        })
    }
}

/// Resolve a config path relative to the root directory.
/// Absolute paths are used as-is; relative paths are joined with root.
fn resolve_path(root: &str, path: &str) -> std::path::PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new(root).join(p)
    }
}

/// Build a `Cache-Control` header value from a max-age in seconds.
/// A max-age of 0 maps to `no-store`; any positive value maps to `public, max-age=N`.
fn cache_control_value(max_age_secs: u64) -> String {
    if max_age_secs == 0 {
        "no-store".to_string()
    } else {
        format!("public, max-age={}", max_age_secs)
    }
}
