# JavaScript/TypeScript Setup & Integration Guide for KORE v1.3.3

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
| Node.js | 14.0+ | 18.0+ LTS | JavaScript runtime |
| npm | 6.0+ | 9.0+ | Package manager |
| TypeScript | 4.5+ | 5.0+ | Optional but recommended |
| OS Support | Windows 10+ | Windows 10, 11 | Also supports Linux/macOS |
| RAM | 1 GB | 2 GB | For development |
| Disk Space | 500 MB | 2 GB | Node modules + tools |

---

## Installation

### Step 1: Install Node.js & npm

**Official Download:**
```powershell
# From: https://nodejs.org/
# Download LTS version (recommended for production)

# Or use Windows Package Manager
winget install OpenJS.NodeJS.LTS

# Or Chocolatey
choco install nodejs
```

**Verify Installation:**
```powershell
# Check Node.js version
node --version

# Check npm version
npm --version

# Expected output:
# v18.16.0 (or newer)
# 9.6.7 (or newer)
```

### Step 2: Install TypeScript (Optional but Recommended)

```powershell
# Install TypeScript globally
npm install -g typescript

# Verify
tsc --version

# Expected: Version 5.0.0 (or newer)
```

### Step 3: Install Development Tools

```powershell
# Install globally useful tools
npm install -g eslint prettier ts-node

# Verify
eslint --version
prettier --version
ts-node --version
```

---

## Verification

### Quick Check
```powershell
# Test Node.js
node -v
npm -v

# Test JavaScript
node -e "console.log('Node.js is working!')"

# Test npm
npm list -g --depth=0

# Test TypeScript
tsc --version
```

### Complete Environment Check

```powershell
# Create test project
mkdir test-node
cd test-node

# Initialize npm project
npm init -y

# Create test file
@"
console.log('Hello from KORE Node.js!');
console.log('Node version:', process.version);
console.log('npm version:', npm.version);
"@ | Out-File test.js

# Run test
node test.js

# Clean up
cd ..
Remove-Item test-node -Recurse
```

---

## KORE Integration

### Node.js with KORE

Node.js can integrate with KORE for:
- Data analysis tools
- Web dashboards
- REST APIs for KORE data
- Testing frameworks
- Build tools
- DevOps automation

### Setup KORE JavaScript/TypeScript Project

**Step 1: Create Project Structure**

```powershell
# Navigate to KORE directory
cd "c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore"

# Create JavaScript project
mkdir js-tools
cd js-tools

# Initialize npm project
npm init -y

# Install core dependencies
npm install axios express dotenv
npm install --save-dev typescript ts-node @types/node eslint prettier
```

**Step 2: Configure TypeScript**

```powershell
# Generate tsconfig.json
tsc --init

# Edit tsconfig.json for production:
# Uncomment and set:
# "target": "ES2020"
# "module": "commonjs"
# "strict": true
# "esModuleInterop": true
# "skipLibCheck": true
# "forceConsistentCasingInFileNames": true
```

**Step 3: Create ESLint & Prettier Config**

Create `.eslintrc.json`:
```json
{
  "env": {
    "node": true,
    "es2020": true
  },
  "extends": "eslint:recommended",
  "parserOptions": {
    "ecmaVersion": "latest",
    "sourceType": "module"
  },
  "rules": {
    "indent": ["error", 2],
    "quotes": ["error", "single"]
  }
}
```

Create `.prettierrc`:
```json
{
  "semi": true,
  "singleQuote": true,
  "trailingComma": "es5",
  "printWidth": 100
}
```

---

## Common Tasks

### Running JavaScript/TypeScript

```powershell
# Run JavaScript file
node script.js

# Run TypeScript file directly
ts-node script.ts

# Run npm script
npm run build
npm start
npm test
```

### Installing Dependencies

```powershell
# Install package
npm install package-name

# Install development dependency
npm install --save-dev package-name

# Install globally
npm install -g package-name

# Install from package.json
npm install

# Update packages
npm update

# Check outdated packages
npm outdated
```

### KORE Data Processing Example

**src/kore-processor.ts:**
```typescript
import fs from 'fs';

interface KoreMetadata {
  version: string;
  filename: string;
  timestamp: string;
}

export class KoreProcessor {
  constructor(private filePath: string) {}

  async processKoreFile(): Promise<KoreMetadata> {
    try {
      const stats = fs.statSync(this.filePath);
      return {
        version: '1.3.3',
        filename: this.filePath,
        timestamp: new Date().toISOString(),
      };
    } catch (error) {
      console.error('Error processing KORE file:', error);
      throw error;
    }
  }
}
```

### Creating Web API for KORE

**src/server.ts:**
```typescript
import express from 'express';
import dotenv from 'dotenv';
import { KoreProcessor } from './kore-processor';

dotenv.config();

const app = express();
const PORT = process.env.PORT || 3000;

app.use(express.json());

// KORE metadata endpoint
app.get('/api/kore/metadata/:filename', async (req, res) => {
  try {
    const processor = new KoreProcessor(req.params.filename);
    const metadata = await processor.processKoreFile();
    res.json(metadata);
  } catch (error) {
    res.status(500).json({ error: 'Failed to process KORE file' });
  }
});

app.listen(PORT, () => {
  console.log(`KORE API server running on port ${PORT}`);
});
```

### Testing with Jest

```powershell
# Install Jest
npm install --save-dev jest @types/jest ts-jest

# Create jest.config.js
npx jest --init

# Run tests
npm test

# Run with coverage
npm test -- --coverage
```

---

## Troubleshooting

### Issue 1: "node is not recognized"

**Solution:**
```powershell
# Verify Node.js is in PATH
where node

# Restart PowerShell or system
# Reinstall Node.js with path option checked

# Or add manually to PATH
$env:Path += ";C:\Program Files\nodejs"
```

### Issue 2: "npm ERR! 404 Not Found"

**Solution:**
```powershell
# Clear npm cache
npm cache clean --force

# Try installing again
npm install package-name

# Or use npm registry mirror
npm config set registry https://registry.npmmirror.com

# Reset to default
npm config set registry https://registry.npmjs.org/
```

### Issue 3: "EACCES: permission denied"

**Solution:**
```powershell
# On Windows, usually not an issue
# On Mac/Linux:
sudo chown -R $(whoami) ~/.npm

# Or use nvm (Node Version Manager) instead
# https://github.com/nvm-sh/nvm
```

### Issue 4: "Module not found"

**Solution:**
```powershell
# Clean and reinstall
rm -r node_modules
rm package-lock.json
npm install

# Or update TypeScript types
npm install --save-dev @types/module-name
```

### Issue 5: TypeScript compilation errors

**Solution:**
```powershell
# Check tsconfig.json
cat tsconfig.json

# Compile with verbose output
tsc --listFiles

# Or run with ts-node
ts-node script.ts
```

---

## Best Practices

✅ **DO:**
- Use TypeScript for large projects
- Pin exact versions in package.json for production
- Use .env for configuration
- Write unit tests with Jest/Mocha
- Use ESLint and Prettier
- Keep node_modules in .gitignore
- Use async/await for async operations
- Document function signatures with JSDoc

❌ **DON'T:**
- Use `npm install` with `-g` for project dependencies
- Commit node_modules to git
- Ignore TypeScript compilation errors
- Use `var` (use `const` or `let`)
- Make synchronous I/O calls in production
- Leave debugging console.log() in production
- Use `any` type excessively in TypeScript

---

## Project Structure

```
kore-js-tools/
├── package.json
├── tsconfig.json
├── .eslintrc.json
├── .prettierrc
├── .env.example
├── .gitignore
├── src/
│   ├── index.ts
│   ├── kore-processor.ts
│   └── server.ts
├── tests/
│   ├── kore-processor.test.ts
│   └── server.test.ts
├── dist/                    (compiled JavaScript)
└── node_modules/            (should be in .gitignore)
```

---

## npm Scripts in package.json

```json
{
  "scripts": {
    "build": "tsc",
    "start": "node dist/index.js",
    "dev": "ts-node src/index.ts",
    "test": "jest",
    "test:watch": "jest --watch",
    "lint": "eslint src/**/*.ts",
    "format": "prettier --write src/**/*.ts",
    "clean": "rm -rf dist"
  }
}
```

---

## Quick Reference

```powershell
# Node.js and npm
node --version                 # Check Node version
npm --version                  # Check npm version
npm list                        # List dependencies
npm list -g                     # List global packages

# Project setup
npm init                        # Initialize project
npm init -y                     # Initialize with defaults
npm install                     # Install dependencies
npm install package-name        # Install package
npm uninstall package-name      # Remove package

# Running code
node script.js                  # Run JavaScript
ts-node script.ts              # Run TypeScript
npm start                       # Run start script
npm run script-name            # Run custom script

# Development
npm test                        # Run tests
npm run build                   # Build project
npm run lint                    # Lint code
npm run format                  # Format code

# Maintenance
npm update                      # Update packages
npm outdated                    # Show outdated packages
npm audit                       # Check security issues
npm audit fix                   # Fix vulnerabilities
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial setup guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Next:** Go Setup & Integration Guide (coming next)
