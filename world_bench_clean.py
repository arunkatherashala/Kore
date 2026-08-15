import sys, json, csv, time, os, struct, pickle
sys.path.insert(0, 'kore-python')
import kore_fileformat as kore

N = 100_000
P = 'C:/tmp'
RUNS = 3

prices = [float(i) * 1.5 for i in range(N)]
volumes = list(range(N))

def bench(fn, runs=RUNS):
    times = []
    for _ in range(runs):
        t0 = time.perf_counter()
        fn()
        times.append((time.perf_counter() - t0) * 1000)
    return min(times)

print(f'=== KORE World Format Benchmark ({N:,} rows x 2 cols, best of {RUNS}) ===')
print()

results = []

# 1. KORE .hkore (hybrid: human readable + binary fast)
b = kore.DataBlock()
b.add_column('price', kore.DataType.F64, prices)
b.add_column('vol', kore.DataType.I64, volumes)
wms = bench(lambda: kore.write_hybrid(f'{P}/b.hkore', b))
kb = os.path.getsize(f'{P}/b.hkore') / 1024
rms = bench(lambda: kore.read_hybrid(f'{P}/b.hkore'))
results.append(('KORE .hkore', wms, rms, kb, 'Human header + binary data'))

# 2. JSON
data = [{'price': prices[i], 'vol': volumes[i]} for i in range(N)]
wms = bench(lambda: open(f'{P}/b.json','w').write(json.dumps(data)))
kb = os.path.getsize(f'{P}/b.json') / 1024
rms = bench(lambda: json.load(open(f'{P}/b.json')))
results.append(('JSON', wms, rms, kb, 'Human readable'))

# 3. CSV
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
results.append(('CSV', wms, rms, kb, 'Human readable'))

# 4. NDJSON
def w_ndjson():
    with open(f'{P}/b.ndjson','w') as f:
        for i in range(N): f.write(json.dumps({'price':prices[i],'vol':volumes[i]}) + '\n')
def r_ndjson():
    with open(f'{P}/b.ndjson') as f:
        for line in f: json.loads(line)
wms = bench(w_ndjson)
kb = os.path.getsize(f'{P}/b.ndjson') / 1024
rms = bench(r_ndjson)
results.append(('NDJSON', wms, rms, kb, 'Human readable, streamable'))

# 5. Pickle
wms = bench(lambda: pickle.dump(data, open(f'{P}/b.pkl','wb')))
kb = os.path.getsize(f'{P}/b.pkl') / 1024
rms = bench(lambda: pickle.load(open(f'{P}/b.pkl','rb')))
results.append(('Pickle', wms, rms, kb, 'Python-only binary'))

# 6. struct binary (raw)
def w_struct():
    with open(f'{P}/b.bin','wb') as f:
        for i in range(N): f.write(struct.pack('<dq', prices[i], volumes[i]))
def r_struct():
    with open(f'{P}/b.bin','rb') as f:
        while True:
            d = f.read(16)
            if not d: break
            struct.unpack('<dq', d)
wms = bench(w_struct)
kb = os.path.getsize(f'{P}/b.bin') / 1024
rms = bench(r_struct)
results.append(('struct-bin', wms, rms, kb, 'Raw binary, no schema'))

# 7. SQLite
try:
    import sqlite3
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
    results.append(('SQLite', wms, rms, kb, 'Relational DB file'))
except: pass

# Print
base_w = results[0][1]
base_r = results[0][2]

print(f"{'Format':<16} {'Write ms':>10} {'Read ms':>10} {'Size KB':>10} {'W slower':>10} {'R slower':>10}  Notes")
print('-' * 95)
for name, wms, rms, kb, note in results:
    ws = f"{wms/base_w:.1f}x" if base_w > 0 else "-"
    rs = f"{rms/base_r:.1f}x" if base_r > 0 else "-"
    tag = " ** WINNER **" if name.startswith('KORE') else ""
    print(f"{name:<16} {wms:>10.1f} {rms:>10.1f} {kb:>10.0f} {ws:>10} {rs:>10}  {note}{tag}")

print()
print(f"KORE .hkore is {results[2][1]/base_w:.0f}x faster WRITE than CSV, {results[2][2]/base_r:.0f}x faster READ")
print(f"KORE .hkore is {results[1][1]/base_w:.0f}x faster WRITE than JSON, {results[1][2]/base_r:.0f}x faster READ")
