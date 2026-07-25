# Build stage
FROM rust:1 as build-env
WORKDIR /app
COPY . /app
RUN cargo build --release

# Runtime stage - distroless
FROM gcr.io/distroless/cc-debian13
COPY --from=build-env /app/target/release/yahs /yahs
ENTRYPOINT ["/yahs"]
