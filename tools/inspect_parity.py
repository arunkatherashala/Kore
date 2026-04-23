with open('../kore_fileformat_killer/parity_delta_test.killer','rb') as f:
    data = f.read()
print('LEN', len(data))
lines = data.splitlines()
for i,l in enumerate(lines, start=1):
    print(i, repr(l))
