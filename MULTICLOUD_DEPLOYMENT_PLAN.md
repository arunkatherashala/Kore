# Kore Cloud Multi-Platform Deployment Plan ☁️

**Status**: Starting Azure + GCP deployments  
**AWS Status**: ✅ LIVE (http://3.238.217.239:8000)  
**Date**: 2026-05-23

---

## 🎯 Deployment Strategy

### Phase 1: Azure Container Apps ⏰ NOW
- **Region**: East US
- **Service**: Container App with managed identity
- **Storage**: Azure Blob Storage (for file backend)
- **Monitoring**: Application Insights
- **Timeline**: 15-20 minutes

### Phase 2: GCP Cloud Run ⏰ AFTER AZURE
- **Region**: us-central1
- **Service**: Cloud Run (serverless)
- **Storage**: Google Cloud Storage
- **Monitoring**: Cloud Logging
- **Timeline**: 15-20 minutes

---

## 📋 Kore Cloud Service Details

**Binary**: kore-cloud (1.7M, Rust)
**Port**: 8000
**Storage Backend**: Configurable (LOCAL → Cloud)
**Endpoints**:
- ✅ GET `/health` - Health check
- ✅ POST `/api/v1/files/upload` - File upload  
- ✅ GET `/api/v1/files/list` - List files
- ✅ GET `/api/v1/files/{id}/info` - File info

---

## 🔧 Azure Deployment Steps

### 1. Resource Group Creation
```bash
az group create \
  --name kore-cloud-rg \
  --location eastus
```

### 2. Container Registry
```bash
az acr create \
  --resource-group kore-cloud-rg \
  --name korefileformat \
  --sku Basic
```

### 3. Container App Environment
```bash
az containerapp env create \
  --name kore-env \
  --resource-group kore-cloud-rg \
  --location eastus
```

### 4. Container App Deployment
```bash
# Build and push image
docker build -t korefileformat.azurecr.io/kore-cloud:latest .
az acr build --registry korefileformat --image kore-cloud:latest .

# Deploy to Container Apps
az containerapp create \
  --name kore-cloud-azure \
  --resource-group kore-cloud-rg \
  --environment kore-env \
  --image korefileformat.azurecr.io/kore-cloud:latest \
  --target-port 8000 \
  --ingress external \
  --query properties.configuration.ingress.fqdn
```

### 5. Enable Managed Identity & Storage
```bash
# Create storage account
az storage account create \
  --name korefilestorage \
  --resource-group kore-cloud-rg \
  --location eastus

# Assign managed identity
az containerapp identity assign \
  --name kore-cloud-azure \
  --resource-group kore-cloud-rg \
  --system-assigned
```

---

## 🌐 GCP Deployment Steps

### 1. Create GCP Project (if needed)
```bash
gcloud projects create kore-cloud-gcp
gcloud config set project kore-cloud-gcp
```

### 2. Build and Push to Container Registry
```bash
# Configure Docker for GCP
gcloud auth configure-docker

# Build image
docker build -t gcr.io/kore-cloud-gcp/kore-cloud:latest .

# Push to GCP Container Registry
docker push gcr.io/kore-cloud-gcp/kore-cloud:latest
```

### 3. Deploy to Cloud Run
```bash
gcloud run deploy kore-cloud \
  --image gcr.io/kore-cloud-gcp/kore-cloud:latest \
  --platform managed \
  --region us-central1 \
  --port 8000 \
  --memory 512Mi \
  --allow-unauthenticated
```

### 4. Create Cloud Storage Bucket
```bash
gsutil mb -l us-central1 gs://kore-cloud-files/
```

### 5. Attach Service Account & Permissions
```bash
# Create service account
gcloud iam service-accounts create kore-cloud-sa \
  --display-name="Kore Cloud Service"

# Grant storage permissions
gcloud projects add-iam-policy-binding kore-cloud-gcp \
  --member serviceAccount:kore-cloud-sa@kore-cloud-gcp.iam.gserviceaccount.com \
  --role roles/storage.objectAdmin

# Bind to Cloud Run service
gcloud iam service-accounts add-iam-policy-binding \
  kore-cloud-sa@kore-cloud-gcp.iam.gserviceaccount.com \
  --role roles/iam.workloadIdentityUser \
  --member serviceAccount:kore-cloud-sa@kore-cloud-gcp.iam.gserviceaccount.com
```

---

## ✅ Verification URLs

| Platform | Endpoint | Status |
|----------|----------|--------|
| **AWS** | http://3.238.217.239:8000/health | ✅ LIVE |
| **Azure** | `<will-be-shown>` | ⏳ Deploying |
| **GCP** | `<will-be-shown>` | ⏳ Queued |

---

## 📊 Three-Cloud Architecture

```
┌─────────────────────────────────────────────────────┐
│          Kore Cloud v1.2.2 Multi-Cloud             │
├─────────────────────────────────────────────────────┤
│                                                     │
│  AWS (LIVE)           Azure (Deploying)   GCP (Next)
│  ✅ Running           ⏳ Container Apps   ⏳ Cloud Run
│  EC2 t3.small         Container Apps Env  Managed
│  Port 8000            Blob Storage         Cloud Storage
│  3.238.217.239        Managed Identity     Service Account
│                       AppInsights          Cloud Logging
│                                                     │
└─────────────────────────────────────────────────────┘
```

---

## 🚀 Next Actions

1. **Azure**: Execute deployment steps above
2. **GCP**: After Azure completes
3. **Update DNS/Load Balancing**: Point to all three regions
4. **Monitor All**: Sync status across platforms

---

**Progress**: AWS ✅ | Azure ⏳ | GCP ⏳
