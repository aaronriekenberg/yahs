/// Integration tests and unit tests for yahs.
///
/// Unit tests for config parsing, compression negotiation, and path helpers
/// are included here.  Integration tests that start an actual server are in
/// the separate `tests/integration.rs` file.

#[cfg(test)]
mod compression_tests {
    use yahs::compression::negotiate_encoding;
    use yahs::config::PrecompressedEncoding;

    #[test]
    fn test_negotiate_zstd_preferred() {
        let server_prefs = &[
            PrecompressedEncoding::Zstd,
            PrecompressedEncoding::Brotli,
            PrecompressedEncoding::Gzip,
        ];
        let result = negotiate_encoding(Some("gzip, br, zstd"), server_prefs);
        assert_eq!(result, Some(PrecompressedEncoding::Zstd));
    }

    #[test]
    fn test_negotiate_client_prefers_br() {
        // Server prefers zstd, but only gzip is accepted by client.
        let server_prefs = &[
            PrecompressedEncoding::Zstd,
            PrecompressedEncoding::Brotli,
            PrecompressedEncoding::Gzip,
        ];
        let result = negotiate_encoding(Some("gzip"), server_prefs);
        assert_eq!(result, Some(PrecompressedEncoding::Gzip));
    }

    #[test]
    fn test_negotiate_no_matching_encoding() {
        let server_prefs = &[PrecompressedEncoding::Zstd, PrecompressedEncoding::Brotli];
        let result = negotiate_encoding(Some("gzip"), server_prefs);
        assert_eq!(result, None);
    }

    #[test]
    fn test_negotiate_no_accept_encoding_header() {
        let server_prefs = &[PrecompressedEncoding::Zstd];
        let result = negotiate_encoding(None, server_prefs);
        assert_eq!(result, None);
    }

    #[test]
    fn test_negotiate_q_value_ordering() {
        // Client sends br with higher q than zstd.
        let server_prefs = &[PrecompressedEncoding::Zstd, PrecompressedEncoding::Brotli];
        let result = negotiate_encoding(Some("zstd;q=0.5, br;q=0.9"), server_prefs);
        // Server prefers zstd, but client q-values don't affect server preference ordering.
        // The server iterates its preference list; zstd is first and q > 0, so zstd wins.
        assert_eq!(result, Some(PrecompressedEncoding::Zstd));
    }

    #[test]
    fn test_negotiate_zero_q_rejected() {
        let server_prefs = &[PrecompressedEncoding::Zstd];
        let result = negotiate_encoding(Some("zstd;q=0"), server_prefs);
        assert_eq!(result, None);
    }
}

#[cfg(test)]
mod config_tests {
    use std::io::Write;
    use tempfile::NamedTempFile;
    use yahs::config::load_config;

    fn write_config(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn test_load_minimal_health_config() {
        let cfg_content = r#"
[server]
bind = "127.0.0.1:0"

[[locations]]
path = "/health"

[locations.handler]
type = "health"
"#;
        let f = write_config(cfg_content);
        let config = load_config(f.path()).unwrap();
        assert_eq!(config.server.bind, "127.0.0.1:0");
        assert_eq!(config.locations.len(), 1);
        assert_eq!(config.locations[0].path, "/health");
    }

    #[test]
    fn test_load_static_files_config() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_str().unwrap().to_owned();
        let cfg_content = format!(
            r#"
[server]
bind = "127.0.0.1:8080"

[[locations]]
path = "/static"

[locations.handler]
type = "static_files"
root = "{root}"
"#,
            root = root
        );
        let f = write_config(&cfg_content);
        let config = load_config(f.path()).unwrap();
        assert_eq!(config.locations[0].path, "/static");
    }

    #[test]
    fn test_load_proxy_config() {
        let cfg_content = r#"
[server]
bind = "127.0.0.1:8080"

[[upstreams]]
name = "backend"
[[upstreams.backends]]
url = "http://127.0.0.1:3000"

[[locations]]
path = "/api"

[locations.handler]
type = "reverse_proxy"
upstream = "backend"
"#;
        let f = write_config(cfg_content);
        let config = load_config(f.path()).unwrap();
        assert_eq!(config.upstreams.len(), 1);
        assert_eq!(config.upstreams[0].name, "backend");
    }

    #[test]
    fn test_config_missing_upstream_fails() {
        let cfg_content = r#"
[server]
bind = "127.0.0.1:8080"

[[locations]]
path = "/api"

[locations.handler]
type = "reverse_proxy"
upstream = "nonexistent"
"#;
        let f = write_config(cfg_content);
        let result = load_config(f.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("nonexistent"),
            "error should name the missing upstream: {msg}"
        );
    }

    #[test]
    fn test_config_location_must_start_with_slash() {
        let cfg_content = r#"
[server]
bind = "127.0.0.1:8080"

[[locations]]
path = "noslash"

[locations.handler]
type = "health"
"#;
        let f = write_config(cfg_content);
        let result = load_config(f.path());
        assert!(result.is_err());
    }
}
