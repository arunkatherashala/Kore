"""SQLite and CSV side of the log-format benchmark (Python stdlib only).

Run (note -I: isolated mode):
  python -I format_bench_py.py --data DATA.jsonl --n N --work WORKDIR --out RESULT.json [--build-reps 5] [--reps 5]

The data file is produced by format_bench_gen.py and is the same file the Rust
harness (examples/bench_log_formats.rs) reads. Every query returns a `check`
value that the table builder compares with the Rust side, so a harness bug that
makes one side compute something different shows up as a mismatch.

Systems measured here:
  sqlite      one table per KORE table, PRIMARY KEY on event_id, `version` column
              (emulated versioning), default journal (DELETE) + synchronous=FULL
  sqlite_idx  same + secondary indexes on events(occurred_at), events(version), revisions(version)
  csv         events.csv / revisions.csv / revision_causes.csv, `version` column
"""
import argparse
import csv
import datetime
import hashlib
import json
import os
import platform
import random
import shutil
import sqlite3
import statistics
import sys
import time

BATCH = 1000
EVENT_COLS = ["event_id", "version", "occurred_at", "recorded_at", "source", "kind", "text", "tags", "redacted"]
REV_COLS = ["seq", "version", "rev_id", "entity_type", "key", "stance", "confidence", "rationale", "supersedes",
            "valid_from", "author", "recorded_at"]
CSV_REV_COLS = REV_COLS
CAUSE_COLS = ["rev_id", "event_id"]


def now_iso():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def iso(secs):
    return datetime.datetime.fromtimestamp(secs, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def event_id(source, ts, text):
    norm = " ".join(text.split())
    return hashlib.sha256(f"{source}|{ts}|{norm}".encode()).hexdigest()


def append_event(k):
    return ("bench-append", iso(1893456000 + k), "note", f"append event {k} of the bench", "bench")


def stats(xs):
    return {"median": statistics.median(xs), "min": min(xs), "max": max(xs), "reps": len(xs)}


def dir_size(d):
    t = 0
    for root, _, files in os.walk(d):
        for f in files:
            t += os.path.getsize(os.path.join(root, f))
    return t


def load(path):
    events, revs = [], []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            o = json.loads(line)
            if o["t"] == "e":
                events.append((o["source"], o["occurred_at"], o["kind"], o["text"], o["tags"]))
            else:
                revs.append(o)
    return events, revs


def make_batches(events, revs):
    """List of (events, revisions) per batch of BATCH events; revision goes with the batch of its cited event."""
    by_batch = {}
    for r in revs:
        by_batch.setdefault(r["cause"] // BATCH, []).append(r)
    out = []
    for b in range(0, (len(events) + BATCH - 1) // BATCH):
        out.append((events[b * BATCH:(b + 1) * BATCH], by_batch.get(b, [])))
    return out


# --------------------------------------------------------------------------- stores

class SqliteStore:
    def __init__(self, path, indexes=False, journal="DELETE", sync="FULL"):
        self.path = path
        self.con = sqlite3.connect(path, isolation_level=None)
        self.con.execute(f"PRAGMA journal_mode={journal}").fetchall()
        self.con.execute(f"PRAGMA synchronous={sync}")
        self.heads = {}
        self.seq = 0
        c = self.con
        c.execute("CREATE TABLE IF NOT EXISTS events(event_id TEXT PRIMARY KEY, version INTEGER NOT NULL, occurred_at TEXT NOT NULL,"
                  " recorded_at TEXT NOT NULL, source TEXT NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL, tags TEXT NOT NULL, redacted INTEGER NOT NULL)")
        c.execute("CREATE TABLE IF NOT EXISTS revisions(seq INTEGER PRIMARY KEY, version INTEGER NOT NULL, rev_id TEXT NOT NULL UNIQUE,"
                  " entity_type TEXT NOT NULL, key TEXT NOT NULL, stance TEXT NOT NULL, confidence REAL NOT NULL, rationale TEXT NOT NULL,"
                  " supersedes TEXT, valid_from TEXT NOT NULL, author TEXT NOT NULL, recorded_at TEXT NOT NULL)")
        c.execute("CREATE TABLE IF NOT EXISTS revision_causes(rev_id TEXT NOT NULL, event_id TEXT NOT NULL)")
        if indexes:
            c.execute("CREATE INDEX IF NOT EXISTS ix_ev_occ ON events(occurred_at)")
            c.execute("CREATE INDEX IF NOT EXISTS ix_ev_ver ON events(version)")
            c.execute("CREATE INDEX IF NOT EXISTS ix_rev_ver ON revisions(version)")

    def ingest(self, evs, revs, version, ids=None):
        """One transaction = one version. Returns number of new events (0 => rolled back, no version)."""
        c = self.con
        rec = now_iso()
        rows = []
        idl = []
        for (src, ts, kind, text, tags) in evs:
            eid = event_id(src, ts, text)
            idl.append(eid)
            rows.append((eid, version, ts, rec, src, kind, text, tags, 0))
        c.execute("BEGIN")
        cur = c.executemany("INSERT OR IGNORE INTO events VALUES (?,?,?,?,?,?,?,?,?)", rows)
        new_events = cur.rowcount
        new_revs = 0
        for r in revs:
            key = r["key"]
            sup = self.heads.get(key)
            rid = hashlib.sha256(f'{key}|{r["stance"]}|{r["conf"]!r}|{r["rationale"]}|{sup}|{r["valid_from"]}'.encode()).hexdigest()
            self.seq += 1
            cur = c.execute("INSERT OR IGNORE INTO revisions VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
                            (self.seq, version, rid, "belief", key, r["stance"], r["conf"], r["rationale"], sup, r["valid_from"], "rule:bench", rec))
            if cur.rowcount:
                new_revs += 1
                self.heads[key] = rid
                c.execute("INSERT INTO revision_causes VALUES (?,?)", (rid, idl[r["cause"] - (r["cause"] // BATCH) * BATCH]))
            else:
                self.seq -= 1
        if new_events + new_revs == 0:
            c.execute("ROLLBACK")
            return 0
        c.execute("COMMIT")
        return new_events

    def close(self):
        self.con.close()


class CsvStore:
    def __init__(self, d, fsync=True):
        self.d = d
        os.makedirs(d, exist_ok=True)
        self.fsync = fsync
        self.files = {}
        self.writers = {}
        for name, cols in (("events", EVENT_COLS), ("revisions", CSV_REV_COLS), ("revision_causes", CAUSE_COLS)):
            p = os.path.join(d, name + ".csv")
            new = not os.path.exists(p)
            f = open(p, "a", newline="", encoding="utf-8")
            w = csv.writer(f)
            if new:
                w.writerow(cols)
            self.files[name] = f
            self.writers[name] = w
        self.heads = {}
        self.seq = 0
        for f in self.files.values():
            f.flush()

    def ingest(self, evs, revs, version, check_dups=None):
        """check_dups: optional set of known event ids (emulated idempotence)."""
        rec = now_iso()
        we = self.writers["events"]
        idl = []
        new = 0
        for (src, ts, kind, text, tags) in evs:
            eid = event_id(src, ts, text)
            idl.append(eid)
            if check_dups is not None:
                if eid in check_dups:
                    continue
                check_dups.add(eid)
            we.writerow([eid, version, ts, rec, src, kind, text, tags, 0])
            new += 1
        for r in revs:
            key = r["key"]
            sup = self.heads.get(key)
            rid = hashlib.sha256(f'{key}|{r["stance"]}|{r["conf"]!r}|{r["rationale"]}|{sup}|{r["valid_from"]}'.encode()).hexdigest()
            self.seq += 1
            self.writers["revisions"].writerow([self.seq, version, rid, "belief", key, r["stance"], r["conf"], r["rationale"],
                                                sup or "", r["valid_from"], "rule:bench", rec])
            self.writers["revision_causes"].writerow([rid, idl[r["cause"] - (r["cause"] // BATCH) * BATCH]])
            self.heads[key] = rid
        if new or revs:
            for f in self.files.values():
                f.flush()
                if self.fsync:
                    os.fsync(f.fileno())
        return new

    def close(self):
        for f in self.files.values():
            f.close()


# --------------------------------------------------------------------------- queries

def sq_lookup(con, ids):
    s = 0
    for eid in ids:
        row = con.execute("SELECT event_id,occurred_at,source,kind,text,tags FROM events WHERE event_id=?", (eid,)).fetchone()
        s += len(row[4])
    return s


def csv_lookup(d, ids):
    s = 0
    p = os.path.join(d, "events.csv")
    for eid in ids:
        with open(p, "r", newline="", encoding="utf-8") as f:
            r = csv.reader(f)
            next(r)
            for row in r:
                if row[0] == eid:
                    s += len(row[6])
                    break
    return s


def sq_range(con, t1, t2):
    return sorted(con.execute("SELECT kind, COUNT(*) FROM events WHERE occurred_at >= ? AND occurred_at < ? GROUP BY kind", (t1, t2)).fetchall())


def csv_range(d, t1, t2):
    cnt = {}
    with open(os.path.join(d, "events.csv"), "r", newline="", encoding="utf-8") as f:
        r = csv.reader(f)
        next(r)
        for row in r:
            if t1 <= row[2] < t2:
                cnt[row[5]] = cnt.get(row[5], 0) + 1
    return sorted(cnt.items())


def sq_full(con):
    return sorted(con.execute("SELECT source, COUNT(*) FROM events GROUP BY source").fetchall())


def csv_full(d):
    cnt = {}
    with open(os.path.join(d, "events.csv"), "r", newline="", encoding="utf-8") as f:
        r = csv.reader(f)
        next(r)
        for row in r:
            cnt[row[4]] = cnt.get(row[4], 0) + 1
    return sorted(cnt.items())


def sq_asof(con, v):
    ev = sorted(con.execute("SELECT kind, COUNT(*) FROM events WHERE version<=? GROUP BY kind", (v,)).fetchall())
    be = con.execute("SELECT r.key, r.stance FROM revisions r JOIN (SELECT key, MAX(seq) m FROM revisions WHERE version<=? GROUP BY key) x"
                     " ON r.seq=x.m ORDER BY r.key", (v,)).fetchall()
    return ev, [tuple(x) for x in be]


def csv_asof(d, v):
    cnt = {}
    with open(os.path.join(d, "events.csv"), "r", newline="", encoding="utf-8") as f:
        r = csv.reader(f)
        next(r)
        for row in r:
            if int(row[1]) <= v:
                cnt[row[5]] = cnt.get(row[5], 0) + 1
    best = {}
    with open(os.path.join(d, "revisions.csv"), "r", newline="", encoding="utf-8") as f:
        r = csv.reader(f)
        next(r)
        for row in r:
            if int(row[1]) <= v:
                best[row[4]] = row[5]
    return sorted(cnt.items()), sorted(best.items())


def chk(x):
    return hashlib.sha256(json.dumps(x, sort_keys=True).encode()).hexdigest()[:16]


def timed(fn, reps, warm=1):
    for _ in range(warm):
        fn()
    xs = []
    last = None
    for _ in range(reps):
        t = time.perf_counter()
        last = fn()
        xs.append(time.perf_counter() - t)
    return xs, last


# --------------------------------------------------------------------------- tamper

def sq_content_hash(path):
    con = sqlite3.connect(path)
    h = hashlib.sha256()
    for t in ("events", "revisions", "revision_causes"):
        for row in con.execute(f"SELECT * FROM {t} ORDER BY rowid"):
            h.update(repr(row).encode())
    con.close()
    return h.hexdigest()


def csv_content_hash(d):
    h = hashlib.sha256()
    for name, ncol in (("events", 9), ("revisions", 12), ("revision_causes", 2)):
        with open(os.path.join(d, name + ".csv"), "r", newline="", encoding="utf-8") as f:
            r = csv.reader(f, strict=True)
            for row in r:
                if len(row) != ncol:
                    raise ValueError(f"{name}: row with {len(row)} fields")
                h.update(repr(row).encode())
    return h.hexdigest()


def flip(path, off, bit):
    with open(path, "r+b") as f:
        f.seek(off)
        b = f.read(1)
        f.seek(off)
        f.write(bytes([b[0] ^ (1 << bit)]))


def tamper_trials(kind, target, trials, seed, base_hash, work):
    """Flip one random bit per trial in `target` (file), classify, restore."""
    rng = random.Random(seed)
    size = os.path.getsize(target)
    res = {"trials": trials, "detected_by_check": 0, "detected_only_by_read_error": 0, "silent_data_change": 0, "harmless_no_change": 0}
    for _ in range(trials):
        off, bit = rng.randrange(size), rng.randrange(8)
        flip(target, off, bit)
        detected = False
        read_err = False
        try:
            if kind == "sqlite":
                try:
                    con = sqlite3.connect(target)
                    r = con.execute("PRAGMA integrity_check").fetchall()
                    con.close()
                    detected = not (len(r) == 1 and r[0][0] == "ok")
                except sqlite3.DatabaseError:
                    detected = True
                h = None
                if not detected:
                    try:
                        h = sq_content_hash(target)
                    except sqlite3.DatabaseError:
                        read_err = True
            else:  # csv: only structural parse errors can show up
                try:
                    h = csv_content_hash(os.path.dirname(target))
                except (csv.Error, ValueError, UnicodeDecodeError):
                    detected = True
                    h = None
        finally:
            flip(target, off, bit)  # restore
        if detected:
            res["detected_by_check"] += 1
        elif read_err:
            res["detected_only_by_read_error"] += 1
        elif h != base_hash:
            res["silent_data_change"] += 1
        else:
            res["harmless_no_change"] += 1
    return res


# --------------------------------------------------------------------------- main

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True)
    ap.add_argument("--n", type=int, required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--build-reps", type=int, default=5)
    ap.add_argument("--reps", type=int, default=5)
    ap.add_argument("--lookups", type=int, default=100)
    ap.add_argument("--csv-lookups", type=int, default=0, help="0 = automatic by N")
    ap.add_argument("--tamper-trials", type=int, default=40)
    ap.add_argument("--seed", type=int, default=7)
    a = ap.parse_args()
    N = a.n
    shutil.rmtree(a.work, ignore_errors=True)
    os.makedirs(a.work)
    events, revs = load(a.data)
    assert len(events) == N, (len(events), N)
    batches = make_batches(events, revs)
    nb = len(batches)
    ids = [event_id(e[0], e[1], e[3]) for e in events]
    assert len(set(ids)) == N
    L = a.lookups
    lk_idx = [(k * 2654435761 + 12345) % N for k in range(L)]
    csv_L = a.csv_lookups or (100 if N <= 10000 else 20 if N <= 100000 else 5)
    t1, t2 = events[N * 45 // 100][1], events[N * 55 // 100][1]
    asof_versions = sorted({1, max(1, nb // 2), nb})
    R = {"meta": {
        "n": N, "revisions": len(revs), "batches": nb, "batch_size": BATCH, "data": os.path.basename(a.data),
        "python": sys.version, "sqlite": sqlite3.sqlite_version, "platform": platform.platform(),
        "processor": platform.processor(), "cpu_count": os.cpu_count(), "reps": a.reps, "build_reps": a.build_reps,
        "lookups": L, "csv_lookups": csv_L, "range": [t1, t2], "asof_versions": asof_versions,
        "started": now_iso()}, "systems": {}}
    S = R["systems"]

    configs = [("sqlite", dict(indexes=False)), ("sqlite_idx", dict(indexes=True)), ("csv", {})]
    stores = {}
    for name, kw in configs:
        r = S.setdefault(name, {})
        times = []
        for rep in range(a.build_reps):
            d = os.path.join(a.work, f"{name}_{rep}")
            shutil.rmtree(d, ignore_errors=True)
            os.makedirs(d)
            if name == "csv":
                st = CsvStore(d)
            else:
                st = SqliteStore(os.path.join(d, "log.db"), **kw)
            t = time.perf_counter()
            for b, (be, br) in enumerate(batches):
                st.ingest(be, br, b + 1)
            times.append(time.perf_counter() - t)
            if rep < a.build_reps - 1:
                st.close()
                shutil.rmtree(d, ignore_errors=True)
            else:
                stores[name] = (st, d)
        r["build_b1000_s"] = stats(times)
        r["build_b1000_events_per_s"] = stats([N / x for x in times])
        stores[name][0].close()  # flush; reopen below
        r["size_bytes"] = dir_size(stores[name][1])
        print(f"[{name}] build done median {statistics.median(times):.2f}s size {r['size_bytes']}", file=sys.stderr, flush=True)

    # ---- point lookup
    lk_ids = [ids[i] for i in lk_idx]
    for name in ("sqlite", "sqlite_idx"):
        d = stores[name][1]
        t = time.perf_counter()
        con = sqlite3.connect(os.path.join(d, "log.db"))
        con.execute("SELECT COUNT(*) FROM events WHERE event_id=?", (lk_ids[0],)).fetchone()
        S[name]["cold_open_first_query_s"] = time.perf_counter() - t
        xs, last = timed(lambda: sq_lookup(con, lk_ids), a.reps)
        S[name]["lookup_per_op_s"] = stats([x / L for x in xs])
        S[name]["lookup_check"] = last
        con.close()
    xs, last = timed(lambda: csv_lookup(stores["csv"][1], lk_ids[:csv_L]), a.reps)
    S["csv"]["lookup_per_op_s"] = stats([x / csv_L for x in xs])
    S["csv"]["lookup_n_ops"] = csv_L
    S["csv"]["lookup_check_first"] = last
    S["csv"]["lookup_check"] = last if csv_L == L else None
    # expected check for the first csv_L ids from the in-memory data, to validate csv_lookup
    S["csv"]["lookup_expected_first"] = sum(len(events[i][3]) for i in lk_idx[:csv_L])
    for name in ("sqlite", "sqlite_idx"):
        S[name]["lookup_check_first"] = sum(len(events[i][3]) for i in lk_idx[:csv_L])
    S["expected_lookup_check"] = sum(len(events[i][3]) for i in lk_idx)

    # ---- range / aggregate
    for name in ("sqlite", "sqlite_idx"):
        con = sqlite3.connect(os.path.join(stores[name][1], "log.db"))
        xs, last = timed(lambda: sq_range(con, t1, t2), a.reps)
        S[name]["range_s"] = stats(xs)
        S[name]["range_check"] = chk(last)
        xs, last = timed(lambda: sq_full(con), a.reps)
        S[name]["fullagg_s"] = stats(xs)
        S[name]["fullagg_check"] = chk(last)
        con.close()
    xs, last = timed(lambda: csv_range(stores["csv"][1], t1, t2), a.reps)
    S["csv"]["range_s"] = stats(xs)
    S["csv"]["range_check"] = chk(last)
    xs, last = timed(lambda: csv_full(stores["csv"][1]), a.reps)
    S["csv"]["fullagg_s"] = stats(xs)
    S["csv"]["fullagg_check"] = chk(last)

    # ---- as of version
    for name in ("sqlite", "sqlite_idx", "csv"):
        S[name]["asof"] = {}
        for v in asof_versions:
            if name == "csv":
                xs, last = timed(lambda: csv_asof(stores[name][1], v), a.reps)
            else:
                con = sqlite3.connect(os.path.join(stores[name][1], "log.db"))
                xs, last = timed(lambda: sq_asof(con, v), a.reps)
                con.close()
            S[name]["asof"][str(v)] = {"s": stats(xs), "check": chk([list(map(list, last[0])), list(map(list, last[1]))])}

    # ---- tamper (independent copies; restore after each trial)
    if a.tamper_trials > 0 and N <= 100000:
        for name in ("sqlite", "csv"):
            src = stores[name][1]
            dst = os.path.join(a.work, f"tamper_{name}")
            shutil.copytree(src, dst)
            if name == "sqlite":
                target = os.path.join(dst, "log.db")
                base = sq_content_hash(target)
                S[name]["tamper"] = tamper_trials("sqlite", target, a.tamper_trials, a.seed, base, a.work)
            else:
                target = os.path.join(dst, "events.csv")
                base = csv_content_hash(dst)
                S[name]["tamper"] = tamper_trials("csv", target, a.tamper_trials, a.seed, base, a.work)
            shutil.rmtree(dst, ignore_errors=True)

    # ---- idempotent re-ingest (events only; same data again, batches of 1000)
    ev_batches = [(be, [], b + 1) for b, (be, _) in enumerate(batches)]
    st = SqliteStore(os.path.join(stores["sqlite"][1], "log.db"))
    before = st.con.execute("SELECT COUNT(*) FROM events").fetchone()[0]
    sz0 = dir_size(stores["sqlite"][1])
    t = time.perf_counter()
    added = sum(st.ingest(be, [], 10_000 + b) for be, _, b in ev_batches)
    dt = time.perf_counter() - t
    after = st.con.execute("SELECT COUNT(*) FROM events").fetchone()[0]
    st.close()
    S["sqlite"]["reingest"] = {"native": True, "seconds": dt, "rows_before": before, "rows_after": after, "new_events": added,
                               "size_delta_bytes": dir_size(stores["sqlite"][1]) - sz0,
                               "note": "INSERT OR IGNORE on PRIMARY KEY event_id; id computed by the app"}
    cs = CsvStore(stores["csv"][1], fsync=True)
    sz0 = dir_size(stores["csv"][1])
    n_before = sum(1 for _ in open(os.path.join(stores["csv"][1], "events.csv"), "rb")) - 1
    t = time.perf_counter()
    for be, _, b in ev_batches:
        cs.ingest(be, [], 10_000 + b)
    dt_naive = time.perf_counter() - t
    n_after = sum(1 for _ in open(os.path.join(stores["csv"][1], "events.csv"), "rb")) - 1
    S["csv"]["reingest"] = {"native": False, "naive_seconds": dt_naive, "rows_before": n_before, "rows_after_naive": n_after,
                            "size_delta_bytes_naive": dir_size(stores["csv"][1]) - sz0}
    cs.close()
    # emulated: scan existing ids first (app-level dedup), then append only unseen. Run on a fresh copy.
    dst = os.path.join(a.work, "csv_emul")
    shutil.copytree(stores["csv"][1], dst)
    # remove the naive duplicates by truncating to the pre-re-ingest length is not possible after the fact,
    # so build the dedup-emulation on a fresh store loaded with the original data
    shutil.rmtree(dst, ignore_errors=True)
    os.makedirs(dst)
    cs = CsvStore(dst, fsync=True)
    for b, (be, br) in enumerate(batches):
        cs.ingest(be, br, b + 1)
    cs.close()
    sz1 = dir_size(dst)
    t = time.perf_counter()
    known = set()
    with open(os.path.join(dst, "events.csv"), "r", newline="", encoding="utf-8") as f:
        r = csv.reader(f)
        next(r)
        for row in r:
            known.add(row[0])
    cs = CsvStore(dst, fsync=True)
    added = 0
    for be, _, b in ev_batches:
        added += cs.ingest(be, [], 10_000 + b, check_dups=known)
    dt_emul = time.perf_counter() - t
    cs.close()
    S["csv"]["reingest"].update({"emulated_dedup_seconds": dt_emul, "emulated_new_events": added,
                                 "emulated_size_delta_bytes": dir_size(dst) - sz1})
    shutil.rmtree(dst, ignore_errors=True)

    # ---- append, batch of 1 and tail batch of 1000, onto the N-event store
    APP_REPS, APP_N = 5, 100
    kcount = [0]

    def app_events(n):
        out = [append_event(kcount[0] + i) for i in range(n)]
        kcount[0] += n
        return out

    def run_append(label, st, version0):
        v = [version0]
        xs1 = []
        for _ in range(APP_REPS):
            evs = app_events(APP_N)
            t = time.perf_counter()
            for e in evs:
                v[0] += 1
                st.ingest([e], [], v[0])
            xs1.append(time.perf_counter() - t)
        xs2 = []
        for _ in range(APP_REPS):
            evs = app_events(BATCH)
            t = time.perf_counter()
            v[0] += 1
            st.ingest(evs, [], v[0])
            xs2.append(time.perf_counter() - t)
        return {"b1_events_per_s": stats([APP_N / x for x in xs1]), "b1_ms_per_commit": stats([x / APP_N * 1000 for x in xs1]),
                "b1000_events_per_s": stats([BATCH / x for x in xs2]), "b1000_ms_per_batch": stats([x * 1000 for x in xs2]),
                "b1_commits_per_rep": APP_N}

    ver0 = nb + 100_000
    # sqlite primary (DELETE/FULL) on the store itself
    st = SqliteStore(os.path.join(stores["sqlite"][1], "log.db"))
    S["sqlite"]["append"] = run_append("sqlite", st, ver0)
    S["sqlite"]["append"]["config"] = "journal_mode=DELETE synchronous=FULL (sqlite default); 1 txn per commit"
    st.close()
    st = SqliteStore(os.path.join(stores["sqlite_idx"][1], "log.db"), indexes=True)
    S["sqlite_idx"]["append"] = run_append("sqlite_idx", st, ver0)
    S["sqlite_idx"]["append"]["config"] = "as sqlite + 3 secondary indexes"
    st.close()
    # variants on copies of the plain sqlite store
    for vname, journal, sync in (("sqlite_wal_full", "WAL", "FULL"), ("sqlite_off", "DELETE", "OFF")):
        dst = os.path.join(a.work, vname)
        shutil.copytree(stores["sqlite"][1], dst)
        st = SqliteStore(os.path.join(dst, "log.db"), journal=journal, sync=sync)
        S[vname] = {"append": run_append(vname, st, ver0)}
        S[vname]["append"]["config"] = f"journal_mode={journal} synchronous={sync}"
        st.close()
        shutil.rmtree(dst, ignore_errors=True)
    cs = CsvStore(stores["csv"][1], fsync=True)
    S["csv"]["append"] = run_append("csv", cs, ver0)
    S["csv"]["append"]["config"] = "append + flush + os.fsync of each written file per commit"
    cs.close()
    cs = CsvStore(stores["csv"][1], fsync=False)
    S["csv_nofsync"] = {"append": run_append("csv_nofsync", cs, ver0 + 10_000)}
    S["csv_nofsync"]["append"]["config"] = "append + flush, no fsync"
    cs.close()

    R["meta"]["finished"] = now_iso()
    with open(a.out, "w") as f:
        json.dump(R, f, indent=1)
    shutil.rmtree(a.work, ignore_errors=True)


if __name__ == "__main__":
    main()
