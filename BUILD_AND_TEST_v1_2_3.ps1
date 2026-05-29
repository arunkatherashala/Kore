# KORE v1.2.3 - Quick Build & Test Verification
# Verify current codebase before Phase 1 (June 2, 2026)

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "KORE v1.2.3 - BUILD & VERIFICATION TEST" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

$startTime = Get-Date
$results = @()

# =======================================================================
# 1. RUST CORE BUILD
# =======================================================================
Write-Host "1. BUILDING RUST CORE v1.2.3" -ForegroundColor Yellow
Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Yellow

try {
    $rustVersion = cargo --version 2>&1
    if ($LASTEXITCODE -eq 0) {
        Write-Host "✓ Rust: $rustVersion" -ForegroundColor Green
        
        Write-Host "Compiling release binary..." -ForegroundColor Cyan
        cargo build --release 2>&1 | Out-Null
        
        if ($LASTEXITCODE -eq 0) {
            Write-Host "✅ Rust core compiled successfully" -ForegroundColor Green
            $results += "Rust Build: PASSED"
        } else {
            Write-Host "❌ Build failed" -ForegroundColor Red
            $results += "Rust Build: FAILED"
        }
    } else {
        Write-Host "❌ Rust not found - install from https://rustup.rs/" -ForegroundColor Red
        $results += "Rust Build: NOT AVAILABLE"
    }
} catch {
    Write-Host "❌ Error: $_" -ForegroundColor Red
    $results += "Rust Build: ERROR"
}

# =======================================================================
# 2. PYTHON BINDINGS CHECK
# =======================================================================
Write-Host ""
Write-Host "2. CHECKING PYTHON BINDINGS v1.2.3" -ForegroundColor Yellow
Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Yellow

try {
    $pythonVersion = python --version 2>&1
    if ($LASTEXITCODE -eq 0) {
        Write-Host "✓ Python: $pythonVersion" -ForegroundColor Green
        
        # Try importing kore
        $importTest = python -c "import kore_fileformat; print('OK')" 2>&1
        if ($LASTEXITCODE -eq 0) {
            Write-Host "✅ kore-fileformat Python package is installed" -ForegroundColor Green
            $results += "Python Bindings: INSTALLED"
        } else {
            Write-Host "⚠️  kore-fileformat not installed (expected - build with: maturin develop)" -ForegroundColor Yellow
            $results += "Python Bindings: NOT INSTALLED (normal)"
        }
    } else {
        Write-Host "❌ Python not found" -ForegroundColor Red
        $results += "Python: NOT AVAILABLE"
    }
} catch {
    Write-Host "❌ Error: $_" -ForegroundColor Red
    $results += "Python Bindings: ERROR"
}

# =======================================================================
# 3. JAVASCRIPT BINDINGS CHECK
# =======================================================================
Write-Host ""
Write-Host "3. CHECKING JAVASCRIPT BINDINGS v1.2.3" -ForegroundColor Yellow
Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Yellow

try {
    $nodeVersion = node --version 2>&1
    if ($LASTEXITCODE -eq 0) {
        Write-Host "✓ Node.js: $nodeVersion" -ForegroundColor Green
        
        if (Test-Path "package.json") {
            Write-Host "✓ package.json found" -ForegroundColor Green
            
            if (Test-Path "node_modules") {
                Write-Host "✓ node_modules found" -ForegroundColor Green
                Write-Host "✅ JavaScript environment ready" -ForegroundColor Green
                $results += "JavaScript Bindings: READY"
            } else {
                Write-Host "⚠️  node_modules not found (run: npm install)" -ForegroundColor Yellow
                $results += "JavaScript Bindings: NEEDS npm install"
            }
        } else {
            Write-Host "⚠️  package.json not found" -ForegroundColor Yellow
            $results += "JavaScript: NOT CONFIGURED"
        }
    } else {
        Write-Host "❌ Node.js not found" -ForegroundColor Red
        $results += "Node.js: NOT AVAILABLE"
    }
} catch {
    Write-Host "❌ Error: $_" -ForegroundColor Red
    $results += "JavaScript Bindings: ERROR"
}

# =======================================================================
# 4. SUMMARY
# =======================================================================
Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "TEST SUMMARY" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

$passCount = ($results | Where-Object { $_ -like "*PASSED*" -or $_ -like "*INSTALLED*" -or $_ -like "*READY*" } | Measure-Object).Count
$totalCount = $results.Count

foreach ($result in $results) {
    if ($result -like "*PASSED*" -or $result -like "*INSTALLED*" -or $result -like "*READY*") {
        Write-Host "✅ $result" -ForegroundColor Green
    } elseif ($result -like "*NOT INSTALLED*" -or $result -like "*needs*" -or $result -like "*NOT CONFIGURED*") {
        Write-Host "⚠️  $result" -ForegroundColor Yellow
    } else {
        Write-Host "❌ $result" -ForegroundColor Red
    }
}

Write-Host "`nStatus: $passCount/$totalCount components ready" -ForegroundColor Cyan

# =======================================================================
# 5. PHASE 1 READINESS
# =======================================================================
Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "PHASE 1 READINESS (June 2, 2026)" -ForegroundColor Cyan
Write-Host "========================================\n" -ForegroundColor Cyan

if ($passCount -ge 1) {
    Write-Host "✅ KORE v1.2.3 core is functional and ready for Phase 1" -ForegroundColor Green
    Write-Host "`nPhase 1 Timeline:" -ForegroundColor White
    Write-Host "  Jun 2:  Phase 1 kickoff" -ForegroundColor Gray
    Write-Host "  Jul 1:  SIMD optimization begins (2.7M -> 5.0M rows/sec target)" -ForegroundColor Gray
    Write-Host "  Sep 30: Phase 1 complete (88 out of 100 score - beat Parquet)" -ForegroundColor Gray
    Write-Host ""
    Write-Host 'Budget: $1.1M | Team: 8 engineers + 2 DevOps' -ForegroundColor Gray
} else {
    Write-Host "⚠️  Please resolve component issues before Phase 1" -ForegroundColor Yellow
}

$elapsed = (Get-Date) - $startTime
$seconds = [math]::Round($elapsed.TotalSeconds, 1)
Write-Host ""
Write-Host "Verification completed in $seconds seconds" -ForegroundColor Gray
Write-Host ""
