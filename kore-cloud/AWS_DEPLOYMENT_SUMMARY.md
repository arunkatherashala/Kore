# Kore v1.2.2 - AWS EC2 Deployment Summary

## ✅ Deployment Status: IN PROGRESS

**Launched:** May 23, 2026  
**Current Phase:** Rust compilation on EC2  
**Expected Completion:** 10-25 minutes from launch

---

## EC2 Instance Details

| Property | Value |
|----------|-------|
| **Instance ID** | i-0a0b4330fec192d3e |
| **Instance Type** | t3.small |
| **Public IP** | 3.237.77.211 |
| **Private IP** | 172.31.11.224 |
| **Region** | us-east-1 |
| **Status** | Running ✅ |
| **AMI** | Amazon Linux 2 (ami-0d5e7e27578d32e47) |

---

## What's Happening Now

The EC2 instance is currently running the startup script which:

1. ✅ **Complete**: Installed Rust toolchain
2. ✅ **Complete**: Cloned Kore repository from GitHub
3. 🔄 **IN PROGRESS**: Compiling Kore with `cargo build --release`
4. ⏳ **Pending**: Starting systemd service
5. ⏳ **Pending**: API listening on port 8080

---

## Access & Testing

Once compilation completes, you can access via:

```bash
# SSH Access
ssh -i kore-cloud-key.pem ec2-user@3.237.77.211

# Check service status
ssh -i kore-cloud-key.pem ec2-user@3.237.77.211 "systemctl status kore-cloud"

# View logs
ssh -i kore-cloud-key.pem ec2-user@3.237.77.211 "journalctl -u kore-cloud -f"

# Test API
curl http://3.237.77.211:8080/api/v1/status
```

---

## AWS Infrastructure

### S3 Bucket
- **Name**: kore-cloud-859551525785-us-east-1
- **Purpose**: Store compiled binary
- **Versioning**: Enabled

### Security Group
- **ID**: sg-03526a84e323c6383
- **Ingress Rules**:
  - SSH (22): 0.0.0.0/0
  - HTTP (8080): 0.0.0.0/0

### IAM
- **Role**: kore-cloud-ec2-role
- **Instance Profile**: kore-cloud-profile
- **Permissions**: S3 read access for binary retrieval

### Key Pair
- **Name**: kore-cloud-key
- **Location**: kore-cloud-key.pem (in kore-cloud directory)
- **Format**: RSA Private Key

---

## Database (Optional)

If you want to enable PostgreSQL backend:

```bash
# Generated password for future RDS setup
DB_PASSWORD=DqTW6B1yLn3EZ5sfYmketCKVFXw7gPxA

# Create RDS PostgreSQL instance manually, then SSH and update:
export DATABASE_URL="postgresql://admin:${DB_PASSWORD}@RDS_ENDPOINT:5432/kore"
sudo systemctl restart kore-cloud
```

---

## Monitoring

### Watch Compilation Progress
```powershell
# Check if instance is still compiling
Get-Content "c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\kore-cloud\compilation_monitor.log" -Tail 20
```

### Real-time Log Viewing
```bash
ssh -i kore-cloud-key.pem ec2-user@3.237.77.211 "tail -f /var/log/messages"
```

---

## Cleanup (When Ready)

```powershell
# Terminate instance
aws ec2 terminate-instances --instance-ids i-0a0b4330fec192d3e --region us-east-1

# Delete security group
aws ec2 delete-security-group --group-id sg-03526a84e323c6383 --region us-east-1

# Delete IAM resources
aws iam remove-role-from-instance-profile --instance-profile-name kore-cloud-profile --role-name kore-cloud-ec2-role
aws iam delete-instance-profile --instance-profile-name kore-cloud-profile
aws iam delete-role-policy --role-name kore-cloud-ec2-role --policy-name S3Access
aws iam delete-role --role-name kore-cloud-ec2-role

# Delete S3 bucket (empty first if needed)
aws s3 rb s3://kore-cloud-859551525785-us-east-1 --force
```

---

## Troubleshooting

If API doesn't respond after 30 minutes:

```bash
# SSH to instance
ssh -i kore-cloud-key.pem ec2-user@3.237.77.211

# Check if process is running
ps aux | grep kore

# View full logs
journalctl -u kore-cloud --no-pager | tail -50

# Check available disk space
df -h

# Check memory
free -h

# Restart service manually
sudo systemctl restart kore-cloud
```

---

## Deployment Timeline

- **Launch**: Completed ✅
- **Infrastructure Setup**: 2-3 minutes ✅
- **Git Clone**: 1-2 minutes ✅
- **Rust Compile**: 15-25 minutes ⏳ (IN PROGRESS)
- **Service Start**: 30-60 seconds ⏳
- **API Ready**: Expected within 20-30 minutes total

---

## Quick Links

- [AWS Console - EC2 Instances](https://us-east-1.console.aws.amazon.com/ec2/home?region=us-east-1#Instances:)
- [AWS Console - Security Groups](https://us-east-1.console.aws.amazon.com/ec2/home?region=us-east-1#SecurityGroups:)
- [AWS Console - S3 Buckets](https://s3.console.aws.amazon.com/s3/home)
- GitHub: https://github.com/arunkatherashala/Kore

---

**Generated**: May 23, 2026  
**Account ID**: 859551525785  
**Region**: us-east-1
