"""Markdown tables from bench_v4_rows results (docs/kore_speed_v4.md section 6).

Usage:  python -I rows_tables.py res_10000.json res_100000.json res_1000000.json
Per metric and size: before and after = median [min-max] over repetitions of the per-repetition median;
ratio = after / before of the same repetition, median [min-max].
"""
import json
import sys

LABELS = [
    ("get_event_decode", "get one event (8 columns, 1 row), bytes in memory"),
    ("get_event_disk", "get one event, file read + sha256 + decode (kore-self get_event shape)"),
    ("one_column_one_row", "one column (`text`) of one row"),
    ("tags_100_rows", "one column (`tags`) of 100 rows"),
]
for size in (1, 10, 100, 1000):
    for d in ("dict", "nodict"):
        dl = "with dictionary" if d == "dict" else "no dictionary"
        LABELS.append((f"write_{size}_{d}_default", f"write {size}-event block, {dl}: default (same bytes)"))
        LABELS.append((f"write_{size}_{d}_here", f"write {size}-event block, {dl}: forced calling thread (same bytes)"))
        LABELS.append((f"write_{size}_{d}_rayon", f"write {size}-event block, {dl}: forced rayon (same bytes)"))
        LABELS.append((f"write_{size}_{d}_fast", f"write {size}-event block, {dl}: `fast()` (other bytes)"))
        LABELS.append((f"write_{size}_{d}_quick", f"write {size}-event block, {dl}: `quick()` (other bytes)"))


def t(x):
    x *= 1e6
    return f"{x:.0f} us" if x < 1000 else f"{x / 1000:.2f} ms"


def mmm(v, f):
    s = sorted(v)
    return f"{f(s[len(s) // 2])} [{f(s[0])}-{f(s[-1])}]"


def main():
    res = [json.load(open(p)) for p in sys.argv[1:]]
    head = "| | " + " | ".join(f"N={r['n']:,} ({r['reps']} reps)" for r in res) + " |"
    print(head)
    print("|---|" + "---|" * len(res))
    for key, label in LABELS:
        if not all(key in r for r in res):
            continue
        cells_b, cells_a, cells_r = [], [], []
        for r in res:
            v = r[key]
            cells_b.append(mmm([p["before"] for p in v], t))
            cells_a.append(mmm([p["after"] for p in v], t))
            cells_r.append(mmm([p["after"] / p["before"] for p in v], lambda x: f"{x:.2f}"))
        print(f"| {label} | " + " | ".join(f"{b} -> {a}, **ratio {q}**" for b, a, q in zip(cells_b, cells_a, cells_r)) + " |")
    print()
    print("| bytes (mean per block) | " + " | ".join(f"N={r['n']:,}" for r in res) + " |")
    print("|---|" + "---|" * len(res))
    for size in (1, 10, 100, 1000):
        for d in ("dict", "nodict"):
            k = f"bytes_{size}_{d}"
            if not all(k in r for r in res):
                continue
            cells = []
            for r in res:
                b = r[k]
                cells.append(f"v4 {b['v4_mean']:.1f}, v5 {b['v5_mean']:.1f} ({b['v5_mean'] - b['v4_mean']:+.1f}), quick {b['quick_mean']:.1f} ({100 * (b['quick_mean'] / b['v4_mean'] - 1):+.1f} %), quick+v5 {b['quick_v5_mean']:.1f}, fast {b['fast_mean']:.1f} [{b['blocks']} blocks, all byte-identical]")
            print(f"| {size}-event, {'with dictionary' if d == 'dict' else 'no dictionary'} | " + " | ".join(cells) + " |")
    print()
    for r in res:
        b = r["dataset_bytes"]
        n = r["n"]
        print(f"N={n:,}: {b['segments']} segments of 1000 events with the dictionary ({b['dict_bytes']:,} B object): "
              f"v4 {b['v4']:,} B ({(b['v4'] + b['dict_bytes']) / n:.1f} B/event incl. dictionary), "
              f"v5 {b['v5']:,} B ({b['v5'] - b['v4']:+,}), quick {b['quick']:,} B ({100 * (b['quick'] / b['v4'] - 1):+.2f} %); "
              f"default writer = reference bytes for all {b['identical_segments']} segments.")


if __name__ == "__main__":
    main()
