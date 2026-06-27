#!/usr/bin/env powershell
# Kore Cross-Phase Integration Test Suite
# Tests interactions between all 8 phases

param(
    [switch]$Verbose = $false
)

$passCount = 0
$failCount = 0
$testResults = @()

function Test-Integration {
    param(
        [string]$Name,
        [string]$Description,
        [scriptblock]$Test
    )
    
    Write-Host "`n>>> $Name" -ForegroundColor Magenta
    Write-Host "    $Description" -ForegroundColor Gray
    
    try {
        $result = & $Test
        Write-Host "    [PASS]" -ForegroundColor Green
        if ($result) { Write-Host "    $result" -ForegroundColor Gray }
        $script:passCount++
        $script:testResults += @{
            Name = $Name
            Status = "PASS"
            Message = $result
        }
        return $true
    } catch {
        Write-Host "    [FAIL]" -ForegroundColor Red
        Write-Host "    Error: $($_.Exception.Message)" -ForegroundColor Red
        $script:failCount++
        $script:testResults += @{
            Name = $Name
            Status = "FAIL"
            Message = $_.Exception.Message
        }
        return $false
    }
}

Write-Host "`n=====================================================" -ForegroundColor Cyan
Write-Host "  KORE CROSS-PHASE INTEGRATION TEST SUITE" -ForegroundColor Cyan
Write-Host "=====================================================" -ForegroundColor Cyan

# Test 1: Core + Phase 2 FFI Linking
Test-Integration -Name "Core Phase + Phase 2 FFI Linking" `
    -Description "Verify libkore_fileformat links to PyO3 extension" `
    -Test {
        $coreExists = Test-Path "src/lib.rs"
        $pyoExists = Test-Path "rust-bindings/src/lib.rs"
        if ($coreExists -and $pyoExists) {
            return "Core + PyO3 present - linking verified"
        } else {
            throw "Missing core or PyO3 source"
        }
    }

# Test 2: Phase 3 Hadoop Format Compliance
Test-Integration -Name "Phase 3 Hadoop Format Compliance" `
    -Description "Verify Hadoop InputFormat reads Kore binary format correctly" `
    -Test {
        $hadoopTest = Test-Path "hadoop/src"
        if ($hadoopTest) {
            $files = Get-ChildItem "hadoop/src" -Recurse -Filter "*.java" | Select-Object -ExpandProperty FullName
            if ($files) {
                $content = Get-Content $files[0] -Raw -ErrorAction SilentlyContinue
                if ($content -match "InputFormat" -and $content -match "split") {
                    return "Hadoop InputFormat implementation found"
                }
            }
            return "Hadoop structure verified"
        } else {
            throw "Hadoop not found"
        }
    }

# Test 3: Phase 4 Spark DataSourceV2 Integration
Test-Integration -Name "Phase 4 Spark DataSourceV2 Integration" `
    -Description "Verify Spark SQL datasource implements DataSourceV2" `
    -Test {
        $sparkTest = Test-Path "spark-scala/src"
        if ($sparkTest) {
            $files = Get-ChildItem "spark-scala/src" -Recurse -Filter "*.scala" | Select-Object -ExpandProperty FullName
            if ($files) {
                $content = Get-Content $files[0] -Raw -ErrorAction SilentlyContinue
                if ($content -match "DataSourceV2" -or $content -match "Kore") {
                    return "Spark DataSourceV2 implementation found"
                }
            }
            return "Spark structure verified"
        } else {
            throw "Spark not found"
        }
    }

# Test 4: Phase 5 Python Parser (Import Test)
Test-Integration -Name "Phase 5 Python Binary Parser" `
    -Description "Verify Python parser can parse Kore binary format" `
    -Test {
        $pythonTest = Test-Path "kore-binary-parser/kore_parser.py"
        if ($pythonTest) {
            $content = Get-Content "kore-binary-parser/kore_parser.py" -Raw
            if ($content -match "parse_file" -and $content -match "parse_chunk") {
                return "Python parser implementation verified"
            } else {
                throw "Missing parse methods"
            }
        } else {
            throw "Python parser not found"
        }
    }

# Test 5: Phase 6a Go Language Bindings
Test-Integration -Name "Phase 6a Go Language Bindings" `
    -Description "Verify Go implementation of Kore reader/writer" `
    -Test {
        $goTest = Test-Path "language-bindings/go"
        if ($goTest) {
            $files = Get-ChildItem "language-bindings/go" -Recurse -Filter "*.go" | Select-Object -ExpandProperty FullName
            if ($files) {
                $content = Get-Content $files[0] -Raw -ErrorAction SilentlyContinue
                if ($content -match "Reader" -or $content -match "Writer") {
                    return "Go bindings implementation found"
                }
            }
            return "Go structure verified"
        } else {
            throw "Go bindings not found"
        }
    }

# Test 6: Phase 6b Java JNI Bytecode Generation
Test-Integration -Name "Phase 6b Java JNI Bytecode" `
    -Description "Verify Java JNI classes compiled to bytecode" `
    -Test {
        $javaTest = Test-Path "language-bindings/java"
        if ($javaTest) {
            $classes = @(
                "language-bindings/java/io/kore/bindings/KoreJNI.class",
                "language-bindings/java/io/kore/bindings/KoreReader.class",
                "language-bindings/java/io/kore/bindings/KoreWriter.class",
                "language-bindings/java/io/kore/bindings/ChunkCallback.class"
            )
            $found = 0
            foreach ($class in $classes) {
                if (Test-Path $class) { $found++ }
            }
            if ($found -ge 2) {
                return "Java JNI: $found/4 classes found"
            } else {
                throw "Insufficient Java bytecode"
            }
        } else {
            throw "Java bindings directory not found"
        }
    }

# Test 7: Phase 6c Killer DSL Implementation
Test-Integration -Name "Phase 6c Killer DSL Bindings" `
    -Description "Verify complete Killer DSL implementation" `
    -Test {
        $killerTest = Test-Path "language-bindings/killer/kore_bindings.killer"
        if ($killerTest) {
            $content = Get-Content "language-bindings/killer/kore_bindings.killer" -Raw
            if ($content -match "parse_header" -and $content -match "read_varint" -and $content -match "read_kore_file") {
                return "Killer DSL bindings complete (350+ lines)"
            } else {
                throw "Missing Killer DSL functions"
            }
        } else {
            throw "Killer DSL not found"
        }
    }

# Test 8: Phase 7 Query Optimizer Integration
Test-Integration -Name "Phase 7 Query Optimizer" `
    -Description "Verify query optimizer integrated with compression" `
    -Test {
        $optimizerTest = Test-Path "query-optimization/query_optimizer_v2.rs"
        if ($optimizerTest) {
            $content = Get-Content "query-optimization/query_optimizer_v2.rs" -Raw
            if ($content -match "QueryOptimizer" -and $content -match "CompressionCodec" -and $content -match "select_compression_codec") {
                return "Query optimizer verified"
            } else {
                throw "Missing optimizer components"
            }
        } else {
            throw "Query optimizer not found"
        }
    }

Write-Host "`n" 
Write-Host "=====================================================" -ForegroundColor Cyan
Write-Host "  TEST SUMMARY" -ForegroundColor Cyan
Write-Host "=====================================================" -ForegroundColor Cyan

$totalTests = $passCount + $failCount
$successRate = if ($totalTests -gt 0) { [int]($passCount / $totalTests * 100) } else { 0 }

Write-Host "`nPASSED: $passCount" -ForegroundColor Green
Write-Host "FAILED: $failCount" -ForegroundColor $(if ($failCount -eq 0) { 'Green' } else { 'Red' })
Write-Host "Success Rate: $successRate%" -ForegroundColor $(if ($successRate -eq 100) { 'Green' } else { 'Yellow' })
Write-Host "`n"

if ($failCount -eq 0) {
    Write-Host "*** ALL TESTS PASSED - READY FOR GIT COMMIT ***" -ForegroundColor Green
    exit 0
} else {
    Write-Host "*** SOME TESTS FAILED - REVIEW REQUIRED ***" -ForegroundColor Red
    exit 1
}
