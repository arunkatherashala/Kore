# Docker Setup & Cloud Emulators Guide

## Overview

This guide provides complete instructions for using Docker to run cloud storage emulators (LocalStack for S3, Azurite for Azure, GCS Emulator) alongside Kore for local development and testing.

---

## 🐳 Docker Basics for Kore Development

### What is Docker?

Docker is a containerization platform that lets you run cloud services (S3, Azure, GCS) locally without needing actual cloud accounts. Think of it as a lightweight virtual machine that runs just the services you need.

### Why Docker for Kore?

- **No Cloud Costs**: Test against real APIs without paying for cloud services
- **Fast Feedback**: Instant feedback during development
- **Reproducible**: Same environment across team members and CI/CD
- **Isolated**: Emulators run in isolated containers, won't interfere with your system

---

## 📦 Installation

### Windows

1. **Download Docker Desktop**
   - Visit: https://www.docker.com/products/docker-desktop
   - Click "Download for Windows"
   - System Requirements: Windows 10/11 Pro, Enterprise, or Education (with WSL2)

2. **Install**
   ```powershell
   # Run the installer
   # Follow on-screen prompts
   # Restart computer when prompted
   ```

3. **Verify Installation**
   ```powershell
   docker --version
   # Output: Docker version 29.4.2, build 055a478
   
   docker run hello-world
   # Should display "Hello from Docker!"
   ```

### macOS

1. **Download Docker Desktop**
   - Visit: https://www.docker.com/products/docker-desktop
   - Choose Intel or Apple Silicon version
   - Click "Download"

2. **Install**
   ```bash
   # Open DMG and drag Docker to Applications
   # Launch Docker from Applications
   ```

3. **Verify**
   ```bash
   docker --version
   docker run hello-world
   ```

### Linux

```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get install docker.io docker-compose

# Start Docker daemon
sudo systemctl start docker

# Verify
docker --version
docker run hello-world
```

---

## 🚀 Quick Start: Run All Cloud Emulators

### Option 1: Docker Compose (Recommended)

Create `docker-compose.yml` in your Kore repo root:

```yaml
version: '3.8'

services:
  # LocalStack for AWS S3 emulation
  localstack:
    image: localstack/localstack:latest
    container_name: localstack-s3
    ports:
      - "4566:4566"  # LocalStack services endpoint
    environment:
      SERVICES: s3
      DEBUG: 1
      DOCKER_HOST: unix:///var/run/docker.sock
    volumes:
      - "${TMPDIR}:/tmp/localstack"
      - "/var/run/docker.sock:/var/run/docker.sock"
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:4566/_localstack/health"]
      interval: 10s
      timeout: 5s
      retries: 5

  # Azurite for Azure Blob Storage emulation
  azurite:
    image: mcr.microsoft.com/azure-storage/azurite
    container_name: azurite-blob
    ports:
      - "10000:10000"  # Blob Storage
      - "10001:10001"  # Queue Storage
      - "10002:10002"  # Table Storage
    environment:
      AZURITE_ACCOUNTS: "devstoreaccount1:Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw=="
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:10000"]
      interval: 10s
      timeout: 5s
      retries: 5

volumes:
  localstack-data:
```

**Start all emulators**:
```bash
cd /path/to/Kore
docker-compose up -d

# Check status
docker-compose ps
docker-compose logs -f localstack
docker-compose logs -f azurite
```

**Stop all emulators**:
```bash
docker-compose down
```

### Option 2: Individual Docker Commands

#### LocalStack (S3)
```bash
# Start LocalStack with S3
docker run -d \
  --name localstack-s3 \
  -p 4566:4566 \
  -e SERVICES=s3 \
  -e DEBUG=1 \
  localstack/localstack:latest

# Check if running
docker ps | grep localstack

# View logs
docker logs -f localstack-s3

# Stop
docker stop localstack-s3
docker rm localstack-s3
```

#### Azurite (Azure)
```bash
# Start Azurite
docker run -d \
  --name azurite-blob \
  -p 10000:10000 \
  -p 10001:10001 \
  -p 10002:10002 \
  mcr.microsoft.com/azure-storage/azurite

# Check if running
docker ps | grep azurite

# View logs
docker logs -f azurite-blob

# Stop
docker stop azurite-blob
docker rm azurite-blob
```

#### Google Cloud Storage Emulator
```bash
# Note: GCS Emulator is more complex; alternative is to use real GCP with service account

# For gcloud emulator:
docker run -d \
  --name gcs-emulator \
  -p 4443:4443 \
  oittaa/gcp-emulator:latest

# Alternative: Use real GCP with service account JSON
```

---

## 🔌 Connectivity

### LocalStack S3 Endpoint

**From Local Machine**:
```
http://localhost:4566
```

**From Docker Container** (if connecting from another container):
```
http://localstack:4566
```

**Configure in Kore**:
```rust
// In tests or code
let s3_config = AwsConfig::builder()
    .region(Region::new("us-east-1"))
    .endpoint_url("http://localhost:4566")
    .credentials_provider(StaticCredentialsProvider::new(
        Credentials::for_tests()
    ))
    .build();

let s3_client = S3Client::new(&s3_config);
```

### Azurite Connection String

**For Local Development**:
```
DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==;BlobEndpoint=http://127.0.0.1:10000/devstoreaccount1;
```

**Configure in Kore**:
```rust
let reader = AzureBlobReader::new(
    "devstoreaccount1",
    "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw=="
);
```

---

## 🧪 Testing with Emulators

### Run Kore Integration Tests

```bash
# Ensure emulators are running
docker-compose up -d

# Run integration tests
cargo test --features s3,azure,gcs --test integration_tests -- --nocapture --test-threads=1

# Output
# running 4 tests
# test test_s3_localstack_integration ... ok
# test test_azure_azurite_integration ... ok
# test test_gcs_emulator_integration ... ok
# test test_emulator_setup_instructions ... ok
```

### Create Test Buckets/Containers

#### S3 (LocalStack)
```bash
# Connect to LocalStack
aws --endpoint-url=http://localhost:4566 s3 mb s3://test-bucket

# List buckets
aws --endpoint-url=http://localhost:4566 s3 ls

# Upload test file
echo "test data" > test.txt
aws --endpoint-url=http://localhost:4566 s3 cp test.txt s3://test-bucket/

# Download file
aws --endpoint-url=http://localhost:4566 s3 cp s3://test-bucket/test.txt .
```

#### Azure (Azurite)
```bash
# Azure CLI with Azurite
az storage container create \
  --name test-container \
  --connection-string "DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==;BlobEndpoint=http://127.0.0.1:10000/devstoreaccount1;"

# Upload blob
az storage blob upload \
  --file test.txt \
  --name test-blob \
  --container-name test-container \
  --connection-string "DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==;BlobEndpoint=http://127.0.0.1:10000/devstoreaccount1;"
```

---

## 🐛 Troubleshooting

### Container Won't Start

```bash
# Check logs
docker logs <container-name>

# Common issues:
# 1. Port already in use
docker ps  # Find what's using port 4566 or 10000

# 2. Insufficient disk space
docker system df
docker system prune  # Clean up unused images/containers

# 3. Resource limits
# Docker Desktop: Settings → Resources → increase memory/CPU
```

### Connection Refused

```bash
# Verify container is running
docker ps

# Test connectivity
curl http://localhost:4566/_localstack/health
curl http://localhost:10000

# If failing, restart container
docker restart <container-name>
```

### High Disk Usage

```bash
# Clean up Docker resources
docker system prune -a

# Remove specific container
docker rm <container-name>

# Remove image
docker rmi <image-name>
```

---

## 📊 Health Checks

### Check LocalStack Health

```bash
# Get health status
curl http://localhost:4566/_localstack/health

# Expected output:
# {
#   "services": {
#     "s3": "running"
#   },
#   "features": [...]
# }
```

### Check Azurite Health

```bash
curl -v http://127.0.0.1:10000

# Expected: 403 (authentication required) = working
```

---

## 🔄 CI/CD Integration

### GitHub Actions with Docker Compose

```yaml
name: Integration Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    
    services:
      localstack:
        image: localstack/localstack
        ports:
          - 4566:4566
        env:
          SERVICES: s3
      azurite:
        image: mcr.microsoft.com/azure-storage/azurite
        ports:
          - 10000:10000
    
    steps:
      - uses: actions/checkout@v4
      
      - uses: dtolnay/rust-toolchain@stable
      
      - name: Wait for services
        run: |
          until curl -s http://localhost:4566/_localstack/health; do sleep 1; done
          until curl -s http://127.0.0.1:10000; do sleep 1; done
      
      - name: Run tests
        run: cargo test --features s3,azure --test integration_tests
```

---

## 📚 References

- **Docker**: https://docs.docker.com
- **LocalStack**: https://docs.localstack.cloud
- **Azurite**: https://github.com/Azure/Azurite
- **GCS Emulator**: https://cloud.google.com/docs/emulator
- **AWS CLI with LocalStack**: https://docs.aws.amazon.com/cli/latest/userguide/

---

## ✅ Summary

You now have:
- ✅ Docker installed and configured
- ✅ Docker Compose for all emulators
- ✅ Integration test framework ready
- ✅ Health check procedures
- ✅ Troubleshooting guide
- ✅ CI/CD integration example

**Next**: Run `docker-compose up -d` and `cargo test --test integration_tests` to verify! 🚀
