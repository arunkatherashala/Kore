"""Comprehensive correctness + performance test for .hkore and .kore."""
import sys, time, os, array as _arr
sys.path.insert(0, 'kore-python')
import kore_fileformat as kore

os.makedirs('C:/tmp', exist_ok=True)
PASS = 0; FAIL = 0

def check(label, got, expected):
    global PASS, FAIL
    if got == expected:
        print(f"  OK  {label}")
        PASS += 1
    else:
        print(f"  FAIL {label}: got={repr(got)[:60]}  expected={repr(expected)[:60]}")
        FAIL += 1

def close_enough(a, b, tol=1e-9):
    return all(abs(x-y) < tol for x,y in zip(a,b))

print("=" * 60)
print("CORRECTNESS TESTS")
print("=" * 60)

# --- 1. Basic F64 round-trip ---
b = kore.DataBlock()
b.add_column('x', kore.DataType.F64, [1.5, 2.5, 3.5])
b.add_column('y', kore.DataType.I64, [10, 20, 30])

kore.write_hybrid('C:/tmp/t.hkore', b)
kore.write_file('C:/tmp/t.kore', b)

h = kore.read_hybrid('C:/tmp/t.hkore')
k = kore.read_file('C:/tmp/t.kore')

check('hkore rows', h.num_rows, 3)
check('kore rows',  k.num_rows, 3)
check('hkore F64', list(h.get_column('x').data), [1.5, 2.5, 3.5])
check('kore F64',  list(k.get_column('x').data), [1.5, 2.5, 3.5])
check('hkore I64', list(h.get_column('y').data), [10, 20, 30])
check('kore I64',  list(k.get_column('y').data), [10, 20, 30])

# --- 2. Large round-trip ---
N = 50_000
b2 = kore.DataBlock()
b2.add_column('price', kore.DataType.F64, [float(i)*1.5 for i in range(N)])
b2.add_column('vol',   kore.DataType.I64, [i*2 for i in range(N)])

kore.write_hybrid('C:/tmp/big.hkore', b2)
kore.write_file('C:/tmp/big.kore', b2)

h2 = kore.read_hybrid('C:/tmp/big.hkore')
k2 = kore.read_file('C:/tmp/big.kore')

check('hkore large rows', h2.num_rows, N)
check('kore large rows',  k2.num_rows, N)
check('hkore price[0]',   h2.get_column('price').data[0], 0.0)
check('hkore price[-1]',  h2.get_column('price').data[N-1], float(N-1)*1.5)
check('kore price[0]',    list(k2.get_column('price').data[:1]), [0.0])
check('hkore vol[-1]',    h2.get_column('vol').data[N-1], (N-1)*2)

# --- 3. hkore col.data type is array.array (no tolist overhead) ---
check('hkore data type', type(h2.get_column('price').data).__name__, 'array')
check('hkore I64 data type', type(h2.get_column('vol').data).__name__, 'array')

# --- 4. Round-trip: hkore → hkore (array.array fast path) ---
h3 = kore.read_hybrid('C:/tmp/big.hkore')
kore.write_hybrid('C:/tmp/big2.hkore', h3)
h4 = kore.read_hybrid('C:/tmp/big2.hkore')
check('hkore→hkore round-trip rows',      h4.num_rows, N)
check('hkore→hkore round-trip price[-1]', h4.get_column('price').data[N-1], float(N-1)*1.5)

# --- 5. inspect_hybrid ---
header = kore.read_hybrid_header('C:/tmp/big.hkore')
check('header starts with KORE2', header[:5], 'KORE2')
check('header has schema', '# Schema:' in header, True)

# --- 6. hkore_stats ---
stats = kore.hkore_stats('C:/tmp/big.hkore')
check('stats format', stats['format'], 'v2-raw')
check('stats overhead < 1%', stats['overhead_pct'] < 1.0, True)
check('stats header < 1KB', stats['header_kb'] < 1.0, True)

# --- 7. Multi-column ---
b3 = kore.DataBlock()
for i in range(8):
    b3.add_column(f'col{i}', kore.DataType.F64, [float(i+j) for j in range(100)])
kore.write_hybrid('C:/tmp/wide.hkore', b3)
h5 = kore.read_hybrid('C:/tmp/wide.hkore')
check('8-col hkore', h5.num_columns, 8)
check('8-col values', h5.get_column('col7').data[99], 7.0+99)

# --- 8. kore backward compat ---
kore.write_file('C:/tmp/compat.kore', b)
k3 = kore.read_file('C:/tmp/compat.kore')
check('kore compat', k3.num_rows, 3)

print()
print("=" * 60)
print("PERFORMANCE TESTS (100K rows × 4 cols)")
print("=" * 60)

N = 100_000
bp = kore.DataBlock()
bp.add_column('price', kore.DataType.F64, [float(i)*1.5 for i in range(N)])
bp.add_column('qty',   kore.DataType.F64, [float(i) for i in range(N)])
bp.add_column('vol',   kore.DataType.F64, [float(i)*2.0 for i in range(N)])
bp.add_column('val',   kore.DataType.I64, [i*3 for i in range(N)])

# Warm up
kore.write_hybrid('C:/tmp/p.hkore', bp)
kore.write_file('C:/tmp/p.kore', bp)
for _ in range(3): kore.read_hybrid('C:/tmp/p.hkore')
for _ in range(3): kore.read_file('C:/tmp/p.kore')

def perf(label, fn, runs=5):
    ts = [fn() for _ in range(runs)]
    ms = min(ts)*1000
    print(f"  {label:<30} {ms:6.1f}ms  {ms*1e6/N:6.0f} ns/row")

perf('write_hybrid',     lambda: [kore.write_hybrid('C:/tmp/p.hkore', bp), time.perf_counter()][1] - time.perf_counter() or _perf_write_hybrid(bp))
perf('read_hybrid',      lambda: [time.perf_counter(), kore.read_hybrid('C:/tmp/p.hkore'), time.perf_counter()][2] - [time.perf_counter(), kore.read_hybrid('C:/tmp/p.hkore'), time.perf_counter()][0])

def _pw():
    t=time.perf_counter(); kore.write_hybrid('C:/tmp/p.hkore', bp); return time.perf_counter()-t
def _pr():
    t=time.perf_counter(); kore.read_hybrid('C:/tmp/p.hkore'); return time.perf_counter()-t
def _kw():
    t=time.perf_counter(); kore.write_file('C:/tmp/p.kore', bp); return time.perf_counter()-t
def _kr():
    t=time.perf_counter(); kore.read_file('C:/tmp/p.kore'); return time.perf_counter()-t

ts=[_pw() for _ in range(7)]; ms=min(ts)*1000; print(f"  {'write_hybrid':<30} {ms:6.1f}ms  {ms*1e6/N:6.0f} ns/row")
ts=[_pr() for _ in range(7)]; ms=min(ts)*1000; print(f"  {'read_hybrid':<30} {ms:6.1f}ms  {ms*1e6/N:6.0f} ns/row")
ts=[_kw() for _ in range(7)]; ms=min(ts)*1000; print(f"  {'write_file (.kore)':<30} {ms:6.1f}ms  {ms*1e6/N:6.0f} ns/row")
ts=[_kr() for _ in range(7)]; ms=min(ts)*1000; print(f"  {'read_file (.kore)':<30} {ms:6.1f}ms  {ms*1e6/N:6.0f} ns/row")

# Clean up
for f in ['t.hkore','t.kore','big.hkore','big.kore','big2.hkore','wide.hkore','compat.kore','p.hkore','p.kore']:
    try: os.remove(f'C:/tmp/{f}')
    except: pass

print()
print(f"RESULT: {PASS} passed, {FAIL} failed")
