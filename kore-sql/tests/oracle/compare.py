"""Replay the queries dumped by fuzz_oracle.rs (FUZZ_DIR/<scale>_<seed>/) in SQLite and compare with kore's answers.
usage: python compare.py FUZZ_DIR [-v]"""
import sqlite3, sys, os, csv, math
NULLTOK = chr(92) + "N"
EMPTYTOK = chr(92) + "E"

def load(db, d, name):
    with open(os.path.join(d, name + ".csv"), newline="", encoding="utf-8") as f:
        rd = csv.reader(f)
        raw = next(rd)
        # header cells are `name` (fuzz_oracle: f = REAL, s = TEXT, else INTEGER) or `name:i|f|s` (typed)
        hdr = [h.split(":")[0] for h in raw]
        kinds = {}
        for h in raw:
            if ":" in h: n, k = h.split(":"); kinds[n] = {"i": "INTEGER", "f": "REAL", "s": "TEXT"}[k]
        default = {"id": "INTEGER", "f": "REAL", "fg": "REAL", "s": "TEXT", "dt": "TEXT", "s2": "TEXT"}
        types = {c: kinds.get(c) or default.get(c, "INTEGER") for c in hdr}
        db.execute("create table %s (%s)" % (name, ", ".join(c + " " + types[c] for c in hdr)))
        def conv(c, v):
            if v == NULLTOK: return None
            if v == EMPTYTOK: return ""
            if types[c] == "REAL": return float(v)
            if types[c] == "TEXT": return v
            return int(v)
        db.executemany("insert into %s values (%s)" % (name, ",".join("?" * len(hdr))),
                       ([conv(c, v) for c, v in zip(hdr, row)] for row in rd))

def cellstr(v):
    if v is None: return "NULL"
    if isinstance(v, float):
        if v == int(v) and abs(v) < 1e15: return str(int(v))
        return repr(v)
    return str(v)

def eq_cell(a, b):
    if a == b: return True
    try:
        # two integers are equal or they are not (a relative tolerance would hide BIGINT key bugs)
        return int(a) == int(b)
    except ValueError:
        pass
    try:
        x, y = float(a), float(b)
        return math.isclose(x, y, rel_tol=1e-6, abs_tol=1e-6)
    except ValueError:
        return False

def eq_rows(a, b):
    if len(a) != len(b): return False
    for ra, rb in zip(a, b):
        ca, cb = ra.split("\t"), rb.split("\t")
        if len(ca) != len(cb) or not all(eq_cell(x, y) for x, y in zip(ca, cb)): return False
    return True

def sortkey(r):
    out = []
    for c in r.split("\t"):
        try: out.append((0, int(c), ""))
        except ValueError:
            try: out.append((0, round(float(c), 4), ""))
            except ValueError: out.append((1, 0, c))
    return out

def main():
    root = sys.argv[1]; verbose = "-v" in sys.argv
    bad = total = skipped = 0
    for sub in sorted(os.listdir(root)):
        d = os.path.join(root, sub)
        if not os.path.exists(os.path.join(d, "kore.txt")): continue
        db = sqlite3.connect(":memory:"); db.execute("pragma case_sensitive_like=on")
        for fn in sorted(os.listdir(d)):
            if fn.endswith(".csv"): load(db, d, fn[:-4])
        qs = [l.rstrip("\n").split("\t", 1) for l in open(os.path.join(d, "queries.txt"), encoding="utf-8")]
        ans = {}; cur = None
        for l in open(os.path.join(d, "kore.txt"), encoding="utf-8"):
            l = l.rstrip("\n")
            if l.startswith("Q ") and len(l.split(" ")) == 3 and l.split(" ")[1].isdigit() and (cur is None or len(ans[cur][1]) >= ans[cur][0]):
                _, i, n = l.split(" "); cur = int(i); ans[cur] = (int(n), [])
            elif l.startswith("E ") and (cur is None or len(ans[cur][1]) >= ans[cur][0]):
                p = l.split(" ", 2); cur = int(p[1]); ans[cur] = (-1, l)
            else:
                ans[cur][1].append(l)
        for i, (o, sql) in enumerate(qs):
            total += 1
            # known SQLite / Spark differences: integer division (Spark's / is a DOUBLE division) and concat() with a NULL
            # argument (NULL in Spark, treated as '' by SQLite)
            if "--spark" in sys.argv and (" / " in sql or "concat(" in sql): skipped += 1; continue
            a = ans.get(i)
            try:
                rows = db.execute(sql).fetchall()
            except Exception as e:
                skipped += 1
                if verbose: print("SQLITE-ERR", sub, i, sql, e)
                continue
            if a is None: continue
            if a[0] == -1:
                bad += 1; print("KORE-ERR", sub, i, sql, "\n    ", a[1]); continue
            exp = ["\t".join(cellstr(v) for v in r) for r in rows]
            got = a[1]
            if o == "U": exp.sort(key=sortkey); got = sorted(got, key=sortkey)
            if len(exp) > 200_000: skipped += 1; continue
            if not eq_rows(exp, got):
                bad += 1
                sys.stdout.reconfigure(encoding="utf-8", errors="replace"); print("MISMATCH", sub, i, sql)
                print("   sqlite %d rows: %s" % (len(exp), " | ".join(exp[:5])))
                print("   kore   %d rows: %s" % (len(got), " | ".join(got[:5])))
    print("compared %d queries, %d mismatches, %d skipped" % (total, bad, skipped))
    sys.exit(1 if bad else 0)
main()
