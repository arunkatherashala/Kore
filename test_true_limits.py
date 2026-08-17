"""TRUE LIMITATIONS test suite for .kore / .hkore vs Parquet / ORC.
Probes:
  L1. Cardinality effect on compression (low-card wins vs high-card losses)
  L2. Data-shape shootout — repetitive ints, random floats, random strings, wide schema
  L3. Scale — 10M rows: does it hold up?
  L4. Column projection — read 1 of 5 cols
  L5. Null handling correctness — do nulls survive?
  L6. Roundtrip correctness — do values actually match end-to-end?
Each test isolates a concrete failure mode and reports honest numbers, not spin."""
import os, sys, time, gc, random, string, tracemalloc
sys.path.insert(0, 'kore-python')
import kore_fileformat as kore
import pyarrow as pa, pyarrow.parquet as pq, pyarrow.orc as orc

os.makedirs('C:/tmp/limits', exist_ok=True)

def bytes_of(path): return os.path.getsize(path)

def time_ms(fn):
    gc.collect()
    t0 = time.perf_counter()
    r = fn()
    return (time.perf_counter() - t0) * 1000, r

def make_kore_block(cols):
    """cols = [(name, dtype, data)]"""
    b = kore.DataBlock()
    for name, dt, data in cols:
        b.add_column(name, dt, data)
    return b

def bench_all(name, cols_kore, cols_pa):
    """Write + read all 4 formats; return dict of results."""
    b = make_kore_block(cols_kore)
    table = pa.table(cols_pa)
    row = {'test': name, 'N': b.num_rows, 'ncols': b.num_columns}
    paths = {
        'kore':     f'C:/tmp/limits/{name}.kore',
        'hkore':    f'C:/tmp/limits/{name}.hkore',
        'pq_snap':  f'C:/tmp/limits/{name}.snap.parquet',
        'pq_zstd':  f'C:/tmp/limits/{name}.zstd.parquet',
        'orc':      f'C:/tmp/limits/{name}.orc',
    }
    ops = [
        ('kore',    lambda: kore.write_kore(paths['kore'], b),           lambda: kore.read_kore(paths['kore'])),
        ('hkore',   lambda: kore.write_hybrid(paths['hkore'], b),        lambda: kore.read_hybrid(paths['hkore'])),
        ('pq_snap', lambda: pq.write_table(table, paths['pq_snap'], compression='SNAPPY'), lambda: pq.read_table(paths['pq_snap'])),
        ('pq_zstd', lambda: pq.write_table(table, paths['pq_zstd'], compression='ZSTD'),   lambda: pq.read_table(paths['pq_zstd'])),
        ('orc',     lambda: orc.write_table(table, paths['orc']),        lambda: orc.read_table(paths['orc'])),
    ]
    for tag, w, r in ops:
        try:
            wms,_ = time_ms(w); sz = bytes_of(paths[tag])
            rms,_ = time_ms(r)
            row[tag] = (wms, rms, sz/1024)
        except Exception as e:
            row[tag] = ('ERR', 'ERR', str(e)[:60])
    return row

def print_row(row):
    print(f"\n  {row['test']}: N={row['N']:,}, cols={row['ncols']}")
    print(f"    {'format':<10} {'write ms':>10} {'read ms':>10} {'size KB':>10}")
    print(f"    {'-'*45}")
    for tag in ('kore','hkore','pq_snap','pq_zstd','orc'):
        v = row.get(tag)
        if v is None: continue
        w,r,s = v
        if w == 'ERR':
            print(f"    {tag:<10} ERROR: {s}")
        else:
            print(f"    {tag:<10} {w:>10.0f} {r:>10.0f} {s:>10.0f}")

# ======================================================================
# L1. CARDINALITY EFFECT
# ======================================================================
print("=" * 78)
print("L1. Cardinality effect on compression (N=100_000, 3 STR columns)")
print("=" * 78)
print("     Does .kore's compression advantage survive when strings are unique?")

N1 = 100_000
random.seed(42)

# 1a. Low cardinality (4 unique strings) — the earlier "18% smaller" scenario
regions4 = ['East','West','North','South'] * (N1 // 4)
row1a = bench_all('L1a_lowcard4',
    [('a', kore.DataType.STR, regions4), ('b', kore.DataType.STR, regions4), ('c', kore.DataType.STR, regions4)],
    {'a': regions4, 'b': regions4, 'c': regions4})
print_row(row1a)

# 1b. Medium cardinality (100 unique strings)
cats100 = [f'cat_{i%100}' for i in range(N1)]
row1b = bench_all('L1b_medcard100',
    [('a', kore.DataType.STR, cats100), ('b', kore.DataType.STR, cats100), ('c', kore.DataType.STR, cats100)],
    {'a': cats100, 'b': cats100, 'c': cats100})
print_row(row1b)

# 1c. High cardinality (all unique random 12-char strings)
uniq = [''.join(random.choices(string.ascii_letters+string.digits, k=12)) for _ in range(N1)]
row1c = bench_all('L1c_allunique',
    [('a', kore.DataType.STR, uniq), ('b', kore.DataType.STR, uniq), ('c', kore.DataType.STR, uniq)],
    {'a': uniq, 'b': uniq, 'c': uniq})
print_row(row1c)

# ======================================================================
# L2. DATA-SHAPE SHOOTOUT
# ======================================================================
print("\n" + "=" * 78)
print("L2. Data-shape shootout (N=500_000)")
print("=" * 78)

N2 = 500_000
random.seed(7)

# 2a. Repetitive integers (all 42) — pure compression test
row2a = bench_all('L2a_int_allsame',
    [('x', kore.DataType.I64, [42]*N2)],
    {'x': [42]*N2})
print_row(row2a)

# 2b. Sequential integers — mild compression
row2b = bench_all('L2b_int_seq',
    [('x', kore.DataType.I64, list(range(N2)))],
    {'x': list(range(N2))})
print_row(row2b)

# 2c. Random floats — hardest to compress
randf = [random.random()*1e6 for _ in range(N2)]
row2c = bench_all('L2c_float_random',
    [('x', kore.DataType.F64, randf)],
    {'x': randf})
print_row(row2c)

# 2d. Wide schema (20 cols, mixed types)
wide_kore, wide_pa = [], {}
for i in range(20):
    dt = [kore.DataType.I64, kore.DataType.F64, kore.DataType.STR][i % 3]
    if dt == kore.DataType.I64: d = [i*100 + j for j in range(N2)]
    elif dt == kore.DataType.F64: d = [j*0.5 for j in range(N2)]
    else: d = [f'val_{j%50}' for j in range(N2)]
    wide_kore.append((f'c{i:02d}', dt, d))
    wide_pa[f'c{i:02d}'] = d
row2d = bench_all('L2d_wide20cols', wide_kore, wide_pa)
print_row(row2d)

# ======================================================================
# L3. SCALE — 10M rows
# ======================================================================
print("\n" + "=" * 78)
print("L3. Scale test (N=5_000_000, realistic mixed)")
print("=" * 78)

N3 = 5_000_000
random.seed(13)
ids = list(range(N3))
prices = [round(random.uniform(0.01, 9999.99), 2) for _ in range(N3)]
qtys = [random.randint(1, 10000) for _ in range(N3)]
cities10 = [random.choice(['NYC','London','Tokyo','Mumbai','Berlin','Paris','Sydney','LA','SF','Chicago']) for _ in range(N3)]
# names: mid-cardinality random-looking (5-15 char)
names_var = [''.join(random.choices(string.ascii_letters, k=random.randint(5,15))) for _ in range(N3)]

row3 = bench_all('L3_scale5M',
    [('id', kore.DataType.I64, ids), ('price', kore.DataType.F64, prices),
     ('qty', kore.DataType.I64, qtys), ('name', kore.DataType.STR, names_var),
     ('city', kore.DataType.STR, cities10)],
    {'id': ids, 'price': prices, 'qty': qtys, 'name': names_var, 'city': cities10})
print_row(row3)

# free memory
del ids, prices, qtys, cities10, names_var
gc.collect()

# ======================================================================
# L4. COLUMN PROJECTION
# ======================================================================
print("\n" + "=" * 78)
print("L4. Column projection — read 1 of 5 cols (N=500_000)")
print("=" * 78)
print("     Does .kore actually skip unread columns? Fair comparison to parquet.")

N4 = 500_000
random.seed(21)
ids4    = list(range(N4))
prices4 = [random.random()*1000 for _ in range(N4)]
qtys4   = [random.randint(1, 999) for _ in range(N4)]
names4  = [''.join(random.choices(string.ascii_letters, k=10)) for _ in range(N4)]
cities4 = [random.choice(['NYC','London','Tokyo','Mumbai','Berlin']) for _ in range(N4)]

b4 = kore.DataBlock()
b4.add_column('id',    kore.DataType.I64, ids4)
b4.add_column('price', kore.DataType.F64, prices4)
b4.add_column('qty',   kore.DataType.I64, qtys4)
b4.add_column('name',  kore.DataType.STR, names4)
b4.add_column('city',  kore.DataType.STR, cities4)
kore.write_kore('C:/tmp/limits/L4.kore', b4)

table4 = pa.table({'id':ids4,'price':prices4,'qty':qtys4,'name':names4,'city':cities4})
pq.write_table(table4, 'C:/tmp/limits/L4.parquet', compression='ZSTD')

# Full read
full_k,_ = time_ms(lambda: kore.read_kore('C:/tmp/limits/L4.kore'))
full_p,_ = time_ms(lambda: pq.read_table('C:/tmp/limits/L4.parquet'))

# Projection read (id only)
proj_k,_ = time_ms(lambda: kore.read_kore('C:/tmp/limits/L4.kore', columns=['id']))
proj_p,_ = time_ms(lambda: pq.read_table('C:/tmp/limits/L4.parquet', columns=['id']))

print(f"    format     full read   1-col read   speedup   projection working?")
print(f"    " + "-"*66)
print(f"    kore     {full_k:>10.0f}ms {proj_k:>10.0f}ms  {full_k/proj_k:>6.2f}x  {'YES' if proj_k < full_k*0.6 else 'NO — reads everything anyway'}")
print(f"    parquet  {full_p:>10.0f}ms {proj_p:>10.0f}ms  {full_p/proj_p:>6.2f}x  {'YES' if proj_p < full_p*0.6 else 'NO'}")

del ids4, prices4, qtys4, names4, cities4, b4, table4
gc.collect()

# ======================================================================
# L5. NULL HANDLING
# ======================================================================
print("\n" + "=" * 78)
print("L5. NULL handling correctness (N=1000)")
print("=" * 78)
print("     Do actual None values survive roundtrip? String cols especially suspect.")

N5 = 1000
prices_null = [None if i % 3 == 0 else float(i) for i in range(N5)]
names_null  = [None if i % 4 == 0 else f'name_{i}' for i in range(N5)]

b5 = kore.DataBlock()
b5.add_column('price', kore.DataType.F64, prices_null)
b5.add_column('name',  kore.DataType.STR, names_null)

# .kore
try:
    kore.write_kore('C:/tmp/limits/L5.kore', b5)
    d5 = kore.read_kore('C:/tmp/limits/L5.kore')
    read_prices = list(d5.columns[0].data)
    read_names  = list(d5.columns[1].data)
    price_null_pres = sum(1 for x in read_prices if x is None or (isinstance(x, float) and x != x))
    name_null_pres  = sum(1 for x in read_names if x is None or x == 'None' or x == '')
    price_orig_nulls = sum(1 for x in prices_null if x is None)
    name_orig_nulls  = sum(1 for x in names_null if x is None)
    print(f"    .kore:  price nulls in={price_orig_nulls}, out={price_null_pres}   name nulls in={name_orig_nulls}, out={name_null_pres}")
    # Check what happened to name None values
    orig_none_indices = [i for i,x in enumerate(names_null) if x is None][:5]
    got = [read_names[i] for i in orig_none_indices]
    print(f"    .kore   sample name-None readback: {got}   (should be None or empty)")
except Exception as e:
    print(f"    .kore FAILED: {e}")

# Parquet reference
table5 = pa.table({'price': prices_null, 'name': names_null})
pq.write_table(table5, 'C:/tmp/limits/L5.parquet', compression='ZSTD')
d5p = pq.read_table('C:/tmp/limits/L5.parquet').to_pydict()
p_pr_null = sum(1 for x in d5p['price'] if x is None)
p_nm_null = sum(1 for x in d5p['name'] if x is None)
print(f"    parquet: price nulls in={sum(1 for x in prices_null if x is None)}, out={p_pr_null}   name nulls in={sum(1 for x in names_null if x is None)}, out={p_nm_null}")

# ======================================================================
# L6. ROUNDTRIP CORRECTNESS
# ======================================================================
print("\n" + "=" * 78)
print("L6. Full-value roundtrip correctness (N=10_000)")
print("=" * 78)
print("     Not just row count — verify every value survives.")

N6 = 10_000
random.seed(99)
r_ids = list(range(N6))
r_prices = [random.random()*1e6 for _ in range(N6)]
r_names = [''.join(random.choices(string.ascii_letters+string.digits, k=random.randint(5,20))) for _ in range(N6)]

b6 = kore.DataBlock()
b6.add_column('id', kore.DataType.I64, r_ids)
b6.add_column('price', kore.DataType.F64, r_prices)
b6.add_column('name', kore.DataType.STR, r_names)

for fmt, w, r in (
    ('kore',   lambda: kore.write_kore('C:/tmp/limits/L6.kore', b6),    lambda: kore.read_kore('C:/tmp/limits/L6.kore')),
    ('hkore',  lambda: kore.write_hybrid('C:/tmp/limits/L6.hkore', b6), lambda: kore.read_hybrid('C:/tmp/limits/L6.hkore')),
):
    try:
        w(); d = r()
        got_ids = list(d.columns[0].data)
        got_prices = list(d.columns[1].data)
        got_names = list(d.columns[2].data)
        id_ok = got_ids == r_ids
        price_ok = all(abs(a-b) < 1e-9 for a,b in zip(got_prices, r_prices))
        name_ok = got_names == r_names
        first_bad_name = next((i for i,(a,b) in enumerate(zip(got_names, r_names)) if a != b), None)
        print(f"    {fmt:<8} ids OK={id_ok}   prices OK={price_ok}   names OK={name_ok}"
              + (f"    (first diff at row {first_bad_name}: {got_names[first_bad_name]!r} vs {r_names[first_bad_name]!r})" if first_bad_name is not None else ""))
    except Exception as e:
        print(f"    {fmt:<8} FAILED: {e}")

# ======================================================================
# SUMMARY
# ======================================================================
print("\n" + "=" * 78)
print("DONE — see limitation findings above.")
print("=" * 78)
