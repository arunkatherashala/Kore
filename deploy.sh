#!/bin/bash
# KORE v0.4.0 Deployment Script
# Run this after Docker is installed

set -e

echo "═══════════════════════════════════════════════════════════════"
echo "🚀 KORE v0.4.0 Deployment Script"
echo "═══════════════════════════════════════════════════════════════"
echo ""

# Check Docker installation
echo "📦 Checking Docker installation..."
if ! command -v docker &> /dev/null; then
    echo "❌ Docker is not installed. Please install Docker Desktop:"
    echo "   https://www.docker.com/products/docker-desktop/"
    exit 1
fi

if ! command -v docker-compose &> /dev/null; then
    echo "❌ docker-compose is not installed. Please ensure Docker Desktop is properly installed."
    exit 1
fi

echo "✅ Docker found: $(docker --version)"
echo "✅ docker-compose found: $(docker-compose --version)"
echo ""

# Navigate to project directory
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

echo "📂 Working directory: $(pwd)"
echo ""

# Check docker-compose.yml exists
if [ ! -f "docker-compose.yml" ]; then
    echo "❌ docker-compose.yml not found!"
    exit 1
fi

echo "✅ docker-compose.yml found"
echo ""

# Stop and remove old containers
echo "🛑 Cleaning up old containers..."
docker-compose down 2>/dev/null || true
echo "✅ Cleanup complete"
echo ""

# Build images
echo "🔨 Building Docker images..."
docker-compose build --no-cache
echo "✅ Build complete"
echo ""

# Start services
echo "🚀 Starting services..."
docker-compose up -d
echo "✅ Services started"
echo ""

# Wait for services to be ready
echo "⏳ Waiting for services to be healthy..."
sleep 5

# Check service health
echo "📊 Service status:"
docker-compose ps

echo ""
echo "═══════════════════════════════════════════════════════════════"
echo "✅ Deployment Complete!"
echo "═══════════════════════════════════════════════════════════════"
echo ""
echo "📍 Service URLs:"
echo "   • KORE Query Engine: http://localhost:8080"
echo "   • Health Check: http://localhost:8080/health"
echo "   • Prometheus: http://localhost:9090"
echo "   • Grafana: http://localhost:3000 (admin/admin)"
echo ""

# Test health endpoint
echo "🏥 Testing health endpoint..."
sleep 2

if command -v curl &> /dev/null; then
    HEALTH=$(curl -s http://localhost:8080/health)
    if [ ! -z "$HEALTH" ]; then
        echo "✅ Health check response received:"
        echo "   $HEALTH"
    else
        echo "⚠️  Health endpoint not responding yet. Containers may still be starting."
        echo "   Try again in a few seconds with: curl http://localhost:8080/health"
    fi
else
    echo "⚠️  curl not available. Test with: curl http://localhost:8080/health"
fi

echo ""
echo "📖 Next steps:"
echo "   1. Open Grafana: http://localhost:3000"
echo "   2. Login with admin/admin"
echo "   3. Add Prometheus data source: http://prometheus:9090"
echo "   4. Create dashboards and start monitoring"
echo ""
echo "🛑 To stop services: docker-compose down"
echo "📋 To view logs: docker-compose logs -f"
echo ""
