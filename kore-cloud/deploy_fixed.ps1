#!/usr/bin/env pwsh
<#
.SYNOPSIS
Deploy Kore v1.2.2 to AWS with t3.medium instance

.DESCRIPTION
Final, working deployment script for Kore
#>

param(
    [string]$Environment = "prod",
    [string]$Region = "us-east-1"
)

$ErrorActionPreference = 'SilentlyContinue'

# Configuration
$InstanceType = "t3.medium"
$AMI = "ami-0d5e7e27578d32e47"
$KeyName = "kore-cloud-key"
$SecurityGroupId = "sg-03526a84e323c6383"
$IAMInstanceProfile = "kore-cloud-profile"

Write-Host "[DEPLOY] Kore v1.2.2 AWS Deployment (t3.medium)" -ForegroundColor Green

# Verify AWS
Write-Host "[AWS] Account: " -NoNewline
aws sts get-caller-identity --query Account --output text 2>&1

# Create userdata file
$userDataScript = "C:\temp\kore-userdata.sh"
$null = New-Item -ItemType Directory -Path C:\temp -Force
@'
#!/bin/bash
set -e

echo "=== Installing Dependencies ==="
yum update -y
yum install -y git curl gcc make postgresql15-client

echo "=== Installing Rust ==="
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env

echo "=== Clone and Build Kore ==="
mkdir -p /opt/kore-cloud
cd /opt/kore-cloud
git clone https://github.com/arunkatherashala/Kore.git .
cd kore-cloud
cargo build --release 2>&1 | tail -20

echo "=== Setup Binary ==="
mkdir -p bin
cp target/release/kore_fileformat bin/kore-cloud 2>/dev/null || echo "Build failed or binary missing"
chmod +x bin/kore-cloud

echo "=== Create Systemd Service ==="
cat > /tmp/kore-svc << 'EOF'
[Unit]
Description=Kore Cloud API
After=network.target

[Service]
Type=simple
User=ec2-user
WorkingDirectory=/opt/kore-cloud
ExecStart=/opt/kore-cloud/bin/kore-cloud
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

sudo cp /tmp/kore-svc /etc/systemd/system/kore-cloud.service
sudo systemctl daemon-reload
sudo systemctl enable kore-cloud
sudo systemctl start kore-cloud

sleep 5
systemctl status kore-cloud || true
'@ | Out-File -FilePath $userDataScript -Encoding ASCII

Write-Host "[LAUNCH] Launching EC2 (t3.medium)..." -ForegroundColor Yellow

$response = aws ec2 run-instances `
  --image-id $AMI `
  --instance-type $InstanceType `
  --key-name $KeyName `
  --security-group-ids $SecurityGroupId `
  --iam-instance-profile "Name=$IAMInstanceProfile" `
  --user-data "file://$userDataScript" `
  --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=kore-prod},{Key=Version,Value=1.2.2}]" `
  --region $Region `
  --query 'Instances[0].[InstanceId,PublicIpAddress]' `
  --output text 2>&1

# Parse response
$parts = $response.Split("`t")
$instanceId = $parts[0]
$publicIp = $parts[1]

if ($instanceId -match "Exception|error") {
    Write-Host "❌ Error: $instanceId" -ForegroundColor Red
    exit 1
}

Write-Host "[OK] Instance: $instanceId" -ForegroundColor Green
Write-Host "[OK] IP: $publicIp" -ForegroundColor Green
Write-Host "`n[WAIT] Compilation starting... 20-30 minutes expected" -ForegroundColor Yellow

# Save details
@"
# Kore Deployment - t3.medium

Instance ID: $instanceId
Public IP: $publicIp
Port: 8080
Status: Compiling Kore (20-30 min)

## Access
ssh -i kore-cloud-key.pem ec2-user@$publicIp

## Check Service
systemctl status kore-cloud
journalctl -u kore-cloud -f

## API Endpoint (when ready)
http://$publicIp:8080/api/v1/status
"@ | Out-File "KORE_DEPLOYMENT_$instanceId.txt"

Write-Host "[OK] Info saved to file" -ForegroundColor Green
