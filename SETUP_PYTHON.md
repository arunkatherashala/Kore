# Python Setup & Integration Guide for KORE v1.3.3

**Last Updated:** June 3, 2026  
**Status:** Production Ready  
**Version:** v1.0

---

## 📋 Table of Contents

1. [Prerequisites](#prerequisites)
2. [Installation](#installation)
3. [Verification](#verification)
4. [KORE Integration](#kore-integration)
5. [Common Tasks](#common-tasks)
6. [Troubleshooting](#troubleshooting)

---

## Prerequisites

| Requirement | Minimum | Recommended | Notes |
|-------------|---------|-------------|-------|
| Python Version | 3.8 | 3.11+ | KORE supports Python 3.8 - 3.12 |
| OS Support | Windows 10+ | Windows 10, 11 | Also supports Linux/macOS |
| RAM | 4 GB | 8 GB | For local testing & development |
| Disk Space | 500 MB | 2 GB | For Python + dependencies |

---

## Installation

### Step 1: Download Python

**Official Website:** https://www.python.org/downloads/

**For Windows:**
```powershell
# Option A: Download installer from official website
# https://www.python.org/downloads/windows/
# Choose "Windows installer (64-bit)" for modern systems

# Option B: Using Windows Package Manager (if installed)
winget install Python.Python.3.11
```

### Step 2: Install Python

**Windows Installation:**

1. Run the downloaded installer
2. **IMPORTANT:** Check "Add Python to PATH" ✅
3. Choose "Install Now" (recommended) or customize
4. Wait for installation to complete

**Verify Installation:**
```powershell
python --version
pip --version
```

**Expected Output:**
```
Python 3.11.x (or your installed version)
pip 23.x.x from C:\Users\YourName\AppData\Local\Programs\Python\Python311\lib\site-packages\pip
```

---

## Verification

### Quick Check
```powershell
# Test 1: Python executable
python -c "print('Python is installed!')"

# Test 2: PIP package manager
pip list

# Test 3: Python version details
python -c "import sys; print(sys.version)"
```

### Complete Environment Setup
```powershell
# Create test virtual environment
python -m venv test_env

# Activate environment (Windows)
.\test_env\Scripts\Activate.ps1

# Verify activation (you should see (test_env) in terminal)
python --version

# Deactivate when done
deactivate
```

---

## KORE Integration

### Setup KORE Python Environment

**Step 1: Create Project Virtual Environment**
```powershell
cd "c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore"

# Create virtual environment
python -m venv .venv

# Activate it (Windows PowerShell)
.\.venv\Scripts\Activate.ps1
```

**Step 2: Install Dependencies**

KORE uses Python for:
- Data processing scripts
- Benchmark utilities
- Test data generation
- Analysis tools

```powershell
# Install core data science stack
pip install --upgrade pip
pip install numpy pandas matplotlib
pip install pytest pytest-cov
pip install pyspark  # Optional: for distributed testing
```

**Step 3: Verify KORE Python Integration**

```powershell
# Check installed packages
pip list

# Run Python in KORE context
python -c "import numpy; import pandas; print('KORE Python environment ready!')"
```

---

## Common Tasks

### Running Python Scripts in KORE Context

```powershell
# Activate KORE environment
cd "c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore"
.\.venv\Scripts\Activate.ps1

# Run a Python script
python your_script.py

# Run with arguments
python your_script.py --arg1 value1 --arg2 value2

# Exit environment
deactivate
```

### Creating Python Test Scripts for KORE

**Example: test_kore_data.py**
```python
#!/usr/bin/env python3
"""
KORE v1.3.3 Data Validation Script
Tests KORE database output for correctness
"""

import numpy as np
import pandas as pd

def test_kore_output():
    """Validate KORE generated data"""
    print("Testing KORE output...")
    # Add your test logic here
    print("✅ All tests passed!")

if __name__ == "__main__":
    test_kore_output()
```

**Run it:**
```powershell
python test_kore_data.py
```

### Using Python with KORE Benchmarks

```powershell
# Example: Generate benchmark data
python -c "
import numpy as np
data = np.random.randn(1000000)
print(f'Generated {len(data)} benchmark points')
"
```

---

## Troubleshooting

### Issue 1: "python is not recognized"

**Solution:**
```powershell
# Verify Python installation path
Get-Command python

# If not found, add to PATH manually:
# 1. Open Environment Variables (System Properties)
# 2. Add Python installation path (usually C:\Users\YourName\AppData\Local\Programs\Python\Python311)
# 3. Restart PowerShell
```

### Issue 2: "Permission Denied" on Activation Script

**Solution:**
```powershell
# Fix PowerShell execution policy
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser

# Then try activation again
.\.venv\Scripts\Activate.ps1
```

### Issue 3: "ModuleNotFoundError" after installing packages

**Solution:**
```powershell
# Make sure virtual environment is activated
.\.venv\Scripts\Activate.ps1

# Reinstall the package
pip install --force-reinstall package_name
```

### Issue 4: Slow pip install

**Solution:**
```powershell
# Use fast mirror for pip
pip install -i https://mirrors.aliyun.com/pypi/simple/ package_name

# Or upgrade pip first
python -m pip install --upgrade pip
```

---

## Best Practices

✅ **DO:**
- Always use virtual environments per project
- Pin dependency versions in requirements.txt
- Test scripts before running in production
- Keep Python updated to latest stable version
- Document all custom scripts with docstrings

❌ **DON'T:**
- Install packages globally without reason
- Mix different Python versions in same project
- Run pip as administrator
- Ignore dependency conflicts
- Use outdated Python versions

---

## Environment Checklist

Before working with KORE Python scripts:

- [ ] Python 3.8+ installed
- [ ] `python --version` works
- [ ] `pip --version` works
- [ ] Virtual environment created (`.venv`)
- [ ] Virtual environment activated
- [ ] Core packages installed (numpy, pandas)
- [ ] Pytest installed for testing
- [ ] All scripts have been tested

---

## Advanced Configuration

### Configure Python for KORE Performance

**Create `.venv\Lib\site-packages\sitecustomize.py`:**
```python
# Optimize Python for KORE
import sys
import os

# Disable bytecode generation in production
if os.getenv('KORE_PRODUCTION'):
    sys.dont_write_bytecode = True

# Set thread limit for better performance
import threading
threading.stack_size(32768)
```

### Integration with Cargo

Some KORE workflows may call Python from Rust:

```rust
// In src/main.rs or build.rs
use std::process::Command;

fn run_python_analysis() -> Result<()> {
    let output = Command::new("python")
        .arg("scripts/analyze.py")
        .output()?;
    
    println!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}
```

---

## Additional Resources

| Resource | Link | Purpose |
|----------|------|---------|
| Python Official Docs | https://docs.python.org/ | Language reference |
| NumPy Docs | https://numpy.org/ | Numerical computing |
| Pandas Docs | https://pandas.pydata.org/ | Data analysis |
| PyTest Docs | https://docs.pytest.org/ | Testing framework |
| Virtual Environments | https://docs.python.org/3/tutorial/venv.html | Environment management |

---

## Quick Reference Commands

```powershell
# Activate environment
.\.venv\Scripts\Activate.ps1

# Deactivate environment
deactivate

# Install package
pip install package_name

# Install from requirements
pip install -r requirements.txt

# List installed packages
pip list

# Freeze current environment
pip freeze > requirements.txt

# Run Python script
python script.py

# Run Python REPL
python

# Run tests
pytest tests/

# Run tests with coverage
pytest --cov tests/
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial setup guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Next:** Maven Setup & Integration Guide (coming next)
