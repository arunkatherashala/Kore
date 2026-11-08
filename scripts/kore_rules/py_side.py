"""Other formats for the rule-mode benchmark (docs/kore_rule_mode.md), on the exact blocks that
bench_rules dumped as Arrow buffers.

  python -I py_side.py DUMP_DIR OUT.json

Sizes: Parquet zstd level 22 (pyarrow, dictionary encoding on, one row group per 1M rows), CSV + xz -9
(pyarrow CSV writer, lzma preset 9), raw column buffers + xz -9 (the Arrow buffers concatenated).
"""
import json
import lzma
import os
import sys
import tempfile
import time

import pyarrow as pa
import pyarrow.csv as pacsv
import pyarrow.parquet as pq


def load(d):
    meta = json.load(open(os.path.join(d, "meta.json")))
    n = meta["rows"]
    arrays, names, raw = [], [], []
    for k, c in enumerate(meta["columns"]):
        valid = open(os.path.join(d, f"{k}.valid"), "rb").read()
        values = open(os.path.join(d, f"{k}.values"), "rb").read()
        raw += [valid, values]
        ty = {"int64": pa.int64(), "float64": pa.float64(), "bool": pa.bool_(), "large_string": pa.large_string()}[c["type"]]
        if c["type"] == "large_string":
            offs = open(os.path.join(d, f"{k}.offsets"), "rb").read()
            raw.append(offs)
            a = pa.Array.from_buffers(ty, n, [pa.py_buffer(valid), pa.py_buffer(offs), pa.py_buffer(values)])
        else:
            a = pa.Array.from_buffers(ty, n, [pa.py_buffer(valid), pa.py_buffer(values)])
        arrays.append(a)
        names.append(c["name"])
    return pa.table(arrays, names=names), b"".join(raw)


def main():
    d, out = sys.argv[1], sys.argv[2]
    t, raw = load(d)
    res = {"rows": t.num_rows, "cols": t.num_columns, "pyarrow": pa.__version__}
    with tempfile.TemporaryDirectory(dir=os.path.dirname(os.path.abspath(out))) as tmp:
        p = os.path.join(tmp, "t.parquet")
        s = time.perf_counter()
        pq.write_table(t, p, compression="zstd", compression_level=22, row_group_size=1 << 20)
        res["parquet_zstd22_write_s"] = time.perf_counter() - s
        res["parquet_zstd22"] = os.path.getsize(p)
        s = time.perf_counter()
        back = pq.read_table(p, memory_map=False)
        res["parquet_zstd22_read_s"] = time.perf_counter() - s
        assert back.equals(t)
        del back
        os.remove(p)
        c = os.path.join(tmp, "t.csv")
        pacsv.write_csv(t, c)
        csv = open(c, "rb").read()
        os.remove(c)
    res["csv"] = len(csv)
    s = time.perf_counter()
    res["csv_xz9"] = len(lzma.compress(csv, preset=9))
    res["csv_xz9_s"] = time.perf_counter() - s
    del csv
    s = time.perf_counter()
    res["raw_xz9"] = len(lzma.compress(raw, preset=9))
    res["raw_xz9_s"] = time.perf_counter() - s
    res["raw"] = len(raw)
    json.dump(res, open(out, "w"), indent=1)
    print(json.dumps(res))


if __name__ == "__main__":
    main()
