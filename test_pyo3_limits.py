"""PyO3-native path limitation tests — .kore (Rust native) vs Parquet vs ORC.
Uses kore_py.write_kore / kore_py.read_kore (NOT the pure-Python ctypes path).
Probes:
  L1. Cardinality effect on compression (low, medium, all-unique strings)
  L2. Data-shape (repetitive ints, sequential ints, random floats, wide 20-col schema)
  L3. Scale (5M rows realistic mixed)
  L4. Nulls — does PyO3 API even accept None? (spec-level limitation)
  L5. Column projection — does read_kore support columns=? (spec-level limitation)
  L6. Read-side lossy get_*_column behavior (data-correctness on None readback)
  L7. Roundtrip correctness (values, not just counts)
"""
import os, sys, time, gc, random, string
import kore_py
import pyarrow as pa, pyarrow.parquet as pq, pyarrow.orc as orc

os.makedirs('C:/tmp/pyo3lim', exist_ok=True)

def bench(path_kore, path_pq_z, path_orc, add_kore, pa_dict):
    """Run write+read for kore(PyO3) + parquet-zstd + orc. Returns dict."""
    row = {}
    # KORE PyO3
    b = kore_py.PyDataBlock()
    for name, kind, data in add_kore:
        if kind == 'i64': b.add_i64_column(name, data)
        elif kind == 'f64': b.add_f64_column(name, data)
        elif kind == 'str': b.add_str_column(name, data)
    gc.collect()
    try:
        t0 = time.perf_counter(); kore_py.write_kore(path_kore, b); wms = (time.perf_counter()-t0)*1000
        sz = os.path.getsize(path_kore)/1024
        t0 = time.perf_counter(); d = kore_py.read_kore(path_kore); rms = (time.perf_counter()-t0)*1000
        row['kore_native'] = (wms, rms, sz)
    except Exception as e:
        row['kore_native'] = ('ERR', 'ERR', str(e)[:60])

    # Parquet ZSTD
    t = pa.table(pa_dict)
    t0 = time.perf_counter(); pq.write_table(t, path_pq_z, compression='ZSTD'); wms = (time.perf_counter()-t0)*1000
    sz = os.path.getsize(path_pq_z)/1024
    t0 = time.perf_counter(); pq.read_table(path_pq_z); rms = (time.perf_counter()-t0)*1000
    row['pq_zstd'] = (wms, rms, sz)

    # ORC
    t0 = time.perf_counter(); orc.write_table(t, path_orc); wms = (time.perf_counter()-t0)*1000
    sz = os.path.getsize(path_orc)/1024
    t0 = time.perf_counter(); orc.read_table(path_orc); rms = (time.perf_counter()-t0)*1000
    row['orc'] = (wms, rms, sz)
    return row

def print_row(name, row):
    print(f"\n  {name}")
    print(f"    {'format':<14} {'write ms':>10} {'read ms':>10} {'size KB':>10}")
    print(f"    {'-'*48}")
    for tag in ('kore_native','pq_zstd','orc'):
        v = row.get(tag)
        if v is None: continue
        w, r, s = v
        if w == 'ERR':
            print(f"    {tag:<14} ERROR: {s}")
        else:
            print(f"    {tag:<14} {w:>10.0f} {r:>10.0f} {s:>10.0f}")

# =====================================================================
# L1. CARDINALITY effect (100K rows × 3 STR columns)
# =====================================================================
print("=" * 78)
print("L1. Cardinality effect (100K rows × 3 STR cols) — PyO3 native path")
print("=" * 78)

N1 = 100_000
random.seed(42)

# 1a. Low card (4 unique)
low = ['East','West','North','South'] * (N1 // 4)
r1a = bench('C:/tmp/pyo3lim/L1a.kore','C:/tmp/pyo3lim/L1a.pq','C:/tmp/pyo3lim/L1a.orc',
    [('a','str',low),('b','str',low),('c','str',low)],
    {'a':low,'b':low,'c':low})
print_row('L1a low-cardinality (4 unique)', r1a)

# 1b. Medium (100 unique)
med = [f'cat_{i%100}' for i in range(N1)]
r1b = bench('C:/tmp/pyo3lim/L1b.kore','C:/tmp/pyo3lim/L1b.pq','C:/tmp/pyo3lim/L1b.orc',
    [('a','str',med),('b','str',med),('c','str',med)],
    {'a':med,'b':med,'c':med})
print_row('L1b medium-cardinality (100 unique)', r1b)

# 1c. All unique
uniq = [''.join(random.choices(string.ascii_letters+string.digits, k=12)) for _ in range(N1)]
r1c = bench('C:/tmp/pyo3lim/L1c.kore','C:/tmp/pyo3lim/L1c.pq','C:/tmp/pyo3lim/L1c.orc',
    [('a','str',uniq),('b','str',uniq),('c','str',uniq)],
    {'a':uniq,'b':uniq,'c':uniq})
print_row('L1c all-unique (worst compression case)', r1c)

# =====================================================================
# L2. DATA-SHAPE (500K rows)
# =====================================================================
print("\n" + "=" * 78)
print("L2. Data-shape shootout (500K rows) — PyO3 native")
print("=" * 78)

N2 = 500_000
random.seed(7)

r2a = bench('C:/tmp/pyo3lim/L2a.kore','C:/tmp/pyo3lim/L2a.pq','C:/tmp/pyo3lim/L2a.orc',
    [('x','i64',[42]*N2)], {'x':[42]*N2})
print_row('L2a int all-same (extreme compression test)', r2a)

r2b = bench('C:/tmp/pyo3lim/L2b.kore','C:/tmp/pyo3lim/L2b.pq','C:/tmp/pyo3lim/L2b.orc',
    [('x','i64',list(range(N2)))], {'x':list(range(N2))})
print_row('L2b int sequential', r2b)

randf = [random.random()*1e6 for _ in range(N2)]
r2c = bench('C:/tmp/pyo3lim/L2c.kore','C:/tmp/pyo3lim/L2c.pq','C:/tmp/pyo3lim/L2c.orc',
    [('x','f64',randf)], {'x':randf})
print_row('L2c float random (hardest to compress)', r2c)

# Wide schema — 20 cols
wide_kore, wide_pa = [], {}
for i in range(20):
    kind = ['i64','f64','str'][i % 3]
    if kind == 'i64':   d = [i*100 + j for j in range(N2)]
    elif kind == 'f64': d = [j*0.5 for j in range(N2)]
    else:               d = [f'val_{j%50}' for j in range(N2)]
    wide_kore.append((f'c{i:02d}', kind, d))
    wide_pa[f'c{i:02d}'] = d
r2d = bench('C:/tmp/pyo3lim/L2d.kore','C:/tmp/pyo3lim/L2d.pq','C:/tmp/pyo3lim/L2d.orc', wide_kore, wide_pa)
print_row('L2d wide schema (20 cols mixed)', r2d)

del wide_kore, wide_pa, randf; gc.collect()

# =====================================================================
# L3. SCALE (5M rows realistic)
# =====================================================================
print("\n" + "=" * 78)
print("L3. Scale (5M rows realistic mixed) — PyO3 native")
print("=" * 78)
N3 = 5_000_000
random.seed(13)
ids = list(range(N3))
prices = [round(random.uniform(0.01, 9999.99), 2) for _ in range(N3)]
qtys = [random.randint(1, 10000) for _ in range(N3)]
cities10 = [random.choice(['NYC','London','Tokyo','Mumbai','Berlin','Paris','Sydney','LA','SF','Chicago']) for _ in range(N3)]
names_var = [''.join(random.choices(string.ascii_letters, k=random.randint(5,15))) for _ in range(N3)]

r3 = bench('C:/tmp/pyo3lim/L3.kore','C:/tmp/pyo3lim/L3.pq','C:/tmp/pyo3lim/L3.orc',
    [('id','i64',ids),('price','f64',prices),('qty','i64',qtys),('name','str',names_var),('city','str',cities10)],
    {'id':ids,'price':prices,'qty':qtys,'name':names_var,'city':cities10})
print_row('L3 scale 5M rows × 5 cols', r3)
del ids, prices, qtys, cities10, names_var; gc.collect()

# =====================================================================
# L4. NULL SUPPORT (API-level)
# =====================================================================
print("\n" + "=" * 78)
print("L4. NULL support at PyO3 API level — does add_*_column accept None?")
print("=" * 78)

def try_null(kind, data):
    b = kore_py.PyDataBlock()
    try:
        if kind == 'f64': b.add_f64_column('x', data)
        elif kind == 'i64': b.add_i64_column('x', data)
        elif kind == 'str': b.add_str_column('x', data)
        return "OK — accepted"
    except Exception as e:
        return f"REJECTED: {type(e).__name__}: {str(e)[:80]}"

print(f"    F64 with None: {try_null('f64', [1.0, None, 2.0])}")
print(f"    I64 with None: {try_null('i64', [1, None, 2])}")
print(f"    STR with None: {try_null('str', ['a', None, 'c'])}")
print("    (Parquet natively supports nulls via arrow.)")

# =====================================================================
# L5. COLUMN PROJECTION (API-level)
# =====================================================================
print("\n" + "=" * 78)
print("L5. Column projection — does PyO3 read_kore support columns=?")
print("=" * 78)
try:
    _ = kore_py.read_kore('C:/tmp/pyo3lim/L1a.kore', columns=['a'])
    print("    PyO3 read_kore(columns=...) : SUPPORTED")
except TypeError as e:
    print(f"    PyO3 read_kore(columns=...) : NOT SUPPORTED ({str(e)[:80]})")
except Exception as e:
    print(f"    PyO3 read_kore(columns=...) : ERROR {type(e).__name__}: {str(e)[:80]}")

# =====================================================================
# L6. LOSSY READBACK — get_*_column silently substitutes None → NaN/0/""
# =====================================================================
print("\n" + "=" * 78)
print("L6. Read-side data-correctness — how are Nones surfaced?")
print("=" * 78)
print("    (We can't inject nulls at write, but the get_*_column methods silently")
print("     substitute unwrap_or defaults — a data-lossy pattern seen in lib.rs.)")
print("    From lib.rs:")
print("      Float64: x.unwrap_or(f64::NAN)  → None becomes NaN")
print("      Int64:   x.unwrap_or(0)         → None becomes 0 (INDISTINGUISHABLE from real 0)")
print("      Str:     x.unwrap_or_default()  → None becomes empty string")
print("      StrDict: code==u8::MAX → empty string   (data corruption risk)")

# =====================================================================
# L7. ROUNDTRIP CORRECTNESS (10K rows, all values checked)
# =====================================================================
print("\n" + "=" * 78)
print("L7. Full-value roundtrip correctness (10K rows) — PyO3 native")
print("=" * 78)
N7 = 10_000
random.seed(99)
r_ids = list(range(N7))
r_prices = [random.random()*1e6 for _ in range(N7)]
r_names  = [''.join(random.choices(string.ascii_letters+string.digits, k=random.randint(5,20))) for _ in range(N7)]

b7 = kore_py.PyDataBlock()
b7.add_i64_column('id', r_ids)
b7.add_f64_column('price', r_prices)
b7.add_str_column('name', r_names)
kore_py.write_kore('C:/tmp/pyo3lim/L7.kore', b7)
d7 = kore_py.read_kore('C:/tmp/pyo3lim/L7.kore')

got_ids = d7.get_i64_column('id')
got_prices = d7.get_f64_column('price')
got_names = d7.get_str_column('name')

id_ok = got_ids == r_ids
price_ok = all(abs(a-b) < 1e-9 for a,b in zip(got_prices, r_prices))
name_ok = got_names == r_names
first_bad_name = next((i for i,(a,b) in enumerate(zip(got_names, r_names)) if a != b), None)
print(f"    ids     : {'OK' if id_ok else 'MISMATCH'}")
print(f"    prices  : {'OK' if price_ok else 'MISMATCH'}")
print(f"    names   : {'OK' if name_ok else 'MISMATCH'}"
      + (f"   (first diff at row {first_bad_name}: {got_names[first_bad_name]!r} vs {r_names[first_bad_name]!r})"
         if first_bad_name is not None else ""))

# =====================================================================
# SUMMARY
# =====================================================================
print("\n" + "=" * 78)
print("PyO3-native limits pass complete.")
print("=" * 78)
