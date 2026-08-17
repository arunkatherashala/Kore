import kore_py, os

print("=== Debug decode_strs ===")

# 1M rows, low cardinality strings (should compress well)
N = 1_000_000
cities = ['NYC','London','Tokyo','Mumbai','Berlin'] * (N // 5)
ids = list(range(N))

b = kore_py.PyDataBlock()
b.add_str_column('city', cities)
b.add_i64_column('id', ids)

kore_py.write_kore('C:/tmp/dbg.kore', b)
sz = os.path.getsize('C:/tmp/dbg.kore')
print(f"Written: {N} rows, {sz/1024:.0f} KB")

# Try read
try:
    d = kore_py.read_kore('C:/tmp/dbg.kore')
    print(f"Read OK: {d.num_rows()} rows, {d.num_columns()} cols")
    print(f"Cities[0:5]: {d.get_str_column('city')[:5]}")
except Exception as e:
    print(f"READ FAILED: {e}")

# Try with bytes to see raw structure
with open('C:/tmp/dbg.kore', 'rb') as f:
    raw = f.read()
print(f"\nFile analysis:")
print(f"  Magic: {raw[:4]}")
print(f"  Total bytes: {len(raw)}")

# Parse header manually
import struct
pos = 4
ver = struct.unpack_from('<H', raw, pos)[0]; pos += 2
ncols = struct.unpack_from('<I', raw, pos)[0]; pos += 4
nrows = struct.unpack_from('<Q', raw, pos)[0]; pos += 8
print(f"  Version: {ver}, Cols: {ncols}, Rows: {nrows}")

for i in range(ncols):
    nl = struct.unpack_from('<H', raw, pos)[0]; pos += 2
    name = raw[pos:pos+nl].decode(); pos += nl
    dtype = raw[pos]; pos += 1
    print(f"  Col {i}: '{name}' dtype={dtype}")

# Column data sections
for i in range(ncols):
    comp = raw[pos]; pos += 1
    data_len = struct.unpack_from('<Q', raw, pos)[0]; pos += 8
    print(f"  Col {i} data: comp={comp} data_len={data_len} (ends at {pos+data_len})")
    pos += data_len

print(f"  Final pos: {pos} / {len(raw)} (remaining: {len(raw)-pos} bytes)")
