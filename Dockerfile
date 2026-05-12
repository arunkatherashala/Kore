# Multi-language Kore Format Runtime
FROM ubuntu:22.04

LABEL maintainer="Arun Kather Ashala <arunkatherashala@gmail.com>"
LABEL description="KORE Binary Format - Complete 8-language ecosystem"
LABEL version="1.0.0"

# Install system dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    git \
    libssl-dev \
    pkg-config \
    openjdk-17-jdk \
    python3 \
    python3-pip \
    python3-dev \
    golang \
    scala \
    rustc \
    cargo \
    maven \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy Kore Format source code
COPY . .

# Build Rust core
RUN cargo build --release

# Install Python package
RUN pip install -e .

# Create entry point
ENTRYPOINT ["/bin/bash"]

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD python3 -c "import kore_parser; print('Kore Format OK')" || exit 1
