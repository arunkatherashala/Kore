# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 1.7.x   | Yes       |
| 1.6.x   | Security fixes only |
| < 1.6   | No        |

## Reporting a Vulnerability

If you discover a security vulnerability in KORE FileFormat, please report it responsibly.

**Do NOT open a public issue.**

Email: **arunkatherashala@gmail.com**

Include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

You will receive an acknowledgment within 48 hours. A fix will be prioritized based on severity.

## Security Features

KORE FileFormat includes:
- **CRC32 integrity checks** on all data blocks
- **Magic byte validation** to prevent file corruption
- **Version compatibility checks** for safe upgrades
- **No arbitrary code execution** — data-only format
