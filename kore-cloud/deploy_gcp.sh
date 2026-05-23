#!/bin/bash
# GCP Deployment Script
# Usage: ./deploy_gcp.sh [environment] [region] [email]

set -e

ENVIRONMENT=${1:-prod}
REGION=${2:-us-central1}
ALERT_EMAIL=${3:-alerts@example.com}
PROJECT_ID=$(gcloud config get-value project)

echo "🚀 Deploying Kore Cloud to GCP"
echo "   Environment: $ENVIRONMENT"
echo "   Region: $REGION"
echo "   Project: $PROJECT_ID"
echo "   Alert Email: $ALERT_EMAIL"
echo ""

# Step 1: Enable required APIs
echo "🔧 Enabling required APIs..."
gcloud services enable \
    run.googleapis.com \
    sqladmin.googleapis.com \
    storage.googleapis.com \
    cloudkms.googleapis.com \
    artifactregistry.googleapis.com \
    compute.googleapis.com

# Step 2: Create Artifact Registry
echo "📁 Creating Artifact Registry..."
if ! gcloud artifacts repositories describe kore --location $REGION 2>/dev/null; then
    echo "   Creating new Artifact Registry repository..."
    gcloud artifacts repositories create kore \
        --repository-format docker \
        --location $REGION
fi

# Step 3: Build Docker image
echo "📦 Building Docker image..."
docker build -t kore-cloud:latest .

# Step 4: Configure Docker authentication
echo "🔑 Configuring Docker authentication..."
gcloud auth configure-docker $REGION-docker.pkg.dev

# Step 5: Tag and push image
echo "📤 Pushing image to Artifact Registry..."
AR_IMAGE="$REGION-docker.pkg.dev/$PROJECT_ID/kore/kore-cloud:latest"
docker tag kore-cloud:latest $AR_IMAGE
docker tag kore-cloud:latest $REGION-docker.pkg.dev/$PROJECT_ID/kore/kore-cloud:$(date +%Y%m%d-%H%M%S)
docker push $AR_IMAGE

# Step 6: Generate password
echo "🔐 Generating database password..."
DB_PASSWORD=$(openssl rand -base64 32)
echo "   Password: $DB_PASSWORD (SAVE THIS!)"

# Step 7: Create terraform.tfvars
echo "⚙️  Creating terraform.tfvars..."
cd terraform/gcp

cat > terraform.tfvars <<EOF
gcp_project_id = "$PROJECT_ID"
gcp_region     = "$REGION"
environment    = "$ENVIRONMENT"
db_password    = "$DB_PASSWORD"
log_level      = "info"
alert_email    = "$ALERT_EMAIL"
gcp_domain     = "example.com"  # REPLACE with your domain
EOF

echo "   ⚠️  Update terraform.tfvars with your domain name!"
echo ""

# Step 8: Terraform initialization
echo "🏗️  Initializing Terraform..."
terraform init

# Step 9: Terraform plan
echo "📋 Planning deployment..."
terraform plan -out=tfplan

# Step 10: Prompt for approval
echo ""
read -p "Proceed with deployment? (yes/no): " -n 3 -r
echo
if [[ ! $REPLY =~ ^yes$ ]]; then
    echo "Deployment cancelled."
    exit 1
fi

# Step 11: Apply deployment
echo "🚀 Applying Terraform configuration..."
terraform apply tfplan

# Step 12: Get outputs
echo ""
echo "✅ Deployment complete!"
echo ""
echo "📊 Deployment Outputs:"
CLOUD_RUN_URL=$(terraform output -raw cloud_run_url)
CLOUD_SQL_CONN=$(terraform output -raw cloud_sql_connection_name)
STORAGE_BUCKET=$(terraform output -raw storage_bucket_name)
SERVICE_ACCOUNT=$(terraform output -raw cloud_run_service_account)

echo "   Cloud Run URL: $CLOUD_RUN_URL"
echo "   Cloud SQL Connection: $CLOUD_SQL_CONN"
echo "   Storage Bucket: $STORAGE_BUCKET"
echo "   Service Account: $SERVICE_ACCOUNT"

echo ""
echo "🔍 Testing API..."
sleep 10
if curl -H "Content-Type: application/json" $CLOUD_RUN_URL/api/v1/status 2>/dev/null | grep -q "ok"; then
    echo "✅ API is responding!"
else
    echo "⏳ API still initializing, check status with:"
    echo "   curl $CLOUD_RUN_URL/api/v1/status"
fi

echo ""
echo "📝 Next steps:"
echo "   1. Configure Cloud Run domain mapping for your custom domain"
echo "   2. Monitor logs: gcloud run logs read kore-cloud-$ENVIRONMENT --limit 100"
echo "   3. Check service: gcloud run services describe kore-cloud-$ENVIRONMENT --region $REGION"

cd ../..
