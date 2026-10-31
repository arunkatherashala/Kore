"""Diagnostic control for the SQLite batch-insert cost (not part of the main tables).

Question: is SQLite's slower batch insert at larger N caused by maintaining a unique index on a
random (sha256) key, rather than by the harness? Loads N events in batches of 1000 with
journal_mode=DELETE synchronous=FULL under three schemas and reports ms per batch (median over
batches, first 5 and last 5 batches separately).

  python -I format_bench_diag_sqlite.py DATA.jsonl WORKDIR
"""
import hashlib
import json
import os
import shutil
import sqlite3
import statistics
import sys
import time

data, work = sys.argv[1], sys.argv[2]
shutil.rmtree(work, ignore_errors=True)
os.makedirs(work)
rows = []
with open(data, encoding="utf-8") as f:
    for line in f:
        o = json.loads(line)
        if o["t"] == "e":
            rows.append((o["source"], o["occurred_at"], o["kind"], o["text"], o["tags"]))
N = len(rows)

SCHEMAS = {
    "A_pk_event_id_random_key (the benchmark schema)": "CREATE TABLE events(event_id TEXT PRIMARY KEY, version INTEGER, occurred_at TEXT, recorded_at TEXT, source TEXT, kind TEXT, text TEXT, tags TEXT, redacted INTEGER)",
    "B_no_unique_index (rowid only; NOT idempotent)": "CREATE TABLE events(event_id TEXT, version INTEGER, occurred_at TEXT, recorded_at TEXT, source TEXT, kind TEXT, text TEXT, tags TEXT, redacted INTEGER)",
    "C_without_rowid_pk_event_id": "CREATE TABLE events(event_id TEXT PRIMARY KEY, version INTEGER, occurred_at TEXT, recorded_at TEXT, source TEXT, kind TEXT, text TEXT, tags TEXT, redacted INTEGER) WITHOUT ROWID",
}
for name, ddl in SCHEMAS.items():
    p = os.path.join(work, name[:1] + ".db")
    con = sqlite3.connect(p, isolation_level=None)
    con.execute("PRAGMA journal_mode=DELETE")
    con.execute("PRAGMA synchronous=FULL")
    con.execute(ddl)
    ts = []
    for b in range(0, N, 1000):
        batch = [(hashlib.sha256(f"{r[0]}|{r[1]}|{r[3]}".encode()).hexdigest(), b // 1000 + 1, r[1], "2026-01-01T00:00:00Z", r[0], r[2], r[3], r[4], 0) for r in rows[b:b + 1000]]
        t = time.perf_counter()
        con.execute("BEGIN")
        con.executemany("INSERT INTO events VALUES (?,?,?,?,?,?,?,?,?)", batch)
        con.execute("COMMIT")
        ts.append((time.perf_counter() - t) * 1000)
    con.close()
    print(f"{name}: N={N} size={os.path.getsize(p)/1e6:.1f} MB  ms/batch median={statistics.median(ts):.1f} first5={statistics.median(ts[:5]):.1f} last5={statistics.median(ts[-5:]):.1f}")
shutil.rmtree(work, ignore_errors=True)
