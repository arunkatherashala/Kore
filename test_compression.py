import sys, time, os, array
sys.path.insert(0, 'kore-python')
import kore_fileformat as kore
import pyarrow as pa, pyarrow.parquet as pq

N = 1_000_000
prices = array.array('d', (float(i)*1.5 for i in range(N)))
volumes = array.array('q', range(N))

b = kore.DataBlock()
b.add_column('price', kore.DataType.F64, prices)
b.add_column('vol', kore.DataType.I64, volumes)

results = []

# .kore zstd
t0=time.perf_counter(); kore.write_kore('C:/tmp/c.kore', b, compression='zstd'); w=(time.perf_counter()-t0)*1000
t0=time.perf_counter(); kore.read_kore('C:/tmp/c.kore'); r=(time.perf_counter()-t0)*1000
results.append(('KORE .kore (zstd)', w, r, os.path.getsize('C:/tmp/c.kore')/1024))

# .kore raw
t0=time.perf_counter(); kore.write_kore('C:/tmp/r.kore', b, compression=None); w=(time.perf_counter()-t0)*1000
t0=time.perf_counter(); kore.read_kore('C:/tmp/r.kore'); r=(time.perf_counter()-t0)*1000
results.append(('KORE .kore (raw)', w, r, os.path.getsize('C:/tmp/r.kore')/1024))

# .hkore
t0=time.perf_counter(); kore.write_hybrid('C:/tmp/c.hkore', b); w=(time.perf_counter()-t0)*1000
t0=time.perf_counter(); kore.read_hybrid('C:/tmp/c.hkore'); r=(time.perf_counter()-t0)*1000
results.append(('KORE .hkore', w, r, os.path.getsize('C:/tmp/c.hkore')/1024))

# Parquet snappy
table = pa.table({'price': prices, 'vol': volumes})
t0=time.perf_counter(); pq.write_table(table, 'C:/tmp/c.parquet', compression='SNAPPY'); w=(time.perf_counter()-t0)*1000
t0=time.perf_counter(); pq.read_table('C:/tmp/c.parquet'); r=(time.perf_counter()-t0)*1000
results.append(('Parquet (snappy)', w, r, os.path.getsize('C:/tmp/c.parquet')/1024))

# Parquet zstd
t0=time.perf_counter(); pq.write_table(table, 'C:/tmp/cz.parquet', compression='ZSTD'); w=(time.perf_counter()-t0)*1000
t0=time.perf_counter(); pq.read_table('C:/tmp/cz.parquet'); r=(time.perf_counter()-t0)*1000
results.append(('Parquet (zstd)', w, r, os.path.getsize('C:/tmp/cz.parquet')/1024))

print(f"=== 1M ROWS — KORE vs Parquet (SIZE + SPEED) ===")
print()
hdr = f"  {'Format':<22} {'Write ms':>10} {'Read ms':>10} {'Size KB':>10} {'ns/row':>8}"
print(hdr)
print("  " + "-" * 62)
for name, wms, rms, kb in results:
    print(f"  {name:<22} {wms:>10.0f} {rms:>10.0f} {kb:>10.0f} {rms*1e6/N:>8.0f}")

print()
kz = results[0]  # kore zstd
pz = results[4]  # parquet zstd
print(f"  KORE zstd vs Parquet zstd:")
print(f"    Size:  KORE {kz[3]:.0f}KB vs Parquet {pz[3]:.0f}KB")
print(f"    Read:  KORE {kz[2]:.0f}ms vs Parquet {pz[2]:.0f}ms ({pz[2]/kz[2]:.1f}x faster)")
print(f"    Write: KORE {kz[1]:.0f}ms vs Parquet {pz[1]:.0f}ms ({pz[1]/kz[1]:.1f}x faster)")
