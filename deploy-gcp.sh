#!/bin/bash

# GCP Deployment Script for Kore Cloud
# Usage: ./deploy-gcp.sh <project-id>

PROJECT_ID=${1:-"kore-cloud-gcp"}
REGION="us-central1"
SERVICE_NAME="kore-cloud"
IMAGE_NAME="kore-cloud"

echo "🚀 Kore Cloud GCP Deployment Script"
echo "====================================="

# 1. Set project
echo "Setting GCP project to $PROJECT_ID..."
gcloud config set project "$PROJECT_ID"

# 2. Enable required APIs
echo "Enabling required APIs..."
gcloud services enable \
  containerregistry.googleapis.com \
  run.googleapis.com \
  storage-api.googleapis.com

# 3. Build image locally
echo "Building Docker image..."
docker build -t gcr.io/$PROJECT_ID/$IMAGE_NAME:latest ../../kore-cloud/

# 4. Configure Docker for GCP
echo "Authenticating with GCP..."
gcloud auth configure-docker

# 5. Push to Container Registry
echo "Pushing image to GCP Container Registry..."
docker push gcr.io/$PROJECT_ID/$IMAGE_NAME:latest

# 6. Deploy to Cloud Run
echo "Deploying to Cloud Run..."
gcloud run deploy "$SERVICE_NAME" \
  --image gcr.io/$PROJECT_ID/$IMAGE_NAME:latest \
  --platform managed \
  --region "$REGION" \
  --port 8000 \
  --memory 512Mi \
  --timeout 3600 \
  --allow-unauthenticated \
  --set-env-vars "STORAGE_BACKEND=GCS,GCS_BUCKET=gs://kore-cloud-files" || echo "Service may already exist"

# 7. Create Cloud Storage bucket
echo "Creating Cloud Storage bucket..."
gsutil mb -l "$REGION" -b on "gs://kore-cloud-files-$PROJECT_ID" 2>/dev/null || echo "Bucket already exists"

# 8. Get service URL
echo "Retrieving service URL..."
SERVICE_URL=$(gcloud run services describe "$SERVICE_NAME" \
  --region "$REGION" \
  --format 'value(status.url)')

echo ""
echo "✅ GCP Deployment Complete!"
echo "============================"
echo "Service URL: $SERVICE_URL/health"
echo "Region: $REGION"
echo "Project: $PROJECT_ID"
echo ""
