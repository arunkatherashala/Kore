# Deploy Kore Cloud to GCP using Cloud Shell

## ✅ Quick Deploy (Copy & Paste)

1. **Open Cloud Shell** in GCP Console (click the `>_` icon at top-right)

2. **Copy and paste these commands:**

```bash
# Set project
PROJECT_ID="aerial-citron-493616-k7"
REGION="us-central1"
SERVICE_NAME="kore-cloud"

gcloud config set project $PROJECT_ID

# Enable APIs
gcloud services enable \
  cloudbuild.googleapis.com \
  run.googleapis.com \
  storage-api.googleapis.com \
  containerregistry.googleapis.com

# Clone kore repository
cd /tmp
git clone https://github.com/arunkatherashala/Kore.git
cd Kore

# Build using Cloud Build (no local Docker needed!)
echo "Building Docker image..."
gcloud builds submit \
  --tag "gcr.io/$PROJECT_ID/kore-cloud:latest" \
  ./kore-cloud/ \
  --timeout 1800s

# Deploy to Cloud Run
echo "Deploying to Cloud Run..."
gcloud run deploy "$SERVICE_NAME" \
  --image "gcr.io/$PROJECT_ID/kore-cloud:latest" \
  --platform managed \
  --region "$REGION" \
  --port 8000 \
  --memory 512Mi \
  --timeout 3600 \
  --allow-unauthenticated

# Create Cloud Storage bucket for files
echo "Creating Cloud Storage bucket..."
gsutil mb -l "$REGION" "gs://kore-cloud-$PROJECT_ID" 2>/dev/null || echo "Bucket already exists"

# Get service URL
SERVICE_URL=$(gcloud run services describe "$SERVICE_NAME" \
  --region "$REGION" \
  --format 'value(status.url)')

echo ""
echo "✅ DEPLOYMENT COMPLETE!"
echo "================================"
echo "Service URL: $SERVICE_URL"
echo "Health Check: $SERVICE_URL/health"
echo "Test: curl $SERVICE_URL/health"
```

## 📋 What This Does

1. **Cloud Build** - Compiles kore-cloud from source (no local Docker)
2. **Cloud Run** - Deploys as serverless service (auto-scaling)
3. **Cloud Storage** - Creates file storage bucket
4. **Returns** - Live URL to your Kore Cloud API

## ⏱️ Timeline
- **Build**: 3-5 minutes (first time)
- **Deploy**: 1-2 minutes
- **Total**: ~5 minutes

## 🔍 Monitor Progress

In Cloud Shell, you'll see:
- `Building gcr.io/aerial-citron-493616-k7/kore-cloud:latest`
- `Uploading context...`
- `Building...` (watch the logs)
- `Deploying revision...`
- Service URL appears at the end ✅
