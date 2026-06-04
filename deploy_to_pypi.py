#!/usr/bin/env python3
"""
KORE v1.3.3 - Python PyPI Deployment Script
Automates building, testing, and publishing to PyPI
"""

import os
import sys
import subprocess
import json
from pathlib import Path

class PyPIDeployer:
    def __init__(self):
        self.project_root = Path.cwd()
        self.version = "1.3.3"
        self.package_name = "kore-fileformat"
        self.success_count = 0
        self.total_steps = 0
        
    def log(self, step, message):
        """Log step completion"""
        self.total_steps += 1
        print(f"\n{'='*70}")
        print(f"✅ STEP {step}: {message}")
        print(f"{'='*70}")
        
    def run_command(self, cmd, description):
        """Run shell command safely"""
        print(f"\n  📌 {description}...")
        try:
            result = subprocess.run(
                cmd,
                shell=True,
                capture_output=True,
                text=True,
                cwd=self.project_root
            )
            
            if result.returncode != 0:
                print(f"  ❌ Failed: {result.stderr[:200]}")
                return False
            
            # Show last line of output
            if result.stdout:
                last_line = result.stdout.strip().split('\n')[-1]
                print(f"  ✓ {last_line[:100]}")
            
            self.success_count += 1
            return True
        except Exception as e:
            print(f"  ❌ Error: {e}")
            return False
    
    def step_1_verify_environment(self):
        """Step 1: Verify environment setup"""
        self.log(1, "Verify Environment Setup")
        
        checks = [
            ("python --version", "Python installation"),
            ("pip --version", "pip installation"),
            ("rustc --version", "Rust installation"),
            ("cargo --version", "Cargo installation"),
        ]
        
        for cmd, desc in checks:
            self.run_command(cmd, desc)
    
    def step_2_verify_versions(self):
        """Step 2: Verify version numbers"""
        self.log(2, "Verify Version Numbers")
        
        # Check pyproject.toml
        with open("pyproject.toml") as f:
            content = f.read()
            if f'version = "{self.version}"' in content:
                print(f"  ✓ pyproject.toml: version = {self.version}")
                self.success_count += 1
            else:
                print(f"  ✗ pyproject.toml: version mismatch")
        
        # Check Cargo.toml
        with open("Cargo.toml") as f:
            content = f.read()
            if f'version = "{self.version}"' in content:
                print(f"  ✓ Cargo.toml: version = {self.version}")
                self.success_count += 1
            else:
                print(f"  ✗ Cargo.toml: version mismatch")
    
    def step_3_build_wheels(self):
        """Step 3: Build Python wheels"""
        self.log(3, "Build Python Wheels")
        
        # Clean old builds
        self.run_command("rmdir /s /q target\\wheels 2>nul || echo 'No old wheels'", 
                        "Clean old wheels")
        self.run_command("rmdir /s /q dist 2>nul || echo 'No old dist'", 
                        "Clean old dist")
        self.run_command("rmdir /s /q build 2>nul || echo 'No old build'", 
                        "Clean old build")
        
        # Build with maturin
        self.run_command("maturin build --release", "Build Python wheel with maturin")
        
        # List wheels
        wheels = list(Path("target/wheels").glob("*.whl"))
        print(f"\n  📦 Built {len(wheels)} wheel(s):")
        for wheel in wheels:
            size_mb = wheel.stat().st_size / (1024*1024)
            print(f"     • {wheel.name} ({size_mb:.1f} MB)")
            self.success_count += 1
    
    def step_4_build_source_dist(self):
        """Step 4: Build source distribution"""
        self.log(4, "Build Source Distribution")
        
        self.run_command("python -m build --sdist", "Build source distribution")
        
        # List sdist
        sdist = list(Path("dist").glob("*.tar.gz"))
        if sdist:
            for s in sdist:
                size_mb = s.stat().st_size / (1024*1024)
                print(f"  📦 {s.name} ({size_mb:.1f} MB)")
                self.success_count += 1
    
    def step_5_verify_package(self):
        """Step 5: Verify package with twine"""
        self.log(5, "Verify Package with Twine")
        
        self.run_command("twine check dist\\*", "Validate package with twine")
    
    def step_6_test_import(self):
        """Step 6: Test package import"""
        self.log(6, "Test Package Import")
        
        test_code = '''
import sys
try:
    from kore_fileformat import *
    print(f"✓ Import successful")
    sys.exit(0)
except ImportError as e:
    print(f"✗ Import failed: {e}")
    sys.exit(1)
'''
        
        with open("test_import.py", "w") as f:
            f.write(test_code)
        
        self.run_command("python test_import.py", "Test import of kore_fileformat")
    
    def step_7_test_functionality(self):
        """Step 7: Test basic functionality"""
        self.log(7, "Test Functionality")
        
        test_code = '''
from kore_fileformat import compress, decompress
import sys

try:
    # Test compression
    data = b"Hello World" * 1000
    compressed = compress(data, compression_level=9)
    
    # Test decompression
    decompressed = decompress(compressed)
    
    assert decompressed == data, "Decompression mismatch!"
    
    ratio = len(data) / len(compressed)
    print(f"✓ Compression works! Ratio: {ratio:.2f}:1")
    sys.exit(0)
except Exception as e:
    print(f"✗ Test failed: {e}")
    sys.exit(1)
'''
        
        with open("test_functionality.py", "w") as f:
            f.write(test_code)
        
        self.run_command("python test_functionality.py", "Test compression/decompression")
    
    def step_8_deployment_summary(self):
        """Step 8: Show deployment summary"""
        self.log(8, "Deployment Summary")
        
        wheels = list(Path("target/wheels").glob("*.whl"))
        sdist = list(Path("dist").glob("*.tar.gz"))
        
        print(f"\n  📊 DEPLOYMENT READY:")
        print(f"     Package: {self.package_name}")
        print(f"     Version: {self.version}")
        print(f"     Wheels built: {len(wheels)}")
        print(f"     Source dist: {len(sdist)}")
        print(f"\n  🚀 To publish to PyPI, run:")
        print(f"     twine upload dist/*")
        print(f"\n  📝 To publish to TestPyPI first (recommended):")
        print(f"     twine upload --repository testpypi dist/*")
    
    def run_all(self):
        """Run all deployment steps"""
        print(f"\n{'='*70}")
        print(f"🐍 KORE v{self.version} - Python PyPI Deployment")
        print(f"{'='*70}")
        
        self.step_1_verify_environment()
        self.step_2_verify_versions()
        self.step_3_build_wheels()
        self.step_4_build_source_dist()
        self.step_5_verify_package()
        self.step_6_test_import()
        self.step_7_test_functionality()
        self.step_8_deployment_summary()
        
        # Final summary
        print(f"\n{'='*70}")
        print(f"✅ DEPLOYMENT COMPLETE")
        print(f"{'='*70}")
        print(f"\n  Successful steps: {self.success_count}/{self.total_steps}")
        
        if self.success_count == self.total_steps:
            print(f"\n  🎉 All steps completed successfully!")
            print(f"  📦 Ready for PyPI deployment!")
            return 0
        else:
            print(f"\n  ⚠️ Some steps failed. Check logs above.")
            return 1

if __name__ == "__main__":
    deployer = PyPIDeployer()
    sys.exit(deployer.run_all())
