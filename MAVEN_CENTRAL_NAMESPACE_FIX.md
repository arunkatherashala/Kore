# Maven Central Namespace Verification - CRITICAL FIX REQUIRED

## Current Status: ❌ FAILED

**Error**: "Namespace 'io.github.arunkatherashala' is not allowed"

This prevents ALL Maven Central deployments until resolved.

---

## Problem Analysis

Maven Central requires that the namespace `io.github.arunkatherashala` be **explicitly claimed and verified** before any artifacts can be published under that groupId.

### Why This Happens

For `io.github.*` namespaces (GitHub-based packages), Maven Central follows these rules:

1. **Auto-Verification via GitHub**: The namespace is auto-verified IF:
   - A GitHub repository exists at `github.com/[username]/[reponame]`
   - The `io.github.[username]` namespace hasn't been claimed by someone else yet
   - The deployment is made from the GitHub account's authorized user

2. **Manual Claim Required**: If auto-verification fails, you must manually claim the namespace in Maven Central's portal

---

## Solution Steps

### Step 1: Verify GitHub Account Ownership
Ensure:
- ✅ GitHub account: `arunkatherashala`
- ✅ Repository: `github.com/arunkatherashala/Kore` (public)
- ✅ User has authorization to publish from GitHub Actions

### Step 2: Claim Namespace in Maven Central
Follow these steps in Maven Central Portal:

**Option A: Automatic Claim (Recommended)**
1. Go to: https://central.sonatype.com/
2. Sign in with your account (the one authorized for `io.github.arunkatherashala`)
3. Navigate to **Publishing** → **Namespaces**
4. Click **+ Claim Namespace**
5. Select **GitHub** as the provider
6. Provide: `io.github.arunkatherashala`
7. Verify ownership by:
   - Creating a file in your GitHub repo at `.github/maven/verification`
   - Or following the Maven Central verification process
8. Click **Claim**

**Option B: Manual Verification via GitHub**
1. Create a verification file in the repository:
   - Path: `.github/maven/verification`
   - Content: Your Maven Central user ID or verification token
2. Maven Central will check this file to verify ownership
3. Once verified, the namespace is claimed

### Step 3: Update Workflow (Already Done ✅)
The workflow has been updated with:
- ✅ Proper checksum format (hash value only, no filename)
- ✅ GPG signature generation (if private key secret is set)
- ✅ Correct ZIP bundle structure (all files at root level)

**BUT** Still needs:
- 🔴 `MAVEN_GPG_PRIVATE_KEY` secret (for signing)
- 🔴 Namespace to be claimed in Maven Central

### Step 4: Set Up GPG Signing

If you want GPG-signed artifacts (recommended for security):

**A. Generate GPG Key** (if you don't have one):
```bash
gpg --gen-key
```

**B. Export Private Key**:
```bash
gpg --export-secret-key -a [your-key-id] | base64 -w 0 > gpg_private_key.txt
```

**C. Add to GitHub Secrets**:
1. Go to: GitHub Repo → Settings → Secrets and Variables → Actions
2. Create secret: `MAVEN_GPG_PRIVATE_KEY`
3. Paste the base64-encoded key from step B
4. Also ensure these secrets exist:
   - `CENTRAL_PORTAL_TOKEN_USERNAME`
   - `CENTRAL_PORTAL_TOKEN_PASSWORD`
   - `MAVEN_GPG_PASSPHRASE` (your GPG key passphrase)

---

## Verification Checklist

Before next deployment attempt:

- [ ] GitHub account `arunkatherashala` verified
- [ ] Repository is public and accessible
- [ ] Namespace `io.github.arunkatherashala` claimed in Maven Central
- [ ] GitHub secrets configured:
  - [ ] `CENTRAL_PORTAL_TOKEN_USERNAME`
  - [ ] `CENTRAL_PORTAL_TOKEN_PASSWORD`
  - [ ] `MAVEN_GPG_PRIVATE_KEY` (optional but recommended)
  - [ ] `MAVEN_GPG_PASSPHRASE` (optional but recommended)
- [ ] Workflow updated with:
  - [ ] ✅ Correct checksum format
  - [ ] ✅ GPG signing code added
  - [ ] ✅ Proper ZIP structure

---

## Deployment Process (After Fixes)

Once namespace is claimed and secrets are set up:

```bash
# Option 1: Tag-based (automatic)
git tag v1.2.3
git push origin v1.2.3
# Workflow triggers automatically

# Option 2: Manual trigger
gh workflow run publish-maven.yml -R arunkatherashala/Kore --ref main
```

Expected outcome:
- ✅ HTTP/2 201 response from Maven Central
- ✅ All checksums validated (hash format correct)
- ✅ All signatures verified (if GPG key provided)
- ✅ Namespace verified (once claimed)
- ✅ Artifact appears in search after 10-15 minutes

---

## Troubleshooting

| Error | Cause | Solution |
|-------|-------|----------|
| "Namespace not allowed" | Not claimed in Maven Central | Follow Step 2 above |
| "Invalid checksum" | Old checksum format (with filename) | Workflow already fixed ✅ |
| "Missing signature" | No GPG key set up | Set `MAVEN_GPG_PRIVATE_KEY` secret or skip signing |
| "File path ./ invalid" | ZIP structure incorrect | Workflow already fixed ✅ |

---

## Next Action

1. **Claim the namespace** in Maven Central (most critical)
   - Visit: https://central.sonatype.com/publishing/namespaces
   - Claim: `io.github.arunkatherashala`

2. **Set up GPG signing** (optional but recommended)
   - Generate key and add to GitHub secrets
   - Set `MAVEN_GPG_PRIVATE_KEY` and `MAVEN_GPG_PASSPHRASE`

3. **Test deployment**:
   ```bash
   gh workflow run publish-maven.yml -R arunkatherashala/Kore --ref main
   ```

---

**Reference**: https://central.sonatype.org/publish/publish-guide/#namespace-claim
