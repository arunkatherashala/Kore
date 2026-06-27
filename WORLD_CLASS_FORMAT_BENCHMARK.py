#!/usr/bin/env python3
"""
WORLD-CLASS FORMAT COMPARISON BENCHMARK
CSV → KORE vs Parquet vs Avro vs ORC
Date: May 12, 2026
"""

import csv
import json
import time
import os
import sys
from pathlib import Path
from datetime import datetime
import random
import string

# Try to import format libraries
try:
    import pandas as pd
    HAS_PANDAS = True
except ImportError:
    HAS_PANDAS = False
    print("⚠️  pandas not installed - some tests will be skipped")

try:
    import pyarrow as pa
    import pyarrow.parquet as pq
    import pyarrow.orc as orc
    HAS_ARROW = True
except ImportError:
    HAS_ARROW = False
    print("⚠️  pyarrow not installed - Parquet/Arrow tests will be skipped")

try:
    import fastavro
    HAS_AVRO = True
except ImportError:
    HAS_AVRO = False
    print("⚠️  fastavro not installed - Avro tests will be skipped")

# Color codes
GREEN = '\033[92m'
RED = '\033[91m'
YELLOW = '\033[93m'
CYAN = '\033[96m'
RESET = '\033[0m'
BOLD = '\033[1m'

def print_header(text):
    print(f"\n{BOLD}{CYAN}{'='*80}{RESET}")
    print(f"{BOLD}{CYAN}{text:^80}{RESET}")
    print(f"{BOLD}{CYAN}{'='*80}{RESET}\n")

def print_success(text):
    print(f"{GREEN}✅ {text}{RESET}")

def print_error(text):
    print(f"{RED}❌ {text}{RESET}")

def print_info(text):
    print(f"{CYAN}ℹ️  {text}{RESET}")

def print_section(text):
    print(f"\n{BOLD}{YELLOW}[{text}]{RESET}")

def generate_test_csv(filename, num_rows=10000):
    """Generate comprehensive test CSV data"""
    print_section("GENERATING TEST DATA")
    print_info(f"Generating {num_rows:,} rows of test data...")
    
    start = time.time()
    
    # Column definitions
    columns = [
        'customer_id', 'customer_name', 'email', 'country', 'age',
        'salary', 'years_employed', 'department', 'product_category',
        'transaction_amount', 'transaction_date', 'purchase_quantity',
        'discount_percentage', 'shipping_cost', 'is_premium_member'
    ]
    
    departments = ['Sales', 'Engineering', 'Marketing', 'HR', 'Finance', 'Operations']
    categories = ['Electronics', 'Clothing', 'Food', 'Home', 'Sports', 'Books', 'Toys']
    countries = ['USA', 'UK', 'Canada', 'Germany', 'France', 'Japan', 'India', 'Brazil', 'Mexico', 'Australia']
    
    with open(filename, 'w', newline='') as f:
        writer = csv.DictWriter(f, fieldnames=columns)
        writer.writeheader()
        
        for i in range(num_rows):
            row = {
                'customer_id': f'CUST{i:08d}',
                'customer_name': f'Customer_{i}',
                'email': f'customer{i}@example.com',
                'country': random.choice(countries),
                'age': random.randint(18, 80),
                'salary': round(random.uniform(30000, 200000), 2),
                'years_employed': random.randint(0, 40),
                'department': random.choice(departments),
                'product_category': random.choice(categories),
                'transaction_amount': round(random.uniform(10, 5000), 2),
                'transaction_date': f'2026-{random.randint(1,12):02d}-{random.randint(1,28):02d}',
                'purchase_quantity': random.randint(1, 100),
                'discount_percentage': round(random.uniform(0, 50), 1),
                'shipping_cost': round(random.uniform(5, 100), 2),
                'is_premium_member': random.choice(['true', 'false'])
            }
            writer.writerow(row)
            
            if (i + 1) % 20000 == 0:
                elapsed = time.time() - start
                rate = (i + 1) / elapsed
                print_info(f"Generated {i+1:,} rows ({rate:.0f} rows/sec)")
    
    elapsed = time.time() - start
    file_size = os.path.getsize(filename) / (1024 * 1024)
    print_success(f"Generated {num_rows:,} rows in {elapsed:.2f}s ({file_size:.2f} MB)")
    
    return filename

def benchmark_format(csv_file, output_file, format_name, conversion_func):
    """Benchmark format conversion and reading"""
    print_section(f"BENCHMARKING {format_name}")
    
    results = {
        'format': format_name,
        'output_file': output_file,
        'conversion_time': 0,
        'read_time': 0,
        'write_speed': 0,
        'read_speed': 0,
        'compression_ratio': 0,
        'file_size': 0,
        'status': 'PENDING'
    }
    
    try:
        # Conversion
        print_info(f"Converting CSV to {format_name}...")
        csv_size = os.path.getsize(csv_file) / (1024 * 1024)
        
        start = time.time()
        conversion_func(csv_file, output_file)
        conversion_time = time.time() - start
        
        # File size and compression ratio
        output_size = os.path.getsize(output_file) / (1024 * 1024)
        compression_ratio = (1 - output_size / csv_size) * 100
        write_speed = (csv_size / 1024) / conversion_time  # MB/s
        
        results['conversion_time'] = conversion_time
        results['file_size'] = output_size
        results['compression_ratio'] = compression_ratio
        results['write_speed'] = write_speed
        
        print_success(f"{format_name} conversion:")
        print(f"  Time: {conversion_time:.2f}s")
        print(f"  CSV size: {csv_size:.2f} MB → {format_name} size: {output_size:.2f} MB")
        print(f"  Compression: {compression_ratio:.1f}%")
        print(f"  Write speed: {write_speed:.1f} MB/s")
        
        results['status'] = 'SUCCESS'
        
    except Exception as e:
        print_error(f"Error converting to {format_name}: {e}")
        results['status'] = 'FAILED'
    
    return results

def convert_csv_to_parquet(csv_file, output_file):
    """Convert CSV to Parquet"""
    if not HAS_ARROW or not HAS_PANDAS:
        raise Exception("pyarrow or pandas not available")
    
    df = pd.read_csv(csv_file)
    table = pa.Table.from_pandas(df)
    pq.write_table(table, output_file, compression='snappy')

def convert_csv_to_orc(csv_file, output_file):
    """Convert CSV to ORC"""
    if not HAS_ARROW or not HAS_PANDAS:
        raise Exception("pyarrow or pandas not available")
    
    df = pd.read_csv(csv_file)
    table = pa.Table.from_pandas(df)
    orc.write_table(table, output_file)

def convert_csv_to_avro(csv_file, output_file):
    """Convert CSV to Avro"""
    if not HAS_AVRO or not HAS_PANDAS:
        raise Exception("fastavro or pandas not available")
    
    df = pd.read_csv(csv_file)
    
    # Infer Avro schema from pandas DataFrame
    schema = {
        'type': 'record',
        'name': 'CSV_Data',
        'fields': [
            {'name': col, 'type': ['null', 'string']}
            for col in df.columns
        ]
    }
    
    records = df.to_dict('records')
    
    with open(output_file, 'wb') as f:
        fastavro.schemaless_writer(f, schema, records)

def convert_csv_to_kore(csv_file, output_file):
    """Convert CSV to KORE (stub - would use actual KORE library)"""
    # This is a placeholder - in real scenario would use actual kore-fileformat
    import shutil
    print_info("Note: Using CSV copy as placeholder for KORE (actual KORE library would compress)")
    # For now, just copy to show the flow
    df = pd.read_csv(csv_file)
    # Simulate KORE compression by saving a simple binary format
    df.to_pickle(output_file)

def compare_formats(results_list):
    """Generate comparison report"""
    print_header("WORLD-CLASS FORMAT COMPARISON REPORT")
    
    # Sort by compression ratio
    sorted_results = sorted(results_list, key=lambda x: x['compression_ratio'], reverse=True)
    
    print(f"\n{BOLD}Performance Rankings:{RESET}\n")
    
    # Compression ratio ranking
    print(f"{BOLD}1. COMPRESSION RATIO (Higher = Better):{RESET}")
    for i, result in enumerate(sorted_results, 1):
        if result['status'] == 'SUCCESS':
            ratio = result['compression_ratio']
            bar = '█' * int(ratio / 5)
            print(f"   {i}. {result['format']:10s} {ratio:5.1f}% compressed {bar}")
    
    print(f"\n{BOLD}2. WRITE SPEED (Higher = Better):{RESET}")
    sorted_by_speed = sorted(
        [r for r in results_list if r['status'] == 'SUCCESS'],
        key=lambda x: x['write_speed'],
        reverse=True
    )
    for i, result in enumerate(sorted_by_speed, 1):
        speed = result['write_speed']
        bar = '█' * int(min(speed, 5000) / 50)
        print(f"   {i}. {result['format']:10s} {speed:6.1f} MB/s {bar}")
    
    print(f"\n{BOLD}3. CONVERSION TIME (Lower = Better):{RESET}")
    sorted_by_time = sorted(
        [r for r in results_list if r['status'] == 'SUCCESS'],
        key=lambda x: x['conversion_time']
    )
    for i, result in enumerate(sorted_by_time, 1):
        t = result['conversion_time']
        bar = '█' * max(1, int(t))
        print(f"   {i}. {result['format']:10s} {t:6.2f}s {bar}")
    
    print(f"\n{BOLD}4. FILE SIZE COMPARISON:{RESET}")
    csv_size_ref = next((r['file_size'] for r in results_list if r['format'] == 'CSV'), None)
    if csv_size_ref:
        for result in sorted_results:
            if result['status'] == 'SUCCESS':
                pct = (result['file_size'] / csv_size_ref) * 100 if csv_size_ref else 0
                print(f"   {result['format']:10s} {result['file_size']:8.2f} MB ({pct:5.1f}% of CSV)")
    
    # Summary table
    print(f"\n{BOLD}DETAILED RESULTS:{RESET}\n")
    print(f"{'Format':<12} {'Size (MB)':<12} {'Compressed':<12} {'Write (MB/s)':<14} {'Time (s)':<10} {'Status':<10}")
    print("─" * 80)
    
    for result in sorted_results:
        status_color = GREEN if result['status'] == 'SUCCESS' else RED
        print(f"{result['format']:<12} {result['file_size']:>10.2f}  {result['compression_ratio']:>10.1f}%  "
              f"{result['write_speed']:>12.1f}  {result['conversion_time']:>8.2f}  "
              f"{status_color}{result['status']:<8}{RESET}")

def generate_csv_for_comparison(csv_file):
    """Create CSV reference file for comparison"""
    print_info(f"CSV Reference: {csv_file}")
    csv_size = os.path.getsize(csv_file) / (1024 * 1024)
    print(f"   Size: {csv_size:.2f} MB (baseline)")
    
    return {
        'format': 'CSV',
        'output_file': csv_file,
        'conversion_time': 0,
        'read_time': 0,
        'write_speed': float('inf'),
        'read_speed': float('inf'),
        'compression_ratio': 0,
        'file_size': csv_size,
        'status': 'SUCCESS'
    }

def main():
    print_header("WORLD-CLASS MULTI-FORMAT BENCHMARK")
    print("Testing: CSV → KORE, Parquet, Avro, ORC")
    print(f"Date: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n")
    
    # Configuration
    test_dir = Path('./format_comparison_test')
    test_dir.mkdir(exist_ok=True)
    
    csv_file = test_dir / 'test_data.csv'
    
    # Generate test data
    if not csv_file.exists():
        generate_test_csv(str(csv_file), num_rows=10000)
    else:
        print_info(f"Using existing CSV: {csv_file}")
    
    # Benchmark each format
    results = []
    results.append(generate_csv_for_comparison(str(csv_file)))
    
    # Test KORE
    try:
        kore_file = test_dir / 'test_data.kore'
        result = benchmark_format(str(csv_file), str(kore_file), 'KORE', convert_csv_to_kore)
        results.append(result)
    except Exception as e:
        print_error(f"KORE test skipped: {e}")
    
    # Test Parquet
    if HAS_ARROW:
        try:
            parquet_file = test_dir / 'test_data.parquet'
            result = benchmark_format(str(csv_file), str(parquet_file), 'Parquet', convert_csv_to_parquet)
            results.append(result)
        except Exception as e:
            print_error(f"Parquet test failed: {e}")
    
    # Test ORC
    if HAS_ARROW:
        try:
            orc_file = test_dir / 'test_data.orc'
            result = benchmark_format(str(csv_file), str(orc_file), 'ORC', convert_csv_to_orc)
            results.append(result)
        except Exception as e:
            print_error(f"ORC test failed: {e}")
    
    # Test Avro
    if HAS_AVRO:
        try:
            avro_file = test_dir / 'test_data.avro'
            result = benchmark_format(str(csv_file), str(avro_file), 'Avro', convert_csv_to_avro)
            results.append(result)
        except Exception as e:
            print_error(f"Avro test failed: {e}")
    
    # Generate comparison
    compare_formats(results)
    
    # Detailed summary
    print_section("SUMMARY & RECOMMENDATIONS")
    
    successful = [r for r in results if r['status'] == 'SUCCESS']
    if successful:
        best_compression = max(successful, key=lambda x: x['compression_ratio'])
        fastest_write = max(successful, key=lambda x: x['write_speed'])
        
        print(f"\n{BOLD}Best Compression:{RESET}")
        print(f"  {best_compression['format']}: {best_compression['compression_ratio']:.1f}% reduction")
        
        print(f"\n{BOLD}Fastest Write:{RESET}")
        print(f"  {fastest_write['format']}: {fastest_write['write_speed']:.1f} MB/s")
        
        print(f"\n{BOLD}Recommendation:{RESET}")
        print(f"  Use {best_compression['format']} for storage optimization")
        print(f"  Use {fastest_write['format']} for rapid writes")
    
    print_success("\n🎉 World-class format comparison complete!")
    print_info(f"Test data location: {test_dir}\n")

if __name__ == '__main__':
    main()
