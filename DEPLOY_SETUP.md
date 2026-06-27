# KORE v0.4.0 Deployment Setup Guide

## Current Status
✅ **Build**: Complete and tested  
✅ **Tests**: 176/176 passing (100%)  
✅ **Code**: Production-ready, zero unsafe blocks  
❌ **Docker**: Not currently installed on system  

---

## Prerequisites

### 1. Install Docker Desktop
**Windows**: [Download Docker Desktop](https://www.docker.com/products/docker-desktop/)
- Requires Windows 10/11 Professional or Enterprise
- Or use WSL 2 (Windows Subsystem for Linux 2)
- Verify: `docker --version` and `docker-compose --version`

### 2. System Requirements
- **CPU**: 2+ cores (4 recommended)
- **Memory**: 4GB (8GB for production)
- **Disk**: 20GB free space
- **Network**: For health checks and metrics collection

---

## Quick Start (Once Docker Installed)

### Step 1: Navigate to KORE Directory
```bash
cd c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore
```

### Step 2: Start All Services
```bash
docker-compose up -d
```

### Step 3: Verify Services
```bash
docker-compose ps
```

Expected output:
```
NAME                    STATUS
kore-query-engine       Up (healthy)
prometheus              Up
grafana                 Up
```

### Step 4: Test Health Endpoint
```bash
curl http://localhost:8080/health
```

Expected response:
```json
{
  "status": "healthy",
  "uptime_secs": 5,
  "queries_processed": 0,
  "errors": 0,
  "memory_mb": 128
}
```

---

## Service Access

Once running:

| Service | URL | Purpose |
|---------|-----|---------|
| KORE Query Engine | http://localhost:8080 | Query execution |
| Health Check | http://localhost:8080/health | Service status |
| Prometheus | http://localhost:9090 | Metrics storage |
| Grafana | http://localhost:3000 | Dashboards & visualization |

**Grafana Credentials**:
- Username: `admin`
- Password: `admin`

---

## Deployment Commands

### Start All Services
```bash
docker-compose up -d
```

### Stop All Services
```bash
docker-compose down
```

### View Logs
```bash
# All services
docker-compose logs -f

# Specific service
docker-compose logs -f kore-query-engine
```

### Rebuild Images
```bash
docker-compose build --no-cache
```

### Restart Services
```bash
docker-compose restart
```

---

## Configuration

### Environment Variables
Edit `docker-compose.yml` to customize:

```yaml
environment:
  - RUST_LOG=info          # Change to debug for verbose logging
  - KORE_ENV=production    # Or: development, staging
  - KORE_PORT=8080         # Change port if needed
  - KORE_HOST=0.0.0.0      # Bind address
```

### Resource Limits
Modify container resources in `docker-compose.yml`:

```yaml
mem_limit: 4g              # Max memory per container
cpus: '2'                  # Max CPU cores
```

---

## Health Checks

### Verify Service Health
```bash
curl -v http://localhost:8080/health
```

### Check Docker Container Health
```bash
docker-compose ps
```
Look for `(healthy)` status.

### View Container Stats
```bash
docker stats kore-query-engine
```

---

## Monitoring

### Prometheus Dashboard
1. Open http://localhost:9090
2. Metrics should appear from `kore-query-engine`
3. Query examples:
   - `kore_queries_total` - Total queries processed
   - `rate(kore_query_duration_ms[5m])` - Query rate

### Grafana Dashboards
1. Open http://localhost:3000
2. Login with admin/admin
3. Add Prometheus data source: http://prometheus:9090
4. Create dashboard with metrics

---

## Troubleshooting Deployment

### Docker Not Found
```bash
# Check if Docker is installed
docker --version
docker-compose --version

# If missing, reinstall Docker Desktop
```

### Port Already in Use
```bash
# Find what's using port 8080
netstat -ano | findstr :8080

# Or use docker-compose on different port
# Edit docker-compose.yml: "8081:8080"
```

### Container Fails to Start
```bash
# Check logs
docker-compose logs kore-query-engine

# Rebuild and restart
docker-compose down
docker-compose build --no-cache
docker-compose up -d
```

### Health Check Failing
```bash
# View service logs
docker logs kore-query-engine

# Manual health check
curl http://localhost:8080/health

# If unhealthy: check error rate, memory usage
```

---

## Next Steps

1. **Install Docker Desktop**: [https://www.docker.com/products/docker-desktop/](https://www.docker.com/products/docker-desktop/)
2. **Start Services**: `docker-compose up -d`
3. **Verify Health**: `curl http://localhost:8080/health`
4. **Access Monitoring**: Open http://localhost:3000 (Grafana)
5. **Run Tests**: Execute sample queries via health endpoint

---

## Advanced Deployment

### Kubernetes (Production)
For Kubernetes deployment, create deployment manifest:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: kore-query-engine
spec:
  replicas: 3
  selector:
    matchLabels:
      app: kore
  template:
    metadata:
      labels:
        app: kore
    spec:
      containers:
      - name: kore
        image: kore:0.4.0
        ports:
        - containerPort: 8080
        env:
        - name: RUST_LOG
          value: info
        resources:
          requests:
            memory: "2Gi"
            cpu: "1"
          limits:
            memory: "4Gi"
            cpu: "2"
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
```

### Cloud Deployment (AWS, Azure, GCP)
Services available:
- **AWS ECS**: Container orchestration
- **Azure Container Instances**: Serverless containers
- **GCP Cloud Run**: Serverless execution
- **All Support**: Load balancing, auto-scaling, monitoring

See [DEPLOYMENT.md](DEPLOYMENT.md) for detailed cloud setup.

---

## Support

For issues:
1. Check [TROUBLESHOOTING.md](TROUBLESHOOTING.md)
2. Review [DEPLOYMENT.md](DEPLOYMENT.md)
3. Check Docker logs: `docker-compose logs`
4. Verify health: `curl http://localhost:8080/health`
