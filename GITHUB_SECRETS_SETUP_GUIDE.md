# 🔐 GitHub Secrets Configuration for Kore v1.1.0 Automated Publishing

**Status**: Complete Step-by-Step Setup Guide  
**Target**: Enable automated publishing to 4 registries (crates.io, PyPI, Maven Central, npm)  
**Time Required**: 30-45 minutes

---

## 📋 Overview

This guide walks you through configuring 6 GitHub secrets that enable the CI/CD pipeline to automatically publish Kore packages to:

1. **crates.io** - Rust packages (`CARGO_TOKEN`)
2. **PyPI** - Python packages (`PYPI_TOKEN`)
3. **Maven Central** - Java packages (`MAVEN_CENTRAL_USERNAME`, `MAVEN_CENTRAL_PASSWORD`)
4. **npm Registry** - JavaScript packages (`NPM_TOKEN`)
5. **GPG** - Signing releases (`GPG_PASSPHRASE`)

---

## 🔑 Secret #1: Cargo Registry Token (crates.io)

### Step 1: Create crates.io Account

Visit [crates.io](https://crates.io/) and create a free account if you don't have one.

### Step 2: Generate API Token

1. Log in to [crates.io](https://crates.io/)
2. Click your **Profile** (top right)
3. Select **Account Settings** → **API Tokens**
4. Click **Generate New Token**
5. Name it: `github-actions-kore`
6. Copy the token (starts with `crte_`)

### Step 3: Save Token to GitHub

1. Go to your Kore repository on GitHub
2. **Settings** → **Secrets and variables** → **Actions**
3. Click **New repository secret**
4. **Name**: `CARGO_TOKEN`
5. **Value**: Paste your crates.io token
6. Click **Add secret**

### Verification
```bash
cd C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore
cargo publish --token ${{ secrets.CARGO_TOKEN }} --dry-run
```

---

## 🔑 Secret #2: PyPI Token

### Step 1: Create PyPI Account

Visit [pypi.org](https://pypi.org) and create a free account.

### Step 2: Generate API Token

1. Log in to [pypi.org](https://pypi.org/)
2. Click your **Account Settings** (top right)
3. Scroll down to **API Tokens**
4. Click **Create token**
5. **Token Name**: `github-actions-kore`
6. **Scope**: Entire account (or project-specific)
7. Copy the token (starts with `pypi-`)

### Step 3: Save Token to GitHub

1. **Settings** → **Secrets and variables** → **Actions**
2. Click **New repository secret**
3. **Name**: `PYPI_TOKEN`
4. **Value**: Paste your PyPI token
5. Click **Add secret**

### Verification
```bash
# In your workflow, you can test with:
python -m twine upload --repository pypi dist/* \
  --username __token__ --password ${{ secrets.PYPI_TOKEN }} \
  --skip-existing
```

---

## 🔑 Secret #3 & #4: Maven Central Credentials

### Step 1: Create Sonatype JIRA Account

1. Go to [issues.sonatype.org](https://issues.sonatype.org)
2. Click **Sign up** (top right)
3. Create an account with email: `arunkatherashala@gmail.com`
4. **Organization ID**: `com.kore`
5. **Project URL**: `https://github.com/arunkatherashala/Kore`

### Step 2: Create Maven Central Account

1. Log in to [Sonatype Nexus](https://oss.sonatype.org/)
2. Click **Sign Up** (top left)
3. Create account with:
   - **Username**: `kore-publisher` (or your choice)
   - **Email**: `arunkatherashala@gmail.com`
   - **Password**: Strong password (save securely!)

### Step 3: Request Namespace

1. Log in to [Sonatype JIRA](https://issues.sonatype.org)
2. Create a new **issue**:
   - **Project**: Community Developers
   - **Type**: New Project
   - **Summary**: "Hosting for Kore columnar file format"
   - **Group ID**: `com.kore`
   - **Project URL**: `https://github.com/arunkatherashala/Kore`
   - **SCM URL**: `https://github.com/arunkatherashala/Kore.git`

Wait for approval (usually 24-48 hours).

### Step 4: Save Credentials to GitHub

Once approved:

1. **Settings** → **Secrets and variables** → **Actions**
2. Create `MAVEN_CENTRAL_USERNAME`:
   - **Value**: Your Sonatype username
3. Create `MAVEN_CENTRAL_PASSWORD`:
   - **Value**: Your Sonatype password

### Workflow Configuration

```xml
<!-- In pom.xml -->
<distributionManagement>
  <snapshotRepository>
    <id>ossrh</id>
    <url>https://oss.sonatype.org/content/repositories/snapshots</url>
  </snapshotRepository>
  <repository>
    <id>ossrh</id>
    <url>https://oss.sonatype.org/service/local/staging/deploy/maven2/</url>
  </repository>
</distributionManagement>
```

```yaml
# In GitHub Actions workflow
- name: Publish to Maven Central
  run: |
    mvn deploy \
      -DskipTests \
      -Dusername=${{ secrets.MAVEN_CENTRAL_USERNAME }} \
      -Dpassword=${{ secrets.MAVEN_CENTRAL_PASSWORD }}
```

---

## 🔑 Secret #5: NPM Registry Token

### Step 1: Create npm Account

1. Go to [npmjs.com](https://www.npmjs.com/)
2. Click **Sign Up** (top right)
3. Create account with email: `arunkatherashala@gmail.com`

### Step 2: Generate Access Token

1. Log in to [npm](https://www.npmjs.com/)
2. Click your **Profile** (top right) → **Access Tokens**
3. Click **Generate New Token**
4. **Token Type**: Automation
5. **Token Name**: `github-actions-kore`
6. Copy the token (starts with `npm_`)

### Step 3: Save Token to GitHub

1. **Settings** → **Secrets and variables** → **Actions**
2. Click **New repository secret**
3. **Name**: `NPM_TOKEN`
4. **Value**: Paste your npm token
5. Click **Add secret**

### Verification

```bash
npm config set //registry.npmjs.org/:_authToken=${{ secrets.NPM_TOKEN }}
npm publish
```

---

## 🔑 Secret #6: GPG Passphrase (Optional for Signing)

### Step 1: Generate GPG Key (If Not Already Done)

```bash
gpg --full-generate-key
# Follow prompts:
# - Kind: RSA and RSA
# - Size: 4096
# - Expiration: 1 year (or your preference)
# - Name: Sai Arun Kumar Ktherashala
# - Email: arunkatherashala@gmail.com
# - Passphrase: (create strong passphrase)
```

### Step 2: Export Public Key

```bash
gpg --armor --export arunkatherashala@gmail.com > public-key.asc
# Upload public key to keyserver:
# https://keys.openpgp.org/
# or
gpg --send-keys --keyserver keyserver.ubuntu.com <KEY_ID>
```

### Step 3: Export Secret Key

```bash
gpg --armor --export-secret-keys arunkatherashala@gmail.com > private-key.asc
# KEEP THIS PRIVATE!
```

### Step 4: Save Passphrase to GitHub

1. **Settings** → **Secrets and variables** → **Actions**
2. Click **New repository secret**
3. **Name**: `GPG_PASSPHRASE`
4. **Value**: Your GPG passphrase
5. Click **Add secret**

### Workflow Configuration

```yaml
- name: Import GPG key
  env:
    GPG_PRIVATE_KEY: ${{ secrets.GPG_PRIVATE_KEY }}
    GPG_PASSPHRASE: ${{ secrets.GPG_PASSPHRASE }}
  run: |
    echo "$GPG_PRIVATE_KEY" | gpg --import
    gpg --list-secret-keys
```

---

## ✅ Verification Checklist

After adding all 6 secrets, verify they're configured correctly:

```powershell
# In GitHub Actions, you can test each secret:

# 1. Test Cargo Token
cargo login ${{ secrets.CARGO_TOKEN }}
cargo publish --dry-run

# 2. Test PyPI Token
python -m twine check dist/*

# 3. Test Maven Credentials
mvn clean package
mvn deploy -DskipTests

# 4. Test npm Token
npm config set //registry.npmjs.org/:_authToken=${{ secrets.NPM_TOKEN }}
npm publish --dry-run

# 5. Test GPG Passphrase
gpg --list-secret-keys
```

---

## 🚀 Complete GitHub Actions Workflow Example

See `.github/workflows/cloud-connectors.yml` for the complete CI/CD pipeline that uses all 6 secrets:

```yaml
name: Publish All Platforms

on:
  push:
    tags:
      - 'v*'

jobs:
  # Python wheels to PyPI
  publish-python:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: PyO3/maturin-action@v1
        with:
          command: publish
          args: --token ${{ secrets.PYPI_TOKEN }}

  # Rust to crates.io
  publish-crate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - run: cargo publish --token ${{ secrets.CARGO_TOKEN }}

  # Java to Maven Central
  publish-java:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - run: |
          mvn deploy \
            -Dusername=${{ secrets.MAVEN_CENTRAL_USERNAME }} \
            -Dpassword=${{ secrets.MAVEN_CENTRAL_PASSWORD }}

  # JavaScript to npm
  publish-javascript:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - run: npm publish --token ${{ secrets.NPM_TOKEN }}
```

---

## 🔒 Security Best Practices

✅ **DO:**
- Use unique tokens for each registry
- Regenerate tokens periodically (every 6-12 months)
- Limit token scope to what's necessary
- Store passphrases securely
- Audit GitHub secrets quarterly

❌ **DON'T:**
- Share tokens in code or documentation
- Use personal access tokens instead of registry-specific tokens
- Commit `.env` files with secrets
- Use the same token across multiple projects
- Share secrets via email or chat

---

## 🛠️ Troubleshooting

### Secret Not Found in Workflow
- Verify secret name matches exactly (case-sensitive)
- Ensure secret was saved (GitHub should show checkmark)
- Wait a few seconds after adding secret before triggering workflow

### Authentication Failed
- Verify token hasn't expired
- Check token has correct scope/permissions
- Ensure credentials match the registry
- Test manually: `cargo login`, `npm login`, etc.

### Publishing Still Fails
- Check GitHub Actions logs for detailed error
- Verify version isn't already published
- Ensure package metadata is correct
- For Maven, verify namespace was approved

---

## 📞 Support

For issues with registries:

- **crates.io**: [crates.io/me](https://crates.io/me)
- **PyPI**: [pypi.org/account](https://pypi.org/account/)
- **Maven Central**: [issues.sonatype.org](https://issues.sonatype.org)
- **npm**: [npmjs.com/settings/profile](https://www.npmjs.com/settings/profile)

---

## 🎉 Next Steps

Once all 6 secrets are configured:

1. Tag a release: `git tag v1.1.0 && git push origin v1.1.0`
2. GitHub Actions automatically publishes to all 4 registries
3. Verify packages appear in each registry
4. Announce release to community

**Congratulations!** Kore v1.1.0 is now available on all major package registries! 🚀

---

**Configuration Status:**
- [x] CARGO_TOKEN (crates.io)
- [ ] PYPI_TOKEN (PyPI)
- [ ] MAVEN_CENTRAL_USERNAME (Maven)
- [ ] MAVEN_CENTRAL_PASSWORD (Maven)
- [ ] NPM_TOKEN (npm)
- [ ] GPG_PASSPHRASE (Signing)

**Last Updated**: May 14, 2026  
**For v1.1.0 Release Cycle**
