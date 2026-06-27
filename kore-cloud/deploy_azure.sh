#!/bin/bash
# Azure Deployment Script
# Usage: ./deploy_azure.sh [environment] [region]

set -e

ENVIRONMENT=${1:-prod}
REGION=${2:-eastus}
SUBSCRIPTION_ID=$(az account show --query id --output tsv)
RESOURCE_GROUP="rg-kore-$ENVIRONMENT"

echo "🚀 Deploying Kore Cloud to Azure"
echo "   Environment: $ENVIRONMENT"
echo "   Region: $REGION"
echo "   Subscription: $SUBSCRIPTION_ID"
echo ""

# Step 1: Create resource group
echo "📁 Creating resource group..."
az group create --name $RESOURCE_GROUP --location $REGION

# Step 2: Build Docker image
echo "📦 Building Docker image..."
docker build -t kore-cloud:latest .

# Step 3: Create container registry
echo "📁 Creating container registry..."
ACR_NAME="korereg${ENVIRONMENT}$(echo $SUBSCRIPTION_ID | cut -c1-8)"
ACR_RESOURCE_GROUP=$RESOURCE_GROUP

if ! az acr show --resource-group $ACR_RESOURCE_GROUP --name $ACR_NAME 2>/dev/null; then
    echo "   Creating new ACR..."
    az acr create \
        --resource-group $ACR_RESOURCE_GROUP \
        --name $ACR_NAME \
        --sku Standard
fi

ACR_LOGIN_SERVER=$(az acr show \
    --resource-group $ACR_RESOURCE_GROUP \
    --name $ACR_NAME \
    --query loginServer \
    --output tsv)

# Step 4: Login to ACR
echo "🔑 Logging in to ACR..."
az acr login --name $ACR_NAME

# Step 5: Tag and push image
echo "📤 Pushing image to ACR..."
docker tag kore-cloud:latest $ACR_LOGIN_SERVER/kore-cloud:latest
docker tag kore-cloud:latest $ACR_LOGIN_SERVER/kore-cloud:$(date +%Y%m%d-%H%M%S)
docker push $ACR_LOGIN_SERVER/kore-cloud:latest

# Step 6: Generate password
echo "🔐 Generating database password..."
DB_PASSWORD=$(openssl rand -base64 32)
echo "   Password: $DB_PASSWORD (SAVE THIS!)"

# Step 7: Create terraform.tfvars
echo "⚙️  Creating terraform.tfvars..."
cd terraform/azure

cat > terraform.tfvars <<EOF
azure_subscription_id = "$SUBSCRIPTION_ID"
azure_region          = "$REGION"
environment           = "$ENVIRONMENT"
db_password           = "$DB_PASSWORD"
log_level             = "info"
EOF

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
CONTAINER_FQDN=$(terraform output -raw container_group_fqdn)
POSTGRES_FQDN=$(terraform output -raw postgres_fqdn)
STORAGE_ACCOUNT=$(terraform output -raw storage_account_name)
ACR_SERVER=$(terraform output -raw container_registry_login_server)

echo "   Container FQDN: $CONTAINER_FQDN"
echo "   PostgreSQL FQDN: $POSTGRES_FQDN"
echo "   Storage Account: $STORAGE_ACCOUNT"
echo "   ACR Server: $ACR_SERVER"

echo ""
echo "🔍 Testing API..."
sleep 10
if curl -H "Content-Type: application/json" http://$CONTAINER_FQDN:8000/api/v1/status 2>/dev/null | grep -q "ok"; then
    echo "✅ API is responding!"
else
    echo "⏳ API still initializing, check status with:"
    echo "   curl http://$CONTAINER_FQDN:8000/api/v1/status"
fi

echo ""
echo "📝 Next steps:"
echo "   1. Configure DNS to point to: $CONTAINER_FQDN"
echo "   2. Monitor logs: az container logs --resource-group $RESOURCE_GROUP --name kore-cloud-$ENVIRONMENT"
echo "   3. Access container: az container exec --resource-group $RESOURCE_GROUP --name kore-cloud-$ENVIRONMENT --exec-command /bin/sh"

cd ../..
