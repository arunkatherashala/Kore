import kore_py, struct, zstandard as zstd

b = kore_py.PyDataBlock()
b.add_str_column('city', ['NYC','London','Tokyo'])
b.add_i64_column('id', [1,2,3])
kore_py.write_kore('C:/tmp/small.kore', b)

with open('C:/tmp/small.kore','rb') as f:
    raw = f.read()
print(f'Total: {len(raw)} bytes')
pos = 4+2+4+8
for i in range(2):
    nl = struct.unpack_from('<H',raw,pos)[0]; pos+=2
    name = raw[pos:pos+nl].decode(); pos+=nl
    dt = raw[pos]; pos+=1
    print(f'Col {i}: {name} dtype={dt}')

comp = raw[pos]; pos+=1
dlen = struct.unpack_from('<Q',raw,pos)[0]; pos+=8
print(f'\nCol 0: comp={comp} dlen={dlen}')
col0 = raw[pos:pos+dlen]

if comp == 6:
    inner = col0[0]
    print(f'Inner comp: {inner}')
    dec = zstd.ZstdDecompressor().decompress(col0[1:])
    print(f'Decompressed: {len(dec)} bytes')
    print(f'Hex: {dec.hex()}')
    n = struct.unpack_from('<I', dec, 0)[0]
    print(f'n={n}')
    nulls = dec[4:4+n]
    print(f'null_flags: {list(nulls)}')
    off_start = 4+n
    off_end = off_start + (n+1)*4
    for i in range(n+1):
        o = struct.unpack_from('<I', dec, off_start+i*4)[0]
        print(f'offset[{i}]={o}')
    data_start = off_end
    str_data = dec[data_start:]
    print(f'string_data ({len(str_data)} bytes): {str_data}')
