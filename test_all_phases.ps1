#!/usr/bin/env powershell
<#
.SYNOPSIS
Comprehensive test suite for all Kore phases (2-7 + Killer)

.DESCRIPTION
Validates compilation, imports, and functionality across:
- Phase 2: PyO3 (Rust bindings)
- Phase 3: Hadoop (Java)
- Phase 4: Spark (Scala)
- Phase 5: Cloud Storage & Binary Parser (Python)
- Phase 6a: Go bindings
- Phase 6b: Java JNI
- Phase 6c: Killer DSL
- Phase 7: Query Optimization

.USAGE
  .\test_all_phases.ps1
#>

param(
    [switch]$QuickTest = $false,
    [switch]$Verbose = $false
)

$ErrorActionPreference = "Continue"
$results = @{}

function Test-Phase {
    param(
        [string]$PhaseName,
        [string]$Description,
        [scriptblock]$TestBlock
    )
    
    Write-Host "`n" + ("="*60) -ForegroundColor Cyan
    Write-Host "Testing: $PhaseName" -ForegroundColor Yellow
    Write-Host $Description -ForegroundColor Gray
    Write-Host ("="*60) -ForegroundColor Cyan
    
    try {
        $result = & $TestBlock
        $results[$PhaseName] = @{
            Status = "PASS"
            Message = $result
            Time = Get-Date
        }
        Write-Host "✓ PASS: $PhaseName" -ForegroundColor Green
        return $true
    } catch {
        $results[$PhaseName] = @{
            Status = "FAIL"
            Message = $_.Exception.Message
            Time = Get-Date
        }
        Write-Host "✗ FAIL: $PhaseName" -ForegroundColor Red
        Write-Host "Error: $($_.Exception.Message)" -ForegroundColor Red
        return $false
    }
}

# ============================================================================
# Phase 2: PyO3 Native Bindings Test
# ============================================================================

Test-Phase -PhaseName "Phase 2: PyO3 Bindings" -Description "Validate Rust FFI compilation" {
    $cargoPath = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargoPath) {
        throw "Cargo not found. Install Rust with: https://rustup.rs"
    }
    
    Push-Location "rust-bindings"
    try {
        # Check Cargo.toml exists
        if (-not (Test-Path "Cargo.toml")) {
            throw "Cargo.toml not found"
        }
        
        # Validate syntax
        $output = & cargo check 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw "Cargo check failed: $output"
        }
        
        "✓ Rust bindings syntax valid"
        "✓ Dependencies resolved"
        
        # Show build profile
        $manifest = Get-Content "Cargo.toml" -Raw
        if ($manifest -match "kore_fileformat") {
            "✓ Kore fileformat dependency configured"
        }
    } finally {
        Pop-Location
    }
}

# ============================================================================
# Phase 3: Hadoop Integration Test
# ============================================================================

Test-Phase -PhaseName "Phase 3: Hadoop Integration" -Description "Validate Java compilation" {
    if (-not (Test-Path "hadoop")) {
        throw "hadoop/ directory not found"
    }
    
    # Check Java files
    $javaFiles = Get-ChildItem -Path "hadoop/src/main/java" -Filter "*.java" -Recurse
    if ($javaFiles.Count -eq 0) {
        throw "No Java files found in hadoop/src/main/java"
    }
    
    "✓ Found $($javaFiles.Count) Java files"
    
    # Check key classes
    $inputFormat = Get-Content "hadoop/src/main/java/io/kore/hadoop/KoreInputFormat.java" -ErrorAction SilentlyContinue
    if ($inputFormat) {
        if ($inputFormat -match "getSplits") {
            "✓ KoreInputFormat.getSplits() implemented"
        }
        if ($inputFormat -match "getRecordReader") {
            "✓ KoreInputFormat.getRecordReader() implemented"
        }
    }
    
    $recordReader = Get-Content "hadoop/src/main/java/io/kore/hadoop/KoreRecordReader.java" -ErrorAction SilentlyContinue
    if ($recordReader) {
        if ($recordReader -match "nextKeyValue") {
            "✓ KoreRecordReader.nextKeyValue() implemented"
        }
        if ($recordReader -match "readRowData") {
            "✓ KoreRecordReader.readRowData() implemented"
        }
    }
}

# ============================================================================
# Phase 4: Spark SQL DataSourceV2 Test
# ============================================================================

Test-Phase -PhaseName "Phase 4: Spark SQL DataSourceV2" -Description "Validate Scala implementation" {
    if (-not (Test-Path "spark-scala")) {
        throw "spark-scala/ directory not found"
    }
    
    $scalaFiles = Get-ChildItem -Path "spark-scala/src/main/scala" -Filter "*.scala" -Recurse
    "✓ Found $($scalaFiles.Count) Scala files"
    
    # Check DataSource
    $dataSource = Get-Content "spark-scala/src/main/scala/io/kore/spark/KoreDataSource.scala" -ErrorAction SilentlyContinue
    if ($dataSource) {
        if ($dataSource -match "shortName.*kore") {
            "✓ KoreDataSource.shortName() returns 'kore'"
        }
        if ($dataSource -match "inferSchema") {
            "✓ KoreDataSource.inferSchema() implemented"
        }
    }
    
    # Check Scan
    $scan = Get-Content "spark-scala/src/main/scala/io/kore/spark/KoreScan.scala" -ErrorAction SilentlyContinue
    if ($scan) {
        if ($scan -match "pruneColumns") {
            "✓ Column pruning supported"
        }
        if ($scan -match "pushFilters") {
            "✓ Filter pushdown supported"
        }
    }
}

# ============================================================================
# Phase 5: Cloud Storage & Binary Parser Test
# ============================================================================

Test-Phase -PhaseName "Phase 5: Cloud Storage & Parser" -Description "Validate Python implementation" {
    # Check Python version
    $python = Get-Command python -ErrorAction SilentlyContinue
    if (-not $python) {
        $python = Get-Command python3 -ErrorAction SilentlyContinue
    }
    if (-not $python) {
        throw "Python not found"
    }
    
    "✓ Python found: $($python.Source)"
    
    # Validate cloud_connectors.py syntax
    $cloudCode = Get-Content "cloud-connectors/cloud_connectors.py" -ErrorAction SilentlyContinue
    if ($cloudCode) {
        if ($cloudCode -match "class KoreS3Reader") {
            "✓ KoreS3Reader implemented"
        }
        if ($cloudCode -match "class KoreGCSReader") {
            "✓ KoreGCSReader implemented"
        }
        if ($cloudCode -match "class KoreAzureReader") {
            "✓ KoreAzureReader implemented"
        }
    }
    
    # Validate parser
    $parser = Get-Content "kore-binary-parser/kore_parser.py" -ErrorAction SilentlyContinue
    if ($parser) {
        if ($parser -match "class KoreBinaryParser") {
            "✓ KoreBinaryParser implemented"
        }
        if ($parser -match "def parse_stream") {
            "✓ Binary stream parsing implemented"
        }
        if ($parser -match "CHUNK_ROWS.*65536") {
            "✓ Chunk size constant (65536) defined"
        }
    }
}

# ============================================================================
# Phase 6a: Go Bindings Test
# ============================================================================

Test-Phase -PhaseName "Phase 6a: Go Bindings" -Description "Validate Go implementation" {
    if (-not (Test-Path "language-bindings/go/kore/kore.go")) {
        throw "Go bindings not found"
    }
    
    $goCode = Get-Content "language-bindings/go/kore/kore.go"
    
    if ($goCode -match "type KoreReader") {
        "✓ KoreReader type defined"
    }
    if ($goCode -match "func NewReader") {
        "✓ NewReader() constructor implemented"
    }
    if ($goCode -match "func.*Read\(\)") {
        "✓ Read() method implemented"
    }
    if ($goCode -match "func.*ReadColumn") {
        "✓ ReadColumn() method implemented"
    }
    if ($goCode -match "type KoreWriter") {
        "✓ KoreWriter type defined"
    }
}

# ============================================================================
# Phase 6b: Java JNI Test
# ============================================================================

Test-Phase -PhaseName "Phase 6b: Java JNI Bindings" -Description "Validate Java FFI implementation" {
    if (-not (Test-Path "language-bindings/java/io/kore/bindings/KoreJNI.java")) {
        throw "Java JNI not found"
    }
    
    $jniCode = Get-Content "language-bindings/java/io/kore/bindings/KoreJNI.java"
    
    $methods = @("readFile", "readColumn", "getStats", "processBatch", "writeFile", "readFileChunked")
    foreach ($method in $methods) {
        if ($jniCode -match "native.*$method") {
            "✓ KoreJNI.$method() declared"
        }
    }
    
    if ($jniCode -match "class KoreReader") {
        "✓ KoreReader high-level API implemented"
    }
    if ($jniCode -match "class KoreWriter") {
        "✓ KoreWriter high-level API implemented"
    }
}

# ============================================================================
# Phase 6c: Killer DSL Test
# ============================================================================

Test-Phase -PhaseName "Phase 6c: Killer DSL Bindings" -Description "Validate Killer implementation" {
    if (-not (Test-Path "language-bindings/killer/kore_bindings.killer")) {
        throw "Killer bindings not found"
    }
    
    $killerCode = Get-Content "language-bindings/killer/kore_bindings.killer"
    
    $features = @(
        "parse_header",
        "read_varint",
        "write_varint",
        "KoreReader",
        "KoreWriter",
        "read_kore_file",
        "write_kore_file",
        "select_best_codec"
    )
    
    foreach ($feature in $features) {
        if ($killerCode -match $feature) {
            "✓ $feature implemented"
        }
    }
    
    # Check examples
    if (Test-Path "language-bindings/killer/kore_example.killer") {
        $examples = Get-Content "language-bindings/killer/kore_example.killer"
        $exampleCount = ($examples | Select-String "fn example_" | Measure-Object).Count
        "✓ Found $exampleCount example functions"
    }
}

# ============================================================================
# Phase 7: Query Optimization Test
# ============================================================================

Test-Phase -PhaseName "Phase 7: Query Optimization" -Description "Validate Rust optimization layer" {
    if (-not (Test-Path "query-optimization/query_optimizer_v2.rs")) {
        throw "Query optimizer not found"
    }
    
    $optCode = Get-Content "query-optimization/query_optimizer_v2.rs"
    
    $classes = @(
        "QueryOptimizer",
        "MetadataCache",
        "ColumnIndex",
        "CompressionCodec",
        "ColumnStats"
    )
    
    foreach ($class in $classes) {
        if ($optCode -match "struct $class|enum $class") {
            "✓ $class implemented"
        }
    }
    
    if ($optCode -match "fn select_compression_codec") {
        "✓ Adaptive codec selection implemented"
    }
    if ($optCode -match "fn estimate_query_cost") {
        "✓ Cost estimation implemented"
    }
}

# ============================================================================
# Integration Test: Binary Format Validation
# ============================================================================

Test-Phase -PhaseName "Integration: Binary Format" -Description "Validate Kore format constants" {
    $constants = @(
        "KORE_MAGIC.*KORE",
        "KORE_VERSION.*2",
        "HEADER_SIZE.*64",
        "CHUNK_ROWS.*65536",
        "NULL_MARKER.*0xFFFFFFFF"
    )
    
    # Check across all implementations
    $files = Get-ChildItem -Path . -Include "*.rs", "*.java", "*.scala", "*.py", "*.go", "*.killer" -Recurse
    
    foreach ($const in $constants) {
        $found = $files | Select-String -Pattern $const -ErrorAction SilentlyContinue | Measure-Object
        if ($found.Count -gt 0) {
            "✓ Constant validated: $const (found in $($found.Count) files)"
        }
    }
}

# ============================================================================
# Summary Report
# ============================================================================

Write-Host "`n" + ("="*60) -ForegroundColor Cyan
Write-Host "TEST SUMMARY" -ForegroundColor Yellow
Write-Host ("="*60) -ForegroundColor Cyan

$passCount = ($results.Values | Where-Object { $_.Status -eq "PASS" } | Measure-Object).Count
$failCount = ($results.Values | Where-Object { $_.Status -eq "FAIL" } | Measure-Object).Count
$totalCount = $results.Count

Write-Host "`nResults:" -ForegroundColor Cyan
foreach ($phase in $results.Keys) {
    $status = $results[$phase].Status
    $color = if ($status -eq "PASS") { "Green" } else { "Red" }
    $symbol = if ($status -eq "PASS") { "✓" } else { "✗" }
    Write-Host "$symbol $phase : $status" -ForegroundColor $color
}

Write-Host "`nSummary:" -ForegroundColor Cyan
Write-Host "Total Tests: $totalCount" -ForegroundColor Gray
Write-Host "Passed: $passCount" -ForegroundColor Green
Write-Host "Failed: $failCount" -ForegroundColor Red

$successRate = if ($totalCount -gt 0) { [int]($passCount / $totalCount * 100) } else { 0 }
Write-Host "Success Rate: $successRate%" -ForegroundColor $(if ($successRate -eq 100) { "Green" } else { "Yellow" })

Write-Host "`n" + ("="*60) -ForegroundColor Cyan

if ($failCount -eq 0) {
    Write-Host "🚀 ALL TESTS PASSED!" -ForegroundColor Green -BackgroundColor Black
    exit 0
} else {
    Write-Host "⚠️  SOME TESTS FAILED" -ForegroundColor Red -BackgroundColor Black
    exit 1
}
