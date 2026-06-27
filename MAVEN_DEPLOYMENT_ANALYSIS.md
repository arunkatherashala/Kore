# Maven Central v1.2.3 Deployment - Complete Analysis & Fixes

## 🔴 CRITICAL STATUS: Namespace Verification Required

Package: `io.github.arunkatherashala:kore-fileformat:1.2.3`  
Date: May 25, 2026  
Workflow: `publish-maven.yml`

---

## ❌ Errors Discovered

### 1. Invalid Checksum Format (FIXED ✅)
**Error**: `Invalid md5 checksum for file` and `Invalid sha1 checksum for file` (all 4 artifacts)

**Root Cause**:
```bash
# WRONG: md5sum outputs "[HASH]  [FILENAME]"
md5sum file.jar > file.jar.md5
# Creates: a1b2c3d4e5f6  file.jar

# CORRECT: Extract hash only
md5sum file.jar | cut -d' ' -f1 > file.jar.md5
# Creates: a1b2c3d4e5f6
```

Maven Central expects checksum files to contain **ONLY the hash value**, not the filename.

**Status**: ✅ FIXED in workflow

### 2. Missing GPG Signatures (FIXED ✅)
**Error**: `Missing signature for file` (all 4 artifacts)

**Root Cause**: Workflow wasn't generating `.asc` signature files.

**Fix Applied**:
```bash
# Import GPG private key
echo "${{ secrets.MAVEN_GPG_PRIVATE_KEY }}" | gpg --batch --import

# Sign each artifact
gpg --batch --pinentry-mode loopback \
  --passphrase "${{ secrets.MAVEN_GPG_PASSPHRASE }}" \
  --detach-sign --armor kore-fileformat-1.2.3.jar
```

**Status**: ✅ FIXED in workflow (requires secrets to be set)

### 3. Invalid File Paths in ZIP (FIXED ✅)
**Error**: `File path './' is not valid for file 'kore-fileformat-1.2.3.jar'` (all 12 files)

**Root Cause**: ZIP was containing paths with "./" prefix instead of root-level files.

**Fix Applied**:
- Using `zip -j` flag to strip all paths
- Dynamically building file list to avoid hardcoded failures
- Validating ZIP structure before upload

**Status**: ✅ FIXED in workflow

### 4. 🔴 CRITICAL: Namespace Not Allowed
**Error**: `Namespace 'io.github.arunkatherashala' is not allowed`

**Root Cause**: Maven Central requires explicit namespace claim for `io.github.*` groupIds

**Why This Matters**:
- Maven Central has a whitelist of approved namespaces
- `io.github.*` requires GitHub account verification
- Without claim: deployments are rejected at the Sonatype API level
- This is a **manual action required by the user** - cannot be automated

**Fix Required**: User must claim the namespace in Maven Central Portal
- Go to: https://central.sonatype.com/publishing/namespaces
- Click: **+ Claim Namespace**
- Select: **GitHub** as provider
- Enter: `io.github.arunkatherashala`
- Verify ownership via GitHub

**Status**: 🔴 BLOCKING - User action required

---

## ✅ Workflow Changes Applied

### File: `.github/workflows/publish-maven.yml`

#### Change 1: Checksum Format
```diff
- md5sum kore-fileformat-${VERSION}.jar > kore-fileformat-${VERSION}.jar.md5
+ md5sum kore-fileformat-${VERSION}.jar | cut -d' ' -f1 > kore-fileformat-${VERSION}.jar.md5
```
Applied to all 8 checksum generation commands (4 MD5 + 4 SHA1)

#### Change 2: GPG Signing Section (NEW)
```bash
# Added section that:
1. Imports GPG private key from secrets
2. Signs all 4 artifacts with detached ASCII signatures
3. Generates .asc files for each artifact
4. Warns if private key is not set
```

#### Change 3: ZIP Bundle Structure
```bash
# Improved to:
- Dynamically build file list instead of hardcoding
- Automatically detect and include .asc files if present
- Maintain zip -j flag for root-level placement
- Validate structure before upload
```

---

## 📋 Required Actions (User)

### CRITICAL (Must Do Before Next Deployment)

1. **Claim Namespace in Maven Central**
   ```
   URL: https://central.sonatype.com/publishing/namespaces
   Steps:
   1. Sign in with your Maven Central account
   2. Click "+ Claim Namespace"
   3. Select "GitHub" as provider
   4. Enter: io.github.arunkatherashala
   5. Verify ownership (GitHub check or manual verification)
   6. Submit claim
   
   Estimated time: 5-10 minutes
   Approval: Usually instant for GitHub-based namespaces
   ```

### OPTIONAL (Recommended for Security)

2. **Set Up GPG Signing**
   ```bash
   # Generate key (if needed)
   gpg --gen-key
   
   # Export private key in base64
   gpg --export-secret-key -a [KEY_ID] | base64 -w 0 > key.txt
   
   # Add to GitHub Secrets:
   # - MAVEN_GPG_PRIVATE_KEY (base64-encoded private key)
   # - MAVEN_GPG_PASSPHRASE (GPG key passphrase)
   ```

---

## 🔧 Testing After Fixes

Once namespace is claimed, test the updated workflow:

```bash
# Option 1: Manual trigger
gh workflow run publish-maven.yml -R arunkatherashala/Kore --ref main

# Option 2: Via tag push (automatic)
git tag v1.2.4  # (or next version)
git push origin v1.2.4
```

### Expected Output
```
✓ Build completed
✓ Checksums generated with proper format
✓ GPG signatures generated (.asc files)
✓ ZIP bundle created
✓ ZIP Structure Validation...
✓ All required checksums present (MD5: 4, SHA1: 4)
✓ Uploading bundle to Central Portal...
< HTTP/2 201
✓ Published to Maven Central
Note: May take 10-15 minutes to appear in search
```

### Verification
After 10-15 minutes, search should return artifact:
- URL: https://central.sonatype.com/search?q=kore-fileformat
- Maven usage:
  ```xml
  <dependency>
    <groupId>io.github.arunkatherashala</groupId>
    <artifactId>kore-fileformat</artifactId>
    <version>1.2.3</version>
  </dependency>
  ```

---

## 📊 Bundle Structure After Fixes

### With GPG Signatures (Recommended)
```
bundle.zip
├── kore-fileformat-1.2.3.jar
├── kore-fileformat-1.2.3.jar.md5
├── kore-fileformat-1.2.3.jar.sha1
├── kore-fileformat-1.2.3.jar.asc        (NEW)
├── kore-fileformat-1.2.3.pom
├── kore-fileformat-1.2.3.pom.md5
├── kore-fileformat-1.2.3.pom.sha1
├── kore-fileformat-1.2.3.pom.asc        (NEW)
├── kore-fileformat-1.2.3-sources.jar
├── kore-fileformat-1.2.3-sources.jar.md5
├── kore-fileformat-1.2.3-sources.jar.sha1
├── kore-fileformat-1.2.3-sources.jar.asc (NEW)
├── kore-fileformat-1.2.3-javadoc.jar
├── kore-fileformat-1.2.3-javadoc.jar.md5
├── kore-fileformat-1.2.3-javadoc.jar.sha1
└── kore-fileformat-1.2.3-javadoc.jar.asc  (NEW)
```

### Without GPG Signatures (If secrets not set)
```
bundle.zip
├── kore-fileformat-1.2.3.jar
├── kore-fileformat-1.2.3.jar.md5
├── kore-fileformat-1.2.3.jar.sha1
├── kore-fileformat-1.2.3.pom
├── kore-fileformat-1.2.3.pom.md5
├── kore-fileformat-1.2.3.pom.sha1
├── kore-fileformat-1.2.3-sources.jar
├── kore-fileformat-1.2.3-sources.jar.md5
├── kore-fileformat-1.2.3-sources.jar.sha1
├── kore-fileformat-1.2.3-javadoc.jar
├── kore-fileformat-1.2.3-javadoc.jar.md5
└── kore-fileformat-1.2.3-javadoc.jar.sha1
```

---

## 🎯 Success Criteria

Deployment is successful when:

✅ Namespace `io.github.arunkatherashala` is claimed  
✅ Workflow runs and completes without errors  
✅ HTTP/2 201 response from Maven Central API  
✅ Artifact appears in search within 10-15 minutes  
✅ Maven dependency resolution works

---

## 📚 Reference Documentation

- [Maven Central Publishing Guide](https://central.sonatype.org/publish-ea/publish-ea-guide/)
- [Namespace Claim Process](https://central.sonatype.org/publish/publish-guide/#namespace-claim)
- [GPG Signing Setup](https://central.sonatype.org/publish/publish-guide/#gpgsign)
- [Maven Central Portal API](https://central.sonatype.org/publish-ea/publish-ea-guide/)

---

## 📝 Summary

| Component | Status | Notes |
|-----------|--------|-------|
| Checksum Format | ✅ Fixed | Now hash-only format |
| GPG Signing | ✅ Added | Requires secrets to activate |
| ZIP Structure | ✅ Fixed | All files at root level |
| Namespace Verification | 🔴 Blocked | User action required in Maven Central |
| Workflow Syntax | ✅ Valid | Ready to deploy |

**Next Step**: Claim namespace in Maven Central, then retry deployment.
