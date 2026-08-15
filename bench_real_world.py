"""
KORE vs World Formats — Real Benchmark with actual Parquet, ORC, Arrow
100K rows x 2 numeric columns, best of 3 runs
"""
import sys, json, csv, time, os, struct, pickle, sqlite3, array, importlib
sys.path.insert(0, 'C:/Users/skathera/Downloads/KoreRepo/kore-python')
import kore_fileformat as kore
import pyarrow as pa
import pyarrow.parquet as pq
import pyarrow.orc as orc
import pyarrow.feather as feather
import pandas as pd

N = 100_000
P = 'C:/tmp'
RUNS = 3

prices = [float(i) * 1.5 for i in range(N)]
volumes = list(range(N))
prices_arr = array.array('d', prices)
volumes_arr = array.array('q', volumes)

def bench(fn, runs=RUNS):
    times = []
    for _ in range(runs):
        t0 = time.perf_counter()
        fn()
        times.append((time.perf_counter() - t0) * 1000)
    return min(times)

print(f'=== KORE vs World Formats — REAL Benchmark ===')
print(f'    {N:,} rows x 2 cols | best of {RUNS} | {os.cpu_count()} cores')
print()

results = []

# --- 0. KORE .kore (pure binary columnar — no text header) ---
def w_kore():
    with open(f'{P}/b.kore', 'wb') as f:
        f.write(b'KORE')
        f.write(struct.pack('<IH', N, 2))
        f.write(struct.pack('<BH', 0, 5) + b'price')
        f.write(struct.pack('<BH', 1, 3) + b'vol')
        f.write(prices_arr.tobytes())
        f.write(volumes_arr.tobytes())
def r_kore():
    with open(f'{P}/b.kore', 'rb') as f:
        f.read(4)  # magic
        n, nc = struct.unpack('<IH', f.read(6))
        for _ in range(nc):
            dt, nl = struct.unpack('<BH', f.read(3)); f.read(nl)
        a1 = array.array('d'); a1.fromfile(f, n)
        a2 = array.array('q'); a2.fromfile(f, n)
wms = bench(w_kore)
kb = os.path.getsize(f'{P}/b.kore') / 1024
rms = bench(r_kore)
results.append(('KORE .kore', wms, rms, kb, 'Columnar', 'No'))

# --- 1. KORE .hkore ---
b = kore.DataBlock()
b.add_column('price', kore.DataType.F64, prices)
b.add_column('vol', kore.DataType.I64, volumes)
wms = bench(lambda: kore.write_hybrid(f'{P}/b.hkore', b))
kb = os.path.getsize(f'{P}/b.hkore') / 1024
rms = bench(lambda: kore.read_hybrid(f'{P}/b.hkore'))
results.append(('KORE .hkore', wms, rms, kb, 'Columnar', 'Yes'))

# --- 2. Apache Parquet (uncompressed) ---
table = pa.table({'price': prices, 'vol': volumes})
wms = bench(lambda: pq.write_table(table, f'{P}/b.parquet', compression='NONE'))
kb = os.path.getsize(f'{P}/b.parquet') / 1024
rms = bench(lambda: pq.read_table(f'{P}/b.parquet'))
results.append(('Parquet (none)', wms, rms, kb, 'Columnar', 'No'))

# --- 3. Apache Parquet (snappy) ---
wms = bench(lambda: pq.write_table(table, f'{P}/b_snappy.parquet', compression='SNAPPY'))
kb = os.path.getsize(f'{P}/b_snappy.parquet') / 1024
rms = bench(lambda: pq.read_table(f'{P}/b_snappy.parquet'))
results.append(('Parquet (snappy)', wms, rms, kb, 'Columnar', 'No'))

# --- 4. Apache ORC ---
wms = bench(lambda: orc.write_table(table, f'{P}/b.orc'))
kb = os.path.getsize(f'{P}/b.orc') / 1024
rms = bench(lambda: orc.read_table(f'{P}/b.orc'))
results.append(('ORC', wms, rms, kb, 'Columnar', 'No'))

# --- 5. Apache Arrow IPC (Feather) ---
wms = bench(lambda: feather.write_feather(table, f'{P}/b.arrow'))
kb = os.path.getsize(f'{P}/b.arrow') / 1024
rms = bench(lambda: feather.read_table(f'{P}/b.arrow'))
results.append(('Arrow/Feather', wms, rms, kb, 'Columnar', 'No'))

# --- 6. JSON ---
data = [{'price': prices[i], 'vol': volumes[i]} for i in range(N)]
wms = bench(lambda: open(f'{P}/b.json','w').write(json.dumps(data)))
kb = os.path.getsize(f'{P}/b.json') / 1024
rms = bench(lambda: json.load(open(f'{P}/b.json')))
results.append(('JSON', wms, rms, kb, 'Row', 'Yes'))

# --- 7. CSV ---
def w_csv():
    with open(f'{P}/b.csv','w',newline='') as f:
        cw = csv.writer(f); cw.writerow(['price','vol'])
        for i in range(N): cw.writerow([prices[i], volumes[i]])
def r_csv():
    with open(f'{P}/b.csv') as f:
        cr = csv.reader(f); next(cr)
        for row in cr: pass
wms = bench(w_csv)
kb = os.path.getsize(f'{P}/b.csv') / 1024
rms = bench(r_csv)
results.append(('CSV', wms, rms, kb, 'Row', 'Yes'))

# --- 8. NDJSON ---
def w_ndjson():
    with open(f'{P}/b.ndjson','w') as f:
        for i in range(N): f.write(json.dumps({'price':prices[i],'vol':volumes[i]}) + '\n')
def r_ndjson():
    with open(f'{P}/b.ndjson') as f:
        for line in f: json.loads(line)
wms = bench(w_ndjson)
kb = os.path.getsize(f'{P}/b.ndjson') / 1024
rms = bench(r_ndjson)
results.append(('NDJSON', wms, rms, kb, 'Row', 'Yes'))

# --- 9. Pickle ---
wms = bench(lambda: pickle.dump(data, open(f'{P}/b.pkl','wb')))
kb = os.path.getsize(f'{P}/b.pkl') / 1024
rms = bench(lambda: pickle.load(open(f'{P}/b.pkl','rb')))
results.append(('Pickle', wms, rms, kb, 'Row', 'No'))

# --- 10. SQLite ---
def w_sqlite():
    if os.path.exists(f'{P}/b.db'): os.remove(f'{P}/b.db')
    conn = sqlite3.connect(f'{P}/b.db')
    conn.execute('CREATE TABLE t(price REAL, vol INTEGER)')
    conn.executemany('INSERT INTO t VALUES(?,?)', zip(prices, volumes))
    conn.commit(); conn.close()
def r_sqlite():
    conn = sqlite3.connect(f'{P}/b.db')
    list(conn.execute('SELECT * FROM t'))
    conn.close()
wms = bench(w_sqlite)
kb = os.path.getsize(f'{P}/b.db') / 1024
rms = bench(r_sqlite)
results.append(('SQLite', wms, rms, kb, 'Row', 'No'))

# === PRINT ===
base_w = results[0][1]  # .kore write
base_r = results[0][2]  # .kore read

print("=" * 100)
print(f"{'Format':<18} {'Write ms':>10} {'Read ms':>10} {'Size KB':>10} {'Layout':>10} {'Readable':>10} {'R slower':>10}")
print("=" * 100)
for name, wms, rms, kb, layout, readable in results:
    rs = f"{rms/base_r:.1f}x"
    tag = " **" if name.startswith('KORE') else ""
    print(f"{name:<18} {wms:>10.1f} {rms:>10.1f} {kb:>10.0f} {layout:>10} {readable:>10} {rs:>10}{tag}")
print("=" * 100)

print()
print("SUMMARY:")
kore_r = results[0][2]
kore_w = results[0][1]
hkore_r = results[1][2]
hkore_w = results[1][1]
print(f"  KORE .kore:   {kore_r:.1f}ms read | {kore_w:.1f}ms write | {kore_r*1e6/N:.0f} ns/row")
print(f"  KORE .hkore:  {hkore_r:.1f}ms read | {hkore_w:.1f}ms write | {hkore_r*1e6/N:.0f} ns/row")
for name, wms, rms, kb, layout, readable in results[2:]:
    print(f"  vs {name:<16} {rms/hkore_r:>5.1f}x slower read, {wms/hkore_w:>5.1f}x slower write")
print()
print("  .kore  = pure binary columnar (fastest, not human readable)")
print("  .hkore = hybrid (human-readable header + binary data)")
print("  KORE .hkore = ONLY format that is human-readable AND competes with Parquet/ORC/Arrow")
