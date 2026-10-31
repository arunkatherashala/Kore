
#### N=1,000,000 (rdata)

| metric | before (old code) | after, format 1 | after, format 2 |
|---|---|---|---|
| total size on disk | 229.82 MB [229.82 MB-229.82 MB] (runs=1) | 229.82 MB [229.82 MB-229.82 MB] (runs=1) | 88.72 MB [88.72 MB-88.72 MB] (runs=1) |
|   events | 222.07 MB [222.07 MB-222.07 MB] (runs=1) | 222.07 MB [222.07 MB-222.07 MB] (runs=1) | 84.73 MB [84.73 MB-84.73 MB] (runs=1) |
|   revisions | 4.30 MB [4.30 MB-4.30 MB] (runs=1) | 4.30 MB [4.30 MB-4.30 MB] (runs=1) | 1.98 MB [1.98 MB-1.98 MB] (runs=1) |
|   revision_causes | 2.81 MB [2.81 MB-2.81 MB] (runs=1) | 2.81 MB [2.81 MB-2.81 MB] (runs=1) | 1.34 MB [1.34 MB-1.34 MB] (runs=1) |
|   MANIFEST | 0.65 MB [0.65 MB-0.65 MB] (runs=1) | 0.65 MB [0.65 MB-0.65 MB] (runs=1) | 0.67 MB [0.67 MB-0.67 MB] (runs=1) |
| bulk build, events/s (batches of 1000) | 11,877/s [11,877/s-11,877/s] (runs=1) | 45,175/s [45,175/s-45,175/s] (runs=1) | 29,340/s [29,340/s-29,340/s] (runs=1) |
| open() | 3.83 s [3.74 s-4.30 s] (runs=1) | 3.00 s [1.95 s-3.18 s] (runs=1) | 370.24 ms [369.64 ms-438.59 ms] (runs=1) |
| open() + decode latest snapshot | n/a | 1.82 s [1.64 s-1.92 s] (runs=1) | 2.44 s [2.13 s-2.46 s] (runs=1) |
| verify() | 2.29 s [2.28 s-2.29 s] (runs=1) | 1.22 s [1.13 s-1.22 s] (runs=1) | 1.71 s [1.58 s-1.76 s] (runs=1) |
| lookup: has_event (existence) | 83 ns [82 ns-96 ns] (runs=1) | 84 ns [83 ns-85 ns] (runs=1) | 98 ns [86 ns-99 ns] (runs=1) |
| lookup: scan of in-memory snapshot | 8.27 ms [8.06 ms-8.37 ms] (runs=1) | 8.26 ms [8.13 ms-8.77 ms] (runs=1) | 9.07 ms [8.90 ms-9.73 ms] (runs=1) |
| lookup: get_event(id) (new API) | n/a | 466 ns [445 ns-550 ns] (runs=1) | 2.00 ms [1.98 ms-2.05 ms] (runs=1) |
| lookup: SQL, registered ctx | 1.08 s [1.05 s-1.74 s] (runs=1) | 1.37 s [1.33 s-1.52 s] (runs=1) | 1.47 s [1.44 s-1.90 s] (runs=1) |
| append 1 event/commit, per commit | 120.63 ms [116.88 ms-135.50 ms] (runs=1) | 12.70 ms [11.81 ms-21.74 ms] (runs=1) | 16.55 ms [16.17 ms-21.65 ms] (runs=1) |
| append 1000-event batch | 119.84 ms [115.64 ms-129.60 ms] (runs=1) | 18.46 ms [18.09 ms-21.09 ms] (runs=1) | 42.27 ms [33.51 ms-129.41 ms] (runs=1) |
| range query, SQL registered ctx | 784.08 ms [754.09 ms-789.96 ms] (runs=1) | 816.72 ms [785.21 ms-825.65 ms] (runs=1) | 834.63 ms [747.06 ms-865.46 ms] (runs=1) |
| as-of mid version, open_at | 2.21 s [2.20 s-2.25 s] (runs=1) | 2.23 s [2.15 s-2.33 s] (runs=1) | 4.02 s [3.28 s-4.11 s] (runs=1) |

| fact | before (old code) | after, format 1 | after, format 2 |
|---|---|---|---|
| file count | 3002 | 3002 | 3002 |
| re-ingest same events: new versions | 0 | 0 | 0 |
| re-ingest same events: seconds (median of runs shown individually) | 160.03 | 6.90 | 18.77 |
| revision replay: revisions before->after | 20000->40000 | 20000->40000 | 20000->20000 |
| revision replay: new versions | 1000 | 1000 | 0 |
| tamper trials / detected by verify / detected only by open / silent / harmless | None | None | None |
| log_format recorded | 1 | 1 | 2 |

Query-hash equality across all runs and formats: asof v1=MATCH, asof v1000=MATCH, asof v500=MATCH, fullagg=MATCH, lookup=MATCH, range=MATCH
