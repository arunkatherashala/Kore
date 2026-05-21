# Kore v1.2.1 — Multi-Platform Distribution Guide

**Last Updated**: May 21, 2026  
**Current Version**: 1.2.1  
**Distribution Channels**: 8 Platforms ✅

---

## 📦 Complete Distribution Channels

### 1. **Python (PyPI)**
- **Package**: `kore-fileformat`
- **Registry**: https://pypi.org/project/kore-fileformat/
- **Install**: `pip install kore-fileformat==1.2.1`
- **Status**: ✅ Published
- **Workflow**: `.github/workflows/publish-pypi.yml`
- **Authentication**: OIDC Trusted Publishers (PyPI)

**Latest Releases**:
```bash
pip install kore-fileformat  # Latest
pip install kore-fileformat==1.2.1  # Specific version
```

---

### 2. **.NET / NuGet**
- **Package**: `kore-fileformat`
- **Registry**: https://www.nuget.org/packages/kore-fileformat/
- **Install**: `dotnet add package kore-fileformat --version 1.2.1`
- **Status**: ✅ Published
- **Workflow**: `.github/workflows/publish-nuget.yml`
- **Authentication**: GitHub Secrets (`NUGET_API_KEY`)

**Supported Frameworks**:
- .NET 6.0, 7.0, 8.0
- .NET Framework 4.7.2, 4.8
- .NET Standard 2.1

**Installation via Package Manager Console**:
```powershell
Install-Package kore-fileformat -Version 1.2.1
```

**Installation via PackageReference**:
```xml
<PackageReference Include="kore-fileformat" Version="1.2.1" />
```

---

### 3. **Ruby (RubyGems)**
- **Package**: `kore-fileformat`
- **Registry**: https://rubygems.org/gems/kore-fileformat/
- **Install**: `gem install kore-fileformat --version 1.2.1`
- **Status**: ✅ Published
- **Workflow**: `.github/workflows/publish-ruby.yml`
- **Authentication**: GitHub Secrets (`RUBYGEMS_API_KEY`)

**Gemfile**:
```ruby
gem 'kore-fileformat', '~> 1.2.1'
```

---

### 4. **JavaScript/Node.js (npm)**
- **Package**: `kore-fileformat`
- **Registry**: https://www.npmjs.com/package/kore-fileformat/
- **Install**: `npm install kore-fileformat@1.2.1`
- **Status**: ✅ Published
- **Workflow**: `.github/workflows/publish-nodejs.yml`
- **Authentication**: GitHub Secrets (`NPM_TOKEN`)

**package.json**:
```json
{
  "dependencies": {
    "kore-fileformat": "^1.2.1"
  }
}
```

---

### 5. **Java (Maven Central)**
- **Package**: `kore-fileformat`
- **Group ID**: `com.korefileformat`
- **Artifact ID**: `kore-fileformat`
- **Registry**: Maven Central (https://mvnrepository.com/artifact/com.korefileformat/kore-fileformat)
- **Status**: ✅ Published
- **Workflow**: `.github/workflows/publish-maven.yml`
- **Authentication**: GitHub Secrets (`MAVEN_USERNAME`, `MAVEN_PASSWORD`, `MAVEN_GPG_PASSPHRASE`)

**pom.xml**:
```xml
<dependency>
    <groupId>com.korefileformat</groupId>
    <artifactId>kore-fileformat</artifactId>
    <version>1.2.1</version>
</dependency>
```

---

### 6. **Rust (Crates.io)**
- **Crate**: `kore_fileformat`
- **Registry**: https://crates.io/crates/kore_fileformat/
- **Install**: `cargo add kore_fileformat@1.2.1`
- **Status**: ✅ Published
- **Workflow**: `.github/workflows/publish-crates.yml`
- **Authentication**: GitHub Secrets (`CARGO_REGISTRY_TOKEN`)

**Cargo.toml**:
```toml
[dependencies]
kore_fileformat = "1.2.1"
```

---

### 7. **Docker (GHCR - GitHub Container Registry)**
- **Image**: `ghcr.io/arunkatherashala/kore:latest`
- **Registry**: https://github.com/arunkatherashala/Kore/pkgs/container/kore
- **Status**: ✅ Published
- **Workflow**: `.github/workflows/publish-docker.yml`
- **Authentication**: GitHub Secrets (`GITHUB_TOKEN` - built-in)

**Pull & Run**:
```bash
docker pull ghcr.io/arunkatherashala/kore:latest
docker run ghcr.io/arunkatherashala/kore:latest
```

**Purpose**: Multi-language development reference image with Rust, Python, Java, .NET, Node.js, Go, C#, and Ruby environments.

---

### 8. **GitHub Releases**
- **Repository**: https://github.com/arunkatherashala/Kore/releases/tag/v1.2.1
- **Artifacts**: Native binaries, source code, release notes
- **Status**: ✅ Published
- **Workflow**: `.github/workflows/publish-release.yml`
- **Authentication**: `secrets.GITHUB_TOKEN` (built-in)

**Available Artifacts**:
- Source code (tar.gz, zip)
- Native binary: `kore_fileformat.dll` (Windows x64)
- Documentation
- Compiled JAR, Gem, Wheel archives

---

## 🔄 Automated Release Workflow

### Trigger Mechanism
All platforms publish automatically on **git tag push** matching pattern `v*`:

```bash
# Push a tag to trigger all 8 platform workflows simultaneously
git tag v1.2.1
git push origin v1.2.1
```

**This triggers ALL 8 platforms**:
- PyPI ✅
- NuGet ✅
- RubyGems ✅
- npm ✅
- Maven Central ✅
- Crates.io ✅
- Docker/GHCR ✅
- GitHub Releases ✅

### Manual Trigger
Trigger individual workflows:

```bash
# PyPI
gh workflow run publish-pypi.yml -R arunkatherashala/Kore --ref main

# NuGet
gh workflow run publish-nuget.yml -R arunkatherashala/Kore --ref main

# RubyGems
gh workflow run publish-ruby.yml -R arunkatherashala/Kore --ref main

# npm
gh workflow run publish-nodejs.yml -R arunkatherashala/Kore --ref main

# Maven
gh workflow run publish-maven.yml -R arunkatherashala/Kore --ref main

# Crates.io
gh workflow run publish-crates.yml -R arunkatherashala/Kore --ref main

# Docker
gh workflow run publish-docker.yml -R arunkatherashala/Kore --ref main

# GitHub Release
gh workflow run publish-release.yml -R arunkatherashala/Kore --ref main
```

---

## 📋 Release Process Checklist

### Step 1: Update Version Numbers
Update **4 configuration files**:

```bash
# 1. Rust (Cargo.toml)
version = "1.2.1"

# 2. Python (pyproject.toml)
version = "1.2.1"

# 3. Python init (__init__.py)
__version__ = "1.2.1"

# 4. Node.js (package.json)
"version": "1.2.1"

# 5. .NET (csproj) - [optional if using Auto-versioning]
<Version>1.2.1</Version>

# 6. Ruby (gemspec) - updated automatically via VERSION file
```

### Step 2: Build & Test Locally
```bash
# Rust
cargo build --release
cargo test

# Python
python -m pytest tests/

# .NET
dotnet test kore-fileformat-nuget/

# Ruby
bundle exec rspec spec/

# Node.js
npm test
```

### Step 3: Commit Changes
```bash
git add .
git commit -m "v1.2.1: Release with [features/fixes]"
git push origin develop-v1.1.6
```

### Step 4: Create & Push Tag
```bash
git tag -a v1.2.1 -m "v1.2.1: Kore release"
git push origin v1.2.1
```

### Step 5: Verify All Platforms (10-15 minutes)
Check GitHub Actions: https://github.com/arunkatherashala/Kore/actions

```bash
# Monitor specific workflow
gh run list --workflow="publish-pypi.yml" -R arunkatherashala/Kore --limit 1 --json status,conclusion

# Check overall status
gh run list -R arunkatherashala/Kore --limit 8
```

---

## 🔐 Secrets Configuration

### Required GitHub Secrets
Set in **Settings → Secrets and variables → Actions**:

| Secret Name | Source | Platform | Required |
|------------|--------|----------|----------|
| `PYPI_API_TOKEN` | PyPI (OIDC) | PyPI | ✅ OIDC only |
| `NUGET_API_KEY` | NuGet.org | NuGet | ✅ |
| `RUBYGEMS_API_KEY` | RubyGems.org | Ruby | ✅ |
| `NPM_TOKEN` | npm registry | npm | ✅ |
| `MAVEN_USERNAME` | Sonatype OSSRH | Maven | ✅ |
| `MAVEN_PASSWORD` | Sonatype OSSRH | Maven | ✅ |
| `MAVEN_GPG_PASSPHRASE` | GPG key passphrase | Maven | ✅ |
| `CARGO_REGISTRY_TOKEN` | Crates.io | Rust | ✅ |
| `GITHUB_TOKEN` | Built-in | Docker/Release | Built-in ✅ |

### Create Secrets:
```bash
gh secret set RUBYGEMS_API_KEY --body "your_rubygems_token"
gh secret set NPM_TOKEN --body "your_npm_token"
gh secret set NUGET_API_KEY --body "your_nuget_token"
# ... etc
```

---

## 🧪 Testing Across All Platforms

### Pre-Release Testing

**Python**:
```bash
pytest tests/ -v
pip install kore-fileformat==1.2.1
python -c "from kore_fileformat import compress; print('✅ Python works')"
```

**NuGet**:
```bash
dotnet test kore-fileformat-nuget/
dotnet add package kore-fileformat --version 1.2.1
```

**Ruby**:
```bash
bundle exec rspec spec/
gem install kore-fileformat --version 1.2.1
ruby -e "require 'kore_fileformat'; puts '✅ Ruby works'"
```

**npm**:
```bash
npm test
npm install kore-fileformat@1.2.1
node -e "require('kore-fileformat'); console.log('✅ npm works')"
```

**Java**:
```bash
mvn test
# Add to pom.xml and build
mvn clean install
```

**Rust**:
```bash
cargo test --release
cargo add kore_fileformat@1.2.1
```

---

## 🐛 Troubleshooting

### Platform-Specific Issues

| Issue | Platform | Solution |
|-------|----------|----------|
| "version already exists" | PyPI / npm / RubyGems | Increment version, create new tag |
| "403 Forbidden" | Maven | Check `MAVEN_USERNAME/PASSWORD` secrets |
| "Invalid tag format" | Docker/GHCR | Remove special chars from tag name |
| "Cannot authenticate" | npm | Verify `NPM_TOKEN` is valid |
| "Process completed with exit code 1" | Ruby | Check if version exists; add `continue-on-error: true` |
| "Resource not accessible" | GitHub Release | Add `permissions: contents: write` to workflow |

### Check Workflow Logs:
```bash
# View last run
gh run list --workflow="publish-pypi.yml" -R arunkatherashala/Kore --limit 1

# Get detailed logs
gh run view <RUN_ID> -R arunkatherashala/Kore --log
```

---

## 📊 Release Status Dashboard

**v1.2.1 Status (May 21, 2026)**

| Platform | Status | Version | Last Updated |
|----------|--------|---------|--------------|
| Python (PyPI) | ✅ Live | 1.2.1 | May 21, 2026 |
| .NET (NuGet) | ✅ Live | 1.2.1 | May 21, 2026 |
| Ruby (RubyGems) | ✅ Live | 1.2.1 | May 21, 2026 |
| JavaScript (npm) | ✅ Live | 1.2.1 | May 21, 2026 |
| Java (Maven) | ✅ Live | 1.2.1 | May 21, 2026 |
| Rust (Crates.io) | ✅ Live | 1.2.1 | May 21, 2026 |
| Docker (GHCR) | ✅ Live | latest | May 21, 2026 |
| GitHub Release | ✅ Live | v1.2.1 | May 21, 2026 |

**Build Tests**: 
- ✅ 11 NuGet xUnit tests (PASS)
- ✅ 10+ Ruby RSpec tests (PASS)
- ✅ 50+ Python pytest tests (PASS)
- ✅ 40+ npm tests (PASS)

---

## 🚀 Quick Links

- **GitHub Repository**: https://github.com/arunkatherashala/Kore
- **Actions Workflows**: https://github.com/arunkatherashala/Kore/actions
- **Release Tags**: https://github.com/arunkatherashala/Kore/releases
- **Documentation**: https://github.com/arunkatherashala/Kore/blob/main/README.md

---

## 📝 Version History

| Version | Date | Platforms | Notes |
|---------|------|-----------|-------|
| 1.2.1 | May 21, 2026 | 8/8 ✅ | Ruby + NuGet workflows fixed, all green ✅ |
| 1.2.0 | May 2026 | 6/8 | Added NuGet, Ruby support |
| 1.1.6 | Earlier | 4/8 | PyPI, npm, Maven, Crates.io |
| 1.0.0 | Earlier | 1/8 | Rust only |

---

**Maintained by**: Arun Kather Ashala  
**Last Verified**: May 21, 2026 (All 8 platforms live ✅)
