"""Storage-efficiency side (SQLite / CSV / gzip-CSV) for docs/format_bench.md, section 'Storage efficiency'.

  python -I format_bench_size_py.py --data DATA.jsonl --n N --work WORKDIR --out RESULT.json

Builds the same logical store as format_bench_py.py (events + revisions + revision_causes, `version` column,
batches of 1000; no fsync here because only sizes and read paths are measured), then reports sizes and
the cost of answering a point lookup / a range query from each representation.
"""
import argparse
import csv
import datetime
import gzip
import hashlib
import io
import json
import os
import shutil
import sqlite3
import statistics
import time

BATCH = 1000
EVENT_COLS = ["event_id", "version", "occurred_at", "recorded_at", "source", "kind", "text", "tags", "redacted"]
REV_COLS = ["seq", "version", "rev_id", "entity_type", "key", "stance", "confidence", "rationale", "supersedes", "valid_from", "author", "recorded_at"]
CAUSE_COLS = ["rev_id", "event_id"]
REC = "2026-01-01T00:00:00Z"


def event_id(source, ts, text):
    return hashlib.sha256(f"{source}|{ts}|{' '.join(text.split())}".encode()).hexdigest()


def med(xs):
    return {"median": statistics.median(xs), "min": min(xs), "max": max(xs), "reps": len(xs)}


def timed(fn, reps):
    xs = []
    last = None
    for _ in range(reps):
        t = time.perf_counter()
        last = fn()
        xs.append(time.perf_counter() - t)
    return xs, last


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True)
    ap.add_argument("--n", type=int, required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--reps", type=int, default=5)
    a = ap.parse_args()
    N = a.n
    shutil.rmtree(a.work, ignore_errors=True)
    os.makedirs(a.work)
    events, revs = [], []
    with open(a.data, encoding="utf-8") as f:
        for line in f:
            o = json.loads(line)
            if o["t"] == "e":
                events.append((o["source"], o["occurred_at"], o["kind"], o["text"], o["tags"]))
            else:
                revs.append(o)
    assert len(events) == N
    R = {"n": N, "raw_text_bytes": sum(len(e[3].encode()) for e in events), "raw_jsonl_bytes": os.path.getsize(a.data),
         "avg_text_bytes": sum(len(e[3].encode()) for e in events) / N}
    ids = [event_id(e[0], e[1], e[3]) for e in events]
    by_batch = {}
    for r in revs:
        by_batch.setdefault(r["cause"] // BATCH, []).append(r)

    # ---- CSV and SQLite
    cd = os.path.join(a.work, "csv")
    os.makedirs(cd)
    fe = open(os.path.join(cd, "events.csv"), "w", newline="", encoding="utf-8")
    fr = open(os.path.join(cd, "revisions.csv"), "w", newline="", encoding="utf-8")
    fc = open(os.path.join(cd, "revision_causes.csv"), "w", newline="", encoding="utf-8")
    we, wr, wc = csv.writer(fe), csv.writer(fr), csv.writer(fc)
    we.writerow(EVENT_COLS)
    wr.writerow(REV_COLS)
    wc.writerow(CAUSE_COLS)
    sp = os.path.join(a.work, "log.db")
    con = sqlite3.connect(sp, isolation_level=None)
    con.execute("PRAGMA synchronous=OFF")
    con.execute("CREATE TABLE events(event_id TEXT PRIMARY KEY, version INTEGER NOT NULL, occurred_at TEXT NOT NULL, recorded_at TEXT NOT NULL, source TEXT NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL, tags TEXT NOT NULL, redacted INTEGER NOT NULL)")
    con.execute("CREATE TABLE revisions(seq INTEGER PRIMARY KEY, version INTEGER NOT NULL, rev_id TEXT NOT NULL UNIQUE, entity_type TEXT NOT NULL, key TEXT NOT NULL, stance TEXT NOT NULL, confidence REAL NOT NULL, rationale TEXT NOT NULL, supersedes TEXT, valid_from TEXT NOT NULL, author TEXT NOT NULL, recorded_at TEXT NOT NULL)")
    con.execute("CREATE TABLE revision_causes(rev_id TEXT NOT NULL, event_id TEXT NOT NULL)")
    heads, seq = {}, 0
    for b in range((N + BATCH - 1) // BATCH):
        lo = b * BATCH
        rows = [(ids[i], b + 1, events[i][1], REC, events[i][0], events[i][2], events[i][3], events[i][4], 0) for i in range(lo, min(lo + BATCH, N))]
        con.execute("BEGIN")
        con.executemany("INSERT INTO events VALUES (?,?,?,?,?,?,?,?,?)", rows)
        we.writerows(rows)
        for r in by_batch.get(b, []):
            sup = heads.get(r["key"])
            rid = hashlib.sha256(f'{r["key"]}|{r["stance"]}|{r["conf"]!r}|{r["rationale"]}|{sup}|{r["valid_from"]}'.encode()).hexdigest()
            seq += 1
            rv = (seq, b + 1, rid, "belief", r["key"], r["stance"], r["conf"], r["rationale"], sup, r["valid_from"], "rule:bench", REC)
            con.execute("INSERT INTO revisions VALUES (?,?,?,?,?,?,?,?,?,?,?,?)", rv)
            con.execute("INSERT INTO revision_causes VALUES (?,?)", (rid, ids[r["cause"]]))
            wr.writerow([x if x is not None else "" for x in rv])
            wc.writerow([rid, ids[r["cause"]]])
            heads[r["key"]] = rid
        con.execute("COMMIT")
    for f in (fe, fr, fc):
        f.close()
    con.close()
    csv_files = [os.path.join(cd, n) for n in ("events.csv", "revisions.csv", "revision_causes.csv")]
    R["csv_bytes"] = sum(os.path.getsize(p) for p in csv_files)
    R["sqlite_bytes"] = os.path.getsize(sp)
    con = sqlite3.connect(sp, isolation_level=None)
    t = time.perf_counter()
    con.execute("VACUUM")
    R["sqlite_vacuum_s"] = time.perf_counter() - t
    con.close()
    R["sqlite_vacuum_bytes"] = os.path.getsize(sp)
    print("built csv/sqlite", R["csv_bytes"], R["sqlite_bytes"], R["sqlite_vacuum_bytes"], flush=True)

    # ---- gzip of the CSV files (each file compressed separately, sizes summed)
    gz = {}
    for lvl in (6, 9):
        tot, tc, td = 0, 0.0, 0.0
        for p in csv_files:
            q = f"{p}.l{lvl}.gz"
            t = time.perf_counter()
            with open(p, "rb") as fi, gzip.GzipFile(q, "wb", compresslevel=lvl, mtime=0) as fo:
                shutil.copyfileobj(fi, fo, 1 << 20)
            tc += time.perf_counter() - t
            tot += os.path.getsize(q)
            t = time.perf_counter()
            with gzip.open(q, "rb") as fi:
                while fi.read(1 << 20):
                    pass
            td += time.perf_counter() - t
        gz[lvl] = {"bytes": tot, "compress_s": tc, "full_decompress_s": td}
    R["gzip"] = gz
    print("gzip done", gz, flush=True)

    # ---- per-column raw size and gzip-6 size of the events columns (whole column, and in 1000-row chunks like the KORE segments)
    cols = {"event_id": ids, "occurred_at": [e[1] for e in events], "source": [e[0] for e in events], "kind": [e[2] for e in events],
            "text": [e[3] for e in events], "tags": [e[4] for e in events]}
    cb = {}
    for name, vals in cols.items():
        raw = sum(len(v.encode()) + 1 for v in vals)
        whole = len(gzip.compress(chr(10).join(vals).encode(), 6, mtime=0))
        chunked = sum(len(gzip.compress(chr(10).join(vals[i:i + BATCH]).encode(), 6, mtime=0)) for i in range(0, N, BATCH))
        cb[name] = {"raw": raw, "gzip6_whole_column": whole, "gzip6_1000row_chunks": chunked}
    R["events_column_sizes"] = cb

    # ---- queryability
    L = 100
    lk = [(k * 2654435761 + 12345) % N for k in range(L)]
    t1, t2 = events[N * 45 // 100][1], events[N * 55 // 100][1]
    con = sqlite3.connect(sp)
    xs, _ = timed(lambda: [con.execute("SELECT text FROM events WHERE event_id=?", (ids[i],)).fetchone() for i in lk], a.reps)
    R["sqlite_lookup_per_op_s"] = med([x / L for x in xs])
    xs, last = timed(lambda: sorted(con.execute("SELECT kind, COUNT(*) FROM events WHERE occurred_at >= ? AND occurred_at < ? GROUP BY kind", (t1, t2)).fetchall()), a.reps)
    R["sqlite_range_s"] = med(xs)
    R["range_check"] = hashlib.sha256(json.dumps(last).encode()).hexdigest()[:16]
    con.close()

    def scan(opener, ids_wanted):
        found = 0
        with opener() as f:
            r = csv.reader(io.TextIOWrapper(f, encoding="utf-8", newline=""))
            next(r)
            for row in r:
                if row[0] in ids_wanted:
                    found += 1
                    ids_wanted.discard(row[0])
                    if not ids_wanted:
                        break
        return found

    def range_scan(opener):
        cnt = {}
        with opener() as f:
            r = csv.reader(io.TextIOWrapper(f, encoding="utf-8", newline=""))
            next(r)
            for row in r:
                if t1 <= row[2] < t2:
                    cnt[row[5]] = cnt.get(row[5], 0) + 1
        return sorted(cnt.items())

    ev_csv = csv_files[0]
    for label, opener in (("csv", lambda: open(ev_csv, "rb")), ("gzip6_csv", lambda: gzip.open(ev_csv + ".l6.gz", "rb")), ("gzip9_csv", lambda: gzip.open(ev_csv + ".l9.gz", "rb"))):
        R[f"{label}_lookup_one_s"] = med(timed(lambda: scan(opener, {ids[N // 2]}), 3)[0])
        R[f"{label}_lookup_one_idx"] = N // 2
        xs, last = timed(lambda: range_scan(opener), 3)
        R[f"{label}_range_s"] = med(xs)
        R[f"{label}_range_result_rows"] = len(last)
        print(label, "queried", flush=True)
    # cross-check that range results agree between sqlite and the csv paths
    c2 = sqlite3.connect(sp)
    sq = [list(x) for x in sorted(c2.execute("SELECT kind, COUNT(*) FROM events WHERE occurred_at >= ? AND occurred_at < ? GROUP BY kind", (t1, t2)).fetchall())]
    c2.close()
    R["range_agree"] = [list(x) for x in range_scan(lambda: open(ev_csv, "rb"))] == sq
    with open(a.out, "w") as f:
        json.dump(R, f, indent=1)
    shutil.rmtree(a.work, ignore_errors=True)


if __name__ == "__main__":
    main()
