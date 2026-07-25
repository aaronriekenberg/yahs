# Build stage
FROM rust:1 as build-env
WORKDIR /app
COPY . /app
RUN cargo build --release

# Runtime stage - distroless
FROM gcr.io/distroless/cc-debian13

# Copy the compiled binary
COPY --from=build-env /app/target/release/yahs /usr/local/bin/yahs

# Copy the example config as the default
COPY yahs.example.toml /etc/yahs.toml

# Set working directory (used as root for relative paths in config)
WORKDIR /app

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/yahs", "--config", "/etc/yahs.toml"]
