import sys, os
sys.path.insert(0, 'kore-python')
import kore_fileformat as kore

b = kore.DataBlock()
b.add_column('price', kore.DataType.F64, [1.5, 2.5, 3.5])
b.add_column('qty', kore.DataType.I64, [10, 20, 30])
kore.write_file('C:/tmp/v3test.kore', b)

with open('C:/tmp/v3test.kore', 'rb') as f:
    header = f.read(350).decode('utf-8', errors='replace')
print(header)
print(f"Size: {os.path.getsize('C:/tmp/v3test.kore')} bytes")
b2 = kore.read_file('C:/tmp/v3test.kore')
print(f"Read back: rows={b2.num_rows}, price={list(b2.get_column('price').data)}, qty={list(b2.get_column('qty').data)}")
os.remove('C:/tmp/v3test.kore')
