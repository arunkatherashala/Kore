# KORE v0.2.0 CI/CD Pipeline

Automated build, test, and deployment infrastructure for KORE data format.

## Workflows

### 1. **ci-cd.yml** - Main CI/CD Pipeline

Triggers on:
- Push to `main`, `release/*` branches
- Pull requests against `main`
- Manual workflow dispatch

#### Jobs:

1. **build-main** (Multi-platform)
   - Builds on: Ubuntu, Windows, macOS
   - Rust versions: Stable, Nightly
   - Caches Cargo artifacts
   - Uploads release artifacts

2. **test-main**
   - Unit tests (`cargo test --lib`)
   - Doc tests (`cargo test --doc`)
   - Integration tests (all test files)

3. **build-spark**
   - Builds `language-bindings/spark`
   - Runs 12 integration tests
   - Validates schema inference and batch reading

4. **build-aws-glue**
   - Builds `language-bindings/aws-glue`
   - Runs S3, CloudWatch, IAM tests
   - Validates ETL operations

5. **build-snowflake**
   - Builds `language-bindings/snowflake`
   - Runs 7 unit + async tests
   - Validates table loading and export

6. **quality**
   - `cargo fmt` - Format check
   - `cargo clippy` - Lint warnings
   - `cargo doc` - Documentation generation

7. **security**
   - `cargo audit` - Dependency vulnerability scan
   - Continues on error (advisory only)

8. **coverage**
   - Uses `cargo-tarpaulin` for code coverage
   - Uploads to Codecov
   - 300s timeout per crate

9. **release** (only on main/push)
   - Creates GitHub Release
   - Tags with version number
   - Includes release notes

10. **status** - Final status report
    - Aggregates results from all jobs
    - Exits with error if any job fails

### 2. **dev-build.yml** - Development Build

Triggers on:
- Push to `develop/v0.2.0`
- Pull requests against `develop/v0.2.0`
- Nightly schedule (2 AM UTC)

#### Jobs:

1. **dev-build**
   - Builds all modules in sequence
   - Runs comprehensive test suite
   - Placeholder for benchmarks

2. **format-check**
   - Ensures code follows rustfmt rules

3. **lint**
   - Runs clippy with all warnings

4. **deps**
   - Checks for outdated dependencies
   - Checks for duplicate dependencies

5. **docs**
   - Builds full documentation with private items

6. **report** - Development build summary

### 3. **release.yml** - Release & Deploy

Triggers on:
- Push of git tags matching `v*`

#### Jobs:

1. **build-release** (Multi-platform)
   - Builds for: Linux x86_64, macOS x86_64, Windows x86_64
   - Creates platform-specific binaries
   - Uploads artifacts

2. **create-release**
   - Downloads all artifacts
   - Generates release notes
   - Creates GitHub Release with assets

3. **publish-crates**
   - Publishes main crate to crates.io
   - Publishes Spark binding
   - Publishes AWS Glue binding
   - Publishes Snowflake binding
   - Idempotent (skips if already published)

4. **notify** - Release completion notification

## Configuration

### Secrets

Set these in GitHub repository settings:

```
CARGO_TOKEN     - Crates.io API token for publishing
GITHUB_TOKEN    - Auto-populated by GitHub (used for releases)
```

### Environment Variables

Set in workflow files:

```yaml
CARGO_TERM_COLOR: always      # Colored output
RUST_BACKTRACE: 1             # Full backtrace on panic
```

## Matrix Strategy

### OS Matrix (ci-cd.yml)
```
Ubuntu Latest  + Rust Stable
Ubuntu Latest  + Rust Nightly
Windows Latest + Rust Stable
Windows Latest + Rust Nightly
macOS Latest   + Rust Stable
macOS Latest   + Rust Nightly
```

### Target Matrix (release.yml)
```
x86_64-unknown-linux-gnu      (Linux)
x86_64-apple-darwin           (macOS)
x86_64-pc-windows-msvc        (Windows)
```

## Caching Strategy

- Caches: `~/.cargo/bin`, `~/.cargo/registry`, `~/.cargo/git`, `target/`
- Key: `${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}`
- Hits reduce build time by ~70%

## Build Times

### Without Cache
- Main crate: ~2-3 minutes
- All modules: ~6-8 minutes total

### With Cache
- Main crate: ~30-60 seconds
- All modules: ~2-3 minutes total

## Test Coverage

### Unit Tests
- Main crate: All modules tested
- Spark: 12 integration tests
- AWS Glue: 7 async tests
- Snowflake: 7 unit + 2 async tests
- Gorilla: 9 compression tests

### Total
- ~45 test cases
- ~98% code coverage target

## Artifact Artifacts

All builds produce:
- Compiled libraries (`*.rlib`)
- Platform binaries
- Documentation (HTML)
- Source maps for debugging

## Status Badges

Add to README.md:

```markdown
[![CI/CD](https://github.com/kore-io/kore/actions/workflows/ci-cd.yml/badge.svg)](https://github.com/kore-io/kore/actions/workflows/ci-cd.yml)
[![Dev Build](https://github.com/kore-io/kore/actions/workflows/dev-build.yml/badge.svg)](https://github.com/kore-io/kore/actions/workflows/dev-build.yml)
[![Release](https://github.com/kore-io/kore/actions/workflows/release.yml/badge.svg)](https://github.com/kore-io/kore/actions/workflows/release.yml)
```

## Manual Trigger

To manually trigger workflows:

```bash
# Trigger CI/CD workflow
gh workflow run ci-cd.yml --ref develop/v0.2.0

# Trigger dev build
gh workflow run dev-build.yml --ref develop/v0.2.0

# Trigger release (requires tag)
git tag v0.2.0
git push --tags
```

## Troubleshooting

### Build Fails
- Check `RUST_BACKTRACE=1` output
- Verify dependencies are available
- Check for platform-specific issues

### Tests Fail
- Run locally: `cargo test`
- Check test output in GitHub Actions logs
- Review recent code changes

### Publish Fails
- Verify `CARGO_TOKEN` is set correctly
- Check if version already exists on crates.io
- Ensure all dependencies are published first

## Future Enhancements

- [ ] Performance benchmarking on every commit
- [ ] Integration tests with real Snowflake/AWS accounts
- [ ] Automated dependency updates
- [ ] Binary distribution to GitHub Releases
- [ ] Container image builds and push to Docker Hub
- [ ] Deployment to cloud platforms
- [ ] Automated changelog generation
- [ ] Performance regression detection

## Contributing

When making changes:

1. Ensure local tests pass: `cargo test`
2. Format code: `cargo fmt`
3. Check lints: `cargo clippy`
4. Push to feature branch
5. GitHub Actions automatically runs CI/CD

## Performance Metrics

Track in Actions > All Workflows:

- Total build time
- Test count and pass rate
- Code coverage percentage
- Dependency count
- Release build size

---

**KORE v0.2.0** | CI/CD Infrastructure | May 2026
