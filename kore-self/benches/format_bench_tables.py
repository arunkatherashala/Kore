"""Build the markdown result tables from kore_N.json / py_N.json and cross-check results.

  python -I format_bench_tables.py RESULTS_DIR > tables.md
"""
import json
import os
import sys

d = sys.argv[1]
Ns = [n for n in (10000, 100000, 1000000) if os.path.exists(os.path.join(d, f"kore_{n}.json")) and os.path.exists(os.path.join(d, f"py_{n}.json"))]
K = {n: json.load(open(os.path.join(d, f"kore_{n}.json"))) for n in Ns}
P = {n: json.load(open(os.path.join(d, f"py_{n}.json"))) for n in Ns}


def t(x):
    if x is None:
        return "-"
    if x >= 1:
        return f"{x:.2f} s"
    if x >= 1e-3:
        return f"{x*1e3:.2f} ms"
    if x >= 1e-6:
        return f"{x*1e6:.1f} us"
    return f"{x*1e9:.0f} ns"


def msf(x):
    return f"{x:.2f} ms"


def rate(x):
    return f"{x:,.0f}"


def mmm(s, f=t):
    if s is None:
        return "-"
    return f"{f(s['median'])} [{f(s['min'])}-{f(s['max'])}] (n={s['reps']})"


def mb(b):
    return f"{b/1e6:.2f} MB"


def table(title, rows, note=None):
    print(f"\n#### {title}\n")
    print("| system | " + " | ".join(f"N={n:,}" for n in Ns) + " |")
    print("|---|" + "---|" * len(Ns))
    for name, fn in rows:
        cells = []
        for n in Ns:
            try:
                cells.append(fn(n))
            except (KeyError, TypeError):
                cells.append("n/a")
        print(f"| {name} | " + " | ".join(cells) + " |")
    if note:
        print(f"\n{note}")


def py(sysname, *path):
    def f(n, g=lambda x: x):
        x = P[n]["systems"][sysname]
        for p in path:
            x = x[p]
        return x
    return f


def kget(n, *path):
    x = K[n]
    for p in path:
        x = x[p]
    return x


def pget(n, sysname, *path):
    x = P[n]["systems"][sysname]
    for p in path:
        x = x[p]
    return x


print("### Consistency checks (same query result on every system)\n")
ok_all = True
for n in Ns:
    lines = []
    exp = P[n]["systems"]["expected_lookup_check"]
    kc = kget(n, "lookup", "check")
    lines.append(("lookup", kc == exp and pget(n, "sqlite", "lookup_check") == exp and pget(n, "sqlite_idx", "lookup_check") == exp
                  and pget(n, "csv", "lookup_check_first") == pget(n, "csv", "lookup_expected_first")))
    kr = kget(n, "range", "check")
    lines.append(("range", kr == kget(n, "range", "native_rust_check") == pget(n, "sqlite", "range_check") == pget(n, "sqlite_idx", "range_check") == pget(n, "sqlite_cache", "range_check") == pget(n, "csv", "range_check")))
    kf = kget(n, "fullagg", "check")
    lines.append(("fullagg", kf == kget(n, "fullagg", "native_rust_check") == pget(n, "sqlite", "fullagg_check") == pget(n, "sqlite_idx", "fullagg_check") == pget(n, "sqlite_cache", "fullagg_check") == pget(n, "csv", "fullagg_check")))
    for v in K[n]["asof"]:
        c = kget(n, "asof", v, "check")
        lines.append((f"asof v{v}", c == pget(n, "sqlite", "asof", v, "check") == pget(n, "sqlite_idx", "asof", v, "check") == pget(n, "sqlite_cache", "asof", v, "check") == pget(n, "csv", "asof", v, "check")))
    ok_all &= all(x[1] for x in lines)
    print(f"- N={n:,}: " + ", ".join(f"{a}={'MATCH' if b else 'MISMATCH'}" for a, b in lines))
print(f"\nAll checks match: **{ok_all}**")

print("\n### Run conditions\n")
for n in Ns:
    m = P[n]["meta"]
    print(f"- N={n:,}: python {m['python'].split()[0]}, sqlite {m['sqlite']}, {m['platform']}, cpu_count={m['cpu_count']}, "
          f"reps={m['reps']}, build_reps={m['build_reps']}, csv_lookup_ops={m['csv_lookups']}, started {m['started']}, finished {m['finished']}")

table("(a) On-disk size after loading the dataset in batches of 1000", [
    ("KORE versioned log (events+revisions+causes+MANIFEST)", lambda n: mb(kget(n, "size_bytes"))),
    ("SQLite (PK on event_id, version column)", lambda n: mb(pget(n, "sqlite", "size_bytes"))),
    ("SQLite [256 MiB cache] (PK on event_id, version column)", lambda n: mb(pget(n, "sqlite_cache", "size_bytes"))),
    ("SQLite + 3 secondary indexes", lambda n: mb(pget(n, "sqlite_idx", "size_bytes"))),
    ("CSV (3 files, version column)", lambda n: mb(pget(n, "csv", "size_bytes"))),
], "Raw JSONL input for reference: " + ", ".join(f"N={n:,}: {mb(os.path.getsize(os.path.join(d, 'data_%d.jsonl' % n))) if os.path.exists(os.path.join(d, 'data_%d.jsonl' % n)) else 'n/a'}" for n in Ns) +
      ". KORE file count: " + ", ".join(f"N={n:,}: {kget(n, 'file_count')}" for n in Ns) + ".")

table("(b1) Bulk load, batches of 1000 events, fsync per commit: events/s (from-scratch build, includes ~N/50 revisions)", [
    ("KORE", lambda n: mmm(kget(n, "build_b1000_events_per_s"), rate)),
    ("SQLite (DELETE journal, synchronous=FULL)", lambda n: mmm(pget(n, "sqlite", "build_b1000_events_per_s"), rate)),
    ("SQLite [256 MiB cache] (DELETE journal, synchronous=FULL)", lambda n: mmm(pget(n, "sqlite_cache", "build_b1000_events_per_s"), rate)),
    ("SQLite + indexes (same sync)", lambda n: mmm(pget(n, "sqlite_idx", "build_b1000_events_per_s"), rate)),
    ("CSV (append + flush + fsync per batch)", lambda n: mmm(pget(n, "csv", "build_b1000_events_per_s"), rate)),
])
table("(b2) Append of 1 event per commit onto an existing N-event store: ms per commit (100 commits per rep)", [
    ("KORE (one new version per event)", lambda n: mmm(kget(n, "append", "b1_ms_per_commit"), msf)),
    ("SQLite DELETE/FULL (default)", lambda n: mmm(pget(n, "sqlite", "append", "b1_ms_per_commit"), msf)),
    ("SQLite [256 MiB cache] DELETE/FULL (default)", lambda n: mmm(pget(n, "sqlite_cache", "append", "b1_ms_per_commit"), msf)),
    ("SQLite DELETE/FULL + indexes", lambda n: mmm(pget(n, "sqlite_idx", "append", "b1_ms_per_commit"), msf)),
    ("SQLite WAL/FULL", lambda n: mmm(pget(n, "sqlite_wal_full", "append", "b1_ms_per_commit"), msf)),
    ("SQLite DELETE/synchronous=OFF (no fsync)", lambda n: mmm(pget(n, "sqlite_off", "append", "b1_ms_per_commit"), msf)),
    ("CSV fsync", lambda n: mmm(pget(n, "csv", "append", "b1_ms_per_commit"), msf)),
    ("CSV no fsync", lambda n: mmm(pget(n, "csv_nofsync", "append", "b1_ms_per_commit"), msf)),
])
table("(b3) Append of one 1000-event batch onto an existing N-event store: ms per batch", [
    ("KORE", lambda n: mmm(kget(n, "append", "b1000_ms_per_batch"), msf)),
    ("SQLite DELETE/FULL (default)", lambda n: mmm(pget(n, "sqlite", "append", "b1000_ms_per_batch"), msf)),
    ("SQLite [256 MiB cache] DELETE/FULL (default)", lambda n: mmm(pget(n, "sqlite_cache", "append", "b1000_ms_per_batch"), msf)),
    ("SQLite DELETE/FULL + indexes", lambda n: mmm(pget(n, "sqlite_idx", "append", "b1000_ms_per_batch"), msf)),
    ("SQLite WAL/FULL", lambda n: mmm(pget(n, "sqlite_wal_full", "append", "b1000_ms_per_batch"), msf)),
    ("SQLite DELETE/synchronous=OFF (no fsync)", lambda n: mmm(pget(n, "sqlite_off", "append", "b1000_ms_per_batch"), msf)),
    ("CSV fsync", lambda n: mmm(pget(n, "csv", "append", "b1000_ms_per_batch"), msf)),
    ("CSV no fsync", lambda n: mmm(pget(n, "csv_nofsync", "append", "b1000_ms_per_batch"), msf)),
])

table("(c) Point lookup by event id, per lookup (store already open; row returned unless noted)", [
    ("KORE has_event (hash index, existence only, no row)", lambda n: mmm(kget(n, "lookup", "has_event_existence_only_per_op_s"))),
    ("KORE Rust linear scan of the in-memory snapshot (early exit)", lambda n: mmm(kget(n, "lookup", "native_scan_per_op_s"))),
    ("KORE SQL (kore-sql) on a pre-registered context", lambda n: mmm(kget(n, "lookup", "sql_registered_ctx_per_op_s"))),
    ("KORE SQL end-to-end run_readonly_sql (re-registers tables each call)", lambda n: mmm(kget(n, "lookup", "sql_end_to_end_run_readonly_sql_per_op_s"))),
    ("SQLite PRIMARY KEY lookup", lambda n: mmm(pget(n, "sqlite", "lookup_per_op_s"))),
    ("SQLite [256 MiB cache] PRIMARY KEY lookup", lambda n: mmm(pget(n, "sqlite_cache", "lookup_per_op_s"))),
    ("SQLite + indexes PK lookup", lambda n: mmm(pget(n, "sqlite_idx", "lookup_per_op_s"))),
    ("CSV full-file scan with early exit (re-opens the file each lookup)", lambda n: mmm(pget(n, "csv", "lookup_per_op_s"))),
])
table("(c2) Open / first-query cost (warm OS file cache)", [
    ("KORE open() (reads + sha256-verifies every segment, builds index)", lambda n: mmm(kget(n, "open_s"))),
    ("KORE verify() (full re-verify from disk)", lambda n: mmm(kget(n, "verify_s"))),
    ("KORE register_into (snapshot -> SQL tables, one run)", lambda n: t(kget(n, "register_into_s"))),
    ("SQLite connect + first indexed query (one run)", lambda n: t(pget(n, "sqlite", "cold_open_first_query_s"))),
])

table("(d1) Range aggregate: COUNT by kind for the middle 10% of the time range", [
    ("KORE SQL, pre-registered context", lambda n: mmm(kget(n, "range", "sql_registered_ctx_s"))),
    ("KORE SQL end-to-end run_readonly_sql", lambda n: mmm(kget(n, "range", "sql_end_to_end_s"))),
    ("KORE hand-written Rust loop over snapshot rows", lambda n: mmm(kget(n, "range", "native_rust_loop_s"))),
    ("SQLite (no index on occurred_at: scan)", lambda n: mmm(pget(n, "sqlite", "range_s"))),
    ("SQLite [256 MiB cache] (no index on occurred_at: scan)", lambda n: mmm(pget(n, "sqlite_cache", "range_s"))),
    ("SQLite + index on occurred_at", lambda n: mmm(pget(n, "sqlite_idx", "range_s"))),
    ("CSV scan (csv module)", lambda n: mmm(pget(n, "csv", "range_s"))),
])
table("(d2) Full-table aggregate: COUNT by source", [
    ("KORE SQL, pre-registered context", lambda n: mmm(kget(n, "fullagg", "sql_registered_ctx_s"))),
    ("KORE SQL end-to-end run_readonly_sql", lambda n: mmm(kget(n, "fullagg", "sql_end_to_end_s"))),
    ("KORE hand-written Rust loop", lambda n: mmm(kget(n, "fullagg", "native_rust_loop_s"))),
    ("SQLite", lambda n: mmm(pget(n, "sqlite", "fullagg_s"))),
    ("SQLite [256 MiB cache]", lambda n: mmm(pget(n, "sqlite_cache", "fullagg_s"))),
    ("SQLite + indexes", lambda n: mmm(pget(n, "sqlite_idx", "fullagg_s"))),
    ("CSV scan", lambda n: mmm(pget(n, "csv", "fullagg_s"))),
])

for which, label in (("first", 0), ("mid", 1), ("last", 2)):
    def ver(n, label=label):
        vs = sorted(int(v) for v in K[n]["asof"])
        return str(vs[min(label, len(vs) - 1)])
    rows = []
    rows.append(("KORE native AS OF: open_at (disk read + sha256 of segments 0..v)", lambda n, ver=ver: mmm(kget(n, "asof", ver(n), "open_at_s"))))
    rows.append(("KORE register_into (snapshot -> SQL tables)", lambda n, ver=ver: mmm(kget(n, "asof", ver(n), "register_s"))))
    rows.append(("KORE 2 queries (events by kind + beliefs_current)", lambda n, ver=ver: mmm(kget(n, "asof", ver(n), "queries_s"))))
    rows.append(("**KORE total** (open_at + register + queries)", lambda n, ver=ver: mmm(kget(n, "asof", ver(n), "total_s"))))
    rows.append(("SQLite emulated (version column, no index)", lambda n, ver=ver: mmm(pget(n, "sqlite", "asof", ver(n), "s"))))
    rows.append(("SQLite emulated [256 MiB cache] (no index)", lambda n, ver=ver: mmm(pget(n, "sqlite_cache", "asof", ver(n), "s"))))
    rows.append(("SQLite emulated + indexes on version", lambda n, ver=ver: mmm(pget(n, "sqlite_idx", "asof", ver(n), "s"))))
    rows.append(("CSV scan with version filter", lambda n, ver=ver: mmm(pget(n, "csv", "asof", ver(n), "s"))))
    table(f"(e) State as of version v = {which} version (v = " + ", ".join(f"{n:,}: v{ver(n)}" for n in Ns) + ")", rows)

print("\n#### (f) Tamper detection: one random bit flipped per trial, uniform over all bytes of the store (SQLite: the .db file; CSV: events.csv; KORE: every file incl. MANIFEST)\n")
print("| system | N | trials | detected by check | detected only by read error | silent data change | harmless (data identical) |")
print("|---|---|---|---|---|---|---|")
for n in Ns:
    if "tamper" in K[n]:
        x = K[n]["tamper"]
        print(f"| KORE verify()/self_verify | {n:,} | {x['trials']} | {x['detected_by_check']} | {x['detected_only_by_open']} (open) | {x['silent_data_change']} | {x['harmless_no_change']} |")
    for sname, lab in (("sqlite", "SQLite PRAGMA integrity_check"), ("csv", "CSV strict parse + 9-column check")):
        x = P[n]["systems"][sname].get("tamper")
        if x:
            print(f"| {lab} | {n:,} | {x['trials']} | {x['detected_by_check']} | {x['detected_only_by_read_error']} | {x['silent_data_change']} | {x['harmless_no_change']} |")
print("\nKORE by file kind and outcome: " + "; ".join(f"N={n:,}: {K[n]['tamper']['by_dir_and_outcome']}" for n in Ns if 'tamper' in K[n]))

print("\n#### (g) Idempotent re-ingest of the same N events (batches of 1000)\n")
print("| system | N | rows before | rows after | new versions/commits | time | size change | note |")
print("|---|---|---|---|---|---|---|---|")
for n in Ns:
    r = K[n]["reingest_events"]
    print(f"| KORE append_events | {n:,} | {r['events_rows_before']:,} | {r['events_rows_after']:,} | {r['new_versions_created']} | {t(r['seconds'])} | {r['size_delta_bytes']} B | native; no version created |")
    r = P[n]["systems"]["sqlite"]["reingest"]
    print(f"| SQLite INSERT OR IGNORE (PK) | {n:,} | {r['rows_before']:,} | {r['rows_after']:,} | 0 (rolled back) | {t(r['seconds'])} | {r['size_delta_bytes']} B | native via PK; id computed by the app |")
    r = P[n]["systems"]["csv"]["reingest"]
    print(f"| CSV naive append | {n:,} | {r['rows_before']:,} | {r['rows_after_naive']:,} | - | {t(r['naive_seconds'])} | +{r['size_delta_bytes_naive']:,} B | duplicates: not available natively |")
    print(f"| CSV emulated (scan ids first) | {n:,} | - | - | - | {t(r['emulated_dedup_seconds'])} | {r['emulated_size_delta_bytes']} B | emulated in app code |")
print("\nKORE revision replay (extra observation, same revisions re-submitted in original order): " +
      "; ".join(f"N={n:,}: revisions {K[n]['reingest_revisions_replay']['revisions_before']} -> {K[n]['reingest_revisions_replay']['revisions_after']}, versions {K[n]['reingest_revisions_replay']['versions_before']} -> {K[n]['reingest_revisions_replay']['versions_after']}" for n in Ns))
