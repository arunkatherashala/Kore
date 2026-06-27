# JavaScript/Node.js Setup Guide

## Prerequisites

- Node.js 14.0.0 or higher
- npm or yarn
- (For building from source) Rust 1.70+ and Cargo

## Installation Methods

### Method 1: From npm (Easiest)

```bash
npm install kore-fileformat
```

That's it! You're ready to use KORE.

### Method 2: From Source

If you want to build from source:

```bash
# Clone the repository
git clone https://github.com/arunkatherashala/Kore.git
cd Kore/nodejs

# Install dependencies
npm install

# Build the native module
npm run build

# Run tests
npm test
```

## Verification

Create a test file to verify the installation:

```javascript
// test-kore.js
const { Kore } = require('kore-fileformat');

async function test() {
  console.log('✅ KORE loaded successfully!');
  
  // Create test data
  const schema = {
    fields: [
      { name: 'id', type: 'int64' },
      { name: 'message', type: 'string' }
    ]
  };
  
  const data = [
    { id: 1, message: 'Hello' },
    { id: 2, message: 'World' }
  ];
  
  // Write test
  await Kore.write('test.kore', schema, data);
  console.log('✅ Write test passed');
  
  // Read test
  const result = await Kore.read('test.kore');
  console.log('✅ Read test passed');
  console.log('Data:', result);
  
  // Stats test
  const stats = await Kore.getStats('test.kore');
  console.log('✅ Stats test passed');
  console.log(`File size: ${stats.fileSize} bytes`);
  console.log(`Compression: ${(stats.compressionRatio * 100).toFixed(1)}%`);
}

test().catch(console.error);
```

Run the test:

```bash
node test-kore.js
```

## Common Issues & Solutions

### Issue: "Cannot find module 'kore-fileformat'"

**Solution:**
```bash
# Clear npm cache
npm cache clean --force

# Reinstall
npm install kore-fileformat
```

### Issue: "Native module not found"

**Solution:**
```bash
# If building from source
cd nodejs
npm install
npm run build
```

### Issue: "Port/Binding mismatch on Windows"

**Solution:**
```bash
npm run build:debug
```

### Issue: Module loads but function calls fail

**Solution:**
Ensure you're on a supported platform:
- macOS 10.13+
- Ubuntu 18.04+
- Windows 10+
- CentOS 7+

## System Requirements

| OS | Version | Architecture |
|---|---------|--------------|
| macOS | 10.13+ | Intel, Apple Silicon |
| Linux | 18.04+ | x86_64, ARM64 |
| Windows | 10+ | x86_64 |

## Performance Tips

### 1. Batch Operations

```javascript
// Good: Write once
const largeDataset = [...]; // 100k rows
await Kore.write('data.kore', schema, largeDataset);

// Avoid: Multiple small writes
for (let i = 0; i < 100000; i++) {
  await Kore.write('data.kore', schema, [row]);
}
```

### 2. Read Only What You Need

```javascript
// Good: Read specific column
const ids = await Kore.readColumn('data.kore', 'id');

// Avoid: Read all data if you only need one column
const allData = await Kore.read('data.kore');
const ids = allData.map(r => r.id);
```

### 3. Use Class for Multiple Operations

```javascript
// Good: Reuse instance
const kore = new Kore();
await kore.load('data.kore');
const count = await kore.getRowCount();
const names = await kore.readColumn('name');

// Avoid: Multiple loads
await Kore.read('data.kore');
const stats = await Kore.getStats('data.kore');
```

## API Quick Reference

```javascript
const { Kore } = require('kore-fileformat');

// Functional API
await Kore.write(filename, schema, data);
const data = await Kore.read(filename);
const column = await Kore.readColumn(filename, 'columnName');
const stats = await Kore.getStats(filename);

// Object-Oriented API
const kore = new Kore();
await kore.load(filename);
await kore.save(filename);
const count = await kore.getRowCount();
const cols = await kore.getColumnNames();
const data = await kore.readAll();
```

## Next Steps

1. ✅ Installation complete
2. 📖 Read the full [README.md](README.md)
3. 💡 Check out the [examples/](examples/) directory
4. 🚀 Start using KORE in your project!

## Getting Help

- 📚 GitHub Discussions: https://github.com/arunkatherashala/Kore/discussions
- 🐛 Report Issues: https://github.com/arunkatherashala/Kore/issues
- 💬 Chat with us: https://github.com/arunkatherashala/Kore/discussions

---

**Ready to get 50x faster performance?** Let's go! 🚀
