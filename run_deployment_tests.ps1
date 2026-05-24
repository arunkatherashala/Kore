#!/usr/bin/env pwsh
# KORE v1.2.3 - Deployment Test Suite

$passed = @()
$failed = @()

Write-Host "KORE v1.2.3 - Deployment Test Suite"
Write-Host "====================================`n"

# Test 1: Cargo.toml version
Write-Host "Test 1: Rust version in Cargo.toml... " -NoNewline
try {
    if ((Get-Content Cargo.toml | Select-String 'version = "1.2.3"')) {
        Write-Host "PASS" -ForegroundColor Green
        $passed += "Cargo.toml version"
    } else {
        throw "Version mismatch"
    }
} catch {
    Write-Host "FAIL" -ForegroundColor Red
    $failed += "Cargo.toml version"
}

# Test 2: pyproject.toml version
Write-Host "Test 2: Python version in pyproject.toml... " -NoNewline
try {
    if ((Get-Content pyproject.toml | Select-String 'version = "1.2.3"')) {
        Write-Host "PASS" -ForegroundColor Green
        $passed += "pyproject.toml version"
    } else {
        throw "Version mismatch"
    }
} catch {
    Write-Host "FAIL" -ForegroundColor Red
    $failed += "pyproject.toml version"
}

# Test 3: package.json version
Write-Host "Test 3: JavaScript version in package.json... " -NoNewline
try {
    if ((Get-Content package.json | Select-String '"version": "1.2.3"')) {
        Write-Host "PASS" -ForegroundColor Green
        $passed += "package.json version"
    } else {
        throw "Version mismatch"
    }
} catch {
    Write-Host "FAIL" -ForegroundColor Red
    $failed += "package.json version"
}

# Test 4: Rust lib.rs exists
Write-Host "Test 4: Rust source file src/lib.rs... " -NoNewline
try {
    if (Test-Path "src/lib.rs") {
        Write-Host "PASS" -ForegroundColor Green
        $passed += "Rust source"
    } else {
        throw "File not found"
    }
} catch {
    Write-Host "FAIL" -ForegroundColor Red
    $failed += "Rust source"
}

# Test 5: Python package
Write-Host "Test 5: Python package kore_fileformat/__init__.py... " -NoNewline
try {
    if (Test-Path "kore_fileformat/__init__.py") {
        Write-Host "PASS" -ForegroundColor Green
        $passed += "Python package"
    } else {
        throw "File not found"
    }
} catch {
    Write-Host "FAIL" -ForegroundColor Red
    $failed += "Python package"
}

# Test 6: Java Spark connector
Write-Host "Test 6: Spark connector projects/spark-connector/pom.xml... " -NoNewline
try {
    if (Test-Path "projects/spark-connector/pom.xml") {
        Write-Host "PASS" -ForegroundColor Green
        $passed += "Spark connector"
    } else {
        throw "File not found"
    }
} catch {
    Write-Host "FAIL" -ForegroundColor Red
    $failed += "Spark connector"
}

# Test 7: Hadoop connector
Write-Host "Test 7: Hadoop connector projects/hadoop-connector/pom.xml... " -NoNewline
try {
    if (Test-Path "projects/hadoop-connector/pom.xml") {
        Write-Host "PASS" -ForegroundColor Green
        $passed += "Hadoop connector"
    } else {
        throw "File not found"
    }
} catch {
    Write-Host "FAIL" -ForegroundColor Red
    $failed += "Hadoop connector"
}

# Test 8: Git tag
Write-Host "Test 8: Git tag v1.2.3 exists... " -NoNewline
try {
    $tag = (git tag -l "v1.2.3" 2>$null)
    if ($tag -eq "v1.2.3") {
        Write-Host "PASS" -ForegroundColor Green
        $passed += "Git tag"
    } else {
        throw "Tag not found"
    }
} catch {
    Write-Host "FAIL" -ForegroundColor Red
    $failed += "Git tag"
}

# Test 9: README exists
Write-Host "Test 9: README.md exists... " -NoNewline
try {
    if (Test-Path "README.md") {
        Write-Host "PASS" -ForegroundColor Green
        $passed += "README"
    } else {
        throw "File not found"
    }
} catch {
    Write-Host "FAIL" -ForegroundColor Red
    $failed += "README"
}

# Test 10: License file
Write-Host "Test 10: KUOPL-LICENSE file... " -NoNewline
try {
    if (Test-Path "KUOPL-LICENSE") {
        Write-Host "PASS" -ForegroundColor Green
        $passed += "License file"
    } else {
        throw "File not found"
    }
} catch {
    Write-Host "FAIL" -ForegroundColor Red
    $failed += "License file"
}

# Summary
Write-Host "`n====================================`n"
Write-Host "RESULTS:"
Write-Host "Passed: $($passed.Count)/10"
Write-Host "Failed: $($failed.Count)/10"

if ($failed.Count -gt 0) {
    Write-Host "`nFailed tests:" -ForegroundColor Red
    $failed | ForEach-Object { Write-Host "  - $_" }
}

Write-Host "`n====================================`n"

if ($failed.Count -eq 0) {
    Write-Host "SUCCESS: All tests passed!"  -ForegroundColor Green
    Write-Host "v1.2.3 is ready for deployment`n" -ForegroundColor Green
    exit 0
} else {
    Write-Host "FAILURE: Some tests failed" -ForegroundColor Red
    Write-Host "Review failures and fix before deploying`n" -ForegroundColor Red
    exit 1
}
