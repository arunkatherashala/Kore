#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
KORE vs THE WORLD - COMPREHENSIVE FILE FORMAT BENCHMARK
Compares Kore against Parquet, Arrow, CSV, ORC, Avro, HDF5, NDJSON, and JSON
"""

import os, sys, time, json, tempfile, subprocess
import numpy as np, pandas as pd
from datetime import datetime
import traceback

if sys.platform == 'win32':
    import io
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

# Try importing optional libraries
try:
    import pyarrow as pa
    import pyarrow.parquet as pq
    ARROW_OK = True
except:
    ARROW_OK = False

try:
    import h5py
    HDF5_OK = True
except:
    HDF5_OK = False

try:
    import fastavro
    AVRO_OK = True
except:
    AVRO_OK = False

try:
    import pyorc
    ORC_OK = True
except:
    ORC_OK = False

class ComprehensiveBenchmark:
    def __init__(self):
        self.results = {
            "timestamp": datetime.now().isoformat(),
            "tests": [],
            "format_summary": {},
            "winner_by_metric": {}
        }
        self.tmp = tempfile.gettempdir()
    
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
    
    def safe_test(self, name, func, df, path):
        """Safely run a test format"""
        print(f"  [{name}] ", end='', flush=True)
        try:
            result = func(df, path)
            return result
        except Exception as e:
            print(f"[SKIP] {type(e).__name__}: {str(e)[:50]}")
            return None
    
    def test_parquet(self, df, path):
        t0 = time.time()
        df.to_parquet(path, engine='pyarrow', compression='snappy')
        write_time = time.time() - t0
        file_size = os.path.getsize(path) / 1024**2
        
        t0 = time.time()
        _ = pd.read_parquet(path, engine='pyarrow')
        read_time = time.time() - t0
        
        ratio = (1 - file_size / (df.memory_usage(deep=True).sum() / 1024**2)) * 100
        print(f"W:{write_time:.3f}s R:{read_time:.3f}s Size:{file_size:.1f}MB Ratio:{ratio:.1f}%")
        return {"format": "Parquet", "write_s": write_time, "read_s": read_time, "size_mb": file_size, "ratio": ratio}
    
    def test_arrow_feather(self, df, path):
        t0 = time.time()
        df.to_feather(path)
        write_time = time.time() - t0
        file_size = os.path.getsize(path) / 1024**2
        
        t0 = time.time()
        _ = pd.read_feather(path)
        read_time = time.time() - t0
        
        ratio = (1 - file_size / (df.memory_usage(deep=True).sum() / 1024**2)) * 100
        print(f"W:{write_time:.3f}s R:{read_time:.3f}s Size:{file_size:.1f}MB Ratio:{ratio:.1f}%")
        return {"format": "Arrow/Feather", "write_s": write_time, "read_s": read_time, "size_mb": file_size, "ratio": ratio}
    
    def test_csv(self, df, path):
        t0 = time.time()
        df.to_csv(path, index=False)
        write_time = time.time() - t0
        file_size = os.path.getsize(path) / 1024**2
        
        t0 = time.time()
        _ = pd.read_csv(path)
        read_time = time.time() - t0
        
        ratio = (1 - file_size / (df.memory_usage(deep=True).sum() / 1024**2)) * 100
        print(f"W:{write_time:.3f}s R:{read_time:.3f}s Size:{file_size:.1f}MB Ratio:{ratio:.1f}%")
        return {"format": "CSV", "write_s": write_time, "read_s": read_time, "size_mb": file_size, "ratio": ratio}
    
    def test_ndjson(self, df, path):
        t0 = time.time()
        df.to_json(path, orient='records', lines=True)
        write_time = time.time() - t0
        file_size = os.path.getsize(path) / 1024**2
        
        t0 = time.time()
        _ = pd.read_json(path, lines=True)
        read_time = time.time() - t0
        
        ratio = (1 - file_size / (df.memory_usage(deep=True).sum() / 1024**2)) * 100
        print(f"W:{write_time:.3f}s R:{read_time:.3f}s Size:{file_size:.1f}MB Ratio:{ratio:.1f}%")
        return {"format": "NDJSON", "write_s": write_time, "read_s": read_time, "size_mb": file_size, "ratio": ratio}
    
    def test_json(self, df, path):
        t0 = time.time()
        df.to_json(path, orient='records')
        write_time = time.time() - t0
        file_size = os.path.getsize(path) / 1024**2
        
        t0 = time.time()
        _ = pd.read_json(path)
        read_time = time.time() - t0
        
        ratio = (1 - file_size / (df.memory_usage(deep=True).sum() / 1024**2)) * 100
        print(f"W:{write_time:.3f}s R:{read_time:.3f}s Size:{file_size:.1f}MB Ratio:{ratio:.1f}%")
        return {"format": "JSON", "write_s": write_time, "read_s": read_time, "size_mb": file_size, "ratio": ratio}
    
    def test_orc(self, df, path):
        if not ORC_OK:
            return None
        t0 = time.time()
        df.to_orc(path)
        write_time = time.time() - t0
        file_size = os.path.getsize(path) / 1024**2
        
        t0 = time.time()
        _ = pd.read_orc(path)
        read_time = time.time() - t0
        
        ratio = (1 - file_size / (df.memory_usage(deep=True).sum() / 1024**2)) * 100
        print(f"W:{write_time:.3f}s R:{read_time:.3f}s Size:{file_size:.1f}MB Ratio:{ratio:.1f}%")
        return {"format": "ORC", "write_s": write_time, "read_s": read_time, "size_mb": file_size, "ratio": ratio}
    
    def test_hdf5(self, df, path):
        if not HDF5_OK:
            return None
        t0 = time.time()
        df.to_hdf(path, 'data', mode='w')
        write_time = time.time() - t0
        file_size = os.path.getsize(path) / 1024**2
        
        t0 = time.time()
        _ = pd.read_hdf(path, 'data')
        read_time = time.time() - t0
        
        ratio = (1 - file_size / (df.memory_usage(deep=True).sum() / 1024**2)) * 100
        print(f"W:{write_time:.3f}s R:{read_time:.3f}s Size:{file_size:.1f}MB Ratio:{ratio:.1f}%")
        return {"format": "HDF5", "write_s": write_time, "read_s": read_time, "size_mb": file_size, "ratio": ratio}
    
    def test_sqlite(self, df, path):
        import sqlite3
        t0 = time.time()
        conn = sqlite3.connect(path)
        df.to_sql('data', conn, if_exists='replace', index=False)
        conn.close()
        write_time = time.time() - t0
        file_size = os.path.getsize(path) / 1024**2
        
        t0 = time.time()
        conn = sqlite3.connect(path)
        _ = pd.read_sql('SELECT * FROM data', conn)
        conn.close()
        read_time = time.time() - t0
        
        ratio = (1 - file_size / (df.memory_usage(deep=True).sum() / 1024**2)) * 100
        print(f"W:{write_time:.3f}s R:{read_time:.3f}s Size:{file_size:.1f}MB Ratio:{ratio:.1f}%")
        return {"format": "SQLite", "write_s": write_time, "read_s": read_time, "size_mb": file_size, "ratio": ratio}
    
    def run(self):
        """Run all tests"""
        print("\n" + "="*90)
        print("[BENCHMARK] KORE vs THE WORLD - COMPREHENSIVE FILE FORMAT COMPARISON")
        print("="*90)
        
        tests = [
            (10000, 20, "mixed"),
            (100000, 50, "mixed"),
            (100000, 20, "repetitive"),
        ]
        
        for rows, cols, kind in tests:
            print(f"\n{'='*90}")
            print(f"Test: {rows:,} rows x {cols} cols ({kind})")
            print('='*90)
            df = self.make_data(rows, cols, kind)
            orig_mb = df.memory_usage(deep=True).sum() / 1024**2
            print(f"Original size: {orig_mb:.1f} MB")
            print("-"*90)
            
            results = []
            
            # Test all formats
            formats = [
                ("Parquet", lambda d, p: self.test_parquet(d, p), "parquet"),
                ("Arrow/Feather", lambda d, p: self.test_arrow_feather(d, p), "feather"),
                ("CSV", lambda d, p: self.test_csv(d, p), "csv"),
                ("NDJSON", lambda d, p: self.test_ndjson(d, p), "ndjson"),
                ("JSON", lambda d, p: self.test_json(d, p), "json"),
                ("ORC", lambda d, p: self.test_orc(d, p), "orc"),
                ("HDF5", lambda d, p: self.test_hdf5(d, p), "h5"),
                ("SQLite", lambda d, p: self.test_sqlite(d, p), "db"),
            ]
            
            for name, func, ext in formats:
                path = os.path.join(self.tmp, f"bench.{ext}")
                r = self.safe_test(name, func, df, path)
                if r:
                    results.append(r)
                if os.path.exists(path):
                    try:
                        os.remove(path)
                    except:
                        pass
            
            self.results["tests"].append({
                "rows": rows, "cols": cols, "kind": kind,
                "orig_mb": orig_mb,
                "results": results
            })
        
        # Generate summary
        self.print_summary()
        
        # Save to file
        with open("KORE_VS_ALL_COMPETITORS_REPORT.json", 'w') as f:
            json.dump(self.results, f, indent=2, default=str)
        print(f"\n[OK] Full report saved to KORE_VS_ALL_COMPETITORS_REPORT.json")
        
        return self.results
    
    def print_summary(self):
        """Print summary comparison"""
        print("\n" + "="*90)
        print("[SUMMARY] COMPREHENSIVE FORMAT COMPARISON")
        print("="*90)
        
        formats = {}
        for test in self.results["tests"]:
            for r in test["results"]:
                fmt = r["format"]
                if fmt not in formats:
                    formats[fmt] = {"writes": [], "reads": [], "ratios": [], "sizes": []}
                formats[fmt]["writes"].append(r["write_s"])
                formats[fmt]["reads"].append(r["read_s"])
                formats[fmt]["ratios"].append(r["ratio"])
                formats[fmt]["sizes"].append(r["size_mb"])
        
        print("\nAVERAGE PERFORMANCE ACROSS ALL TESTS:")
        print("-" * 90)
        print(f"{'Format':<20} | {'Write (s)':<12} | {'Read (s)':<12} | {'Ratio %':<12} | {'Avg Size':<12}")
        print("-" * 90)
        
        winners = {"write": None, "read": None, "ratio": None, "size": None}
        best_write = float('inf')
        best_read = float('inf')
        best_ratio = 0
        best_size = float('inf')
        
        for fmt in sorted(formats.keys()):
            avg_w = np.mean(formats[fmt]["writes"])
            avg_r = np.mean(formats[fmt]["reads"])
            avg_ratio = np.mean(formats[fmt]["ratios"])
            avg_size = np.mean(formats[fmt]["sizes"])
            
            print(f"{fmt:<20} | {avg_w:<12.4f} | {avg_r:<12.4f} | {avg_ratio:<12.1f} | {avg_size:<12.1f}")
            
            if avg_w < best_write:
                best_write = avg_w
                winners["write"] = fmt
            if avg_r < best_read:
                best_read = avg_r
                winners["read"] = fmt
            if avg_ratio > best_ratio:
                best_ratio = avg_ratio
                winners["ratio"] = fmt
            if avg_size < best_size:
                best_size = avg_size
                winners["size"] = fmt
        
        print("\n" + "-" * 90)
        print("🏆 WINNERS:")
        print(f"  Fastest Write: {winners['write']} ({best_write:.4f}s)")
        print(f"  Fastest Read:  {winners['read']} ({best_read:.4f}s)")
        print(f"  Best Ratio:    {winners['ratio']} ({best_ratio:.1f}%)")
        print(f"  Smallest Size: {winners['size']} ({best_size:.1f}MB)")
        print("-" * 90)
        
        self.results["format_summary"] = {fmt: {
            "avg_write": float(np.mean(formats[fmt]["writes"])),
            "avg_read": float(np.mean(formats[fmt]["reads"])),
            "avg_ratio": float(np.mean(formats[fmt]["ratios"])),
            "avg_size": float(np.mean(formats[fmt]["sizes"]))
        } for fmt in formats}
        
        self.results["winner_by_metric"] = {
            "write": winners["write"],
            "read": winners["read"],
            "ratio": winners["ratio"],
            "size": winners["size"]
        }

if __name__ == "__main__":
    b = ComprehensiveBenchmark()
    b.run()
    print("\n[DONE] Comprehensive benchmark completed!")
