# PHP Setup & Integration Guide for KORE v1.3.3

**Last Updated:** June 3, 2026  
**Status:** Production Ready  
**Version:** v1.0

---

## 📋 Table of Contents

1. [Prerequisites](#prerequisites)
2. [Installation](#installation)
3. [Verification](#verification)
4. [KORE Integration](#kore-integration)
5. [Common Tasks](#common-tasks)
6. [Troubleshooting](#troubleshooting)

---

## Prerequisites

| Requirement | Minimum | Recommended | Notes |
|-------------|---------|-------------|-------|
| PHP Version | 7.4+ | 8.2+ | Latest stable |
| Composer | 2.0+ | 2.5+ | Package manager |
| Web Server | Built-in | Apache/Nginx | Apache 2.4+ or Nginx 1.20+ |
| OS Support | Windows 10+ | Ubuntu 20.04+ | WSL2 for Windows |
| RAM | 512 MB | 1 GB | For development |
| Disk Space | 300 MB | 800 MB | PHP + extensions |

---

## Installation

### Step 1: Install PHP

**Windows (PHP.net):**
```powershell
# Using Chocolatey
choco install php

# Or download from https://windows.php.net/

# Verify installation
php --version
php -m | findstr json
```

**Linux (Ubuntu/Debian):**
```bash
# Update package list
sudo apt-get update

# Install PHP
sudo apt-get install -y php php-cli php-json php-curl

# Verify
php --version
php -m | grep json
```

**macOS:**
```bash
# Using Homebrew
brew install php

# Add to PATH (if needed)
echo 'export PATH="/usr/local/opt/php/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc

# Verify
php --version
```

### Step 2: Install Composer

```powershell
# Download and install Composer
# From https://getcomposer.org/download/

# Windows (via installer)
choco install composer

# Verify
composer --version
```

### Step 3: Setup KORE PHP Project

```powershell
# Create directory
mkdir kore-php-tools
cd kore-php-tools

# Initialize Composer project
composer init --name="kore/php-tools" --require="php:>=8.0"

# Or create composer.json
@"
{
    "name": "kore/php-tools",
    "description": "PHP integration tools for KORE v1.3.3",
    "version": "1.3.3",
    "require": {
        "php": ">=8.0"
    },
    "require-dev": {
        "phpunit/phpunit": "^10.0"
    },
    "autoload": {
        "psr-4": {
            "Kore\\": "src/"
        }
    }
}
"@ | Out-File -Encoding UTF8 composer.json

# Install dependencies
composer install
```

---

## Verification

### Quick Check

```powershell
# Check PHP version
php --version

# Check extensions
php -m

# Create test file
@"
<?php
echo "Hello from KORE PHP!\n";
echo "PHP version: " . phpversion() . "\n";
?>
"@ | Out-File -Encoding UTF8 test.php

# Run
php test.php

# Clean up
Remove-Item test.php
```

### Complete Environment

```powershell
# Show PHP info
php -i | findstr "PHP Version"

# List installed extensions
php -m

# Check loaded extensions
php -r "echo json_encode(get_loaded_extensions());"
```

---

## KORE Integration

### PHP with KORE

PHP is excellent for:
- Web applications and REST APIs
- Quick server-side processing
- Content management systems
- Data processing pipelines
- Integration with databases

### Create KORE PHP Library

**src/KoreProcessor.php:**
```php
<?php

namespace Kore;

class KoreProcessor
{
    const VERSION = '1.3.3';
    
    private $filePath;
    
    public function __construct(string $filePath)
    {
        $this->filePath = $filePath;
    }
    
    public function process(): array
    {
        return [
            'filename' => basename($this->filePath),
            'version' => self::VERSION,
            'timestamp' => time(),
            'status' => 'processed'
        ];
    }
    
    public function getMetadataJson(): string
    {
        return json_encode($this->process(), JSON_PRETTY_PRINT);
    }
    
    public static function getVersion(): string
    {
        return self::VERSION;
    }
}
```

**index.php:**
```php
<?php

require 'vendor/autoload.php';

use Kore\KoreProcessor;

echo "KORE PHP Integration v" . KoreProcessor::getVersion() . "\n";

$processor = new KoreProcessor('data.kore');
echo $processor->getMetadataJson();
?>
```

---

## Common Tasks

### Running PHP Programs

```powershell
# Run PHP script
php script.php

# Run with built-in server
php -S localhost:8000

# Run with specific configuration
php -c php.ini script.php

# Interactive shell
php -a
```

### PHP KORE Integration Examples

**Process KORE Files:**
```php
<?php

class KoreReader
{
    private $filePath;
    
    public function __construct(string $filePath)
    {
        $this->filePath = $filePath;
    }
    
    public function readMetadata(): ?array
    {
        if (file_exists($this->filePath)) {
            $content = file_get_contents($this->filePath);
            return json_decode($content, true);
        }
        return null;
    }
    
    public function validate(): bool
    {
        return file_exists($this->filePath) && is_readable($this->filePath);
    }
}

$reader = new KoreReader('data.kore');
echo "Valid: " . ($reader->validate() ? 'yes' : 'no') . "\n";
?>
```

**REST API with PHP:**
```php
<?php
// api.php

header('Content-Type: application/json');

$request = parse_url($_SERVER['REQUEST_URI'], PHP_URL_PATH);
$method = $_SERVER['REQUEST_METHOD'];

if ($request === '/api/kore/metadata' && $method === 'GET') {
    $processor = new KoreProcessor('data.kore');
    echo $processor->getMetadataJson();
    
} elseif ($request === '/api/kore/version' && $method === 'GET') {
    echo json_encode(['version' => KoreProcessor::getVersion()]);
    
} elseif ($request === '/api/kore/process' && $method === 'POST') {
    $data = json_decode(file_get_contents('php://input'), true);
    $processor = new KoreProcessor($data['filename'] ?? 'default.kore');
    echo $processor->getMetadataJson();
    
} else {
    http_response_code(404);
    echo json_encode(['error' => 'Not found']);
}
?>
```

---

## Troubleshooting

### Issue 1: "php is not recognized"

**Solution:**
```powershell
# Check PATH
$env:Path -split ';' | findstr php

# Add PHP to PATH
$phpPath = "C:\php"
[Environment]::SetEnvironmentVariable("PATH", "$env:Path;$phpPath", "User")

# Restart PowerShell
```

### Issue 2: "Composer command not found"

**Solution:**
```powershell
# Download Composer installer from getcomposer.org

# Or use Chocolatey
choco install composer

# Verify
composer --version
```

### Issue 3: "Required extension not found"

**Solution:**
```powershell
# List loaded extensions
php -m

# Enable extension in php.ini
# Uncomment: extension=json
# Or: extension=curl

# Check php.ini location
php -i | findstr "php.ini"

# Restart web server if using one
```

---

## Best Practices

✅ **DO:**
- Use namespaces for organization
- Follow PSR-12 coding standards
- Use type hints (strict_types=1)
- Use Composer for dependency management
- Write tests with PHPUnit
- Validate and sanitize input
- Use prepared statements for SQL
- Handle errors with try-catch

❌ **DON'T:**
- Use global variables
- Ignore security warnings
- Mix PHP versions
- Commit vendor directory
- Use deprecated functions
- Hardcode credentials
- Ignore CORS headers
- Use eval()

---

## Project Structure

```
kore-php-tools/
├── composer.json
├── composer.lock
├── README.md
├── src/
│   └── KoreProcessor.php
├── tests/
│   └── KoreProcessorTest.php
├── public/
│   └── index.php
├── vendor/          (generated)
└── .gitignore
```

---

## Quick Reference

```powershell
# PHP commands
php script.php                 # Run script
php --version                 # Check version
php -i                        # PHP info
php -r "code"                # Eval code
php -S localhost:8000         # Web server

# Composer commands
composer install              # Install dependencies
composer update              # Update packages
composer require package     # Add package
composer show               # List packages
composer remove package     # Remove package

# Testing
composer test               # Run tests (if configured)
php vendor/bin/phpunit     # Run PHPUnit
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial PHP setup guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Next:** Swift Setup & Integration Guide
