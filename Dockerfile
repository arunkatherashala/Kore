# KORE Binary Format - Documentation & Reference Image
# Purpose: Multi-language library reference with bindings examples
# Includes: Rust source, Python/JS/Java/Go/C#/Ruby/C++ examples, docs

FROM debian:bookworm-slim

LABEL maintainer="Arun Kather Ashala <arunkatherashala@gmail.com>"
LABEL description="KORE Binary Format - Multi-language Library Reference"
LABEL version="1.0.0"

WORKDIR /kore

# Install build tools and language runtimes for reference
RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    git \
    rustc \
    cargo \
    python3 \
    python3-pip \
    nodejs \
    npm \
    openjdk-17-jdk-headless \
    maven \
    golang-go \
    ruby \
    mono-complete \
    && rm -rf /var/lib/apt/lists/*

# Copy library source and documentation
COPY . /kore

# Build Python wheel for reference
RUN cd /kore && python3 -m pip install wheel build && \
    (python3 -m build 2>/dev/null || echo "Note: Python build skipped if no setup.py") || true

# Document available bindings
RUN echo "=== KORE Multi-Language Library Reference ===" > /kore/BINDINGS.txt && \
    echo "" >> /kore/BINDINGS.txt && \
    echo "✅ Python: kore-fileformat (PyPI)" >> /kore/BINDINGS.txt && \
    echo "✅ JavaScript: kore-fileformat (npm)" >> /kore/BINDINGS.txt && \
    echo "✅ Java: com.github.arunkatherashala:kore_fileformat (Maven)" >> /kore/BINDINGS.txt && \
    echo "✅ Rust: kore_fileformat (crates.io)" >> /kore/BINDINGS.txt && \
    echo "✅ Scala, Go, C#/.NET, Ruby: Bindings available" >> /kore/BINDINGS.txt

ENTRYPOINT ["/bin/bash"]
CMD ["-c", "echo ''; echo '🎯 KORE Library Reference Container'; echo ''; cat /kore/README.md | head -30; echo ''; echo 'See /kore/BINDINGS.txt for language ecosystem info'; echo ''"]
