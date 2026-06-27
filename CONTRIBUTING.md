# Contributing to Kore

Thank you for your interest in contributing to Kore! We welcome contributions from everyone.

## 🚀 Quick Start

### Prerequisites
- Rust 1.70+ (for Rust development)
- Python 3.9+ (for Python bindings)
- Node.js 18+ (for JavaScript bindings)
- Java 17+ (for Java bindings)

### Setting up your environment

```bash
# Clone the repository
git clone https://github.com/arunkatherashala/Kore.git
cd Kore

# Build Rust library
cargo build --release

# Setup Python environment (optional)
python -m venv .venv
source .venv/bin/activate  # On Windows: .venv\Scripts\activate
pip install -e .

# Setup Node.js (optional)
cd nodejs && npm install && cd ..

# Setup Java (optional)
cd java && mvn clean install && cd ..
```

---

## 📝 Commit Messages (Semantic Versioning)

We use **semantic commit messages** that automatically trigger version bumps and publishing:

### Commit Format
```
<type>: <description>
[optional body]
[optional footer]
```

### Types

| Type | Version Bump | Use When |
|------|-------------|----------|
| `feat:` | **Minor** (1.0.0 → 1.1.0) | New feature added |
| `fix:` | **Patch** (1.0.0 → 1.0.1) | Bug fix |
| `docs:` | No bump | Documentation changes |
| `style:` | No bump | Code style (formatting, missing semicolons) |
| `refactor:` | No bump | Code refactoring without new features |
| `perf:` | No bump | Performance improvements |
| `test:` | No bump | Adding or updating tests |
| `chore:` | No bump | Build, CI/CD, dependency updates |
| `BREAKING:` | **Major** (1.0.0 → 2.0.0) | Breaking API changes |

### Examples

```bash
# New feature (bumps minor version)
git commit -m "feat: add delta compression algorithm"

# Bug fix (bumps patch version)
git commit -m "fix: handle null values in encoder"

# Breaking change (bumps major version)
git commit -m "BREAKING: remove deprecated Reader API"

# Documentation (no version bump)
git commit -m "docs: update README with examples"
```

---

## 🔄 Automatic CI/CD Pipeline

When you push commits or create a pull request:

### On Pull Request
- ✅ Multi-language testing (Rust, Python, Node.js, Java)
- ✅ Code quality checks (linting, formatting)
- ✅ Security scanning (vulnerability detection)
- ✅ Test coverage analysis

### On Merge to Main
- 🔄 Auto-versioning detects commit type
- 📦 Automatic publishing to 7 registries:
  - PyPI (Python)
  - Maven Central (Java)
  - npm (JavaScript)
  - crates.io (Rust)
  - Docker Hub (Container image)
  - NuGet (C#)
  - RubyGems (Ruby)
- 📚 API documentation auto-generated to GitHub Pages
- 🏷️ GitHub Release created with changelog
- 🔒 Security scanning performed

---

## 🧪 Testing

### Run All Tests
```bash
# Rust tests
cargo test --lib

# Python tests
cd python && python -m pytest && cd ..

# JavaScript tests
cd nodejs && npm test && cd ..

# Java tests
cd java && mvn test && cd ..
```

### Run Specific Tests
```bash
# Rust specific test
cargo test --lib test_name

# Python specific test
cd python && python -m pytest tests/test_file.py::test_name && cd ..
```

---

## 📋 Pull Request Process

1. **Create a fork** and branch from `main`
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Make your changes** and commit with semantic messages
   ```bash
   git commit -m "feat: add new feature"
   ```

3. **Push to your fork**
   ```bash
   git push origin feature/your-feature-name
   ```

4. **Create a Pull Request** on GitHub
   - Describe what you changed and why
   - Reference any related issues
   - Ensure tests pass

5. **Address review feedback**
   - Push additional commits if requested
   - Don't force push once PR is open

6. **Merge**
   - Maintainers will merge once approved
   - Automatic publishing happens immediately

---

## 🐛 Reporting Bugs

Use GitHub Issues to report bugs:

1. **Check existing issues** - Search before creating a new one
2. **Provide details**:
   - Operating system and version
   - Rust/Python/Node.js version
   - Minimal code to reproduce
   - Expected vs actual behavior
   - Error messages and logs

---

## 💡 Feature Requests

Use GitHub Discussions for feature ideas:

1. Describe the use case
2. Explain why you need it
3. Provide examples of how it would be used
4. Link related issues if applicable

---

## 🏗️ Code Style

### Rust
```bash
# Format code
cargo fmt

# Check with clippy
cargo clippy -- -D warnings
```

### Python
```bash
# Format with black
pip install black
black python/

# Lint with pylint
pip install pylint
pylint python/kore_fileformat/
```

### JavaScript
```bash
# Format with prettier
cd nodejs && npm run format && cd ..

# Lint with eslint
cd nodejs && npm run lint && cd ..
```

---

## 📚 Documentation

- Update README.md for user-facing features
- Add docstrings/comments for complex logic
- Update CHANGELOG.md for notable changes
- API docs auto-generate from code comments

---

## ✅ Checklist Before Submitting

- [ ] Code follows project style guidelines
- [ ] All tests pass locally
- [ ] New tests added for new features
- [ ] Documentation updated
- [ ] Commit messages follow semantic format
- [ ] No breaking changes (or marked with BREAKING:)
- [ ] No large unnecessary files committed

---

## 📞 Questions?

- 💬 Ask in GitHub Discussions
- 🐛 Open an issue for bugs
- 📧 Email the maintainers
- 💻 Check existing documentation

---

## 📄 License

By contributing, you agree that your contributions will be licensed under the same MIT License as the project.

---

**Thank you for contributing to Kore! 🚀**
