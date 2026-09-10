# ============================================================================
# KORE v1.8.0 - Production Docker Image
# Multi-stage build for minimal final image size
# ============================================================================

# Stage 1: Builder - Compile KORE binaries
FROM rust:1.75-slim as builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy source code
COPY Cargo.* ./
COPY kore-* ./

# Build all KORE binaries in release mode
RUN cargo build --release \
    -p kore-coord \
    -p kore-worker \
    -p kore-cli \
    --target-dir /build/target

# ============================================================================
# Stage 2: Runtime - Minimal production image
FROM debian:bookworm-slim

LABEL maintainer="KORE Team <team@kore.dev>"
LABEL version="1.8.0"
LABEL description="KORE v1.8.0 - 339x Faster SQL Engine"

WORKDIR /app

# Install runtime dependencies only
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy binaries from builder
COPY --from=builder /build/target/release/kore-coord /usr/local/bin/kore-coord
COPY --from=builder /build/target/release/kore-worker /usr/local/bin/kore-worker
COPY --from=builder /build/target/release/kore-cli /usr/local/bin/kore

# Create non-root user for security
RUN groupadd -r kore && useradd -r -g kore kore
USER kore

# Create data directory with proper permissions
RUN mkdir -p /app/data && chown kore:kore /app/data
VOLUME ["/app/data"]

# Expose coordinator and worker ports
EXPOSE 9876 9877

# Health check - Query coordinator status
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:9876/health || exit 1

# Default entrypoint: Start coordinator
ENTRYPOINT ["kore-coord"]

# ============================================================================
# Usage:
# 
# Build image:
#   docker build -t kore-engine:1.8.0 .
# 
# Run as coordinator:
#   docker run -d --name kore-coord \
#     -p 9876:9876 \
#     -p 9877:9877 \
#     -v kore-data:/app/data \
#     kore-engine:1.8.0
#
# Run as worker (connect to coordinator):
#   docker run -d --name kore-worker \
#     kore-engine:1.8.0 \
#     kore-worker --coordinator kore-coord:9876
#
# Run CLI (one-shot query):
#   docker run --rm kore-engine:1.8.0 \
#     kore --query "SELECT * FROM lineitem LIMIT 10"
#
# ============================================================================
