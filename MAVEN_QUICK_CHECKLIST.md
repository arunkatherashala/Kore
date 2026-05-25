# Maven Central v1.2.3 - Quick Action Checklist

## 🚨 IMMEDIATE ACTION REQUIRED

**Blocker**: Namespace `io.github.arunkatherashala` must be claimed in Maven Central

---

## Step 1: Claim Namespace (5-10 minutes)
- [ ] Open: https://central.sonatype.com/publishing/namespaces
- [ ] Sign in with your Maven Central account
- [ ] Click "+ Claim Namespace" button
- [ ] Select "GitHub" as the provider
- [ ] Enter namespace: `io.github.arunkatherashala`
- [ ] Complete ownership verification
- [ ] Confirm claim is approved

**Timeline**: Usually instant for GitHub namespaces

---

## Step 2: (Optional) Set Up GPG Signing (10-15 minutes)

### If you DON'T have a GPG key:
```bash
# Generate new key
gpg --gen-key
# Follow prompts to create key
```

### Export and add to GitHub Secrets:
```bash
# List your keys
gpg --list-keys

# Export private key (replace KEY_ID with your actual ID)
gpg --export-secret-key -a KEY_ID | base64 -w 0 > gpg_key.txt

# Copy the output to clipboard
cat gpg_key.txt
```

### Add to GitHub:
- Go to: GitHub Repo → Settings → Secrets and variables → Actions
- Click: New repository secret
- Name: `MAVEN_GPG_PRIVATE_KEY`
- Value: Paste the base64 key from above
- Click: Add secret

- Repeat for: `MAVEN_GPG_PASSPHRASE` (your GPG key passphrase)

**Note**: If you skip this, deployment will still work but without cryptographic signatures

---

## Step 3: Test Deployment

### Wait for namespace claim approval (~5-10 minutes), then:

```bash
# Option A: Manual workflow trigger
gh workflow run publish-maven.yml -R arunkatherashala/Kore --ref main

# Option B: Via git tag (automatic trigger)
git tag v1.2.4
git push origin v1.2.4
```

### Monitor workflow:
```bash
# Check status
gh run list --workflow=publish-maven.yml -R arunkatherashala/Kore --limit 1

# Watch logs
gh run view [RUN_ID] --log -R arunkatherashala/Kore
```

### Expected output:
```
✓ Build completed
✓ Checksums generated with proper format
✓ ZIP bundle created
✓ All required checksums present (MD5: 4, SHA1: 4)
✓ Uploading bundle to Central Portal...
< HTTP/2 201
✓ Published to Maven Central
Note: May take 10-15 minutes to appear in search
```

---

## Step 4: Verify in Maven Central

### After 10-15 minutes of deployment:
- [ ] Go to: https://central.sonatype.com/search?q=kore-fileformat
- [ ] Verify version 1.2.3 appears
- [ ] Click on artifact to view details
- [ ] Confirm checksums are displayed
- [ ] Test Maven dependency resolution:

```bash
# In a test Maven project
mvn dependency:resolve -Dartifact=io.github.arunkatherashala:kore-fileformat:1.2.3
```

---

## Troubleshooting

### Issue: "Namespace 'io.github.arunkatherashala' is not allowed"
**Solution**: 
- Claim the namespace in Maven Central (Step 1)
- May take a few minutes to propagate
- Retry deployment after 5 minutes

### Issue: "Invalid checksum"
**Status**: ✅ Already fixed in workflow
- No action needed
- Workflow now generates checksums in correct format

### Issue: "Missing signature"
**Options**:
- Set up GPG signing (Step 2 - Optional)
- Or deployment will warn but continue without signatures

### Issue: Workflow fails during checksum validation
**Check**:
```bash
# View detailed logs
gh run view [RUN_ID] --log -R arunkatherashala/Kore | grep -A5 -B5 "ERROR"
```

---

## What Was Fixed

✅ **Checksum format**: Now generates hash-only format (not filename)  
✅ **ZIP structure**: All files placed at root level (no subdirectories)  
✅ **GPG signing**: Added support for signing artifacts  
✅ **Workflow validation**: Enhanced error checking

---

## Timeline

| Step | Time | Status |
|------|------|--------|
| Claim namespace | 5-10 min | ⏳ Pending (you) |
| Wait for approval | 1-5 min | ⏳ Auto |
| Run workflow | 2-3 min | ⏳ Pending |
| Index in Maven Central | 10-15 min | ⏳ Auto |
| **Total** | **20-35 min** | ⏳ In progress |

---

## Quick Links

- 🔗 Claim Namespace: https://central.sonatype.com/publishing/namespaces
- 🔗 Search Maven Central: https://central.sonatype.com/search?q=kore-fileformat
- 🔗 GitHub Workflow: https://github.com/arunkatherashala/Kore/actions/workflows/publish-maven.yml
- 🔗 Maven Guide: https://central.sonatype.org/publish-ea/publish-ea-guide/

---

**Ready to proceed?** → Go to Step 1 above 👆
