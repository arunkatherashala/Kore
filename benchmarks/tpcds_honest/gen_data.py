"""TPC-DS data + query extraction using DuckDB's built-in dsdgen / tpcds_queries().

    python gen_data.py --sf 0.1 --out data/sf0.1
    python gen_data.py --sf 1   --out data/sf1

Writes the 24 tables as <table>.csv (header, ISO dates; what KORE's load_csv reads) and <table>.parquet
(native types: DATE, DECIMAL; what the DuckDB reference reads), plus queries.json (the 99 stock queries) next
to this script. Data is DuckDB's dsdgen output (the official TPC-DS generator code), default RNG seed.
"""
import argparse
import json
import os

import duckdb

HERE = os.path.dirname(os.path.abspath(__file__))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sf", type=float, required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    out = os.path.abspath(a.out)
    os.makedirs(out, exist_ok=True)
    con = duckdb.connect(":memory:")
    con.execute(f"CALL dsdgen(sf={a.sf})")
    tables = [r[0] for r in con.execute("show tables").fetchall()]
    schema = {}
    for t in tables:
        p = os.path.join(out, t).replace("\\", "/")
        schema[t] = [(r[0], r[1]) for r in con.execute(f"describe {t}").fetchall()]
        # KORE's load_csv infers types from a sample; a header `col::str` declares a text column (zip codes etc.)
        sel = ", ".join(f'"{c}" AS "{c}::str"' if ty.startswith(("VARCHAR", "CHAR")) else f'"{c}"' for c, ty in schema[t])
        con.execute(f"COPY (SELECT {sel} FROM {t}) TO '{p}.csv' (HEADER, DELIMITER ',')")
        con.execute(f"COPY {t} TO '{p}.parquet' (FORMAT PARQUET)")
        n = con.execute(f"select count(*) from {t}").fetchone()[0]
        print(f"{t:24} {n:>10} rows", flush=True)
    json.dump(schema, open(os.path.join(out, "schema.json"), "w"), indent=1)
    qs = {f"Q{n}": q for n, q in con.execute("select query_nr, query from tpcds_queries() order by 1").fetchall()}
    json.dump(qs, open(os.path.join(HERE, "queries.json"), "w"), indent=1)
    print(f"{len(qs)} queries -> queries.json, {len(tables)} tables -> {out}")


if __name__ == "__main__":
    main()
