# KORE versioned log vs SQLite vs plain CSV: measured comparison

Scope: the same event log held in three formats. Everything below is a measurement on one machine under load (see "Run conditions"); nothing is claimed beyond the numbers shown. Where a system lacks a feature natively it is marked **emulated** or **not available**. KORE losses are reported with the same prominence as wins.

**Read this first: the timing numbers were taken while sibling jobs kept the machine at roughly 70-80 % CPU (two `measure_being` processes, `rustc`, Windows security/indexing services). Sizes and the correctness/tamper/idempotence outcomes are not load-dependent; all timings are noisy and should be re-run on an idle machine (one command, below) before anything is quoted externally. Min-max ranges in the tables show the noise.**

## 1. What was compared

| | KORE | SQLite | CSV |
|---|---|---|---|
| implementation | `kore-self/src/versioned/*` through its public Rust API (`VersionedLog::commit_batch`, `open`, `open_at`, `verify`, `run_readonly_sql`, `register_into`), release build; storage = `.kore` segments written by `kore-store` (per column encoding + LZ4 per segment), one segment per table per commit, hash-chained `MANIFEST` | Python 3.14.5 stdlib `sqlite3`, SQLite 3.50.4 | Python stdlib `csv` |
| schema | events / revisions / revision_causes (same columns as KORE) | same three tables; `event_id TEXT PRIMARY KEY`; extra `version` column on every row (emulated versioning); revisions have a `seq` | three files, `version` column |
| versions | one version per commit (native) | emulated: `version` column = batch number | emulated: `version` column |
| durability per commit | temp-file write + `fsync` per segment, rename, `fsync` of MANIFEST.tmp, rename (directory fsync is best effort and is a no-op on Windows) | default `journal_mode=DELETE`, `synchronous=FULL`, one transaction per commit (variants below) | append + `flush` + `os.fsync` per written file per commit |

fsync comparability: KORE cannot run without fsync, so the primary comparison is fsync-per-commit on all three. SQLite WAL/FULL, SQLite synchronous=OFF and CSV-without-fsync rows are extra reference points; they are not equivalent to KORE's guarantee. The KORE commit also does more work per commit than SQLite (validation, sha256 id, sha256 of every segment, MANIFEST rewrite and a hash-chain entry). Python-side overhead (sha256 id per event, `executemany`, interpreter calls) is included in SQLite/CSV timings.

### Native vs emulated

| capability | KORE | SQLite | CSV |
|---|---|---|---|
| state as of version v | native (`open_at(AsOf::Version(v))`, plus `AsOf::Time`) | emulated (append-only tables + `version` column + query discipline) | emulated (scan with filter) |
| tamper detection | native (`verify()` = sha256 per segment + hash-chain check + stray-file check) | `PRAGMA integrity_check` (structural only; no data checksums unless a checksum VFS is loaded; not used here) | not available |
| idempotent re-ingest | native (content-derived event id; a repeat is a no-op and creates no version) | native through the PRIMARY KEY with `INSERT OR IGNORE`, but the application computes the id | not available (duplicates); emulated by scanning ids first |
| row lookup by id | `has_event` is existence only; no id-to-row API (rows only via snapshot scan or SQL) | native (B-tree) | scan |
| SQL | `kore-sql` over an in-memory snapshot (all tables are registered first) | native | not available |
| secondary indexes | none | yes (variant measured) | none |

## 2. Reproduction

Build and run (Git Bash; no new dependencies; temp dirs only):

```
cd C:\Users\skathera\kw\self-bench
set CARGO_TARGET_DIR=C:/Users/skathera/kw/target-selfbench
cargo build --release --offline -p kore-self --example bench_log_formats

# main benchmark (generates data_N.jsonl with seed 20261031 if missing; 1M uses 2 build reps)
sh kore-self/benches/run_format_bench.sh C:/Users/skathera/AppData/Local/Temp/kfb/res "10000 100000 1000000"

# storage-efficiency benchmark (realistic text mix, seed 20261101)
sh kore-self/benches/run_format_bench_size.sh C:/Users/skathera/AppData/Local/Temp/kfb/res3 "10000 100000 1000000"
python -I kore-self/benches/format_bench_size_tables.py <res3 dir>

# SQLite insert-cost diagnostic
python -I kore-self/benches/format_bench_diag_sqlite.py <data_100000.jsonl> <tempdir>
```

Files (all under `kore-self/`): `examples/bench_log_formats.rs` (KORE side; `--size-only` for the storage section), `benches/format_bench_gen.py` (uniform dataset), `benches/format_bench_gen_realistic.py` (mixed text), `benches/format_bench_py.py` (SQLite + CSV side), `benches/format_bench_size_py.py`, `benches/format_bench_tables.py`, `benches/format_bench_size_tables.py`, `benches/run_format_bench*.sh`, `benches/format_bench_diag_sqlite.py`. Raw result JSON and the generated tables are kept in `docs/format_bench_results/`. Python is run with `-I`.

Dataset and method:
- Deterministic generator, seed recorded (20261031 uniform, 20261101 realistic). Both sides read the same JSONL file. SHA-256 of the files: `data_10000` 0ab905ae..1a61, `data_100000` a7c1611c..c797, `data_1000000` a25a716e..004a, `rdata_10000` 9063a507..c11f, `rdata_100000` 2118d09a..8577, `rdata_1000000` f201354a..2288.
- Events: strictly increasing `occurred_at` (30 s step plus jitter), unique text, 8 sources, 5 kinds, 0-2 tags (uniform set) or 0-4 tags (realistic set). One belief revision per 50 events (100 keys), each citing its event; loaded in the same batch.
- Each result carries a `check` (hash of the query result). The table builder compares KORE, SQLite (3 variants) and CSV; every check matched (see the consistency section in the results).
- Warm-up: 1 untimed run, then 5 timed repetitions (3 for a few CSV/gzip scans, stated in the table notes); median and [min-max] reported. 1M used 2 build repetitions only and 5 CSV lookups per repetition (100 at 10k, 20 at 100k).
- Order within a run: build, size, lookup, range, as-of, tamper (N <= 100k only), re-ingest, append (the append tests modify the store, so they come last).
- Not cold-cache: all reads came from the warm OS file cache.

## 3. Run conditions

- Machine: Windows 11 Enterprise 10.0.26100, Intel64 Family 6 Model 189 (8 logical CPUs), 32 GB RAM (about 8.5 GB free at the start), temp dir on C: (about 98 % full, about 12 GB free). rustc 1.96.0, Python 3.14.5, SQLite 3.50.4.
- Other load: `typeperf` showed 72-80 % total CPU before, between and after every measured step (`docs/format_bench_results/load_main.txt`). Top consumers observed: two `measure_being` processes (sibling worktrees), a `rustc`, another `python`, BioIso, svchost, SearchIndexer. During the storage-efficiency runs the same sibling jobs were running but CPU was not sampled.
- Consequence: the 1M main run took about 30 minutes for the Python side alone. Treat every time as an upper-biased, noisy figure. Re-run on an idle machine is pending (the coordinator will say when).

## 4. Main results (uniform synthetic events; each KORE vs SQLite vs CSV pair measured by the same script, same dataset)

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


### Harness checks behind these tables

- Result-hash match across all systems for lookup, range, full aggregate and three as-of versions at every N (first block above).
- Harness bug found and fixed before reporting: the table builder printed millisecond values with a seconds formatter (first draft showed "10.93 s" for 10.93 ms); fixed, outputs re-checked against the raw JSON.
- The SQLite bulk-insert slowdown at 100k looked suspicious (about 5x slower per batch than at 10k, unaffected by `synchronous=OFF` or a 256 MiB cache). Control experiment (`format_bench_diag_sqlite.py`, 100k events, batches of 1000, DELETE/FULL, run under load): with the schema used here (random sha256 PRIMARY KEY) the median was 139.1 ms/batch (first five batches 15.2 ms, last five 192.2 ms); the same data with no unique index (rowid only) 8.8 ms/batch flat; `WITHOUT ROWID` with the same key 274.7 ms/batch (first five 22.2, last five 412.7). So the growth comes from maintaining a unique index on a random key, not from the harness. Content-addressed ids are therefore a hard case for SQLite and are what the idempotence feature requires; a different key design would change the SQLite numbers.
- SQLite variants on page cache: the 256 MiB-cache variant did not change bulk or tail-append cost; it did reduce some scan times at 10k and 100k (noise and warm cache, not explained further).
- KORE revision replay (extra observation, not a speed result): re-submitting the same revisions in their original order created a new revision and version each time (200 to 400 revisions at 10k, 2,000 to 4,000 at 100k, 20,000 to 40,000 at 1M), because a repeated revision with `supersedes = None` supersedes the current head and is therefore not identical to it. Events are idempotent; revisions are only idempotent when identical to the current head. For SQLite the behaviour depends on how the application derives `rev_id`; not tested beyond the events.

### What KORE wins, loses, ties (from the tables above; N=1M figures unless stated; all under load)

| metric | KORE | comparison | verdict |
|---|---|---|---|
| (a) size, uniform data | 161.79 MB | SQLite 357.65 MB, SQLite+indexes 403.62 MB, CSV 256.39 MB | smaller than all three |
| (b1) bulk load, batches of 1000 | 43.6k ev/s (10k), 31.1k (100k), 8.2k (1M, n=2) | SQLite 47.5k, 8.4k, 4.2k; CSV 135.6k, 10.7k (noisy rep: min 6.4k), 103.4k | 10k: ranges overlap with SQLite (tie); 100k and 1M: KORE higher than SQLite; CSV higher than KORE at 10k and 1M. KORE rate falls 5x from 10k to 1M |
| (b2) one event per commit | 11.2 ms, 27.5 ms, 152.9 ms (10k, 100k, 1M) | SQLite default 4.3, 5.3, 7.0 ms; SQLite WAL/FULL 0.6, 0.8, 1.0 ms; CSV fsync 0.6, 1.8, 1.5 ms | KORE loses at every N, and its per-commit cost grows with N |
| (b3) one 1000-event batch appended | 15.9, 29.4, 228.8 ms | SQLite default 31.6, 145.8, 279.9 ms; CSV fsync 4.2, 3.9, 7.5 ms | KORE lower than SQLite at 10k and 100k; ranges overlap at 1M (tie); CSV lower than KORE |
| (c) lookup by id, row returned | Rust scan 11.6-16.8 ms at 1M; kore-sql 1.36 s at 1M | SQLite PK 60-70 us at every N; CSV 1.27 s at 1M | KORE loses on row retrieval; `has_event` (50 ns, existence only, no row) is a different operation |
| (c2) open | 6.9 s at 1M (decodes and checks every segment, builds index) | SQLite connect + first query 2.4 ms | KORE loses |
| (d) range aggregate (10 % of range) | kore-sql 0.91 s at 1M | SQLite scan 0.53 s, indexed 0.19 s, CSV 2.85 s | KORE loses to SQLite, beats CSV; the hand-written Rust loop (69 ms) is a bound, not an API |
| (e) state as of version v | first version: 8.4 ms total; middle 6.2 s; last 12.4 s | SQLite emulated unindexed 0.55 s / 0.97 s / 0.91 s; indexed on version 0.5 ms / 0.50 s / 1.0 s; CSV about 3 s | KORE cheaper only for early versions (it reads segments 0..v) than unindexed SQLite and CSV; loses to SQLite+index at every measured version; cost grows with v. Correctness: native, with a snapshot hash per version |
| (f) tamper detection (40 random single-bit flips) | 40/40 detected at 10k and 100k, 0 silent | SQLite integrity_check detected 17 (+1 read error) at 10k and 18 (+4) at 100k, 21 and 17 silent data changes; CSV detected 8 and 7 by parsing, 32 and 33 silent | KORE wins (SQLite/CSV have no data checksums) |
| (g) idempotent re-ingest | no new version, 0 B, 301.5 s at 1M | SQLite 0 rows added, 0 B, 25.0 s; CSV naive duplicates (2M rows, +251 MB), emulated 6.2 s | KORE and SQLite both correct; KORE 12x slower at 1M under this harness |

Observed causes of KORE's growth with N (from reading `versioned/mod.rs`, not modified): each `commit_batch` clones the whole in-memory id index (`self.index.clone()`) and re-reads and parses the MANIFEST; the cost per commit is therefore O(N) and O(versions). This accounts for the slow single-event commits and the 301 s re-ingest (1,000 no-op batches at 1M). It is a hypothesis consistent with the numbers (per-commit cost 11 to 153 ms across 10k to 1M), not a profiled result.

## 5. Storage efficiency (realistic text mix)

Question: how much event memory fits in how little space, and does it stay queryable.

Data: `format_bench_gen_realistic.py`. Per event: 60 % short note (5-25 words, Zipf-skewed vocabulary), 10 % boilerplate phrase (40 fixed phrases), 22 % medium entry (3-8 sentences from a pool of 400 recurring sentences), 8 % document of 1,000-5,000 bytes (same pool, paragraph breaks); timestamps, tags (0-4 of 40), source, kind, plus one revision per 50 events. The text is synthetic; the pool of recurring sentences makes it more compressible than many real notes, and real text will give different ratios. KORE side: the same log API, batches of 1000, size taken from the directory; SQLite and CSV hold the same three tables with the `version` column. gzip: each of the three CSV files compressed separately with Python `gzip`, levels 6 and 9, sizes summed. Sizes are deterministic; times are under load.

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


### Reading the storage results

- **KORE is not smaller than gzip-compressed CSV.** At 1M events: KORE 229.82 MB, gzip-6 CSV 138.43 MB, gzip-9 CSV 142.78 MB (gzip-9 is slightly larger than gzip-6 on this data; checked directly with `zlib` on the text column, so it is zlib behaviour and not a harness fault). KORE is 1.66x the size of gzip-6 CSV. KORE is smaller than plain CSV (229.82 vs 521.27 MB, 0.44x) and than SQLite (229.82 vs 685.60 MB, 0.34x; 676.61 MB after VACUUM, which reclaimed only 1.3 %).
- Compression ratio against raw text (371.00 MB at 1M): KORE 1.61, gzip-6 2.68, gzip-9 2.60, CSV 0.71, SQLite 0.54 (below 1.0 means the store is larger than the bare text because it also holds ids, timestamps, tags and revisions).
- **What dominates the KORE size** at 1M (229.82 MB total): the `events` segments are 222.07 MB (96.6 %); within them `text` is 117.02 MB (50.9 % of the total), `event_id` 68.32 MB (29.7 %), `occurred_at` 10.03 MB, `tags` 9.34 MB, `source` 6.55 MB, `kind` 6.58 MB, `recorded_at` 4.30 MB. The revisions and revision_causes segments are 7.11 MB (3.1 %). The hash-chain `MANIFEST` is 0.645 MB (0.28 %) and the 3,002 files add no measured overhead beyond their content. So neither the hash chain nor the manifest nor per-segment headers is significant; the column encodings are. From `kore-store/src/writer.rs` (read only): integer columns use delta or RLE, string columns are stored as length-prefixed bytes and then LZ4 is tried per column per 1000-row segment; there is no entropy coder (no Huffman/ANS). Consequences visible in the per-column table: the 64-character hex `event_id` (4 bits of entropy per character) stays about the same size as its raw text (68.3 vs 65.0 MB) whereas gzip nearly halves it (35.5 MB); `text` is 117.0 MB under LZ4 against 66.8 MB under gzip-6 of the whole column; low-cardinality columns (`kind`, `source`) are stored as plain strings (6.6 MB each) against under 1 MB with gzip; `recorded_at` repeats one value per batch yet costs 4.3 MB. These are measured differences; whether other encodings would help KORE was not tested. Compression across 1000-row segments is also bounded by segment size (gzip applied per 1000-row chunk gives numbers close to whole-column gzip for `text` and `event_id`, so segmenting itself is a minor factor).
- **Bytes per event vs N**: KORE 227.6, 229.8, 229.8 B at 10k, 100k, 1M (+1.0 % first to last, i.e. close to linear); SQLite +4.5 %, CSV +4.2 %, gzip-6 +1.8 %.
- **Largest N in a 100 MB budget** (extrapolated from bytes/event at 1M, assuming it stays flat; not tested up to 100 MB): KORE about 435,000 events, gzip-6 CSV about 722,000, gzip-9 CSV about 700,000, plain CSV about 192,000, SQLite about 146,000 (148,000 after VACUUM). The uniform-data runs are not used for this.
- **Queryability without decompressing everything**:
  - SQLite reads only the pages it needs: id lookup 60.6 us and a 10 % range query 1.06 s at 1M (scan, no index).
  - gzip-CSV must stream-decompress from the start: lookup (mid-file id) 4.0 s and range 8.65 s at 1M; full decompress alone 1.74 s (3 files, no parsing).
  - KORE is not queryable without decoding: `open()` reads, sha256-checks and decodes every segment into memory (9.17 s at 1M; the decoded log stays resident, memory was not measured), after which queries run in memory: `has_event` 47 ns (existence only), row lookup via Rust scan 11.6 ms, kore-sql 1.27 s, range via kore-sql 0.98 s. Individual segments are decodable on their own (that is how AS OF reads them) but the API offers no id or time seek into a segment; AS OF version v decodes segments 0..v.
  - So for "queryable while stored small": SQLite answers lookups in microseconds from a file 3x larger than KORE's; KORE needs the whole log decoded in RAM first; gzip-CSV is smallest on disk but needs a full decompression for any question.
- Plain-gzip comparison caveat: gzip of CSV loses the version column semantics, the hash chain and every KORE feature; it is a size reference only.

## 6. What is NOT measured

- Resident memory of the open KORE log or of the Python processes; cold-cache (first read after boot) timings; page-cache effects beyond the single 256 MiB SQLite variant.
- Concurrency, multiple writers, crash and power-loss recovery (KORE has tests for the crash path in `versioned/tests.rs`; not exercised here), Windows Defender influence on fsync-heavy runs.
- KORE `forget` (erasure) cost and semantics, predictions and outcomes tables, `AsOf::Time`, `diff`, `verify_against` (rollback detection by recorded head), forged-chain attacks (from reading the code, not tested: an attacker who rewrites a segment, its sha256 and every later chain entry would pass `verify()` alone; `verify_against` with an externally recorded head hash is the intended guard).
- Full-text search, joins beyond the single-table queries here, other SQLite tuning (page size, `auto_vacuum`, compression extensions, zstd), other CSV tooling (pandas, DuckDB).
- Real, non-synthetic text. Throughput on non-Windows file systems.
- The 100 MB budget was not filled for any format; the figures are extrapolations.
- Tamper-detection used 40 uniformly random bit flips per system and N (seed 7); rates are samples, not probabilities. SQLite's checksum VFS (`cksumvfs`) and SQLite encryption/checksum extensions were not enabled (not in the stdlib build).
- Timings were taken under heavy sibling load; idle-machine re-run pending.

## 7. Other harness problems found

- Table builder unit bug (ms printed as s): fixed.
- Size harness left a SQLite connection open at the end so the work directory could not be removed on Windows (silent `ignore_errors`); fixed (connection closed).
- One CSV 100k bulk-load repetition took 20 s instead of about 0.8 s (range 6.4k-132k events/s); attributed to machine load, left in the table as measured.
- Variant copies of the SQLite store for WAL and synchronous=OFF were taken after the primary append test, so they started 1,500 events larger; the effect is negligible against 10k-1M.
- `run_format_bench.sh` is committed with LF endings; Git on this machine warns it may convert to CRLF on checkout, which would break `sh`.
