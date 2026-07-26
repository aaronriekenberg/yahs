# Claude — yahs Project Guide

## Project Overview

**yahs** (yet another http server) is a high-performance, configurable HTTP/1.1 and HTTP/2 web server written in Rust, inspired by nginx. It serves static files, manages reverse proxies, and provides features like precompressed file serving, structured logging, and graceful shutdown.

## Key Features

- **Static file serving** with index file support and directory stripping
- **Reverse proxy** with round-robin, IP-hash, and random load balancing
- **Precompressed files** (zstd, brotli, gzip) with content negotiation
- **Caching** with per-path rules and Cache-Control headers
- **HTTP/2 support** via the hyper framework
- **Structured JSON logging** with per-request correlation IDs
- **Graceful shutdown** on SIGTERM/Ctrl-C
- **TOML-based configuration** (nginx-like syntax)
- **Custom error pages** (4xx/5xx HTML)
- **Reverse proxy features**: hop-by-hop header stripping, connection pooling, TCP keepalive

## Project Structure

```
src/
├── main.rs              CLI entry point
├── lib.rs               Public library surface
├── config/              TOML config types, loader, validation
├── server/
│   ├── core.rs          TCP listener, request router, graceful shutdown
│   ├── error_files.rs   Pre-loaded error-file store (4xx/5xx HTML)
│   └── state.rs         Shared AppState (Arc<Config> + Arc<ErrorFileStore>)
├── handler/
│   ├── mod.rs           Handler trait + response helpers
│   ├── static_files.rs  Static file server
│   ├── proxy.rs         Reverse proxy + load balancer
│   └── health.rs        /health JSON endpoint
├── middleware/
│   ├── logging.rs       Per-request context + access log emission
│   └── request_id.rs    UUID-based request IDs
├── compression/         Content negotiation for precompressed files
├── logging/             Structured JSON tracing subscriber
└── error.rs             AppError enum + HTTP status mapping
```

## Core Concepts

### Handler Types

1. **static_files** — Serves local files with cache rules, blocked paths, precompression support
2. **reverse_proxy** — Forwards requests to upstream clusters with load balancing
3. **health** — Returns JSON status endpoint

### Configuration

- TOML-based, single-file configuration
- Top-level `root` directory for relative paths
- Per-location handlers with type-specific options
- Upstream definitions for reverse proxy with connection pooling settings

### Request Processing

1. Request arrives → connection assigned
2. Request ID and correlation context injected
3. Routed to appropriate handler based on path
4. Response logged with structured JSON format
5. Response sent with appropriate headers (cache, compression, etc.)

## Common Tasks

### Adding a New Handler Type

1. Create handler module in `src/handler/`
2. Implement `Handler` trait
3. Add to config types in `src/config/`
4. Update router in `src/server/core.rs`

### Modifying Configuration Options

1. Update types in `src/config/`
2. Add validation logic
3. Update `yahs.example.toml` documentation
4. Handle backward compatibility

### Debugging

- Set `RUST_LOG=debug` for detailed logging
- Access logs are JSON lines on stdout (INFO level)
- Each request has a `request_id` for tracing

## Development Commands

```bash
cargo build              # Build debug binary
cargo build --release   # Build optimized binary
cargo test              # Run tests
cargo clippy            # Lint
cargo fmt               # Format code
./target/release/yahs --config yahs.toml  # Run server
```

## Testing

Tests are located in `tests/` directory. Run with `cargo test`.

## Docker

Built with distroless base for minimal size. Default config at `/etc/yahs.toml`. See README for docker-compose examples.

## Key Dependencies

- **hyper** — HTTP/1.1 and HTTP/2
- **tokio** — Async runtime
- **tracing** — Structured logging
- **serde** — Serialization
- **toml** — Configuration format

## Performance Considerations

- Connection pooling for upstream proxies
- TCP_NODELAY and TCP keepalive configuration
- Precompressed file serving reduces bandwidth
- Graceful shutdown prevents dropped connections
- Structured logging for efficient monitoring
