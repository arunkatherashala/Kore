# Azure Deployment Script for Kore Cloud
# Usage: .\deploy_azure.ps1 -Environment prod -Location eastus

param(
    [string]$Environment = "prod",
    [string]$Location = "eastus"
)

$ErrorActionPreference = "Stop"

Write-Host "🚀 Deploying Kore Cloud to Azure" -ForegroundColor Cyan
Write-Host "   Environment: $Environment" -ForegroundColor Gray
Write-Host "   Location: $Location" -ForegroundColor Gray
Write-Host ""

# Step 1: Get Azure subscription
Write-Host "🔍 Getting Azure subscription..." -ForegroundColor Yellow
$subscription = az account show --query id --output tsv
if (-not $subscription) {
    Write-Host "❌ Failed to get Azure subscription. Ensure az CLI is logged in." -ForegroundColor Red
    exit 1
}
Write-Host "   Subscription ID: $subscription" -ForegroundColor Green

# Step 2: Create resource group
Write-Host "📁 Creating resource group..." -ForegroundColor Yellow
$resourceGroup = "kore-cloud-$Environment"
az group create --name $resourceGroup --location $Location
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Failed to create resource group" -ForegroundColor Red
    exit 1
}
Write-Host "   ✅ Resource group created" -ForegroundColor Green

# Step 3: Build Docker image
Write-Host "📦 Building Docker image..." -ForegroundColor Yellow
$imageName = "kore-cloud:latest"
docker build -t $imageName .
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Docker build failed" -ForegroundColor Red
    exit 1
}
Write-Host "   ✅ Docker image built successfully" -ForegroundColor Green

# Step 4: Create container registry
Write-Host "🔐 Creating Azure Container Registry..." -ForegroundColor Yellow
$registryName = "korecloud$(Get-Random -Minimum 100000 -Maximum 999999)"
az acr create --resource-group $resourceGroup --name $registryName --sku Standard --location $Location
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Failed to create container registry" -ForegroundColor Red
    exit 1
}
Write-Host "   ✅ Container Registry created: $registryName" -ForegroundColor Green

# Step 5: Login to Azure Container Registry
Write-Host "🔑 Logging in to ACR..." -ForegroundColor Yellow
az acr login --name $registryName
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ ACR login failed" -ForegroundColor Red
    exit 1
}
Write-Host "   ✅ ACR login successful" -ForegroundColor Green

# Step 6: Tag and push image
Write-Host "📤 Pushing image to ACR..." -ForegroundColor Yellow
$registryUrl = "$registryName.azurecr.io"
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
docker tag $imageName "$registryUrl/kore-cloud:latest"
docker tag $imageName "$registryUrl/kore-cloud:$timestamp"
docker push "$registryUrl/kore-cloud:latest"
docker push "$registryUrl/kore-cloud:$timestamp"
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Docker push failed" -ForegroundColor Red
    exit 1
}
Write-Host "   ✅ Image pushed to ACR" -ForegroundColor Green

# Step 7: Generate database password
Write-Host "🔐 Generating database password..." -ForegroundColor Yellow
$DB_PASSWORD = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes((New-Guid).ToString())) -replace "[^a-zA-Z0-9]", ""
$DB_PASSWORD = $DB_PASSWORD.Substring(0, 32)
Write-Host "   Password: $DB_PASSWORD (SAVE THIS!)" -ForegroundColor Cyan

# Step 8: Create terraform.tfvars
Write-Host "⚙️  Creating terraform.tfvars..." -ForegroundColor Yellow
$terraformDir = Join-Path $PSScriptRoot "terraform\azure"
if (-not (Test-Path $terraformDir)) {
    Write-Host "❌ Terraform directory not found: $terraformDir" -ForegroundColor Red
    exit 1
}

$tfvarsContent = @"
azure_location              = "$Location"
environment                 = "$Environment"
resource_group_name         = "$resourceGroup"
app_name                    = "kore-cloud"
container_image             = "$registryUrl/kore-cloud:latest"
container_registry_url      = "$registryUrl"
container_registry_name     = "$registryName"
postgres_password           = "$DB_PASSWORD"
postgres_sku                = "B_Standard_B2s"
container_cpu               = "1.0"
container_memory            = "1.5"
container_instances         = 2
"@

$tfvarsPath = Join-Path $terraformDir "terraform.tfvars"
Set-Content -Path $tfvarsPath -Value $tfvarsContent
Write-Host "   ✅ terraform.tfvars created at $tfvarsPath" -ForegroundColor Green

# Step 9: Initialize Terraform
Write-Host "🏗️  Initializing Terraform..." -ForegroundColor Yellow
Push-Location $terraformDir
terraform init
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Terraform init failed" -ForegroundColor Red
    Pop-Location
    exit 1
}
Write-Host "   ✅ Terraform initialized" -ForegroundColor Green

# Step 10: Plan Terraform
Write-Host "📋 Planning Terraform deployment..." -ForegroundColor Yellow
terraform plan -out=tfplan
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Terraform plan failed" -ForegroundColor Red
    Pop-Location
    exit 1
}
Write-Host "   ✅ Terraform plan created" -ForegroundColor Green

# Step 11: Apply Terraform
Write-Host "⏳ Applying Terraform... (this may take 10-15 minutes)" -ForegroundColor Yellow
$confirm = Read-Host "   Continue with deployment? (yes/no)"
if ($confirm -ne "yes") {
    Write-Host "   ⏹️  Deployment cancelled" -ForegroundColor Yellow
    Pop-Location
    exit 0
}

terraform apply tfplan
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Terraform apply failed" -ForegroundColor Red
    Pop-Location
    exit 1
}
Write-Host "   ✅ Terraform applied successfully" -ForegroundColor Green

# Step 12: Get outputs
Write-Host "📊 Deployment outputs:" -ForegroundColor Yellow
$container_fqdn = terraform output -raw container_fqdn 2>$null
$postgres_fqdn = terraform output -raw postgres_fqdn 2>$null
$storage_account = terraform output -raw storage_account_name 2>$null

if ($container_fqdn) { Write-Host "   Container FQDN: $container_fqdn" -ForegroundColor Cyan }
if ($postgres_fqdn) { Write-Host "   PostgreSQL FQDN: $postgres_fqdn" -ForegroundColor Cyan }
if ($storage_account) { Write-Host "   Storage Account: $storage_account" -ForegroundColor Cyan }

Pop-Location

Write-Host ""
Write-Host "✅ Azure deployment complete!" -ForegroundColor Green
Write-Host "   💾 Save your database password: $DB_PASSWORD" -ForegroundColor Cyan
Write-Host "   🌐 Your API will be available at: http://$container_fqdn:8000/api/v1" -ForegroundColor Cyan
Write-Host ""
