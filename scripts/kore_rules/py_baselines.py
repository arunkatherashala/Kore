"""More baselines for the rule-mode benchmark (docs/kore_rule_mode.md), on the dumps of bench_rules.

  python -I py_baselines.py DUMP_DIR OUT.json

Sizes in bytes of: DuckDB native file (CREATE TABLE AS from the Arrow table, CHECKPOINT), SQLite (one
table, one transaction, VACUUM) without and with an index on the key column, Parquet zstd-22 with dictionary
encoding forced off and on (`use_dictionary` False / True), and Parquet zstd-22 of the table sorted by the
key column (dictionary on). Key = meta.json "key", else the first column.
"""
import json
import os
import sqlite3
import sys
import tempfile

import duckdb
import pyarrow as pa
import pyarrow.parquet as pq

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from py_side import load  # noqa: E402


def main():
    d, out = sys.argv[1], sys.argv[2]
    meta = json.load(open(os.path.join(d, "meta.json")))
    t, _ = load(d)
    key = meta.get("key") or t.column_names[0]
    res = {"rows": t.num_rows, "cols": t.num_columns, "key": key, "duckdb": duckdb.__version__, "sqlite_version": sqlite3.sqlite_version}
    with tempfile.TemporaryDirectory(dir=os.path.dirname(os.path.abspath(out))) as tmp:
        for name, kw, tab in [("parquet_zstd22_dict_off", {"use_dictionary": False}, t),
                              ("parquet_zstd22_dict_on", {"use_dictionary": True}, t),
                              ("parquet_zstd22_sorted_by_key", {"use_dictionary": True}, t.sort_by(key))]:
            p = os.path.join(tmp, name + ".parquet")
            pq.write_table(tab, p, compression="zstd", compression_level=22, row_group_size=1 << 20, **kw)
            res[name] = os.path.getsize(p)
            os.remove(p)
        p = os.path.join(tmp, "t.duckdb")
        c = duckdb.connect(p)
        c.register("src", t)
        c.sql("CREATE TABLE t AS SELECT * FROM src")
        c.sql("CHECKPOINT")
        c.close()
        res["duckdb_file"] = os.path.getsize(p)
        os.remove(p)
        wal = p + ".wal"
        if os.path.exists(wal):
            os.remove(wal)
        p = os.path.join(tmp, "t.sqlite")
        db = sqlite3.connect(p)
        cols = ", ".join(f'"{n}"' for n in t.column_names)
        db.execute(f"CREATE TABLE t ({cols})")
        q = f"INSERT INTO t VALUES ({', '.join('?' * t.num_columns)})"
        with db:
            for batch in t.to_batches(max_chunksize=50_000):
                db.executemany(q, zip(*[c.to_pylist() for c in batch.columns]))
        db.execute("VACUUM")
        db.commit()
        res["sqlite"] = os.path.getsize(p)
        db.execute(f'CREATE INDEX k ON t ("{key}")')
        db.commit()
        db.execute("VACUUM")
        db.commit()
        res["sqlite_with_key_index"] = os.path.getsize(p)
        db.close()
        os.remove(p)
    json.dump(res, open(out, "w"), indent=1)
    print(json.dumps(res))


if __name__ == "__main__":
    main()
