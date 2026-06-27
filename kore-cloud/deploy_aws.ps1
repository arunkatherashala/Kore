# AWS Deployment Script for Kore Cloud
# Usage: .\deploy_aws.ps1 -Environment prod -Region us-east-1

param(
    [string]$Environment = "prod",
    [string]$Region = "us-east-1"
)

$ErrorActionPreference = "Stop"

Write-Host "🚀 Deploying Kore Cloud to AWS" -ForegroundColor Cyan
Write-Host "   Environment: $Environment" -ForegroundColor Gray
Write-Host "   Region: $Region" -ForegroundColor Gray
Write-Host ""

# Step 1: Get AWS Account ID
Write-Host "🔍 Getting AWS Account ID..." -ForegroundColor Yellow
$ACCOUNT_ID = aws sts get-caller-identity --query Account --output text
if (-not $ACCOUNT_ID) {
    Write-Host "❌ Failed to get AWS Account ID. Ensure AWS CLI is configured." -ForegroundColor Red
    exit 1
}
Write-Host "   Account ID: $ACCOUNT_ID" -ForegroundColor Green

# Step 2: Build Docker image
Write-Host "📦 Building Docker image..." -ForegroundColor Yellow
$imageName = "kore-cloud:latest"
docker build -t $imageName .
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Docker build failed" -ForegroundColor Red
    exit 1
}
Write-Host "   ✅ Docker image built successfully" -ForegroundColor Green

# Step 3: Create ECR repository
Write-Host "📁 Creating ECR repository..." -ForegroundColor Yellow
$ECR_REPO_NAME = "kore-cloud"
$ECR_REPO_URL = "$ACCOUNT_ID.dkr.ecr.$Region.amazonaws.com/$ECR_REPO_NAME"

$repoExists = aws ecr describe-repositories --repository-names $ECR_REPO_NAME --region $Region 2>$null
if (-not $repoExists) {
    Write-Host "   Creating new ECR repository..." -ForegroundColor Gray
    aws ecr create-repository --repository-name $ECR_REPO_NAME --region $Region
    Write-Host "   ✅ ECR repository created" -ForegroundColor Green
} else {
    Write-Host "   ✅ ECR repository already exists" -ForegroundColor Green
}

# Step 4: Login to ECR
Write-Host "🔑 Logging in to ECR..." -ForegroundColor Yellow
$loginToken = aws ecr get-login-password --region $Region
$loginToken | docker login --username AWS --password-stdin $ECR_REPO_URL
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ ECR login failed" -ForegroundColor Red
    exit 1
}
Write-Host "   ✅ ECR login successful" -ForegroundColor Green

# Step 5: Tag and push image
Write-Host "📤 Pushing image to ECR..." -ForegroundColor Yellow
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
docker tag $imageName $ECR_REPO_URL`:latest
docker tag $imageName $ECR_REPO_URL`:$timestamp
docker push $ECR_REPO_URL`:latest
docker push $ECR_REPO_URL`:$timestamp
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Docker push failed" -ForegroundColor Red
    exit 1
}
Write-Host "   ✅ Image pushed to ECR" -ForegroundColor Green

# Step 6: Generate database password
Write-Host "🔐 Generating database password..." -ForegroundColor Yellow
$DB_PASSWORD = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes((New-Guid).ToString())) -replace "[^a-zA-Z0-9]", ""
$DB_PASSWORD = $DB_PASSWORD.Substring(0, 32)
Write-Host "   Password: $DB_PASSWORD (SAVE THIS!)" -ForegroundColor Cyan

# Step 7: Create terraform.tfvars
Write-Host "⚙️  Creating terraform.tfvars..." -ForegroundColor Yellow
$terraformDir = Join-Path $PSScriptRoot "terraform\aws"
if (-not (Test-Path $terraformDir)) {
    Write-Host "❌ Terraform directory not found: $terraformDir" -ForegroundColor Red
    exit 1
}

$tfvarsContent = @"
aws_region           = "$Region"
environment          = "$Environment"
app_name             = "kore-cloud"
container_image      = "$ECR_REPO_URL`:latest"
rds_password         = "$DB_PASSWORD"
rds_instance_class   = "db.t3.micro"
ecs_task_cpu         = "1024"
ecs_task_memory      = "2048"
ecs_desired_count    = 2
"@

$tfvarsPath = Join-Path $terraformDir "terraform.tfvars"
Set-Content -Path $tfvarsPath -Value $tfvarsContent
Write-Host "   ✅ terraform.tfvars created at $tfvarsPath" -ForegroundColor Green

# Step 8: Initialize Terraform
Write-Host "🏗️  Initializing Terraform..." -ForegroundColor Yellow
Push-Location $terraformDir
terraform init
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Terraform init failed" -ForegroundColor Red
    Pop-Location
    exit 1
}
Write-Host "   ✅ Terraform initialized" -ForegroundColor Green

# Step 9: Plan Terraform
Write-Host "📋 Planning Terraform deployment..." -ForegroundColor Yellow
terraform plan -out=tfplan
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Terraform plan failed" -ForegroundColor Red
    Pop-Location
    exit 1
}
Write-Host "   ✅ Terraform plan created" -ForegroundColor Green

# Step 10: Apply Terraform
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

# Step 11: Get outputs
Write-Host "📊 Deployment outputs:" -ForegroundColor Yellow
$alb_dns = terraform output -raw alb_dns_name 2>$null
$rds_endpoint = terraform output -raw rds_endpoint 2>$null
$s3_bucket = terraform output -raw s3_bucket_name 2>$null

if ($alb_dns) { Write-Host "   ALB DNS: $alb_dns" -ForegroundColor Cyan }
if ($rds_endpoint) { Write-Host "   RDS Endpoint: $rds_endpoint" -ForegroundColor Cyan }
if ($s3_bucket) { Write-Host "   S3 Bucket: $s3_bucket" -ForegroundColor Cyan }

Pop-Location

Write-Host ""
Write-Host "✅ AWS deployment complete!" -ForegroundColor Green
Write-Host "   💾 Save your database password: $DB_PASSWORD" -ForegroundColor Cyan
Write-Host "   🌐 Your API will be available at: http://$alb_dns/api/v1" -ForegroundColor Cyan
Write-Host ""
