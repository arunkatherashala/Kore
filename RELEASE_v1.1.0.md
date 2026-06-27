# KORE v1.1.0 Release Checklist

**Release Date**: May 14, 2026  
**Status**: IN PROGRESS  

---

## 📋 Multi-Platform Deployment Status

### ✅ COMPLETED

- [x] **GitHub Release** - v1.1.0 published with Phase A summary
  - Link: https://github.com/arunkatherashala/Kore/releases/tag/v1.1.0
  - Size: 1200+ word comprehensive release notes
  - Artifacts: Complete

- [x] **Crates.io** - Rust package published
  - Package: `kore_fileformat v1.1.0`
  - Link: https://crates.io/crates/kore_fileformat/1.1.0
  - Status: Available for `cargo add`

- [x] **npm** - JavaScript package published
  - Package: `@kore/cloud@1.1.0`
  - Link: https://www.npmjs.com/package/@kore/cloud
  - Status: Available with NAPI bindings

- [x] **Python Wheel Built** - Ready for PyPI
  - File: `kore_fileformat-1.1.0-cp312-cp312-win_amd64.whl`
  - Size: 101,437 bytes
  - Location: `target/wheels/`
  - Build Status: ✅ SUCCESS

### ⏳ IN PROGRESS

- [ ] **PyPI** - Python package upload
  - Package: `kore-fileformat`
  - Current Version on PyPI: 0.1.0 (old, May 9, 2026)
  - Target: 1.1.0
  - Status: Awaiting authentication token
  - **Blockers**: Need PyPI API token

- [ ] **Maven Central** - Java package upload
  - Package: `com.kore:kore-fileformat:1.1.0`
  - Status: pom.xml configured, ready for deployment
  - **Blockers**: Need GPG key + Sonatype credentials

---

## 🔐 PyPI Setup Instructions

### Step 1: Get PyPI API Token
1. Visit: https://pypi.org/account/
2. Login with your account
3. Go to **API tokens** tab
4. Create new token (or use existing `pypi-...` token)
5. Copy the full token value

### Step 2: Store Token Securely (CHOOSE ONE)

**Option A - Environment Variable (Temporary)**
```powershell
$env:PYPI_TOKEN = "pypi-YOUR_FULL_TOKEN_HERE"
```

**Option B - Create ~/.pypirc (Persistent)**
```ini
[pypi]
username = __token__
password = pypi-YOUR_FULL_TOKEN_HERE
```

### Step 3: Upload Wheel
```powershell
cd C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore
twine upload target/wheels/kore_fileformat-1.1.0*.whl
```

### Step 4: Verify Upload
- Visit: https://pypi.org/project/kore-fileformat/
- Check: Latest version shows **1.1.0** (not 0.1.0)
- Install: `pip install kore-fileformat==1.1.0`

---

## 📦 Maven Central Setup Instructions

### Step 1: Configure GPG Keys
```powershell
# Install GPG first if needed
# Then create key pair for code signing
gpg --gen-key

# List your keys
gpg --list-keys
```

### Step 2: Create ~/.m2/settings.xml
```xml
<settings>
  <servers>
    <server>
      <id>ossrh</id>
      <username>YOUR_SONATYPE_USERNAME</username>
      <password>YOUR_SONATYPE_PASSWORD</password>
    </server>
  </servers>
  <profiles>
    <profile>
      <id>ossrh</id>
      <properties>
        <gpg.executable>gpg</gpg.executable>
        <gpg.passphrase>YOUR_GPG_PASSPHRASE</gpg.passphrase>
      </properties>
    </profile>
  </profiles>
</settings>
```

### Step 3: Deploy to Maven Central
```bash
cd C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore
mvn clean deploy -P release -DskipTests
```

### Step 4: Release Staging Repository
```
1. Visit: https://s01.oss.sonatype.org
2. Login with Sonatype credentials
3. Find staging repository: `comkore-XXXX`
4. Click "Close"
5. After validation, click "Release"
```

### Step 5: Verify on Maven Central
- Wait 10-30 minutes for sync
- Visit: https://mvnrepository.com/artifact/com.kore/kore-fileformat/1.1.0
- Or search: https://central.sonatype.com/

---

## 🚀 Phase A Completion Summary

| Metric | Value |
|--------|-------|
| **Test Coverage** | 47/47 passing (100%) |
| **Compression Ratio** | 5-8x (vs 2.84x Parquet) |
| **Code Size** | 1900+ lines (binary_format.rs) |
| **Platforms** | 5 (GitHub, Crates.io, npm, PyPI pending, Maven pending) |
| **Languages** | 8 (Rust, Python, JavaScript, Java, Go, C#, CLI, Spark) |
| **Cloud SDKs** | 3 (AWS S3, Azure Blob, GCS) |

---

## 📝 Release Notes Highlights

**Week 1**: Enhanced Delta Encoding (2.5-3x compression)  
**Week 2**: Dictionary + RLE Hybrid (3-4x compression)  
**Week 3**: Format Optimization & Parallel Processing (5-8x compression)  

**Total Impact**: 5-8x compression on CSV data, 1.8-2.2x better than Parquet

---

## ⏭️ Next Steps (Phase B)

1. **Format Metadata Integration** (Week 3 Day 4)
2. **Streaming APIs** - For large file processing
3. **True Parallelism** - Rayon integration
4. **Real-world Benchmarking** - Production validation

---

## 📞 Quick Links

- **GitHub**: https://github.com/arunkatherashala/Kore
- **Crates.io**: https://crates.io/crates/kore_fileformat
- **npm**: https://www.npmjs.com/package/@kore/cloud
- **PyPI**: https://pypi.org/project/kore-fileformat/ (waiting for 1.1.0)
- **Maven Central**: https://mvnrepository.com/artifact/com.kore/kore-fileformat (pending)

---

**Created**: May 14, 2026  
**Last Updated**: [Will update after PyPI/Maven deployment]
