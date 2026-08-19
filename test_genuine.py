"""GENUINE real-world test: 1M rows, mixed types, KORE vs Parquet vs ORC"""
import sys, time, os, array, random, string
sys.path.insert(0, 'kore-python')
import kore_fileformat as kore
import pyarrow as pa, pyarrow.parquet as pq, pyarrow.orc as orc

print('=== GENUINE TEST — Real-World Mixed Data, 1M rows ===\n')

N = 1_000_000
random.seed(42)
ids = list(range(1, N+1))
prices = [round(random.uniform(0.01, 9999.99), 2) for _ in range(N)]
quantities = [random.randint(1, 10000) for _ in range(N)]
names = [''.join(random.choices(string.ascii_letters, k=random.randint(5,15))) for _ in range(N)]
cities = [random.choice(['NYC','London','Tokyo','Mumbai','Berlin','Paris','Sydney','LA','SF','Chicago']) for _ in range(N)]
print(f'Data: {N:,} rows x 5 cols (id I64, price F64, qty I64, name STR, city STR)\n')

b = kore.DataBlock()
b.add_column('id', kore.DataType.I64, ids)
b.add_column('price', kore.DataType.F64, prices)
b.add_column('qty', kore.DataType.I64, quantities)
b.add_column('name', kore.DataType.STR, names)
b.add_column('city', kore.DataType.STR, cities)

R = []
# KORE .kore zstd
t0=time.perf_counter(); kore.write_kore('C:/tmp/g.kore', b); w=(time.perf_counter()-t0)*1000
t0=time.perf_counter(); d=kore.read_kore('C:/tmp/g.kore'); r=(time.perf_counter()-t0)*1000
assert d.num_rows==N and len(d.columns)==5
R.append(('KORE .kore (zstd)', w, r, os.path.getsize('C:/tmp/g.kore')/1024))

# KORE .hkore
t0=time.perf_counter(); kore.write_hybrid('C:/tmp/g.hkore', b); w=(time.perf_counter()-t0)*1000
t0=time.perf_counter(); d2=kore.read_hybrid('C:/tmp/g.hkore'); r=(time.perf_counter()-t0)*1000
assert d2.num_rows==N
R.append(('KORE .hkore', w, r, os.path.getsize('C:/tmp/g.hkore')/1024))

# Parquet snappy
table = pa.table({'id':ids,'price':prices,'qty':quantities,'name':names,'city':cities})
t0=time.perf_counter(); pq.write_table(table,'C:/tmp/g_s.pq',compression='SNAPPY'); w=(time.perf_counter()-t0)*1000
t0=time.perf_counter(); pq.read_table('C:/tmp/g_s.pq'); r=(time.perf_counter()-t0)*1000
R.append(('Parquet (snappy)', w, r, os.path.getsize('C:/tmp/g_s.pq')/1024))

# Parquet zstd
t0=time.perf_counter(); pq.write_table(table,'C:/tmp/g_z.pq',compression='ZSTD'); w=(time.perf_counter()-t0)*1000
t0=time.perf_counter(); pq.read_table('C:/tmp/g_z.pq'); r=(time.perf_counter()-t0)*1000
R.append(('Parquet (zstd)', w, r, os.path.getsize('C:/tmp/g_z.pq')/1024))

# ORC
t0=time.perf_counter(); orc.write_table(table,'C:/tmp/g.orc'); w=(time.perf_counter()-t0)*1000
t0=time.perf_counter(); orc.read_table('C:/tmp/g.orc'); r=(time.perf_counter()-t0)*1000
R.append(('ORC', w, r, os.path.getsize('C:/tmp/g.orc')/1024))

hdr = "  {:<22} {:>10} {:>10} {:>10}".format('Format','Write ms','Read ms','Size KB')
print(hdr)
print("  " + "-"*55)
for n,w,r,k in R:
    print("  {:<22} {:>10.0f} {:>10.0f} {:>10.0f}".format(n,w,r,k))

print()
print('Roundtrip: .kore={} rows OK, .hkore={} rows OK'.format(d.num_rows, d2.num_rows))
print()
# Winners
sizes = [(n,k) for n,w,r,k in R]
reads = [(n,r) for n,w,r,k in R]
writes = [(n,w) for n,w,r,k in R]
print('SMALLEST: {} ({:.0f} KB)'.format(*min(sizes, key=lambda x:x[1])))
print('FASTEST READ: {} ({:.0f} ms)'.format(*min(reads, key=lambda x:x[1])))
print('FASTEST WRITE: {} ({:.0f} ms)'.format(*min(writes, key=lambda x:x[1])))
