import kore_py, time, os, random, string

print("=== KORE PyO3 Native Module Test ===")

# Basic test
b = kore_py.PyDataBlock()
b.add_str_column("city", ["NYC","London","Tokyo"])
b.add_f64_column("price", [100.5, 200.3, 300.7])
b.add_i64_column("qty", [10, 20, 30])
kore_py.write_kore("C:/tmp/native.kore", b)
d = kore_py.read_kore("C:/tmp/native.kore")
print(f"Basic: {d.num_rows()} rows, {d.num_columns()} cols")
print(f"Cities: {d.get_str_column('city')}")
print(f"Prices: {d.get_f64_column('price')}")
print()

# Performance test: 1M rows with strings
N = 1_000_000
random.seed(42)
ids = list(range(N))
prices = [round(random.uniform(0.01, 9999.99), 2) for _ in range(N)]
quantities = [random.randint(1, 10000) for _ in range(N)]
names = [''.join(random.choices(string.ascii_letters, k=random.randint(5,15))) for _ in range(N)]
cities = [random.choice(['NYC','London','Tokyo','Mumbai','Berlin','Paris','Sydney','LA','SF','Chicago']) for _ in range(N)]

print(f"=== 1M Rows Benchmark (PyO3 Native) ===")

b2 = kore_py.PyDataBlock()
b2.add_i64_column("id", ids)
b2.add_f64_column("price", prices)
b2.add_i64_column("qty", quantities)
b2.add_str_column("name", names)
b2.add_str_column("city", cities)

t0 = time.perf_counter()
kore_py.write_kore("C:/tmp/native_1m.kore", b2)
wms = (time.perf_counter() - t0) * 1000

t0 = time.perf_counter()
d2 = kore_py.read_kore("C:/tmp/native_1m.kore")
rms = (time.perf_counter() - t0) * 1000

kb = os.path.getsize("C:/tmp/native_1m.kore") / 1024
print(f"  PyO3 NATIVE: W={wms:.0f}ms R={rms:.0f}ms Size={kb:.0f}KB ({rms*1e6/N:.0f} ns/row)")
print(f"  Roundtrip: {d2.num_rows()} rows, {d2.num_columns()} cols")
print()

# Compare with Parquet
import pyarrow as pa, pyarrow.parquet as pq
table = pa.table({'id':ids,'price':prices,'qty':quantities,'name':names,'city':cities})
t0 = time.perf_counter()
pq.write_table(table, 'C:/tmp/native_1m.parquet', compression='ZSTD')
pwms = (time.perf_counter() - t0) * 1000
t0 = time.perf_counter()
pq.read_table('C:/tmp/native_1m.parquet')
prms = (time.perf_counter() - t0) * 1000
pkb = os.path.getsize('C:/tmp/native_1m.parquet') / 1024

print(f"  Parquet ZSTD: W={pwms:.0f}ms R={prms:.0f}ms Size={pkb:.0f}KB ({prms*1e6/N:.0f} ns/row)")
print()
print(f"  KORE vs Parquet:")
print(f"    Write: {pwms/wms:.1f}x {'KORE faster' if wms < pwms else 'Parquet faster'}")
print(f"    Read:  {prms/rms:.1f}x {'KORE faster' if rms < prms else 'Parquet faster'}")
print(f"    Size:  KORE {kb:.0f}KB vs Parquet {pkb:.0f}KB")
