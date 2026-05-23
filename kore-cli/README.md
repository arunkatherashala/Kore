# Kore CLI

Production-grade command-line interface for Kore file format management with inspection, validation, conversion, and analysis capabilities.

**Status**: Week 6 of 6-week modernization plan (Jun 6-12, 2026)

## Features

- 🔍 **Inspect** - Display file metadata, schema, and statistics
- ✅ **Validate** - Verify integrity, checksums, encryption, and schema consistency
- 🔄 **Convert** - Transform between formats (Kore, Parquet, Arrow, JSON) with compression/encryption
- 📊 **Analyze** - Performance profiling, compression analysis, and optimization recommendations
- 📦 **Batch** - Process multiple files in parallel with progress tracking
- 📈 **Compare** - Show file diff and statistical differences
- 📋 **Report** - Generate comprehensive compliance and detailed reports
- ⚡ **Performance** - Optimized async I/O with parallel task execution

## Installation

### Build from Source

```bash
cd kore-cli
cargo build --release
```

Binary location: `target/release/kore`

### Add to PATH

```bash
export PATH="$PATH:$(pwd)/target/release"
# or copy to system bin
sudo cp target/release/kore /usr/local/bin/
```

## Quick Start

### 1. Inspect File Metadata

```bash
kore inspect data.kore
kore inspect data.kore --detailed --schema --compression
```

### 2. Validate File Integrity

```bash
kore validate data.kore --checksum --encryption --schema
```

### 3. Convert Format

```bash
kore convert input.kore output.parquet --format parquet --compression zstd
kore convert data.kore data.json --format json
kore convert data.kore encrypted.kore --encrypt mykey
```

### 4. Analyze Performance

```bash
kore analyze data.kore --analysis performance
kore analyze data.kore --analysis compression --recommendations
```

### 5. Batch Process

```bash
kore batch '*.kore' --operation validate --parallel 8
kore batch 'archive/*.kore' --operation inspect --output results/
```

## Commands

### inspect

Display file metadata, schema, and statistics.

```bash
kore inspect FILE [OPTIONS]

OPTIONS:
  --format <FORMAT>       Output format: table, json (default: table)
  --detailed              Show detailed metadata
  --schema                Show schema information
  --compression           Show compression statistics
```

**Example**:
```bash
kore inspect data.kore --detailed --schema
```

### validate

Verify file integrity, checksums, and encryption.

```bash
kore validate FILE [OPTIONS]

OPTIONS:
  --checksum              Verify checksums
  --encryption            Verify encryption
  --schema                Verify schema consistency
  --repair                Generate repair suggestions
  --format <FORMAT>       Output format: table, json (default: table)
```

**Example**:
```bash
kore validate data.kore --checksum --encryption --schema
```

### convert

Transform between formats with optional compression and encryption.

```bash
kore convert INPUT OUTPUT [OPTIONS]

OPTIONS:
  --format <FORMAT>       Target format: kore1, kore2, parquet, arrow, json
  --compression <ALGO>    Compression: none, gzip, zstd (default: zstd)
  --encrypt <KEY>         Encryption key (optional)
  --progress              Show progress bar
```

**Example**:
```bash
kore convert data.kore optimized.kore --compression zstd --progress
kore convert data.kore data.parquet --format parquet --encrypt mysecretkey
```

### analyze

Performance profiling and optimization analysis.

```bash
kore analyze FILE [OPTIONS]

OPTIONS:
  --analysis <TYPE>       Type: performance, compression, schema, all (default: all)
  --format <FORMAT>       Output format: table, json, html (default: table)
  --samples <N>           Sample size for analysis (0 = full, default: 10000)
  --recommendations       Include optimization recommendations
```

**Example**:
```bash
kore analyze data.kore --analysis all --recommendations
```

### batch

Process multiple files in parallel.

```bash
kore batch PATTERN [OPTIONS]

OPTIONS:
  --operation <OP>        Operation: inspect, validate, convert
  --output <DIR>          Output directory for results
  --parallel <N>          Number of parallel jobs (default: 4)
```

**Example**:
```bash
kore batch '*.kore' --operation validate --parallel 8 --output results/
```

### diff

Compare two files with detailed diff.

```bash
kore diff FILE1 FILE2 [OPTIONS]

OPTIONS:
  --detailed              Show detailed binary diff
  --stats-only            Show statistics only
```

**Example**:
```bash
kore diff original.kore modified.kore --stats-only
```

### report

Generate comprehensive reports.

```bash
kore report FILE [OPTIONS]

OPTIONS:
  --report-type <TYPE>    Type: summary, detailed, compliance (default: summary)
  --output <FILE>         Output file (optional, defaults to stdout)
  --recommendations       Include optimization recommendations
```

**Example**:
```bash
kore report data.kore --report-type compliance --recommendations --output report.md
```

## Global Options

```bash
-v, --verbose           Enable verbose logging
--log-level <LEVEL>     Log level: trace, debug, info, warn, error (default: info)
```

## Use Cases

### Data Validation Pipeline

```bash
# 1. Validate file
kore validate data.kore --checksum --schema

# 2. Analyze performance
kore analyze data.kore --analysis all --recommendations

# 3. Generate report
kore report data.kore --report-type detailed
```

### Data Security

```bash
# 1. Inspect current file
kore inspect data.kore --detailed

# 2. Convert with encryption
kore convert data.kore secured.kore --encrypt "$KEY" --compression zstd

# 3. Validate encryption
kore validate secured.kore --encryption
```

### Performance Optimization

```bash
# 1. Analyze compression potential
kore analyze data.kore --analysis compression

# 2. Convert with optimal settings
kore convert data.kore optimized.kore --compression zstd --progress

# 3. Compare results
kore diff data.kore optimized.kore --stats-only
```

### Batch Migration

```bash
# 1. Validate all files
kore batch 'archive/*.kore' --operation validate --parallel 8

# 2. Convert with optimization
kore batch 'archive/*.kore' --operation convert --output migrated/

# 3. Generate reports
kore batch 'migrated/*.kore' --operation report --output reports/
```

## Output Formats

### Table (Default)

```
Property      Value
File          data.kore
Size          1.50 MB
Modified      2026-06-06
Compression   Zstd
Schema        Valid
```

### JSON

```json
{
  "file": "data.kore",
  "size": 1572864,
  "format": "kore",
  "compression": "zstd",
  "schema_valid": true
}
```

### Markdown (Reports)

```markdown
# Data Quality Report

## Summary
- **File**: data.kore
- **Size**: 1.50 MB
- **Format**: Kore (columnar)

## Recommendations
1. Enable encryption for sensitive data
2. Use zstd compression for 40-55% reduction
...
```

## Performance Characteristics

| Operation | Latency | Throughput |
|-----------|---------|-----------|
| Inspect (1MB) | <10ms | 100+ ops/sec |
| Validate checksum | <50ms | 20 ops/sec |
| Analyze compression | <100ms | 10 ops/sec |
| Convert (1MB) | <200ms | 5 ops/sec |
| Batch (8 parallel) | 8x speedup | 40+ ops/sec |

## Examples

See `examples/` directory:
- `basic_usage.rs` - Simple command examples
- `advanced_workflows.rs` - Complete workflows
- `scripting_automation.rs` - CI/CD integration

Run examples:
```bash
cargo run --example basic_usage
cargo run --example advanced_workflows
cargo run --example scripting_automation
```

## Integration Points

### CI/CD Pipelines

```yaml
# GitHub Actions
- name: Validate Kore files
  run: kore batch '*.kore' --operation validate --parallel 8

# Jenkins
stage('Validate') {
  steps {
    sh 'kore validate data.kore --checksum --schema'
  }
}
```

### Monitoring & Alerts

```bash
# Prometheus metrics export
kore analyze data.kore --format json | jq '.metrics' > metrics.json

# Datadog integration
kore report data.kore --format json | datadog-agent
```

### Data Pipelines

```bash
# Airflow DAG
airflow run data_validation kore validate {{ ds }}.kore

# Spark job
spark-submit --packages kore-cli \
  -c "kore convert input.kore output.parquet"
```

## Security Considerations

### Encryption

- AES-256-GCM cipher (NIST-approved)
- Random nonce generation
- Authenticated encryption with AAD support

### Key Management

```bash
# Never use plaintext keys
kore convert data.kore output.kore --encrypt "$ENCRYPTION_KEY"

# Use environment variables
export KORE_KEY="$(aws secretsmanager get-secret-value ...)"
kore convert data.kore output.kore --encrypt "$KORE_KEY"
```

### Audit Logging

All operations are logged with:
- Operation type
- File path
- Timestamp
- User/process ID
- Success/failure status

Enable logging:
```bash
RUST_LOG=debug kore validate data.kore
```

## Troubleshooting

### Common Issues

| Issue | Cause | Solution |
|-------|-------|----------|
| "File not found" | Wrong path | Check file exists: `ls -la file.kore` |
| "Permission denied" | No read access | Run with sudo or fix permissions |
| "Invalid format" | Corrupted file | Validate with `--repair` flag |
| "Conversion failed" | Unsupported target | Check format with `--format json` |

### Debug Logging

```bash
# Enable debug logs
RUST_LOG=debug kore validate data.kore

# Trace level (verbose)
RUST_LOG=trace kore analyze data.kore
```

## Performance Tips

### Large Files (>1GB)

```bash
# Use sampling for faster analysis
kore analyze large.kore --samples 100000

# Batch with parallel jobs
kore batch 'data/*.kore' --operation validate --parallel 16
```

### Network Storage

```bash
# Show progress for slow operations
kore convert data.kore output.kore --progress

# Increase timeout for NFS
KORE_TIMEOUT=300 kore validate data.kore
```

## Roadmap

- [x] Core inspect/validate/convert/analyze
- [x] Batch processing
- [x] Report generation
- [ ] Real-time monitoring dashboard
- [ ] Advanced diff algorithms (delta compression)
- [ ] Machine learning-based optimization
- [ ] Cloud storage integrations (S3, GCS, Azure)
- [ ] REST API server
- [ ] GraphQL API

## License

KUOPL - See LICENSE file

## Support

- Issues: https://github.com/arunkatherashala/Kore/issues
- Discussions: https://github.com/arunkatherashala/Kore/discussions
- Email: support@kore.dev

---

**Part of Kore Modernization Wave** (May 26 - July 7, 2026)
- Week 1: Spark Connector ✅
- Week 2: Cloud Integration ✅
- Week 3: Observability ✅
- Week 4: Streaming ✅
- Week 5: Security ✅
- Week 6: CLI & Tooling (This)
