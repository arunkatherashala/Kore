# Kore Cloud Binary Deployment - Quick Start

## ✅ What Changed
- **Skipped Docker build** (was causing daemon crashes)
- **Using pre-compiled binary** (`target/release/kore-cloud.exe`)
- **Direct EC2 deployment** instead of ECS container orchestration
- **Simpler, faster deployment** (~10-15 minutes instead of 30+)

## 🚀 Deploy Now

Run this command from the `kore-cloud` directory:

```powershell
.\deploy_aws_binary.ps1 -Environment prod -Region us-east-1
```

## 📋 What Gets Created

1. **S3 Bucket** - Stores the kore-cloud binary
2. **RDS PostgreSQL** - Database for the application (db.t3.micro)
3. **EC2 Instance** - Runs the application (t3.small, Amazon Linux 2)
4. **Security Groups** - Network access rules
5. **IAM Role** - Permissions for EC2 to access S3 and other services

## 🔑 Important Files

After deployment, you'll get:
- `kore-cloud-key.pem` - EC2 SSH key (SAVE THIS!)
- Deployment summary with:
  - Instance ID
  - Public IP
  - Database endpoint
  - Database password

## 📊 Deployment Timeline

| Step | What Happens | Time |
|------|---|---|
| 1-5 | Setup S3, upload binary, create IAM | ~2 min |
| 6-9 | Launch EC2 and RDS | ~7 min |
| 10-12 | Initialize services and verify | ~3 min |
| **Total** | | **~10-12 minutes** |

## 🔗 After Deployment

Once complete, you can:

```bash
# SSH into the instance
ssh -i kore-cloud-key.pem ec2-user@<PUBLIC_IP>

# Check service status
systemctl status kore-cloud

# View logs
journalctl -u kore-cloud -f

# Test the API
curl http://<PUBLIC_IP>:8080/api/v1/status
```

## 💾 Costs (AWS Free Tier)

- **RDS db.t3.micro**: Free for 12 months
- **EC2 t3.small**: ~$0.02/hour (may exceed free tier)
- **S3**: First 5GB free, then $0.023/GB
- **Data transfer**: Free within region

**Estimated monthly cost: $15-20**

## ⚠️ Prerequisites

✅ AWS CLI configured with credentials  
✅ Binary exists at `target/release/kore-cloud.exe`  
✅ AWS region has capacity (us-east-1 is recommended)

---

**Ready? Run the deployment script!**

```powershell
.\deploy_aws_binary.ps1
```
