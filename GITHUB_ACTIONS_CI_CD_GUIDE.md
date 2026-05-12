# KORE v1.0.0 - GitHub Actions CI/CD Automation Setup Guide

## Overview
KORE v1.0.0 now has automated CI/CD pipelines that publish to all major package registries when you push a version tag (like `v1.0.0`).

## 🚀 How Automation Works

1. **Push a tag** (e.g., `git push origin v1.0.0`)
2. **GitHub Actions automatically**:
   - Builds all language bindings
   - Publishes to 6 package registries in parallel
   - Generates deployment summary
3. **All platforms updated** simultaneously

## 📦 Supported Publishing Platforms

| Platform | Registry | Workflow | Requires |
|----------|----------|----------|----------|
| Python | PyPI | `publish-pypi.yml` | `PYPI_TOKEN` |
| JavaScript | npm | `publish-nodejs.yml` | `NPM_TOKEN` |
| Java | Maven Central | `publish-maven.yml` | `OSSRH_*` + `MAVEN_GPG_PASSPHRASE` |
| Containers | Docker Hub | `publish-docker.yml` | `DOCKER_USERNAME`, `DOCKER_PASSWORD` |
| .NET | NuGet | `publish-nuget.yml` | `NUGET_API_KEY` |
| Ruby | RubyGems | `publish-rubygems.yml` | `RUBYGEMS_API_KEY` |

## 🔐 GitHub Secrets Setup

To enable automated publishing, add these secrets to your GitHub repository:

### Repository Settings → Secrets and variables → Actions

#### 1. PyPI (Python Publishing)
```
Secret Name: PYPI_TOKEN
Value: Get from https://pypi.org/manage/account/token/
Type: Create a "Project-specific" or "Account-wide" token
```

#### 2. npm (JavaScript Publishing)
```
Secret Name: NPM_TOKEN
Value: Get from https://www.npmjs.com/settings/your-username/tokens
Type: Create an "Automation" token (allows publish)
```

#### 3. Maven Central (Java Publishing)
```
Secret Names:
- OSSRH_USERNAME: Your Sonatype JIRA username
- OSSRH_PASSWORD: Your Sonatype JIRA password
- MAVEN_GPG_PASSPHRASE: Your GPG key passphrase

Get from: https://central.sonatype.org/
Setup: Follow Sonatype OSSRH setup guide
```

#### 4. Docker Hub (Container Publishing)
```
Secret Names:
- DOCKER_USERNAME: Your Docker Hub username
- DOCKER_PASSWORD: Your Docker Hub personal access token (not password!)

Get from: https://hub.docker.com/settings/security
Create a personal access token with read/write permissions
```

#### 5. NuGet (.NET Publishing)
```
Secret Name: NUGET_API_KEY
Value: Get from https://www.nuget.org/account/ApiKeys
Type: Create a new API key with "Push" permission
```

#### 6. RubyGems (Ruby Publishing)
```
Secret Name: RUBYGEMS_API_KEY
Value: Get from https://rubygems.org/profile/api_keys
Type: Create new API key
```

## ⚙️ Workflow Files

### Master Orchestrator
- **`publish-all-platforms.yml`** - Runs all publishers in parallel, generates summary

### Individual Publishers
- **`publish-pypi.yml`** - Python packages
- **`publish-nodejs.yml`** - JavaScript bindings (already existed)
- **`publish-maven.yml`** - Java/Hadoop integration
- **`publish-docker.yml`** - Container images
- **`publish-nuget.yml`** - .NET packages
- **`publish-rubygems.yml`** - Ruby gems

## 🎯 Triggering Automated Publishing

### Option 1: Via Git Tag (Recommended for Production)
```bash
git tag v1.0.0
git push origin v1.0.0
# GitHub Actions automatically starts publishing to all platforms
```

### Option 2: Manual Trigger in GitHub UI
1. Go to Actions tab
2. Select "publish-all-platforms"
3. Click "Run workflow"
4. All platforms publish

### Option 3: GitHub Release
1. Go to Releases
2. Create new release with tag `v1.0.0`
3. Workflows trigger automatically

## 📊 Monitoring Publishing Status

1. Go to **Actions** tab in GitHub
2. Click on **publish-all-platforms** workflow
3. View **Deployment Summary** which shows:
   - PyPI: `success` / `failure` / `skipped`
   - npm: `success` / `failure` / `skipped`
   - Maven: `success` / `failure` / `skipped`
   - Docker: `success` / `failure` / `skipped`
   - NuGet: `success` / `failure` / `skipped`
   - RubyGems: `success` / `failure` / `skipped`

## ✅ Verification

After publishing completes, verify on each platform:

```bash
# PyPI
pip search kore-fileformat  # or check https://pypi.org/project/kore-fileformat/

# npm
npm view kore-fileformat version

# Docker Hub
docker pull saiarunkumar/kore:1.0.0

# Maven Central
# Check https://central.sonatype.com/artifact/io.kore/kore-hadoop/1.0.0

# NuGet
dotnet package search kore-fileformat

# RubyGems
gem search kore-fileformat
```

## 🔄 Current Status (v1.0.0)

- ✅ GitHub workflows configured for all 6 platforms
- ✅ v1.0.0 tag created and pushed
- ✅ All source code on GitHub
- ✅ npm package already published (v0.4.0)
- ⏳ Awaiting GitHub secrets to activate automated publishing

## 📝 Next Steps

1. **Add GitHub Secrets** - Follow the setup guide above
2. **Test Workflows** - Manually trigger one workflow to verify setup
3. **Monitor Publishing** - Check Actions tab for results
4. **Verify Packages** - Confirm on each registry (see Verification section)

## 💡 Tips

- **Optional Platforms**: Workflows with `continue-on-error: true` don't fail if credentials missing
- **Selective Publishing**: Edit `publish-all-platforms.yml` to disable specific platforms
- **Custom Triggers**: Add new triggers (e.g., on release published, on schedule, etc.)
- **Notifications**: Add Slack/email notifications by extending workflows

## 🆘 Troubleshooting

| Issue | Solution |
|-------|----------|
| "403 Forbidden" on PyPI | Check PYPI_TOKEN format (should be `pypi-*`) |
| npm publish fails | Verify NPM_TOKEN is "Automation" type token |
| Docker push fails | Ensure DOCKER_PASSWORD is personal access token, not password |
| Maven fails | Check OSSRH credentials and GPG setup |
| NuGet fails | Verify NuGet API key has "Push" permission |
| RubyGems fails | Ensure gem structure exists in repo root |

## 📚 References

- [PyPI Publishing](https://packaging.python.org/en/latest/guides/publishing-package-distribution-releases-to-pypi/)
- [npm Publishing](https://docs.npmjs.com/packages-and-modules/contributing-packages-to-the-registry)
- [Maven Central Publishing](https://central.sonatype.org/publish/publish-guide/)
- [Docker Hub Publishing](https://docs.docker.com/docker-hub/)
- [NuGet Publishing](https://learn.microsoft.com/en-us/nuget/nuget-org/publish-a-package)
- [RubyGems Publishing](https://guides.rubygems.org/publishing/)

---

**Status**: Automated CI/CD pipelines ready. Add GitHub secrets to activate.
