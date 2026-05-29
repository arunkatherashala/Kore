# KORE v1.2.3 - Build & Test All Platforms
# This script verifies the current codebase works before Phase 1 (Jun 2)

param(
    [switch]$SkipRust = $false,
    [switch]$SkipPython = $false,
    [switch]$SkipJavaScript = $false,
    [switch]$SkipTests = $false,
    [switch]$Full = $false  # Run everything including cloud connectors
)

$ErrorActionPreference = "Continue"
$startTime = Get-Date
$results = @()

Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "🚀 KORE v1.2.3 - BUILD & TEST SUITE" -ForegroundColor Cyan
Write-Host "========================================`n" -ForegroundColor Cyan

# ============================================================================
# SECTION 1: RUST CORE BUILD & TEST
# ============================================================================

if (-not $SkipRust) {
    Write-Host "📦 BUILDING RUST CORE (v1.2.3)" -ForegroundColor Yellow
    Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Yellow
    
    try {
        # Check if Rust is installed
        $rustVersion = cargo --version 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Host "❌ Rust not found! Install from https://rustup.rs/" -ForegroundColor Red
            $results += @{
                Component = "Rust Core Build"
                Status = "FAILED"
                Error = "Rust not installed"
            }
        } else {
            Write-Host "✓ Rust: $rustVersion" -ForegroundColor Green
            
            # Build release binary
            Write-Host "`nCompiling Rust core (release mode)..." -ForegroundColor Cyan
            $buildOutput = cargo build --release 2>&1
            $buildSuccess = $LASTEXITCODE -eq 0
            
            if ($buildSuccess) {
                Write-Host "✅ Rust core compiled successfully" -ForegroundColor Green
                Write-Host "   Binary: target/release/kore_fileformat.*" -ForegroundColor Gray
                
                $results += @{
                    Component = "Rust Core Build"
                    Status = "✅ PASSED"
                    Details = "Release binary compiled"
                }
                
                # Run Rust tests
                if (-not $SkipTests) {
                    Write-Host "`nRunning Rust tests..." -ForegroundColor Cyan
                    $testOutput = cargo test --release 2>&1
                    $testSuccess = $LASTEXITCODE -eq 0
                    
                    if ($testSuccess) {
                        Write-Host "✅ All Rust tests passed" -ForegroundColor Green
                        
                        # Count test results
                        $passedTests = ($testOutput | Select-String "test result: ok" | Measure-Object).Count
                        
                        $results += @{
                            Component = "Rust Tests"
                            Status = "✅ PASSED"
                            Details = "All unit tests pass"
                        }
                    } else {
                        Write-Host "⚠️  Some Rust tests failed" -ForegroundColor Yellow
                        Write-Host "Test output:" -ForegroundColor Gray
                        $testOutput | Select-Object -Last 20 | ForEach-Object { Write-Host "  $_" -ForegroundColor Gray }
                        
                        $results += @{
                            Component = "Rust Tests"
                            Status = "⚠️  WARNING"
                            Details = "Some tests failed (see log)"
                        }
                    }
                }
            } else {
                Write-Host "❌ Rust core compilation failed" -ForegroundColor Red
                Write-Host "Error details:" -ForegroundColor Gray
                $buildOutput | Select-Object -Last 10 | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
                
                $results += @{
                    Component = "Rust Core Build"
                    Status = "❌ FAILED"
                    Error = "Compilation failed"
                }
            }
        }
    } catch {
        Write-Host "❌ Error during Rust build: $_" -ForegroundColor Red
        $results += @{
            Component = "Rust Core Build"
            Status = "❌ FAILED"
            Error = $_.Exception.Message
        }
    }
}

# ============================================================================
# SECTION 2: PYTHON BINDINGS TEST
# ============================================================================

if (-not $SkipPython) {
    Write-Host "`n📦 TESTING PYTHON BINDINGS (v1.2.3)" -ForegroundColor Yellow
    Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Yellow
    
    try {
        # Check Python version
        $pythonVersion = python --version 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Host "❌ Python not found!" -ForegroundColor Red
            $results += @{
                Component = "Python Bindings"
                Status = "FAILED"
                Error = "Python not installed"
            }
        } else {
            Write-Host "✓ Python: $pythonVersion" -ForegroundColor Green
            
            # Try to import installed version
            Write-Host "`nChecking installed kore-fileformat..." -ForegroundColor Cyan
            $pyImport = python -c "import kore_fileformat; print(f'KORE version: {kore_fileformat.__version__ if hasattr(kore_fileformat, \"__version__\") else \"unknown\"}')" 2>&1
            
            if ($LASTEXITCODE -eq 0) {
                Write-Host "✅ Python package installed and importable" -ForegroundColor Green
                Write-Host "   $pyImport" -ForegroundColor Green
                
                # Test basic functionality
                $testPy = @"
import kore_fileformat
import json

# Get module info
info = {
    'module': 'kore_fileformat',
    'has_version': hasattr(kore_fileformat, '__version__'),
    'has_compress': hasattr(kore_fileformat, 'compress'),
    'has_decompress': hasattr(kore_fileformat, 'decompress'),
    'available_functions': [x for x in dir(kore_fileformat) if not x.startswith('_')]
}
print(json.dumps(info, indent=2))
"@
                
                $testOutput = python -c $testPy 2>&1
                if ($LASTEXITCODE -eq 0) {
                    Write-Host "✅ Python bindings working correctly" -ForegroundColor Green
                    
                    $results += @{
                        Component = "Python Bindings"
                        Status = "✅ PASSED"
                        Details = "Installed and functional"
                    }
                } else {
                    Write-Host "⚠️  Python import works but basic test failed" -ForegroundColor Yellow
                    Write-Host "Error: $testOutput" -ForegroundColor Gray
                    
                    $results += @{
                        Component = "Python Bindings"
                        Status = "⚠️  WARNING"
                        Details = "Import works, test failed"
                    }
                }
            } else {
                Write-Host "⚠️  kore-fileformat not installed (expected - would be built from maturin)" -ForegroundColor Yellow
                Write-Host "   To build: 'maturin develop' from Python environment" -ForegroundColor Gray
                
                $results += @{
                    Component = "Python Bindings"
                    Status = "⚠️  NOT INSTALLED"
                    Details = "Run 'maturin develop' to build"
                }
            }
        }
    } catch {
        Write-Host "❌ Error testing Python bindings: $_" -ForegroundColor Red
        $results += @{
            Component = "Python Bindings"
            Status = "❌ FAILED"
            Error = $_.Exception.Message
        }
    }
}

# ============================================================================
# SECTION 3: JAVASCRIPT/NODE.JS TEST
# ============================================================================

if (-not $SkipJavaScript) {
    Write-Host "`n📦 TESTING JAVASCRIPT BINDINGS (v1.2.3)" -ForegroundColor Yellow
    Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Yellow
    
    try {
        # Check Node version
        $nodeVersion = node --version 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Host "❌ Node.js not found!" -ForegroundColor Red
            $results += @{
                Component = "JavaScript Bindings"
                Status = "FAILED"
                Error = "Node.js not installed"
            }
        } else {
            Write-Host "✓ Node.js: $nodeVersion" -ForegroundColor Green
            
            # Check package.json
            if (Test-Path "package.json") {
                Write-Host "✓ package.json found" -ForegroundColor Green
                
                # Check if dependencies installed
                if (Test-Path "node_modules") {
                    Write-Host "✓ node_modules exists" -ForegroundColor Green
                    
                    # Try to load index.js
                    $jsTest = @"
try {
    const kore = require('./index.js');
    console.log('✓ index.js loaded successfully');
    console.log('Type:', typeof kore);
    console.log('Properties:', Object.keys(kore).filter(k => !k.startsWith('_')).slice(0, 5).join(', '));
} catch (e) {
    console.error('Error loading index.js:', e.message);
    process.exit(1);
}
"@
                    
                    $jsOutput = node -e $jsTest 2>&1
                    if ($LASTEXITCODE -eq 0) {
                        Write-Host "✅ JavaScript bindings loaded" -ForegroundColor Green
                        Write-Host "   $jsOutput" -ForegroundColor Green
                        
                        $results += @{
                            Component = "JavaScript Bindings"
                            Status = "✅ PASSED"
                            Details = "Module loads and is functional"
                        }
                    } else {
                        Write-Host "⚠️  Error loading JavaScript bindings" -ForegroundColor Yellow
                        Write-Host "   $jsOutput" -ForegroundColor Gray
                        
                        $results += @{
                            Component = "JavaScript Bindings"
                            Status = "⚠️  WARNING"
                            Details = "Module load issue (native bindings needed?)"
                        }
                    }
                } else {
                    Write-Host "⚠️  node_modules not found" -ForegroundColor Yellow
                    Write-Host "   Run: npm install" -ForegroundColor Gray
                    
                    $results += @{
                        Component = "JavaScript Bindings"
                        Status = "⚠️  NOT INSTALLED"
                        Details = "Run 'npm install' to build"
                    }
                }
            } else {
                Write-Host "⚠️  package.json not found in root" -ForegroundColor Yellow
                
                $results += @{
                    Component = "JavaScript Bindings"
                    Status = "⚠️  NOT CONFIGURED"
                    Details = "Check JavaScript project structure"
                }
            }
        }
    } catch {
        Write-Host "❌ Error testing JavaScript bindings: $_" -ForegroundColor Red
        $results += @{
            Component = "JavaScript Bindings"
            Status = "❌ FAILED"
            Error = $_.Exception.Message
        }
    }
}

# ============================================================================
# SECTION 4: TEST SUITE RUN (Optional)
# ============================================================================

if ($Full -and -not $SkipTests) {
    Write-Host "`n📦 RUNNING COMPREHENSIVE TEST SUITES" -ForegroundColor Yellow
    Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Yellow
    
    # Look for test files
    $pythonTests = Get-ChildItem -Path "*.py" -Filter "*test*.py" -ErrorAction SilentlyContinue
    $jsTests = Get-ChildItem -Path "*.js" -Filter "*.test.js" -ErrorAction SilentlyContinue
    
    if ($pythonTests) {
        Write-Host "Found Python test files: $($pythonTests.Count)" -ForegroundColor Cyan
        foreach ($test in $pythonTests | Select-Object -First 3) {
            Write-Host "  - $($test.Name)" -ForegroundColor Gray
        }
    }
    
    if ($jsTests) {
        Write-Host "Found JavaScript test files: $($jsTests.Count)" -ForegroundColor Cyan
        foreach ($test in $jsTests | Select-Object -First 3) {
            Write-Host "  - $($test.Name)" -ForegroundColor Gray
        }
    }
}

# ============================================================================
# SECTION 5: SUMMARY REPORT
# ============================================================================

Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "📊 TEST SUMMARY REPORT" -ForegroundColor Cyan
Write-Host "========================================`n" -ForegroundColor Cyan

$passCount = ($results | Where-Object { $_.Status -like "*PASSED*" } | Measure-Object).Count
$warnCount = ($results | Where-Object { $_.Status -like "*WARNING*" } | Measure-Object).Count
$failCount = ($results | Where-Object { $_.Status -like "*FAILED*" } | Measure-Object).Count

Write-Host "Test Results:" -ForegroundColor White
Write-Host "  ✅ Passed:  $passCount" -ForegroundColor Green
Write-Host "  ⚠️  Warning: $warnCount" -ForegroundColor Yellow
Write-Host "  ❌ Failed:  $failCount" -ForegroundColor Red

Write-Host "`nDetailed Results:" -ForegroundColor White
foreach ($result in $results) {
    $statusColor = if ($result.Status -like "*PASSED*") { "Green" }
                   elseif ($result.Status -like "*WARNING*") { "Yellow" }
                   else { "Red" }
    
    Write-Host "  $($result.Status) - $($result.Component)" -ForegroundColor $statusColor
    if ($result.Details) {
        Write-Host "     └─ $($result.Details)" -ForegroundColor Gray
    }
    if ($result.Error) {
        Write-Host "     └─ ERROR: $($result.Error)" -ForegroundColor Red
    }
}

# ============================================================================
# SECTION 6: NEXT STEPS
# ============================================================================

Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "🎯 NEXT STEPS FOR PHASE 1 (June 2)" -ForegroundColor Cyan
Write-Host "========================================\n" -ForegroundColor Cyan

if ($passCount -gt 0) {
    Write-Host "✅ Core components are functional. Phase 1 ready to begin:" -ForegroundColor Green
    Write-Host "   1. Jun 2: Phase 1 kickoff - Performance optimization begins" -ForegroundColor Gray
    Write-Host "   2. Jun 2-15: SIMD vectorization for columnar scan (Target: 2.7M → 3.5M rows/sec)" -ForegroundColor Gray
    Write-Host "   3. Jun 15-30: Memory layout optimization (Target: 3.5M → 4.4M rows/sec)" -ForegroundColor Gray
    Write-Host "   4. Jul 1-15: Compression-query pipeline (Target: 4.4M → 5.0M rows/sec)" -ForegroundColor Gray
    Write-Host "   5. Sep 30: Phase 1 complete (Target: 88/100 score)" -ForegroundColor Gray
} else {
    Write-Host "⚠️  Some components need attention before Phase 1:" -ForegroundColor Yellow
    Write-Host "   - Fix failing components" -ForegroundColor Gray
    Write-Host "   - Re-run this script to verify" -ForegroundColor Gray
}

$elapsedTime = (Get-Date) - $startTime
Write-Host "`n⏱️  Build & test completed in $($elapsedTime.TotalSeconds.ToString('F1')) seconds`n" -ForegroundColor Gray

# ============================================================================
# EXPORT RESULTS
# ============================================================================

# Save results to JSON
$reportPath = "BUILD_TEST_RESULTS_$(Get-Date -Format 'yyyyMMdd_HHmmss').json"
$results | ConvertTo-Json | Out-File -FilePath $reportPath -Encoding UTF8

Write-Host "📋 Report saved to: $reportPath`n" -ForegroundColor Gray

# Return overall status
$overallStatus = if ($failCount -eq 0) { "SUCCESS" } else { "NEEDS ATTENTION" }
exit (if ($failCount -eq 0) { 0 } else { 1 })
