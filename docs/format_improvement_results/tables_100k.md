
#### N=100,000 (rdata)

| metric | before (old code) | after, format 1 | after, format 2 |
|---|---|---|---|
| total size on disk | 22.98 MB [22.98 MB-22.98 MB] (runs=3) | 22.98 MB [22.98 MB-22.98 MB] (runs=3) | 8.86 MB [8.86 MB-8.86 MB] (runs=3) |
|   events | 22.21 MB [22.21 MB-22.21 MB] (runs=3) | 22.21 MB [22.21 MB-22.21 MB] (runs=3) | 8.47 MB [8.47 MB-8.47 MB] (runs=3) |
|   revisions | 0.42 MB [0.42 MB-0.42 MB] (runs=3) | 0.42 MB [0.42 MB-0.42 MB] (runs=3) | 0.20 MB [0.20 MB-0.20 MB] (runs=3) |
|   revision_causes | 0.28 MB [0.28 MB-0.28 MB] (runs=3) | 0.28 MB [0.28 MB-0.28 MB] (runs=3) | 0.13 MB [0.13 MB-0.13 MB] (runs=3) |
|   MANIFEST | 0.06 MB [0.06 MB-0.06 MB] (runs=3) | 0.06 MB [0.06 MB-0.06 MB] (runs=3) | 0.07 MB [0.07 MB-0.07 MB] (runs=3) |
| bulk build, events/s (batches of 1000) | 29,489/s [24,123/s-36,088/s] (runs=3) | 36,925/s [34,256/s-39,545/s] (runs=3) | 28,402/s [22,682/s-33,502/s] (runs=3) |
| open() | 309.82 ms [254.39 ms-598.72 ms] (runs=3) | 143.30 ms [121.40 ms-196.32 ms] (runs=3) | 35.92 ms [26.43 ms-55.82 ms] (runs=3) |
| open() + decode latest snapshot | n/a | 169.21 ms [132.85 ms-270.52 ms] (runs=3) | 138.84 ms [122.28 ms-200.01 ms] (runs=3) |
| verify() | 196.46 ms [178.19 ms-473.92 ms] (runs=3) | 71.13 ms [53.97 ms-144.71 ms] (runs=3) | 64.86 ms [57.79 ms-118.58 ms] (runs=3) |
| lookup: has_event (existence) | 43 ns [28 ns-130 ns] (runs=3) | 110 ns [99 ns-265 ns] (runs=3) | 94 ns [85 ns-140 ns] (runs=3) |
| lookup: scan of in-memory snapshot | 590.0 us [511.1 us-1.26 ms] (runs=3) | 1.31 ms [955.6 us-5.69 ms] (runs=3) | 767.9 us [573.6 us-1.40 ms] (runs=3) |
| lookup: get_event(id) (new API) | n/a | 574 ns [513 ns-7.0 us] (runs=3) | 2.25 ms [1.89 ms-3.28 ms] (runs=3) |
| lookup: SQL, registered ctx | 122.76 ms [104.82 ms-137.36 ms] (runs=3) | 141.16 ms [124.08 ms-182.89 ms] (runs=3) | 112.16 ms [103.47 ms-170.94 ms] (runs=3) |
| append 1 event/commit, per commit | 21.79 ms [20.64 ms-32.17 ms] (runs=3) | 10.94 ms [8.88 ms-13.63 ms] (runs=3) | 9.00 ms [8.17 ms-15.79 ms] (runs=3) |
| append 1000-event batch | 25.71 ms [22.83 ms-31.54 ms] (runs=3) | 14.66 ms [12.00 ms-22.40 ms] (runs=3) | 14.25 ms [11.70 ms-24.90 ms] (runs=3) |
| range query, SQL registered ctx | 84.14 ms [74.44 ms-99.56 ms] (runs=3) | 87.82 ms [79.86 ms-119.49 ms] (runs=3) | 83.55 ms [76.73 ms-116.98 ms] (runs=3) |
| as-of mid version, open_at | 118.81 ms [97.64 ms-229.96 ms] (runs=3) | 36.94 ms [33.97 ms-51.47 ms] (runs=3) | 39.06 ms [31.64 ms-47.88 ms] (runs=3) |

| fact | before (old code) | after, format 1 | after, format 2 |
|---|---|---|---|
| file count | 302 | 302 | 302 |
| re-ingest same events: new versions | 0 | 0 | 0 |
| re-ingest same events: seconds (median of runs shown individually) | 1.60 / 1.92 / 5.93 | 0.56 / 0.68 / 0.75 | 0.51 / 0.61 / 1.05 |
| revision replay: revisions before->after | 2000->4000 | 2000->4000 | 2000->2000 |
| revision replay: new versions | 100 | 100 | 0 |
| tamper trials / detected by verify / detected only by open / silent / harmless | 40/40/0/0/0 | 40/40/0/0/0 | 40/40/0/0/0 |
| log_format recorded | 1 | 1 | 2 |

Query-hash equality across all runs and formats: asof v1=MATCH, asof v100=MATCH, asof v50=MATCH, fullagg=MATCH, lookup=MATCH, range=MATCH
