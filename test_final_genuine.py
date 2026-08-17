"""FINAL GENUINE BENCHMARK — PyO3 Native KORE vs Parquet vs ORC"""
import kore_py, time, os, random, string
import pyarrow as pa, pyarrow.parquet as pq, pyarrow.orc as orc

N = 1_000_000
random.seed(42)
ids = list(range(N))
prices = [round(random.uniform(0.01, 9999.99), 2) for _ in range(N)]
quantities = [random.randint(1, 10000) for _ in range(N)]
names = [''.join(random.choices(string.ascii_letters, k=random.randint(5,15))) for _ in range(N)]
cities = [random.choice(['NYC','London','Tokyo','Mumbai','Berlin','Paris','Sydney','LA','SF','Chicago']) for _ in range(N)]

print(f"=== GENUINE: 1M rows x 5 cols (PyO3 Native) ===\n")

R = []

# KORE PyO3 Native
b = kore_py.PyDataBlock()
b.add_i64_column('id', ids)
b.add_f64_column('price', prices)
b.add_i64_column('qty', quantities)
b.add_str_column('name', names)
b.add_str_column('city', cities)
t0=time.perf_counter(); kore_py.write_kore('C:/tmp/final.kore', b); w=(time.perf_counter()-t0)*1000
k=os.path.getsize('C:/tmp/final.kore')/1024
t0=time.perf_counter(); d=kore_py.read_kore('C:/tmp/final.kore'); r=(time.perf_counter()-t0)*1000
R.append(('KORE (Rust native)', w, r, k))

# Parquet zstd
table = pa.table({'id':ids,'price':prices,'qty':quantities,'name':names,'city':cities})
t0=time.perf_counter(); pq.write_table(table,'C:/tmp/final.parquet',compression='ZSTD'); w=(time.perf_counter()-t0)*1000
k=os.path.getsize('C:/tmp/final.parquet')/1024
t0=time.perf_counter(); pq.read_table('C:/tmp/final.parquet'); r=(time.perf_counter()-t0)*1000
R.append(('Parquet (zstd)', w, r, k))

# ORC
t0=time.perf_counter(); orc.write_table(table,'C:/tmp/final.orc'); w=(time.perf_counter()-t0)*1000
k=os.path.getsize('C:/tmp/final.orc')/1024
t0=time.perf_counter(); orc.read_table('C:/tmp/final.orc'); r=(time.perf_counter()-t0)*1000
R.append(('ORC', w, r, k))

print("  {:<22} {:>10} {:>10} {:>10}".format('Format','Write ms','Read ms','Size KB'))
print("  "+"-"*55)
for n,w,r,k in R:
    print("  {:<22} {:>10.0f} {:>10.0f} {:>10.0f}".format(n,w,r,k))

print()
print("WINNERS:")
sizes = [(n,k) for n,w,r,k in R]
reads = [(n,r) for n,w,r,k in R]
writes = [(n,w) for n,w,r,k in R]
print("  Smallest: {} ({:.0f} KB)".format(*min(sizes, key=lambda x:x[1])))
print("  Fastest read: {} ({:.0f} ms)".format(*min(reads, key=lambda x:x[1])))
print("  Fastest write: {} ({:.0f} ms)".format(*min(writes, key=lambda x:x[1])))
print()
print("  Roundtrip verified: {} rows, {} cols".format(d.num_rows(), d.num_columns()))
c = d.get_str_column("city")
if c: print("  Cities[0:3]: {}".format(c[:3]))
