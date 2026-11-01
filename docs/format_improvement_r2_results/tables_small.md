
#### N=10,000 (rdata)

| metric | BEFORE (round-1 binary, format 2) | AFTER, format 2 | AFTER, format 3 (journal) |
|---|---|---|---|
| total size on disk | 0.88 MB [0.88 MB-0.88 MB] (runs=5) | 0.88 MB [0.88 MB-0.88 MB] (runs=5) | 0.88 MB [0.88 MB-0.88 MB] (runs=5) |
|   events | 0.84 MB [0.84 MB-0.84 MB] (runs=5) | 0.84 MB [0.84 MB-0.84 MB] (runs=5) | 0.84 MB [0.84 MB-0.84 MB] (runs=5) |
|   revisions | 0.02 MB [0.02 MB-0.02 MB] (runs=5) | 0.02 MB [0.02 MB-0.02 MB] (runs=5) | 0.02 MB [0.02 MB-0.02 MB] (runs=5) |
|   revision_causes | 0.01 MB [0.01 MB-0.01 MB] (runs=5) | 0.01 MB [0.01 MB-0.01 MB] (runs=5) | 0.01 MB [0.01 MB-0.01 MB] (runs=5) |
|   MANIFEST | 0.01 MB [0.01 MB-0.01 MB] (runs=5) | 0.01 MB [0.01 MB-0.01 MB] (runs=5) | 0.01 MB [0.01 MB-0.01 MB] (runs=5) |
| bulk build, events/s (batches of 1000) | 26,937/s [23,799/s-31,046/s] (runs=5) | 37,659/s [29,835/s-43,520/s] (runs=5) | 41,033/s [28,227/s-45,199/s] (runs=5) |
| open() | 7.43 ms [3.34 ms-23.40 ms] (runs=5) | 5.13 ms [3.04 ms-15.31 ms] (runs=5) | 3.81 ms [3.01 ms-6.04 ms] (runs=5) |
| open() + decode latest snapshot | 16.14 ms [13.92 ms-60.41 ms] (runs=5) | 27.44 ms [13.51 ms-42.40 ms] (runs=5) | 14.96 ms [13.42 ms-49.83 ms] (runs=5) |
| verify() | 12.00 ms [9.97 ms-27.22 ms] (runs=5) | 10.73 ms [8.29 ms-54.49 ms] (runs=5) | 10.60 ms [7.60 ms-27.63 ms] (runs=5) |
| lookup: has_event (existence) | 86 ns [80 ns-116 ns] (runs=5) | 92 ns [81 ns-138 ns] (runs=5) | 89 ns [84 ns-99 ns] (runs=5) |
| lookup: scan of in-memory snapshot | 19.3 us [16.3 us-33.7 us] (runs=5) | 18.7 us [17.9 us-21.9 us] (runs=5) | 19.1 us [16.7 us-59.6 us] (runs=5) |
| lookup: get_event(id) (new API) | 2.11 ms [1.86 ms-3.38 ms] (runs=5) | 1.02 ms [907.2 us-1.78 ms] (runs=5) | 1.02 ms [918.5 us-1.42 ms] (runs=5) |
| lookup: SQL, registered ctx | 16.52 ms [13.38 ms-23.33 ms] (runs=5) | 18.07 ms [15.62 ms-26.50 ms] (runs=5) | 19.07 ms [16.86 ms-23.39 ms] (runs=5) |
| append 1 event/commit, per commit | 9.33 ms [8.36 ms-19.78 ms] (runs=5) | 8.99 ms [8.06 ms-13.50 ms] (runs=5) | 6.38 ms [5.51 ms-11.04 ms] (runs=5) |
| append 1000-event batch | 14.60 ms [13.19 ms-19.63 ms] (runs=5) | 13.92 ms [11.66 ms-20.55 ms] (runs=5) | 10.50 ms [9.38 ms-15.43 ms] (runs=5) |
| range query, SQL registered ctx | 13.01 ms [11.84 ms-24.95 ms] (runs=5) | 13.37 ms [10.99 ms-30.02 ms] (runs=5) | 13.44 ms [11.30 ms-25.28 ms] (runs=5) |
| as-of mid version, open_at | 4.69 ms [3.65 ms-14.26 ms] (runs=5) | 7.50 ms [3.51 ms-17.11 ms] (runs=5) | 4.91 ms [3.97 ms-28.25 ms] (runs=5) |

| fact | BEFORE (round-1 binary, format 2) | AFTER, format 2 | AFTER, format 3 (journal) |
|---|---|---|---|
| file count | 32 | 32 | 32 |
| re-ingest same events: new versions | 0 | 0 | 0 |
| re-ingest same events: seconds (median of runs shown individually) | 0.04 / 0.05 | 0.03 | 0.03 |
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
| bulk build, events/s (batches of 1000) | 28,893/s [27,580/s-29,628/s] (runs=5) | 35,733/s [31,830/s-36,352/s] (runs=5) | 38,694/s [33,784/s-40,519/s] (runs=5) |
| open() | 30.87 ms [28.27 ms-42.58 ms] (runs=5) | 37.33 ms [29.45 ms-53.19 ms] (runs=5) | 36.47 ms [29.19 ms-64.05 ms] (runs=5) |
| open() + decode latest snapshot | 143.97 ms [126.32 ms-212.22 ms] (runs=5) | 141.52 ms [123.32 ms-240.19 ms] (runs=5) | 136.39 ms [119.30 ms-187.27 ms] (runs=5) |
| verify() | 66.56 ms [59.58 ms-84.62 ms] (runs=5) | 63.70 ms [53.79 ms-161.84 ms] (runs=5) | 65.93 ms [59.88 ms-91.36 ms] (runs=5) |
| lookup: has_event (existence) | 95 ns [87 ns-109 ns] (runs=5) | 97 ns [92 ns-106 ns] (runs=5) | 92 ns [86 ns-104 ns] (runs=5) |
| lookup: scan of in-memory snapshot | 730.3 us [618.1 us-1.24 ms] (runs=5) | 721.0 us [652.8 us-1.14 ms] (runs=5) | 862.2 us [616.5 us-1.20 ms] (runs=5) |
| lookup: get_event(id) (new API) | 2.22 ms [2.00 ms-2.41 ms] (runs=5) | 1.14 ms [1.04 ms-1.42 ms] (runs=5) | 1.15 ms [965.4 us-1.39 ms] (runs=5) |
| lookup: SQL, registered ctx | 117.79 ms [112.77 ms-145.60 ms] (runs=5) | 125.23 ms [111.16 ms-162.39 ms] (runs=5) | 119.61 ms [110.91 ms-181.53 ms] (runs=5) |
| append 1 event/commit, per commit | 9.35 ms [8.44 ms-10.53 ms] (runs=5) | 9.56 ms [8.70 ms-11.37 ms] (runs=5) | 6.27 ms [5.67 ms-6.78 ms] (runs=5) |
| append 1000-event batch | 14.69 ms [13.97 ms-18.50 ms] (runs=5) | 13.82 ms [13.25 ms-104.05 ms] (runs=5) | 10.61 ms [9.88 ms-15.52 ms] (runs=5) |
| range query, SQL registered ctx | 86.67 ms [77.07 ms-155.20 ms] (runs=5) | 86.29 ms [74.13 ms-125.72 ms] (runs=5) | 91.80 ms [79.34 ms-233.22 ms] (runs=5) |
| as-of mid version, open_at | 37.25 ms [31.79 ms-63.72 ms] (runs=5) | 33.99 ms [31.92 ms-84.52 ms] (runs=5) | 35.44 ms [31.60 ms-62.87 ms] (runs=5) |

| fact | BEFORE (round-1 binary, format 2) | AFTER, format 2 | AFTER, format 3 (journal) |
|---|---|---|---|
| file count | 302 | 302 | 302 |
| re-ingest same events: new versions | 0 | 0 | 0 |
| re-ingest same events: seconds (median of runs shown individually) | 0.51 / 0.52 / 0.54 | 0.32 / 0.34 / 0.37 / 0.44 | 0.33 / 0.34 / 0.38 / 0.39 |
| revision replay: revisions before->after | 2000->2000 | 2000->2000 | 2000->2000 |
| revision replay: new versions | 0 | 0 | 0 |
| tamper trials / detected by verify / detected only by open / silent / harmless | 40/40/0/0/0 | 40/40/0/0/0 | 40/40/0/0/0 |
| log_format recorded | 2 | 2 | 3 |

Query-hash equality across all runs and formats: asof v1=MATCH, asof v100=MATCH, asof v50=MATCH, fullagg=MATCH, lookup=MATCH, range=MATCH
