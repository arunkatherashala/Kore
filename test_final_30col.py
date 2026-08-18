"""FINAL BENCHMARK: 10M rows x 30 cols — KORE vs Parquet vs ORC (side by side)"""
import kore_py, time, os, random
import pyarrow as pa, pyarrow.parquet as pq, pyarrow.orc as orc

N = 10_000_000
random.seed(42)
print(f'=== FINAL: {N:,} rows x 30 cols — KORE vs Parquet vs ORC ===')
print('Generating data...')

f_cols = {f'metric_{i}': [random.uniform(0,10000) for _ in range(N)] for i in range(15)}
i_cols = {f'count_{i}': [random.randint(0,1000000) for _ in range(N)] for i in range(10)}
labels = ['Alpha','Beta','Gamma','Delta','Epsilon','Zeta','Eta','Theta','Iota','Kappa','Lambda','Mu','Nu','Xi','Omicron','Pi','Rho','Sigma','Tau','Upsilon']
s_cols = {f'label_{i}': [random.choice(labels) for _ in range(N)] for i in range(5)}
print(f'Data ready: {N:,} x 30 = {N*30/1e6:.0f}M cells\n')

R = []

# KORE
b = kore_py.PyDataBlock()
for k,v in f_cols.items(): b.add_f64_column(k,v)
for k,v in i_cols.items(): b.add_i64_column(k,v)
for k,v in s_cols.items(): b.add_str_column(k,v)

print("[KORE] Writing...")
t0=time.perf_counter(); kore_py.write_kore('C:/tmp/final30.kore',b); kw=(time.perf_counter()-t0)*1000
ksz=os.path.getsize('C:/tmp/final30.kore')/(1024*1024)
print("[KORE] Reading...")
t0=time.perf_counter(); kore_py.read_kore('C:/tmp/final30.kore'); kr=(time.perf_counter()-t0)*1000
R.append(('KORE (Rust)', kw, kr, ksz))
print(f"  KORE:    W={kw:.0f}ms  R={kr:.0f}ms  Size={ksz:.0f}MB")

# Parquet ZSTD
all_data = {}; all_data.update(f_cols); all_data.update(i_cols); all_data.update(s_cols)
table = pa.table(all_data)
print("[Parquet] Writing...")
t0=time.perf_counter(); pq.write_table(table,'C:/tmp/final30.parquet',compression='ZSTD'); pw=(time.perf_counter()-t0)*1000
psz=os.path.getsize('C:/tmp/final30.parquet')/(1024*1024)
print("[Parquet] Reading...")
t0=time.perf_counter(); pq.read_table('C:/tmp/final30.parquet'); pr=(time.perf_counter()-t0)*1000
R.append(('Parquet (zstd)', pw, pr, psz))
print(f"  Parquet: W={pw:.0f}ms  R={pr:.0f}ms  Size={psz:.0f}MB")

# ORC
print("[ORC] Writing...")
t0=time.perf_counter(); orc.write_table(table,'C:/tmp/final30.orc'); ow=(time.perf_counter()-t0)*1000
osz=os.path.getsize('C:/tmp/final30.orc')/(1024*1024)
print("[ORC] Reading...")
t0=time.perf_counter(); orc.read_table('C:/tmp/final30.orc'); orr=(time.perf_counter()-t0)*1000
R.append(('ORC', ow, orr, osz))
print(f"  ORC:     W={ow:.0f}ms  R={orr:.0f}ms  Size={osz:.0f}MB")

# Results
print(f"\n{'='*60}")
print(f"  FINAL RESULTS: {N:,} rows x 30 cols ({N*30/1e6:.0f}M cells)")
print(f"{'='*60}")
print(f"\n  {'Format':<18} {'Write ms':>10} {'Read ms':>10} {'Size MB':>10}")
print(f"  {'-'*50}")
for n,w,r,s in R:
    print(f"  {n:<18} {w:>10,.0f} {r:>10,.0f} {s:>10,.0f}")

print(f"\n  WINNERS:")
w_win = min(R, key=lambda x:x[1])[0]
r_win = min(R, key=lambda x:x[2])[0]
s_win = min(R, key=lambda x:x[3])[0]
print(f"    Fastest write: {w_win}")
print(f"    Fastest read:  {r_win}")
print(f"    Smallest file: {s_win}")
