# 🎯 KORE - Installation & Testing Workflow Guide

**Quick Answer:** You DON'T need to uninstall/reinstall for tests!

---

## 📋 Workflows by Scenario

### Scenario 1: **DEVELOPMENT TESTING** (Most Common)
```bash
# For developers working on KORE code

# Step 1: Make code changes
# ... edit src/*.rs files ...

# Step 2: Run tests (NO install needed!)
cargo test --lib --release

# Step 3: Check warnings
cargo clippy --lib --release

# Step 4: Build release
cargo build --release --lib

# Done! Tests run on local code, no install needed.
```

**Why no install?** Cargo tests run against your local source code in `/target/release/deps`, not against installed packages.

---

### Scenario 2: **USER INSTALLATION TESTING** (Full Cycle)
```bash
# For testing KORE as an end-user (after release)

# Step 1: Install ONCE
pip install kore-fileformat  # or npm, maven

# Step 2: Use it in your project
python -c "import kore_fileformat; ..."
# or JavaScript:
# const kore = require('kore-fileformat');
# or Java:
# <dependency><artifactId>kore-fileformat</artifactId>...

# Step 3: Test thoroughly
# ... run your tests, benchmarks, etc ...

# Step 4: Uninstall when done (if needed)
pip uninstall kore-fileformat -y

# Step 5: Clean reinstall (if testing upgrade)
pip install kore-fileformat==1.1.6  # new version
```

**Why this way?** You install once, use it like normal users would, then uninstall. No repetitive install/uninstall between each test.

---

### Scenario 3: **CI/CD PIPELINE** (Automated)
```bash
# In GitHub Actions / CI server

# Step 1: Checkout code
git clone https://github.com/arunkatherashala/Kore.git

# Step 2: Build from source (no pip install)
cargo build --release --lib

# Step 3: Run all tests once
cargo test --lib --release

# Step 4: If all pass, publish to PyPI/Maven/npm
# (workflows auto-trigger on git tag)
git tag v1.1.5
git push origin v1.1.5

# The release workflows handle installation to PyPI, Maven, npm
# (users then: pip install, npm install, maven add dependency)
```

**Why this way?** CI/CD builds from source, tests locally, then publishes to registries. No install/uninstall.

---

### Scenario 4: **UPGRADING/DOWNGRADING** (When Needed)
```bash
# Example: Test if v1.1.4 works, then upgrade to v1.1.5

# Step 1: Install v1.1.4
pip install kore-fileformat==1.1.4

# Step 2: Test v1.1.4
python test_kore_install.py

# Step 3: Upgrade (NOT uninstall + reinstall)
pip install --upgrade kore-fileformat==1.1.5

# Step 4: Test v1.1.5
python test_kore_install.py

# No uninstall needed! pip --upgrade handles it.
```

---

## ✅ **CORRECT WORKFLOW SUMMARY**

| Scenario | Uninstall First? | Install Count | Install Between Tests? |
|----------|------------------|---------------|----------------------|
| **Dev Testing** | N/A (no pip) | 0 | No (use `cargo test`) |
| **User Testing** | Maybe later | 1 | No (use it once) |
| **CI/CD** | N/A | 0 | No (build from source) |
| **Upgrade** | No! | 1 (upgrade) | No (use --upgrade) |
| **Downgrade** | Only if upgrade fails | 1 | No (use specific version) |

---

## ❌ MISTAKES TO AVOID

### ❌ **WRONG:** Uninstall-test-reinstall loop
```bash
# DON'T DO THIS - inefficient and unnecessary!
pip uninstall kore-fileformat -y
pip install kore-fileformat
python test.py
pip uninstall kore-fileformat -y  # repeat 100x
pip install kore-fileformat
```

### ❌ **WRONG:** Running tests without cargo
```bash
# DON'T DO THIS - requires installation first
python -c "from kore_fileformat import compress"
# (will fail unless installed)
```

### ❌ **WRONG:** Mixing cargo tests and pip installs
```bash
# DON'T DO THIS - confusing and redundant
cargo test  # tests local code
pip install kore-fileformat  # installs wheels
python test_script.py  # tests installed package
# (different code paths!)
```

---

## ✅ **BEST PRACTICES**

### For Developers
```bash
# 1. Make changes to src/
# 2. Test with cargo (tests local code)
cargo test --lib --release

# 3. If all pass, commit
git add -A
git commit -m "feature: ..."

# 4. No pip install needed during development!
```

### For Release Testing
```bash
# 1. Install from PyPI
pip install kore-fileformat==1.1.5

# 2. Create test script (test_real_install.py)
python test_kore_install.py

# 3. Run integration tests
# (test against real installation)

# 4. Report any issues
# (GitHub issues)

# 5. Uninstall only if upgrading
pip install --upgrade kore-fileformat==1.1.6
```

### For CI/CD
```bash
# .github/workflows/test.yml
- name: Build KORE
  run: cargo build --release --lib

- name: Run all tests
  run: cargo test --lib --release

# If tests pass, auto-publish (separate workflow)
```

---

## 📊 **TEST QUICK START**

### Option 1: Development (Recommended for coding)
```bash
cd c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore
cargo test --lib --release
# Result: 355 passed, 0 failed, ~0.5s
```

### Option 2: User Installation (Recommended for users)
```bash
# If Python 3.9+ is available:
pip install kore-fileformat
python test_kore_install.py
# Result: ✅ Tests PASSED - KORE IS READY TO USE!
```

### Option 3: npm (For JavaScript)
```bash
npm install kore-fileformat
node test_kore_install.js
# Result: ✅ Tests PASSED - KORE IS READY TO USE!
```

---

## 🔄 **EXAMPLE: PROPER TEST CYCLE**

```bash
# Week 1: Feature development
cd Kore
cargo test --lib --release          # ✅ 355 passed
git commit -m "add feature X"

# Week 2: Release preparation  
git tag v1.1.5
git push origin v1.1.5
# GitHub Actions auto-publishes to PyPI/npm/Maven

# Week 3: User testing
pip install kore-fileformat==1.1.5   # Install once
python test_kore_install.py          # Test once
# ... use KORE in real project ...
pip uninstall kore-fileformat -y     # Cleanup (optional)

# Zero uninstall-reinstall cycles! ✅
```

---

## 📋 **QUICK REFERENCE TABLE**

| Action | Command | Install First? | Uninstall First? |
|--------|---------|-----------------|------------------|
| Code change → test | `cargo test --lib --release` | ❌ No | N/A |
| New release → publish | `git tag v1.1.5 && git push` | ❌ No | N/A |
| Install & test | `pip install && python test.py` | ✅ Yes (once) | ❌ No |
| Upgrade version | `pip install --upgrade` | ✅ Yes (auto) | ❌ No |
| Switch version | `pip install kore==1.1.4` | ✅ Yes (auto) | ❌ No |
| Full cleanup | `pip uninstall` | N/A | ✅ Yes |

---

## 🎯 **SUMMARY**

**The Golden Rule:**
> **Don't uninstall/reinstall between tests!** 
> 
> - **Developers**: Use `cargo test` (no pip involved)
> - **Users**: Install once, test as needed, upgrade when available
> - **CI/CD**: Build from source, test locally, publish to registries

This is faster, cleaner, and matches how everyone else uses packages! 🚀

---

##Files Referenced

- [INSTALLATION_TESTING_GUIDE.md](INSTALLATION_TESTING_GUIDE.md) - Detailed installation steps
- [test_kore_install.py](test_kore_install.py) - Python verification script
- [test_kore_install.js](test_kore_install.js) - JavaScript verification script

---

**Status: ✅ 355/355 tests passing | 0 warnings | Ready for production!**
