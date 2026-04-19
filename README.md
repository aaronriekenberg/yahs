# yahs

**yahs** — *yet another http server* — is a configurable, extensible web server
written in Rust, inspired by nginx.

## Features

| Feature | Status |
|---|---|
| Static file serving | ✅ |
| Blocked paths (glob patterns → 404) | ✅ |
| Per-path cache rules (glob → `Cache-Control: max-age`) | ✅ |
| Precompressed files (zstd · brotli · gzip) | ✅ |
| ETag / If-Modified-Since / If-None-Match | ✅ |
| Byte-range requests | ✅ |
| Reverse proxy with round-robin / ip-hash / random LB | ✅ |
| Hop-by-hop header stripping | ✅ |
| Connect + request timeouts | ✅ |
| Reused reverse-proxy client connection pool | ✅ |
| Configurable TCP keepalive (server + proxy upstream) | ✅ |
| Health / readiness endpoint | ✅ |
| Custom HTML error pages (4xx / 5xx) | ✅ |
| Structured JSON access logs | ✅ |
| Per-request correlation IDs | ✅ |
| Graceful shutdown (SIGTERM / Ctrl-C) | ✅ |
| TOML configuration (nginx-like) | ✅ |
| Extra response headers per location | ✅ |
| HTTP/1.1 + HTTP/2 (via hyper) | ✅ |

## Quick start

```bash
# Build
cargo build --release

# Copy and edit the example config
cp yahs.example.toml yahs.toml
$EDITOR yahs.toml

# Run
./target/release/yahs --config yahs.toml
```

## Configuration

yahs is configured with a single TOML file.  See [`yahs.example.toml`](yahs.example.toml)
for a fully commented example covering all supported options.

### `root`

```toml
root = "./www"   # default: "."
```

Top-level directory used as the base for **all** file paths in the config:

- Static file serving — `static_files` locations serve files from this directory.
- Error files — `client_error_file` and `server_error_file` paths are resolved relative to `root` (absolute paths are used as-is).

Defaults to `"."` (the process working directory).

### Minimal example

```toml
root = "./www"

[server]
bind = "0.0.0.0:8080"

[[locations]]
path = "/"

[locations.handler]
type  = "static_files"
```

### Reverse proxy example

```toml
[server]
bind = "0.0.0.0:8080"

[[upstreams]]
name     = "backend"
strategy = "round_robin"

  [[upstreams.backends]]
  url = "http://127.0.0.1:3000"

[[locations]]
path = "/api"

  [locations.handler]
  type     = "reverse_proxy"
  upstream = "backend"
```

## Architecture

```
src/
├── main.rs          CLI entry point
├── lib.rs           Public library surface (for tests / embedding)
├── config/          TOML config types, loader, and validation
├── server/
│   ├── core.rs      TCP listener, request router, graceful shutdown
│   ├── error_files.rs  Pre-loaded error-file store (4xx / 5xx HTML bodies)
│   └── state.rs     Shared AppState (Arc<Config> + Arc<ErrorFileStore>)
├── handler/
│   ├── mod.rs       Handler trait + response helpers
│   ├── static_files.rs  Static file server
│   ├── proxy.rs     Reverse proxy + upstream load balancer
│   └── health.rs    /health JSON endpoint
├── middleware/
│   ├── logging.rs   Per-request context + access log emission
│   └── request_id.rs UUID-based request IDs
├── compression/     Content negotiation for precompressed files
├── logging/         Structured JSON tracing subscriber
└── error.rs         AppError enum + HTTP status mapping
```

## Handler types

### `static_files`

Serves files from a local directory.  The root directory is taken from the
top-level [`root`](#root) configuration key.

| Key | Default | Description |
|---|---|---|
| `index` | `["index.html"]` | Index file names tried for directory requests |
| `strip_prefix` | `true` | Remove location prefix before path lookup |
| `cache_max_age_secs` | `3600` | `Cache-Control: max-age` fallback value |
| `cache_rules` | `[]` | Ordered per-path cache rules (see below) |
| `blocked_paths` | `[]` | Glob patterns for paths that return 404 (see below) |
| `precompressed` | `true` | Serve `.zst` / `.br` / `.gz` variants when accepted |
| `encodings` | `["zstd","brotli","gzip"]` | Encoding preference order |

#### Blocked paths

`blocked_paths` is a list of glob patterns matched against the request path
relative to the location prefix (after percent-decoding, before filesystem
lookup).  Any match returns **404 Not Found** immediately.

Plain segment names without wildcards (e.g. `".git"`) automatically match at
any depth — `".git"`, `"repo/.git"`, `"repo/.git/config"`, etc.  Patterns with
wildcards follow standard glob syntax via the
[globset](https://docs.rs/globset) crate.

```toml
[locations.handler]
type          = "static_files"
blocked_paths = [".git", ".env", ".htaccess"]
```

#### Per-path cache rules

`cache_rules` is an ordered list of `{pattern, max_age_secs}` entries.  The
first matching rule wins; if nothing matches the request path falls back to
`cache_max_age_secs`.

```toml
[locations.handler]
type               = "static_files"
cache_max_age_secs = 3600        # 1 hour (default / fallback)

[[locations.handler.cache_rules]]
pattern      = "images/*.png"
max_age_secs = 120               # 2 minutes

[[locations.handler.cache_rules]]
pattern      = "**/*.js"
max_age_secs = 86400             # 1 day
```

### `reverse_proxy`

Forwards requests to an upstream cluster.

| Key | Default | Description |
|---|---|---|
| `upstream` | *(required)* | Name of the upstream cluster |
| `strip_prefix` | `true` | Remove location prefix before forwarding |
| `request_timeout_ms` | `30000` | Total request timeout |
| `max_idle_connections_per_host` | `32` | Max idle pooled upstream connections per host |
| `http2_prior_knowledge` | `false` | Use HTTP/2 cleartext (h2c) for upstream connections |
| `tcp_keepalive_enabled` | `false` | Enable TCP keepalive for upstream sockets |
| `tcp_keepalive_secs` | `15` | Upstream TCP keepalive probe idle time |
| `cache_max_age_secs` | `0` | Sets proxied response `Cache-Control` (`no-store` when `0`, else `public, max-age=N`) |
| `extra_request_headers` | `{}` | Headers injected into the forwarded request |
| `remove_request_headers` | `[]` | Request headers stripped before forwarding |

### `health`

Returns `{"status":"ok","version":"…"}` as JSON.  No configuration needed.

## Custom error files

yahs can serve a pre-loaded HTML file in place of the default plain-text body
for any 4xx (client error) or 5xx (server error) response, regardless of which
handler produced the error.  Files are read once at startup — the server will
refuse to start if a configured path is missing or unreadable.

The original HTTP status code is always preserved (e.g. `404 Not Found` stays
`404`), so clients and logs can still distinguish individual error types.

```toml
[error_files]
# Served for any 4xx response.
client_error_file = "errors/4xx.html"
client_error_cache_max_age_secs = 0    # 0 → no-store (default)

# Served for any 5xx response.
server_error_file = "errors/5xx.html"
server_error_cache_max_age_secs = 0    # 0 → no-store (default)
```

Paths are resolved relative to the top-level [`root`](#root) directory.
Absolute paths are used as-is.

Both keys are optional.  Omitting the whole `[error_files]` section (or either
key within it) leaves the corresponding error responses unchanged.

| Key | Default | Description |
|---|---|---|
| `client_error_file` | *(none)* | Path to an HTML file served for 4xx responses |
| `server_error_file` | *(none)* | Path to an HTML file served for 5xx responses |
| `client_error_cache_max_age_secs` | `0` | `Cache-Control: no-store` when `0`, else `public, max-age=N` |
| `server_error_cache_max_age_secs` | `0` | `Cache-Control: no-store` when `0`, else `public, max-age=N` |



| Strategy | Description |
|---|---|
| `round_robin` | Cycle through backends in order (default) |
| `ip_hash` | Hash on client IP (falls back to round-robin) |
| `random` | Random selection |

Backends support a `weight` field (default `1`) for weighted round-robin.

## Precompressed files

When `precompressed = true`, yahs looks for files with `.zst`, `.br`, or `.gz`
appended to the original path and serves the best match according to the
client's `Accept-Encoding` header.  The `Vary: Accept-Encoding` header is
always included.

```
www/
├── index.html
├── index.html.br    ← served when client accepts brotli
├── index.html.gz    ← served when client accepts gzip
└── index.html.zst   ← served when client accepts zstd
```

## Access logs

Every request emits a JSON line to stdout at `INFO` level:

```json
{
  "timestamp": "2026-04-18T10:00:00.000Z",
  "request_id": "550e8400-e29b-41d4-a716-446655440000",
  "remote_addr": "127.0.0.1:54321",
  "method": "GET",
  "uri": "/static/index.html",
  "http_version": "HTTP/1.1",
  "status": 200,
  "response_bytes": 1234,
  "duration_ns": 420000,
  "request_headers": {
    "accept": ["*/*"]
  },
  "response_headers": {
    "content-type": ["text/html"]
  },
  "location": "/static"
}
```

## Development

```bash
# Build
cargo build

# Run tests
cargo test

# Clippy
cargo clippy

# Format
cargo fmt
```

## License

MIT
