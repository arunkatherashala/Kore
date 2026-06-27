#!/bin/bash

# Azure Deployment Script for Kore Cloud
# Usage: ./deploy-azure.sh <subscription-id>

SUBSCRIPTION_ID=${1:-""}
REGION="eastus"
RG_NAME="kore-cloud-rg"
ACR_NAME="korefileformat"
CONTAINER_APP_NAME="kore-cloud-azure"
ENV_NAME="kore-env"

echo "🚀 Kore Cloud Azure Deployment Script"
echo "=========================================="

# 1. Set subscription
if [ -n "$SUBSCRIPTION_ID" ]; then
  echo "Setting subscription to $SUBSCRIPTION_ID..."
  az account set --subscription "$SUBSCRIPTION_ID"
fi

# 2. Create resource group
echo "Creating resource group..."
az group create \
  --name "$RG_NAME" \
  --location "$REGION" || echo "Resource group already exists"

# 3. Create container registry
echo "Creating container registry..."
az acr create \
  --resource-group "$RG_NAME" \
  --name "$ACR_NAME" \
  --sku Basic \
  --admin-enabled true || echo "ACR already exists"

# 4. Build and push image
echo "Building and pushing Docker image..."
az acr build \
  --registry "$ACR_NAME" \
  --image kore-cloud:latest \
  ../../ || echo "Build skipped"

# 5. Create Container App environment
echo "Creating Container App environment..."
az containerapp env create \
  --name "$ENV_NAME" \
  --resource-group "$RG_NAME" \
  --location "$REGION" || echo "Environment already exists"

# 6. Deploy to Container Apps
echo "Deploying to Container Apps..."
az containerapp create \
  --name "$CONTAINER_APP_NAME" \
  --resource-group "$RG_NAME" \
  --environment "$ENV_NAME" \
  --image "${ACR_NAME}.azurecr.io/kore-cloud:latest" \
  --target-port 8000 \
  --ingress external \
  --registry-server "${ACR_NAME}.azurecr.io" \
  --registry-username "$(az acr credential show -n $ACR_NAME --query username -o tsv)" \
  --registry-password "$(az acr credential show -n $ACR_NAME --query passwords[0].value -o tsv)" \
  --cpu 0.5 \
  --memory 1.0Gi || echo "Container app already exists"

# 7. Get FQDN
echo "Retrieving service URL..."
FQDN=$(az containerapp show \
  --name "$CONTAINER_APP_NAME" \
  --resource-group "$RG_NAME" \
  --query properties.configuration.ingress.fqdn -o tsv)

echo ""
echo "✅ Azure Deployment Complete!"
echo "================================"
echo "Service URL: https://$FQDN/health"
echo "Region: $REGION"
echo "Resource Group: $RG_NAME"
echo ""
