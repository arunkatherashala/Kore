# KORE v1.3.3 Production Deployment Procedure

**Last Updated:** June 3, 2026  
**Status:** Production Ready  
**Version:** v1.0

---

## 📋 Table of Contents

1. [Pre-Deployment Checklist](#pre-deployment-checklist)
2. [Deployment Strategies](#deployment-strategies)
3. [Docker Deployment](#docker-deployment)
4. [Kubernetes Deployment](#kubernetes-deployment)
5. [Cloud Deployment](#cloud-deployment)
6. [Configuration Management](#configuration-management)
7. [Monitoring & Logging](#monitoring--logging)
8. [Troubleshooting](#troubleshooting)

---

## Pre-Deployment Checklist

### System Requirements

| Component | Minimum | Recommended | Production |
|-----------|---------|-------------|-----------|
| CPU | 2 cores | 4 cores | 8+ cores |
| RAM | 2 GB | 4 GB | 16+ GB |
| Storage | 10 GB | 50 GB | 500+ GB |
| Network | 1 Mbps | 100 Mbps | 1+ Gbps |
| OS | Windows/Linux | Ubuntu 20.04+ | Ubuntu 22.04+ |

### Pre-Deployment Verification

```powershell
# 1. Verify KORE build
cd "c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore"
cargo build --release
# Expected: Finished release profile [optimized]

# 2. Run full test suite
cargo test --lib --release
# Expected: test result: ok. 685 passed; 0 failed; 0 ignored

# 3. Generate documentation
cargo doc --no-deps --release

# 4. Verify git status
git status
# Expected: working tree clean

# 5. Verify version
grep version Cargo.toml | head -1
# Expected: version = "1.3.3"
```

### Deployment Artifacts

```powershell
# Create deployment directory
mkdir -p deployment/kore-v1.3.3

# Copy release binary
Copy-Item "target/release/kore.exe" "deployment/kore-v1.3.3/"

# Copy configuration
Copy-Item "config.yaml" "deployment/kore-v1.3.3/"

# Copy setup guides
Copy-Item "SETUP_*.md" "deployment/kore-v1.3.3/"

# Copy release notes
Copy-Item "RELEASE_v1.3.3.md" "deployment/kore-v1.3.3/"

# Verify
Get-ChildItem "deployment/kore-v1.3.3/"
```

---

## Deployment Strategies

### Strategy 1: Standalone Binary (Simple)

**Best For:** Small deployments, testing environments

```powershell
# 1. Copy binary to server
scp target/release/kore.exe user@server:/opt/kore/

# 2. Set permissions
ssh user@server "chmod +x /opt/kore/kore"

# 3. Create data directory
ssh user@server "mkdir -p /data/kore"

# 4. Start service
ssh user@server "/opt/kore/kore --config /etc/kore/config.yaml"
```

### Strategy 2: Systemd Service (Linux)

**Best For:** Production Linux deployments

```bash
# 1. Create systemd service file
sudo tee /etc/systemd/system/kore.service > /dev/null <<EOF
[Unit]
Description=KORE v1.3.3 Database Engine
After=network.target

[Service]
Type=simple
User=kore
WorkingDirectory=/opt/kore
ExecStart=/opt/kore/kore --config /etc/kore/config.yaml
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

# 2. Enable and start service
sudo systemctl daemon-reload
sudo systemctl enable kore
sudo systemctl start kore

# 3. Verify
sudo systemctl status kore
```

### Strategy 3: Container Deployment (Docker)

**Best For:** Multi-environment consistency

[See Docker Deployment section below]

### Strategy 4: Orchestrated Deployment (Kubernetes)

**Best For:** Large-scale, high-availability deployments

[See Kubernetes Deployment section below]

---

## Docker Deployment

### Step 1: Create Dockerfile

**Dockerfile:**
```dockerfile
# Build stage
FROM rust:1.75-slim as builder

WORKDIR /build

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy source code
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build release binary
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /build/target/release/kore /app/

# Create data directory
RUN mkdir -p /data/kore

# Expose API port
EXPOSE 8000

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/health || exit 1

# Default command
CMD ["/app/kore", "--config", "/app/config.yaml"]
```

### Step 2: Build Docker Image

```powershell
# Navigate to KORE directory
cd "c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore"

# Build image
docker build -t kore:1.3.3 -t kore:latest .

# Verify
docker images | findstr kore
```

### Step 3: Run Docker Container

```powershell
# Basic run
docker run -d `
  --name kore-prod `
  -p 8000:8000 `
  -v /data/kore:/data/kore `
  -v /etc/kore/config.yaml:/app/config.yaml:ro `
  kore:1.3.3

# With environment variables
docker run -d `
  --name kore-prod `
  -p 8000:8000 `
  -e RUST_LOG=info `
  -e KORE_DATA_DIR=/data/kore `
  -v /data/kore:/data/kore `
  kore:1.3.3

# Verify container
docker ps | findstr kore
docker logs kore-prod
```

### Step 4: Docker Compose (Multiple Services)

**docker-compose.yml:**
```yaml
version: '3.9'

services:
  kore:
    image: kore:1.3.3
    container_name: kore-engine
    ports:
      - "8000:8000"
    volumes:
      - /data/kore:/data/kore
      - ./config.yaml:/app/config.yaml:ro
    environment:
      RUST_LOG: info
      KORE_DATA_DIR: /data/kore
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8000/health"]
      interval: 30s
      timeout: 10s
      retries: 3

  postgres:
    image: postgres:15-alpine
    container_name: kore-postgres
    ports:
      - "5432:5432"
    environment:
      POSTGRES_DB: kore_metadata
      POSTGRES_USER: kore_user
      POSTGRES_PASSWORD: ${DB_PASSWORD}
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./init.sql:/docker-entrypoint-initdb.d/init.sql
    restart: unless-stopped

  redis:
    image: redis:7-alpine
    container_name: kore-redis
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data
    restart: unless-stopped

volumes:
  postgres_data:
  redis_data:
```

**Start Services:**
```powershell
# Set environment variables
$env:DB_PASSWORD = "secure_password_here"

# Start all services
docker-compose up -d

# Verify
docker-compose ps

# View logs
docker-compose logs -f kore
```

---

## Kubernetes Deployment

### Step 1: Create Kubernetes Manifests

**kore-deployment.yaml:**
```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: kore-engine
  namespace: default
  labels:
    app: kore
    version: v1.3.3
spec:
  replicas: 3
  selector:
    matchLabels:
      app: kore
  template:
    metadata:
      labels:
        app: kore
        version: v1.3.3
    spec:
      containers:
      - name: kore
        image: kore:1.3.3
        imagePullPolicy: IfNotPresent
        ports:
        - containerPort: 8000
          name: api
          protocol: TCP
        env:
        - name: RUST_LOG
          value: "info"
        - name: KORE_DATA_DIR
          value: "/data/kore"
        resources:
          requests:
            memory: "512Mi"
            cpu: "500m"
          limits:
            memory: "2Gi"
            cpu: "2000m"
        volumeMounts:
        - name: data
          mountPath: /data/kore
        - name: config
          mountPath: /app/config.yaml
          subPath: config.yaml
        livenessProbe:
          httpGet:
            path: /health
            port: 8000
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /ready
            port: 8000
          initialDelaySeconds: 5
          periodSeconds: 5
      volumes:
      - name: data
        persistentVolumeClaim:
          claimName: kore-data-pvc
      - name: config
        configMap:
          name: kore-config
```

**kore-service.yaml:**
```yaml
apiVersion: v1
kind: Service
metadata:
  name: kore-service
  namespace: default
  labels:
    app: kore
spec:
  type: LoadBalancer
  ports:
  - port: 8000
    targetPort: 8000
    protocol: TCP
    name: api
  selector:
    app: kore
```

### Step 2: Deploy to Kubernetes

```bash
# Create namespace (optional)
kubectl create namespace kore-prod

# Apply manifests
kubectl apply -f kore-deployment.yaml
kubectl apply -f kore-service.yaml
kubectl apply -f kore-config.yaml
kubectl apply -f kore-pvc.yaml

# Verify deployment
kubectl get deployments
kubectl get services
kubectl get pods

# View logs
kubectl logs -f deployment/kore-engine

# Scale deployment
kubectl scale deployment kore-engine --replicas=5
```

---

## Cloud Deployment

### Azure Deployment

**Step 1: Prepare Azure Resources**

```powershell
# Login to Azure
az login

# Create resource group
az group create `
  --name kore-rg `
  --location eastus

# Create container registry
az acr create `
  --resource-group kore-rg `
  --name koreregistry `
  --sku Basic

# Build and push Docker image
az acr build `
  --registry koreregistry `
  --image kore:1.3.3 .

# Create App Service plan
az appservice plan create `
  --name kore-plan `
  --resource-group kore-rg `
  --sku B2 `
  --is-linux

# Create web app
az webapp create `
  --resource-group kore-rg `
  --plan kore-plan `
  --name kore-prod `
  --deployment-container-image-name koreregistry.azurecr.io/kore:1.3.3
```

### AWS Deployment (ECS)

**Step 1: Push to ECR**

```bash
# Login to ECR
aws ecr get-login-password | docker login --username AWS --password-stdin 123456789.dkr.ecr.us-east-1.amazonaws.com

# Tag image
docker tag kore:1.3.3 123456789.dkr.ecr.us-east-1.amazonaws.com/kore:1.3.3

# Push image
docker push 123456789.dkr.ecr.us-east-1.amazonaws.com/kore:1.3.3

# Create ECS task definition
aws ecs register-task-definition \
  --family kore-task \
  --container-definitions file://task-definition.json

# Create ECS service
aws ecs create-service \
  --cluster kore-cluster \
  --service-name kore-service \
  --task-definition kore-task:1 \
  --desired-count 3
```

### GCP Deployment (Cloud Run)

```bash
# Build and push to Container Registry
gcloud builds submit --tag gcr.io/PROJECT-ID/kore:1.3.3

# Deploy to Cloud Run
gcloud run deploy kore \
  --image gcr.io/PROJECT-ID/kore:1.3.3 \
  --platform managed \
  --region us-central1 \
  --memory 2Gi \
  --cpu 2 \
  --timeout 3600
```

---

## Configuration Management

### Environment-Specific Configs

**config.prod.yaml:**
```yaml
kore:
  version: "1.3.3"
  data_dir: "/data/kore"
  log_level: "warn"

api:
  host: "0.0.0.0"
  port: 8000
  workers: 16
  timeout: 60s

database:
  postgres:
    host: "postgres.prod.internal"
    port: 5432
    max_connections: 100
    ssl: true

cache:
  redis:
    host: "redis.prod.internal"
    port: 6379
    ttl: 3600

security:
  tls: true
  cert_path: "/etc/kore/certs/server.crt"
  key_path: "/etc/kore/certs/server.key"
  rate_limit: 10000
```

---

## Monitoring & Logging

### Health Checks

```bash
# API health check
curl -X GET http://localhost:8000/health

# Expected response:
# {
#   "status": "healthy",
#   "version": "1.3.3",
#   "timestamp": "2026-06-03T10:30:00Z"
# }
```

---

## Post-Deployment Verification

```bash
#!/bin/bash
# deployment-verify.sh

echo "=== KORE v1.3.3 Deployment Verification ==="

# 1. Health check
echo "1. Health Check..."
curl -f http://localhost:8000/health || exit 1

# 2. Database connectivity
echo "2. Database Check..."
psql -c "SELECT 1" || exit 1

# 3. Cache connectivity
echo "3. Cache Check..."
redis-cli ping || exit 1

# 4. Verify binary
echo "5. Binary Verification..."
/opt/kore/kore --version

echo ""
echo "✅ All checks passed!"
echo "KORE v1.3.3 is ready for production"
```

---

## Rollback Procedure

```bash
# If issues occur, rollback to previous version

# Docker rollback
docker pull kore:1.3.2
docker stop kore-prod
docker run -d --name kore-prod kore:1.3.2

# Kubernetes rollback
kubectl rollout undo deployment/kore-engine

# Verify rollback
kubectl rollout status deployment/kore-engine
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Complete production deployment procedure for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Support:** For issues, refer to RELEASE_v1.3.3.md and INTEGRATION_GUIDE.md
