import kore_py
d = kore_py.read_kore('C:/tmp/dbg2.kore')
print(f'Read: {d.num_rows()} rows, {d.num_columns()} cols')
print(f'Cities: {d.get_str_column("city")}')
print(f'IDs: {d.get_i64_column("id")}')
print()

# Now 1M test
import time, os, random, string
N = 1_000_000
cities = ['NYC','London','Tokyo','Mumbai','Berlin'] * (N//5)
ids = list(range(N))
b = kore_py.PyDataBlock()
b.add_str_column('city', cities)
b.add_i64_column('id', ids)
t0=time.perf_counter(); kore_py.write_kore('C:/tmp/fix_1m.kore', b); wms=(time.perf_counter()-t0)*1000
kb = os.path.getsize('C:/tmp/fix_1m.kore')/1024
print(f'1M Write: {wms:.0f}ms Size: {kb:.0f}KB')
t0=time.perf_counter(); d2=kore_py.read_kore('C:/tmp/fix_1m.kore'); rms=(time.perf_counter()-t0)*1000
print(f'1M Read: {rms:.0f}ms Rows: {d2.num_rows()}')
print(f'Cities[0:5]: {d2.get_str_column("city")[:5]}')
print(f'IDs[0:5]: {d2.get_i64_column("id")[:5]}')
