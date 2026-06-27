# 🚀 Kore Cloud PowerShell Deployment Scripts

Native Windows PowerShell scripts for deploying to AWS, Azure, and GCP (no bash required).

## Prerequisites

- **Docker**: https://www.docker.com/products/docker-desktop
- **AWS CLI** (for AWS deployment): https://aws.amazon.com/cli/
- **Azure CLI** (for Azure deployment): https://docs.microsoft.com/cli/azure/
- **GCP SDK** (for GCP deployment): https://cloud.google.com/sdk

All CLIs must be authenticated and configured before running scripts.

## Quick Start

### AWS Deployment (15 minutes)

```powershell
# Basic (default: prod environment, us-east-1 region)
.\deploy_aws.ps1

# Custom environment and region
.\deploy_aws.ps1 -Environment prod -Region us-west-2
```

**What it does:**
1. Builds Docker image
2. Creates ECR repository
3. Pushes image to ECR
4. Generates database password
5. Creates terraform.tfvars with all settings
6. Initializes and applies Terraform
7. Outputs ALB DNS, RDS endpoint, S3 bucket

**Cost**: ~$337/month (ECS $150, RDS $100, S3 $23, ALB $32, NAT $32)

---

### Azure Deployment (15 minutes)

```powershell
# Basic (default: prod environment, eastus region)
.\deploy_azure.ps1

# Custom environment and location
.\deploy_azure.ps1 -Environment prod -Location westus2
```

**What it does:**
1. Creates resource group
2. Builds Docker image
3. Creates Azure Container Registry
4. Pushes image to ACR
5. Generates database password
6. Creates terraform.tfvars with all settings
7. Initializes and applies Terraform
8. Outputs Container FQDN, PostgreSQL FQDN, Storage account

**Cost**: ~$138/month (Container $35, PostgreSQL $50, Blob $18, ACR $25, Insights $10)

---

### GCP Deployment (15 minutes)

```powershell
# Basic (default: prod environment, us-central1 region)
.\deploy_gcp.ps1

# Custom environment, region, and email
.\deploy_gcp.ps1 -Environment prod -Region us-central1 -Email your-email@example.com
```

**What it does:**
1. Enables required GCP APIs
2. Creates Artifact Registry
3. Builds Docker image
4. Configures Docker for GCP
5. Pushes image to Artifact Registry
6. Generates database password
7. Creates terraform.tfvars with all settings
8. Initializes and applies Terraform
9. Outputs Cloud Run URL, Cloud SQL connection, Storage bucket

**Cost**: ~$180/month (Cloud Run $120, Cloud SQL $20, Cloud Storage $20, Load Balancer $20)

---

## Full Deployment Sequence

Deploy to all three clouds in order:

```powershell
# Step 1: AWS (15 min)
cd kore-cloud
.\deploy_aws.ps1 -Environment prod -Region us-east-1

# Step 2: Azure (15 min)
.\deploy_azure.ps1 -Environment prod -Location eastus

# Step 3: GCP (15 min)
.\deploy_gcp.ps1 -Environment prod -Region us-central1 -Email your-email@example.com
```

**Total Time**: ~45 minutes for all 3 clouds live

---

## Environment Variables

Each script creates `terraform/[provider]/terraform.tfvars` with:

- Cloud credentials
- Database password (randomly generated)
- Container image URLs
- Resource names and locations
- CPU/memory allocations
- Instance counts

**⚠️ IMPORTANT**: Save the database password! It's displayed in yellow during deployment.

---

## Troubleshooting

### Script won't execute
```powershell
# Allow script execution (Windows 10/11)
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
```

### Docker not found
```powershell
# Ensure Docker Desktop is running
docker ps
```

### AWS CLI not authenticated
```powershell
# Configure AWS credentials
aws configure
```

### Azure CLI not authenticated
```powershell
# Login to Azure
az login
```

### GCP CLI not authenticated
```powershell
# Login to GCP
gcloud auth login
gcloud config set project YOUR_PROJECT_ID
```

### Terraform apply fails
```powershell
# Check if terraform.tfvars has correct values
Get-Content terraform/[provider]/terraform.tfvars

# Check cloud credentials
aws sts get-caller-identity      # AWS
az account show                  # Azure
gcloud config list               # GCP
```

---

## Next Steps

After deployment:

1. **Verify health checks** (health endpoints):
   - AWS: `http://[ALB_DNS]/api/v1/status`
   - Azure: `http://[CONTAINER_FQDN]:8000/api/v1/status`
   - GCP: `[CLOUD_RUN_URL]/api/v1/status`

2. **Test upload/download**:
   ```bash
   curl -X POST http://[API_URL]/api/v1/upload -F "file=@test.txt"
   ```

3. **Monitor logs**:
   - AWS CloudWatch: Check ECS service logs
   - Azure Monitor: Check Container Instances logs
   - GCP Logging: Check Cloud Run logs

4. **Setup monitoring** (see respective cloud provider docs)

5. **Configure backups** (RDS, PostgreSQL, Cloud SQL all support automated backups)

---

## Security Notes

- Database passwords are randomly generated (32 characters)
- All connections use HTTPS/TLS
- Container registries are private
- Database connections are encrypted
- All cloud providers support VPC/networking isolation

---

## Rollback

To destroy all resources (delete deployment):

```powershell
# AWS
cd terraform/aws
terraform destroy

# Azure
cd terraform/azure
terraform destroy

# GCP
cd terraform/gcp
terraform destroy
```

---

**Version**: 1.2.2  
**Last Updated**: May 23, 2026  
**Platform**: Windows PowerShell 7.0+
