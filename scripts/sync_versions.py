#!/usr/bin/env python3
"""
Sync version numbers across all project files.
Reads version from pyproject.toml and updates README.md, Cargo.toml, etc.
Run before building: python scripts/sync_versions.py
"""

import re
import sys
from pathlib import Path

def get_version_from_pyproject():
    """Extract version from pyproject.toml"""
    pyproject = Path("pyproject.toml")
    with open(pyproject, encoding='utf-8') as f:
        content = f.read()
    
    match = re.search(r'version\s*=\s*"([^"]+)"', content)
    if match:
        return match.group(1)
    raise ValueError("Could not find version in pyproject.toml")

def get_version_from_cargo():
    """Extract version from Cargo.toml"""
    cargo = Path("Cargo.toml")
    with open(cargo, encoding='utf-8') as f:
        content = f.read()
    
    match = re.search(r'version\s*=\s*"([^"]+)"', content)
    if match:
        return match.group(1)
    raise ValueError("Could not find version in Cargo.toml")

def update_readme(version):
    """Update all version references in README.md"""
    readme = Path("README.md")
    with open(readme, encoding='utf-8') as f:
        content = f.read()
    
    # Update pip install commands
    content = re.sub(
        r'pip install kore-fileformat==[\d.]+',
        f'pip install kore-fileformat=={version}',
        content
    )
    
    with open(readme, 'w', encoding='utf-8') as f:
        f.write(content)
    
    print(f"✅ Updated README.md with version {version}")

def check_versions_match():
    """Verify all version files are in sync"""
    py_version = get_version_from_pyproject()
    cargo_version = get_version_from_cargo()
    
    if py_version != cargo_version:
        print(f"⚠️  Version mismatch:")
        print(f"   pyproject.toml: {py_version}")
        print(f"   Cargo.toml: {cargo_version}")
        print(f"\n🔧 Syncing Cargo.toml to match pyproject.toml...")
        
        cargo = Path("Cargo.toml")
        with open(cargo, encoding='utf-8') as f:
            cargo_content = f.read()
        
        cargo_content = re.sub(
            r'version\s*=\s*"[^"]+"',
            f'version = "{py_version}"',
            cargo_content,
            count=1  # Only replace first occurrence (the main package)
        )
        
        with open(cargo, 'w', encoding='utf-8') as f:
            f.write(cargo_content)
        
        print(f"✅ Updated Cargo.toml to {py_version}")
    else:
        print(f"✅ Version check passed: {py_version}")
    
    return py_version

if __name__ == "__main__":
    try:
        version = check_versions_match()
        update_readme(version)
        print(f"\n✅ All versions synced to {version}")
        sys.exit(0)
    except Exception as e:
        print(f"❌ Error: {e}")
        sys.exit(1)
