#!/usr/bin/env node

/**
 * KORE v1.1.5 - npm Installation Verification Script
 * 
 * Quick test to verify KORE is installed and working correctly in Node.js
 * Run this after: npm install kore-fileformat
 * 
 * Usage:
 *   node test_kore_install.js
 */

const assert = require('assert');

async function test_npm_installation() {
    console.log('='.repeat(60));
    console.log('📦 KORE NPM INSTALLATION TEST');
    console.log('='.repeat(60));
    console.log();
    
    // Test 1: Import
    console.log('Test 1: Requiring kore-fileformat...');
    try {
        const kore = require('kore-fileformat');
        console.log('  ✅ SUCCESS - Module loaded');
    } catch (err) {
        console.log(`  ❌ FAILED - ${err.message}`);
        return false;
    }
    console.log();
    
    // Test 2: Basic compression
    console.log('Test 2: Basic compression/decompression...');
    try {
        const kore = require('kore-fileformat');
        
        const testData = Buffer.from('hello world'.repeat(100));
        const compressed = kore.compress(testData);
        const decompressed = kore.decompress(compressed);
        
        if (Buffer.compare(decompressed, testData) === 0) {
            const ratio = (compressed.length / testData.length * 100).toFixed(1);
            console.log('  ✅ SUCCESS');
            console.log(`     Original: ${testData.length} bytes`);
            console.log(`     Compressed: ${compressed.length} bytes`);
            console.log(`     Ratio: ${ratio}%`);
        } else {
            console.log('  ❌ FAILED - Data mismatch after decompression');
            return false;
        }
    } catch (err) {
        console.log(`  ❌ FAILED - ${err.message}`);
        return false;
    }
    console.log();
    
    // Test 3: Performance (100KB data)
    console.log('Test 3: Performance test (100KB data)...');
    try {
        const kore = require('kore-fileformat');
        
        const data = Buffer.alloc(100 * 1024, 'x');
        
        const t0 = process.hrtime.bigint();
        const compressed = kore.compress(data);
        const t1 = process.hrtime.bigint();
        const decompressed = kore.decompress(compressed);
        const t2 = process.hrtime.bigint();
        
        const compressTime = Number(t1 - t0) / 1e6; // ms
        const decompressTime = Number(t2 - t1) / 1e6; // ms
        
        if (Buffer.compare(decompressed, data) === 0) {
            const compressSpeed = (data.length / (compressTime / 1000) / 1e6).toFixed(0);
            const decompressSpeed = (data.length / (decompressTime / 1000) / 1e6).toFixed(0);
            const ratio = (compressed.length / data.length * 100).toFixed(1);
            
            console.log('  ✅ SUCCESS');
            console.log(`     Compression: ${compressTime.toFixed(2)}ms (${compressSpeed} MB/s)`);
            console.log(`     Decompression: ${decompressTime.toFixed(2)}ms (${decompressSpeed} MB/s)`);
            console.log(`     Ratio: ${ratio}%`);
        } else {
            console.log('  ❌ FAILED - Data mismatch');
            return false;
        }
    } catch (err) {
        console.log(`  ❌ FAILED - ${err.message}`);
        return false;
    }
    console.log();
    
    // Test 4: Different data patterns
    console.log('Test 4: Testing different data patterns...');
    try {
        const kore = require('kore-fileformat');
        
        const patterns = {
            'Repetitive': Buffer.alloc(10000, 'A'),
            'Random': Buffer.from(Array.from({length: 5000}, (_, i) => i % 256)),
            'Mixed': Buffer.from('catdogbird'.repeat(1000)),
        };
        
        for (const [name, pattern] of Object.entries(patterns)) {
            const compressed = kore.compress(pattern);
            const decompressed = kore.decompress(compressed);
            const ratio = (compressed.length / pattern.length * 100).toFixed(1);
            
            if (Buffer.compare(decompressed, pattern) === 0) {
                console.log(`  ✅ ${name}: ${ratio}% compression`);
            } else {
                console.log(`  ❌ ${name}: Data mismatch`);
                return false;
            }
        }
    } catch (err) {
        console.log(`  ❌ FAILED - ${err.message}`);
        return false;
    }
    console.log();
    
    return true;
}

// Run tests
test_npm_installation().then(success => {
    console.log('='.repeat(60));
    if (success) {
        console.log('✅ ALL TESTS PASSED - KORE IS READY TO USE!');
        console.log('='.repeat(60));
        process.exit(0);
    } else {
        console.log('❌ TESTS FAILED - KORE INSTALLATION INCOMPLETE');
        console.log('='.repeat(60));
        process.exit(1);
    }
}).catch(err => {
    console.error(`❌ UNEXPECTED ERROR: ${err.message}`);
    process.exit(1);
});
