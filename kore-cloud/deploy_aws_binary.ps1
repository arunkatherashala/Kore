# AWS Deployment Script for Kore Cloud Binary
# Deploys pre-compiled kore-cloud.exe to EC2 directly (no Docker needed)
# Usage: .\deploy_aws_binary.ps1 -Environment prod -Region us-east-1

param(
    [string]$Environment = "prod",
    [string]$Region = "us-east-1"
)

$ErrorActionPreference = "Stop"

Write-Host "🚀 Deploying Kore Cloud Binary to AWS EC2" -ForegroundColor Cyan
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

# Step 2: Verify binary exists
Write-Host "📦 Verifying kore-cloud binary..." -ForegroundColor Yellow
$binaryPath = ".\target\release\kore-cloud.exe"
if (-not (Test-Path $binaryPath)) {
    Write-Host "❌ Binary not found at $binaryPath" -ForegroundColor Red
    exit 1
}
$binarySize = (Get-Item $binaryPath).Length / 1MB
Write-Host "   ✅ Binary found: $([math]::Round($binarySize, 2)) MB" -ForegroundColor Green

# Step 3: Create S3 bucket for binary storage
Write-Host "📁 Setting up S3 bucket..." -ForegroundColor Yellow
$S3_BUCKET = "kore-cloud-$ACCOUNT_ID-$Region"
$bucketExists = aws s3api head-bucket --bucket $S3_BUCKET --region $Region 2>$null
if (-not $bucketExists) {
    Write-Host "   Creating S3 bucket: $S3_BUCKET" -ForegroundColor Gray
    if ($Region -eq "us-east-1") {
        aws s3api create-bucket --bucket $S3_BUCKET --region $Region
    } else {
        aws s3api create-bucket --bucket $S3_BUCKET --region $Region --create-bucket-configuration LocationConstraint=$Region
    }
    # Enable versioning
    aws s3api put-bucket-versioning --bucket $S3_BUCKET --versioning-configuration Status=Enabled --region $Region
    Write-Host "   ✅ S3 bucket created" -ForegroundColor Green
} else {
    Write-Host "   ✅ S3 bucket already exists" -ForegroundColor Green
}

# Step 4: Upload binary to S3
Write-Host "📤 Uploading binary to S3..." -ForegroundColor Yellow
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
aws s3 cp $binaryPath "s3://$S3_BUCKET/kore-cloud-$timestamp" --region $Region
aws s3 cp $binaryPath "s3://$S3_BUCKET/kore-cloud-latest" --region $Region
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ S3 upload failed" -ForegroundColor Red
    exit 1
}
Write-Host "   ✅ Binary uploaded to S3" -ForegroundColor Green

# Step 5: Generate database password
Write-Host "🔐 Generating database password..." -ForegroundColor Yellow
$DB_PASSWORD = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes((New-Guid).ToString())) -replace "[^a-zA-Z0-9]", ""
$DB_PASSWORD = $DB_PASSWORD.Substring(0, 32)
Write-Host "   Password: $DB_PASSWORD (SAVE THIS!)" -ForegroundColor Cyan

# Step 6: Get latest Amazon Linux 2 AMI
Write-Host "🖥️  Finding latest Amazon Linux 2 AMI..." -ForegroundColor Yellow
$AMI_ID = aws ec2 describe-images `
    --owners amazon `
    --filters "Name=name,Values=amzn2-ami-hvm-*-x86_64-gp2" `
    --query 'Images | sort_by(@, &CreationDate) | [-1].ImageId' `
    --output text `
    --region $Region
Write-Host "   AMI ID: $AMI_ID" -ForegroundColor Green

# Step 7: Create or import EC2 key pair
Write-Host "🔑 Setting up EC2 key pair..." -ForegroundColor Yellow
$KEY_NAME = "kore-cloud-key"
$keyExists = aws ec2 describe-key-pairs --key-names $KEY_NAME --region $Region 2>$null
if (-not $keyExists) {
    Write-Host "   Creating new key pair..." -ForegroundColor Gray
    aws ec2 create-key-pair --key-name $KEY_NAME --query 'KeyMaterial' --output text --region $Region | Out-File -FilePath "$KEY_NAME.pem" -Encoding UTF8
    Write-Host "   ✅ Key pair created and saved to $KEY_NAME.pem" -ForegroundColor Green
    Write-Host "   ⚠️  KEEP THIS FILE SAFE - You'll need it to SSH into instances!" -ForegroundColor Red
} else {
    Write-Host "   ✅ Key pair already exists" -ForegroundColor Green
}
Write-Host "🔐 Creating security group..." -ForegroundColor Yellow
$SG_NAME = "kore-cloud-$Environment"
$sgExists = aws ec2 describe-security-groups --filters "Name=group-name,Values=$SG_NAME" --region $Region --query 'SecurityGroups[0].GroupId' --output text 2>$null
if ($sgExists -eq "None" -or -not $sgExists) {
    $SG_ID = aws ec2 create-security-group --group-name $SG_NAME --description "Kore Cloud security group" --region $Region --query 'GroupId' --output text
    # Allow SSH
    aws ec2 authorize-security-group-ingress --group-id $SG_ID --protocol tcp --port 22 --cidr 0.0.0.0/0 --region $Region
    # Allow HTTP
    aws ec2 authorize-security-group-ingress --group-id $SG_ID --protocol tcp --port 8080 --cidr 0.0.0.0/0 --region $Region
    # Allow HTTPS
    aws ec2 authorize-security-group-ingress --group-id $SG_ID --protocol tcp --port 8443 --cidr 0.0.0.0/0 --region $Region
    Write-Host "   ✅ Security group created: $SG_ID" -ForegroundColor Green
} else {
    $SG_ID = $sgExists
    Write-Host "   ✅ Security group already exists: $SG_ID" -ForegroundColor Green
}

# Step 8: Create RDS PostgreSQL database
Write-Host "🗄️  Creating RDS PostgreSQL database..." -ForegroundColor Yellow
$DB_IDENTIFIER = "kore-cloud-$Environment"
$dbExists = aws rds describe-db-instances --db-instance-identifier $DB_IDENTIFIER --region $Region --query 'DBInstances[0].DBInstanceIdentifier' --output text 2>$null
if ($dbExists -eq "None" -or -not $dbExists) {
    Write-Host "   Creating RDS instance (this takes ~5 minutes)..." -ForegroundColor Gray
    aws rds create-db-instance `
        --db-instance-identifier $DB_IDENTIFIER `
        --db-instance-class db.t3.micro `
        --engine postgres `
        --engine-version 15.3 `
        --allocated-storage 20 `
        --storage-type gp2 `
        --master-username admin `
        --master-user-password $DB_PASSWORD `
        --no-publicly-accessible `
        --multi-az `
        --backup-retention-period 7 `
        --region $Region | Out-Null
    Write-Host "   ✅ RDS instance created" -ForegroundColor Green
    
    # Wait for RDS to be available
    Write-Host "   ⏳ Waiting for RDS to become available..." -ForegroundColor Yellow
    aws rds wait db-instance-available --db-instance-identifier $DB_IDENTIFIER --region $Region
    Write-Host "   ✅ RDS is now available" -ForegroundColor Green
} else {
    Write-Host "   ✅ RDS database already exists" -ForegroundColor Green
}

# Step 9: Get RDS endpoint
Write-Host "🔗 Getting RDS endpoint..." -ForegroundColor Yellow
$DB_HOST = aws rds describe-db-instances --db-instance-identifier $DB_IDENTIFIER --region $Region --query 'DBInstances[0].Endpoint.Address' --output text
Write-Host "   Database host: $DB_HOST" -ForegroundColor Green

# Step 10: Create IAM role for EC2
Write-Host "🔑 Creating IAM role for EC2..." -ForegroundColor Yellow
$ROLE_NAME = "kore-cloud-ec2-role"
$roleExists = aws iam get-role --role-name $ROLE_NAME 2>$null
if (-not $roleExists) {
    $trustPolicy = @"
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Service": "ec2.amazonaws.com"
      },
      "Action": "sts:AssumeRole"
    }
  ]
}
"@
    $trustPolicy | Set-Content -Path "trust-policy.json" -Encoding UTF8
    aws iam create-role --role-name $ROLE_NAME --assume-role-policy-document file://trust-policy.json
    
    # Attach policy for S3 access
    $s3Policy = @"
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:ListBucket"
      ],
      "Resource": [
        "arn:aws:s3:::$S3_BUCKET/*",
        "arn:aws:s3:::$S3_BUCKET"
      ]
    }
  ]
}
"@
    $s3Policy | Set-Content -Path "s3-policy.json" -Encoding UTF8
    aws iam put-role-policy --role-name $ROLE_NAME --policy-name S3Access --policy-document file://s3-policy.json
    
    Remove-Item -Path "trust-policy.json", "s3-policy.json" -ErrorAction SilentlyContinue
    Write-Host "   ✅ IAM role created" -ForegroundColor Green
} else {
    Write-Host "   ✅ IAM role already exists" -ForegroundColor Green
}

# Step 11: Create instance profile
Write-Host "👤 Creating instance profile..." -ForegroundColor Yellow
$PROFILE_NAME = "kore-cloud-profile"
$profileExists = aws iam get-instance-profile --instance-profile-name $PROFILE_NAME 2>$null
if (-not $profileExists) {
    aws iam create-instance-profile --instance-profile-name $PROFILE_NAME
    aws iam add-role-to-instance-profile --instance-profile-name $PROFILE_NAME --role-name $ROLE_NAME
    Write-Host "   ✅ Instance profile created" -ForegroundColor Green
} else {
    Write-Host "   ✅ Instance profile already exists" -ForegroundColor Green
}

# Step 12: Create user data script
Write-Host "📝 Creating EC2 user data script..." -ForegroundColor Yellow
$userDataScript = @"
#!/bin/bash
set -e

echo "=== Kore Cloud EC2 Initialization ===" 

# Install dependencies
yum update -y
yum install -y aws-cli postgresql15-devel

# Download binary from S3
aws s3 cp s3://$S3_BUCKET/kore-cloud-latest /opt/kore-cloud --region $Region
chmod +x /opt/kore-cloud

# Create systemd service
cat > /etc/systemd/system/kore-cloud.service << 'SERVICEEOF'
[Unit]
Description=Kore Cloud API Server
After=network.target

[Service]
Type=simple
User=ec2-user
ExecStart=/opt/kore-cloud
Environment="RUST_LOG=info"
Environment="DATABASE_URL=postgresql://admin:$DB_PASSWORD@$DB_HOST:5432/kore"
Environment="S3_BUCKET=$S3_BUCKET"
Environment="AWS_REGION=$Region"
Restart=on-failure
RestartSec=10

[Install]
WantedBy=multi-user.target
SERVICEEOF

systemctl daemon-reload
systemctl enable kore-cloud
systemctl start kore-cloud

echo "=== Kore Cloud service started ===" 
"@

# Encode for EC2
$userDataEncoded = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($userDataScript))

# Step 13: Launch EC2 instance
Write-Host "🚀 Launching EC2 instance..." -ForegroundColor Yellow
$INSTANCE_NAME = "kore-cloud-$Environment"
$result = aws ec2 run-instances `
    --image-id $AMI_ID `
    --instance-type t3.small `
    --key-name kore-cloud-key `
    --security-group-ids $SG_ID `
    --iam-instance-profile Name=$PROFILE_NAME `
    --user-data $userDataScript `
    --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=$INSTANCE_NAME},{Key=Environment,Value=$Environment}]" `
    --region $Region `
    --query 'Instances[0].InstanceId' `
    --output text

if (-not $result) {
    Write-Host "❌ EC2 instance launch failed" -ForegroundColor Red
    exit 1
}
$INSTANCE_ID = $result
Write-Host "   Instance ID: $INSTANCE_ID" -ForegroundColor Green

# Step 14: Wait for instance to be running
Write-Host "⏳ Waiting for EC2 instance to be running..." -ForegroundColor Yellow
aws ec2 wait instance-running --instance-ids $INSTANCE_ID --region $Region
Write-Host "   ✅ Instance is running" -ForegroundColor Green

# Step 15: Get instance details
Write-Host "📊 Getting instance details..." -ForegroundColor Yellow
$PUBLIC_IP = aws ec2 describe-instances --instance-ids $INSTANCE_ID --region $Region --query 'Reservations[0].Instances[0].PublicIpAddress' --output text
$PRIVATE_IP = aws ec2 describe-instances --instance-ids $INSTANCE_ID --region $Region --query 'Reservations[0].Instances[0].PrivateIpAddress' --output text

Write-Host ""
Write-Host "✅ Deployment Complete!" -ForegroundColor Green
Write-Host ""
Write-Host "📋 Deployment Summary:" -ForegroundColor Cyan
Write-Host "   Instance ID: $INSTANCE_ID" -ForegroundColor Yellow
Write-Host "   Public IP: $PUBLIC_IP" -ForegroundColor Yellow
Write-Host "   Private IP: $PRIVATE_IP" -ForegroundColor Yellow
Write-Host "   Database Host: $DB_HOST" -ForegroundColor Yellow
Write-Host "   Database Password: $DB_PASSWORD (SAVE THIS!)" -ForegroundColor Cyan
Write-Host "   S3 Bucket: $S3_BUCKET" -ForegroundColor Yellow
Write-Host ""
Write-Host "🔗 API Endpoint: http://$PUBLIC_IP`:8080" -ForegroundColor Green
Write-Host ""
Write-Host "📝 Next Steps:" -ForegroundColor Cyan
Write-Host "   1. SSH into instance: ssh -i kore-cloud-key.pem ec2-user@$PUBLIC_IP" -ForegroundColor Gray
Write-Host "   2. Check service status: systemctl status kore-cloud" -ForegroundColor Gray
Write-Host "   3. View logs: journalctl -u kore-cloud -f" -ForegroundColor Gray
Write-Host ""
