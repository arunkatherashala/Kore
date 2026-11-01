
#### N=10,000 (rdata)

| metric | BEFORE (round-1 binary, format 2) | AFTER, format 2 | AFTER, format 3 (journal) |
|---|---|---|---|
| total size on disk | 0.88 MB [0.88 MB-0.88 MB] (runs=5) | 0.88 MB [0.88 MB-0.88 MB] (runs=5) | 0.88 MB [0.88 MB-0.88 MB] (runs=5) |
|   events | 0.84 MB [0.84 MB-0.84 MB] (runs=5) | 0.84 MB [0.84 MB-0.84 MB] (runs=5) | 0.84 MB [0.84 MB-0.84 MB] (runs=5) |
|   revisions | 0.02 MB [0.02 MB-0.02 MB] (runs=5) | 0.02 MB [0.02 MB-0.02 MB] (runs=5) | 0.02 MB [0.02 MB-0.02 MB] (runs=5) |
|   revision_causes | 0.01 MB [0.01 MB-0.01 MB] (runs=5) | 0.01 MB [0.01 MB-0.01 MB] (runs=5) | 0.01 MB [0.01 MB-0.01 MB] (runs=5) |
|   MANIFEST | 0.01 MB [0.01 MB-0.01 MB] (runs=5) | 0.01 MB [0.01 MB-0.01 MB] (runs=5) | 0.01 MB [0.01 MB-0.01 MB] (runs=5) |
| bulk build, events/s (batches of 1000) | 20,313/s [17,684/s-33,724/s] (runs=5) | 27,994/s [9,984/s-42,791/s] (runs=5) | 30,500/s [25,963/s-33,333/s] (runs=5) |
| open() | 14.31 ms [5.18 ms-53.26 ms] (runs=5) | 13.38 ms [5.16 ms-25.41 ms] (runs=5) | 12.25 ms [4.02 ms-43.39 ms] (runs=5) |
| open() + decode latest snapshot | 46.33 ms [19.92 ms-217.71 ms] (runs=5) | 41.84 ms [20.41 ms-77.55 ms] (runs=5) | 44.95 ms [20.92 ms-98.37 ms] (runs=5) |
| verify() | 22.56 ms [12.12 ms-46.46 ms] (runs=5) | 19.71 ms [10.79 ms-68.87 ms] (runs=5) | 26.09 ms [13.23 ms-60.47 ms] (runs=5) |
| lookup: has_event (existence) | 98 ns [67 ns-111 ns] (runs=5) | 100 ns [67 ns-114 ns] (runs=5) | 99 ns [88 ns-237 ns] (runs=5) |
| lookup: scan of in-memory snapshot | 29.4 us [13.3 us-94.7 us] (runs=5) | 22.3 us [13.4 us-67.4 us] (runs=5) | 28.1 us [19.3 us-84.3 us] (runs=5) |
| lookup: get_event(id) (new API) | 3.16 ms [2.43 ms-4.80 ms] (runs=5) | 1.83 ms [1.39 ms-3.21 ms] (runs=5) | 1.68 ms [1.44 ms-2.82 ms] (runs=5) |
| lookup: SQL, registered ctx | 29.43 ms [23.76 ms-51.37 ms] (runs=5) | 25.65 ms [18.80 ms-39.67 ms] (runs=5) | 26.88 ms [19.25 ms-50.36 ms] (runs=5) |
| append 1 event/commit, per commit | 10.62 ms [6.61 ms-13.69 ms] (runs=5) | 11.02 ms [6.69 ms-17.42 ms] (runs=5) | 7.78 ms [5.92 ms-9.05 ms] (runs=5) |
| append 1000-event batch | 15.35 ms [10.45 ms-20.24 ms] (runs=5) | 14.69 ms [10.78 ms-18.82 ms] (runs=5) | 11.43 ms [10.17 ms-16.26 ms] (runs=5) |
| range query, SQL registered ctx | 18.39 ms [11.49 ms-51.63 ms] (runs=5) | 16.91 ms [9.85 ms-63.39 ms] (runs=5) | 17.93 ms [12.76 ms-40.57 ms] (runs=5) |
| as-of mid version, open_at | 16.89 ms [10.01 ms-47.25 ms] (runs=5) | 11.26 ms [6.29 ms-67.90 ms] (runs=5) | 13.20 ms [8.52 ms-51.59 ms] (runs=5) |

| fact | BEFORE (round-1 binary, format 2) | AFTER, format 2 | AFTER, format 3 (journal) |
|---|---|---|---|
| file count | 32 | 32 | 32 |
| re-ingest same events: new versions | 0 | 0 | 0 |
| re-ingest same events: seconds (median of runs shown individually) | 0.05 / 0.06 / 0.07 / 0.09 | 0.04 / 0.05 | 0.04 / 0.05 / 0.06 / 0.08 |
| revision replay: revisions before->after | 200->200 | 200->200 | 200->200 |
| revision replay: new versions | 0 | 0 | 0 |
| tamper trials / detected by verify / detected only by open / silent / harmless | 40/40/0/0/0 | 40/40/0/0/0 | 40/40/0/0/0 |
| log_format recorded | 2 | 2 | 3 |

Query-hash equality across all runs and formats: asof v1=MATCH, asof v10=MATCH, asof v5=MATCH, fullagg=MATCH, lookup=MATCH, range=MATCH

#### N=100,000 (rdata)

| metric | BEFORE (round-1 binary, format 2) | AFTER, format 2 | AFTER, format 3 (journal) |
|---|---|---|---|
| total size on disk | 8.86 MB [8.86 MB-8.86 MB] (runs=5) | 8.91 MB [8.91 MB-8.91 MB] (runs=5) | 8.91 MB [8.91 MB-8.91 MB] (runs=5) |
|   events | 8.47 MB [8.47 MB-8.47 MB] (runs=5) | 8.51 MB [8.51 MB-8.51 MB] (runs=5) | 8.51 MB [8.51 MB-8.51 MB] (runs=5) |
|   revisions | 0.20 MB [0.20 MB-0.20 MB] (runs=5) | 0.20 MB [0.20 MB-0.20 MB] (runs=5) | 0.20 MB [0.20 MB-0.20 MB] (runs=5) |
|   revision_causes | 0.13 MB [0.13 MB-0.13 MB] (runs=5) | 0.13 MB [0.13 MB-0.13 MB] (runs=5) | 0.13 MB [0.13 MB-0.13 MB] (runs=5) |
|   MANIFEST | 0.07 MB [0.07 MB-0.07 MB] (runs=5) | 0.07 MB [0.07 MB-0.07 MB] (runs=5) | 0.07 MB [0.07 MB-0.07 MB] (runs=5) |
| bulk build, events/s (batches of 1000) | 25,411/s [19,163/s-27,263/s] (runs=5) | 32,214/s [30,445/s-34,332/s] (runs=5) | 33,989/s [25,451/s-37,970/s] (runs=5) |
| open() | 64.49 ms [30.41 ms-133.53 ms] (runs=5) | 42.70 ms [31.08 ms-66.02 ms] (runs=5) | 52.36 ms [29.75 ms-154.95 ms] (runs=5) |
| open() + decode latest snapshot | 166.53 ms [143.48 ms-253.94 ms] (runs=5) | 177.04 ms [141.65 ms-2.33 s] (runs=5) | 187.84 ms [141.54 ms-294.84 ms] (runs=5) |
| verify() | 70.45 ms [64.05 ms-105.74 ms] (runs=5) | 68.65 ms [59.51 ms-115.18 ms] (runs=5) | 90.22 ms [64.99 ms-176.33 ms] (runs=5) |
| lookup: has_event (existence) | 97 ns [94 ns-125 ns] (runs=5) | 104 ns [92 ns-116 ns] (runs=5) | 100 ns [95 ns-116 ns] (runs=5) |
| lookup: scan of in-memory snapshot | 933.3 us [667.5 us-1.23 ms] (runs=5) | 977.8 us [677.3 us-1.52 ms] (runs=5) | 1.05 ms [732.3 us-1.30 ms] (runs=5) |
| lookup: get_event(id) (new API) | 2.79 ms [2.32 ms-3.52 ms] (runs=5) | 1.25 ms [1.05 ms-2.23 ms] (runs=5) | 1.66 ms [1.32 ms-2.27 ms] (runs=5) |
| lookup: SQL, registered ctx | 148.03 ms [129.30 ms-232.98 ms] (runs=5) | 141.53 ms [127.42 ms-235.09 ms] (runs=5) | 142.02 ms [121.92 ms-202.95 ms] (runs=5) |
| append 1 event/commit, per commit | 10.10 ms [8.60 ms-11.76 ms] (runs=5) | 10.18 ms [8.50 ms-23.28 ms] (runs=5) | 6.47 ms [5.96 ms-7.00 ms] (runs=5) |
| append 1000-event batch | 16.16 ms [14.14 ms-24.58 ms] (runs=5) | 16.45 ms [13.60 ms-20.40 ms] (runs=5) | 11.80 ms [10.64 ms-14.42 ms] (runs=5) |
| range query, SQL registered ctx | 93.49 ms [84.88 ms-244.81 ms] (runs=5) | 99.51 ms [88.79 ms-215.94 ms] (runs=5) | 113.73 ms [82.10 ms-220.05 ms] (runs=5) |
| as-of mid version, open_at | 42.28 ms [35.67 ms-102.41 ms] (runs=5) | 43.17 ms [36.86 ms-78.33 ms] (runs=5) | 49.48 ms [37.56 ms-162.99 ms] (runs=5) |

| fact | BEFORE (round-1 binary, format 2) | AFTER, format 2 | AFTER, format 3 (journal) |
|---|---|---|---|
| file count | 302 | 302 | 302 |
| re-ingest same events: new versions | 0 | 0 | 0 |
| re-ingest same events: seconds (median of runs shown individually) | 0.59 / 0.63 / 0.66 / 0.71 / 0.74 | 0.42 / 0.46 / 0.47 / 0.50 / 0.54 | 0.40 / 0.45 / 0.69 |
| revision replay: revisions before->after | 2000->2000 | 2000->2000 | 2000->2000 |
| revision replay: new versions | 0 | 0 | 0 |
| tamper trials / detected by verify / detected only by open / silent / harmless | 40/40/0/0/0 | 40/40/0/0/0 | 40/40/0/0/0 |
| log_format recorded | 2 | 2 | 3 |

Query-hash equality across all runs and formats: asof v1=MATCH, asof v100=MATCH, asof v50=MATCH, fullagg=MATCH, lookup=MATCH, range=MATCH
