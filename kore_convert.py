#!/usr/bin/env python3
"""kore-convert: Universal format converter — any format ↔ .hkore/.kore"""
import sys, os, time, argparse, csv, json

sys.path.insert(0, os.path.join(os.path.dirname(__file__), 'kore-python'))
import kore_fileformat as kore

SUPPORTED = {'.csv', '.json', '.ndjson', '.hkore', '.kore', '.parquet', '.orc', '.tsv'}

def read_any(path):
    ext = os.path.splitext(path)[1].lower()
    if ext == '.csv' or ext == '.tsv':
        delim = '\t' if ext == '.tsv' else ','
        with open(path, newline='') as f:
            reader = csv.DictReader(f, delimiter=delim)
            rows = list(reader)
        if not rows:
            return kore.DataBlock()
        cols = {k: [] for k in rows[0].keys()}
        for r in rows:
            for k, v in r.items():
                cols[k].append(v)
        b = kore.DataBlock()
        for name, vals in cols.items():
            # Auto-detect type
            try:
                int_vals = [int(v) for v in vals]
                b.add_column(name, kore.DataType.I64, int_vals)
            except (ValueError, TypeError):
                try:
                    float_vals = [float(v) for v in vals]
                    b.add_column(name, kore.DataType.F64, float_vals)
                except (ValueError, TypeError):
                    b.add_column(name, kore.DataType.STR, vals)
        return b

    elif ext == '.json':
        with open(path) as f:
            data = json.load(f)
        if isinstance(data, list) and data:
            cols = {k: [] for k in data[0].keys()}
            for r in data:
                for k in cols:
                    cols[k].append(r.get(k))
            b = kore.DataBlock()
            for name, vals in cols.items():
                if all(isinstance(v, int) for v in vals if v is not None):
                    b.add_column(name, kore.DataType.I64, vals)
                elif all(isinstance(v, (int, float)) for v in vals if v is not None):
                    b.add_column(name, kore.DataType.F64, [float(v) if v else 0.0 for v in vals])
                else:
                    b.add_column(name, kore.DataType.STR, [str(v) if v else '' for v in vals])
            return b
        return kore.DataBlock()

    elif ext == '.ndjson':
        rows = []
        with open(path) as f:
            for line in f:
                if line.strip():
                    rows.append(json.loads(line))
        if not rows:
            return kore.DataBlock()
        cols = {k: [] for k in rows[0].keys()}
        for r in rows:
            for k in cols:
                cols[k].append(r.get(k))
        b = kore.DataBlock()
        for name, vals in cols.items():
            if all(isinstance(v, int) for v in vals if v is not None):
                b.add_column(name, kore.DataType.I64, vals)
            elif all(isinstance(v, (int, float)) for v in vals if v is not None):
                b.add_column(name, kore.DataType.F64, [float(v) if v else 0.0 for v in vals])
            else:
                b.add_column(name, kore.DataType.STR, [str(v) if v else '' for v in vals])
        return b

    elif ext == '.hkore':
        return kore.read_hybrid(path)

    elif ext == '.parquet':
        try:
            import pyarrow.parquet as pq
            table = pq.read_table(path)
            b = kore.DataBlock()
            for col_name in table.column_names:
                col = table.column(col_name)
                arr = col.to_pylist()
                if col.type in ('int64', 'int32', 'int16', 'int8'):
                    b.add_column(col_name, kore.DataType.I64, [v if v is not None else 0 for v in arr])
                elif col.type in ('float', 'double', 'float32', 'float64'):
                    b.add_column(col_name, kore.DataType.F64, [v if v is not None else 0.0 for v in arr])
                else:
                    b.add_column(col_name, kore.DataType.STR, [str(v) if v is not None else '' for v in arr])
            return b
        except ImportError:
            print("ERROR: pip install pyarrow to read .parquet files")
            sys.exit(1)

    elif ext == '.orc':
        try:
            import pyarrow.orc as orc_mod
            table = orc_mod.read_table(path)
            b = kore.DataBlock()
            for col_name in table.column_names:
                arr = table.column(col_name).to_pylist()
                b.add_column(col_name, kore.DataType.STR, [str(v) for v in arr])
            return b
        except ImportError:
            print("ERROR: pip install pyarrow to read .orc files")
            sys.exit(1)

    else:
        print(f"ERROR: unsupported format '{ext}'")
        sys.exit(1)


def write_any(block, path):
    ext = os.path.splitext(path)[1].lower()
    if ext == '.hkore':
        kore.write_hybrid(path, block)

    elif ext == '.csv' or ext == '.tsv':
        delim = '\t' if ext == '.tsv' else ','
        with open(path, 'w', newline='') as f:
            w = csv.writer(f, delimiter=delim)
            names = [c.name for c in block.columns]
            w.writerow(names)
            for i in range(block.num_rows):
                row = [block.columns[j].data[i] for j in range(len(block.columns))]
                w.writerow(row)

    elif ext == '.json':
        names = [c.name for c in block.columns]
        rows = []
        for i in range(block.num_rows):
            row = {names[j]: block.columns[j].data[i] for j in range(len(names))}
            rows.append(row)
        with open(path, 'w') as f:
            json.dump(rows, f)

    elif ext == '.ndjson':
        names = [c.name for c in block.columns]
        with open(path, 'w') as f:
            for i in range(block.num_rows):
                row = {names[j]: block.columns[j].data[i] for j in range(len(names))}
                f.write(json.dumps(row) + '\n')

    elif ext == '.parquet':
        try:
            import pyarrow as pa, pyarrow.parquet as pq
            arrays = {}
            for c in block.columns:
                arrays[c.name] = list(c.data)
            table = pa.table(arrays)
            pq.write_table(table, path)
        except ImportError:
            print("ERROR: pip install pyarrow to write .parquet files")
            sys.exit(1)
    else:
        print(f"ERROR: unsupported output format '{ext}'")
        sys.exit(1)


def main():
    parser = argparse.ArgumentParser(
        prog='kore-convert',
        description='Convert between any data format and KORE (.hkore)')
    parser.add_argument('src', help='Source file (csv/json/ndjson/parquet/orc/hkore)')
    parser.add_argument('dst', help='Destination file (csv/json/ndjson/parquet/hkore)')
    parser.add_argument('--benchmark', '-b', action='store_true', help='Show speed comparison')
    args = parser.parse_args()

    src_ext = os.path.splitext(args.src)[1].lower()
    dst_ext = os.path.splitext(args.dst)[1].lower()

    if src_ext not in SUPPORTED:
        print(f"ERROR: unsupported source format '{src_ext}'. Supported: {SUPPORTED}")
        sys.exit(1)
    if dst_ext not in SUPPORTED:
        print(f"ERROR: unsupported destination format '{dst_ext}'. Supported: {SUPPORTED}")
        sys.exit(1)

    print(f"Converting: {args.src} ({src_ext}) → {args.dst} ({dst_ext})")
    t0 = time.perf_counter()
    block = read_any(args.src)
    read_ms = (time.perf_counter() - t0) * 1000

    t0 = time.perf_counter()
    write_any(block, args.dst)
    write_ms = (time.perf_counter() - t0) * 1000

    src_kb = os.path.getsize(args.src) / 1024
    dst_kb = os.path.getsize(args.dst) / 1024
    print(f"Done! {block.num_rows:,} rows × {block.num_columns} cols")
    print(f"  Read:  {read_ms:.1f}ms | Write: {write_ms:.1f}ms | Total: {read_ms+write_ms:.1f}ms")
    print(f"  Size:  {src_kb:.0f}KB → {dst_kb:.0f}KB ({dst_kb/src_kb*100:.0f}%)")

    if args.benchmark and dst_ext == '.hkore':
        t0 = time.perf_counter()
        kore.read_hybrid(args.dst)
        r_ms = (time.perf_counter() - t0) * 1000
        print(f"  KORE read speed: {r_ms:.1f}ms ({r_ms*1e6/block.num_rows:.0f} ns/row)")


if __name__ == '__main__':
    main()
