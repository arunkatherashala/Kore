
#### N=1,000,000 (rdata)

| metric | BEFORE (round-1 binary, format 2) | AFTER, format 2 | AFTER, format 3 (journal) |
|---|---|---|---|
| total size on disk | 88.72 MB [88.72 MB-88.72 MB] (runs=3) | 89.16 MB [89.16 MB-89.16 MB] (runs=3) | 89.16 MB [89.16 MB-89.16 MB] (runs=3) |
|   events | 84.73 MB [84.73 MB-84.73 MB] (runs=3) | 85.17 MB [85.17 MB-85.17 MB] (runs=3) | 85.17 MB [85.17 MB-85.17 MB] (runs=3) |
|   revisions | 1.98 MB [1.98 MB-1.98 MB] (runs=3) | 1.98 MB [1.98 MB-1.98 MB] (runs=3) | 1.98 MB [1.98 MB-1.98 MB] (runs=3) |
|   revision_causes | 1.34 MB [1.34 MB-1.34 MB] (runs=3) | 1.34 MB [1.34 MB-1.34 MB] (runs=3) | 1.34 MB [1.34 MB-1.34 MB] (runs=3) |
|   MANIFEST | 0.67 MB [0.67 MB-0.67 MB] (runs=3) | 0.67 MB [0.67 MB-0.67 MB] (runs=3) | 0.67 MB [0.67 MB-0.67 MB] (runs=3) |
| bulk build, events/s (batches of 1000) | 31,820/s [26,410/s-33,506/s] (runs=3) | 39,942/s [35,661/s-45,899/s] (runs=3) | 44,554/s [37,417/s-49,576/s] (runs=3) |
| open() | 359.12 ms [330.59 ms-474.65 ms] (runs=3) | 361.43 ms [309.64 ms-478.26 ms] (runs=3) | 469.34 ms [337.11 ms-577.60 ms] (runs=3) |
| open() + decode latest snapshot | 2.35 s [1.94 s-2.83 s] (runs=3) | 2.08 s [1.87 s-3.01 s] (runs=3) | 2.39 s [1.83 s-3.43 s] (runs=3) |
| verify() | 1.61 s [1.40 s-1.74 s] (runs=3) | 1.50 s [1.37 s-2.37 s] (runs=3) | 1.33 s [1.13 s-1.59 s] (runs=3) |
| lookup: has_event (existence) | 89 ns [86 ns-102 ns] (runs=3) | 86 ns [67 ns-105 ns] (runs=3) | 82 ns [67 ns-89 ns] (runs=3) |
| lookup: scan of in-memory snapshot | 9.51 ms [8.77 ms-15.08 ms] (runs=3) | 10.03 ms [8.94 ms-13.28 ms] (runs=3) | 9.09 ms [8.71 ms-15.53 ms] (runs=3) |
| lookup: get_event(id) (new API) | 2.55 ms [1.89 ms-3.75 ms] (runs=3) | 1.14 ms [954.1 us-1.47 ms] (runs=3) | 1.14 ms [1.00 ms-2.26 ms] (runs=3) |
| lookup: SQL, registered ctx | 1.76 s [1.54 s-3.24 s] (runs=3) | 1.82 s [1.31 s-2.16 s] (runs=3) | 1.57 s [1.40 s-2.74 s] (runs=3) |
| append 1 event/commit, per commit | 14.11 ms [13.18 ms-18.37 ms] (runs=3) | 15.47 ms [13.48 ms-18.10 ms] (runs=3) | 5.77 ms [4.94 ms-11.35 ms] (runs=3) |
| append 1000-event batch | 25.22 ms [24.33 ms-123.04 ms] (runs=3) | 26.03 ms [20.70 ms-36.84 ms] (runs=3) | 14.82 ms [11.67 ms-20.75 ms] (runs=3) |
| range query, SQL registered ctx | 969.71 ms [724.90 ms-1.30 s] (runs=3) | 825.96 ms [725.53 ms-1.40 s] (runs=3) | 892.56 ms [787.29 ms-1.13 s] (runs=3) |
| as-of mid version, open_at | 4.93 s [4.54 s-5.29 s] (runs=3) | 4.03 s [3.61 s-5.10 s] (runs=3) | 3.99 s [3.73 s-4.69 s] (runs=3) |

| fact | BEFORE (round-1 binary, format 2) | AFTER, format 2 | AFTER, format 3 (journal) |
|---|---|---|---|
| file count | 3002 | 3002 | 3002 |
| re-ingest same events: new versions | 0 | 0 | 0 |
| re-ingest same events: seconds (median of runs shown individually) | 7.09 / 8.52 / 8.70 | 3.45 / 3.59 / 3.72 | 3.34 / 4.33 / 4.53 |
| revision replay: revisions before->after | 20000->20000 | 20000->20000 | 20000->20000 |
| revision replay: new versions | 0 | 0 | 0 |
| tamper trials / detected by verify / detected only by open / silent / harmless | None | None | None |
| log_format recorded | 2 | 2 | 3 |

Query-hash equality across all runs and formats: asof v1=MATCH, asof v1000=MATCH, asof v500=MATCH, fullagg=MATCH, lookup=MATCH, range=MATCH
