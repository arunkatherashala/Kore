# Kore Cloud v1.2.2 - AWS Deployment Complete ✅

## Deployment Summary
Successfully deployed Kore Cloud REST API to AWS EC2 on **$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')**

## Instance Details
- **Instance ID**: i-091b2aaf885ec4c74
- **Instance Type**: t3.small
- **Region**: us-east-1
- **AMI**: Amazon Linux 2 (ami-0d5e7e27578d32e47)
- **Public IP**: 44.193.84.232
- **Private IP**: 172.31.5.106

## Security & Access
- **Key Pair**: kore-cloud-key.pem (saved in kore-cloud directory)
- **Security Group**: sg-03526a84e323c6383 (allows SSH:22, HTTP:8080)
- **IAM Role**: kore-cloud-ec2-role (S3 access for binary retrieval)
- **Instance Profile**: kore-cloud-profile

## S3 Storage
- **Bucket**: kore-cloud-859551525785-us-east-1
- **Binary Location**: s3://kore-cloud-859551525785-us-east-1/kore-cloud-latest (1.38 MB)
- **Versioning**: Enabled

## Service Configuration
- **Binary Location on EC2**: /opt/kore-cloud/kore-cloud
- **Service Name**: kore-cloud
- **User**: ec2-user
- **systemd Unit**: /etc/systemd/system/kore-cloud.service
- **Environment Variables**:
  - RUST_LOG=info
  - S3_BUCKET=kore-cloud-859551525785-us-east-1
  - AWS_REGION=us-east-1

## API Endpoints
- **Base URL**: http://44.193.84.232:8080
- **Status Check**: curl http://44.193.84.232:8080/api/v1/status

## Next Steps

### 1. SSH into Instance
```powershell
ssh -i kore-cloud-key.pem ec2-user@44.193.84.232
```

### 2. Check Service Status
```bash
systemctl status kore-cloud
journalctl -u kore-cloud -n 50 -f  # Follow logs
```

### 3. Verify Service
```bash
curl http://localhost:8080/api/v1/status
```

### 4. (Optional) Setup RDS PostgreSQL
Database password for future use: **vtacJrDjzAVslebUhuNy3HpYqCXonkQS**

If using PostgreSQL backend, create RDS instance and set `DATABASE_URL` env var:
```bash
export DATABASE_URL=postgresql://admin:PASSWORD@RDS_ENDPOINT:5432/kore
```

## Cleanup Commands
```powershell
# Terminate instance (if needed)
aws ec2 terminate-instances --instance-ids i-091b2aaf885ec4c74 --region us-east-1

# Delete security group
aws ec2 delete-security-group --group-id sg-03526a84e323c6383 --region us-east-1

# Delete IAM role/profile
aws iam remove-role-from-instance-profile --instance-profile-name kore-cloud-profile --role-name kore-cloud-ec2-role
aws iam delete-instance-profile --instance-profile-name kore-cloud-profile
aws iam delete-role --role-name kore-cloud-ec2-role

# Delete S3 bucket (if empty)
aws s3 rb s3://kore-cloud-859551525785-us-east-1
```

## AWS Credentials
- **AWS Account ID**: 859551525785
- **Region**: us-east-1
- **Access**: Configured via AWS CLI default credentials
