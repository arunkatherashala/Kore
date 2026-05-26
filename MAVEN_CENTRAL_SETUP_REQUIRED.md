# ⚠️ Maven Central Publishing - Secrets Required

## Status
The automated Maven Central publishing workflow is **ready** but requires GitHub Secrets to be configured.

## What's Working
✅ Maven build (POM.xml is correct)
✅ JAR/Sources/Javadoc generation  
✅ GitHub Actions workflow setup
✅ Deployment command (`mvn deploy`)

## What's Missing
❌ GitHub Secrets for Maven Central credentials

## Required GitHub Secrets

You need to add **2 secrets** to GitHub Actions for your repository:

### 1. `CENTRAL_PORTAL_TOKEN_USERNAME`
- **Where to get it**: https://central.sonatype.com/account
- **What it is**: Your Maven Central Portal username
- **Steps**:
  1. Log in to Maven Central Portal
  2. Go to Account > View Profile
  3. Copy your Username
  4. Add to GitHub Secrets with name `CENTRAL_PORTAL_TOKEN_USERNAME`

### 2. `CENTRAL_PORTAL_TOKEN_PASSWORD`
- **Where to get it**: https://central.sonatype.com/account
- **What it is**: Your Maven Central Portal API token password
- **Steps**:
  1. Log in to Maven Central Portal
  2. Go to Account > View Profile  
  3. Scroll down to "User Token"
  4. Click "Generate Token" (or use existing one)
  5. Copy the **password** part (not username!)
  6. Add to GitHub Secrets with name `CENTRAL_PORTAL_TOKEN_PASSWORD`

## How to Add Secrets to GitHub

### Option A: Via Web UI (Easiest)
1. Go to: https://github.com/arunkatherashala/Kore/settings/secrets/actions
2. Click "New repository secret"
3. Add each secret:
   - **Name**: `CENTRAL_PORTAL_TOKEN_USERNAME`
   - **Secret**: [your username from Maven Central Portal]
4. Repeat for `CENTRAL_PORTAL_TOKEN_PASSWORD`

### Option B: Via GitHub CLI
```bash
# Set username secret
gh secret set CENTRAL_PORTAL_TOKEN_USERNAME -R arunkatherashala/Kore

# Set password secret  
gh secret set CENTRAL_PORTAL_TOKEN_PASSWORD -R arunkatherashala/Kore
```

## After Adding Secrets

Once secrets are configured, trigger the publish workflow:

```bash
# Manual trigger
gh workflow run publish-maven.yml -R arunkatherashala/Kore --ref main

# Or push a git tag
git tag v1.2.3
git push origin v1.2.3
```

## Workflow Details

The workflow will:
1. ✅ Check out code
2. ✅ Set up Java 21
3. ✅ Read credentials from GitHub Secrets
4. ✅ Run `mvn clean deploy` to Maven Central
5. ✅ Publish artifacts:
   - JAR (compiled code)
   - Sources JAR
   - Javadoc JAR
   - POM file
   - MD5/SHA1 checksums

## Verification

After deployment:
1. Wait 10-15 minutes for Maven Central indexing
2. Search for the artifact:
   https://central.sonatype.com/search?q=kore-fileformat
3. Verify version 1.2.3 appears

## Troubleshooting

| Error | Cause | Fix |
|-------|-------|-----|
| "secrets not set" | GitHub Secrets not configured | Add secrets from Maven Central Portal account |
| "402 Payment Required" | Old credentials or wrong endpoint | Verify secrets are from current Portal token |
| "403 Forbidden" | Invalid credentials | Check credentials in Portal, regenerate token if needed |
| "POM parsing error" | XML syntax issue | This is already fixed (v0b0893c) |

## Questions?

See the workflow file: `.github/workflows/publish-maven.yml`
