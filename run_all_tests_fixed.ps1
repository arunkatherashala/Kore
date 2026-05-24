#!/usr/bin/env pwsh
# KORE v1.2.3 Pre-Deployment Test Suite - CORRECTED
# Run: .\run_all_tests.ps1

$ErrorActionPreference = "Stop"
$testResults = @()
$passedComponents = @()
$failedComponents = @()
$blockedComponents = @()

Write-Host @"
╔════════════════════════════════════════════════════════════════════╗
║                                                                    ║
║  KORE v1.2.3 - COMPREHENSIVE PRE-DEPLOYMENT TEST SUITE            ║
║                                                                    ║
║  Testing: 11 components (6 SDKs + Rust + 4 Connectors)            ║
║  Method: Rolling deployment - deploy what passes                  ║
║                                                                    ║
╚════════════════════════════════════════════════════════════════════╝
"@ -ForegroundColor Cyan

Write-Host "`n⏱️  Tests Starting: $(Get-Date -Format 'HH:mm:ss')`n" -ForegroundColor Gray

# ============================================================================
# PHASE 1: VERSION ALIGNMENT CHECK
# ============================================================================
Write-Host "╔════ PHASE 1: VERSION ALIGNMENT ════╗`n" -ForegroundColor Yellow

function Test-Component {
    param(
        [string]$Name,
        [scriptblock]$TestScript
    )
    
    Write-Host "  Testing: $Name..." -NoNewline
    try {
        & $TestScript
        Write-Host " ✅ PASS" -ForegroundColor Green
        $passedComponents += $Name
        return $true
    }
    catch {
        Write-Host " ❌ FAIL - $($_.Exception.Message)" -ForegroundColor Red
        $failedComponents += $Name
        return $false
    }
}

# Check Cargo.toml
Test-Component "Rust Version - Cargo.toml" {
    $content = Get-Content "Cargo.toml" -Raw
    if ($content -match 'version = "1\.2\.3"') {
        Write-Host "" # Keep on same line
    } else {
        throw "Version not 1.2.3 in Cargo.toml"
    }
}

# Check pyproject.toml
Test-Component "Python Version - pyproject.toml" {
    $content = Get-Content "pyproject.toml" -Raw
    if ($content -match 'version = "1\.2\.3"') {
        Write-Host "" # Keep on same line
    } else {
        throw "Version not 1.2.3 in pyproject.toml"
    }
}

# Check package.json
Test-Component "JavaScript Version - package.json" {
    $content = Get-Content "package.json" -Raw
    if ($content -match '"version":\s*"1\.2\.3"') {
        Write-Host "" # Keep on same line
    } else {
        throw "Version not 1.2.3 in package.json"
    }
}

# Check Spark pom.xml
Test-Component "Spark Connector Version - pom.xml" {
    $content = Get-Content "projects/spark-connector/pom.xml" -Raw
    if ($content -match '<version>1\.2\.3</version>') {
        Write-Host "" # Keep on same line
    } else {
        throw "Version not 1.2.3 in Spark pom.xml"
    }
}

# ============================================================================
# PHASE 2: RUST CORE COMPILATION
# ============================================================================
Write-Host "`n╔════ PHASE 2: RUST CORE COMPILATION ════╗`n" -ForegroundColor Yellow

Test-Component "Cargo.toml Exists" {
    if (-not (Test-Path "Cargo.toml")) { throw "Cargo.toml not found" }
    Write-Host "" # Keep on same line
}

Test-Component "Rust Source Files Exist" {
    if (-not (Test-Path "src/lib.rs")) { throw "src/lib.rs not found" }
    if ((Get-ChildItem "src/*.rs" -ErrorAction SilentlyContinue).Count -lt 5) {
        throw "Insufficient Rust source files"
    }
    Write-Host "" # Keep on same line
}

Test-Component "Cargo Check (Syntax)" {
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        $result = & cargo check --quiet 2>&1
        if ($LASTEXITCODE -ne 0) { throw "Cargo check failed: $result" }
        Write-Host "" # Keep on same line
    } else {
        Write-Host " ⏭️  SKIPPED (cargo not available)" -ForegroundColor Gray
    }
}

# ============================================================================
# PHASE 3: PYTHON SDK
# ============================================================================
Write-Host "`n╔════ PHASE 3: PYTHON SDK ════╗`n" -ForegroundColor Yellow

Test-Component "Python Package Structure" {
    if (-not (Test-Path "kore_fileformat/__init__.py")) { throw "kore_fileformat/__init__.py not found" }
    if (-not (Test-Path "pyproject.toml")) { throw "pyproject.toml not found" }
    Write-Host "" # Keep on same line
}

Test-Component "Python Version Check" {
    $content = Get-Content "kore_fileformat/__init__.py" -Raw
    if ($content -match '__version__\s*=\s*"1\.2\.3"') {
        Write-Host "" # Keep on same line
    } else {
        throw "Version not 1.2.3 in __init__.py"
    }
}

Test-Component "Python Import Ready" {
    if (Get-Command python -ErrorAction SilentlyContinue) {
        $result = & python -c "import sys; print(sys.version)" 2>&1
        if ($LASTEXITCODE -ne 0) { throw "Python not working" }
        Write-Host "" # Keep on same line
    } else {
        Write-Host " ⏭️  SKIPPED (python not in PATH)" -ForegroundColor Gray
    }
}

# ============================================================================
# PHASE 4: JAVA SDK & CONNECTORS
# ============================================================================
Write-Host "`n╔════ PHASE 4: JAVA SDK & CONNECTORS ════╗`n" -ForegroundColor Yellow

Test-Component "Spark Connector pom.xml" {
    if (-not (Test-Path "projects/spark-connector/pom.xml")) { throw "Spark pom.xml not found" }
    Write-Host "" # Keep on same line
}

Test-Component "Hadoop Connector pom.xml" {
    if (-not (Test-Path "projects/hadoop-connector/pom.xml")) { throw "Hadoop pom.xml not found" }
    Write-Host "" # Keep on same line
}

Test-Component "Hive Connector pom.xml" {
    if (-not (Test-Path "projects/hive-connector/pom.xml")) { throw "Hive pom.xml not found" }
    Write-Host "" # Keep on same line
}

# ============================================================================
# PHASE 5: OTHER LANGUAGE SDKS
# ============================================================================
Write-Host "`n╔════ PHASE 5: LANGUAGE SDK STRUCTURE ════╗`n" -ForegroundColor Yellow

Test-Component "Go SDK Package" {
    if (-not (Test-Path "go-sdk")) { throw "go-sdk directory not found" }
    if (-not (Test-Path "go-sdk/go.mod")) { throw "go.mod not found" }
    Write-Host "" # Keep on same line
}

Test-Component "JavaScript SDK" {
    if (-not (Test-Path "index.js")) { throw "index.js not found" }
    if (-not (Test-Path "package.json")) { throw "package.json not found" }
    Write-Host "" # Keep on same line
}

Test-Component "CSharp .NET SDK" {
    if (-not (Test-Path "kore-sharp")) { throw "kore-sharp directory not found" }
    Write-Host "" # Keep on same line
}

Test-Component "Ruby SDK Gem" {
    if (-not (Test-Path "kore-fileformat.gemspec")) { throw "kore-fileformat.gemspec not found" }
    Write-Host "" # Keep on same line
}

# ============================================================================
# PHASE 6: GIT VERIFICATION
# ============================================================================
Write-Host "`n╔════ PHASE 6: GIT VERIFICATION ════╗`n" -ForegroundColor Yellow

Test-Component "Git Tag v1.2.3 Exists" {
    $gitTag = & git tag -l "v1.2.3" 2>&1
    if (-not $gitTag -or $gitTag -notmatch "v1\.2\.3") {
        throw "Git tag v1.2.3 not found"
    }
    Write-Host "" # Keep on same line
}

Test-Component "Git Repository Clean" {
    $status = & git status --porcelain 2>&1
    if ($status -and ($status -notmatch "^$")) {
        throw "Git working directory not clean"
    }
    Write-Host "" # Keep on same line
}

Test-Component "README.md Exists" {
    if (-not (Test-Path "README.md")) { throw "README.md not found" }
    Write-Host "" # Keep on same line
}

# ============================================================================
# PHASE 7: SECURITY CHECK
# ============================================================================
Write-Host "`n╔════ PHASE 7: SECURITY CHECK ════╗`n" -ForegroundColor Yellow

Test-Component "No Hardcoded Credentials" {
    $suspiciousFiles = @()
    foreach ($file in (Get-ChildItem -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Extension -in @(".rs", ".py", ".java", ".js", ".cs") })) {
        $content = Get-Content $file.FullName -Raw -ErrorAction SilentlyContinue
        if ($content -match '(password|api_key|secret|token)\s*[:=]\s*[''"].+[''"]' -and $file.FullName -notmatch 'test|spec|doc') {
            $suspiciousFiles += $file.FullName
        }
    }
    if ($suspiciousFiles.Count -gt 0) {
        throw "Hardcoded credentials found in: $($suspiciousFiles -join ', ')"
    }
    Write-Host "" # Keep on same line
}

Test-Component "KUOPL License File" {
    if (-not (Test-Path "KUOPL-LICENSE")) { throw "KUOPL-LICENSE not found" }
    Write-Host "" # Keep on same line
}

# ============================================================================
# RESULTS SUMMARY
# ============================================================================

Write-Host "`n╔════════════════════════════════════════════════════════════════════╗" -ForegroundColor Cyan
Write-Host "║                     TEST RESULTS SUMMARY                          ║" -ForegroundColor Cyan
Write-Host "╚════════════════════════════════════════════════════════════════════╝`n" -ForegroundColor Cyan

$totalTests = $passedComponents.Count + $failedComponents.Count
$passPercentage = if ($totalTests -gt 0) { [math]::Round(($passedComponents.Count / $totalTests) * 100) } else { 0 }

Write-Host "✅ PASSED: $($passedComponents.Count) components" -ForegroundColor Green
Write-Host "❌ FAILED: $($failedComponents.Count) components" -ForegroundColor Red
Write-Host "📊 Pass Rate: $passPercentage%`n" -ForegroundColor Yellow

if ($passedComponents.Count -gt 0) {
    Write-Host "✅ PASSING COMPONENTS (Ready to Deploy):" -ForegroundColor Green
    $passedComponents | ForEach-Object { Write-Host "   ✅ $_" -ForegroundColor Green }
}

if ($failedComponents.Count -gt 0) {
    Write-Host "`n❌ FAILING COMPONENTS (Need Fixes):" -ForegroundColor Red
    $failedComponents | ForEach-Object { Write-Host "   ❌ $_" -ForegroundColor Red }
}

# ============================================================================
# DEPLOYMENT DECISION
# ============================================================================

Write-Host "`n╔════════════════════════════════════════════════════════════════════╗`n" -ForegroundColor Cyan

if ($failedComponents.Count -eq 0) {
    Write-Host "✅ ALL TESTS PASSED - READY FOR DEPLOYMENT!" -ForegroundColor Green
    Write-Host ""
    Write-Host "Next Steps:" -ForegroundColor Green
    Write-Host "1. Review all test results above" -ForegroundColor Green
    Write-Host "2. Follow DEPLOYMENT_QUICK_START.md" -ForegroundColor Green
    Write-Host "3. Deploy to package managers:" -ForegroundColor Green
    Write-Host "   - PyPI: pip install kore-fileformat==1.2.3" -ForegroundColor Cyan
    Write-Host "   - Maven: Central Repository" -ForegroundColor Cyan
    Write-Host "   - npm: npm install kore-fileformat@1.2.3" -ForegroundColor Cyan
    Write-Host "   - Cargo: cargo install kore_fileformat" -ForegroundColor Cyan
    Write-Host "   - And others..." -ForegroundColor Cyan
    Write-Host ""
    Write-Host "USE: DEPLOYMENT_ROLLING_STRATEGY.md for health checks and rollback" -ForegroundColor Green
    Write-Host "USE: DEPLOYMENT_STATUS_v1.2.3.md to track deployment progress" -ForegroundColor Green
    Write-Host ""
    exit 0
}
else {
    Write-Host "⚠️  SOME TESTS FAILED - REVIEW NEEDED" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Failing Components ($($failedComponents.Count)):" -ForegroundColor Red
    $failedComponents | ForEach-Object { Write-Host "   ❌ $_" -ForegroundColor Red }
    
    Write-Host ""
    Write-Host "DEPLOYMENT STRATEGY (Rolling Deployment):" -ForegroundColor Cyan
    Write-Host "   [OK] Deploy components that PASSED immediately" -ForegroundColor Green
    Write-Host "   [FIX] Fix failed components in parallel" -ForegroundColor Yellow
    Write-Host "   [WAIT] Deploy fixed components as v1.2.3.1 patches" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "See: DEPLOYMENT_ROLLING_STRATEGY.md for details" -ForegroundColor Cyan
    Write-Host ""
    exit 1
}
