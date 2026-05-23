# Multi-stage build for Kore Cloud API

# Stage 1: Builder
FROM rust:1.75-alpine as builder

WORKDIR /app

# Install dependencies
RUN apk add --no-cache \
    build-base \
    postgresql-dev \
    openssl-dev \
    ca-certificates

# Copy manifest and source
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

# Build with all features
RUN cargo build --release --features postgres,s3

# Stage 2: Runtime
FROM alpine:3.18

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    postgresql-client \
    libssl3 \
    libcrypto3

# Create non-root user
RUN addgroup -g 1001 kore && \
    adduser -u 1001 -G kore -h /app -s /sbin/nologin -D kore

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/kore-cloud /usr/local/bin/kore-cloud

# Copy documentation
COPY README.md /app/
COPY CONTRIBUTING.md /app/
COPY LICENSE /app/

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
