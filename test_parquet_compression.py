#!/usr/bin/env python3
"""Test Parquet compression on hardest_dataset.csv"""

import pandas as pd
import os
from pathlib import Path

csv_file = Path("sample_10mb.csv")
parquet_file = Path("sample_10mb.parquet")

if csv_file.exists():
    print(f"📖 Reading: {csv_file.name} ({csv_file.stat().st_size / 1e6:.2f} MB)")
    
    # Read CSV
    df = pd.read_csv(csv_file)
    print(f"   Rows: {len(df):,} | Columns: {len(df.columns)}")
    
    # Write to Parquet with Snappy compression
    df.to_parquet(parquet_file, compression='snappy', index=False)
    
    csv_size = csv_file.stat().st_size
    parquet_size = parquet_file.stat().st_size
    ratio = (parquet_size / csv_size) * 100
    
    print(f"\n✅ Parquet Output: {parquet_file.name}")
    print(f"   Size: {parquet_size / 1e6:.2f} MB")
    print(f"   Ratio: {ratio:.1f}% (saved {csv_size - parquet_size:.0f} bytes)")
    print(f"\n🎯 Compression: {ratio:.1f}%")
else:
    print(f"❌ Missing: {csv_file.name}")
    print("   Use: python tools/make_parity_test.py")
