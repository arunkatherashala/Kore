"""KORE vs Parquet vs ORC — 1 BILLION cells (50M rows x 20 cols)"""
import kore_py, time, os, random, string
import pyarrow as pa, pyarrow.parquet as pq, pyarrow.orc as orc

# 50M rows x 20 cols = 1 billion cells
N = 10_000_000
NCOLS_NUM = 15
NCOLS_STR = 5

print(f"=== 1 BILLION CELLS: {N:,} rows x 20 cols ({NCOLS_NUM} numeric + {NCOLS_STR} string) ===\n")
print("Generating data...")

random.seed(42)

# Generate columns
num_cols = {}
for i in range(10):
    num_cols[f'metric_{i}'] = [random.uniform(0, 10000) for _ in range(N)]
for i in range(5):
    num_cols[f'count_{i}'] = [random.randint(0, 1000000) for _ in range(N)]

str_values = ['Alpha','Beta','Gamma','Delta','Epsilon','Zeta','Eta','Theta','Iota','Kappa',
              'Lambda','Mu','Nu','Xi','Omicron','Pi','Rho','Sigma','Tau','Upsilon']
str_cols = {}
for i in range(5):
    str_cols[f'label_{i}'] = [random.choice(str_values) for _ in range(N)]

print(f"Data generated: {N*20/1e9:.1f} billion cells\n")

R = []

# === KORE (Rust PyO3) ===
print("[1/3] KORE writing...")
b = kore_py.PyDataBlock()
for name, data in num_cols.items():
    if 'metric' in name:
        b.add_f64_column(name, data)
    else:
        b.add_i64_column(name, data)
for name, data in str_cols.items():
    b.add_str_column(name, data)

t0 = time.perf_counter()
kore_py.write_kore('C:/tmp/billion.kore', b)
kw = (time.perf_counter() - t0) * 1000
ksz = os.path.getsize('C:/tmp/billion.kore') / (1024*1024)
print(f"  KORE write: {kw:.0f}ms  Size: {ksz:.0f}MB")

print("[1/3] KORE reading...")
t0 = time.perf_counter()
d = kore_py.read_kore('C:/tmp/billion.kore')
kr = (time.perf_counter() - t0) * 1000
print(f"  KORE read: {kr:.0f}ms  Rows: {d.num_rows()}")
R.append(('KORE (Rust)', kw, kr, ksz))

# === Parquet zstd ===
print("[2/3] Parquet writing...")
all_data = {}
all_data.update(num_cols)
all_data.update(str_cols)
table = pa.table(all_data)

t0 = time.perf_counter()
pq.write_table(table, 'C:/tmp/billion.parquet', compression='ZSTD')
pw = (time.perf_counter() - t0) * 1000
psz = os.path.getsize('C:/tmp/billion.parquet') / (1024*1024)
print(f"  Parquet write: {pw:.0f}ms  Size: {psz:.0f}MB")

print("[2/3] Parquet reading...")
t0 = time.perf_counter()
pq.read_table('C:/tmp/billion.parquet')
pr = (time.perf_counter() - t0) * 1000
print(f"  Parquet read: {pr:.0f}ms")
R.append(('Parquet (zstd)', pw, pr, psz))

# === ORC ===
print("[3/3] ORC writing...")
t0 = time.perf_counter()
orc.write_table(table, 'C:/tmp/billion.orc')
ow = (time.perf_counter() - t0) * 1000
osz = os.path.getsize('C:/tmp/billion.orc') / (1024*1024)
print(f"  ORC write: {ow:.0f}ms  Size: {osz:.0f}MB")

print("[3/3] ORC reading...")
t0 = time.perf_counter()
orc.read_table('C:/tmp/billion.orc')
orr = (time.perf_counter() - t0) * 1000
print(f"  ORC read: {orr:.0f}ms")
R.append(('ORC', ow, orr, osz))

# === RESULTS ===
print(f"\n{'='*60}")
print(f"  RESULTS: {N:,} rows x 20 cols = {N*20/1e9:.1f} BILLION cells")
print(f"{'='*60}")
print("\n  {:<20} {:>10} {:>10} {:>10}".format('Format','Write ms','Read ms','Size MB'))
print("  "+"-"*52)
for n,w,r,k in R:
    print("  {:<20} {:>10,.0f} {:>10,.0f} {:>10,.0f}".format(n,w,r,k))

print("\n  WINNERS:")
print("  Fastest write: {}".format(min(R, key=lambda x:x[1])[0]))
print("  Fastest read:  {}".format(min(R, key=lambda x:x[2])[0]))
print("  Smallest file: {}".format(min(R, key=lambda x:x[3])[0]))
