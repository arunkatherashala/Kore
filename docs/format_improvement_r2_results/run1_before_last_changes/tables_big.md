
#### N=1,000,000 (rdata)

| metric | BEFORE (round-1 binary, format 2) | AFTER, format 2 | AFTER, format 3 (journal) |
|---|---|---|---|
| total size on disk | 88.72 MB [88.72 MB-88.72 MB] (runs=3) | 89.16 MB [89.16 MB-89.16 MB] (runs=3) | 89.16 MB [89.16 MB-89.16 MB] (runs=3) |
|   events | 84.73 MB [84.73 MB-84.73 MB] (runs=3) | 85.17 MB [85.17 MB-85.17 MB] (runs=3) | 85.17 MB [85.17 MB-85.17 MB] (runs=3) |
|   revisions | 1.98 MB [1.98 MB-1.98 MB] (runs=3) | 1.98 MB [1.98 MB-1.98 MB] (runs=3) | 1.98 MB [1.98 MB-1.98 MB] (runs=3) |
|   revision_causes | 1.34 MB [1.34 MB-1.34 MB] (runs=3) | 1.34 MB [1.34 MB-1.34 MB] (runs=3) | 1.34 MB [1.34 MB-1.34 MB] (runs=3) |
|   MANIFEST | 0.67 MB [0.67 MB-0.67 MB] (runs=3) | 0.67 MB [0.67 MB-0.67 MB] (runs=3) | 0.67 MB [0.67 MB-0.67 MB] (runs=3) |
| bulk build, events/s (batches of 1000) | 22,785/s [22,038/s-24,043/s] (runs=3) | 29,056/s [27,558/s-31,251/s] (runs=3) | 32,966/s [32,720/s-34,252/s] (runs=3) |
| open() | 568.79 ms [469.54 ms-676.33 ms] (runs=3) | 525.83 ms [430.42 ms-586.79 ms] (runs=3) | 607.98 ms [432.61 ms-703.20 ms] (runs=3) |
| open() + decode latest snapshot | 3.40 s [2.84 s-3.85 s] (runs=3) | 3.10 s [2.63 s-3.51 s] (runs=3) | 3.07 s [2.45 s-4.10 s] (runs=3) |
| verify() | 1.56 s [1.34 s-1.98 s] (runs=3) | 1.69 s [931.81 ms-1.83 s] (runs=3) | 1.70 s [1.35 s-2.00 s] (runs=3) |
| lookup: has_event (existence) | 94 ns [92 ns-100 ns] (runs=3) | 95 ns [90 ns-111 ns] (runs=3) | 112 ns [98 ns-314 ns] (runs=3) |
| lookup: scan of in-memory snapshot | 12.99 ms [10.40 ms-14.32 ms] (runs=3) | 11.13 ms [9.07 ms-13.19 ms] (runs=3) | 11.51 ms [9.68 ms-12.70 ms] (runs=3) |
| lookup: get_event(id) (new API) | 4.15 ms [2.86 ms-4.61 ms] (runs=3) | 2.14 ms [1.16 ms-2.65 ms] (runs=3) | 1.64 ms [1.29 ms-2.11 ms] (runs=3) |
| lookup: SQL, registered ctx | 2.59 s [2.04 s-3.42 s] (runs=3) | 2.48 s [2.15 s-3.30 s] (runs=3) | 2.27 s [1.88 s-2.98 s] (runs=3) |
| append 1 event/commit, per commit | 16.62 ms [14.58 ms-19.70 ms] (runs=3) | 19.63 ms [16.83 ms-32.36 ms] (runs=3) | 6.79 ms [6.01 ms-11.07 ms] (runs=3) |
| append 1000-event batch | 33.70 ms [29.25 ms-46.38 ms] (runs=3) | 31.30 ms [27.39 ms-143.00 ms] (runs=3) | 17.94 ms [16.91 ms-26.89 ms] (runs=3) |
| range query, SQL registered ctx | 1.05 s [973.50 ms-1.29 s] (runs=3) | 1.21 s [925.60 ms-1.33 s] (runs=3) | 1.29 s [965.68 ms-2.05 s] (runs=3) |
| as-of mid version, open_at | 5.18 s [4.72 s-6.84 s] (runs=3) | 5.25 s [3.92 s-5.86 s] (runs=3) | 4.98 s [4.86 s-5.63 s] (runs=3) |

| fact | BEFORE (round-1 binary, format 2) | AFTER, format 2 | AFTER, format 3 (journal) |
|---|---|---|---|
| file count | 3002 | 3002 | 3002 |
| re-ingest same events: new versions | 0 | 0 | 0 |
| re-ingest same events: seconds (median of runs shown individually) | 11.07 / 13.33 / 9.31 | 11.18 / 20.94 | 5.51 / 5.84 / 7.75 |
| revision replay: revisions before->after | 20000->20000 | 20000->20000 | 20000->20000 |
| revision replay: new versions | 0 | 0 | 0 |
| tamper trials / detected by verify / detected only by open / silent / harmless | None | None | None |
| log_format recorded | 2 | 2 | 3 |

Query-hash equality across all runs and formats: asof v1=MATCH, asof v1000=MATCH, asof v500=MATCH, fullagg=MATCH, lookup=MATCH, range=MATCH
