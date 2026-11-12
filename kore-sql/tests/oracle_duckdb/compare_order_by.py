"""ORDER BY oracle, DuckDB side: reruns every case written by `kore-sql/examples/order_by_oracle_gen.rs` in DuckDB
and compares KORE's result row by row (order matters).

Usage (DuckDB from an existing env, nothing installed; script outside the data dir, path as an argument):
    python -I compare_order_by.py CASE_DIR

Each table `t_<tid>.tsv` is loaded as text and cast to its declared types (cell encoding: see the generator). For
each query a view `t` over that table is created and the DuckDB SQL is run. A KORE cell matches a DuckDB value when:
NULL == NULL; text equal (after unescaping); numbers equal as doubles (so a COUNT returned as Int64 or Float64
matches; NaN == NaN; -0.0 == 0.0); bools equal. Prints one line per group and every disagreement (first 20);
exit 1 on any disagreement. Nothing is written.
"""
import math
import os
import sys

import duckdb

DUCK_TYPES = {"BIGINT", "DOUBLE", "VARCHAR", "BOOLEAN"}


def unesc(s):
    assert s.startswith("="), s
    out, i = [], 1
    while i < len(s):
        c = s[i]
        if c == "\\":
            n = s[i + 1]
            out.append({"\\": "\\", "t": "\t", "n": "\n", "r": "\r"}[n])
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def kore_value(cell):
    """A KORE cell as a Python value (text stays text, everything else is parsed later by the DuckDB type)."""
    if cell == "\\N":
        return None
    if cell.startswith("="):
        return ("s", unesc(cell))
    return ("raw", cell)


def same(k, d):
    if k is None or d is None:
        return k is None and d is None
    tag, v = k
    if isinstance(d, str):
        return tag == "s" and v == d
    if tag != "raw":
        return False
    if isinstance(d, bool):
        return v == ("true" if d else "false")
    x = float(v)  # Rust {:?}: NaN, inf, -inf, 1e300, -0.0
    y = float(d)
    if math.isnan(x) or math.isnan(y):
        return math.isnan(x) and math.isnan(y)
    return x == y


def lit(s):
    assert "'" not in s
    return "'" + s + "'"


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    case_dir = sys.argv[1]
    con = duckdb.connect()
    # the defaults this script assumes: binary text order (no collation), explicit NULLS clauses everywhere
    assert con.execute("SELECT current_setting('default_collation')").fetchone()[0] == ""

    with open(os.path.join(case_dir, "tables.tsv"), encoding="utf-8") as fh:
        for line in fh:
            tid, schema = line.rstrip("\n").split("\t")
            cols = [c.split(":") for c in schema.split(",")]
            assert all(t in DUCK_TYPES for _, t in cols)
            path = os.path.join(case_dir, f"t_{tid}.tsv").replace("\\", "/")
            spec = ", ".join(f"'{n}': 'VARCHAR'" for n, _ in cols)
            raw = f"read_csv({lit(path)}, delim='\t', header=true, quote='', escape='', auto_detect=false, columns={{{spec}}}, nullstr='\x01', strict_mode=true)"
            exprs = []
            for n, t in cols:
                e = f'"{n}"'
                null = f"CASE WHEN {e} = '\\N' THEN NULL ELSE {e} END"
                if t == "VARCHAR":
                    # '=' prefix removed; escapes undone
                    v = f"substr({null}, 2)"
                    v = f"replace(replace(replace(replace(replace({v}, '\\\\', chr(1)), '\\t', chr(9)), '\\n', chr(10)), '\\r', chr(13)), chr(1), '\\')"
                    exprs.append(f'{v} AS "{n}"')
                else:
                    exprs.append(f'CAST({null} AS {t}) AS "{n}"')
            con.execute(f"CREATE TABLE t_{tid} AS SELECT {', '.join(exprs)} FROM {raw}")
            # loaded row count equals the file's data lines
            with open(path, encoding="utf-8") as tf:
                nlines = sum(1 for _ in tf) - 1
            assert con.execute(f"SELECT COUNT(*) FROM t_{tid}").fetchone()[0] == nlines, tid

    groups = {}
    bad = []
    with open(os.path.join(case_dir, "queries.tsv"), encoding="utf-8") as fh:
        for line in fh:
            qid, tid, group, kore_sql, duck_sql = line.rstrip("\n").split("\t")
            con.execute(f"CREATE OR REPLACE VIEW t AS SELECT * FROM t_{tid}")
            want = con.execute(duck_sql).fetchall()
            with open(os.path.join(case_dir, f"r_{qid}.tsv"), encoding="utf-8") as rf:
                got = [[kore_value(c) for c in l.rstrip("\n").split("\t")] for l in rf]
            g = groups.setdefault(group, [0, 0, 0])
            g[0] += 1
            g[2] += len(want)
            ok = len(got) == len(want) and all(
                len(gr) == len(wr) and all(same(k, d) for k, d in zip(gr, wr)) for gr, wr in zip(got, want))
            if not ok:
                g[1] += 1
                first = next((i for i, (gr, wr) in enumerate(zip(got, want))
                              if len(gr) != len(wr) or not all(same(k, d) for k, d in zip(gr, wr))), min(len(got), len(want)))
                bad.append((qid, group, kore_sql, duck_sql, len(got), len(want), first,
                            got[first] if first < len(got) else None, want[first] if first < len(want) else None))

    total = sum(g[0] for g in groups.values())
    for name, (n, nbad, rows) in groups.items():
        print(f"{name}: queries={n} rows_compared={rows} disagreements={nbad}")
    print(f"TOTAL queries={total} disagreements={len(bad)}")
    for b in bad[:20]:
        print("DISAGREE", b)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
