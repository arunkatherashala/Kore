# GitHub Secrets Setup Guide for Kore v1.0.0

## 🎯 Overview

This guide walks you through setting up **6 GitHub Secrets** required for automated publishing of Kore to:
- **Rust crates.io** (crate registry)
- **PyPI** (Python package registry)
- **Maven Central** (Java repository)
- **npm** (JavaScript package registry)

Once configured, the CI/CD pipeline will automatically publish Kore to all registries when you create a version tag.

---

## 📋 Secrets Required

| Secret Name | Platform | Usage | Priority |
|---|---|---|---|
| **CARGO_TOKEN** | crates.io | Publish Rust crate | ⭐⭐⭐ High |
| **PYPI_TOKEN** | PyPI | Publish Python wheel | ⭐⭐⭐ High |
| **MAVEN_CENTRAL_USERNAME** | Maven Central (OSSRH) | Java package auth | ⭐⭐ Medium |
| **MAVEN_CENTRAL_PASSWORD** | Maven Central (OSSRH) | Java package auth | ⭐⭐ Medium |
| **GPG_PASSPHRASE** | Maven Central | Sign Java artifacts | ⭐⭐ Medium |
| **NPM_TOKEN** | npm | Publish JS package | ⭐⭐ Medium |

---

## 🚀 Step-by-Step Setup

### Step 1: Get crates.io API Token (Rust)

**Time: 5 minutes**

1. **Visit crates.io**:
   - Go to https://crates.io/me (must be logged in)
   - If not logged in, click "Log in" and authenticate with GitHub

2. **Generate Token**:
   - Click "Account Settings" in the top right
   - Click "API Tokens" in left sidebar
   - Click "New Token"
   - Token will be auto-generated (looks like: `cratesio_abcd1234...`)
   - Copy the token

3. **Add to GitHub Secrets**:
   - Go to: `https://github.com/arunkatherashala/Kore/settings/secrets/actions`
   - Click "New repository secret"
   - Name: `CARGO_TOKEN`
   - Value: Paste the crates.io token
   - Click "Add secret"

**Verify**: Check that `CARGO_TOKEN` appears in the secrets list

---

### Step 2: Get PyPI API Token (Python)

**Time: 5 minutes**

1. **Visit PyPI**:
   - Go to https://pypi.org/ (must be logged in)
   - If not logged in, click "Sign in" and authenticate

2. **Generate Token**:
   - Click your profile icon (top right) → "Account settings"
   - Click "API tokens" in left sidebar
   - Click "Create token"
   - Scope: Select "Entire account" (or specific project if preferred)
   - Token name: `kore-ci-cd`
   - Click "Create token"
   - Copy the token (starts with: `pypi-...`)

3. **Add to GitHub Secrets**:
   - Go to: `https://github.com/arunkatherashala/Kore/settings/secrets/actions`
   - Click "New repository secret"
   - Name: `PYPI_TOKEN`
   - Value: Paste the PyPI token
   - Click "Add secret"

**Verify**: Check that `PYPI_TOKEN` appears in the secrets list

---

### Step 3: Setup Maven Central (Java)

**Time: 10-15 minutes**

Maven Central uses OSSRH (Sonatype OSS Repository Hosting). You need to create an account and generate credentials.

#### 3a. Create OSSRH Account

1. **Visit OSSRH**:
   - Go to https://issues.sonatype.org/
   - Click "Sign Up" (top right)
   - Fill in the form:
     - **Full name**: Sai Arun Kumar Ktherashala
     - **Email**: arunkatherashala@gmail.com
     - **Username**: Pick a unique username (e.g., `arunkatherashala-kore`)
     - **Password**: Strong password (save it securely)
   - Click "Sign up"
   - Check your email for verification link
   - Click link to verify account

#### 3b. Create Jira Issue for Namespace

1. **Login to OSSRH**:
   - Go to https://issues.sonatype.org/login
   - Use your new account credentials

2. **Create new issue**:
   - Click "Create" (top navigation)
   - Fill in:
     - **Project**: Community License (Maven Central)
     - **Summary**: `Namespace request for com.arun.kore`
     - **Description**: 
       ```
       Request namespace for Kore project
       
       Group ID: com.arun.kore
       Package: kore-cloud-java
       GitHub: https://github.com/arunkatherashala/Kore
       
       This is the Kore columnar file format library with cloud connectors.
       ```
     - **Attach**: Project website/repo proof
   - Click "Create"
   - **WAIT for approval** (usually 1-2 business days)
   - Save the issue ID (e.g., OSSRH-12345)

3. **Wait for Approval Email**
   - Sonatype will email when approved
   - Comment in issue will say: "Namespace approved"

#### 3c. Get OSSRH Credentials

1. **Login to OSSRH**:
   - Go to https://issues.sonatype.org/login
   - Use your OSSRH account

2. **Find your credentials**:
   - Your OSSRH username = the username you signed up with
   - Your OSSRH password = the password you signed up with
   - These are typically the same as your Jira account

#### 3d. Add Maven Secrets to GitHub

1. **Add MAVEN_CENTRAL_USERNAME**:
   - Go to: `https://github.com/arunkatherashala/Kore/settings/secrets/actions`
   - Click "New repository secret"
   - Name: `MAVEN_CENTRAL_USERNAME`
   - Value: Your OSSRH username
   - Click "Add secret"

2. **Add MAVEN_CENTRAL_PASSWORD**:
   - Click "New repository secret"
   - Name: `MAVEN_CENTRAL_PASSWORD`
   - Value: Your OSSRH password
   - Click "Add secret"

**Note**: Save these credentials securely. You'll need them for future releases.

---

### Step 4: Generate GPG Key for Maven

**Time: 10 minutes**

Maven requires artifacts to be signed with GPG. We'll generate a key and configure GitHub secrets.

#### 4a. Generate GPG Key (Local Machine)

If you don't have GPG installed:
```bash
# Windows (with Chocolatey)
choco install gnupg

# macOS
brew install gnupg

# Linux (Ubuntu/Debian)
sudo apt-get install gnupg
```

**Generate key**:
```bash
gpg --gen-key
# Or (newer GPG versions):
gpg --full-generate-key
```

Follow prompts:
- **Key type**: RSA (press Enter for default)
- **Key size**: 4096
- **Validity**: 3y (3 years)
- **Real name**: Sai Arun Kumar Ktherashala
- **Email**: arunkatherashala@gmail.com
- **Comment**: Kore CI/CD Signing Key
- **Passphrase**: Create a STRONG passphrase (you'll need this)

**Example output**:
```
sec   rsa4096 2026-05-14 [SC] [expires: 2029-05-14]
      ABCDEF1234567890ABCDEF1234567890ABCDEF12
```

#### 4b. Export Public Key to Maven Central

```bash
# List keys
gpg --list-keys

# Export public key (replace with your key ID from above)
gpg --armor --export ABCDEF1234567890ABCDEF1234567890ABCDEF12 > public-key.asc

# Upload to key servers
gpg --keyserver hkp://keyserver.ubuntu.com:80 --send-keys ABCDEF1234567890ABCDEF1234567890ABCDEF12
gpg --keyserver hkp://pgp.mit.edu --send-keys ABCDEF1234567890ABCDEF1234567890ABCDEF12
```

**Save the public key file** - you might need it later.

#### 4c. Add GPG_PASSPHRASE to GitHub

1. **Go to GitHub Secrets**:
   - `https://github.com/arunkatherashala/Kore/settings/secrets/actions`
   - Click "New repository secret"
   - Name: `GPG_PASSPHRASE`
   - Value: The passphrase you created in step 4a
   - Click "Add secret"

**Note**: This passphrase is SENSITIVE. GitHub keeps it encrypted.

---

### Step 5: Get npm API Token (JavaScript)

**Time: 5 minutes**

1. **Visit npm**:
   - Go to https://www.npmjs.com/ (must be logged in)
   - If not logged in, click "Sign in" and create account

2. **Generate Token**:
   - Click your profile icon (top right) → "Account"
   - Click "Tokens" in left sidebar
   - Click "Generate new token"
   - Choose: "Granular Access Token"
   - Fill in:
     - **Token name**: `kore-ci-cd`
     - **Permissions**: 
       - `Read and Publish packages`
       - `Read and write access`
     - **Packages and scopes**: Select "All packages"
     - **Expiration**: 1 year (or longer)
   - Click "Generate"
   - Copy the token (starts with: `npm_...`)

3. **Add to GitHub Secrets**:
   - Go to: `https://github.com/arunkatherashala/Kore/settings/secrets/actions`
   - Click "New repository secret"
   - Name: `NPM_TOKEN`
   - Value: Paste the npm token
   - Click "Add secret"

**Verify**: Check that `NPM_TOKEN` appears in the secrets list

---

## ✅ Verification Checklist

After adding all secrets, verify they're configured:

```bash
# Navigate to repository
cd c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore

# View secrets (GitHub CLI required)
gh secret list

# Expected output:
# NPM_TOKEN
# CARGO_TOKEN
# PYPI_TOKEN
# MAVEN_CENTRAL_USERNAME
# MAVEN_CENTRAL_PASSWORD
# GPG_PASSPHRASE
```

Or check manually:
1. Go to: `https://github.com/arunkatherashala/Kore/settings/secrets/actions`
2. You should see **6 secrets** in the list
3. All should show: ✅ Added by @arunkatherashala

---

## 🚀 Test Publishing

Once all secrets are configured, test the workflow:

### Step 1: Create a Test Tag
```bash
cd c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore

# Create annotated tag
git tag -a v1.0.0-test -m "Test release for CI/CD verification"

# Push tag
git push origin v1.0.0-test
```

### Step 2: Monitor GitHub Actions
1. Go to: `https://github.com/arunkatherashala/Kore/actions`
2. You should see workflow: "Release (v1.0.0-test)"
3. Watch the workflow run through all jobs:
   - ✅ Integration tests
   - ✅ Python wheels build
   - ✅ Java bindings build
   - ✅ JavaScript bindings build
   - ✅ Security scanning
   - ✅ Documentation build
   - ✅ **Publish to registries** (should use your secrets)

### Step 3: Check Published Packages
After workflow completes:

- **PyPI**: https://pypi.org/project/kore-fileformat/
- **npm**: https://www.npmjs.com/package/kore-fileformat
- **crates.io**: https://crates.io/crates/kore_fileformat
- **Maven Central**: https://search.maven.org/artifact/com.arun.kore/kore-cloud-java

### Step 4: Delete Test Tag
```bash
# Delete local tag
git tag -d v1.0.0-test

# Delete remote tag
git push --delete origin v1.0.0-test
```

---

## 📋 Publishing for Real Releases

Once verified, here's how to publish a real release:

### For v1.0.0 (Already Released)
```bash
# Already tagged, just verify packages published
# Check: PyPI, npm, crates.io, Maven
```

### For v1.0.1 (Patch Release)
```bash
cd c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore

# Update version numbers
# Edit: Cargo.toml, pyproject.toml, package.json, pom.xml
# Version: 1.0.1

# Commit changes
git add Cargo.toml pyproject.toml package.json pom.xml
git commit -m "chore: Bump version to 1.0.1"
git push origin main

# Create and push tag
git tag -a v1.0.1 -m "Release v1.0.1: Bug fixes"
git push origin v1.0.1

# Watch GitHub Actions automatically publish to all registries!
```

### For v1.1.0 (Minor Release)
```bash
# Same process as above, but with version 1.1.0
```

---

## 🔒 Security Best Practices

### For GitHub Secrets
- ✅ **Rotate tokens annually** - Regenerate tokens yearly
- ✅ **Use specific scopes** - Only grant necessary permissions
- ✅ **Monitor usage** - Check GitHub Actions logs for token usage
- ✅ **Don't share credentials** - Keep secrets confidential
- ✅ **Use environment protection** - Require approval for releases

### For Local Credentials
- ✅ **Secure GPG passphrase** - Use password manager
- ✅ **Backup GPG key** - Export and save securely
- ✅ **Don't commit secrets** - Never add to git history
- ✅ **Revoke if compromised** - Regenerate immediately

---

## 🐛 Troubleshooting

### "Publishing failed: Unauthorized (401)"

**Cause**: Incorrect token or secret not added  
**Fix**:
1. Verify secret exists: `https://github.com/arunkatherashala/Kore/settings/secrets/actions`
2. Verify token is current (not expired)
3. Regenerate token if needed
4. Update GitHub secret with new token

### "Maven: Group ID not approved"

**Cause**: OSSRH namespace not yet approved  
**Fix**:
1. Check Jira issue for approval status
2. If rejected, see feedback and resubmit
3. If approved, ensure username/password are correct

### "GPG signature failed"

**Cause**: Invalid GPG key or passphrase  
**Fix**:
1. Verify GPG_PASSPHRASE is correct
2. Verify GPG key is published to keyservers
3. Check key expiration: `gpg --list-keys`
4. If expired, create new key

### "npm: 403 Forbidden"

**Cause**: Token missing write permissions  
**Fix**:
1. Go to npm account settings
2. Regenerate token with "Read and write" permissions
3. Update NPM_TOKEN secret

---

## 📞 Support

If you encounter issues:

1. **Check logs**: Go to GitHub Actions → workflow run → check logs
2. **Review documentation**: See [CI_CD_SECRETS_SETUP.md](CI_CD_SECRETS_SETUP.md)
3. **Contact support**:
   - Email: arunkatherashala@gmail.com
   - GitHub Issues: https://github.com/arunkatherashala/Kore/issues

---

## ✨ Next Steps

After setting up secrets:

1. ✅ All 6 secrets configured
2. ✅ Test tag published successfully
3. ✅ Verified packages in all registries
4. ✅ Ready for production releases!

**Next**: Create v1.1.0 release branch and begin development!

---

**Setup Complete!** 🎉

Your Kore v1.0.0 is now configured for automated, multi-registry publishing!

**Last Updated**: May 14, 2026  
**Status**: Ready for Configuration ✅
