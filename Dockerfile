# Multi-language Kore Format Runtime
FROM ubuntu:22.04

LABEL maintainer="Arun Kather Ashala <arunkatherashala@gmail.com>"
LABEL description="KORE Binary Format - Complete 8-language ecosystem"
LABEL version="0.1.0"

# Install system dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    git \
    python3 \
    python3-pip \
    rustc \
    cargo \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy Kore Format source code
COPY . .

# Build Rust core
RUN cargo build --release

# Install Python package
RUN pip install -e .

# Install Python dependencies
RUN pip install boto3 google-cloud-storage azure-storage-blob

# Create entry point
ENTRYPOINT ["/bin/bash"]

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD python3 -c "import kore_fileformat; print('Kore Format OK')" || exit 1
