# GCP Deployment Script for Kore Cloud
# Usage: .\deploy_gcp.ps1 -Environment prod -Region us-central1 -Email your-email@example.com

param(
    [string]$Environment = "prod",
    [string]$Region = "us-central1",
    [string]$Email = "default@example.com"
)

$ErrorActionPreference = "Stop"

Write-Host "🚀 Deploying Kore Cloud to GCP" -ForegroundColor Cyan
Write-Host "   Environment: $Environment" -ForegroundColor Gray
Write-Host "   Region: $Region" -ForegroundColor Gray
Write-Host "   Email: $Email" -ForegroundColor Gray
Write-Host ""

# Step 1: Get GCP project
Write-Host "🔍 Getting GCP project..." -ForegroundColor Yellow
$project = gcloud config get-value project
if (-not $project) {
    Write-Host "❌ Failed to get GCP project. Ensure gcloud CLI is configured." -ForegroundColor Red
    exit 1
}
Write-Host "   Project ID: $project" -ForegroundColor Green

# Step 2: Enable required APIs
Write-Host "⚙️  Enabling required GCP APIs..." -ForegroundColor Yellow
$apis = @(
    "container.googleapis.com",
    "cloudrun.googleapis.com",
    "cloudsql.googleapis.com",
    "storage-api.googleapis.com",
    "artifactregistry.googleapis.com",
    "compute.googleapis.com"
)

foreach ($api in $apis) {
    gcloud services enable $api --project=$project
}
Write-Host "   ✅ APIs enabled" -ForegroundColor Green

# Step 3: Create Artifact Registry
Write-Host "📁 Creating Artifact Registry..." -ForegroundColor Yellow
$registryName = "kore-cloud"
$registryUrl = "$Region-docker.pkg.dev/$project/$registryName"

gcloud artifacts repositories create $registryName `
    --repository-format=docker `
    --location=$Region `
    --project=$project 2>$null || Write-Host "   (Repository may already exist)" -ForegroundColor Gray

Write-Host "   ✅ Artifact Registry ready" -ForegroundColor Green

# Step 4: Build Docker image
Write-Host "📦 Building Docker image..." -ForegroundColor Yellow
$imageName = "kore-cloud:latest"
docker build -t $imageName .
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Docker build failed" -ForegroundColor Red
    exit 1
}
Write-Host "   ✅ Docker image built successfully" -ForegroundColor Green

# Step 5: Configure Docker for GCP
Write-Host "🔑 Configuring Docker for GCP..." -ForegroundColor Yellow
gcloud auth configure-docker "$Region-docker.pkg.dev" --quiet
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Docker configuration failed" -ForegroundColor Red
    exit 1
}
Write-Host "   ✅ Docker configured for GCP" -ForegroundColor Green

# Step 6: Tag and push image
Write-Host "📤 Pushing image to Artifact Registry..." -ForegroundColor Yellow
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$fullImageUrl = "$registryUrl/kore-cloud:latest"
$versionedUrl = "$registryUrl/kore-cloud:$timestamp"

docker tag $imageName $fullImageUrl
docker tag $imageName $versionedUrl
docker push $fullImageUrl
docker push $versionedUrl
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Docker push failed" -ForegroundColor Red
    exit 1
}
Write-Host "   ✅ Image pushed to Artifact Registry" -ForegroundColor Green

# Step 7: Generate database password
Write-Host "🔐 Generating database password..." -ForegroundColor Yellow
$DB_PASSWORD = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes((New-Guid).ToString())) -replace "[^a-zA-Z0-9]", ""
$DB_PASSWORD = $DB_PASSWORD.Substring(0, 32)
Write-Host "   Password: $DB_PASSWORD (SAVE THIS!)" -ForegroundColor Cyan

# Step 8: Create terraform.tfvars
Write-Host "⚙️  Creating terraform.tfvars..." -ForegroundColor Yellow
$terraformDir = Join-Path $PSScriptRoot "terraform\gcp"
if (-not (Test-Path $terraformDir)) {
    Write-Host "❌ Terraform directory not found: $terraformDir" -ForegroundColor Red
    exit 1
}

$tfvarsContent = @"
gcp_project          = "$project"
gcp_region           = "$Region"
environment          = "$Environment"
app_name             = "kore-cloud"
container_image      = "$fullImageUrl"
artifact_registry    = "$registryUrl"
db_password          = "$DB_PASSWORD"
db_tier              = "db.f1-micro"
cloud_run_cpu        = "2"
cloud_run_memory     = "4Gi"
cloud_run_instances  = 2
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
$cloud_run_url = terraform output -raw cloud_run_url 2>$null
$cloud_sql_connection = terraform output -raw cloud_sql_connection_name 2>$null
$storage_bucket = terraform output -raw storage_bucket_name 2>$null

if ($cloud_run_url) { Write-Host "   Cloud Run URL: $cloud_run_url" -ForegroundColor Cyan }
if ($cloud_sql_connection) { Write-Host "   Cloud SQL Connection: $cloud_sql_connection" -ForegroundColor Cyan }
if ($storage_bucket) { Write-Host "   Storage Bucket: $storage_bucket" -ForegroundColor Cyan }

Pop-Location

Write-Host ""
Write-Host "✅ GCP deployment complete!" -ForegroundColor Green
Write-Host "   💾 Save your database password: $DB_PASSWORD" -ForegroundColor Cyan
Write-Host "   🌐 Your API will be available at: $cloud_run_url/api/v1" -ForegroundColor Cyan
Write-Host ""
