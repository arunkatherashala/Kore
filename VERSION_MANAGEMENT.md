# 🔄 Version Management Guide

## Problem Solved
**Before:** Version numbers were hardcoded in `pyproject.toml`, `Cargo.toml`, and `README.md`, causing:
- ❌ Mismatched versions across files
- ❌ Stale version info in README/PyPI
- ❌ Manual sync errors

**Now:** Versions are automatically synced from a single source of truth (pyproject.toml).

---

## 🎯 Version Source of Truth

### Primary Source
**`pyproject.toml`** - The source of truth for ALL versions
- Update version here first
- All other files auto-sync from this

### Secondary Sources (Auto-Synced)
- `Cargo.toml` - Synced from pyproject.toml
- `README.md` - Synced from pyproject.toml

---

## 📝 Workflow for Version Bumps

### Step 1: Update pyproject.toml
```bash
# Edit pyproject.toml
version = "1.3.4"  # Change only here
```

### Step 2: Run sync script
```bash
python scripts/sync_versions.py
```

**Output:**
```
✅ Version check passed: 1.3.4
✅ Updated README.md with version 1.3.4
✅ Updated Cargo.toml to 1.3.4
✅ All versions synced to 1.3.4
```

### Step 3: Commit
```bash
git add -A
git commit -m "chore: bump version to 1.3.4"
git push
```

---

## 🔐 Automatic Checks

### GitHub Actions (`version-sync.yml`)
Runs on every PR and push to main:
- ✅ Checks version consistency across files
- ✅ Fails PR if versions mismatch
- ✅ Auto-fixes on main branch

### Pre-Commit Hook (Optional)
Install to auto-sync before committing locally:
```bash
cp scripts/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

Now every `git commit` automatically syncs versions.

---

## 🚫 What Not To Do

### ❌ DON'T manually edit multiple files
```bash
# Wrong! Causes mismatches
vi pyproject.toml    # change 1.3.3 → 1.3.4
vi Cargo.toml        # change 1.3.3 → 1.3.4
vi README.md         # change 1.3.1 → 1.3.4 (oops, different!)
```

### ❌ DON'T hardcode versions in README
```bash
# Wrong! Gets out of sync
pip install kore-fileformat==1.3.1  # Now outdated
```

### ✅ DO use the sync script
```bash
# Right! Auto-syncs everything
python scripts/sync_versions.py
```

---

## 🔍 Verify Versions Match

```bash
# Quick check
grep version pyproject.toml
grep version Cargo.toml
grep "pip install" README.md | grep "=="
```

Should all show the same version number.

---

## 📚 Files Involved

| File | Role | Updated By |
|------|------|-----------|
| `pyproject.toml` | Source of truth | Manual (you) |
| `Cargo.toml` | Synced | `sync_versions.py` |
| `README.md` | Synced | `sync_versions.py` |
| `.github/workflows/version-sync.yml` | CI check | GitHub Actions |
| `scripts/sync_versions.py` | Automation | Runs on demand / pre-commit |
| `scripts/pre-commit` | Local hook | Runs before each commit |

---

## ⚡ Quick Reference

```bash
# Check versions
python scripts/sync_versions.py

# Fix mismatches
python scripts/sync_versions.py  # Auto-fixes

# Bump version
vi pyproject.toml               # Edit version
python scripts/sync_versions.py # Sync everything
git add -A && git commit -m "chore: bump to X.Y.Z"
```

---

## 🎉 Result

✅ Single source of truth (pyproject.toml)  
✅ No more version mismatches  
✅ No more stale README info on PyPI  
✅ Automatic enforcement via CI/CD  
✅ One command to sync everything  

**Never worry about version consistency again!**
