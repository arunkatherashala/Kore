"""Check that every package file carries the same version as pyproject.toml.

The weekly release bumps the version in all language files; a file whose version had drifted
was never updated by a plain search-and-replace of the old number (for example
kore_fileformat.__version__ stayed at 1.7.30 while PyPI shipped 1.8.21). This check fails CI
when any file disagrees, so a drift is caught before a release.

Usage (from the repo root): python -I scripts/check_versions.py [--expect X.Y.Z]
Exit status 0 when all agree, 1 otherwise. Stdlib only.
"""
import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# file -> list of (description, regex with one group = the version, expected suffix)
CHECKS = {
    'pyproject.toml': [('version', r'(?m)^version\s*=\s*"([^"]+)"', '')],
    'kore-python/kore_fileformat.py': [('__version__', r'(?m)^__version__\s*=\s*"([^"]+)"', '')],
    'kore-node/package.json': [('"version"', r'"version"\s*:\s*"([^"]+)"', '')],
    'kore-php/composer.json': [('"version"', r'"version"\s*:\s*"([^"]+)"', '')],
    'kore-ruby/kore_fileformat.gemspec': [('spec.version', r'spec\.version\s*=\s*"([^"]+)"', '')],
    'kore-go/go.mod': [('bindings comment', r'Go bindings v([0-9][^\s]*)', '')],
    'maven/pom.xml': [('project <version>', r'<version>([^<]+)</version>', '')],
    'csharp/Kore.FileFormat/Kore.FileFormat.csproj': [
        ('<Version>', r'<Version>([^<]+)</Version>', ''),
        ('<PackageVersion>', r'<PackageVersion>([^<]+)</PackageVersion>', ''),
        ('<AssemblyVersion>', r'<AssemblyVersion>([^<]+)</AssemblyVersion>', '.0'),
        ('<FileVersion>', r'<FileVersion>([^<]+)</FileVersion>', '.0'),
    ],
    'kore-standalone/Cargo.toml': [('version', r'(?m)^version\s*=\s*"([^"]+)"', '')],
}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--expect', help='required version (default: the one in pyproject.toml)')
    a = ap.parse_args(argv)
    text = (REPO / 'pyproject.toml').read_text(encoding='utf-8')
    want = a.expect or re.search(CHECKS['pyproject.toml'][0][1], text).group(1)
    problems = []
    for rel, checks in CHECKS.items():
        path = REPO / rel
        if not path.exists():
            problems.append(f'{rel}: missing')
            continue
        body = path.read_text(encoding='utf-8', errors='replace')
        for what, rx, suffix in checks:
            m = re.search(rx, body)  # first match: the package's own version
            got = m.group(1) if m else None
            if got != want + suffix:
                problems.append(f'{rel} {what}: {got!r}, expected {want + suffix!r}')
            else:
                print(f'ok   {rel} {what} = {got}')
    for p in problems:
        print(f'FAIL {p}')
    print(f'version {want}: ' + ('all files agree' if not problems else f'{len(problems)} mismatch(es)'))
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
