#!/usr/bin/env pwsh
<#
.SYNOPSIS
Deploy Kore v1.2.2 to AWS - FINAL PRODUCTION DEPLOYMENT
Uses t3.medium for Rust compilation with sufficient resources

.DESCRIPTION
Creates EC2 instance with:
- Larger instance type (t3.medium) for Rust compilation
- Compiled Kore binary running on port 8080
- Systemd service for automatic restart
#>

param(
    [string]$Environment = "prod",
    [string]$Region = "us-east-1"
)

$ErrorActionPreference = 'SilentlyContinue'

# Configuration
$InstanceType = "t3.medium"  # CRITICAL FIX: Larger instance for compilation
$AMI = "ami-0d5e7e27578d32e47"  # Amazon Linux 2
$KeyName = "kore-cloud-key"
$SecurityGroupId = "sg-03526a84e323c6383"
$IAMInstanceProfile = "kore-cloud-profile"
$BucketName = "kore-cloud-859551525785-us-east-1"

Write-Host "🚀 Kore v1.2.2 AWS Deployment (FINAL - t3.medium instance)" -ForegroundColor Green

# Verify AWS CLI
Write-Host "✓ AWS Account: " -NoNewline
aws sts get-caller-identity --query Account --output text 2>&1

# User data script - Compile Kore on instance (stored as raw string to avoid PowerShell parsing)
$userDataContent = @"
#!/bin/bash
set -e

echo "=== Installing dependencies ==="
yum update -y
yum install -y git curl build-essential postgresql15-client gcc make

echo "=== Installing Rust ==="
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
source `$HOME/.cargo/env

echo "=== Cloning Kore repository ==="
mkdir -p /opt/kore-cloud
cd /opt/kore-cloud
git clone https://github.com/arunkatherashala/Kore.git .

echo "=== Building Kore (this may take 20-30 minutes) ==="
cd /opt/kore-cloud/kore-cloud
timeout 1800 cargo build --release 2>&1 | tail -50 || echo "Build may still be running..."

echo "=== Preparing binary ==="
mkdir -p /opt/kore-cloud/bin
if [ -f target/release/kore_fileformat ]; then
    cp target/release/kore_fileformat /opt/kore-cloud/bin/kore-cloud
    chmod +x /opt/kore-cloud/bin/kore-cloud
    echo "✓ Binary ready: /opt/kore-cloud/bin/kore-cloud"
else
    echo "⚠ Warning: Binary not found, service will fail"
fi

echo "=== Creating systemd service ==="
cat > /etc/systemd/system/kore-cloud.service << 'EOFSERVICE'
[Unit]
Description=Kore Cloud API Server
After=network.target

[Service]
Type=simple
User=ec2-user
WorkingDirectory=/opt/kore-cloud
ExecStart=/opt/kore-cloud/bin/kore-cloud
Environment="RUST_LOG=info"
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOFSERVICE

echo "=== Starting Kore service ==="
systemctl daemon-reload
systemctl enable kore-cloud
systemctl start kore-cloud

echo "=== Waiting for service to respond ==="
for i in {1..30}; do
  if systemctl is-active --quiet kore-cloud; then
    sleep 2
    if curl -s http://localhost:8080/api/v1/status > /dev/null 2>&1; then
      echo "✓ Kore service is running and responding!"
      break
    fi
  fi
  sleep 1
done

systemctl status kore-cloud
"@

# Convert to base64 for AWS
$userDataBytes = [System.Text.Encoding]::UTF8.GetBytes($userDataContent)
$userDataBase64 = [Convert]::ToBase64String($userDataBytes)

Write-Host "📤 Launching EC2 instance (t3.medium)..." -ForegroundColor Yellow

# Launch instance
$response = aws ec2 run-instances `
  --image-id $AMI `
  --instance-type $InstanceType `
  --key-name $KeyName `
  --security-group-ids $SecurityGroupId `
  --iam-instance-profile "Name=$IAMInstanceProfile" `
  --user-data $userDataContent `
  --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=kore-cloud-prod-medium},{Key=Environment,Value=$Environment}]" `
  --region $Region `
  --query 'Instances[0].[InstanceId,PublicIpAddress]' `
  --output text 2>&1

if ($response -match "Exception|error|Error") {
    Write-Host "❌ Launch failed: $response" -ForegroundColor Red
    exit 1
}

# Parse response
$instanceId, $publicIp = $response.Split("`t")

Write-Host "✓ Instance launched: $instanceId" -ForegroundColor Green
Write-Host "✓ Public IP: $publicIp" -ForegroundColor Green

# Wait for instance to get public IP if needed
$attempts = 0
while ([string]::IsNullOrEmpty($publicIp) -and $attempts -lt 10) {
    Start-Sleep -Seconds 3
    $publicIp = aws ec2 describe-instances --instance-ids $instanceId --region $Region --query 'Reservations[0].Instances[0].PublicIpAddress' --output text 2>&1
    $attempts++
}

Write-Host "`n📋 DEPLOYMENT INFO:" -ForegroundColor Cyan
Write-Host "Instance ID: $instanceId"
Write-Host "Public IP: $publicIp"
Write-Host "Region: $Region"
Write-Host "Instance Type: $InstanceType"
Write-Host "Port: 8080"
Write-Host "`n⏱️  Kore is compiling... Expected: 20-30 minutes"
Write-Host "📊 Monitor progress:"
Write-Host "   ssh -i kore-cloud-key.pem ec2-user@$publicIp"
Write-Host "   systemctl status kore-cloud"
Write-Host "   journalctl -u kore-cloud -f"
Write-Host "`n🌐 Once ready, API will be at:"
Write-Host "   http://$publicIp:8080/api/v1/status"

# Save info
$infoContent = @"
# Kore AWS Deployment - t3.medium Instance

Deployment Date: $(Get-Date)
Instance ID: $instanceId
Public IP: $publicIp
Region: $Region
Instance Type: $InstanceType

## SSH Access
ssh -i kore-cloud-key.pem ec2-user@$publicIp

## Check Service Status
systemctl status kore-cloud
journalctl -u kore-cloud -f

## API Endpoint
http://$publicIp:8080/api/v1/status

## Expected Timeline
- Compilation: 20-30 minutes
- Service startup: ~1 minute after compilation
- API ready: ~31-31 minutes total
"@

$infoContent | Out-File -FilePath "DEPLOYMENT_MEDIUM_INFO.md" -Encoding UTF8
Write-Host "`n✓ Deployment info saved to DEPLOYMENT_MEDIUM_INFO.md" -ForegroundColor Green
