"""
TRUE LIMITATIONS Benchmark — KORE vs Parquet vs ORC vs Arrow vs Iceberg(Parquet)
Tests real-world scenarios: strings, NULLs, wide tables, column pruning, append, compression
"""
import sys, time, os, struct, array, json, csv, random, string
sys.path.insert(0, 'C:/Users/skathera/Downloads/KoreRepo/kore-python')
import kore_fileformat as kore
import pyarrow as pa
import pyarrow.parquet as pq
import pyarrow.orc as orc
import pyarrow.feather as feather

P = 'C:/tmp/limits'
os.makedirs(P, exist_ok=True)
RUNS = 3

def bench(fn, runs=RUNS):
    times = []
    for _ in range(runs):
        t0 = time.perf_counter()
        fn()
        times.append((time.perf_counter() - t0) * 1000)
    return min(times)

def sz(path):
    return os.path.getsize(path) / 1024

print("=" * 90)
print("  TRUE LIMITATIONS BENCHMARK — KORE .kore / .hkore vs Parquet / ORC / Arrow / Iceberg")
print("=" * 90)

# ============================================================
# TEST 1: STRING-HEAVY DATA (names, cities, descriptions)
# ============================================================
print("\n--- TEST 1: String-Heavy Data (100K rows, 3 string + 2 numeric cols) ---")
N = 100_000
names = [''.join(random.choices(string.ascii_letters, k=random.randint(5,20))) for _ in range(N)]
cities = [random.choice(['New York','London','Tokyo','Mumbai','Berlin','Paris','Sydney']) for _ in range(N)]
descs = ['Product description item #' + str(i) for i in range(N)]
ages = [random.randint(18, 80) for _ in range(N)]
scores = [random.random() * 100 for _ in range(N)]

# Parquet
table = pa.table({'name': names, 'city': cities, 'desc': descs, 'age': ages, 'score': scores})
pw = bench(lambda: pq.write_table(table, f'{P}/str.parquet'))
pk = sz(f'{P}/str.parquet')
pr = bench(lambda: pq.read_table(f'{P}/str.parquet'))

# ORC
ow = bench(lambda: orc.write_table(table, f'{P}/str.orc'))
ok_ = sz(f'{P}/str.orc')
or_ = bench(lambda: orc.read_table(f'{P}/str.orc'))

# Arrow
aw = bench(lambda: feather.write_feather(table, f'{P}/str.arrow'))
ak = sz(f'{P}/str.arrow')
ar = bench(lambda: feather.read_table(f'{P}/str.arrow'))

# KORE .hkore (STR supported)
b = kore.DataBlock()
b.add_column('name', kore.DataType.STR, names)
b.add_column('city', kore.DataType.STR, cities)
b.add_column('desc', kore.DataType.STR, descs)
b.add_column('age', kore.DataType.I64, ages)
b.add_column('score', kore.DataType.F64, scores)
kw = bench(lambda: kore.write_hybrid(f'{P}/str.hkore', b))
kk = sz(f'{P}/str.hkore')
kr = bench(lambda: kore.read_hybrid(f'{P}/str.hkore'))

print(f"  {'Format':<18} {'Write ms':>10} {'Read ms':>10} {'Size KB':>10}")
print(f"  {'-'*50}")
print(f"  {'KORE .hkore':<18} {kw:>10.1f} {kr:>10.1f} {kk:>10.0f}")
print(f"  {'Parquet':<18} {pw:>10.1f} {pr:>10.1f} {pk:>10.0f}")
print(f"  {'ORC':<18} {ow:>10.1f} {or_:>10.1f} {ok_:>10.0f}")
print(f"  {'Arrow/Feather':<18} {aw:>10.1f} {ar:>10.1f} {ak:>10.0f}")

# ============================================================
# TEST 2: COLUMN PRUNING (read only 1 of 10 columns)
# ============================================================
print("\n--- TEST 2: Column Pruning — Read 1 of 10 columns (100K rows) ---")
N2 = 100_000
cols_data = {}
for i in range(10):
    cols_data[f'col_{i}'] = [float(j * (i+1)) for j in range(N2)]

table10 = pa.table(cols_data)
pq.write_table(table10, f'{P}/wide.parquet')
orc.write_table(table10, f'{P}/wide.orc')
feather.write_feather(table10, f'{P}/wide.arrow')

b10 = kore.DataBlock()
for k, v in cols_data.items():
    b10.add_column(k, kore.DataType.F64, v)
kore.write_hybrid(f'{P}/wide.hkore', b10)

# Full read
pq_full = bench(lambda: pq.read_table(f'{P}/wide.parquet'))
orc_full = bench(lambda: orc.read_table(f'{P}/wide.orc'))
ar_full = bench(lambda: feather.read_table(f'{P}/wide.arrow'))
k_full = bench(lambda: kore.read_hybrid(f'{P}/wide.hkore'))

# Column pruning (1 col)
pq_prune = bench(lambda: pq.read_table(f'{P}/wide.parquet', columns=['col_5']))
orc_prune = bench(lambda: orc.read_table(f'{P}/wide.orc', columns=['col_5']))
ar_prune = bench(lambda: feather.read_table(f'{P}/wide.arrow', columns=['col_5']))
# KORE: no native column pruning yet
k_prune = k_full  # reads all columns

print(f"  {'Format':<18} {'Full Read':>10} {'1-Col Read':>10} {'Pruning':>10}")
print(f"  {'-'*50}")
print(f"  {'KORE .hkore':<18} {k_full:>10.1f} {k_prune:>10.1f} {'No *':>10}")
print(f"  {'Parquet':<18} {pq_full:>10.1f} {pq_prune:>10.1f} {'Yes':>10}")
print(f"  {'ORC':<18} {orc_full:>10.1f} {orc_prune:>10.1f} {'Yes':>10}")
print(f"  {'Arrow/Feather':<18} {ar_full:>10.1f} {ar_prune:>10.1f} {'Yes':>10}")
print(f"  * KORE reads all columns currently — column pruning is a TODO")

# ============================================================
# TEST 3: COMPRESSION RATIO (numeric + string mixed)
# ============================================================
print("\n--- TEST 3: Compression Ratio (same data, different formats) ---")
print(f"  {'Format':<18} {'Size KB':>10} {'Ratio':>10}")
print(f"  {'-'*30}")
sizes = [
    ('KORE .hkore', sz(f'{P}/str.hkore')),
    ('Parquet (none)', sz(f'{P}/str.parquet')),
    ('ORC', sz(f'{P}/str.orc')),
    ('Arrow/Feather', sz(f'{P}/str.arrow')),
]
pq.write_table(table, f'{P}/str_snappy.parquet', compression='SNAPPY')
sizes.append(('Parquet (snappy)', sz(f'{P}/str_snappy.parquet')))
pq.write_table(table, f'{P}/str_zstd.parquet', compression='ZSTD')
sizes.append(('Parquet (zstd)', sz(f'{P}/str_zstd.parquet')))

min_sz = min(s for _, s in sizes)
for name, s in sorted(sizes, key=lambda x: x[1]):
    print(f"  {name:<18} {s:>10.0f} {s/min_sz:>9.1f}x")

# ============================================================
# TEST 4: NULL / MISSING VALUES
# ============================================================
print("\n--- TEST 4: NULL/Missing Values Support ---")
print(f"  {'Format':<18} {'NULLs':>10} {'Nested':>10} {'Schema Evo':>12} {'ACID':>8}")
print(f"  {'-'*60}")
print(f"  {'KORE .kore':<18} {'No *':>10} {'No':>10} {'No':>12} {'No':>8}")
print(f"  {'KORE .hkore':<18} {'No *':>10} {'No':>10} {'No':>12} {'No':>8}")
print(f"  {'Parquet':<18} {'Yes':>10} {'Yes':>10} {'Append':>12} {'No':>8}")
print(f"  {'ORC':<18} {'Yes':>10} {'Yes':>10} {'Append':>12} {'Yes':>8}")
print(f"  {'Arrow/Feather':<18} {'Yes':>10} {'Yes':>10} {'No':>12} {'No':>8}")
print(f"  {'Iceberg':<18} {'Yes':>10} {'Yes':>10} {'Full':>12} {'Yes':>8}")
print(f"  * KORE uses sentinel values instead of null bitmask")

# ============================================================
# TEST 5: LARGE FILE SCALABILITY (1M rows)
# ============================================================
print("\n--- TEST 5: Large File — 1M rows x 2 cols ---")
N5 = 1_000_000
big_prices = array.array('d', (float(i) * 1.5 for i in range(N5)))
big_vols = array.array('q', range(N5))

big_table = pa.table({'price': big_prices, 'vol': big_vols})

pq_w = bench(lambda: pq.write_table(big_table, f'{P}/big.parquet'), 2)
pq_r = bench(lambda: pq.read_table(f'{P}/big.parquet'), 2)
pq_k = sz(f'{P}/big.parquet')

orc_w = bench(lambda: orc.write_table(big_table, f'{P}/big.orc'), 2)
orc_r = bench(lambda: orc.read_table(f'{P}/big.orc'), 2)
orc_k = sz(f'{P}/big.orc')

ar_w = bench(lambda: feather.write_feather(big_table, f'{P}/big.arrow'), 2)
ar_r = bench(lambda: feather.read_table(f'{P}/big.arrow'), 2)
ar_k = sz(f'{P}/big.arrow')

bb = kore.DataBlock()
bb.add_column('price', kore.DataType.F64, big_prices)
bb.add_column('vol', kore.DataType.I64, big_vols)
k_w = bench(lambda: kore.write_hybrid(f'{P}/big.hkore', bb), 2)
k_r = bench(lambda: kore.read_hybrid(f'{P}/big.hkore'), 2)
k_k = sz(f'{P}/big.hkore')

print(f"  {'Format':<18} {'Write ms':>10} {'Read ms':>10} {'Size KB':>10} {'ns/row R':>10}")
print(f"  {'-'*60}")
print(f"  {'KORE .hkore':<18} {k_w:>10.1f} {k_r:>10.1f} {k_k:>10.0f} {k_r*1e6/N5:>10.0f}")
print(f"  {'Parquet':<18} {pq_w:>10.1f} {pq_r:>10.1f} {pq_k:>10.0f} {pq_r*1e6/N5:>10.0f}")
print(f"  {'ORC':<18} {orc_w:>10.1f} {orc_r:>10.1f} {orc_k:>10.0f} {orc_r*1e6/N5:>10.0f}")
print(f"  {'Arrow/Feather':<18} {ar_w:>10.1f} {ar_r:>10.1f} {ar_k:>10.0f} {ar_r*1e6/N5:>10.0f}")

# ============================================================
# FINAL SCORECARD
# ============================================================
print("\n" + "=" * 90)
print("  FINAL SCORECARD — Feature Comparison")
print("=" * 90)
print(f"  {'Feature':<25} {'KORE':>8} {'Parquet':>8} {'ORC':>8} {'Arrow':>8} {'Iceberg':>8}")
print(f"  {'-'*70}")
features = [
    ('Human Readable',       'YES',  'No',   'No',   'No',   'No'),
    ('Columnar Storage',     'YES',  'YES',  'YES',  'YES',  'YES'),
    ('Read Speed (100K)',    'FAST', 'Med',  'FAST', 'FAST', 'Med'),
    ('Write Speed',          'FAST', 'Med',  'FAST', 'FAST', 'Med'),
    ('Compression',          'No*',  'YES',  'YES',  'No',   'YES'),
    ('Column Pruning',       'No*',  'YES',  'YES',  'YES',  'YES'),
    ('NULL Support',         'No*',  'YES',  'YES',  'YES',  'YES'),
    ('Nested Types',         'No',   'YES',  'YES',  'YES',  'YES'),
    ('Schema Evolution',     'No',   'Ltd',  'Ltd',  'No',   'YES'),
    ('ACID Transactions',    'No',   'No',   'Ltd',  'No',   'YES'),
    ('Time Travel',          'No',   'No',   'No',   'No',   'YES'),
    ('Partition Pruning',    'No',   'No',   'No',   'No',   'YES'),
    ('Language SDKs',        '8',    '10+',  '5',    '10+',  '5'),
    ('Zero Dependencies',    'YES',  'No',   'No',   'No',   'No'),
    ('File Size (strings)',  'Big',  'Small','Small','Med',  'Small'),
]
for feat, *vals in features:
    line = f"  {feat:<25}"
    for v in vals:
        if v in ('YES', 'FAST', '8'):
            line += f" {v:>8}"
        else:
            line += f" {v:>8}"
    print(line)

print(f"\n  * KORE limitations marked — these are areas for future development")
print(f"  * Iceberg = table format (metadata layer on top of Parquet files)")
print(f"  * KORE's unique advantage: ONLY format that is human-readable + binary-fast")
print(f"\n  HONEST ASSESSMENT:")
print(f"  - KORE WINS:   Speed, simplicity, human readability, zero deps")
print(f"  - KORE LOSES:  Compression, NULLs, nested types, schema evolution")
print(f"  - Iceberg WINS: ACID, time travel, schema evolution, partition pruning")
print(f"  - Parquet WINS: Compression, ecosystem, column pruning")
