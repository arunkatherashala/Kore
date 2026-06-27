#!/bin/bash
set -e
echo "=== FIXED Kore Deployment (Linux paths) ==="

# 1. Install Rust
echo "[1/6] Installing Rust..."
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null 2>&1
source $HOME/.cargo/env

# 2. Install dependencies (corrected for Amazon Linux)
echo "[2/6] Installing dependencies..."
sudo yum update -y >/dev/null 2>&1
sudo yum install -y git gcc make >/dev/null 2>&1

# 3. Clone Kore
echo "[3/6] Cloning Kore repository..."
mkdir -p /opt/kore
cd /opt/kore
rm -rf .git Kore  # Clean up if partial clone exists
git clone https://github.com/arunkatherashala/Kore.git . >/dev/null 2>&1

# 4. Compile (with output)
echo "[4/6] Starting Rust compilation (15-25 minutes)..."
cd /opt/kore/kore-cloud
timeout 1800 cargo build --release 2>&1 | tail -20

# 5. Setup binary
echo "[5/6] Setting up binary..."
mkdir -p /opt/kore/bin
if [ -f target/release/kore_fileformat ]; then
  cp target/release/kore_fileformat /opt/kore/bin/kore-cloud
  chmod +x /opt/kore/bin/kore-cloud
  echo "Binary ready: /opt/kore/bin/kore-cloud"
else
  echo "ERROR: Binary not found!"
  exit 1
fi

# 6. Setup systemd service
echo "[6/6] Setting up systemd service..."
sudo tee /etc/systemd/system/kore-cloud.service > /dev/null << 'SERVICE'
[Unit]
Description=Kore Cloud Service
After=network.target

[Service]
Type=simple
User=ec2-user
WorkingDirectory=/opt/kore
ExecStart=/opt/kore/bin/kore-cloud
Restart=on-failure
RestartSec=5s
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
SERVICE

sudo systemctl daemon-reload
sudo systemctl enable kore-cloud
sudo systemctl start kore-cloud
echo "Service started!"
systemctl status kore-cloud --no-pager

echo "=== DEPLOYMENT COMPLETE ==="
