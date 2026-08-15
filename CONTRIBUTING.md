# Contributing to KORE FileFormat

Thank you for your interest in contributing to KORE!

## How to Contribute

### Reporting Bugs
- Use [GitHub Issues](https://github.com/arunkatherashala/Kore/issues/new?template=bug_report.md)
- Include OS, language SDK version, and a minimal reproduction

### Suggesting Features
- Open a [Feature Request](https://github.com/arunkatherashala/Kore/issues/new?template=feature_request.md)

### Submitting Code

1. Fork the repo
2. Create a branch: `git checkout -b feat/my-feature`
3. Make changes and add tests
4. Run tests: `cargo test` (Rust), `pytest` (Python), `npm test` (Node)
5. Commit with conventional messages: `feat:`, `fix:`, `docs:`, `chore:`
6. Push and open a Pull Request

### Code Style
- **Rust:** `cargo fmt` and `cargo clippy`
- **Python:** PEP 8, type hints preferred
- **Node.js:** ESLint defaults
- **Go:** `gofmt`

### Testing
- All PRs must pass CI
- Add tests for new features
- Maintain or improve code coverage

## SDK Development

Each SDK lives in its own directory:
| SDK | Directory |
|-----|-----------|
| Python | `kore-python/` |
| Node.js | `kore-node/` |
| Rust | `kore-core/`, `kore-io/` |
| Ruby | `kore-ruby/` |
| Java | `maven/` |
| C# | `csharp/` |
| Go | `kore-go/` |
| PHP | `kore-php/` |

## Questions?
Open a [Discussion](https://github.com/arunkatherashala/Kore/discussions) or file an issue.
