# 🚀 AWS Account & Credentials Setup Guide

**Date**: May 23, 2026  
**Email**: arunkatherashala@gmail.com  
**Purpose**: Deploy Kore v1.2.2 to AWS  

---

## Option 1: Complete AWS Signup (Manual Process)

If you need to create a brand new AWS account:

### Step 1: Manual Signup (via Browser)
Go to: https://signin.aws.amazon.com/signup?request_type=register

Fill in:
```
Email: arunkatherashala@gmail.com
Account Name: kore-prod (or your choice)
```

Click "Verify email address" and follow the 5 steps:
1. Email verification (check inbox)
2. Password creation
3. Contact information
4. Billing information (credit/debit card)
5. Phone verification

### Step 2: Access AWS Console
After account created:
1. Go to: https://console.aws.amazon.com/
2. Sign in with your email and password

### Step 3: Create Access Keys (for CLI use)

In AWS Console:
1. Click your account name (top right) → "Security Credentials"
2. Scroll down to "Access keys"
3. Click "Create access key"
4. Save the output (you'll only see it once!):
   - Access Key ID: `AKIA...`
   - Secret Access Key: `[32-character-string]`

---

## Option 2: Use Existing AWS Account

If you already have AWS credentials, proceed to **Step 1 below**.

---

## ✅ NEXT STEP: Configure AWS CLI (After Account Created)

### Prerequisites
- AWS Access Key ID
- AWS Secret Access Key
- Region: `us-east-1` (or your preferred region)

### Method 1: Interactive Configuration (Recommended)

```powershell
# Run this command in PowerShell
aws configure

# You'll be prompted:
AWS Access Key ID [None]: AKIA...
AWS Secret Access Key [None]: [your-secret-key]
Default region name [None]: us-east-1
Default output format [None]: json
```

### Method 2: Direct Script Setup

```powershell
# Run this PowerShell script to set up credentials automatically
$accessKeyId = "AKIA..."          # Replace with your key
$secretAccessKey = "xxx..."       # Replace with your secret key
$region = "us-east-1"

# Create .aws directory if it doesn't exist
$awsDir = "$env:USERPROFILE\.aws"
if (-not (Test-Path $awsDir)) {
    New-Item -ItemType Directory -Path $awsDir -Force | Out-Null
}

# Create credentials file
$credentialsContent = @"
[default]
aws_access_key_id = $accessKeyId
aws_secret_access_key = $secretAccessKey
"@

# Create config file
$configContent = @"
[default]
region = $region
output = json
"@

# Write files
$credentialsContent | Out-File -FilePath "$awsDir\credentials" -Encoding UTF8 -Force
$configContent | Out-File -FilePath "$awsDir\config" -Encoding UTF8 -Force

Write-Host "✅ AWS credentials configured successfully!" -ForegroundColor Green
Write-Host "Location: $awsDir" -ForegroundColor Cyan
```

### Method 3: Environment Variables (Quick & Temporary)

```powershell
# Set environment variables in current PowerShell session
$env:AWS_ACCESS_KEY_ID = "AKIA..."
$env:AWS_SECRET_ACCESS_KEY = "xxx..."
$env:AWS_DEFAULT_REGION = "us-east-1"

# Verify
aws sts get-caller-identity
```

---

## ✅ Verify Configuration

After setting up credentials, test them:

```powershell
# Test AWS access
aws sts get-caller-identity

# Expected output:
# {
#     "UserId": "AIDAI...",
#     "Account": "123456789012",
#     "Arn": "arn:aws:iam::123456789012:user/username"
# }
```

If you see this output with your Account ID, credentials are correctly configured! ✅

---

## 🚀 Next: Deploy to AWS

Once credentials are configured:

```powershell
cd c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\kore-cloud

.\deploy_aws.ps1 -Environment prod -Region us-east-1
```

---

## 📚 Resource Links

- **Create AWS Account**: https://signin.aws.amazon.com/signup?request_type=register
- **AWS Console**: https://console.aws.amazon.com/
- **Security Credentials**: https://console.aws.amazon.com/iam/home#/security_credentials
- **AWS Free Tier**: https://aws.amazon.com/free/
- **AWS CLI Documentation**: https://docs.aws.amazon.com/cli/

---

## ❓ Troubleshooting

### "Invalid ClientTokenId" Error
```
Error message: InvalidClientTokenId when calling the GetCallerIdentity operation
```
**Fix**: 
- Verify your Access Key ID and Secret Access Key are correct
- Make sure you're using the latest/active access key (not revoked)
- Check file location: `~\.aws\credentials` or `~\.aws\config`

### "Unable to locate credentials"
```
Error: Unable to locate credentials. You can configure credentials by running "aws configure"
```
**Fix**: Run `aws configure` and enter your credentials

### "Access Denied"
```
Error: User is not authorized to perform the operation
```
**Fix**: The access key doesn't have required permissions. In AWS Console:
- Go to IAM → Users
- Click your username
- Attach "AdministratorAccess" policy (or required policy)

---

## 🎯 Summary

| Step | Action | Status |
|------|--------|--------|
| 1 | Create AWS account | ⏳ Pending |
| 2 | Generate Access Keys | ⏳ Pending |
| 3 | Configure AWS CLI | ⏳ Pending |
| 4 | Verify with `aws sts get-caller-identity` | ⏳ Pending |
| 5 | Deploy with `.\deploy_aws.ps1` | 🚀 Ready |

---

**Email**: arunkatherashala@gmail.com  
**Deployment Ready**: After Step 4 ✅
