# KORE v0.4.0 Deployment Script (PowerShell)
# Run this after Docker is installed

$ErrorActionPreference = "Stop"

Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "🚀 KORE v0.4.0 Deployment Script" -ForegroundColor Green
Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

# Check Docker installation
Write-Host "📦 Checking Docker installation..." -ForegroundColor Yellow

try {
    $dockerVersion = docker --version
    Write-Host "✅ Docker found: $dockerVersion" -ForegroundColor Green
} catch {
    Write-Host "❌ Docker is not installed. Please install Docker Desktop:" -ForegroundColor Red
    Write-Host "   https://www.docker.com/products/docker-desktop/" -ForegroundColor Red
    exit 1
}

try {
    $composeVersion = docker-compose --version
    Write-Host "✅ docker-compose found: $composeVersion" -ForegroundColor Green
} catch {
    Write-Host "❌ docker-compose is not installed. Please ensure Docker Desktop is properly installed." -ForegroundColor Red
    exit 1
}

Write-Host ""

# Navigate to project directory
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $scriptDir

Write-Host "📂 Working directory: $(Get-Location)" -ForegroundColor Yellow
Write-Host ""

# Check docker-compose.yml exists
if (-not (Test-Path "docker-compose.yml")) {
    Write-Host "❌ docker-compose.yml not found!" -ForegroundColor Red
    exit 1
}

Write-Host "✅ docker-compose.yml found" -ForegroundColor Green
Write-Host ""

# Stop and remove old containers
Write-Host "🛑 Cleaning up old containers..." -ForegroundColor Yellow
try {
    docker-compose down 2>$null
} catch {
    # Ignore errors on first run
}
Write-Host "✅ Cleanup complete" -ForegroundColor Green
Write-Host ""

# Build images
Write-Host "🔨 Building Docker images..." -ForegroundColor Yellow
docker-compose build --no-cache
Write-Host "✅ Build complete" -ForegroundColor Green
Write-Host ""

# Start services
Write-Host "🚀 Starting services..." -ForegroundColor Yellow
docker-compose up -d
Write-Host "✅ Services started" -ForegroundColor Green
Write-Host ""

# Wait for services to be ready
Write-Host "⏳ Waiting for services to be healthy..." -ForegroundColor Yellow
Start-Sleep -Seconds 5

# Check service health
Write-Host "📊 Service status:" -ForegroundColor Yellow
docker-compose ps

Write-Host ""
Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "✅ Deployment Complete!" -ForegroundColor Green
Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

Write-Host "📍 Service URLs:" -ForegroundColor Yellow
Write-Host "   • KORE Query Engine: http://localhost:8080" -ForegroundColor White
Write-Host "   • Health Check: http://localhost:8080/health" -ForegroundColor White
Write-Host "   • Prometheus: http://localhost:9090" -ForegroundColor White
Write-Host "   • Grafana: http://localhost:3000 (admin/admin)" -ForegroundColor White
Write-Host ""

# Test health endpoint
Write-Host "🏥 Testing health endpoint..." -ForegroundColor Yellow
Start-Sleep -Seconds 2

try {
    $health = Invoke-WebRequest -Uri "http://localhost:8080/health" -ErrorAction SilentlyContinue
    if ($health) {
        Write-Host "✅ Health check response received:" -ForegroundColor Green
        Write-Host "   $($health.Content)" -ForegroundColor White
    } else {
        Write-Host "⚠️  Health endpoint not responding yet. Containers may still be starting." -ForegroundColor Yellow
        Write-Host "   Try again in a few seconds with: curl http://localhost:8080/health" -ForegroundColor White
    }
} catch {
    Write-Host "⚠️  Could not test health endpoint. Containers may still be starting." -ForegroundColor Yellow
    Write-Host "   Try again in a few seconds with: Invoke-WebRequest http://localhost:8080/health" -ForegroundColor White
}

Write-Host ""
Write-Host "📖 Next steps:" -ForegroundColor Yellow
Write-Host "   1. Open Grafana: http://localhost:3000" -ForegroundColor White
Write-Host "   2. Login with admin/admin" -ForegroundColor White
Write-Host "   3. Add Prometheus data source: http://prometheus:9090" -ForegroundColor White
Write-Host "   4. Create dashboards and start monitoring" -ForegroundColor White
Write-Host ""

Write-Host "🛑 To stop services: docker-compose down" -ForegroundColor Cyan
Write-Host "📋 To view logs: docker-compose logs -f" -ForegroundColor Cyan
Write-Host ""
