import sys, time, os
sys.path.insert(0, 'kore-python')
import kore_fileformat as kore

N = 100_000
b = kore.DataBlock()
b.add_column('price', kore.DataType.F64, [float(i)*1.5 for i in range(N)])
b.add_column('qty',   kore.DataType.F64, [float(i) for i in range(N)])
b.add_column('vol',   kore.DataType.F64, [float(i)*2.0 for i in range(N)])
b.add_column('val',   kore.DataType.F64, [float(i)*3.0 for i in range(N)])

path = 'C:/tmp/bench.hkore'
os.makedirs('C:/tmp', exist_ok=True)

# Warm up
kore.write_hybrid(path, b)
kore.read_hybrid(path)

# Write benchmark
times = []
for _ in range(5):
    t = time.perf_counter()
    kore.write_hybrid(path, b)
    times.append((time.perf_counter()-t)*1000)
w = min(times)
print(f'write_hybrid: {w:.1f}ms  ({w*1e6/N:.0f} ns/row)  [was 134ms / 1340 ns/row]')

# Read benchmark (warm cache)
times = []
for _ in range(5):
    t = time.perf_counter()
    b2 = kore.read_hybrid(path)
    times.append((time.perf_counter()-t)*1000)
r = min(times)
print(f'read_hybrid:  {r:.1f}ms  ({r*1e6/N:.0f} ns/row)  [was 35ms / 350 ns/row]')

# Round-trip: read->write (array.array already — max speed)
times = []
for _ in range(5):
    t = time.perf_counter()
    b3 = kore.read_hybrid(path)
    kore.write_hybrid(path, b3)
    times.append((time.perf_counter()-t)*1000)
rt = min(times)
print(f'round-trip:   {rt:.1f}ms  ({rt*1e6/N:.0f} ns/row)  [array.array path]')

# Correctness
b4 = kore.read_hybrid(path)
col = b4.get_column('price')
print(f'Correct: rows={b4.num_rows}, price[0]={col.data[0]}, price[99999]={col.data[99999]}')
print(f'Data type: {type(col.data).__name__}  (array.array = no tolist overhead)')

stats = kore.hkore_stats(path)
print(f'File: {stats["total_kb"]:.0f} KB  header={stats["header_kb"]:.1f} KB  overhead={stats["overhead_pct"]:.2f}%')

# inspect
print()
kore.inspect_hybrid(path)

os.remove(path)
