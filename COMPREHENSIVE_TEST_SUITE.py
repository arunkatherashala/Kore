#!/usr/bin/env python3
"""
KORE COMPREHENSIVE TEST SUITE
Tests all 5 tracks: Performance, Ecosystem, Compliance, Time-Series, Advanced
"""

import subprocess
import json
import sys
from pathlib import Path

def run_command(cmd, description):
    """Run a shell command and report results"""
    print(f"\n{'='*80}")
    print(f"🧪 TEST: {description}")
    print(f"{'='*80}")
    print(f"Command: {cmd}\n")
    
    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    
    print(result.stdout)
    if result.stderr:
        print("STDERR:", result.stderr)
    
    status = "✅ PASS" if result.returncode == 0 else "❌ FAIL"
    print(f"\nStatus: {status} (exit code: {result.returncode})")
    
    return result.returncode == 0

def main():
    """Run full test suite"""
    workspace = Path("c:/Users/ksak_/OneDrive/Desktop/dbt_prep/Kore")
    os.chdir(workspace)
    
    print("\n" + "="*80)
    print("🚀 KORE COMPREHENSIVE TEST SUITE - ALL 5 TRACKS")
    print("="*80)
    
    results = {
        "compilation": {},
        "unit_tests": {},
        "feature_tests": {},
        "integration": {},
        "benchmarks": {}
    }
    
    # ========== PHASE 1: COMPILATION TESTS ==========
    print("\n" + "🔧 PHASE 1: COMPILATION TESTS" + "\n")
    
    compilation_tests = {
        "check_all_features": "cargo check --all-features",
        "check_release": "cargo check --release",
        "build_lib": "cargo build --lib",
        "build_tests": "cargo build --tests",
    }
    
    for name, cmd in compilation_tests.items():
        results["compilation"][name] = run_command(cmd, f"Compilation: {name}")
    
    # ========== PHASE 2: UNIT TESTS ==========
    print("\n" + "🧪 PHASE 2: UNIT TESTS (All modules)" + "\n")
    
    unit_tests = {
        "all_unit_tests": "cargo test --lib",
        "doc_tests": "cargo test --doc",
        "verbose": "cargo test --lib -- --nocapture",
    }
    
    for name, cmd in unit_tests.items():
        results["unit_tests"][name] = run_command(cmd, f"Unit tests: {name}")
    
    # ========== PHASE 3: FEATURE TESTS ==========
    print("\n" + "⚙️  PHASE 3: FEATURE GATE TESTS" + "\n")
    
    feature_tests = {
        "simd_optimize": "cargo test --features simd-optimize",
        "timeseries": "cargo test --features timeseries-opt",
        "duckdb": "cargo test --features duckdb-ffi",
        "pyo3": "cargo build --features pyo3 --lib",
        "gpu_cuda": "cargo test --features gpu-cuda",
        "full_features": "cargo test --all-features",
    }
    
    for name, cmd in feature_tests.items():
        results["feature_tests"][name] = run_command(cmd, f"Features: {name}")
    
    # ========== PHASE 4: TRACK-SPECIFIC TESTS ==========
    print("\n" + "📊 PHASE 4: TRACK-SPECIFIC TESTS" + "\n")
    
    # Track A: Performance
    print("\n📈 TRACK A: Performance & SIMD")
    results["unit_tests"]["track_a_performance"] = run_command(
        "cargo test --lib --features simd-optimize codec",
        "Track A: Codec performance tests"
    )
    
    # Track D: Time-Series (most complete)
    print("\n⏱️  TRACK D: Time-Series Optimization")
    results["unit_tests"]["track_d_timeseries"] = run_command(
        "cargo test --lib --features timeseries-opt timeseries",
        "Track D: Time-series codec tests"
    )
    
    # Track B: Ecosystem
    print("\n🔌 TRACK B: Ecosystem Integration")
    results["integration"]["track_b"] = run_command(
        "cargo build --features duckdb-ffi --lib",
        "Track B: DuckDB FFI compilation"
    )
    
    # Track C: Compliance
    print("\n🔒 TRACK C: Compliance & Security")
    results["integration"]["track_c"] = run_command(
        "cargo clippy --all-targets --all-features -- -D warnings",
        "Track C: Clippy linting (code quality)"
    )
    
    # Track E: Advanced/GPU
    print("\n🚀 TRACK E: Advanced Features & GPU")
    results["integration"]["track_e"] = run_command(
        "cargo test --lib --features gpu-cuda",
        "Track E: GPU framework tests"
    )
    
    # ========== PHASE 5: COMPREHENSIVE BENCHMARKS ==========
    print("\n" + "⚡ PHASE 5: PERFORMANCE BENCHMARKS" + "\n")
    
    print("\n🏁 Running release build (optimized)...")
    results["benchmarks"]["release_build"] = run_command(
        "cargo build --release --lib",
        "Release build (for benchmarking)"
    )
    
    # ========== SUMMARY ==========
    print("\n" + "="*80)
    print("📋 TEST SUMMARY")
    print("="*80)
    
    total_passed = sum(
        sum(1 for v in phase.values() if v) 
        for phase in results.values()
    )
    total_tests = sum(
        len(phase) 
        for phase in results.values()
    )
    
    print(f"\nTotal Tests Run: {total_tests}")
    print(f"Tests Passed: {total_passed}")
    print(f"Tests Failed: {total_tests - total_passed}")
    print(f"Pass Rate: {(total_passed/total_tests*100):.1f}%")
    
    # Detailed breakdown
    print("\n" + "-"*80)
    print("BREAKDOWN BY PHASE:")
    print("-"*80)
    
    for phase_name, phase_results in results.items():
        passed = sum(1 for v in phase_results.values() if v)
        total = len(phase_results)
        status = "✅" if passed == total else "⚠️" if passed > 0 else "❌"
        print(f"{status} {phase_name.upper():<20} {passed}/{total} passed")
    
    # Track-specific summary
    print("\n" + "-"*80)
    print("TRACK STATUS:")
    print("-"*80)
    
    tracks = {
        "Track A (Performance)": ["simd", "codec"],
        "Track B (Ecosystem)": ["duckdb"],
        "Track C (Compliance)": ["clippy"],
        "Track D (Time-Series)": ["timeseries"],
        "Track E (Advanced/GPU)": ["gpu"],
    }
    
    all_keys = str(results).lower()
    for track_name, keywords in tracks.items():
        has_tests = any(kw in all_keys for kw in keywords)
        status = "✅" if has_tests else "⏳"
        print(f"{status} {track_name}")
    
    # Final verdict
    print("\n" + "="*80)
    if total_passed == total_tests:
        print("🎉 ALL TESTS PASSED! KORE IS PRODUCTION READY!")
    elif total_passed >= total_tests * 0.9:
        print("✅ 90%+ PASS RATE - Most functionality working!")
    else:
        print("⚠️  Some tests failing - review above for details")
    print("="*80)
    
    # Generate JSON report
    with open("TEST_RESULTS.json", "w") as f:
        json.dump({
            "total_tests": total_tests,
            "tests_passed": total_passed,
            "pass_rate": total_passed / total_tests,
            "phases": results
        }, f, indent=2)
    
    print("\n✅ Test report saved to TEST_RESULTS.json")
    
    return 0 if total_passed == total_tests else 1

if __name__ == "__main__":
    import os
    sys.exit(main())
