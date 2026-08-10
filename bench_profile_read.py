"""Find exactly where the 5ms in read_hybrid goes."""
import sys, time, struct, array as _arr
sys.path.insert(0, 'kore-python')
import kore_fileformat as kore

N = 100_000
b = kore.DataBlock()
b.add_column('price', kore.DataType.F64, [float(i)*1.5 for i in range(N)])
b.add_column('qty',   kore.DataType.F64, [float(i) for i in range(N)])
b.add_column('vol',   kore.DataType.F64, [float(i)*2.0 for i in range(N)])
b.add_column('val',   kore.DataType.F64, [float(i)*3.0 for i in range(N)])

import os; os.makedirs('C:/tmp', exist_ok=True)
kore.write_hybrid('C:/tmp/b.hkore', b)

# Warm cache
for _ in range(3):
    kore.read_hybrid('C:/tmp/b.hkore')

# Step-by-step timing
RUNS = 20

def t(label, fn):
    times = [fn() for _ in range(RUNS)]
    ms = min(times) * 1000
    print(f"  {label:<35} {ms:.3f}ms  ({ms*1e6/N:.1f} ns/row)")

path = 'C:/tmp/b.hkore'

# Step 1: open
t("open()", lambda: [open(path,'rb').close(), 0][1] or 0)

# Step 2: open + read 24 bytes
def s2():
    t0 = time.perf_counter()
    with open(path,'rb') as f: f.read(24)
    return time.perf_counter()-t0
t("open + read(24)", s2)

# Step 3: open + seek + read binary
binary_start = 505
def s3():
    t0 = time.perf_counter()
    with open(path,'rb') as f:
        f.read(24)
        f.seek(binary_start)
        raw = f.read()
    return time.perf_counter()-t0
t("open + seek + f.read(3.2MB)", s3)

# Step 4: add struct unpack + schema
def s4():
    t0 = time.perf_counter()
    with open(path,'rb') as f:
        f.read(24); f.seek(binary_start); raw = f.read()
    _m, nrows, ncols = struct.unpack_from('<4sIH', raw, 0)
    off = 10
    for _ in range(ncols):
        db, nl = struct.unpack_from('<BH', raw, off); off += 3
        raw[off:off+nl].decode(); off += nl
    return time.perf_counter()-t0
t("+ struct parse", s4)

# Step 5: add frombytes (no DataBlock)
def s5():
    t0 = time.perf_counter()
    with open(path,'rb') as f:
        f.read(24); f.seek(binary_start); raw = f.read()
    _m, nrows, ncols = struct.unpack_from('<4sIH', raw, 0)
    off = 10
    cols_meta = []
    for _ in range(ncols):
        db, nl = struct.unpack_from('<BH', raw, off); off += 3
        cols_meta.append(raw[off:off+nl].decode()); off += nl
    mv = memoryview(raw)
    for _ in range(ncols):
        a = _arr.array('d'); a.frombytes(mv[off:off+nrows*8]); off += nrows*8
    return time.perf_counter()-t0
t("+ frombytes × 4 (no DataBlock)", s5)

# Step 6: full read_hybrid
def s6():
    t0 = time.perf_counter()
    kore.read_hybrid(path)
    return time.perf_counter()-t0
t("full read_hybrid()", s6)

# Alternative: per-column f.read
def s7():
    t0 = time.perf_counter()
    with open(path,'rb') as f:
        prefix = f.read(24)
        binary_start2 = int(prefix[13:23])
        f.seek(binary_start2)
        k2rw = f.read(10)
        _m, nrows, ncols = struct.unpack('<4sIH', k2rw)
        cols_meta = []
        for _ in range(ncols):
            db, nl = struct.unpack('<BH', f.read(3))
            cols_meta.append((f.read(nl).decode(), db == 0))
        for name, is_f64 in cols_meta:
            col_raw = f.read(nrows * 8)
            a = _arr.array('d' if is_f64 else 'q')
            a.frombytes(col_raw)
    return time.perf_counter()-t0
t("per-col f.read (no DataBlock)", s7)

# Step: DataBlock construction alone
def s8():
    arrays = [_arr.array('d', b.columns[i].data) for i in range(4)]
    t0 = time.perf_counter()
    blk = kore.DataBlock()
    for i, col in enumerate(b.columns):
        blk.add_column(col.name, kore.DataType.F64, arrays[i])
    return time.perf_counter()-t0
t("DataBlock + add_column × 4", s8)

os.remove('C:/tmp/b.hkore')
