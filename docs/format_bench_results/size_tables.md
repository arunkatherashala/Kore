#### Dataset (realistic mix, see format_bench_gen_realistic.py)

| N | avg text bytes | raw text total | raw JSONL file |
|---|---|---|---|
| 10,000 | 353 | 3.53 MB | 4.69 MB |
| 100,000 | 369 | 36.89 MB | 48.50 MB |
| 1,000,000 | 371 | 371.00 MB | 487.13 MB |

#### Total size, bytes per event, compression ratio vs raw text (raw text bytes / stored bytes; below 1.0 = larger than the text alone)

| format | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE log | 2.28 MB, 227.6 B/event, ratio 1.55 | 22.98 MB, 229.8 B/event, ratio 1.61 | 229.82 MB, 229.8 B/event, ratio 1.61 |
| SQLite | 6.56 MB, 655.8 B/event, ratio 0.54 | 68.12 MB, 681.2 B/event, ratio 0.54 | 685.60 MB, 685.6 B/event, ratio 0.54 |
| SQLite after VACUUM | 6.48 MB, 647.6 B/event, ratio 0.54 | 67.22 MB, 672.2 B/event, ratio 0.55 | 676.61 MB, 676.6 B/event, ratio 0.55 |
| CSV (3 files) | 5.00 MB, 500.3 B/event, ratio 0.70 | 51.81 MB, 518.1 B/event, ratio 0.71 | 521.27 MB, 521.3 B/event, ratio 0.71 |
| gzip -6 of CSV | 1.36 MB, 136.0 B/event, ratio 2.59 | 13.81 MB, 138.1 B/event, ratio 2.67 | 138.43 MB, 138.4 B/event, ratio 2.68 |
| gzip -9 of CSV | 1.40 MB, 140.4 B/event, ratio 2.51 | 14.24 MB, 142.4 B/event, ratio 2.59 | 142.78 MB, 142.8 B/event, ratio 2.60 |

#### Bytes per event vs N (linearity) and largest N that fits in a 100 MB budget

The N-in-100-MB figure is an EXTRAPOLATION: 100,000,000 / (bytes per event at the largest measured N). It assumes bytes/event stays flat beyond the measured range (it was not tested to 100 MB for most formats).

| format | B/event @ 10,000 | B/event @ 100,000 | B/event @ 1,000,000 | change first->last | events in 100 MB (extrapolated) |
|---|---|---|---|---|---|
| KORE log | 227.6 | 229.8 | 229.8 | +1.0% | 435,124 |
| SQLite | 655.8 | 681.2 | 685.6 | +4.5% | 145,857 |
| SQLite after VACUUM | 647.6 | 672.2 | 676.6 | +4.5% | 147,797 |
| CSV (3 files) | 500.3 | 518.1 | 521.3 | +4.2% | 191,839 |
| gzip -6 of CSV | 136.0 | 138.1 | 138.4 | +1.8% | 722,379 |
| gzip -9 of CSV | 140.4 | 142.4 | 142.8 | +1.7% | 700,365 |

#### Queryable without full decompression?

Times: median [min-max] of 3-5 runs; store/file warm in OS cache. Lookup = one event id near the middle of the file (scan paths exit early); range = COUNT by kind over the middle 10% of the time range.

| path | what it does | N=10,000 lookup | N=100,000 lookup | N=1,000,000 lookup | N=10,000 range | N=100,000 range | N=1,000,000 range |
|---|---|---|---|---|---|---|---|
| KORE has_event / SQL (open log) | hash index existence (no row) / kore-sql scan on registered tables | 40 ns [40 ns-45 ns] / 31.40 ms [26.23 ms-33.60 ms] | 46 ns [44 ns-51 ns] / 148.85 ms [144.45 ms-150.11 ms] | 47 ns [46 ns-465 ns] / 1.27 s [1.23 s-2.32 s] | 35.30 ms [17.04 ms-44.15 ms] | 96.91 ms [92.35 ms-97.76 ms] | 979.10 ms [971.88 ms-1.09 s] |
| KORE hand-written Rust scan (open log) | in-memory rows (already decoded at open) | 38.9 us [23.6 us-112.3 us] | 975.3 us [880.7 us-1.12 ms] | 11.59 ms [11.00 ms-14.16 ms] | 166.8 us [164.3 us-233.2 us] | 4.09 ms [3.44 ms-4.46 ms] | 50.81 ms [44.98 ms-56.60 ms] |
| KORE open() cost (paid once) | reads + decodes + sha256-checks every segment | 51.81 ms [43.95 ms-60.39 ms] | 470.41 ms [415.31 ms-513.12 ms] | 9.17 s [7.48 s-11.39 s] | - | - | - |
| SQLite | PK lookup / scan with GROUP BY, pages read on demand | 129.7 us [110.0 us-199.4 us] | 120.5 us [77.2 us-408.4 us] | 60.6 us [50.9 us-108.4 us] | 11.48 ms [8.51 ms-21.10 ms] | 195.03 ms [163.89 ms-214.73 ms] | 1.06 s [1.00 s-1.55 s] |
| plain CSV | scan with csv module (lookup exits early) | 29.44 ms [29.32 ms-39.11 ms] | 555.38 ms [517.52 ms-560.74 ms] | 3.95 s [3.31 s-4.10 s] | 89.09 ms [65.87 ms-107.07 ms] | 826.25 ms [779.72 ms-1.20 s] | 5.87 s [5.52 s-6.43 s] |
| gzip -6 CSV | must stream-decompress from the start | 47.53 ms [44.98 ms-59.04 ms] | 815.79 ms [784.75 ms-859.82 ms] | 4.01 s [3.92 s-4.43 s] | 82.91 ms [71.80 ms-90.06 ms] | 1.56 s [1.26 s-1.66 s] | 8.65 s [8.47 s-9.02 s] |
| gzip -9 CSV | same | 36.72 ms [35.19 ms-45.26 ms] | 492.84 ms [477.37 ms-495.33 ms] | 4.12 s [4.01 s-4.60 s] | 72.45 ms [64.65 ms-81.75 ms] | 781.85 ms [744.25 ms-862.16 ms] | 8.33 s [7.89 s-8.66 s] |

#### Cost to produce / fully decompress

| item | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE build (commit_batch x N/1000, fsync per commit, 1 run) | 301.84 ms | 3.91 s | 147.89 s |
| gzip -6 compress (3 CSV files) | 144.01 ms | 1.51 s | 18.51 s |
| gzip -9 compress | 634.69 ms | 3.13 s | 24.60 s |
| gzip -6 full decompress (read only, no parsing) | 27.33 ms | 178.88 ms | 1.74 s |
| SQLite VACUUM | 247.51 ms | 6.54 s | 27.21 s |

#### What dominates the KORE size (events table, per column)

KORE column figure = that column alone re-encoded with the existing KoreWriter in 1000-row chunks (same chunking as the log). gzip figures = gzip -6 of the column values (one value per line), whole column and in 1000-row chunks. Raw = UTF-8 bytes + 1.


N=10,000: KORE size_by_table = {'MANIFEST': 6804, 'events': 2204656, 'outcomes': 0, 'predictions': 0, 'revision_causes': 28091, 'revisions': 36301, 'schema_meta': 108} ; files = 32

| column | raw | KORE encoded | gzip-6 whole column | gzip-6 1000-row chunks |
|---|---|---|---|---|
| event_id | 650,000 | 683,146 | 355,266 | 354,767 |
| occurred_at | 210,000 | 100,282 | 33,837 | 34,314 |
| source | 64,806 | 65,572 | 7,100 | 8,927 |
| kind | 78,036 | 66,127 | 5,729 | 7,071 |
| text | 3,536,932 | 1,154,159 | 651,578 | 677,949 |
| tags | 121,216 | 93,070 | 26,525 | 28,856 |
| recorded_at (KORE/SQLite/CSV store it; one value per batch) | - | 42,980 | - | - |
| redacted | - | 580 | - | - |

N=100,000: KORE size_by_table = {'MANIFEST': 64765, 'events': 22207519, 'outcomes': 0, 'predictions': 0, 'revision_causes': 280902, 'revisions': 423670, 'schema_meta': 108} ; files = 302

| column | raw | KORE encoded | gzip-6 whole column | gzip-6 1000-row chunks |
|---|---|---|---|---|
| event_id | 6,500,000 | 6,831,566 | 3,553,079 | 3,547,345 |
| occurred_at | 2,100,000 | 1,002,851 | 335,822 | 342,861 |
| source | 649,586 | 655,577 | 69,019 | 89,177 |
| kind | 779,374 | 658,235 | 55,912 | 70,992 |
| text | 36,994,311 | 11,703,656 | 6,657,092 | 6,960,746 |
| tags | 1,216,202 | 932,634 | 261,369 | 289,966 |
| recorded_at (KORE/SQLite/CSV store it; one value per batch) | - | 429,800 | - | - |
| redacted | - | 5,800 | - | - |

N=1,000,000: KORE size_by_table = {'MANIFEST': 645266, 'events': 222066593, 'outcomes': 0, 'predictions': 0, 'revision_causes': 2809076, 'revisions': 4298515, 'schema_meta': 108} ; files = 3002

| column | raw | KORE encoded | gzip-6 whole column | gzip-6 1000-row chunks |
|---|---|---|---|---|
| event_id | 65,000,000 | 68,315,736 | 35,532,311 | 35,474,606 |
| occurred_at | 21,000,000 | 10,028,344 | 3,356,156 | 3,428,533 |
| source | 6,501,103 | 6,554,106 | 686,921 | 891,179 |
| kind | 7,798,536 | 6,579,631 | 557,297 | 710,458 |
| text | 371,998,959 | 117,019,926 | 66,793,030 | 69,853,529 |
| tags | 12,184,838 | 9,338,850 | 2,614,716 | 2,903,610 |
| recorded_at (KORE/SQLite/CSV store it; one value per batch) | - | 4,298,000 | - | - |
| redacted | - | 58,000 | - | - |
