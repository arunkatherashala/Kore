"""Markdown tables for the storage-efficiency section.  python -I format_bench_size_tables.py RESDIR"""
import json
import os
import sys

d = sys.argv[1]
Ns = [n for n in (10000, 100000, 1000000) if os.path.exists(f"{d}/sz_kore_{n}.json") and os.path.exists(f"{d}/sz_py_{n}.json")]
K = {n: json.load(open(f"{d}/sz_kore_{n}.json")) for n in Ns}
P = {n: json.load(open(f"{d}/sz_py_{n}.json")) for n in Ns}
BUDGET = 100_000_000


def t(x):
    if x >= 1:
        return f"{x:.2f} s"
    if x >= 1e-3:
        return f"{x*1e3:.2f} ms"
    if x >= 1e-6:
        return f"{x*1e6:.1f} us"
    return f"{x*1e9:.0f} ns"


def ms(s):
    return f"{t(s['median'])} [{t(s['min'])}-{t(s['max'])}]"


sizes = {}
for n in Ns:
    p, k = P[n], K[n]
    sizes[n] = {"KORE log": k["size_bytes"], "SQLite": p["sqlite_bytes"], "SQLite after VACUUM": p["sqlite_vacuum_bytes"],
                "CSV (3 files)": p["csv_bytes"], "gzip -6 of CSV": p["gzip"]["6"]["bytes"], "gzip -9 of CSV": p["gzip"]["9"]["bytes"]}
fmts = list(sizes[Ns[0]])

print("#### Dataset (realistic mix, see format_bench_gen_realistic.py)\n")
print("| N | avg text bytes | raw text total | raw JSONL file |")
print("|---|---|---|---|")
for n in Ns:
    print(f"| {n:,} | {P[n]['avg_text_bytes']:.0f} | {P[n]['raw_text_bytes']/1e6:.2f} MB | {P[n]['raw_jsonl_bytes']/1e6:.2f} MB |")

print("\n#### Total size, bytes per event, compression ratio vs raw text (raw text bytes / stored bytes; below 1.0 = larger than the text alone)\n")
print("| format | " + " | ".join(f"N={n:,}" for n in Ns) + " |")
print("|---|" + "---|" * len(Ns))
for f in fmts:
    cells = []
    for n in Ns:
        b = sizes[n][f]
        cells.append(f"{b/1e6:.2f} MB, {b/n:.1f} B/event, ratio {P[n]['raw_text_bytes']/b:.2f}")
    print(f"| {f} | " + " | ".join(cells) + " |")

print("\n#### Bytes per event vs N (linearity) and largest N that fits in a 100 MB budget\n")
print("The N-in-100-MB figure is an EXTRAPOLATION: 100,000,000 / (bytes per event at the largest measured N). It assumes bytes/event stays flat beyond the measured range (it was not tested to 100 MB for most formats).\n")
print("| format | " + " | ".join(f"B/event @ {n:,}" for n in Ns) + " | change first->last | events in 100 MB (extrapolated) |")
print("|---|" + "---|" * (len(Ns) + 2))
for f in fmts:
    bpe = [sizes[n][f] / n for n in Ns]
    ch = (bpe[-1] / bpe[0] - 1) * 100
    print(f"| {f} | " + " | ".join(f"{x:.1f}" for x in bpe) + f" | {ch:+.1f}% | {BUDGET/bpe[-1]:,.0f} |")

print("\n#### Queryable without full decompression?\n")
print("Times: median [min-max] of 3-5 runs; store/file warm in OS cache. Lookup = one event id near the middle of the file (scan paths exit early); range = COUNT by kind over the middle 10% of the time range.\n")
print("| path | what it does | " + " | ".join(f"N={n:,} lookup" for n in Ns) + " | " + " | ".join(f"N={n:,} range" for n in Ns) + " |")
print("|---|---|" + "---|" * (2 * len(Ns)))


def row(name, what, lk, rg):
    print(f"| {name} | {what} | " + " | ".join(lk(n) for n in Ns) + " | " + " | ".join(rg(n) for n in Ns) + " |")


row("KORE has_event / SQL (open log)", "hash index existence (no row) / kore-sql scan on registered tables",
    lambda n: f"{ms(K[n]['lookup']['has_event_existence_only_per_op_s'])} / {ms(K[n]['lookup']['sql_registered_ctx_per_op_s'])}",
    lambda n: ms(K[n]["range"]["sql_registered_ctx_s"]))
row("KORE hand-written Rust scan (open log)", "in-memory rows (already decoded at open)",
    lambda n: ms(K[n]["lookup"]["native_scan_per_op_s"]), lambda n: ms(K[n]["range"]["native_rust_loop_s"]))
row("KORE open() cost (paid once)", "reads + decodes + sha256-checks every segment",
    lambda n: ms(K[n]["open_s"]), lambda n: "-")
row("SQLite", "PK lookup / scan with GROUP BY, pages read on demand",
    lambda n: ms(P[n]["sqlite_lookup_per_op_s"]), lambda n: ms(P[n]["sqlite_range_s"]))
row("plain CSV", "scan with csv module (lookup exits early)",
    lambda n: ms(P[n]["csv_lookup_one_s"]), lambda n: ms(P[n]["csv_range_s"]))
row("gzip -6 CSV", "must stream-decompress from the start",
    lambda n: ms(P[n]["gzip6_csv_lookup_one_s"]), lambda n: ms(P[n]["gzip6_csv_range_s"]))
row("gzip -9 CSV", "same",
    lambda n: ms(P[n]["gzip9_csv_lookup_one_s"]), lambda n: ms(P[n]["gzip9_csv_range_s"]))

print("\n#### Cost to produce / fully decompress\n")
print("| item | " + " | ".join(f"N={n:,}" for n in Ns) + " |")
print("|---|" + "---|" * len(Ns))
print("| KORE build (commit_batch x N/1000, fsync per commit, 1 run) | " + " | ".join(t(K[n]["build_b1000_s"]["median"]) for n in Ns) + " |")
print("| gzip -6 compress (3 CSV files) | " + " | ".join(t(P[n]["gzip"]["6"]["compress_s"]) for n in Ns) + " |")
print("| gzip -9 compress | " + " | ".join(t(P[n]["gzip"]["9"]["compress_s"]) for n in Ns) + " |")
print("| gzip -6 full decompress (read only, no parsing) | " + " | ".join(t(P[n]["gzip"]["6"]["full_decompress_s"]) for n in Ns) + " |")
print("| SQLite VACUUM | " + " | ".join(t(P[n]["sqlite_vacuum_s"]) for n in Ns) + " |")

print("\n#### What dominates the KORE size (events table, per column)\n")
print("KORE column figure = that column alone re-encoded with the existing KoreWriter in 1000-row chunks (same chunking as the log). gzip figures = gzip -6 of the column values (one value per line), whole column and in 1000-row chunks. Raw = UTF-8 bytes + 1.\n")
for n in Ns:
    kc = K[n]["events_column_bytes_single_column_reencode"]
    pc = P[n]["events_column_sizes"]
    print(f"\nN={n:,}: KORE size_by_table = {K[n]['size_by_table']} ; files = {K[n]['file_count']}\n")
    print("| column | raw | KORE encoded | gzip-6 whole column | gzip-6 1000-row chunks |")
    print("|---|---|---|---|---|")
    for c in ("event_id", "occurred_at", "source", "kind", "text", "tags"):
        print(f"| {c} | {pc[c]['raw']:,} | {kc[c]:,} | {pc[c]['gzip6_whole_column']:,} | {pc[c]['gzip6_1000row_chunks']:,} |")
    print(f"| recorded_at (KORE/SQLite/CSV store it; one value per batch) | - | {kc['recorded_at']:,} | - | - |")
    print(f"| redacted | - | {kc['redacted']:,} | - | - |")
