
#### N=10,000 (rdata)

| metric | before (old code) | after, format 1 | after, format 2 |
|---|---|---|---|
| total size on disk | 2.28 MB [2.28 MB-2.28 MB] (runs=3) | 2.28 MB [2.28 MB-2.28 MB] (runs=3) | 0.88 MB [0.88 MB-0.88 MB] (runs=3) |
|   events | 2.20 MB [2.20 MB-2.20 MB] (runs=3) | 2.20 MB [2.20 MB-2.20 MB] (runs=3) | 0.84 MB [0.84 MB-0.84 MB] (runs=3) |
|   revisions | 0.04 MB [0.04 MB-0.04 MB] (runs=3) | 0.04 MB [0.04 MB-0.04 MB] (runs=3) | 0.02 MB [0.02 MB-0.02 MB] (runs=3) |
|   revision_causes | 0.03 MB [0.03 MB-0.03 MB] (runs=3) | 0.03 MB [0.03 MB-0.03 MB] (runs=3) | 0.01 MB [0.01 MB-0.01 MB] (runs=3) |
|   MANIFEST | 0.01 MB [0.01 MB-0.01 MB] (runs=3) | 0.01 MB [0.01 MB-0.01 MB] (runs=3) | 0.01 MB [0.01 MB-0.01 MB] (runs=3) |
| bulk build, events/s (batches of 1000) | 50,536/s [41,794/s-51,285/s] (runs=3) | 51,454/s [39,479/s-59,434/s] (runs=3) | 34,281/s [29,847/s-42,971/s] (runs=3) |
| open() | 26.70 ms [18.55 ms-59.52 ms] (runs=3) | 15.29 ms [10.55 ms-28.04 ms] (runs=3) | 4.07 ms [3.25 ms-16.09 ms] (runs=3) |
| open() + decode latest snapshot | n/a | 16.52 ms [11.65 ms-29.34 ms] (runs=3) | 18.34 ms [12.26 ms-21.20 ms] (runs=3) |
| verify() | 17.25 ms [13.63 ms-104.40 ms] (runs=3) | 10.29 ms [7.76 ms-16.54 ms] (runs=3) | 11.90 ms [9.05 ms-17.07 ms] (runs=3) |
| lookup: has_event (existence) | 24 ns [22 ns-67 ns] (runs=3) | 66 ns [63 ns-74 ns] (runs=3) | 64 ns [63 ns-141 ns] (runs=3) |
| lookup: scan of in-memory snapshot | 12.6 us [11.5 us-47.7 us] (runs=3) | 13.5 us [11.9 us-63.4 us] (runs=3) | 12.7 us [11.7 us-29.9 us] (runs=3) |
| lookup: get_event(id) (new API) | n/a | 360 ns [304 ns-413 ns] (runs=3) | 1.72 ms [1.54 ms-2.64 ms] (runs=3) |
| lookup: SQL, registered ctx | 14.66 ms [12.10 ms-29.76 ms] (runs=3) | 14.39 ms [11.90 ms-23.32 ms] (runs=3) | 13.78 ms [10.79 ms-31.03 ms] (runs=3) |
| append 1 event/commit, per commit | 61.48 ms [29.24 ms-139.10 ms] (runs=3) | 28.79 ms [23.89 ms-48.25 ms] (runs=3) | 22.94 ms [7.09 ms-38.92 ms] (runs=3) |
| append 1000-event batch | 33.47 ms [15.45 ms-1692.17 ms] (runs=3) | 25.74 ms [12.92 ms-135.17 ms] (runs=3) | 19.69 ms [9.85 ms-163.29 ms] (runs=3) |
| range query, SQL registered ctx | 10.42 ms [8.66 ms-16.08 ms] (runs=3) | 12.01 ms [8.37 ms-17.85 ms] (runs=3) | 22.27 ms [9.29 ms-48.26 ms] (runs=3) |
| as-of mid version, open_at | 8.03 ms [6.59 ms-14.38 ms] (runs=3) | 5.10 ms [3.32 ms-27.40 ms] (runs=3) | 7.15 ms [4.34 ms-29.88 ms] (runs=3) |

| fact | before (old code) | after, format 1 | after, format 2 |
|---|---|---|---|
| file count | 32 | 32 | 32 |
| re-ingest same events: new versions | 0 | 0 | 0 |
| re-ingest same events: seconds (median of runs shown individually) | 0.08 / 0.15 / 0.51 | 0.05 / 0.07 / 0.13 | 0.03 / 0.36 / 0.39 |
| revision replay: revisions before->after | 200->400 | 200->400 | 200->200 |
| revision replay: new versions | 10 | 10 | 0 |
| tamper trials / detected by verify / detected only by open / silent / harmless | 40/40/0/0/0 | 40/40/0/0/0 | 40/40/0/0/0 |
| log_format recorded | 1 | 1 | 2 |

Query-hash equality across all runs and formats: asof v1=MATCH, asof v10=MATCH, asof v5=MATCH, fullagg=MATCH, lookup=MATCH, range=MATCH
