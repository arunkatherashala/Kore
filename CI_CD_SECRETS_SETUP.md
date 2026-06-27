# CI/CD Publishing Secrets Setup Guide

This guide helps you configure GitHub secrets to enable automated publishing of Kore v1.0.0 to all package registries.

## 📋 Required Secrets Summary

| Secret Name | Registry | Purpose | Expires |
|---|---|---|---|
| `CARGO_TOKEN` | crates.io | Publish Rust crate | Never (tokens don't expire) |
| `PYPI_TOKEN` | PyPI | Publish Python wheels via Maturin | Configurable (recommend 90 days) |
| `MAVEN_CENTRAL_USERNAME` | Maven Central | OSSRH account username | N/A |
| `MAVEN_CENTRAL_PASSWORD` | Maven Central | OSSRH account password | N/A |
| `GPG_PASSPHRASE` | Maven Central | Sign JAR files | N/A |
| `NPM_TOKEN` | npm | Publish JavaScript package | Configurable (recommend 1 year) |

---

## 🚀 Step-by-Step Setup

### 1️⃣ **CARGO_TOKEN** (crates.io)

#### Prerequisites
- crates.io account (free at https://crates.io/users/login)
- Published Rust crate namespace reserved

#### Steps
1. Go to https://crates.io/me (logged in)
2. Click **API Tokens** in sidebar
3. Click **New Token**
4. Name: `github-actions-kore`
5. Scopes: Check `publish-new` and `publish-update`
6. Click **Create**
7. Copy the token value

#### Add to GitHub
```
Repo → Settings → Secrets and variables → Actions
Click "New repository secret"
Name: CARGO_TOKEN
Value: <paste token>
```

---

### 2️⃣ **PYPI_TOKEN** (PyPI via Maturin)

#### Prerequisites
- PyPI account (free at https://pypi.org/account/register/)
- Package namespace `kore-fileformat` reserved on PyPI

#### Steps
1. Go to https://pypi.org/account/tokens/ (logged in)
2. Click **Add API Token**
3. Token name: `github-actions-kore`
4. Scope: Select **Entire account** or specific project
5. Click **Create token**
6. Copy the token (starts with `pypi-`)

#### Add to GitHub
```
Repo → Settings → Secrets and variables → Actions
Click "New repository secret"
Name: PYPI_TOKEN
Value: <paste token>
```

---

### 3️⃣ **MAVEN_CENTRAL_USERNAME** & **MAVEN_CENTRAL_PASSWORD**

Maven Central uses OSSRH (Sonatype JIRA) for authentication and account management.

#### Prerequisites
- OSSRH Sonatype account (free at https://issues.sonatype.org)
- JIRA project created for Kore: https://issues.sonatype.org/projects/KORE

#### Steps (OSSRH Jira Setup)
1. Go to https://issues.sonatype.org (create account if needed)
2. Create new JIRA project:
   - Project key: `KORE`
   - Project name: `Kore File Format`
   - Project type: Community License
3. Request namespace: Create ticket requesting `com.arun.kore` namespace
   - Sonatype team will approve (usually 1-2 business days)
4. Verify Sonatype confirms namespace in JIRA ticket

#### Steps (Credentials)
1. Navigate to Profile Settings in OSSRH
2. Under "User Token" section, click **Generate Token**
3. Copy username and password/token

#### Add to GitHub (2 separate secrets)
```
Secret 1:
Name: MAVEN_CENTRAL_USERNAME
Value: <username from token>

Secret 2:
Name: MAVEN_CENTRAL_PASSWORD
Value: <password from token>
```

#### Maven Configuration (pom.xml already configured)
The workflow uses `pom.xml` which contains:
```xml
<server>
  <id>ossrh</id>
  <username>${env.MAVEN_CENTRAL_USERNAME}</username>
  <password>${env.MAVEN_CENTRAL_PASSWORD}</password>
</server>
```

---

### 4️⃣ **GPG_PASSPHRASE** (JAR Signing)

Maven Central requires GPG signatures for JAR files.

#### Prerequisites
- GPG installed on your machine: https://gnupg.org/download/
- GPG key pair created (run: `gpg --gen-key`)
- Key ID known and distributed to key servers

#### Steps
1. Create GPG keypair (if not done):
   ```bash
   gpg --gen-key
   # Choose RSA, 4096 bits, set passphrase
   # Get key ID: gpg --list-keys
   ```

2. Export public key:
   ```bash
   gpg --export-options export-minimal,export-clean --export <KEY_ID> | \
     gpg --armor > pubkey.asc
   ```

3. Upload to key server:
   ```bash
   gpg --keyserver keys.openpgp.org --send-key <KEY_ID>
   ```

4. Configure in `pom.xml` (already done):
   ```xml
   <gpg.executable>gpg</gpg.executable>
   <gpg.passphrase>${env.GPG_PASSPHRASE}</gpg.passphrase>
   ```

#### Add to GitHub
```
Repo → Settings → Secrets and variables → Actions
Click "New repository secret"
Name: GPG_PASSPHRASE
Value: <your GPG passphrase>
```

⚠️ **Security Warning**: Never commit GPG passphrases to git. Use GitHub secrets only.

---

### 5️⃣ **NPM_TOKEN** (npm Registry)

#### Prerequisites
- npm account (free at https://www.npmjs.com)
- Package namespace `@yourscope/kore-fileformat` reserved (or public `kore-fileformat`)

#### Steps
1. Go to https://www.npmjs.com/settings/~/tokens (logged in)
2. Click **Generate new token**
3. Token type: Select **Automation**
   - Automation tokens don't expire and are ideal for CI/CD
4. Scopes: Select **Read and publish**
5. Click **Generate token**
6. Copy the token (starts with `npm_`)

#### Add to GitHub
```
Repo → Settings → Secrets and variables → Actions
Click "New repository secret"
Name: NPM_TOKEN
Value: <paste token>
```

---

## ✅ Verification Checklist

After adding all 6 secrets, verify setup:

```bash
# 1. Check all secrets added to GitHub
curl -H "Authorization: token <GITHUB_PAT>" \
  https://api.github.com/repos/arunkatherashala/Kore/actions/secrets

# 2. Verify crates.io token (optional)
cargo login --registry crates <CARGO_TOKEN>

# 3. Verify PyPI token (optional)
pip install twine
twine auth --service pypi

# 4. Verify npm token (optional)
npm config set //registry.npmjs.org/:_authToken=<NPM_TOKEN>
```

---

## 🚀 Triggering Automated Publishing

Once all secrets are configured, publishing is automatic on version tags:

### To publish v1.0.0 to all registries:

1. Tag the release:
   ```bash
   git tag -a v1.0.0 -m "Release v1.0.0"
   git push origin v1.0.0
   ```

2. GitHub Actions will automatically:
   - ✅ Run integration tests
   - ✅ Build Java JAR + publish to Maven Central
   - ✅ Build Python wheels + publish to PyPI
   - ✅ Build JavaScript module + publish to npm
   - ✅ Publish Rust crate to crates.io

3. Monitor progress at: https://github.com/arunkatherashala/Kore/actions

---

## 📚 Reference Links

### Registries
- **crates.io API Tokens**: https://crates.io/me
- **PyPI API Tokens**: https://pypi.org/account/tokens/
- **OSSRH JIRA**: https://issues.sonatype.org
- **npm Tokens**: https://www.npmjs.com/settings/~/tokens

### Documentation
- **Cargo Publishing**: https://doc.rust-lang.org/cargo/commands/cargo-publish.html
- **Maturin PyPI**: https://maturin.rs/publish
- **Maven Central Guide**: https://maven.apache.org/repository/
- **npm Publishing**: https://docs.npmjs.com/cli/v10/commands/npm-publish

---

## 🔒 Security Best Practices

1. **Use Automation Tokens**: Choose automation/CI scopes where available
2. **Scope Permissions Narrowly**: Limit token scope to only necessary actions
3. **Set Expiration Dates**: Use shorter expiry (90 days) and rotate regularly
4. **Never Commit Secrets**: Always use GitHub secrets, never hardcode
5. **Audit Access**: Monitor which workflows use which secrets
6. **Rotate Keys**: Replace tokens every 6-12 months

---

## ❓ Troubleshooting

### "Token validation failed"
- Verify token format (some registries have prefixes like `npm_`, `pypi-`)
- Check token scope/permissions
- Ensure no extra whitespace in secret value

### "403 Unauthorized"
- Token may be expired
- Check token still valid on registry website
- Regenerate and update secret if needed

### "Package already exists"
- Don't publish to same version twice
- Increment version number in Cargo.toml/package.json
- Create new tag and push

### GitHub Actions won't start
- Check workflow file syntax: `.github/workflows/cloud-connectors.yml`
- Verify file paths and feature gates are correct
- Check repo settings: Actions must be enabled

---

**Status**: Ready to configure! 🎉 Once secrets are added, v1.0.0+ releases will publish automatically.
