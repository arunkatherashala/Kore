# KORE Binary Format - Lightweight Runtime (pre-built binaries)
FROM debian:bookworm-slim

LABEL maintainer="Arun Kather Ashala <arunkatherashala@gmail.com>"
LABEL description="KORE Binary Format - Lightweight Runtime"
LABEL version="1.0.0"

WORKDIR /app

# Install minimal runtime dependencies
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy pre-built binaries from build stage
COPY ./target/release/kore-fileformat /usr/local/bin/kore-fileformat
RUN chmod +x /usr/local/bin/kore-fileformat

# Copy supporting files
COPY ./src /app/src
COPY ./README.md /app/

ENTRYPOINT ["kore-fileformat"]
