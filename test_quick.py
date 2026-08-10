import sys; sys.path.insert(0,'kore-python')
import kore_fileformat as kore, os
os.makedirs('C:/tmp', exist_ok=True)

# 1. BOOL type fix needed
b = kore.DataBlock()
b.add_column('flag', kore.DataType.BOOL, [True, False, True])
kore.write_file('C:/tmp/x.kore', b)
b2 = kore.read_file('C:/tmp/x.kore')
d = list(b2.get_column('flag').data)
print('BOOL type:', type(d[0]).__name__, '← should be bool')
print('BOOL values:', d, '← should be [True, False, True]')

# 2. 1K unique string round-trip
b = kore.DataBlock()
b.add_column('key', kore.DataType.STR, [f'key_{i}' for i in range(1000)])
b.add_column('v',   kore.DataType.F64, [float(i) for i in range(1000)])
kore.write_file('C:/tmp/x.kore', b)
b2 = kore.read_file('C:/tmp/x.kore')
print('1K strings rows:', b2.num_rows, '← should be 1000')
print('1K strings last:', b2.get_column('key').data[999], '← should be key_999')

os.remove('C:/tmp/x.kore')
