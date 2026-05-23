# Stage 1: Builder
FROM ubuntu:24.04 AS builder

# Install rustup and dependencies
RUN apt-get update && apt-get install -y \
    curl \
    build-essential \
    postgresql-client \
    libpq-dev \
    libssl-dev \
    pkg-config \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly \
    && . $HOME/.cargo/env

WORKDIR /app

# Copy manifest and source
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

# Set cargo env
ENV PATH="/root/.cargo/bin:${PATH}"

# Build with all features
RUN cargo +nightly build --release --features postgres,s3

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    postgresql-client \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -g 1001 kore && \
    useradd -u 1001 -g kore -m -s /usr/sbin/nologin kore

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/kore-cloud /usr/local/bin/kore-cloud

# Copy documentation
COPY README.md /app/

# Set ownership
RUN chown -R kore:kore /app

# Use non-root user
USER kore

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:8000/health || exit 1

# Expose API port
EXPOSE 8000

# Run the application
CMD ["kore-cloud"]
