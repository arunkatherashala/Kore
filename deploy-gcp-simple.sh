#!/bin/bash

# Simplified GCP Deployment for Kore Cloud (using Cloud Build)
# Usage: ./deploy-gcp-simple.sh <project-id>

PROJECT_ID=${1:-""}
REGION="us-central1"
SERVICE_NAME="kore-cloud"

if [ -z "$PROJECT_ID" ]; then
  echo "❌ Usage: ./deploy-gcp-simple.sh <project-id>"
  echo "Example: ./deploy-gcp-simple.sh my-kore-project"
  exit 1
fi

echo "🚀 Kore Cloud GCP Deployment (Cloud Build)"
echo "=========================================="

# 1. Set project
echo "Setting project to $PROJECT_ID..."
gcloud config set project "$PROJECT_ID"

# 2. Enable APIs
echo "Enabling required APIs..."
gcloud services enable \
  cloudbuild.googleapis.com \
  run.googleapis.com \
  storage-api.googleapis.com \
  containerregistry.googleapis.com 2>/dev/null || true

# 3. Build using Cloud Build (no local Docker needed!)
echo "Building Docker image using Cloud Build..."
gcloud builds submit \
  --tag "gcr.io/$PROJECT_ID/kore-cloud:latest" \
  ./kore-cloud/ \
  --timeout 1800s

# 4. Deploy to Cloud Run
echo "Deploying to Cloud Run..."
gcloud run deploy "$SERVICE_NAME" \
  --image "gcr.io/$PROJECT_ID/kore-cloud:latest" \
  --platform managed \
  --region "$REGION" \
  --port 8000 \
  --memory 512Mi \
  --timeout 3600 \
  --allow-unauthenticated \
  --set-env-vars "STORAGE_BACKEND=GCS,GCS_BUCKET=gs://kore-cloud-$PROJECT_ID"

# 5. Create Cloud Storage bucket
echo "Creating Cloud Storage bucket..."
gsutil mb -l "$REGION" "gs://kore-cloud-$PROJECT_ID" 2>/dev/null || echo "Bucket already exists"

# 6. Get service URL
SERVICE_URL=$(gcloud run services describe "$SERVICE_NAME" \
  --region "$REGION" \
  --format 'value(status.url)' 2>/dev/null)

echo ""
echo "✅ GCP Deployment Complete!"
echo "============================"
echo "Service URL: $SERVICE_URL/health"
echo "Region: $REGION"
echo "Project: $PROJECT_ID"
echo ""
echo "Test health check:"
echo "  curl $SERVICE_URL/health"
echo ""
