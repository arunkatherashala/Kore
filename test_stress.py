"""KORE STRESS TEST — Push .kore to its limits until it breaks"""
import kore_py, time, os, random, gc, sys

print("=" * 60)
print("  KORE STRESS TEST — Find the breaking point")
print("=" * 60)

# Test 1: Max rows (double until crash)
print("\n--- TEST 1: Max Rows (2 cols, doubling) ---")
for exp in [20, 21, 22, 23, 24, 25]:
    N = 2 ** exp
    try:
        gc.collect()
        b = kore_py.PyDataBlock()
        b.add_f64_column('x', [float(i) for i in range(N)])
        b.add_i64_column('y', list(range(N)))
        t0 = time.perf_counter()
        kore_py.write_kore(f'C:/tmp/stress_{N}.kore', b)
        wms = (time.perf_counter() - t0) * 1000
        sz = os.path.getsize(f'C:/tmp/stress_{N}.kore') / (1024*1024)
        t0 = time.perf_counter()
        d = kore_py.read_kore(f'C:/tmp/stress_{N}.kore')
        rms = (time.perf_counter() - t0) * 1000
        print(f"  {N:>12,} rows: W={wms:>8,.0f}ms R={rms:>8,.0f}ms Size={sz:>8,.0f}MB  PASS")
        os.remove(f'C:/tmp/stress_{N}.kore')
        del b, d
        gc.collect()
    except Exception as e:
        print(f"  {N:>12,} rows: CRASHED — {type(e).__name__}: {str(e)[:80]}")
        break

# Test 2: Max columns
print("\n--- TEST 2: Max Columns (1000 rows) ---")
for ncols in [10, 50, 100, 200, 500, 1000]:
    try:
        b = kore_py.PyDataBlock()
        for i in range(ncols):
            b.add_f64_column(f'c{i}', [float(j) for j in range(1000)])
        t0 = time.perf_counter()
        kore_py.write_kore('C:/tmp/stress_cols.kore', b)
        wms = (time.perf_counter() - t0) * 1000
        sz = os.path.getsize('C:/tmp/stress_cols.kore') / (1024*1024)
        t0 = time.perf_counter()
        d = kore_py.read_kore('C:/tmp/stress_cols.kore')
        rms = (time.perf_counter() - t0) * 1000
        print(f"  {ncols:>6} cols: W={wms:>6,.0f}ms R={rms:>6,.0f}ms Size={sz:>6,.1f}MB  PASS")
        del b, d; gc.collect()
    except Exception as e:
        print(f"  {ncols:>6} cols: CRASHED — {type(e).__name__}: {str(e)[:80]}")
        break

# Test 3: Max string length
print("\n--- TEST 3: Max String Length (100 rows) ---")
for slen in [100, 1000, 10000, 100000, 1000000]:
    try:
        b = kore_py.PyDataBlock()
        b.add_str_column('big', ['X' * slen for _ in range(100)])
        kore_py.write_kore('C:/tmp/stress_str.kore', b)
        d = kore_py.read_kore('C:/tmp/stress_str.kore')
        actual = d.get_str_column('big')[0]
        ok = len(actual) == slen
        sz = os.path.getsize('C:/tmp/stress_str.kore') / 1024
        print(f"  {slen:>10,} chars: Size={sz:>8,.0f}KB  Roundtrip={'PASS' if ok else 'FAIL'}")
        del b, d; gc.collect()
    except Exception as e:
        print(f"  {slen:>10,} chars: CRASHED — {type(e).__name__}: {str(e)[:80]}")
        break

# Test 4: Streaming chunks (simulate 10 BILLION rows)
print("\n--- TEST 4: Streaming Write (chunks of 10M, target 10 BILLION) ---")
total_rows = 0
total_ms = 0
CHUNK = 10_000_000
TARGET = 1_000_000_000  # 1 billion first, then scale up
os.makedirs('C:/tmp/stream', exist_ok=True)
for chunk in range(TARGET // CHUNK):
    b = kore_py.PyDataBlock()
    b.add_f64_column('val', [random.random() for _ in range(CHUNK)])
    b.add_i64_column('id', list(range(chunk*CHUNK, (chunk+1)*CHUNK)))
    t0 = time.perf_counter()
    kore_py.write_kore(f'C:/tmp/stream/chunk_{chunk:04d}.kore', b)
    total_ms += (time.perf_counter() - t0) * 1000
    total_rows += CHUNK
    sz = os.path.getsize(f'C:/tmp/stream/chunk_{chunk:04d}.kore') / (1024*1024)
    print(f"  {total_rows:>15,} rows  chunk={chunk}  {total_ms:,.0f}ms  {sz:.0f}MB/chunk")
    del b; gc.collect()

print(f"\n  TOTAL: {total_rows:,} rows in {total_ms:,.0f}ms ({total_ms/total_rows*1e6:.0f} ns/row)")

print(f"\n{'='*60}")
print("  STRESS TEST COMPLETE")
print(f"{'='*60}")
