use std::path::{Path, PathBuf};
use std::time::SystemTime;

use async_trait::async_trait;
use globset::{Glob, GlobSet, GlobSetBuilder};
use hyper::{Request, Response, StatusCode, body::Incoming, header};
use tokio::io::AsyncReadExt;
use tracing::{debug, warn};

use crate::compression::negotiate_encoding;
use crate::config::{PrecompressedEncoding, StaticFilesConfig};
use crate::error::AppError;
use crate::handler::{Handler, HandlerResponse, empty_body, stream_body};
use crate::server::state::AppState;

pub struct StaticFilesHandler {
    config: StaticFilesConfig,
    /// Absolute canonical root (resolved at construction time).
    root: PathBuf,
    /// Location prefix this handler was registered under.
    location_prefix: String,
    /// Compiled glob set for blocked paths (returns 404 on match).
    blocked_set: GlobSet,
    /// Compiled glob sets for cache rules (parallel to `config.cache_rules`).
    cache_rule_sets: Vec<GlobSet>,
}

impl StaticFilesHandler {
    pub fn new(
        config: StaticFilesConfig,
        location_prefix: String,
        root: &str,
    ) -> anyhow::Result<Self> {
        let root = std::fs::canonicalize(root)
            .map_err(|e| anyhow::anyhow!("Cannot canonicalize static root '{}': {}", root, e))?;

        // Pre-compile blocked-paths glob set.
        let blocked_set = build_glob_set(&config.blocked_paths)?;

        // Pre-compile one GlobSet per cache rule.
        let cache_rule_sets = config
            .cache_rules
            .iter()
            .map(|rule| build_glob_set(std::slice::from_ref(&rule.pattern)))
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(Self {
            config,
            root,
            location_prefix,
            blocked_set,
            cache_rule_sets,
        })
    }
}

#[async_trait]
impl Handler for StaticFilesHandler {
    async fn handle(
        &self,
        req: Request<Incoming>,
        _state: &AppState,
    ) -> Result<HandlerResponse, AppError> {
        // Only allow GET and HEAD.
        if req.method() != hyper::Method::GET && req.method() != hyper::Method::HEAD {
            return Ok(Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .header(header::ALLOW, "GET, HEAD")
                .body(empty_body())
                .unwrap());
        }

        let uri_path = req.uri().path();

        // Strip the location prefix from the URI path.
        let rel_path = if self.config.strip_prefix {
            uri_path
                .strip_prefix(&self.location_prefix)
                .unwrap_or(uri_path)
        } else {
            uri_path
        };

        // Reject blocked paths before touching the filesystem.
        let decoded_rel = percent_decode(rel_path);
        if self
            .blocked_set
            .is_match(decoded_rel.trim_start_matches('/'))
        {
            return Err(AppError::NotFound);
        }

        // Resolve securely to an absolute path under root.
        let file_path = self.resolve_path(rel_path)?;

        debug!("Serving static file: {}", file_path.display());

        // Determine accepted encoding from the request.
        let accept_encoding = req
            .headers()
            .get(header::ACCEPT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());

        // Negotiate precompressed encoding once; pass it through so that both
        // direct-file and index-file lookups can serve precompressed variants.
        let negotiated = if self.config.precompressed {
            negotiate_encoding(accept_encoding.as_deref(), &self.config.encodings)
        } else {
            None
        };

        // For non-directory requests try the precompressed variant directly.
        // (Directories are handled inside serve_index_or_404 below.)
        if let Some(encoding) = negotiated {
            let compressed_path = append_extension(&file_path, encoding.extension());
            if compressed_path.is_file() {
                return self
                    .serve_regular_file(
                        &req,
                        &compressed_path,
                        Some(encoding.content_encoding()),
                        detect_mime(&file_path),
                        &decoded_rel,
                    )
                    .await;
            }
        }

        // Serve the file (or resolve to an index file if it is a directory).
        // Pass the negotiated encoding so that index-file resolution can also
        // probe for precompressed index variants (e.g. index.html.zst).
        self.serve_file(&req, &file_path, negotiated, &decoded_rel)
            .await
    }
}

impl StaticFilesHandler {
    /// Safely resolve a relative URI path under the static root.
    fn resolve_path(&self, rel_path: &str) -> Result<PathBuf, AppError> {
        // Decode percent-encoding and normalise slashes.
        let decoded = percent_decode(rel_path);

        // Reject paths with null bytes.
        if decoded.contains('\0') {
            return Err(AppError::BadRequest("Invalid path".to_string()));
        }

        // Join with root.
        let joined = self.root.join(decoded.trim_start_matches('/'));

        // Canonicalize to prevent directory traversal.
        let canonical = match joined.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                // File may not exist; still validate the non-canonical path.
                let normalized = normalize_path(&joined);
                if !normalized.starts_with(&self.root) {
                    return Err(AppError::NotFound);
                }
                return Ok(normalized);
            }
        };

        if !canonical.starts_with(&self.root) {
            warn!(
                "Path traversal attempt: {} resolves to {} (outside root {})",
                rel_path,
                canonical.display(),
                self.root.display()
            );
            return Err(AppError::NotFound);
        }

        Ok(canonical)
    }

    /// Read a file and build a response with appropriate headers.
    /// Route to the right serve implementation, handling directories by trying index files.
    async fn serve_file(
        &self,
        req: &Request<Incoming>,
        path: &Path,
        negotiated: Option<PrecompressedEncoding>,
        rel_path: &str,
    ) -> Result<HandlerResponse, AppError> {
        let metadata = match tokio::fs::metadata(path).await {
            Ok(m) => m,
            Err(_) => {
                return self
                    .serve_index_or_404(req, path, negotiated, rel_path)
                    .await;
            }
        };

        if metadata.is_dir() {
            return self
                .serve_index_or_404(req, path, negotiated, rel_path)
                .await;
        }

        if !metadata.is_file() {
            return Err(AppError::NotFound);
        }

        self.serve_regular_file(req, path, None, detect_mime(path), rel_path)
            .await
    }

    /// Serve an actual regular file (no directory / index lookup).
    async fn serve_regular_file(
        &self,
        req: &Request<Incoming>,
        path: &Path,
        content_encoding: Option<&str>,
        content_type: &str,
        rel_path: &str,
    ) -> Result<HandlerResponse, AppError> {
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|_| AppError::NotFound)?;

        let file_size = metadata.len();
        let last_modified = metadata.modified().ok();

        // Build ETag from size + mtime.
        let etag = build_etag(file_size, last_modified);

        // Conditional GET: If-None-Match.
        if let Some(inm) = req.headers().get(header::IF_NONE_MATCH)
            && inm
                .to_str()
                .unwrap_or("")
                .split(',')
                .any(|t| t.trim().trim_matches('"') == etag.trim_matches('"'))
        {
            return Ok(Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, &etag)
                .body(empty_body())
                .unwrap());
        }

        // Conditional GET: If-Modified-Since.
        if let (Some(ims), Some(mtime)) =
            (req.headers().get(header::IF_MODIFIED_SINCE), last_modified)
            && let Ok(ims_str) = ims.to_str()
            && let Ok(ims_time) = httpdate::parse_http_date(ims_str)
            && mtime <= ims_time
        {
            return Ok(Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, &etag)
                .body(empty_body())
                .unwrap());
        }

        // Handle Range request.
        if let Some(range_header) = req.headers().get(header::RANGE)
            && let Ok(range_str) = range_header.to_str()
        {
            return self
                .serve_range(path, range_str, file_size, &etag, content_type)
                .await;
        }

        // Open the file for streaming – no need to load it all into memory.
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|_| AppError::NotFound)?;

        let cache_max_age = self.effective_cache_max_age(rel_path);

        let mut builder = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CONTENT_LENGTH, file_size.to_string())
            .header(header::ETAG, &etag)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(
                header::CACHE_CONTROL,
                format!("public, max-age={}", cache_max_age),
            )
            .header(header::VARY, "Accept-Encoding");

        if let Some(enc) = content_encoding {
            builder = builder.header(header::CONTENT_ENCODING, enc);
        }

        if let Some(mtime) = last_modified {
            builder = builder.header(header::LAST_MODIFIED, httpdate::fmt_http_date(mtime));
        }

        let body = if req.method() == hyper::Method::HEAD {
            empty_body()
        } else {
            stream_body(file)
        };

        Ok(builder.body(body).unwrap())
    }

    /// Try index files when a directory path is requested.
    async fn serve_index_or_404(
        &self,
        req: &Request<Incoming>,
        dir_path: &Path,
        negotiated: Option<PrecompressedEncoding>,
        rel_path: &str,
    ) -> Result<HandlerResponse, AppError> {
        let dir_path = if dir_path.is_file() {
            dir_path.parent().unwrap_or(dir_path)
        } else {
            dir_path
        };

        for index in &self.config.index {
            let index_path = dir_path.join(index);
            if !index_path.is_file() {
                continue;
            }
            let mime = detect_mime(&index_path);

            // Try the precompressed variant of the index file first.
            if let Some(encoding) = negotiated {
                let compressed_index = append_extension(&index_path, encoding.extension());
                if compressed_index.is_file() {
                    return self
                        .serve_regular_file(
                            req,
                            &compressed_index,
                            Some(encoding.content_encoding()),
                            mime,
                            rel_path,
                        )
                        .await;
                }
            }

            return self
                .serve_regular_file(req, &index_path, None, mime, rel_path)
                .await;
        }

        Err(AppError::NotFound)
    }

    /// Return the effective `max-age` for the given relative path.
    /// The first matching `cache_rules` entry wins; falls back to `cache_max_age_secs`.
    fn effective_cache_max_age(&self, rel_path: &str) -> u64 {
        let path = rel_path.trim_start_matches('/');
        for (i, rule_set) in self.cache_rule_sets.iter().enumerate() {
            if rule_set.is_match(path) {
                return self.config.cache_rules[i].max_age_secs;
            }
        }
        self.config.cache_max_age_secs
    }

    /// Serve a byte-range subset of a file.
    async fn serve_range(
        &self,
        path: &Path,
        range_header: &str,
        file_size: u64,
        etag: &str,
        content_type: &str,
    ) -> Result<HandlerResponse, AppError> {
        let (start, end) = parse_range(range_header, file_size)
            .ok_or_else(|| AppError::BadRequest("Invalid range".to_string()))?;

        if start > end || end >= file_size {
            return Ok(Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{}", file_size))
                .body(empty_body())
                .unwrap());
        }

        // Read the slice.
        let mut file = tokio::fs::File::open(path)
            .await
            .map_err(|_| AppError::NotFound)?;

        use tokio::io::AsyncSeekExt;
        file.seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|e| AppError::internal(e.to_string()))?;

        let length = end - start + 1;

        Ok(Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CONTENT_LENGTH, length.to_string())
            .header(
                header::CONTENT_RANGE,
                format!("bytes {}-{}/{}", start, end, file_size),
            )
            .header(header::ETAG, etag)
            .header(header::ACCEPT_RANGES, "bytes")
            .body(stream_body(file.take(length)))
            .unwrap())
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Build a `GlobSet` from a slice of pattern strings.
/// Each pattern is matched case-sensitively.  Patterns without a `/` are
/// automatically treated as path-component substring matches by wrapping them
/// in `**/<pattern>/**` and `**/<pattern>` forms so that e.g. `".git"` blocks
/// any path segment named `.git`.
fn build_glob_set(patterns: &[String]) -> anyhow::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        // If the pattern has no path separator and no wildcard, treat it as a
        // path-component match: block any path that *contains* the segment.
        if !pattern.contains('/') && !pattern.contains('*') && !pattern.contains('?') {
            builder.add(Glob::new(&format!("**/{pattern}"))?);
            builder.add(Glob::new(&format!("**/{pattern}/**"))?);
            builder.add(Glob::new(pattern)?);
        } else {
            builder.add(Glob::new(pattern)?);
        }
    }
    Ok(builder.build()?)
}

fn detect_mime(path: &Path) -> &'static str {
    mime_guess::from_path(path)
        .first_raw()
        .unwrap_or("application/octet-stream")
}

fn build_etag(size: u64, mtime: Option<SystemTime>) -> String {
    let mtime_secs = mtime
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("\"{:x}-{:x}\"", mtime_secs, size)
}

fn append_extension(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(ext);
    PathBuf::from(s)
}

/// Very simple percent-decoding (only %XX sequences).
fn percent_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2]))
        {
            out.push(char::from(h << 4 | l));
            i += 3;
            continue;
        }
        out.push(char::from(bytes[i]));
        i += 1;
    }
    out
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Normalize `..` and `.` components without hitting the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// Parse a simple `bytes=start-end` range header.
/// Returns `(start, end)` (both inclusive).
fn parse_range(header: &str, file_size: u64) -> Option<(u64, u64)> {
    let s = header.trim().strip_prefix("bytes=")?;
    let (start_str, end_str) = s.split_once('-')?;

    if start_str.is_empty() {
        // Suffix range: bytes=-N  → last N bytes.
        let suffix: u64 = end_str.trim().parse().ok()?;
        let start = file_size.saturating_sub(suffix);
        Some((start, file_size.saturating_sub(1)))
    } else {
        let start: u64 = start_str.trim().parse().ok()?;
        let end = if end_str.trim().is_empty() {
            file_size.saturating_sub(1)
        } else {
            end_str.trim().parse().ok()?
        };
        Some((start, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CacheRule, StaticFilesConfig};

    fn make_handler(
        tmp_root: &std::path::Path,
        blocked_paths: Vec<String>,
        cache_rules: Vec<CacheRule>,
        cache_max_age_secs: u64,
    ) -> StaticFilesHandler {
        let config = StaticFilesConfig {
            index: vec!["index.html".to_owned()],
            strip_prefix: true,
            cache_max_age_secs,
            cache_rules,
            blocked_paths,
            precompressed: false,
            encodings: vec![],
        };
        let root = tmp_root.to_str().unwrap();
        StaticFilesHandler::new(config, "/static".to_owned(), root).unwrap()
    }

    // ── blocked_paths ────────────────────────────────────────────────────────

    #[test]
    fn blocked_path_plain_segment_is_blocked() {
        let tmp = tempfile::tempdir().unwrap();
        let handler = make_handler(tmp.path(), vec![".git".to_owned()], vec![], 3600);
        // Bare segment
        assert!(handler.blocked_set.is_match(".git"));
        // Nested
        assert!(handler.blocked_set.is_match("repo/.git"));
        assert!(handler.blocked_set.is_match("repo/.git/config"));
    }

    #[test]
    fn blocked_path_glob_pattern_is_blocked() {
        let tmp = tempfile::tempdir().unwrap();
        let handler = make_handler(tmp.path(), vec!["**/.env".to_owned()], vec![], 3600);
        assert!(handler.blocked_set.is_match("subdir/.env"));
        assert!(!handler.blocked_set.is_match("subdir/app.env")); // different name
    }

    #[test]
    fn blocked_path_unrelated_path_is_not_blocked() {
        let tmp = tempfile::tempdir().unwrap();
        let handler = make_handler(tmp.path(), vec![".git".to_owned()], vec![], 3600);
        assert!(!handler.blocked_set.is_match("index.html"));
        assert!(!handler.blocked_set.is_match("images/logo.png"));
    }

    #[test]
    fn empty_blocked_paths_blocks_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let handler = make_handler(tmp.path(), vec![], vec![], 3600);
        assert!(!handler.blocked_set.is_match(".git"));
        assert!(!handler.blocked_set.is_match("secret/.env"));
    }

    // ── effective_cache_max_age ──────────────────────────────────────────────

    #[test]
    fn cache_rule_first_match_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let rules = vec![
            CacheRule {
                pattern: "images/*.png".to_owned(),
                max_age_secs: 120,
            },
            CacheRule {
                pattern: "**/*.js".to_owned(),
                max_age_secs: 86400,
            },
        ];
        let handler = make_handler(tmp.path(), vec![], rules, 3600);

        assert_eq!(handler.effective_cache_max_age("images/logo.png"), 120);
        assert_eq!(handler.effective_cache_max_age("/images/logo.png"), 120);
        assert_eq!(handler.effective_cache_max_age("app.js"), 86400);
        assert_eq!(handler.effective_cache_max_age("js/bundle.js"), 86400);
    }

    #[test]
    fn cache_rule_fallback_to_default() {
        let tmp = tempfile::tempdir().unwrap();
        let rules = vec![CacheRule {
            pattern: "images/*.png".to_owned(),
            max_age_secs: 120,
        }];
        let handler = make_handler(tmp.path(), vec![], rules, 3600);

        // HTML files don't match any rule → use default
        assert_eq!(handler.effective_cache_max_age("index.html"), 3600);
    }

    #[test]
    fn cache_rule_no_rules_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let handler = make_handler(tmp.path(), vec![], vec![], 7200);
        assert_eq!(handler.effective_cache_max_age("anything.css"), 7200);
    }
}
