# KORE FileFormat — Deployment Guide

How to release a new version to all 6 package registries.

---

## Step 1 — Bump Version Everywhere

Run this PowerShell block from the repo root, replacing `OLD` and `NEW`:

```powershell
$OLD = "1.6.7"
$NEW = "1.6.8"   # <-- change this

$files = @(
  "pyproject.toml",
  "kore-python/kore_fileformat.py",
  "kore-python/README.md",
  "kore-node/package.json",
  "kore-node/README.md",
  "kore-ruby/kore_fileformat.gemspec",
  "kore-ruby/README.md",
  "kore-go/go.mod",
  "kore-go/README.md",
  "kore-php/composer.json",
  "kore-php/README.md",
  "maven/README.md",
  "csharp/Kore.FileFormat/README.md",
  "kore-standalone/Cargo.toml",
  "kore-standalone/README.md"
)
foreach ($f in $files) {
  (Get-Content $f -Raw) -replace [regex]::Escape($OLD), $NEW | Set-Content $f -NoNewline
}
# C# has x.x.x.x pattern too
(Get-Content "csharp/Kore.FileFormat/Kore.FileFormat.csproj" -Raw) `
  -replace "$OLD.0", "$NEW.0" `
  -replace $OLD, $NEW `
  | Set-Content "csharp/Kore.FileFormat/Kore.FileFormat.csproj" -NoNewline
# Java pom.xml
(Get-Content "maven/pom.xml" -Raw) `
  -replace "<version>$OLD</version>", "<version>$NEW</version>" `
  | Set-Content "maven/pom.xml" -NoNewline

Write-Host "Done — verify with: Select-String -Path pyproject.toml -Pattern 'version'"
```

---

## Step 2 — Commit, Tag, Push

```powershell
git add -A
git commit -m "chore: release v$NEW"
git tag "v$NEW"
git push origin fileformat
git push origin fileformat:refs/heads/fileformat
git push origin "v$NEW"
```

> **Important:** `fileformat` is the permanent default branch for kore-fileformat SDK. GitHub Actions reads workflows from this branch for all tag-triggered events.

---

## Step 3 — What Triggers Automatically

Pushing a `v*` tag automatically triggers these workflows:

| Workflow | Registry | Secret Used |
|----------|----------|-------------|
| `publish-pypi.yml` | PyPI | `PYPI_AUG` |
| `publish-nodejs.yml` | npm | `NPM_TOKEN` |
| `publish-crates.yml` | crates.io | `CARGO_REGISTRY_TOKEN` |
| `publish-rubygems.yml` | RubyGems | `RUBYGEMS_API_KEY` |
| `publish-maven.yml` | Maven Central | `CENTRAL_PORTAL_TOKEN_USERNAME` + `CENTRAL_PORTAL_TOKEN_PASSWORD` |

Check all runs at: https://github.com/arunkatherashala/Kore/actions

---

## Step 4 — NuGet (manual until secret is added)

NuGet does NOT yet have a secret. After adding `NUGET_API_KEY`:
1. Go to: https://github.com/arunkatherashala/Kore/actions/workflows/publish-nuget.yml
2. Click **"Run workflow"** → select branch `fileformat` → **Run workflow**

---

## GitHub Secrets Required

Add/update these at: **https://github.com/arunkatherashala/Kore/settings/secrets/actions**

| Secret Name | Registry | Where to Get |
|-------------|----------|--------------|
| `PYPI_AUG` | PyPI | pypi.org → Account → API Tokens → Add token |
| `NPM_TOKEN` | npm | npmjs.com → Avatar → Access Tokens → New Granular Token (Read+Write, Bypass 2FA ✅, 90 days) |
| `CARGO_REGISTRY_TOKEN` | crates.io | crates.io → Account Settings → API Tokens → New Token (publish-new + publish-update) |
| `RUBYGEMS_API_KEY` | RubyGems | rubygems.org → Edit Profile → API Keys |
| `CENTRAL_PORTAL_TOKEN_USERNAME` | Maven | central.sonatype.com → Account → Generate Token (username part) |
| `CENTRAL_PORTAL_TOKEN_PASSWORD` | Maven | central.sonatype.com → Account → Generate Token (password part) |
| `NUGET_API_KEY` | NuGet | nuget.org → API Keys → Create |

---

## Package File Locations

| Language | Version File | Registry Package Name |
|----------|-------------|----------------------|
| Python | `pyproject.toml` + `kore-python/kore_fileformat.py` | `kore-fileformat` |
| Node.js | `kore-node/package.json` | `kore-fileformat` |
| Ruby | `kore-ruby/kore_fileformat.gemspec` | `kore-fileformat` |
| Go | `kore-go/go.mod` (comment only) | module tag `vX.Y.Z` |
| PHP | `kore-php/composer.json` | `arunkatherashala/kore-fileformat` |
| C# | `csharp/Kore.FileFormat/Kore.FileFormat.csproj` | `KoreFileFormat` |
| Java | `maven/pom.xml` | `com.github.arunkatherashala:kore-fileformat` |
| Rust | `kore-standalone/Cargo.toml` | `kore_fileformat` |

---

## npm Token Notes

npm tokens expire every **90 days** (max for write-access tokens).  
When publishing fails with `403 Forbidden` + "Two-factor authentication required":
1. Go to: https://www.npmjs.com/settings/arunkatherashala/tokens/granular-access-tokens/new
2. Name: `github-kore-publish-ci`
3. Check ✅ **"Bypass two-factor authentication (2FA)"**
4. Packages and scopes → **Read and write** → **All packages**
5. Expiration → **90 days**
6. Generate → copy token → update `NPM_TOKEN` secret on GitHub

---

## Verify Deployments

| Registry | Check URL |
|----------|-----------|
| PyPI | https://pypi.org/project/kore-fileformat/ |
| npm | https://www.npmjs.com/package/kore-fileformat |
| crates.io | https://crates.io/crates/kore_fileformat |
| RubyGems | https://rubygems.org/gems/kore-fileformat |
| Maven | https://central.sonatype.com/artifact/com.github.arunkatherashala/kore-fileformat |
| NuGet | https://www.nuget.org/packages/KoreFileFormat |
| Go | https://pkg.go.dev/github.com/arunkatherashala/kore/kore-go |

---

## Current Status (as of 2026-08-08)

| Registry | Version | Status |
|----------|---------|--------|
| PyPI | 1.6.6 | ✅ LIVE |
| npm | 1.6.7 | ✅ LIVE |
| crates.io | 1.6.7 | ✅ LIVE |
| RubyGems | deployed | ✅ LIVE |
| Maven Central | deployed | ✅ LIVE |
| NuGet | — | ❌ Needs NUGET_API_KEY secret |
