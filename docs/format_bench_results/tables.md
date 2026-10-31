### Consistency checks (same query result on every system)

- N=10,000: lookup=MATCH, range=MATCH, fullagg=MATCH, asof v1=MATCH, asof v10=MATCH, asof v5=MATCH
- N=100,000: lookup=MATCH, range=MATCH, fullagg=MATCH, asof v1=MATCH, asof v100=MATCH, asof v50=MATCH
- N=1,000,000: lookup=MATCH, range=MATCH, fullagg=MATCH, asof v1=MATCH, asof v1000=MATCH, asof v500=MATCH

All checks match: **True**

### Run conditions

- N=10,000: python 3.14.5, sqlite 3.50.4, Windows-11-10.0.26100-SP0, cpu_count=8, reps=5, build_reps=5, csv_lookup_ops=100, started 2026-10-31T17:58:31Z, finished 2026-10-31T17:58:58Z
- N=100,000: python 3.14.5, sqlite 3.50.4, Windows-11-10.0.26100-SP0, cpu_count=8, reps=5, build_reps=5, csv_lookup_ops=20, started 2026-10-31T17:59:39Z, finished 2026-10-31T18:07:08Z
- N=1,000,000: python 3.14.5, sqlite 3.50.4, Windows-11-10.0.26100-SP0, cpu_count=8, reps=5, build_reps=2, csv_lookup_ops=5, started 2026-10-31T18:11:27Z, finished 2026-10-31T18:41:15Z

#### (a) On-disk size after loading the dataset in batches of 1000

| system | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE versioned log (events+revisions+causes+MANIFEST) | 1.62 MB | 16.18 MB | 161.79 MB |
| SQLite (PK on event_id, version column) | 3.53 MB | 35.59 MB | 357.65 MB |
| SQLite [256 MiB cache] (PK on event_id, version column) | 3.53 MB | 35.59 MB | 357.65 MB |
| SQLite + 3 secondary indexes | 3.97 MB | 40.01 MB | 403.62 MB |
| CSV (3 files, version column) | 2.52 MB | 25.41 MB | 256.39 MB |

Raw JSONL input for reference: N=10,000: 2.20 MB, N=100,000: 22.10 MB, N=1,000,000: 222.22 MB. KORE file count: N=10,000: 32, N=100,000: 302, N=1,000,000: 3002.

#### (b1) Bulk load, batches of 1000 events, fsync per commit: events/s (from-scratch build, includes ~N/50 revisions)

| system | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE | 43,565 [42,779-46,763] (n=5) | 31,146 [28,029-32,797] (n=5) | 8,237 [8,232-8,241] (n=2) |
| SQLite (DELETE journal, synchronous=FULL) | 47,490 [35,704-47,721] (n=5) | 8,389 [7,559-9,040] (n=5) | 4,159 [3,909-4,408] (n=2) |
| SQLite [256 MiB cache] (DELETE journal, synchronous=FULL) | 44,010 [29,283-48,976] (n=5) | 5,676 [1,353-8,488] (n=5) | 4,346 [3,965-4,728] (n=2) |
| SQLite + indexes (same sync) | 42,384 [33,289-45,159] (n=5) | 7,985 [6,989-8,298] (n=5) | 3,649 [3,632-3,666] (n=2) |
| CSV (append + flush + fsync per batch) | 135,577 [113,948-139,290] (n=5) | 10,721 [6,374-132,032] (n=5) | 103,423 [101,549-105,296] (n=2) |

#### (b2) Append of 1 event per commit onto an existing N-event store: ms per commit (100 commits per rep)

| system | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE (one new version per event) | 11.21 ms [10.13 ms-12.12 ms] (n=5) | 27.48 ms [24.58 ms-29.24 ms] (n=5) | 152.86 ms [140.53 ms-230.80 ms] (n=5) |
| SQLite DELETE/FULL (default) | 4.32 ms [4.16 ms-4.72 ms] (n=5) | 5.31 ms [5.11 ms-5.71 ms] (n=5) | 7.00 ms [6.26 ms-9.75 ms] (n=5) |
| SQLite [256 MiB cache] DELETE/FULL (default) | 4.20 ms [4.02 ms-5.29 ms] (n=5) | 4.47 ms [4.35 ms-7.49 ms] (n=5) | 5.88 ms [5.53 ms-7.58 ms] (n=5) |
| SQLite DELETE/FULL + indexes | 4.68 ms [4.66 ms-8.25 ms] (n=5) | 5.45 ms [5.04 ms-8.83 ms] (n=5) | 8.20 ms [7.21 ms-11.58 ms] (n=5) |
| SQLite WAL/FULL | 0.61 ms [0.57 ms-0.73 ms] (n=5) | 0.81 ms [0.71 ms-1.20 ms] (n=5) | 1.00 ms [0.89 ms-1.64 ms] (n=5) |
| SQLite DELETE/synchronous=OFF (no fsync) | 2.92 ms [2.48 ms-3.51 ms] (n=5) | 3.32 ms [2.97 ms-3.77 ms] (n=5) | 5.18 ms [4.65 ms-6.09 ms] (n=5) |
| CSV fsync | 0.64 ms [0.63 ms-0.66 ms] (n=5) | 1.77 ms [1.36 ms-1.91 ms] (n=5) | 1.51 ms [1.13 ms-2.56 ms] (n=5) |
| CSV no fsync | 0.06 ms [0.02 ms-0.06 ms] (n=5) | 0.02 ms [0.02 ms-0.02 ms] (n=5) | 0.07 ms [0.03 ms-0.07 ms] (n=5) |

#### (b3) Append of one 1000-event batch onto an existing N-event store: ms per batch

| system | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE | 15.94 ms [15.42 ms-18.08 ms] (n=5) | 29.41 ms [27.67 ms-35.55 ms] (n=5) | 228.75 ms [196.16 ms-303.85 ms] (n=5) |
| SQLite DELETE/FULL (default) | 31.57 ms [27.05 ms-36.18 ms] (n=5) | 145.84 ms [129.68 ms-183.42 ms] (n=5) | 279.90 ms [241.75 ms-348.81 ms] (n=5) |
| SQLite [256 MiB cache] DELETE/FULL (default) | 31.05 ms [26.49 ms-33.91 ms] (n=5) | 139.90 ms [107.69 ms-185.57 ms] (n=5) | 253.76 ms [237.99 ms-508.86 ms] (n=5) |
| SQLite DELETE/FULL + indexes | 31.35 ms [29.79 ms-33.90 ms] (n=5) | 181.76 ms [149.07 ms-226.34 ms] (n=5) | 683.47 ms [633.78 ms-1001.40 ms] (n=5) |
| SQLite WAL/FULL | 14.65 ms [13.66 ms-45.16 ms] (n=5) | 188.05 ms [162.09 ms-236.36 ms] (n=5) | 320.54 ms [273.49 ms-332.87 ms] (n=5) |
| SQLite DELETE/synchronous=OFF (no fsync) | 38.14 ms [25.42 ms-60.99 ms] (n=5) | 151.73 ms [106.37 ms-400.19 ms] (n=5) | 232.13 ms [194.04 ms-277.69 ms] (n=5) |
| CSV fsync | 4.17 ms [3.70 ms-7.24 ms] (n=5) | 3.91 ms [3.85 ms-5.35 ms] (n=5) | 7.45 ms [7.11 ms-8.67 ms] (n=5) |
| CSV no fsync | 3.86 ms [3.29 ms-6.91 ms] (n=5) | 3.15 ms [3.14 ms-3.22 ms] (n=5) | 6.22 ms [5.19 ms-8.97 ms] (n=5) |

#### (c) Point lookup by event id, per lookup (store already open; row returned unless noted)

| system | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE has_event (hash index, existence only, no row) | 33 ns [33 ns-35 ns] (n=5) | 36 ns [34 ns-72.0 us] (n=5) | 50 ns [50 ns-54 ns] (n=5) |
| KORE Rust linear scan of the in-memory snapshot (early exit) | 48.0 us [44.2 us-63.5 us] (n=5) | 1.16 ms [1.01 ms-1.38 ms] (n=5) | 16.75 ms [14.46 ms-17.63 ms] (n=5) |
| KORE SQL (kore-sql) on a pre-registered context | 13.65 ms [12.58 ms-14.68 ms] (n=5) | 220.55 ms [131.21 ms-433.16 ms] (n=5) | 1.36 s [1.32 s-1.74 s] (n=5) |
| KORE SQL end-to-end run_readonly_sql (re-registers tables each call) | 22.03 ms [21.49 ms-26.41 ms] (n=5) | 289.82 ms [277.78 ms-305.49 ms] (n=5) | 2.78 s [2.69 s-3.12 s] (n=5) |
| SQLite PRIMARY KEY lookup | 80.3 us [46.7 us-109.8 us] (n=5) | 38.9 us [36.7 us-70.9 us] (n=5) | 69.5 us [59.4 us-112.0 us] (n=5) |
| SQLite [256 MiB cache] PRIMARY KEY lookup | 64.2 us [45.6 us-110.4 us] (n=5) | 91.4 us [39.2 us-100.2 us] (n=5) | 64.6 us [60.8 us-115.5 us] (n=5) |
| SQLite + indexes PK lookup | 45.6 us [41.4 us-120.1 us] (n=5) | 36.6 us [36.2 us-37.9 us] (n=5) | 62.9 us [46.2 us-85.7 us] (n=5) |
| CSV full-file scan with early exit (re-opens the file each lookup) | 12.92 ms [11.23 ms-14.54 ms] (n=5) | 266.03 ms [247.07 ms-293.34 ms] (n=5) | 1.27 s [1.11 s-1.48 s] (n=5) |

#### (c2) Open / first-query cost (warm OS file cache)

| system | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE open() (reads + sha256-verifies every segment, builds index) | 30.34 ms [23.45 ms-47.77 ms] (n=5) | 393.51 ms [333.06 ms-415.62 ms] (n=5) | 6.93 s [6.28 s-8.48 s] (n=5) |
| KORE verify() (full re-verify from disk) | 27.57 ms [18.42 ms-34.09 ms] (n=5) | 303.29 ms [239.77 ms-338.32 ms] (n=5) | 4.30 s [3.77 s-4.37 s] (n=5) |
| KORE register_into (snapshot -> SQL tables, one run) | 11.08 ms | 96.93 ms | 1.74 s |
| SQLite connect + first indexed query (one run) | 714.1 us | 1.03 ms | 2.42 ms |

#### (d1) Range aggregate: COUNT by kind for the middle 10% of the time range

| system | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE SQL, pre-registered context | 15.26 ms [11.10 ms-17.42 ms] (n=5) | 115.06 ms [99.58 ms-134.46 ms] (n=5) | 910.05 ms [790.49 ms-1.01 s] (n=5) |
| KORE SQL end-to-end run_readonly_sql | 17.86 ms [16.84 ms-21.55 ms] (n=5) | 203.01 ms [182.49 ms-236.26 ms] (n=5) | 3.13 s [2.51 s-3.50 s] (n=5) |
| KORE hand-written Rust loop over snapshot rows | 144.2 us [126.0 us-431.8 us] (n=5) | 4.63 ms [3.49 ms-6.13 ms] (n=5) | 69.31 ms [60.35 ms-90.66 ms] (n=5) |
| SQLite (no index on occurred_at: scan) | 7.33 ms [3.63 ms-8.81 ms] (n=5) | 106.37 ms [77.86 ms-122.27 ms] (n=5) | 532.41 ms [424.69 ms-645.73 ms] (n=5) |
| SQLite [256 MiB cache] (no index on occurred_at: scan) | 740.6 us [680.2 us-1.15 ms] (n=5) | 14.22 ms [11.30 ms-22.73 ms] (n=5) | 568.63 ms [514.50 ms-682.19 ms] (n=5) |
| SQLite + index on occurred_at | 257.8 us [249.1 us-345.6 us] (n=5) | 11.03 ms [9.18 ms-14.09 ms] (n=5) | 188.79 ms [91.67 ms-217.08 ms] (n=5) |
| CSV scan (csv module) | 23.15 ms [19.72 ms-30.26 ms] (n=5) | 344.99 ms [316.04 ms-430.12 ms] (n=5) | 2.85 s [2.55 s-3.42 s] (n=5) |

#### (d2) Full-table aggregate: COUNT by source

| system | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE SQL, pre-registered context | 5.75 ms [4.78 ms-9.83 ms] (n=5) | 92.66 ms [87.47 ms-108.20 ms] (n=5) | 1.47 s [1.08 s-1.52 s] (n=5) |
| KORE SQL end-to-end run_readonly_sql | 15.00 ms [11.32 ms-19.58 ms] (n=5) | 184.23 ms [177.26 ms-205.66 ms] (n=5) | 2.27 s [2.21 s-3.10 s] (n=5) |
| KORE hand-written Rust loop | 709.5 us [640.9 us-1.07 ms] (n=5) | 13.01 ms [10.50 ms-18.01 ms] (n=5) | 217.59 ms [164.58 ms-246.66 ms] (n=5) |
| SQLite | 5.96 ms [4.69 ms-9.22 ms] (n=5) | 138.00 ms [110.04 ms-168.46 ms] (n=5) | 917.76 ms [799.48 ms-1.03 s] (n=5) |
| SQLite [256 MiB cache] | 2.26 ms [2.15 ms-2.54 ms] (n=5) | 56.15 ms [36.19 ms-66.08 ms] (n=5) | 735.27 ms [708.05 ms-1.22 s] (n=5) |
| SQLite + indexes | 6.09 ms [5.30 ms-10.57 ms] (n=5) | 157.32 ms [125.17 ms-167.81 ms] (n=5) | 1.80 s [1.41 s-2.32 s] (n=5) |
| CSV scan | 25.87 ms [20.27 ms-28.76 ms] (n=5) | 342.96 ms [327.47 ms-433.28 ms] (n=5) | 2.95 s [2.83 s-3.43 s] (n=5) |

#### (e) State as of version v = first version (v = 10,000: v1, 100,000: v1, 1,000,000: v1)

| system | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE native AS OF: open_at (disk read + sha256 of segments 0..v) | 3.16 ms [2.34 ms-5.49 ms] (n=5) | 1.71 ms [1.59 ms-5.35 ms] (n=5) | 4.20 ms [3.89 ms-6.48 ms] (n=5) |
| KORE register_into (snapshot -> SQL tables) | 842.4 us [805.5 us-873.7 us] (n=5) | 915.3 us [438.6 us-1.01 ms] (n=5) | 901.8 us [556.4 us-35.30 ms] (n=5) |
| KORE 2 queries (events by kind + beliefs_current) | 1.70 ms [844.1 us-1.88 ms] (n=5) | 870.9 us [839.7 us-1.92 ms] (n=5) | 1.59 ms [1.36 ms-2.10 ms] (n=5) |
| **KORE total** (open_at + register + queries) | 5.03 ms [4.89 ms-7.18 ms] (n=5) | 3.47 ms [3.20 ms-7.71 ms] (n=5) | 8.38 ms [6.46 ms-40.96 ms] (n=5) |
| SQLite emulated (version column, no index) | 3.65 ms [3.15 ms-6.08 ms] (n=5) | 69.34 ms [66.18 ms-92.46 ms] (n=5) | 554.94 ms [544.41 ms-754.67 ms] (n=5) |
| SQLite emulated [256 MiB cache] (no index) | 1.69 ms [777.9 us-1.95 ms] (n=5) | 9.06 ms [6.29 ms-14.82 ms] (n=5) | 536.92 ms [483.36 ms-558.86 ms] (n=5) |
| SQLite emulated + indexes on version | 317.1 us [306.2 us-341.0 us] (n=5) | 595.4 us [352.2 us-1.37 ms] (n=5) | 479.4 us [471.6 us-506.2 us] (n=5) |
| CSV scan with version filter | 21.07 ms [20.47 ms-29.46 ms] (n=5) | 344.98 ms [284.90 ms-366.42 ms] (n=5) | 2.85 s [2.76 s-3.03 s] (n=5) |

#### (e) State as of version v = mid version (v = 10,000: v5, 100,000: v50, 1,000,000: v500)

| system | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE native AS OF: open_at (disk read + sha256 of segments 0..v) | 10.60 ms [9.62 ms-17.80 ms] (n=5) | 141.07 ms [125.36 ms-154.21 ms] (n=5) | 4.34 s [3.59 s-8.91 s] (n=5) |
| KORE register_into (snapshot -> SQL tables) | 2.58 ms [2.06 ms-5.65 ms] (n=5) | 50.58 ms [40.50 ms-52.28 ms] (n=5) | 561.73 ms [485.88 ms-615.37 ms] (n=5) |
| KORE 2 queries (events by kind + beliefs_current) | 5.42 ms [4.28 ms-6.83 ms] (n=5) | 78.82 ms [70.02 ms-111.92 ms] (n=5) | 1.14 s [1.04 s-1.23 s] (n=5) |
| **KORE total** (open_at + register + queries) | 20.54 ms [17.99 ms-26.73 ms] (n=5) | 270.46 ms [244.82 ms-300.33 ms] (n=5) | 6.19 s [5.19 s-10.57 s] (n=5) |
| SQLite emulated (version column, no index) | 4.58 ms [4.03 ms-4.97 ms] (n=5) | 93.85 ms [71.83 ms-128.64 ms] (n=5) | 974.72 ms [762.78 ms-1.11 s] (n=5) |
| SQLite emulated [256 MiB cache] (no index) | 1.45 ms [1.34 ms-2.19 ms] (n=5) | 26.05 ms [18.94 ms-36.23 ms] (n=5) | 718.59 ms [704.62 ms-770.58 ms] (n=5) |
| SQLite emulated + indexes on version | 1.30 ms [1.27 ms-1.74 ms] (n=5) | 56.31 ms [42.60 ms-70.54 ms] (n=5) | 497.26 ms [478.97 ms-554.00 ms] (n=5) |
| CSV scan with version filter | 23.74 ms [20.60 ms-29.70 ms] (n=5) | 334.68 ms [312.16 ms-405.61 ms] (n=5) | 3.13 s [3.00 s-3.27 s] (n=5) |

#### (e) State as of version v = last version (v = 10,000: v10, 100,000: v100, 1,000,000: v1000)

| system | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE native AS OF: open_at (disk read + sha256 of segments 0..v) | 21.41 ms [19.05 ms-27.09 ms] (n=5) | 347.47 ms [255.85 ms-393.86 ms] (n=5) | 8.15 s [7.03 s-11.95 s] (n=5) |
| KORE register_into (snapshot -> SQL tables) | 6.96 ms [4.88 ms-8.05 ms] (n=5) | 106.35 ms [105.50 ms-132.48 ms] (n=5) | 1.54 s [1.49 s-2.23 s] (n=5) |
| KORE 2 queries (events by kind + beliefs_current) | 14.62 ms [12.54 ms-19.48 ms] (n=5) | 156.45 ms [151.38 ms-169.73 ms] (n=5) | 1.90 s [1.75 s-2.53 s] (n=5) |
| **KORE total** (open_at + register + queries) | 43.41 ms [41.60 ms-49.65 ms] (n=5) | 615.96 ms [518.65 ms-687.73 ms] (n=5) | 12.44 s [10.43 s-15.34 s] (n=5) |
| SQLite emulated (version column, no index) | 5.19 ms [5.05 ms-6.17 ms] (n=5) | 121.99 ms [94.07 ms-139.71 ms] (n=5) | 912.80 ms [857.48 ms-1.06 s] (n=5) |
| SQLite emulated [256 MiB cache] (no index) | 3.24 ms [2.53 ms-4.33 ms] (n=5) | 45.68 ms [40.46 ms-47.67 ms] (n=5) | 935.19 ms [902.82 ms-958.43 ms] (n=5) |
| SQLite emulated + indexes on version | 5.48 ms [5.37 ms-6.41 ms] (n=5) | 154.07 ms [117.81 ms-164.92 ms] (n=5) | 995.98 ms [958.31 ms-1.08 s] (n=5) |
| CSV scan with version filter | 25.45 ms [21.14 ms-46.40 ms] (n=5) | 405.65 ms [343.21 ms-525.78 ms] (n=5) | 3.06 s [2.97 s-3.12 s] (n=5) |

#### (f) Tamper detection: one random bit flipped per trial, uniform over all bytes of the store (SQLite: the .db file; CSV: events.csv; KORE: every file incl. MANIFEST)

| system | N | trials | detected by check | detected only by read error | silent data change | harmless (data identical) |
|---|---|---|---|---|---|---|
| KORE verify()/self_verify | 10,000 | 40 | 40 | 0 (open) | 0 | 0 |
| SQLite PRAGMA integrity_check | 10,000 | 40 | 17 | 1 | 21 | 1 |
| CSV strict parse + 9-column check | 10,000 | 40 | 8 | 0 | 32 | 0 |
| KORE verify()/self_verify | 100,000 | 40 | 40 | 0 (open) | 0 | 0 |
| SQLite PRAGMA integrity_check | 100,000 | 40 | 18 | 4 | 17 | 1 |
| CSV strict parse + 9-column check | 100,000 | 40 | 7 | 0 | 33 | 0 |

KORE by file kind and outcome: N=10,000: {'events:verify_detected': 36, 'revision_causes:verify_detected': 3, 'revisions:verify_detected': 1}; N=100,000: {'events:verify_detected': 39, 'revisions:verify_detected': 1}

#### (g) Idempotent re-ingest of the same N events (batches of 1000)

| system | N | rows before | rows after | new versions/commits | time | size change | note |
|---|---|---|---|---|---|---|---|
| KORE append_events | 10,000 | 10,000 | 10,000 | 0 | 44.01 ms | 0 B | native; no version created |
| SQLite INSERT OR IGNORE (PK) | 10,000 | 10,000 | 10,000 | 0 (rolled back) | 39.00 ms | 0 B | native via PK; id computed by the app |
| CSV naive append | 10,000 | 10,000 | 20,000 | - | 66.51 ms | +2,488,989 B | duplicates: not available natively |
| CSV emulated (scan ids first) | 10,000 | - | - | - | 54.27 ms | 0 B | emulated in app code |
| KORE append_events | 100,000 | 100,000 | 100,000 | 0 | 2.99 s | 0 B | native; no version created |
| SQLite INSERT OR IGNORE (PK) | 100,000 | 100,000 | 100,000 | 0 (rolled back) | 1.40 s | 0 B | native via PK; id computed by the app |
| CSV naive append | 100,000 | 100,000 | 200,000 | - | 661.96 ms | +24,979,946 B | duplicates: not available natively |
| CSV emulated (scan ids first) | 100,000 | - | - | - | 579.98 ms | 0 B | emulated in app code |
| KORE append_events | 1,000,000 | 1,000,000 | 1,000,000 | 0 | 301.52 s | 0 B | native; no version created |
| SQLite INSERT OR IGNORE (PK) | 1,000,000 | 1,000,000 | 1,000,000 | 0 (rolled back) | 25.03 s | 0 B | native via PK; id computed by the app |
| CSV naive append | 1,000,000 | 1,000,000 | 2,000,000 | - | 8.14 s | +250,959,405 B | duplicates: not available natively |
| CSV emulated (scan ids first) | 1,000,000 | - | - | - | 6.22 s | 0 B | emulated in app code |

KORE revision replay (extra observation, same revisions re-submitted in original order): N=10,000: revisions 200 -> 400, versions 10 -> 20; N=100,000: revisions 2000 -> 4000, versions 100 -> 200; N=1,000,000: revisions 20000 -> 40000, versions 1000 -> 2000
