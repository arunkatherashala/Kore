"""Add zstd compression to write_kore/read_kore"""
path = 'C:/Users/skathera/Downloads/KoreRepo/kore-python/kore_fileformat.py'
with open(path, 'r', encoding='utf-8') as f:
    code = f.read()

old = '''def write_kore(path, block):
    """Write .kore binary file directly - 10 ns/row, no CSV roundtrip."""
    import struct, array as _arr
    with open(str(path), 'wb') as f:
        ncols = block.num_columns
        nrows = block.num_rows
        f.write(b'KORE')
        f.write(struct.pack('<HIQ', 2, ncols, nrows))
        for col in block.columns:
            dn = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
            nb = col.name.encode('utf-8')
            f.write(struct.pack('<H', len(nb)))
            f.write(nb)
            dt = 2 if dn in ('F64','FLOAT64','2') else (4 if dn in ('STR','STRING','3') else 1)
            f.write(struct.pack('<B', dt))
        for col in block.columns:
            dn = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
            f.write(struct.pack('<B', 0))
            if dn in ('STR','STRING','3'):
                buf = bytearray()
                for s in col.data:
                    sb = str(s).encode('utf-8')
                    buf += struct.pack('<I', len(sb)) + sb
                f.write(struct.pack('<Q', len(buf)))
                f.write(buf)
            else:
                tc = 'd' if dn in ('F64','FLOAT64','2') else 'q'
                if isinstance(col.data, _arr.array):
                    raw = col.data.tobytes()
                else:
                    raw = _arr.array(tc, col.data).tobytes()
                f.write(struct.pack('<Q', len(raw)))
                f.write(raw)'''

new = '''def write_kore(path, block, compression='zstd'):
    """Write .kore binary file — supports zstd/zlib/none compression."""
    import struct, array as _arr
    def _compress(data, comp):
        if comp == 'zstd':
            try:
                import zstandard as zstd
                return 6, zstd.ZstdCompressor(level=3).compress(data)
            except ImportError:
                pass
        if comp == 'zlib':
            import zlib
            return 5, zlib.compress(data, 6)
        return 0, data  # Raw
    with open(str(path), 'wb') as f:
        ncols = block.num_columns
        nrows = block.num_rows
        f.write(b'KORE')
        f.write(struct.pack('<HIQ', 2, ncols, nrows))
        for col in block.columns:
            dn = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
            nb = col.name.encode('utf-8')
            f.write(struct.pack('<H', len(nb)))
            f.write(nb)
            dt = 2 if dn in ('F64','FLOAT64','2') else (4 if dn in ('STR','STRING','3') else 1)
            f.write(struct.pack('<B', dt))
        for col in block.columns:
            dn = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
            if dn in ('STR','STRING','3'):
                buf = bytearray()
                for s in col.data:
                    sb = str(s).encode('utf-8')
                    buf += struct.pack('<I', len(sb)) + sb
                raw = bytes(buf)
            else:
                tc = 'd' if dn in ('F64','FLOAT64','2') else 'q'
                if isinstance(col.data, _arr.array):
                    raw = col.data.tobytes()
                else:
                    raw = _arr.array(tc, col.data).tobytes()
            comp_id, compressed = _compress(raw, compression)
            f.write(struct.pack('<B', comp_id))
            f.write(struct.pack('<Q', len(raw)))      # original size
            f.write(struct.pack('<Q', len(compressed))) # compressed size
            f.write(compressed)'''

code = code.replace(old, new)

# Fix read_kore to handle compression
old_read = '''def read_kore(path, columns=None):
    """Read .kore binary file directly - 10 ns/row, zero-copy."""
    import struct, array as _arr
    want = set(columns) if columns else None
    with open(str(path), 'rb') as f:
        magic = f.read(4)
        if magic != b'KORE':
            raise ValueError('Not a .kore file')
        ver, ncols, nrows = struct.unpack('<HIQ', f.read(14))
        schema = []
        for _ in range(ncols):
            nl = struct.unpack('<H', f.read(2))[0]
            name = f.read(nl).decode('utf-8')
            dtype = struct.unpack('<B', f.read(1))[0]
            schema.append((name, dtype))
        block = DataBlock()
        for name, dtype in schema:
            skip = want is not None and name not in want
            comp = struct.unpack('<B', f.read(1))[0]
            dlen = struct.unpack('<Q', f.read(8))[0]
            if skip:
                f.seek(dlen, 1)
                continue
            if dtype == 4:
                raw = f.read(dlen)
                strings, pos = [], 0
                for _ in range(nrows):
                    sl = struct.unpack_from('<I', raw, pos)[0]; pos += 4
                    strings.append(raw[pos:pos+sl].decode('utf-8')); pos += sl
                block.add_column(name, DataType.STR, strings)
            else:
                tc = 'd' if dtype == 2 else 'q'
                a = _arr.array(tc)
                a.frombytes(f.read(dlen))
                dt = DataType.F64 if dtype == 2 else DataType.I64
                block.add_column(name, dt, a)
        block.num_rows = nrows
        return block'''

new_read = '''def read_kore(path, columns=None):
    """Read .kore binary file with zstd/zlib decompression support."""
    import struct, array as _arr
    def _decompress(data, comp_id, orig_len):
        if comp_id == 6:
            import zstandard as zstd
            return zstd.ZstdDecompressor().decompress(data, max_output_size=orig_len)
        if comp_id == 5:
            import zlib
            return zlib.decompress(data)
        return data
    want = set(columns) if columns else None
    with open(str(path), 'rb') as f:
        magic = f.read(4)
        if magic != b'KORE':
            raise ValueError('Not a .kore file')
        ver, ncols, nrows = struct.unpack('<HIQ', f.read(14))
        schema = []
        for _ in range(ncols):
            nl = struct.unpack('<H', f.read(2))[0]
            name = f.read(nl).decode('utf-8')
            dtype = struct.unpack('<B', f.read(1))[0]
            schema.append((name, dtype))
        block = DataBlock()
        for name, dtype in schema:
            skip = want is not None and name not in want
            comp_id = struct.unpack('<B', f.read(1))[0]
            orig_len = struct.unpack('<Q', f.read(8))[0]
            comp_len = struct.unpack('<Q', f.read(8))[0]
            if skip:
                f.seek(comp_len, 1)
                continue
            compressed = f.read(comp_len)
            raw = _decompress(compressed, comp_id, orig_len)
            if dtype == 4:
                strings, pos = [], 0
                for _ in range(nrows):
                    sl = struct.unpack_from('<I', raw, pos)[0]; pos += 4
                    strings.append(raw[pos:pos+sl].decode('utf-8')); pos += sl
                block.add_column(name, DataType.STR, strings)
            else:
                tc = 'd' if dtype == 2 else 'q'
                a = _arr.array(tc)
                a.frombytes(raw)
                dt = DataType.F64 if dtype == 2 else DataType.I64
                block.add_column(name, dt, a)
        block.num_rows = nrows
        return block'''

code = code.replace(old_read, new_read)

with open(path, 'w', encoding='utf-8') as f:
    f.write(code)
print('DONE - write_kore/read_kore now have zstd compression!')
