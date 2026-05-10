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
    openjdk-17-jdk \
    python3 \
    python3-pip \
    golang-1.19 \
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

# Build PyO3 bindings
RUN cd rust-bindings && cargo build --release

# Build Hadoop InputFormat
RUN cd hadoop && mvn clean package -DskipTests

# Build Spark DataSourceV2
RUN cd language-bindings/spark && cargo build --release

# Build AWS Glue Connector
RUN cd language-bindings/aws-glue && cargo build --release

# Build Go bindings
RUN cd language-bindings/go && go build ./kore

# Build Java JNI
RUN cd language-bindings/java && cargo build --release

# Install Python package
RUN pip install -e .

# Install Python dependencies
RUN pip install boto3 google-cloud-storage azure-storage-blob

# Create entry point
ENTRYPOINT ["/bin/bash"]

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD python3 -c "import kore_parser; print('Kore Format OK')" || exit 1
