import sys; sys.path.insert(0, 'kore-python')
import kore_fileformat as kore
import kore_fileformat as kore_pip
import json, csv, time, os, struct, pickle, array

N = 100000
P = 'C:/tmp'

prices = [float(i) * 1.5 for i in range(N)]
volumes = list(range(N))

print(f'=== KORE World Format Benchmark ({N:,} rows x 2 cols) ===')
print()

results = []

# 1. KORE .kore (via installed pip package)
try:
    del sys.modules['kore_fileformat']
except: pass
sys.path_orig = sys.path[:]
sys.path = [p for p in sys.path if 'KoreRepo' not in p]
import importlib
kore_pip = importlib.import_module('kore_fileformat')
w_obj = kore_pip.KoreWriter(f'{P}/b.kore')

# Write CSV first, then convert
with open(f'{P}/_bench.csv','w',newline='') as f:
    cw=csv.writer(f); cw.writerow(['price','vol'])
    for i in range(N): cw.writerow([prices[i],volumes[i]])

t0=time.perf_counter(); w_obj.write_csv(f'{P}/_bench.csv'); w=time.perf_counter()-t0
kb = os.path.getsize(f'{P}/b.kore')/1024
r_obj = kore_pip.KoreReader(f'{P}/b.kore')
t0=time.perf_counter(); r_obj.read_columns(); r=time.perf_counter()-t0
results.append(('KORE .kore', w*1000, r*1000, kb))
sys.path = sys.path_orig

# Reload repo version
if 'kore_fileformat' in sys.modules:
    del sys.modules['kore_fileformat']
sys.path.insert(0, 'kore-python')
kore = importlib.import_module('kore_fileformat')

# 2. KORE .hkore
t0=time.perf_counter(); kore.write_hybrid(f'{P}/b.hkore', b); w=time.perf_counter()-t0
kb = os.path.getsize(f'{P}/b.hkore')/1024
t0=time.perf_counter(); kore.read_hybrid(f'{P}/b.hkore'); r=time.perf_counter()-t0
results.append(('KORE .hkore', w*1000, r*1000, kb))

# 3. JSON
data = [{'price': prices[i], 'vol': volumes[i]} for i in range(N)]
t0=time.perf_counter()
with open(f'{P}/b.json','w') as f: json.dump(data,f)
w=time.perf_counter()-t0
kb = os.path.getsize(f'{P}/b.json')/1024
t0=time.perf_counter()
with open(f'{P}/b.json') as f: json.load(f)
r=time.perf_counter()-t0
results.append(('JSON', w*1000, r*1000, kb))

# 4. CSV
t0=time.perf_counter()
with open(f'{P}/b.csv','w',newline='') as f:
    cw=csv.writer(f); cw.writerow(['price','vol'])
    for i in range(N): cw.writerow([prices[i],volumes[i]])
w=time.perf_counter()-t0
kb = os.path.getsize(f'{P}/b.csv')/1024
t0=time.perf_counter()
with open(f'{P}/b.csv') as f:
    cr=csv.reader(f); next(cr)
    for row in cr: pass
r=time.perf_counter()-t0
results.append(('CSV', w*1000, r*1000, kb))

# 5. NDJSON
t0=time.perf_counter()
with open(f'{P}/b.ndjson','w') as f:
    for i in range(N): f.write(json.dumps({'price':prices[i],'vol':volumes[i]})+'\n')
w=time.perf_counter()-t0
kb = os.path.getsize(f'{P}/b.ndjson')/1024
t0=time.perf_counter()
with open(f'{P}/b.ndjson') as f:
    for line in f: json.loads(line)
r=time.perf_counter()-t0
results.append(('NDJSON', w*1000, r*1000, kb))

# 6. Pickle
t0=time.perf_counter()
with open(f'{P}/b.pkl','wb') as f: pickle.dump(data,f)
w=time.perf_counter()-t0
kb = os.path.getsize(f'{P}/b.pkl')/1024
t0=time.perf_counter()
with open(f'{P}/b.pkl','rb') as f: pickle.load(f)
r=time.perf_counter()-t0
results.append(('Pickle', w*1000, r*1000, kb))

# 7. struct binary
t0=time.perf_counter()
with open(f'{P}/b.bin','wb') as f:
    for i in range(N): f.write(struct.pack('<dq', prices[i], volumes[i]))
w=time.perf_counter()-t0
kb = os.path.getsize(f'{P}/b.bin')/1024
t0=time.perf_counter()
with open(f'{P}/b.bin','rb') as f:
    while True:
        d=f.read(16)
        if not d: break
        struct.unpack('<dq',d)
r=time.perf_counter()-t0
results.append(('struct-bin', w*1000, r*1000, kb))

# 8. MessagePack (if available)
try:
    import msgpack
    t0=time.perf_counter()
    with open(f'{P}/b.msgpack','wb') as f: f.write(msgpack.packb(data))
    w=time.perf_counter()-t0
    kb = os.path.getsize(f'{P}/b.msgpack')/1024
    t0=time.perf_counter()
    with open(f'{P}/b.msgpack','rb') as f: msgpack.unpackb(f.read())
    r=time.perf_counter()-t0
    results.append(('MsgPack', w*1000, r*1000, kb))
except ImportError:
    pass

# Print results
hdr = f"{'Format':<16} {'Write ms':>10} {'Read ms':>10} {'Size KB':>10} {'vs .kore W':>10} {'vs .kore R':>10}"
print(hdr)
print('-' * len(hdr))
kore_w = results[0][1]
kore_r = results[0][2]
for name, wms, rms, kb in results:
    ws = f"{wms/kore_w:.1f}x" if kore_w > 0 else "-"
    rs = f"{rms/kore_r:.1f}x" if kore_r > 0 else "-"
    marker = " <-- FASTEST" if name == 'KORE .kore' else ""
    print(f"{name:<16} {wms:>10.1f} {rms:>10.1f} {kb:>10.0f} {ws:>10} {rs:>10}{marker}")

print()
print("Lower is better. vs .kore = how many times slower than KORE .kore")
