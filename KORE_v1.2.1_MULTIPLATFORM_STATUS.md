# Kore v1.2.1 - Multi-Platform Release Status Report

**Release Date:** May 21, 2026  
**Version:** 1.2.1  
**Total Platforms:** 8  
**Confirmed Live:** 6 ✅  
**Pending Investigation:** 2 (Maven Central, Docker Hub)

---

## Platform Summary

| # | Platform | Status | URL | Command | Notes |
|---|----------|--------|-----|---------|-------|
| 1 | **PyPI** (Python) | ✅ LIVE | https://pypi.org/project/kore-fileformat/1.2.1 | `pip install kore-fileformat==1.2.1` | Wheels built via maturin, OIDC auth |
| 2 | **npm** (JavaScript/Node.js) | ✅ LIVE | https://www.npmjs.com/package/kore-fileformat/v/1.2.1 | `npm install kore-fileformat@1.2.1` | n-api bindings, 2FA token required |
| 3 | **NuGet** (.NET/C#) | ✅ LIVE | https://www.nuget.org/packages/kore-fileformat/1.2.1 | `dotnet add package kore-fileformat --version 1.2.1` | Native Windows bindings |
| 4 | **RubyGems** (Ruby) | ✅ LIVE | https://rubygems.org/gems/kore-fileformat/versions/1.2.1 | `gem install kore-fileformat -v 1.2.1` | FFI bindings, UNIX/Linux focus |
| 5 | **Crates.io** (Rust) | ✅ LIVE | https://crates.io/crates/kore-fileformat/1.2.1 | `cargo add kore-fileformat@1.2.1` | Native Rust implementation |
| 6 | **GHCR** (Docker) | ✅ LIVE | `docker pull ghcr.io/arunkatherashala/kore:1.2.1` | Multi-language reference image | Built-in GitHub token, automatic |
| 7 | **Maven Central** (Java) | 🔄 INVESTIGATING | https://search.maven.org | `mvn dependency:get -Dartifact=io.github.arunkatherashala:kore-fileformat:1.2.1:jar` | Build succeeds, deploy auth fails |
| 8 | **Docker Hub** | ⏳ REQUIRES SETUP | `docker pull arunkatherashala/kore:1.2.1` | Workflow created, needs secrets | DOCKERHUB_USERNAME + DOCKERHUB_TOKEN |

---

## Detailed Platform Status

### ✅ 1. PyPI (Python Wheels)
- **Workflow:** `.github/workflows/publish-pypi.yml`
- **Status:** LIVE & VERIFIED
- **Latest:** v1.2.1 available on PyPI
- **Build System:** maturin (Rust + Python)
- **Authentication:** OIDC Trusted Publishers (no token needed)
- **Verification:**
  ```bash
  pip index versions kore-fileformat | grep 1.2.1
  ```

### ✅ 2. npm (JavaScript/Node.js)
- **Workflow:** `.github/workflows/publish-nodejs.yml`
- **Status:** LIVE & VERIFIED
- **Latest:** v1.2.1 available on npm
- **Build System:** n-api bindings (Node native extensions)
- **Authentication:** npm token (2FA bypass enabled)
- **Special Note:** Required special token with 2FA bypass due to 2FA requirement on npm
- **Verification:**
  ```bash
  npm view kore-fileformat@1.2.1
  ```

### ✅ 3. NuGet (.NET/C#)
- **Workflow:** `.github/workflows/publish-nuget.yml`
- **Status:** LIVE & VERIFIED
- **Latest:** v1.2.1 available on NuGet
- **Build System:** .NET wrapper around native bindings
- **Authentication:** NuGet API token
- **Verification:**
  ```bash
  dotnet nuget search kore-fileformat --exact-match
  ```

### ✅ 4. RubyGems (Ruby)
- **Workflow:** `.github/workflows/publish-rubygems.yml`
- **Status:** LIVE & VERIFIED
- **Latest:** v1.2.1 available on RubyGems
- **Build System:** FFI bindings (Foreign Function Interface)
- **Authentication:** RubyGems API token
- **Verification:**
  ```bash
  gem list -r kore-fileformat
  ```

### ✅ 5. Crates.io (Rust)
- **Workflow:** `.github/workflows/publish-crates.yml`
- **Status:** LIVE & VERIFIED
- **Latest:** v1.2.1 available on Crates.io
- **Build System:** Pure Rust (source root Cargo.toml)
- **Authentication:** Crates.io API token
- **Verification:**
  ```bash
  cargo search kore-fileformat
  ```

### ✅ 6. GHCR (Docker - GitHub Container Registry)
- **Workflow:** `.github/workflows/publish-docker.yml`
- **Status:** LIVE & VERIFIED
- **Image:** `ghcr.io/arunkatherashala/kore:1.2.1` and `ghcr.io/arunkatherashala/kore:latest`
- **Build System:** Multi-language reference image (Rust, Python, JS, Java, Go, C#, Ruby)
- **Authentication:** Built-in `secrets.GITHUB_TOKEN` (automatic)
- **Verification:**
  ```bash
  docker pull ghcr.io/arunkatherashala/kore:1.2.1
  docker run ghcr.io/arunkatherashala/kore:1.2.1 --version
  ```

---

## 🔄 Investigating: Maven Central (Java)

### Current Status
- **Workflow:** `.github/workflows/publish-maven.yml`
- **Build Result:** ✅ BUILD SUCCESS (JAR created correctly)
- **Deploy Result:** ❌ 401 - Unauthorized
- **Root Cause:** Maven authentication not working despite multiple fixes

### What We've Tried
1. ✅ Created proper `maven/pom.xml` with correct coordinates:
   - groupId: `io.github.arunkatherashala`
   - artifactId: `kore-fileformat`
   - version: `1.2.1`
2. ✅ Created KoreFileFormat.java main entry class
3. ✅ Configured Maven settings.xml with credentials
4. ❌ Tried multiple approaches to pass authentication (failed due to variable expansion, here-doc issues)

### Latest Error
```
[ERROR] Nexus connection problem to URL [https://s01.oss.sonatype.org/ ]: 401 - Unauthorized
```

### Recommended Fix
The Maven deployment requires EITHER:
1. **Option A - GPG Signing (Preferred by Maven Central)**
   - Need to set up GPG key in secrets (`MAVEN_GPG_PRIVATE_KEY`, `MAVEN_GPG_PASSPHRASE`)
   - Requires importing GPG key in workflow before deploy
   - More secure but complex setup

2. **Option B - Check Credentials**
   - Verify `MAVEN_USERNAME` and `MAVEN_PASSWORD` secrets are correct
   - Test credentials manually against https://s01.oss.sonatype.org
   - May require new OSSRH token if old one expired

3. **Option C - Use Maven Central Web UI** (Workaround)
   - Manually upload JAR from `maven/target/kore-fileformat-1.2.1.jar` at https://central.sonatype.com

### Testing Maven Deployment Locally
```bash
cd maven
mvn clean deploy -Dusername=YOUR_USERNAME -Dpassword=YOUR_PASSWORD
```

---

## ⏳ Setup Required: Docker Hub

### Current Status
- **Workflow:** `.github/workflows/publish-docker-hub.yml`
- **Status:** Created but not yet triggered (needs secrets)
- **Image Name:** `arunkatherashala/kore:1.2.1`

### What's Needed
1. **Add GitHub Secrets** (once only):
   ```bash
   gh secret set DOCKERHUB_USERNAME -R arunkatherashala/Kore
   gh secret set DOCKERHUB_TOKEN -R arunkatherashala/Kore
   ```

2. **Manual Trigger** (after secrets added):
   ```bash
   gh workflow run publish-docker-hub.yml -R arunkatherashala/Kore --ref main
   ```

### Getting Docker Hub Credentials
1. Go to https://hub.docker.com/settings/security
2. Create a new **Personal Access Token** (with read/write repo permission)
3. Use your Docker Hub username and the token above

### Verify After Publishing
```bash
docker pull arunkatherashala/kore:1.2.1
```

---

## Version Consistency Check

All version sources synchronized to **1.2.1**:
- ✅ `Cargo.toml` (Rust root)
- ✅ `pyproject.toml` (Python)
- ✅ `kore_fileformat/__init__.py` (Python package init)
- ✅ `nodejs/package.json` (npm package)
- ✅ `nodejs/Cargo.toml` (Node.js native bindings)
- ✅ `maven/pom.xml` (Java package)
- ✅ Git tag `v1.2.1` (release trigger)

---

## Automation Notes

### How Releases Work
1. Push git tag: `git tag v1.2.1 && git push origin v1.2.1`
2. GitHub Actions triggers all 8 workflows in parallel
3. Each platform publishes independently (if authenticated)

### Workflow Files Structure
```
.github/workflows/
├── publish-pypi.yml              ✅ (OIDC auth - fully automated)
├── publish-nodejs.yml            ✅ (npm token - 2FA bypass)
├── publish-nuget.yml             ✅ (NuGet token)
├── publish-rubygems.yml          ✅ (RubyGems token)
├── publish-crates.yml            ✅ (Crates.io token)
├── publish-docker.yml            ✅ (GitHub token - automatic)
├── publish-maven.yml             🔄 (Authentication issue)
└── publish-docker-hub.yml        ⏳ (Needs DOCKER secrets)
```

---

## Recommendations

### Short Term (Next Release)
1. Resolve Maven Central authentication issue (priority: HIGH)
2. Set up Docker Hub secrets and confirm workflow (priority: HIGH)
3. Once done: All 8 platforms will be automated ✅

### Long Term  
1. Consider adding:
   - Go language bindings + Go package registry
   - Homebrew formula for macOS
   - Debian/Ubuntu PPA repositories
   - Conda-forge distribution

2. Create automated integration tests that verify all 8 platforms have latest version

3. Set up monitoring/alerts if a platform release fails

---

## Success Metrics

| Metric | Target | Current |
|--------|--------|---------|
| Platforms Live | 8/8 | 6/8 (75%) |
| Automated Workflows | 8/8 | 6/8 (75%) |
| Zero Manual Steps | Yes | Mostly (need Docker Hub secrets + Maven fix) |
| Release Time | <10 min | ~5-7 min (one tag push = all platforms) |

---

## Testing Checklist

Use this to verify all 8 platforms after release:

```bash
# Python
pip install kore-fileformat==1.2.1 && python -c "import kore_fileformat; print(kore_fileformat.__version__)"

# Node.js
npm install kore-fileformat@1.2.1 && node -e "console.log(require('kore-fileformat').version())"

# .NET
dotnet add package kore-fileformat --version 1.2.1

# Ruby
gem install kore-fileformat -v 1.2.1

# Rust
cargo add kore-fileformat@1.2.1

# Docker (GHCR)
docker pull ghcr.io/arunkatherashala/kore:1.2.1

# Java (Maven Central) - once fixed
mvn dependency:get -Dartifact=io.github.arunkatherashala:kore-fileformat:1.2.1:jar

# Docker (Hub) - once setup
docker pull arunkatherashala/kore:1.2.1
```

---

## Summary

Kore v1.2.1 has been **successfully published to 6 out of 8 registry platforms**. The Python, JavaScript, .NET, Ruby, Rust, and Docker (GHCR) ecosystems now have access to the latest version. 

**Next steps:** Resolve Maven Central deployment authentication and configure Docker Hub secrets to achieve full 8/8 platform coverage.

---

*Last Updated: 2026-05-21*  
*Status: MOSTLY COMPLETE - 6/8 platforms live, 2 pending*
