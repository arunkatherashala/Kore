# KORE Binary Format - Rust Runtime
FROM rust:latest

LABEL maintainer="Arun Kather Ashala <arunkatherashala@gmail.com>"
LABEL description="KORE Binary Format - Rust Runtime"
LABEL version="1.0.0"

WORKDIR /app

# Copy source code
COPY . .

# Build with minimal dependencies - skip optional features
RUN cargo build --release --no-default-features 2>&1 || cargo check

# Install runtime dependencies
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

ENTRYPOINT ["/bin/bash"]
