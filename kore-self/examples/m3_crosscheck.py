"""Independent DuckDB cross-check of World M3 task truths (Track M3, docs/m3_design.md).

Usage: python -I crosscheck.py EXPORT_DIR PARQUET_FILE
EXPORT_DIR holds seed_<s>/ with files.meta.tsv, tasks.tsv and the rendered files (written by
`measure_m3 export`). Every rendered table file is read with its explicit, correct dialect; its rows are
checked against the Parquet file itself (joined on trip_id = file row number); then every task's truth is
recomputed by a DuckDB query and compared with the Rust truth. Series truths are recomputed from the
Parquet file. Prints one summary line per seed and every disagreement; exit 1 on any disagreement.
Nothing is written.
"""
import os
import sys

import duckdb

INT = {"VendorID", "passenger_count", "RatecodeID", "PULocationID", "DOLocationID", "payment_type", "trip_id"}
TS = {"tpep_pickup_datetime", "tpep_dropoff_datetime"}
FLAG = {"store_and_fwd_flag"}
COMP = ["fare_amount", "extra", "mta_tax", "tip_amount", "tolls_amount", "improvement_surcharge", "congestion_surcharge", "Airport_fee"]
AIRPORTS = (1, 132, 138)
DELIM_NAME = {44: "comma", 59: "semicolon", 9: "tab", 124: "pipe"}


def canon(c):
    c = int(c)
    s = "-" if c < 0 else ""
    a = abs(c)
    return f"{s}{a // 100}.{a % 100:02d}"


def lit(s):
    if "'" in s:
        raise ValueError("quote in literal")
    return "'" + s + "'"


def typed_expr(name, tok, dec_comma, us):
    e = f"trim(\"{name}\", ' \r')"
    e = f"CASE WHEN {e} = {lit(tok)} OR {e} = '' THEN NULL ELSE {e} END"
    if name in INT:
        return f"CAST({e} AS BIGINT)"
    if name in TS:
        return f"strptime({e}, '%m/%d/%Y %I:%M:%S %p')" if us else f"CAST({e} AS TIMESTAMP)"
    if name in FLAG:
        return e
    if dec_comma:
        e = f"replace({e}, ',', '.')"
    return f"CAST(round(CAST({e} AS DECIMAL(18,2)) * 100) AS BIGINT)"


def read_lines(path):
    # the file as text lines (BOM and line ends removed), read by DuckDB as one column
    q = f"SELECT line FROM read_csv({lit(path)}, delim=chr(1), header=false, quote='', escape='', columns={{'line':'VARCHAR'}}, auto_detect=false, strict_mode=false)"
    rows = [r[0] if r[0] is not None else "" for r in con.execute(q).fetchall()]
    if rows and rows[0].startswith("﻿"):
        rows[0] = rows[0][1:]
    return [r.rstrip("\r") for r in rows]


def main():
    global con
    root, pq = sys.argv[1], sys.argv[2]
    con = duckdb.connect()
    con.execute(f"CREATE TABLE src AS SELECT file_row_number AS trip_id, * FROM read_parquet({lit(pq)}, file_row_number=true)")
    hour = "date_diff('hour', TIMESTAMP '2024-01-01', date_trunc('hour', tpep_pickup_datetime))"
    jan = "tpep_pickup_datetime >= TIMESTAMP '2024-01-01' AND tpep_pickup_datetime < TIMESTAMP '2024-02-01'"
    bad_total = 0
    only = sys.argv[3] if len(sys.argv) > 3 else None
    for sd in sorted(d for d in os.listdir(root) if d.startswith("seed_") and (only is None or d == only)):
        base = os.path.join(root, sd)
        meta = {}
        with open(os.path.join(base, "files.meta.tsv"), encoding="utf-8") as f:
            hdr = f.readline().rstrip("\n").split("\t")
            for line in f:
                r = dict(zip(hdr, line.rstrip("\n").split("\t")))
                meta[r["name"]] = r
        tasks = {}
        with open(os.path.join(base, "tasks.tsv"), encoding="utf-8") as f:
            hdr = f.readline().rstrip("\n").split("\t")
            for line in f:
                r = dict(zip(hdr, line.rstrip("\n").split("\t")))
                tasks.setdefault(r["file"], []).append(r)
        bad = []
        n_tasks = 0
        n_files = 0
        series_cache = {}
        for name, ts in sorted(tasks.items()):
            m = meta[name]
            path = os.path.join(base, name)
            n_files += 1
            if m["pool"] == "series":
                ctx = m["ctx"]
                if ctx not in series_cache:
                    if ctx.startswith("hourly"):
                        cond = {"hourly_city": "TRUE", "hourly_airport": f"PULocationID IN {AIRPORTS}", "hourly_zones": f"PULocationID IN ({m['zones']})"}[ctx]
                        rows = dict(con.execute(f"SELECT {hour} h, count(*) FROM src WHERE {jan} AND {cond} GROUP BY h").fetchall())
                        vals = [rows.get(i, 0) for i in range(744)]
                    else:
                        v = 1 if ctx.endswith("1") else 2
                        rows = dict(con.execute(f"SELECT day(tpep_pickup_datetime) d, count(*) FROM src WHERE {jan} AND VendorID = {v} GROUP BY d").fetchall())
                        vals = [rows.get(i, 0) for i in range(1, 32)]
                    rendered = [int(l.split(",")[1]) for l in read_lines(path)[1:]]
                    if rendered != vals:
                        bad.append(f"{name}: rendered series differs from the Parquet counts")
                    series_cache[ctx] = vals
                vals = series_cache[ctx]
                for t in ts:
                    n_tasks += 1
                    k = int(t["prefix"])
                    v = vals[k]
                    tol = max(3, int(v * 0.05 + 0.5))
                    if t["truth"] != f"tol:{v}/{tol}":
                        bad.append(f"{name} prefix {k}: rust {t['truth']} duckdb tol:{v}/{tol}")
                continue
            d = int(m["delim"])
            dch = chr(d)
            cols = m["cols"].split(",")
            lines = read_lines(path)
            header = m["header"] == "1"
            sep = dch + " " if m["pad"] == "1" else dch
            if m["pool"] in ("fix", "raw"):
                for t in ts:
                    n_tasks += 1
                    if t["tmpl"] == "format_of":
                        cands = [x for x in DELIM_NAME if all(len(l.split(chr(x))) == len(cols) for l in lines)]
                        first = [x.strip() for x in lines[0].split(chr(cands[0]))] if cands else []
                        has_h = first == cols
                        got = f"exact:{DELIM_NAME[cands[0]]}/{'header' if has_h else 'noheader'}" if len(cands) == 1 else f"ambiguous {cands}"
                        if got != t["truth"]:
                            bad.append(f"{name} format: rust {t['truth']} duckdb {got}")
                    elif t["tmpl"] == "fix_line":
                        tok = "" if m["null_tok"] == "<empty>" else m["null_tok"]
                        first_bad = None
                        for i, l in enumerate(lines[1:], start=2):
                            fs = [x.strip() for x in l.split(dch)]
                            if len(fs) != len(cols):
                                first_bad = i
                                break
                            exprs = []
                            for c, x in zip(cols, fs):
                                if x == tok or x == "":
                                    continue
                                if c in INT:
                                    exprs.append(f"TRY_CAST({lit(x)} AS BIGINT) IS NULL")
                                elif c in TS:
                                    exprs.append(f"try_strptime({lit(x)}, '%m/%d/%Y %I:%M:%S %p') IS NULL" if m["us_ts"] == "1" else f"TRY_CAST({lit(x)} AS TIMESTAMP) IS NULL")
                                elif c not in FLAG:
                                    exprs.append(f"TRY_CAST({lit(x.replace(',', '.') if m['decimal_comma'] == '1' else x)} AS DECIMAL(18,2)) IS NULL")
                            if exprs and con.execute("SELECT " + " OR ".join(exprs)).fetchone()[0]:
                                first_bad = i
                                break
                        if f"exact:{first_bad}" != t["truth"]:
                            bad.append(f"{name} fix: rust {t['truth']} duckdb {first_bad}")
                    else:
                        bad.append(f"{name}: unexpected template {t['tmpl']} on a {m['pool']} file")
                continue
            # a data table: the explicit dialect, every column as text, then typed
            tok = "" if m["null_tok"] == "<empty>" else m["null_tok"]
            colspec = "{" + ", ".join(f"'{c}': 'VARCHAR'" for c in cols) + "}"
            nl = "\\r\\n" if m["crlf"] == "1" else "\\n"
            con.execute("DROP VIEW IF EXISTS raw; DROP TABLE IF EXISTS v")
            con.execute(f"CREATE VIEW raw AS SELECT * FROM read_csv({lit(path)}, delim={lit(dch)}, header=false, skip={1 if header else 0}, quote='\"', escape='\"', columns={colspec}, auto_detect=false, new_line='{nl}', all_varchar=true)")
            sel = ", ".join(f"{typed_expr(c, tok, m['decimal_comma'] == '1', m['us_ts'] == '1')} AS \"{c}\"" for c in cols)
            con.execute(f"CREATE TABLE v AS SELECT {sel} FROM raw")
            nrows = con.execute("SELECT count(*) FROM v").fetchone()[0]
            if nrows != int(m["rows"]):
                bad.append(f"{name}: {nrows} rows read, {m['rows']} rendered")
            # every value equals the Parquet file's value on the same trip_id
            conds = []
            for c in cols:
                if c == "trip_id":
                    continue
                sv = f"s.\"{c}\""
                if c not in INT | TS | FLAG:
                    sv = f"CAST(round(s.\"{c}\" * 100) AS BIGINT)"
                conds.append(f"v.\"{c}\" IS DISTINCT FROM {sv}")
            mis = con.execute(f"SELECT count(*), count(s.trip_id) FROM v LEFT JOIN src s ON s.trip_id = v.trip_id WHERE {' OR '.join(conds)} OR s.trip_id IS NULL").fetchone()
            if mis[0] != 0:
                bad.append(f"{name}: {mis[0]} rows differ from the Parquet file")
            for t in ts:
                n_tasks += 1
                got = truth(t, m)
                if got != t["truth"]:
                    bad.append(f"{name} {t['tmpl']} param={t['param']} cols={t['c0']},{t['c1']},{t['c2']},{t['c3']}: rust {t['truth']} duckdb {got}")
        print(f"{sd}: {n_files} files with tasks, {n_tasks} distinct tasks, {len(bad)} disagreements", flush=True)
        for b in bad[:50]:
            print("  DISAGREE", b)
        bad_total += len(bad)
    print(f"TOTAL disagreements: {bad_total}")
    sys.exit(1 if bad_total else 0)


def one(q):
    return con.execute(q).fetchone()


def truth(t, m):
    tm, p, c0, c1, c2, c3 = t["tmpl"], t["param"], t["c0"], t["c1"], t["c2"], t["c3"]
    q = lambda c: f'"{c}"'
    if tm == "col_sum":
        return "exact:" + canon(one(f"SELECT sum({q(c0)}) FROM v")[0])
    if tm == "col_max":
        return "exact:" + canon(one(f"SELECT max({q(c0)}) FROM v")[0])
    if tm == "col_mean":
        s, n = one(f"SELECT sum({q(c0)}), count({q(c0)}) FROM v")
        return f"mean:{int(s)}/{n}"
    if tm == "col_count":
        return f"exact:{one('SELECT count(*) FROM v')[0]}"
    if tm == "col_distinct":
        return f"exact:{one(f'SELECT count(DISTINCT {q(c1)}) FROM v')[0]}"
    if tm == "flt_count":
        k = round(float(p) * 100) if c3 == ">" else int(p)
        return f"exact:{one(f'SELECT count(*) FROM v WHERE {q(c0)} {c3} {k}')[0]}"
    if tm == "flt_sum":
        return "exact:" + canon(one(f"SELECT coalesce(sum({q(c0)}), 0) FROM v WHERE {q(c1)} = {int(p)}")[0])
    if tm == "grp_top":
        r = con.execute(f"SELECT {q(c1)}, sum({q(c0)}) s FROM v WHERE {q(c0)} IS NOT NULL AND {q(c1)} IS NOT NULL GROUP BY 1 ORDER BY s DESC").fetchall()
        if len(r) < 2 or r[0][1] == r[1][1]:
            return "ambiguous"
        return f"exact:{r[0][0]}"
    if tm == "grp_top3":
        r = con.execute(f"SELECT {q(c1)}, count(*) n FROM v WHERE {q(c1)} IS NOT NULL GROUP BY 1 ORDER BY n DESC, 1 ASC LIMIT 3").fetchall()
        return "exact:" + ",".join(str(x[0]) for x in r)
    if tm == "q_median":
        s, k = one(f"WITH x AS (SELECT {q(c0)} val, row_number() OVER (ORDER BY {q(c0)}) rn, count(*) OVER () n FROM v WHERE {q(c0)} IS NOT NULL) SELECT sum(val), count(*) FROM x WHERE rn = (n + 1) // 2 OR rn = n // 2 + 1")
        return f"mean:{int(s)}/{k}"
    if tm == "q_p90":
        n = one(f"SELECT count({q(c0)}) FROM v")[0]
        k = (n * 9 + 9) // 10
        return "exact:" + canon(one(f"SELECT {q(c0)} FROM v WHERE {q(c0)} IS NOT NULL ORDER BY 1 LIMIT 1 OFFSET {k - 1}")[0])
    if tm == "lookup":
        x = one(f"SELECT {q(c0)} FROM v WHERE trip_id = {int(p)}")[0]
        return "exact:" + (str(x) if c0 in INT else canon(x))
    if tm == "an_fare_neg":
        return f"exact:{one(f'SELECT count(*) FROM v WHERE {q(c0)} < 0')[0]}"
    if tm == "an_dist_zero":
        return f"exact:{one(f'SELECT count(*) FROM v WHERE {q(c0)} = 0')[0]}"
    if tm == "an_total_mismatch":
        nn = " AND ".join(f"{q(c)} IS NOT NULL" for c in COMP + ["total_amount"])
        return f"exact:{one(f'SELECT count(*) FROM v WHERE {nn} AND total_amount <> ' + ' + '.join(q(c) for c in COMP))[0]}"
    if tm == "an_out_day":
        return f"exact:{one(f'SELECT count(*) FROM v WHERE {q(c0)} IS NOT NULL AND CAST({q(c0)} AS DATE) <> DATE {lit(p)}')[0]}"
    if tm == "an_out_month":
        lo, hi = "TIMESTAMP '2024-01-01'", "TIMESTAMP '2024-02-01'"
        return f"exact:{one(f'SELECT count(*) FROM v WHERE {q(c0)} < {lo} OR {q(c0)} >= {hi}')[0]}"
    if tm == "an_first_back":
        return f"exact:{one(f'SELECT min(trip_id) FROM v WHERE {q(c1)} < {q(c0)}')[0]}"
    return f"unknown template {tm}"


if __name__ == "__main__":
    main()
