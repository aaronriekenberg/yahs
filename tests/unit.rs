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
        assert_eq!(config.server.max_connections, 0);
        assert_eq!(config.server.http_keepalive_timeout_secs, 75);
        assert!(!config.server.tcp_keepalive_enabled);
        assert_eq!(config.server.tcp_keepalive_secs, 75);
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
max_connection_pool_size = 50
tcp_keepalive_enabled = true
tcp_keepalive_secs = 42
cache_max_age_secs = 120
remove_request_headers = ["x-internal-token"]

[locations.handler.extra_request_headers]
"x-proxy" = "yahs"
"#;
        let f = write_config(cfg_content);
        let config = load_config(f.path()).unwrap();
        assert_eq!(config.upstreams.len(), 1);
        assert_eq!(config.upstreams[0].name, "backend");
        match &config.locations[0].handler {
            yahs::config::HandlerConfig::ReverseProxy(proxy) => {
                assert_eq!(proxy.max_connection_pool_size, 50);
                assert!(proxy.tcp_keepalive_enabled);
                assert_eq!(proxy.tcp_keepalive_secs, 42);
                assert_eq!(proxy.cache_max_age_secs, 120);
                assert_eq!(
                    proxy.remove_request_headers,
                    vec!["x-internal-token".to_string()]
                );
                assert_eq!(
                    proxy.extra_request_headers.get("x-proxy"),
                    Some(&"yahs".to_string())
                );
            }
            _ => panic!("expected reverse_proxy handler"),
        }
    }

    #[test]
    fn test_load_proxy_config_defaults_cache_max_age_to_zero() {
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
        match &config.locations[0].handler {
            yahs::config::HandlerConfig::ReverseProxy(proxy) => {
                assert_eq!(proxy.cache_max_age_secs, 0);
                assert!(proxy.extra_request_headers.is_empty());
                assert!(proxy.remove_request_headers.is_empty());
            }
            _ => panic!("expected reverse_proxy handler"),
        }
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

    #[test]
    fn test_static_files_blocked_paths_and_cache_rules_config() {
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
cache_max_age_secs = 3600
blocked_paths = [".git", ".env"]

[[locations.handler.cache_rules]]
pattern = "images/*.png"
max_age_secs = 120

[[locations.handler.cache_rules]]
pattern = "**/*.js"
max_age_secs = 86400
"#,
            root = root
        );
        let f = write_config(&cfg_content);
        let config = load_config(f.path()).unwrap();
        match &config.locations[0].handler {
            yahs::config::HandlerConfig::StaticFiles(sf) => {
                assert_eq!(sf.cache_max_age_secs, 3600);
                assert_eq!(sf.blocked_paths, vec![".git", ".env"]);
                assert_eq!(sf.cache_rules.len(), 2);
                assert_eq!(sf.cache_rules[0].pattern, "images/*.png");
                assert_eq!(sf.cache_rules[0].max_age_secs, 120);
                assert_eq!(sf.cache_rules[1].pattern, "**/*.js");
                assert_eq!(sf.cache_rules[1].max_age_secs, 86400);
            }
            _ => panic!("expected static_files handler"),
        }
    }

    #[test]
    fn test_static_files_defaults_have_empty_blocked_and_cache_rules() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_str().unwrap().to_owned();
        let cfg_content = format!(
            r#"
[server]
bind = "127.0.0.1:8080"

[[locations]]
path = "/"

[locations.handler]
type = "static_files"
root = "{root}"
"#,
            root = root
        );
        let f = write_config(&cfg_content);
        let config = load_config(f.path()).unwrap();
        match &config.locations[0].handler {
            yahs::config::HandlerConfig::StaticFiles(sf) => {
                assert!(sf.blocked_paths.is_empty());
                assert!(sf.cache_rules.is_empty());
                assert_eq!(sf.cache_max_age_secs, 3600); // default
            }
            _ => panic!("expected static_files handler"),
        }
    }
}
