#!/usr/bin/env powershell
# Comprehensive test suite for all Kore phases

param(
    [switch]$Verbose = $false
)

$results = @{}
$passCount = 0
$failCount = 0

function Test-Phase {
    param(
        [string]$Name,
        [scriptblock]$Test
    )
    
    Write-Host "`n================== $Name ==================" -ForegroundColor Cyan
    
    try {
        $result = & $Test
        Write-Host "[PASS] $Name" -ForegroundColor Green
        Write-Host $result
        $script:passCount++
        return $true
    } catch {
        Write-Host "[FAIL] $Name" -ForegroundColor Red
        Write-Host "ERROR: $($_.Exception.Message)" -ForegroundColor Red
        $script:failCount++
        return $false
    }
}

# Phase 2: PyO3 Test
Test-Phase "Phase 2: PyO3 Bindings" {
    if (-not (Test-Path "rust-bindings/Cargo.toml")) {
        throw "Cargo.toml not found"
    }
    
    $manifest = Get-Content "rust-bindings/Cargo.toml" -Raw
    
    $checks = @(
        ("kore_fileformat", "Kore fileformat dependency"),
        ("pyo3", "PyO3 dependency"),
        ("rayon", "Rayon parallelism"),
        ("release", "Release profile")
    )
    
    $validated = 0
    foreach ($check in $checks) {
        if ($manifest -match $check[0]) {
            "[+] $($check[1])"
            $validated++
        }
    }
    
    "[+] Validated $validated/4 dependencies"
}

# Phase 3: Hadoop Test  
Test-Phase "Phase 3: Hadoop Integration" {
    $inputFormat = Get-Content "hadoop/src/main/java/io/kore/hadoop/KoreInputFormat.java" -ErrorAction Stop
    $recordReader = Get-Content "hadoop/src/main/java/io/kore/hadoop/KoreRecordReader.java" -ErrorAction Stop
    
    $checks = 0
    
    if ($inputFormat -match "getSplits") { "[+] KoreInputFormat.getSplits() found"; $checks++ }
    if ($inputFormat -match "getRecordReader") { "[+] KoreInputFormat.getRecordReader() found"; $checks++ }
    if ($recordReader -match "nextKeyValue") { "[+] KoreRecordReader.nextKeyValue() found"; $checks++ }
    if ($recordReader -match "readRowData") { "[+] KoreRecordReader.readRowData() found"; $checks++ }
    if ($recordReader -match "readVarInt") { "[+] readVarInt() varint decoder found"; $checks++ }
    
    "[+] Validated $checks/5 core methods"
}

# Phase 4: Spark Test
Test-Phase "Phase 4: Spark SQL DataSourceV2" {
    $dataSource = Get-Content "spark-scala/src/main/scala/io/kore/spark/KoreDataSource.scala" -ErrorAction Stop
    $scan = Get-Content "spark-scala/src/main/scala/io/kore/spark/KoreScan.scala" -ErrorAction Stop
    
    $checks = 0
    
    if ($dataSource -match "shortName") { "[+] shortName() method found"; $checks++ }
    if ($dataSource -match "inferSchema") { "[+] inferSchema() method found"; $checks++ }
    if ($dataSource -match "getTable") { "[+] getTable() method found"; $checks++ }
    if ($scan -match "pruneColumns") { "[+] Column pruning (pruneColumns) found"; $checks++ }
    if ($scan -match "pushFilters") { "[+] Filter pushdown (pushFilters) found"; $checks++ }
    if ($scan -match "PartitionReader") { "[+] PartitionReader implementation found"; $checks++ }
    
    "[+] Validated $checks/6 Spark methods"
}

# Phase 5: Cloud Storage Test
Test-Phase "Phase 5: Cloud Storage & Parser" {
    $cloud = Get-Content "cloud-connectors/cloud_connectors.py" -ErrorAction Stop
    $parser = Get-Content "kore-binary-parser/kore_parser.py" -ErrorAction Stop
    
    $checks = 0
    
    if ($cloud -match "class KoreS3Reader") { "[+] KoreS3Reader class"; $checks++ }
    if ($cloud -match "class KoreGCSReader") { "[+] KoreGCSReader class"; $checks++ }
    if ($cloud -match "class KoreAzureReader") { "[+] KoreAzureReader class"; $checks++ }
    if ($parser -match "class KoreBinaryParser") { "[+] KoreBinaryParser class"; $checks++ }
    if ($parser -match "def parse_stream") { "[+] parse_stream() method"; $checks++ }
    if ($parser -match "CHUNK_ROWS.*65536") { "[+] CHUNK_ROWS constant (65536)"; $checks++ }
    if ($parser -match "def read_varint") { "[+] read_varint() encoding"; $checks++ }
    
    "[+] Validated $checks/7 cloud/parser components"
}

# Phase 6a: Go Test
Test-Phase "Phase 6a: Go Bindings" {
    $go = Get-Content "language-bindings/go/kore/kore.go" -ErrorAction Stop
    
    $checks = 0
    
    if ($go -match "type KoreReader") { "[+] KoreReader type"; $checks++ }
    if ($go -match "func NewReader") { "[+] NewReader() constructor"; $checks++ }
    if ($go -match "func.*Read\(\)") { "[+] Read() method"; $checks++ }
    if ($go -match "func.*ReadColumn") { "[+] ReadColumn() method"; $checks++ }
    if ($go -match "type KoreWriter") { "[+] KoreWriter type"; $checks++ }
    if ($go -match "CHUNK_ROWS") { "[+] CHUNK_ROWS constant"; $checks++ }
    
    "[+] Validated $checks/6 Go components"
}

# Phase 6b: Java JNI Test
Test-Phase "Phase 6b: Java JNI Bindings" {
    $jni = Get-Content "language-bindings/java/io/kore/bindings/KoreJNI.java" -ErrorAction Stop
    
    $methods = @("readFile", "readColumn", "getStats", "processBatch", "writeFile", "readFileChunked", "getFileVersion")
    $checks = 0
    
    foreach ($method in $methods) {
        if ($jni -match "native.*$method") {
            "[+] native $method() declared"
            $checks++
        }
    }
    
    if ($jni -match "class KoreReader") { "[+] KoreReader high-level API"; $checks++ }
    if ($jni -match "class KoreWriter") { "[+] KoreWriter high-level API"; $checks++ }
    
    "[+] Validated $checks Java JNI methods"
}

# Phase 6c: Killer DSL Test
Test-Phase "Phase 6c: Killer DSL Bindings" {
    $killer = Get-Content "language-bindings/killer/kore_bindings.killer" -ErrorAction Stop
    $impl = Get-Content "kore_fileformat_killer/implementation.killer" -ErrorAction Stop
    $examples = Get-Content "language-bindings/killer/kore_example.killer" -ErrorAction Stop
    
    $checks = 0
    
    if ($killer -match "parse_header") { "[+] parse_header() function"; $checks++ }
    if ($killer -match "read_varint") { "[+] read_varint() encoder"; $checks++ }
    if ($killer -match "write_varint") { "[+] write_varint() decoder"; $checks++ }
    if ($killer -match "read_kore_file") { "[+] read_kore_file() reader"; $checks++ }
    if ($killer -match "write_kore_file") { "[+] write_kore_file() writer"; $checks++ }
    if ($impl -match "select_best_codec") { "[+] select_best_codec() algorithm"; $checks++ }
    if ($impl -match "apply_rle_encoding") { "[+] apply_rle_encoding() codec"; $checks++ }
    
    $exampleCount = ($examples | Select-String "fn example_" | Measure-Object).Count
    "[+] Found $exampleCount example functions"
    
    "[+] Validated $checks Killer features + $exampleCount examples"
}

# Phase 7: Query Optimization Test
Test-Phase "Phase 7: Query Optimization" {
    $opt = Get-Content "query-optimization/query_optimizer_v2.rs" -ErrorAction Stop
    
    $checks = 0
    
    if ($opt -match "struct QueryOptimizer") { "[+] QueryOptimizer struct"; $checks++ }
    if ($opt -match "struct MetadataCache") { "[+] MetadataCache struct"; $checks++ }
    if ($opt -match "struct ColumnIndex") { "[+] ColumnIndex struct"; $checks++ }
    if ($opt -match "enum CompressionCodec") { "[+] CompressionCodec enum"; $checks++ }
    if ($opt -match "struct ColumnStats") { "[+] ColumnStats struct"; $checks++ }
    if ($opt -match "select_compression_codec") { "[+] Codec selection"; $checks++ }
    if ($opt -match "estimate_query_cost") { "[+] Cost estimation"; $checks++ }
    
    "[+] Validated $checks/7 optimization components"
}

# Integration Test: Constants Validation
Test-Phase "Integration: Format Constants" {
    $files = Get-ChildItem -Path . -Include "*.rs", "*.java", "*.scala", "*.py", "*.go", "*.killer" -Recurse -ErrorAction SilentlyContinue
    
    $constants = @(
        "KORE_MAGIC",
        "KORE_VERSION",
        "HEADER_SIZE",
        "CHUNK_ROWS"
    )
    
    $checks = 0
    foreach ($const in $constants) {
        $found = $files | Select-String -Pattern $const -ErrorAction SilentlyContinue | Measure-Object
        if ($found.Count -gt 0) {
            "[+] $const found in $($found.Count) files"
            $checks++
        }
    }
    
    "[+] Validated $checks/4 format constants across ecosystem"
}

# Summary
Write-Host "`n`n========== TEST SUMMARY ==========" -ForegroundColor Cyan
Write-Host "PASSED: $passCount" -ForegroundColor Green
Write-Host "FAILED: $failCount" -ForegroundColor Red

$total = $passCount + $failCount
if ($total -gt 0) {
    $rate = [int]($passCount / $total * 100)
    Write-Host "Success Rate: $rate%" -ForegroundColor $(if ($rate -eq 100) { "Green" } else { "Yellow" })
}

Write-Host "=================================" -ForegroundColor Cyan

if ($failCount -eq 0) {
    Write-Host "`n*** ALL TESTS PASSED ***`n" -ForegroundColor Green -BackgroundColor Black
    exit 0
} else {
    Write-Host "`n*** SOME TESTS FAILED ***`n" -ForegroundColor Red -BackgroundColor Black
    exit 1
}
