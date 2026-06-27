#!/usr/bin/env python3
"""
KORE v1.1.5 - Installation Verification Script

Quick test to verify KORE is installed and working correctly.
Run this after: pip install kore-fileformat

Usage:
  python test_kore_install.py
"""

import sys
import time

def test_python_installation():
    """Test Python KORE installation"""
    print("=" * 60)
    print("🐍 KORE PYTHON INSTALLATION TEST")
    print("=" * 60)
    print()
    
    # Test 1: Import
    print("Test 1: Importing kore_fileformat...")
    try:
        import kore_fileformat
        print(f"  ✅ SUCCESS - Version: {kore_fileformat.__version__}")
    except ImportError as e:
        print(f"  ❌ FAILED - {e}")
        return False
    print()
    
    # Test 2: Basic compression
    print("Test 2: Basic compression/decompression...")
    try:
        from kore_fileformat import compress, decompress
        
        test_data = b"hello world" * 100
        compressed = compress(test_data)
        decompressed = decompress(compressed)
        
        if decompressed == test_data:
            ratio = len(compressed) / len(test_data) * 100
            print(f"  ✅ SUCCESS")
            print(f"     Original: {len(test_data)} bytes")
            print(f"     Compressed: {len(compressed)} bytes")
            print(f"     Ratio: {ratio:.1f}%")
        else:
            print(f"  ❌ FAILED - Data mismatch after decompression")
            return False
    except Exception as e:
        print(f"  ❌ FAILED - {e}")
        return False
    print()
    
    # Test 3: Performance
    print("Test 3: Performance test (1MB data)...")
    try:
        from kore_fileformat import compress, decompress
        
        data = b"x" * (1024 * 1024)  # 1MB
        
        start = time.time()
        compressed = compress(data)
        compress_time = time.time() - start
        
        start = time.time()
        decompressed = decompress(compressed)
        decompress_time = time.time() - start
        
        if decompressed == data:
            compress_speed = len(data) / compress_time / 1e6
            decompress_speed = len(data) / decompress_time / 1e6
            ratio = len(compressed) / len(data) * 100
            
            print(f"  ✅ SUCCESS")
            print(f"     Compression: {compress_time*1000:.2f}ms ({compress_speed:.0f} MB/s)")
            print(f"     Decompression: {decompress_time*1000:.2f}ms ({decompress_speed:.0f} MB/s)")
            print(f"     Ratio: {ratio:.1f}%")
        else:
            print(f"  ❌ FAILED - Data mismatch")
            return False
    except Exception as e:
        print(f"  ❌ FAILED - {e}")
        return False
    print()
    
    # Test 4: Different data patterns
    print("Test 4: Testing different data patterns...")
    try:
        from kore_fileformat import compress, decompress
        
        patterns = {
            "Repetitive": b"A" * 10000,
            "Random": bytes(range(256)) * 50,
            "Mixed": (b"cat" + b"dog" + b"bird") * 1000,
        }
        
        for name, pattern in patterns.items():
            compressed = compress(pattern)
            decompressed = decompress(compressed)
            ratio = len(compressed) / len(pattern) * 100
            
            if decompressed == pattern:
                print(f"  ✅ {name}: {ratio:.1f}% compression")
            else:
                print(f"  ❌ {name}: Data mismatch")
                return False
    except Exception as e:
        print(f"  ❌ FAILED - {e}")
        return False
    print()
    
    return True

if __name__ == "__main__":
    try:
        success = test_python_installation()
        
        print("=" * 60)
        if success:
            print("✅ ALL TESTS PASSED - KORE IS READY TO USE!")
            print("=" * 60)
            sys.exit(0)
        else:
            print("❌ TESTS FAILED - KORE INSTALLATION INCOMPLETE")
            print("=" * 60)
            sys.exit(1)
    except Exception as e:
        print(f"❌ UNEXPECTED ERROR: {e}")
        sys.exit(1)
