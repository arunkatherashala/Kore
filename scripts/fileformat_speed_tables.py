"""Tables for scripts/fileformat_speed_paired.sh results.

Usage: python -I fileformat_speed_tables.py store|log OUTDIR

Each process run reports a median over its repetitions. A cell is the median [min-max] of those
per-process medians over the rounds; a ratio is the median [min-max] over rounds of the per-round
ratio (the three variants of a round ran back to back). Ratio > 1 = slower / larger than the reference.
"""
import glob, json, os, re, statistics as st, sys
from collections import defaultdict

mode, out = sys.argv[1], sys.argv[2]
runs = defaultdict(dict)  # (n) -> {(variant, round): json}
for f in glob.glob(os.path.join(out, 'r_*.json')):
    m = re.match(r'r_(old|a98|new)_(\d+)_r(\d+)\.json', os.path.basename(f))
    if not m:
        continue
    try:
        runs[int(m.group(2))][(m.group(1), int(m.group(3)))] = json.load(open(f))
    except Exception as e:
        print('unreadable', f, e)

def get(d, path):
    for p in path.split('.'):
        if d is None:
            return None
        d = d.get(p) if isinstance(d, dict) else None
    if isinstance(d, dict) and 'median' in d:
        return d['median']
    return d

if mode == 'store':
    metrics = [(f'{l}.{k}', f'{l}.{k}') for l in ('seg', 'one') for k in ('write_ms', 'open_ms', 'scan_ms', 'proj_ms', 'lookup_ms', 'asof_ms')]
    sizes = [('seg.bytes', 'seg.bytes'), ('one.bytes', 'one.bytes')]
else:
    metrics = [('build events/s', 'build_b1000_events_per_s'), ('open_s', 'open_s'), ('open+latest snapshot_s', 'open_plus_latest_snapshot_s'),
               ('verify_s', 'verify_s'), ('get_event per op_s', 'get_event_per_op_s')]
    sizes = [('size_bytes', 'size_bytes'), ('events dir bytes', 'size_by_table.events'), ('tamper detected', 'tamper.detected_by_check'), ('tamper silent changes', 'tamper.silent_data_change')]

def fmt(xs, p=3):
    xs = [x for x in xs if x is not None]
    if not xs:
        return '-'
    return f'{st.median(xs):.{p}g} [{min(xs):.{p}g}-{max(xs):.{p}g}]'

for n in sorted(runs):
    R = runs[n]
    rounds = sorted({r for (_, r) in R})
    asof_keys = []
    if mode == 'log':
        any_run = next(iter(R.values()))
        asof_keys = [(f'as-of v{k} open_at_s', f'asof.{k}.open_at_s') for k in sorted(any_run.get('asof', {}), key=lambda x: int(x) if x.isdigit() else 0)]
    print(f'\n### n = {n:,} ({len(rounds)} rounds)\n')
    print('| metric | old store | a980727d | this branch | new/old | new/a98 |')
    print('|---|---|---|---|---|---|')
    for label, key in metrics + asof_keys + sizes:
        cols = {v: [get(R.get((v, r)), key) for r in rounds] for v in ('old', 'a98', 'new')}
        def ratio(ref):
            xs = [a / b for a, b in zip(cols['new'], cols[ref]) if a is not None and b not in (None, 0)]
            return fmt(xs, 3)
        print(f'| {label} | {fmt(cols["old"])} | {fmt(cols["a98"])} | {fmt(cols["new"])} | {ratio("old")} | {ratio("a98")} |')
load = os.path.join(out, 'load.txt')
if os.path.exists(load):
    cpu = [int(x) for x in re.findall(r'cpu_(?:before|after)=(\d+)', open(load).read())]
    rc = re.findall(r'rc=(\d+)', open(load).read())
    if cpu:
        print(f'\nCPU total % around runs: median {st.median(cpu)}, min {min(cpu)}, max {max(cpu)} ({len(cpu)} samples); non-zero exit codes: {sum(1 for x in rc if x != "0")}')
