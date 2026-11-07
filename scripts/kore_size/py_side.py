"""Other formats of the kore_size benchmark (docs/kore_size.md).

  python -I py_side.py --dump DUMPDIR --work WORKDIR --out RESULT.json [--reps 3] [--lookups 20] [--skip-slow]

DUMPDIR holds the logical rows of the Digital Life log as written by bench_kore_size --dump
(<table>.jsonl, one JSON array per row: version, then the columns listed in columns.json). Every format
stores exactly those tables and columns plus the `version` column (how a format without native
versions represents the commit a row belongs to). Ids stay 64-char hex text everywhere, except the
explicit "ids as binary(32)" Parquet variant.

Measured per format: total bytes, write time (1 run: from the in-memory rows/Arrow table to files, no
fsync), full read (all tables, all columns, into memory), projection (the `kind` column of events,
counted per value) and point lookup (text of an event by event_id) where the format can answer it
without decompressing everything. Every read result is checked against the expected value.
Optional libraries (pyarrow, duckdb) are used if importable; otherwise the rows say "not measured".
"""
import argparse
import bz2
import collections
import csv
import gzip
import io
import json
import lzma
import os
import shutil
import sqlite3
import statistics
import sys
import time

try:
    import pyarrow as pa
    import pyarrow.compute as pc
    import pyarrow.feather as feather
    import pyarrow.orc as orc
    import pyarrow.parquet as pq
except Exception:  # pragma: no cover
    pa = None
try:
    import duckdb
except Exception:  # pragma: no cover
    duckdb = None

csv.field_size_limit(1 << 30)


def stats(xs):
    return {"median": statistics.median(xs), "min": min(xs), "max": max(xs), "all": xs}


def dsize(p):
    if os.path.isfile(p):
        return os.path.getsize(p)
    t = 0
    for root, _, files in os.walk(p):
        for f in files:
            t += os.path.getsize(os.path.join(root, f))
    return t


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dump", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--reps", type=int, default=3)
    ap.add_argument("--lookups", type=int, default=20)
    ap.add_argument("--scan-lookups", type=int, default=None, help="lookups for full-scan formats (CSV); default = --lookups")
    ap.add_argument("--set", choices=["all", "ref"], default="all", help="ref = only SQLite, Parquet zstd (default level) and DuckDB (reference rows of docs/kore_speed_v4.md)")
    a = ap.parse_args()
    ALL = a.set == "all"
    shutil.rmtree(a.work, ignore_errors=True)
    os.makedirs(a.work)
    cols = json.load(open(os.path.join(a.dump, "columns.json")))
    tables = {}
    for t in sorted(cols):
        rows = []
        with open(os.path.join(a.dump, f"{t}.jsonl"), encoding="utf-8") as f:
            for line in f:
                rows.append(json.loads(line))
        if rows:
            tables[t] = rows
    schema = {t: [("version", "I64")] + [tuple(c) for c in cols[t]] for t in tables}
    ev = tables["events"]
    names = [c[0] for c in schema["events"]]
    I_ID, I_KIND, I_TEXT = names.index("event_id"), names.index("kind"), names.index("text")
    N = len(ev)
    L = a.lookups
    SL = a.scan_lookups or L
    lk = [(k * 2654435761 + 12345) % N for k in range(L)]
    lk_ids = [ev[i][I_ID] for i in lk]
    want_lk = sum(len(ev[i][I_TEXT].encode()) for i in lk)
    want_lk_scan = sum(len(ev[i][I_TEXT].encode()) for i in lk[:SL])
    want_proj = dict(collections.Counter(r[I_KIND] for r in ev))
    want_rows = {t: len(r) for t, r in tables.items()}
    R = {"n_events": N, "rows": want_rows, "reps": a.reps, "lookups": L, "scan_lookups": SL,
         "python": sys.version.split()[0], "sqlite": sqlite3.sqlite_version,
         "pyarrow": pa.__version__ if pa else None, "duckdb": duckdb.__version__ if duckdb else None,
         "lookup_check": want_lk, "projection_check": want_proj, "formats": {}}

    def timed(fn, reps):
        fn()  # warm-up (and correctness check below)
        xs = []
        for _ in range(reps):
            t0 = time.perf_counter()
            fn()
            xs.append(time.perf_counter() - t0)
        return xs

    def record(name, size, write_s, full=None, proj=None, lookup=None, note=""):
        e = {"size_bytes": size, "bytes_per_event": size / N, "write_s": write_s, "note": note}
        for key, val in (("full_read_s", full), ("projection_s", proj), ("lookup_per_op_s", lookup)):
            e[key] = val if val is not None else "not available"
        R["formats"][name] = e
        print(f"{name}: {size} bytes ({size / N:.1f} B/event) write {write_s:.3f}s", flush=True)

    def check_full(got):
        assert got == want_rows, (got, want_rows)

    def check_proj(got):
        assert dict(got) == want_proj, "projection mismatch"

    if ALL:
        # ---------------------------------------------------------------- CSV and compressed CSV
        cd = os.path.join(a.work, "csv")
        os.makedirs(cd)

        def cell(v):
            if v is None:
                return ""
            if v is True:
                return "1"
            if v is False:
                return "0"
            return v

        t0 = time.perf_counter()
        for t, rows in tables.items():
            with open(os.path.join(cd, f"{t}.csv"), "w", newline="", encoding="utf-8") as f:
                w = csv.writer(f)
                w.writerow([c[0] for c in schema[t]])
                for r in rows:
                    w.writerow([cell(v) for v in r])
        csv_write = time.perf_counter() - t0
        csv_files = {t: os.path.join(cd, f"{t}.csv") for t in tables}

        def csv_full(opener):
            out = {}
            for t, p in csv_files.items():
                with opener(p) as f:
                    r = csv.reader(io.TextIOWrapper(f, encoding="utf-8", newline=""))
                    next(r)
                    out[t] = len(list(r))
            return out

        def csv_proj(opener):
            c = collections.Counter()
            with opener(csv_files["events"]) as f:
                r = csv.reader(io.TextIOWrapper(f, encoding="utf-8", newline=""))
                next(r)
                for row in r:
                    c[row[I_KIND]] += 1
            return c

        def csv_lookup():
            s = 0
            for id_ in lk_ids[:SL]:
                with open(csv_files["events"], "rb") as f:
                    for row in csv.reader(io.TextIOWrapper(f, encoding="utf-8", newline="")):
                        if row[I_ID] == id_:
                            s += len(row[I_TEXT].encode())
                            break
            return s

        plain = lambda p: open(p, "rb")
        check_full(csv_full(plain))
        check_proj(csv_proj(plain))
        assert csv_lookup() == want_lk_scan
        record("CSV (3 files)", dsize(cd), csv_write, stats(timed(lambda: csv_full(plain), a.reps)),
               stats(timed(lambda: csv_proj(plain), a.reps)),
               stats([x / SL for x in timed(csv_lookup, a.reps)]), note=f"lookup = scan of events.csv until found ({SL} ids)")

        def compressed(name, suffix, open_w, open_r):
            t0 = time.perf_counter()
            for p in csv_files.values():
                with open(p, "rb") as fi, open_w(p + suffix) as fo:
                    shutil.copyfileobj(fi, fo, 1 << 20)
            comp = time.perf_counter() - t0
            size = sum(dsize(p + suffix) for p in csv_files.values())
            rd = lambda p: open_r(p + suffix)
            check_full(csv_full(rd))
            check_proj(csv_proj(rd))
            record(name, size, csv_write + comp, stats(timed(lambda: csv_full(rd), a.reps)), stats(timed(lambda: csv_proj(rd), a.reps)), None,
                   note=f"write = CSV {csv_write:.3f}s + compress {comp:.3f}s; reads decompress the whole file; point lookup not available without full decompression")
            for p in csv_files.values():
                os.remove(p + suffix)

        compressed("CSV + gzip -6", ".gz6", lambda p: gzip.GzipFile(p, "wb", compresslevel=6, mtime=0), lambda p: gzip.open(p, "rb"))
        compressed("CSV + gzip -9", ".gz9", lambda p: gzip.GzipFile(p, "wb", compresslevel=9, mtime=0), lambda p: gzip.open(p, "rb"))
        compressed("CSV + bz2 -9", ".bz2", lambda p: bz2.BZ2File(p, "wb", compresslevel=9), lambda p: bz2.open(p, "rb"))
        compressed("CSV + xz -9 (lzma preset 9)", ".xz", lambda p: lzma.LZMAFile(p, "wb", preset=9), lambda p: lzma.open(p, "rb"))
        if pa is not None:
            for lvl in (3, 19):
                class ZW:
                    def __init__(self, p, lvl=lvl):
                        self.p, self.buf, self.lvl = p, io.BytesIO(), lvl
                    def __enter__(self):
                        return self.buf
                    def __exit__(self, *x):
                        data = pa.Codec("zstd", compression_level=self.lvl).compress(self.buf.getvalue(), asbytes=True)
                        with open(self.p, "wb") as f:
                            f.write(data)
                compressed(f"CSV + zstd -{lvl} (pyarrow codec)", f".zst{lvl}", ZW, lambda p: pa.CompressedInputStream(pa.OSFile(p, "rb"), "zstd"))
        else:
            R["formats"]["CSV + zstd"] = "not measured (pyarrow not importable)"

    # ---------------------------------------------------------------- SQLite
    sp = os.path.join(a.work, "log.db")
    con = sqlite3.connect(sp, isolation_level=None)
    con.execute("PRAGMA synchronous=OFF")
    sqlt = {"I64": "INTEGER", "F64": "REAL", "Bool": "INTEGER", "Str": "TEXT"}
    for t in tables:
        defs = []
        for c, ty in schema[t]:
            d = f"{c} {sqlt[ty]}"
            if t == "events" and c == "event_id":
                d += " PRIMARY KEY"
            defs.append(d)
        con.execute(f"CREATE TABLE {t}({', '.join(defs)})")
    t0 = time.perf_counter()
    for t, rows in tables.items():
        q = f"INSERT INTO {t} VALUES ({','.join('?' * len(schema[t]))})"
        by_v = collections.OrderedDict()
        for r in rows:
            by_v.setdefault(r[0], []).append(r)
        for v, rs in by_v.items():  # one transaction per version (= per log commit)
            con.execute("BEGIN")
            con.executemany(q, rs)
            con.execute("COMMIT")
    sqlite_write = time.perf_counter() - t0
    con.close()
    s_size = dsize(sp)
    con = sqlite3.connect(sp, isolation_level=None)
    t0 = time.perf_counter()
    con.execute("VACUUM")
    vac = time.perf_counter() - t0
    con.close()
    con = sqlite3.connect(sp)
    sq_full = lambda: {t: len(con.execute(f"SELECT * FROM {t}").fetchall()) for t in tables}
    sq_proj = lambda: collections.Counter(r[0] for r in con.execute("SELECT kind FROM events"))
    sq_lk = lambda: sum(len(con.execute("SELECT text FROM events WHERE event_id=?", (i,)).fetchone()[0].encode()) for i in lk_ids)
    check_full(sq_full())
    check_proj(sq_proj())
    assert sq_lk() == want_lk
    full, proj, look = stats(timed(sq_full, a.reps)), stats(timed(sq_proj, a.reps)), stats([x / L for x in timed(sq_lk, a.reps)])
    con.close()
    note = "synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup)"
    record("SQLite", s_size, sqlite_write, full, proj, look, note=note)
    record("SQLite after VACUUM", dsize(sp), sqlite_write + vac, full, proj, look, note=note + f"; reads measured on the vacuumed file; VACUUM {vac:.3f}s")
    os.remove(sp)

    if pa is None:
        R["formats"]["Parquet/ORC/Arrow IPC"] = "not measured (pyarrow not importable)"
    else:
        # ------------------------------------------------------------ Arrow-based formats
        pat = {"I64": pa.int64(), "F64": pa.float64(), "Bool": pa.bool_(), "Str": pa.string()}
        at = {}
        for t, rows in tables.items():
            arrays = [pa.array([r[k] for r in rows], type=pat[ty]) for k, (c, ty) in enumerate(schema[t])]
            at[t] = pa.table(arrays, names=[c for c, _ in schema[t]])

        def hexcols(tbl):
            out = []
            for name in tbl.column_names:
                col = tbl.column(name)
                if col.type == pa.string():
                    vals = [v for v in col.to_pylist() if v is not None]
                    if vals and all(len(v) == 64 and all(ch in "0123456789abcdef" for ch in v) for v in vals[:1000]) and all(len(v) == 64 for v in vals):
                        col = pa.array([bytes.fromhex(v) if v is not None else None for v in col.to_pylist()], type=pa.binary(32))
                out.append(col)
            return pa.table(out, names=tbl.column_names)

        at_bin = {t: hexcols(tb) for t, tb in at.items()}

        def arrow_format(name, ext, write, read_full, read_cols, lookup, tables_in=at, note=""):
            d = os.path.join(a.work, name.replace(" ", "_").replace("/", "_").replace("(", "").replace(")", ""))
            os.makedirs(d)
            paths = {t: os.path.join(d, f"{t}.{ext}") for t in tables_in}
            t0 = time.perf_counter()
            for t, tb in tables_in.items():
                write(tb, paths[t])
            w = time.perf_counter() - t0
            fr = lambda: {t: read_full(p).num_rows for t, p in paths.items()}
            pr = lambda: collections.Counter(read_cols(paths["events"], ["kind"]).column("kind").to_pylist())
            check_full(fr())
            check_proj(pr())
            lkf = None
            if lookup is not None:
                lf = lambda: sum(lookup(paths["events"], i) for i in lk_ids)
                assert lf() == want_lk, name
                lkf = stats([x / L for x in timed(lf, a.reps)])
            record(name, dsize(d), w, stats(timed(fr, a.reps)), stats(timed(pr, a.reps)), lkf, note=note)
            shutil.rmtree(d)

        def pq_lookup(p, id_):
            key = bytes.fromhex(id_) if pq.read_schema(p).field("event_id").type == pa.binary(32) else id_
            tb = pq.read_table(p, columns=["text"], filters=[("event_id", "==", key)])
            return len(tb.column("text")[0].as_py().encode())

        pqn = "lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column)"
        arrow_format("Parquet zstd (default level)", "parquet", lambda tb, p: pq.write_table(tb, p, compression="zstd"),
                     pq.read_table, lambda p, c: pq.read_table(p, columns=c), pq_lookup, note=pqn)
        ALL and arrow_format("Parquet zstd level 22 (max)", "parquet", lambda tb, p: pq.write_table(tb, p, compression="zstd", compression_level=22),
                     pq.read_table, lambda p, c: pq.read_table(p, columns=c), pq_lookup, note=pqn)
        ALL and arrow_format("Parquet snappy", "parquet", lambda tb, p: pq.write_table(tb, p, compression="snappy"),
                     pq.read_table, lambda p, c: pq.read_table(p, columns=c), pq_lookup, note=pqn)
        ALL and arrow_format("Parquet zstd (default), ids as binary(32)", "parquet", lambda tb, p: pq.write_table(tb, p, compression="zstd"),
                     pq.read_table, lambda p, c: pq.read_table(p, columns=c), pq_lookup, tables_in=at_bin,
                     note=pqn + "; 64-hex id columns stored as fixed_size_binary(32), like KORE v4 HexFixed")

        def orc_lookup(p, id_):
            tb = orc.read_table(p, columns=["event_id", "text"])
            i = pc.index(tb.column("event_id"), id_).as_py()
            return len(tb.column("text")[i].as_py().encode())

        ALL and arrow_format("ORC zstd", "orc", lambda tb, p: orc.write_table(tb, p, compression="zstd"), orc.read_table,
                     lambda p, c: orc.read_table(p, columns=c), orc_lookup, note="lookup = read event_id+text columns, pc.index (no predicate pushdown in pyarrow.orc)")

        def ipc_lookup(p, id_):
            tb = feather.read_table(p, columns=["event_id", "text"], memory_map=True)
            i = pc.index(tb.column("event_id"), id_).as_py()
            return len(tb.column("text")[i].as_py().encode())

        for comp in (("uncompressed", "lz4", "zstd") if ALL else ()):
            arrow_format(f"Arrow IPC / Feather v2 {comp}", "arrow", lambda tb, p, comp=comp: feather.write_feather(tb, p, compression=comp),
                         lambda p: feather.read_table(p, memory_map=True), lambda p, c: feather.read_table(p, columns=c, memory_map=True), ipc_lookup,
                         note="lookup = memory-mapped read of event_id+text, pc.index")

        # ------------------------------------------------------------ DuckDB
        if duckdb is None:
            R["formats"]["DuckDB"] = "not measured (duckdb not importable)"
        else:
            dp = os.path.join(a.work, "log.duckdb")
            dcon = duckdb.connect(dp)
            t0 = time.perf_counter()
            for t, tb in at.items():
                dcon.register("src", tb)
                dcon.execute(f"CREATE TABLE {t} AS SELECT * FROM src")
                dcon.unregister("src")
            dw = time.perf_counter() - t0
            size_default = dsize(dp) + (dsize(dp + ".wal") if os.path.exists(dp + ".wal") else 0)
            t0 = time.perf_counter()
            dcon.execute("CHECKPOINT")
            ck = time.perf_counter() - t0
            dcon.close()
            size_ck = dsize(dp) + (dsize(dp + ".wal") if os.path.exists(dp + ".wal") else 0)
            dcon = duckdb.connect(dp, read_only=True)
            dfull = lambda: {t: dcon.execute(f"SELECT * FROM {t}").fetch_arrow_table().num_rows for t in tables}
            dproj = lambda: collections.Counter(dcon.execute("SELECT kind FROM events").fetch_arrow_table().column("kind").to_pylist())
            dlk = lambda: sum(len(dcon.execute("SELECT text FROM events WHERE event_id = ?", [i]).fetchone()[0].encode()) for i in lk_ids)
            check_full(dfull())
            check_proj(dproj())
            assert dlk() == want_lk
            full, proj, look = stats(timed(dfull, a.reps)), stats(timed(dproj, a.reps)), stats([x / L for x in timed(dlk, a.reps)])
            dcon.close()
            dn = "CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan)"
            record("DuckDB .duckdb (before explicit CHECKPOINT)", size_default, dw, full, proj, look,
                   note=dn + "; size = db file + WAL right after loading; reads measured on the checkpointed file")
            record("DuckDB .duckdb after CHECKPOINT", size_ck, dw + ck, full, proj, look, note=dn + f"; CHECKPOINT {ck:.3f}s")
            os.remove(dp)
    shutil.rmtree(a.work, ignore_errors=True)
    json.dump(R, open(a.out, "w"), indent=1)


if __name__ == "__main__":
    main()
