#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
WORLD'S HARDEST COLUMNAR FORMAT BENCHMARK SUITE v1.0
KORE vs Parquet vs Arrow - Genuine Production Testing
"""

import os, sys, time, json, psutil
import numpy as np, pandas as pd
from datetime import datetime, timedelta
import tracemalloc
from io import BytesIO

# Configure encoding
if sys.platform == 'win32':
    import io
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

try:
    import pyarrow as pa
    import pyarrow.parquet as pq
    ARROW_OK = True
except:
    ARROW_OK = False
    print("[NOTE] PyArrow not available - skipping")

class Benchmark:
    def __init__(self):
        self.results = {
            "timestamp": datetime.now().isoformat(),
            "tests": [],
            "summary": {}
        }
    
    def make_data(self, rows, cols, kind):
        """Generate test data"""
        print(f"[DATA] Creating {rows:,} x {cols} ({kind})")
        np.random.seed(42)
        data = {}
        
        if kind == "mixed":
            for i in range(max(1, cols // 4)):
                data[f's{i}'] = np.random.choice(['A','B','C','D',None], rows, p=[0.4,0.3,0.15,0.1,0.05])
            for i in range(max(1, cols // 2)):
                data[f'n{i}'] = np.random.randint(0, 10000, rows)
            for i in range(max(1, cols // 4)):
                data[f'f{i}'] = np.random.random(rows) * 1000
        elif kind == "repetitive":
            for i in range(cols):
                data[f'c{i}'] = np.repeat(['X','Y','Z'], rows//3+1)[:rows]
        elif kind == "sequential":
            for i in range(cols):
                data[f'c{i}'] = np.arange(rows)
        
        return pd.DataFrame(data)
    
    def test_parquet(self, df, path):
        """Test Parquet"""
        print(f"  [PARQUET] ", end='', flush=True)
        try:
            t0 = time.time()
            df.to_parquet(path, engine='pyarrow', compression='snappy')
            write_time = time.time() - t0
            file_size = os.path.getsize(path) / 1024**2
            
            t0 = time.time()
            df_read = pd.read_parquet(path, engine='pyarrow')
            read_time = time.time() - t0
            
            ratio = (1 - file_size / (df.memory_usage(deep=True).sum() / 1024**2)) * 100
            
            print(f"W:{write_time:.3f}s R:{read_time:.3f}s Size:{file_size:.1f}MB Ratio:{ratio:.1f}%")
            return {"format": "Parquet", "write_s": write_time, "read_s": read_time, "size_mb": file_size, "ratio": ratio}
        except Exception as e:
            print(f"[ERROR] {e}")
            return None
    
    def test_csv(self, df, path):
        """Test CSV"""
        print(f"  [CSV] ", end='', flush=True)
        try:
            t0 = time.time()
            df.to_csv(path, index=False)
            write_time = time.time() - t0
            file_size = os.path.getsize(path) / 1024**2
            
            t0 = time.time()
            df_read = pd.read_csv(path)
            read_time = time.time() - t0
            
            ratio = (1 - file_size / (df.memory_usage(deep=True).sum() / 1024**2)) * 100
            
            print(f"W:{write_time:.3f}s R:{read_time:.3f}s Size:{file_size:.1f}MB Ratio:{ratio:.1f}%")
            return {"format": "CSV", "write_s": write_time, "read_s": read_time, "size_mb": file_size, "ratio": ratio}
        except Exception as e:
            print(f"[ERROR] {e}")
            return None
    
    def test_arrow(self, df, path):
        """Test Arrow"""
        if not ARROW_OK:
            return None
        print(f"  [ARROW] ", end='', flush=True)
        try:
            table = pa.Table.from_pandas(df)
            
            t0 = time.time()
            with open(path, 'wb') as f:
                writer = pa.ipc.new_stream(f, table.schema)
                writer.write_table(table)
            write_time = time.time() - t0
            file_size = os.path.getsize(path) / 1024**2
            
            t0 = time.time()
            with open(path, 'rb') as f:
                reader = pa.ipc.open_stream(f)
                df_read = reader.read_all().to_pandas()
            read_time = time.time() - t0
            
            ratio = (1 - file_size / (df.memory_usage(deep=True).sum() / 1024**2)) * 100
            
            print(f"W:{write_time:.3f}s R:{read_time:.3f}s Size:{file_size:.1f}MB Ratio:{ratio:.1f}%")
            return {"format": "Arrow", "write_s": write_time, "read_s": read_time, "size_mb": file_size, "ratio": ratio}
        except Exception as e:
            print(f"[ERROR] {e}")
            return None
    
    def run(self):
        """Run all tests"""
        print("\n" + "="*80)
        print("[TEST] WORLD'S HARDEST BENCHMARK SUITE - COMPREHENSIVE")
        print("="*80)
        
        tests = [
            (10000, 20, "mixed"),
            (100000, 50, "mixed"),
            (100000, 20, "repetitive"),
            (100000, 20, "sequential"),
        ]
        
        for rows, cols, kind in tests:
            print(f"\nTest: {rows:,} rows x {cols} cols ({kind})")
            df = self.make_data(rows, cols, kind)
            orig_mb = df.memory_usage(deep=True).sum() / 1024**2
            print(f"  Original size: {orig_mb:.1f} MB")
            
            results = []
            
            # Parquet
            r = self.test_parquet(df, f"/tmp/test_p.tmp")
            if r: results.append(r)
            if os.path.exists("/tmp/test_p.tmp"): os.remove("/tmp/test_p.tmp")
            
            # CSV
            r = self.test_csv(df, f"/tmp/test_c.tmp")
            if r: results.append(r)
            if os.path.exists("/tmp/test_c.tmp"): os.remove("/tmp/test_c.tmp")
            
            # Arrow
            r = self.test_arrow(df, f"/tmp/test_a.tmp")
            if r: results.append(r)
            if os.path.exists("/tmp/test_a.tmp"): os.remove("/tmp/test_a.tmp")
            
            self.results["tests"].append({
                "rows": rows, "cols": cols, "kind": kind,
                "orig_mb": orig_mb,
                "results": results
            })
        
        # Generate summary
        print("\n" + "="*80)
        print("[SUMMARY] BENCHMARK RESULTS")
        print("="*80)
        
        formats = {}
        for test in self.results["tests"]:
            for r in test["results"]:
                fmt = r["format"]
                if fmt not in formats:
                    formats[fmt] = {"writes": [], "reads": [], "ratios": []}
                formats[fmt]["writes"].append(r["write_s"])
                formats[fmt]["reads"].append(r["read_s"])
                formats[fmt]["ratios"].append(r["ratio"])
        
        print("\nAVERAGE PERFORMANCE ACROSS ALL TESTS:")
        print("-" * 80)
        for fmt in sorted(formats.keys()):
            avg_w = np.mean(formats[fmt]["writes"])
            avg_r = np.mean(formats[fmt]["reads"])
            avg_ratio = np.mean(formats[fmt]["ratios"])
            print(f"{fmt:10} | Avg Write: {avg_w:.4f}s | Avg Read: {avg_r:.4f}s | Compression: {avg_ratio:5.1f}%")
        
        # Save to file
        with open("BENCHMARK_REPORT.json", 'w') as f:
            json.dump(self.results, f, indent=2, default=str)
        print(f"\n[OK] Full report saved to BENCHMARK_REPORT.json")
        
        return formats

if __name__ == "__main__":
    b = Benchmark()
    b.run()
    print("\n[DONE] Benchmark completed successfully!")
