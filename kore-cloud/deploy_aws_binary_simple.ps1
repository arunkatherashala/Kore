# AWS Deployment Script for Kore Cloud Binary - Simplified
# Deploys pre-compiled kore-cloud.exe to EC2 directly
# Usage: .\deploy_aws_binary_simple.ps1

param(
    [string]$Environment = "prod",
    [string]$Region = "us-east-1"
)

$ErrorActionPreference = "Stop"

Write-Host "Deploying Kore Cloud Binary to AWS EC2" -ForegroundColor Cyan
Write-Host "Environment: $Environment, Region: $Region" -ForegroundColor Gray

# Step 1: Verify AWS CLI
try {
    $ACCOUNT_ID = aws sts get-caller-identity --query Account --output text
    Write-Host "[OK] AWS Account: $ACCOUNT_ID" -ForegroundColor Green
} catch {
    Write-Host "[ERROR] AWS CLI not configured" -ForegroundColor Red
    exit 1
}

# Step 2: Verify binary
$binaryPath = ".\target\release\kore-cloud.exe"
if (-not (Test-Path $binaryPath)) {
    Write-Host "[ERROR] Binary not found at $binaryPath" -ForegroundColor Red
    exit 1
}
$binarySize = [math]::Round((Get-Item $binaryPath).Length / 1MB, 2)
Write-Host "[OK] Binary found: $binarySize MB" -ForegroundColor Green

# Step 3: Create S3 bucket
$S3_BUCKET = "kore-cloud-$ACCOUNT_ID-$Region"
Write-Host "[INFO] Setting up S3 bucket: $S3_BUCKET" -ForegroundColor Yellow

$bucketExists = $null
$oldErrorAction = $ErrorActionPreference
$ErrorActionPreference = 'SilentlyContinue'
$bucketExists = aws s3api head-bucket --bucket $S3_BUCKET --region $Region 2>$null
$ErrorActionPreference = $oldErrorAction

if (-not $bucketExists -or $bucketExists -eq "") {
    $oldErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    if ($Region -eq "us-east-1") {
        aws s3api create-bucket --bucket $S3_BUCKET --region $Region 2>&1 | Out-Null
    } else {
        aws s3api create-bucket --bucket $S3_BUCKET --region $Region --create-bucket-configuration LocationConstraint=$Region 2>&1 | Out-Null
    }
    aws s3api put-bucket-versioning --bucket $S3_BUCKET --versioning-configuration Status=Enabled --region $Region 2>&1 | Out-Null
    $ErrorActionPreference = $oldErrorAction
    Write-Host "[OK] S3 bucket created" -ForegroundColor Green
} else {
    Write-Host "[OK] S3 bucket exists" -ForegroundColor Green
}

# Step 4: Upload binary
Write-Host "[INFO] Uploading binary to S3..." -ForegroundColor Yellow
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
aws s3 cp $binaryPath "s3://$S3_BUCKET/kore-cloud-latest" --region $Region | Out-Null
Write-Host "[OK] Binary uploaded to S3" -ForegroundColor Green

# Step 5: Create database password
$DB_PASSWORD = -join ((65..90) + (97..122) + (48..57) | Get-Random -Count 32 | ForEach-Object {[char]$_})
Write-Host "[SAVE] Database Password: $DB_PASSWORD" -ForegroundColor Cyan

# Step 6: Get AMI
Write-Host "[INFO] Finding Amazon Linux 2 AMI..." -ForegroundColor Yellow
$AMI_ID = aws ec2 describe-images --owners amazon --filters "Name=name,Values=amzn2-ami-hvm-*-x86_64-gp2" --query "sort_by(Images, &CreationDate)[-1].ImageId" --output text --region $Region
Write-Host "[OK] AMI ID: $AMI_ID" -ForegroundColor Green

# Step 7: Create/get key pair
$KEY_NAME = "kore-cloud-key"
$keyExists = $null

$oldErrorAction = $ErrorActionPreference
$ErrorActionPreference = 'SilentlyContinue'
$keyExists = aws ec2 describe-key-pairs --key-names $KEY_NAME --region $Region --query 'KeyPairs[0].KeyName' --output text 2>$null
$ErrorActionPreference = $oldErrorAction

if (-not $keyExists -or $keyExists -eq "None" -or $keyExists -eq "") {
    Write-Host "[INFO] Creating EC2 key pair..." -ForegroundColor Yellow
    $oldErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    aws ec2 create-key-pair --key-name $KEY_NAME --query 'KeyMaterial' --output text --region $Region 2>$null | Out-File -FilePath "$KEY_NAME.pem" -Encoding UTF8
    $ErrorActionPreference = $oldErrorAction
    Write-Host "[OK] Key pair saved to $KEY_NAME.pem" -ForegroundColor Green
} else {
    Write-Host "[OK] Key pair exists" -ForegroundColor Green
}

# Step 8: Create security group
$SG_NAME = "kore-cloud-$Environment"
Write-Host "[INFO] Creating security group..." -ForegroundColor Yellow
$sgId = $null

$oldErrorAction = $ErrorActionPreference
$ErrorActionPreference = 'SilentlyContinue'
$sgId = aws ec2 describe-security-groups --filters "Name=group-name,Values=$SG_NAME" --region $Region --query 'SecurityGroups[0].GroupId' --output text 2>$null
$ErrorActionPreference = $oldErrorAction

if (-not $sgId -or $sgId -eq "None" -or $sgId -eq "") {
    $oldErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    $sgId = aws ec2 create-security-group --group-name $SG_NAME --description "Kore Cloud SG" --region $Region --query 'GroupId' --output text
    aws ec2 authorize-security-group-ingress --group-id $sgId --protocol tcp --port 22 --cidr 0.0.0.0/0 --region $Region 2>&1 | Out-Null
    aws ec2 authorize-security-group-ingress --group-id $sgId --protocol tcp --port 8080 --cidr 0.0.0.0/0 --region $Region 2>&1 | Out-Null
    $ErrorActionPreference = $oldErrorAction
    Write-Host "[OK] Security group created: $sgId" -ForegroundColor Green
} else {
    Write-Host "[OK] Security group exists: $sgId" -ForegroundColor Green
}

# Step 9: Create user data script (stored in file)
$userDataFile = "userdata.sh"
$userDataContent = @'
#!/bin/bash
set -e
yum update -y
yum install -y git curl build-essential postgresql15

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env

# Clone and compile Kore
mkdir -p /opt/kore-cloud
cd /opt/kore-cloud
git clone https://github.com/arunkatherashala/Kore.git .
cd kore-cloud
cargo build --release 2>&1 | tail -20

# Create systemd service
mkdir -p /opt/kore-cloud/bin
cp target/release/kore_fileformat /opt/kore-cloud/bin/kore-cloud || true
chmod +x /opt/kore-cloud/bin/kore-cloud

cat > /etc/systemd/system/kore-cloud.service << EOF
[Unit]
Description=Kore Cloud API
After=network.target
[Service]
Type=simple
User=ec2-user
WorkingDirectory=/opt/kore-cloud
ExecStart=/opt/kore-cloud/bin/kore-cloud
Environment="RUST_LOG=info"
Restart=on-failure
RestartSec=10
[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable kore-cloud
systemctl start kore-cloud || echo "Service may still be starting"
sleep 5
systemctl status kore-cloud || true
'@
$userDataContent | Set-Content -Path $userDataFile -Encoding UTF8

# Step 10: Create IAM role (skip if exists)
$ROLE_NAME = "kore-cloud-ec2-role"
$roleExists = $null

$oldErrorAction = $ErrorActionPreference
$ErrorActionPreference = 'SilentlyContinue'
$roleExists = aws iam get-role --role-name $ROLE_NAME 2>$null
$ErrorActionPreference = $oldErrorAction

if (-not $roleExists -or $roleExists -eq "" -or $roleExists -like "*NoSuchEntity*") {
    Write-Host "[INFO] Creating IAM role..." -ForegroundColor Yellow
    $trustPolicy = @{
        Version = "2012-10-17"
        Statement = @(@{
            Effect = "Allow"
            Principal = @{ Service = "ec2.amazonaws.com" }
            Action = "sts:AssumeRole"
        })
    } | ConvertTo-Json -Depth 10
    
    $trustPolicy | Out-File -FilePath "trust.json" -Encoding UTF8
    
    $oldErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    aws iam create-role --role-name $ROLE_NAME --assume-role-policy-document file://trust.json 2>&1 | Out-Null
    $ErrorActionPreference = $oldErrorAction
    
    $s3Policy = @{
        Version = "2012-10-17"
        Statement = @(@{
            Effect = "Allow"
            Action = @("s3:GetObject", "s3:ListBucket")
            Resource = @("arn:aws:s3:::$S3_BUCKET/*", "arn:aws:s3:::$S3_BUCKET")
        })
    } | ConvertTo-Json -Depth 10
    
    $s3Policy | Out-File -FilePath "s3.json" -Encoding UTF8
    
    $oldErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    aws iam put-role-policy --role-name $ROLE_NAME --policy-name S3Access --policy-document file://s3.json 2>&1 | Out-Null
    $ErrorActionPreference = $oldErrorAction
    
    Remove-Item "trust.json", "s3.json" -ErrorAction SilentlyContinue | Out-Null
    Write-Host "[OK] IAM role created" -ForegroundColor Green
} else {
    Write-Host "[OK] IAM role exists" -ForegroundColor Green
}

# Step 11: Create instance profile
$PROFILE_NAME = "kore-cloud-profile"
$profileExists = $null

$oldErrorAction = $ErrorActionPreference
$ErrorActionPreference = 'SilentlyContinue'
$profileExists = aws iam get-instance-profile --instance-profile-name $PROFILE_NAME 2>$null
$ErrorActionPreference = $oldErrorAction

if (-not $profileExists -or $profileExists -eq "" -or $profileExists -like "*NoSuchEntity*") {
    $oldErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    aws iam create-instance-profile --instance-profile-name $PROFILE_NAME 2>&1 | Out-Null
    aws iam add-role-to-instance-profile --instance-profile-name $PROFILE_NAME --role-name $ROLE_NAME 2>&1 | Out-Null
    $ErrorActionPreference = $oldErrorAction
    Write-Host "[OK] Instance profile created" -ForegroundColor Green
    Write-Host "[INFO] Waiting for IAM propagation..." -ForegroundColor Yellow
    Start-Sleep -Seconds 10
} else {
    Write-Host "[OK] Instance profile exists" -ForegroundColor Green
}

# Step 12: Launch EC2 instance
Write-Host "[INFO] Launching EC2 instance..." -ForegroundColor Yellow
$INSTANCE_NAME = "kore-cloud-$Environment"
$userDataBase64 = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($userDataContent))

$instanceId = aws ec2 run-instances `
    --image-id $AMI_ID `
    --instance-type t3.small `
    --key-name $KEY_NAME `
    --security-group-ids $sgId `
    --iam-instance-profile Name=$PROFILE_NAME `
    --user-data $userDataBase64 `
    --region $Region `
    --query 'Instances[0].InstanceId' `
    --output text

Write-Host "[OK] Instance launched: $instanceId" -ForegroundColor Green

# Step 13: Wait for running
Write-Host "[INFO] Waiting for instance to start..." -ForegroundColor Yellow
aws ec2 wait instance-running --instance-ids $instanceId --region $Region
Write-Host "[OK] Instance is running" -ForegroundColor Green

# Step 14: Get public IP
$publicIp = aws ec2 describe-instances --instance-ids $instanceId --region $Region --query 'Reservations[0].Instances[0].PublicIpAddress' --output text
$privateIp = aws ec2 describe-instances --instance-ids $instanceId --region $Region --query 'Reservations[0].Instances[0].PrivateIpAddress' --output text

Write-Host ""
Write-Host "DEPLOYMENT COMPLETE!" -ForegroundColor Green
Write-Host ""
Write-Host "Instance Details:" -ForegroundColor Cyan
Write-Host "  Instance ID: $instanceId"
Write-Host "  Public IP: $publicIp"
Write-Host "  Private IP: $privateIp"
Write-Host "  Region: $Region"
Write-Host ""
Write-Host "Database Details:" -ForegroundColor Cyan
Write-Host "  Note: Database setup requires additional RDS creation (manual step)"
Write-Host "  Password: $DB_PASSWORD"
Write-Host ""
Write-Host "Next Steps:" -ForegroundColor Yellow
Write-Host "  1. SSH: ssh -i $KEY_NAME.pem ec2-user@$publicIp"
Write-Host "  2. Check: systemctl status kore-cloud"
Write-Host "  3. API: curl http://$publicIp`:8080/api/v1/status"
Write-Host ""

# Cleanup
Remove-Item $userDataFile -ErrorAction SilentlyContinue
