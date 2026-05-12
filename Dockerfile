# Multi-language Kore Format Runtime
FROM rust:latest as builder

WORKDIR /app
COPY . .

# Build Rust core
RUN cargo build --release

# Runtime image
FROM debian:bookworm-slim

WORKDIR /app

# Copy the built binary from builder
COPY --from=builder /app/target/release/kore* /usr/local/bin/

# Install runtime dependencies
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

LABEL maintainer="Arun Kather Ashala <arunkatherashala@gmail.com>"
LABEL description="KORE Binary Format - Rust Runtime"
LABEL version="1.0.0"

ENTRYPOINT ["/bin/bash"]
