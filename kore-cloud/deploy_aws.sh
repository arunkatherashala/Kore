#!/bin/bash
# AWS Deployment Script
# Usage: ./deploy_aws.sh [environment] [region]

set -e

ENVIRONMENT=${1:-prod}
REGION=${2:-us-east-1}
ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)

echo "🚀 Deploying Kore Cloud to AWS"
echo "   Environment: $ENVIRONMENT"
echo "   Region: $REGION"
echo "   Account: $ACCOUNT_ID"
echo ""

# Step 1: Build Docker image
echo "📦 Building Docker image..."
docker build -t kore-cloud:latest .

# Step 2: Create ECR repository
echo "📁 Creating ECR repository..."
ECR_REPO_NAME="kore-cloud"
ECR_REPO_URL="$ACCOUNT_ID.dkr.ecr.$REGION.amazonaws.com/$ECR_REPO_NAME"

if ! aws ecr describe-repositories --repository-names $ECR_REPO_NAME --region $REGION 2>/dev/null; then
    echo "   Creating new ECR repository..."
    aws ecr create-repository --repository-name $ECR_REPO_NAME --region $REGION
fi

# Step 3: Login to ECR
echo "🔑 Logging in to ECR..."
aws ecr get-login-password --region $REGION | \
    docker login --username AWS --password-stdin $ECR_REPO_URL

# Step 4: Tag and push image
echo "📤 Pushing image to ECR..."
docker tag kore-cloud:latest $ECR_REPO_URL:latest
docker tag kore-cloud:latest $ECR_REPO_URL:$(date +%Y%m%d-%H%M%S)
docker push $ECR_REPO_URL:latest

# Step 5: Generate password
echo "🔐 Generating database password..."
DB_PASSWORD=$(openssl rand -base64 32)
echo "   Password: $DB_PASSWORD (SAVE THIS!)"

# Step 6: Create terraform.tfvars
echo "⚙️  Creating terraform.tfvars..."
cd terraform/aws

cat > terraform.tfvars <<EOF
aws_region              = "$REGION"
aws_account_id          = "$ACCOUNT_ID"
environment             = "$ENVIRONMENT"
image_tag               = "latest"
ecr_repository_url      = "$ECR_REPO_URL"
db_password             = "$DB_PASSWORD"
desired_count           = 3
log_level               = "info"
certificate_arn         = "arn:aws:acm:$REGION:$ACCOUNT_ID:certificate/REPLACE_WITH_YOUR_CERT"
vpc_id                  = "vpc-REPLACE_WITH_YOUR_VPC"
database_subnets        = ["subnet-REPLACE", "subnet-REPLACE"]
service_subnets         = ["subnet-REPLACE", "subnet-REPLACE"]
lb_subnets              = ["subnet-REPLACE", "subnet-REPLACE"]
EOF

echo "   ⚠️  Update terraform.tfvars with your VPC, subnets, and certificate!"
echo ""

# Step 7: Terraform initialization
echo "🏗️  Initializing Terraform..."
terraform init

# Step 8: Terraform plan
echo "📋 Planning deployment..."
terraform plan -out=tfplan

# Step 9: Prompt for approval
echo ""
read -p "Proceed with deployment? (yes/no): " -n 3 -r
echo
if [[ ! $REPLY =~ ^yes$ ]]; then
    echo "Deployment cancelled."
    exit 1
fi

# Step 10: Apply deployment
echo "🚀 Applying Terraform configuration..."
terraform apply tfplan

# Step 11: Get outputs
echo ""
echo "✅ Deployment complete!"
echo ""
echo "📊 Deployment Outputs:"
ALB_DNS=$(terraform output -raw alb_dns_name)
RDS_ENDPOINT=$(terraform output -raw rds_endpoint)
S3_BUCKET=$(terraform output -raw s3_bucket_name)
ECS_CLUSTER=$(terraform output -raw ecs_cluster_name)

echo "   ALB DNS: $ALB_DNS"
echo "   RDS Endpoint: $RDS_ENDPOINT"
echo "   S3 Bucket: $S3_BUCKET"
echo "   ECS Cluster: $ECS_CLUSTER"

echo ""
echo "🔍 Testing API..."
sleep 10
if curl -H "Content-Type: application/json" http://$ALB_DNS/api/v1/status 2>/dev/null | grep -q "ok"; then
    echo "✅ API is responding!"
else
    echo "⏳ API still initializing, check status with:"
    echo "   curl http://$ALB_DNS/api/v1/status"
fi

echo ""
echo "📝 Next steps:"
echo "   1. Update Route53 DNS to point to: $ALB_DNS"
echo "   2. Monitor logs: aws logs tail /ecs/kore-cloud-prod --follow"
echo "   3. Check ECS cluster: aws ecs describe-clusters --clusters $ECS_CLUSTER"

cd ../..
