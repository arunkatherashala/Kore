"""TPC-DS tables for the rule-mode benchmark, generated locally by DuckDB's dsdgen (docs/kore_rule_mode.md).

  python -I export_tpcds.py OUT_DIR [SF] [MAX_ROWS]

Uses only an already installed `tpcds` extension (autoinstall/autoload off: nothing is downloaded; if the
extension is missing this fails). Writes one dump per table in the Arrow-buffer layout of bench_rules
(`dump_<table>/meta.json` + `<k>.valid/.values/.offsets`): DECIMAL -> float64 (DOUBLE), DATE -> 'YYYY-MM-DD'
text, every integer -> int64, CHAR/VARCHAR -> string. Rows in dsdgen order, at most MAX_ROWS per table.
"""
import json
import os
import sys

import duckdb
import pyarrow as pa
import pyarrow.compute as pc

TABLES = {"store_sales": "ss_ticket_number", "web_sales": "ws_order_number", "customer": "c_customer_sk",
          "customer_address": "ca_address_sk", "item": "i_item_sk"}


def dump(t, d, key):
    os.makedirs(d, exist_ok=True)
    meta = []
    for k, (name, col) in enumerate(zip(t.column_names, t.columns)):
        a = col.combine_chunks()
        ty = a.type
        if pa.types.is_decimal(ty):
            a = a.cast(pa.float64())
        elif pa.types.is_timestamp(ty):
            a = a.cast(pa.int64())  # epoch units of the timestamp type (e.g. microseconds)
        elif pa.types.is_date(ty):
            a = pc.strftime(a, format="%Y-%m-%d").cast(pa.large_string())
        elif pa.types.is_integer(ty):
            a = a.cast(pa.int64())
        elif pa.types.is_string(ty) or pa.types.is_large_string(ty):
            a = a.cast(pa.large_string())
        elif pa.types.is_floating(ty):
            a = a.cast(pa.float64())
        else:
            raise SystemExit(f"type {ty} of {name}")
        # rebuild without offset so the buffers start at row 0
        a = pa.concat_arrays([a])
        n = len(a)
        bufs = a.buffers()
        valid = bufs[0].to_pybytes()[: (n + 7) // 8] if bufs[0] is not None else b"\xff" * ((n + 7) // 8)
        if a.offset != 0:
            raise SystemExit("unexpected offset")
        open(os.path.join(d, f"{k}.valid"), "wb").write(valid)
        if pa.types.is_large_string(a.type):
            open(os.path.join(d, f"{k}.offsets"), "wb").write(bufs[1].to_pybytes()[: (n + 1) * 8])
            data = bufs[2].to_pybytes() if bufs[2] is not None else b""
            open(os.path.join(d, f"{k}.values"), "wb").write(data)
            meta.append({"name": name, "type": "large_string"})
        else:
            open(os.path.join(d, f"{k}.values"), "wb").write(bufs[1].to_pybytes()[: n * 8])
            meta.append({"name": name, "type": "int64" if pa.types.is_integer(a.type) else "float64"})
    json.dump({"rows": t.num_rows, "columns": meta, "key": key, "source": "duckdb dsdgen"}, open(os.path.join(d, "meta.json"), "w"), indent=1)


def main():
    out = sys.argv[1]
    sf = float(sys.argv[2]) if len(sys.argv) > 2 else 1
    max_rows = int(sys.argv[3]) if len(sys.argv) > 3 else 1_000_000
    c = duckdb.connect()
    c.sql("SET autoinstall_known_extensions=false")
    c.sql("SET autoload_known_extensions=false")
    c.sql("LOAD tpcds")
    c.sql(f"CALL dsdgen(sf={sf})")
    for t, key in TABLES.items():
        tab = c.sql(f"SELECT * FROM {t} LIMIT {max_rows}").arrow()
        if hasattr(tab, "read_all"):
            tab = tab.read_all()
        dump(tab, os.path.join(out, f"dump_tpcds_{t}"), key)
        print(t, tab.num_rows, tab.num_columns)


if __name__ == "__main__":
    main()
