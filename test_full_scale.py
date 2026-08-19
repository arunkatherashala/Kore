"""KORE .kore + .hkore + Parquet — Full 3-way scale test using both Pythons"""
import subprocess, os, json, time

os.makedirs('C:/tmp', exist_ok=True)

MINICONDA = r"C:\Users\skathera\AppData\Local\miniconda3\python.exe"
SYSTEM_PY = "python"
REPO = r"C:/Users/skathera/Downloads/KoreRepo"

for N in [1_000_000, 10_000_000]:
    print(f"\n{'='*70}")
    print(f"  FULL 3-WAY TEST: {N:,} rows x 2 numeric cols")
    print(f"{'='*70}")

    # Step 1: .kore via system Python (has Rust FFI)
    kore_script = f"""
import sys, os, time, csv
sys.path.insert(0, '{REPO}/kore-python')
os.makedirs('C:/tmp', exist_ok=True)
N = {N}
csv_path = 'C:/tmp/_scale.csv'
with open(csv_path, 'w', newline='') as f:
    cw = csv.writer(f); cw.writerow(['price','vol'])
    for i in range(N): cw.writerow([float(i)*1.5, i])
from kore_fileformat import KoreWriter, KoreReader
t0 = time.perf_counter()
w = KoreWriter('C:/tmp/scale.kore')
w.write_csv(csv_path)
write_ms = (time.perf_counter() - t0) * 1000
size_kb = os.path.getsize('C:/tmp/scale.kore') / 1024
t0 = time.perf_counter()
r = KoreReader('C:/tmp/scale.kore')
r.read_columns()
read_ms = (time.perf_counter() - t0) * 1000
import json
print(json.dumps({{"write": write_ms, "read": read_ms, "size": size_kb}}))
"""
    result = subprocess.run([SYSTEM_PY, "-c", kore_script], capture_output=True, text=True, timeout=600)
    if result.returncode == 0 and result.stdout.strip():
        kore_data = json.loads(result.stdout.strip().split('\n')[-1])
        print(f"  .kore    W={kore_data['write']:.0f}ms  R={kore_data['read']:.0f}ms  Size={kore_data['size']:.0f}KB")
    else:
        kore_data = None
        print(f"  .kore    SKIPPED (FFI not available)")
        if result.stderr:
            print(f"           {result.stderr[:200]}")

    # Step 2: .hkore + .parquet via miniconda (has pyarrow)
    hkore_script = f"""
import sys, os, time, array, json
sys.path.insert(0, '{REPO}/kore-python')
import kore_fileformat as kore
import pyarrow as pa, pyarrow.parquet as pq
N = {N}
prices = array.array('d', (float(i)*1.5 for i in range(N)))
volumes = array.array('q', range(N))

# .hkore
b = kore.DataBlock()
b.add_column('price', kore.DataType.F64, prices)
b.add_column('vol', kore.DataType.I64, volumes)
t0 = time.perf_counter()
kore.write_hybrid('C:/tmp/scale.hkore', b)
hkore_w = (time.perf_counter() - t0) * 1000
hkore_kb = os.path.getsize('C:/tmp/scale.hkore') / 1024
t0 = time.perf_counter()
kore.read_hybrid('C:/tmp/scale.hkore')
hkore_r = (time.perf_counter() - t0) * 1000

# Parquet
table = pa.table({{'price': prices, 'vol': volumes}})
t0 = time.perf_counter()
pq.write_table(table, 'C:/tmp/scale.parquet', compression='NONE')
pq_w = (time.perf_counter() - t0) * 1000
pq_kb = os.path.getsize('C:/tmp/scale.parquet') / 1024
t0 = time.perf_counter()
pq.read_table('C:/tmp/scale.parquet')
pq_r = (time.perf_counter() - t0) * 1000

# Parquet snappy
t0 = time.perf_counter()
pq.write_table(table, 'C:/tmp/scale_s.parquet', compression='SNAPPY')
pqs_w = (time.perf_counter() - t0) * 1000
pqs_kb = os.path.getsize('C:/tmp/scale_s.parquet') / 1024
t0 = time.perf_counter()
pq.read_table('C:/tmp/scale_s.parquet')
pqs_r = (time.perf_counter() - t0) * 1000

print(json.dumps({{'hkore_w':hkore_w,'hkore_r':hkore_r,'hkore_kb':hkore_kb,'pq_w':pq_w,'pq_r':pq_r,'pq_kb':pq_kb,'pqs_w':pqs_w,'pqs_r':pqs_r,'pqs_kb':pqs_kb}}))
"""
    result2 = subprocess.run([MINICONDA, "-c", hkore_script], capture_output=True, text=True, timeout=600)
    if result2.returncode == 0 and result2.stdout.strip():
        d = json.loads(result2.stdout.strip().split('\n')[-1])
        print(f"  .hkore   W={d['hkore_w']:.0f}ms  R={d['hkore_r']:.0f}ms  Size={d['hkore_kb']:.0f}KB  ({d['hkore_r']*1e6/N:.0f} ns/row)")
        print(f"  Parquet  W={d['pq_w']:.0f}ms  R={d['pq_r']:.0f}ms  Size={d['pq_kb']:.0f}KB  ({d['pq_r']*1e6/N:.0f} ns/row)")
        print(f"  Pq(snap) W={d['pqs_w']:.0f}ms  R={d['pqs_r']:.0f}ms  Size={d['pqs_kb']:.0f}KB  ({d['pqs_r']*1e6/N:.0f} ns/row)")
    else:
        d = None
        print(f"  .hkore/parquet FAILED")
        if result2.stderr:
            print(f"  {result2.stderr[:300]}")

    # Summary
    if kore_data and d:
        print(f"\n  {'Format':<18} {'Write':>8} {'Read':>8} {'Size':>8} {'ns/row':>8}")
        print(f"  {'-'*45}")
        print(f"  {'KORE .kore':<18} {kore_data['write']:>7.0f}ms {kore_data['read']:>7.0f}ms {kore_data['size']:>7.0f}KB")
        print(f"  {'KORE .hkore':<18} {d['hkore_w']:>7.0f}ms {d['hkore_r']:>7.0f}ms {d['hkore_kb']:>7.0f}KB {d['hkore_r']*1e6/N:>7.0f}")
        print(f"  {'Parquet':<18} {d['pq_w']:>7.0f}ms {d['pq_r']:>7.0f}ms {d['pq_kb']:>7.0f}KB {d['pq_r']*1e6/N:>7.0f}")
        print(f"  {'Parquet (snappy)':<18} {d['pqs_w']:>7.0f}ms {d['pqs_r']:>7.0f}ms {d['pqs_kb']:>7.0f}KB {d['pqs_r']*1e6/N:>7.0f}")

print(f"\n{'='*70}")
print("  DONE!")
print(f"{'='*70}")
