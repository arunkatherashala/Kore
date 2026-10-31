# KORE versioned log: format improvement (before / after)

Scope: branch `self-kore-fmt` (from self-b6 @ 5bcf3f68). Changes are confined to `kore-self` (versioned log layer, its own segment codec, manifest flag). Nothing under `kore-store/` or `kore-core/` was touched; the branches `master-kore-engine` and `fileformat` were not touched. Everything below is a measurement on one machine under load; nothing is claimed beyond the numbers shown, and regressions are listed with the same prominence as gains.

**Read this first.** Timings were taken while the machine was busy (typeperf total CPU samples before and after every run are in `docs/format_improvement_results/load_*.txt`; mean total CPU was 51 % (10k rounds), 61 % (100k) and 58 % (1M), with samples from 14 % to 100 %). BEFORE and AFTER binaries were run back to back, interleaved (old, new/format 1, new/format 2, repeat), but speed numbers are still noisy: use the [min-max] ranges. Sizes, the tamper outcome and the idempotence/consistency outcomes do not depend on load. Re-run on an idle machine before quoting externally. 10k and 100k: 3 interleaved rounds each; 1M: **one** round only (so no spread across rounds; the bracket is the min-max of the in-run repetitions).

## 1. What was built

| item | what changed (all inside `kore-self/src/versioned/`) |
|---|---|
| format flag | `SegRef.enc` in the MANIFEST (absent = v1, `2` = v2; part of the hashed entry text, v1 entries hash byte for byte as before) and `schema_meta.format_version` (1 or 2; the newest row is the log's write format). The MANIFEST header line is unchanged. |
| v2 segments (`codec.rs`, `hz.rs`) | same `.kore` container layout but header version 3 (so the old reader refuses instead of misreading). Per column: 64-hex ids as 32 raw bytes, canonical timestamps as zigzag-delta varints, low-cardinality strings as dictionary + bit-packed codes, `tags` as a token dictionary, other strings length-prefixed; ints delta varint, floats raw, bools bits; then an own LZ77+Huffman stage (`hz`, deterministic, no crate) over the column body when it is smaller. Every codec is lossless for arbitrary input (unfit columns fall back to raw), tested incl. nulls, non-hex ids, bad timestamps. |
| explicit upgrade only | `open_or_create*` never changes an existing log. `VersionedLog::upgrade_to_v2()` appends one commit that records `format_version = 2` (old segments and chain untouched, later segments v2, mixed log). `migrate_to_v2(src, dst, owner)` writes a fresh v2 copy of every version and fails unless `content_hash` is equal at every version. New logs default to format 1; `LogFormat::V2` is opt-in. |
| commit speed | no clone of the id index (ids are registered in place with an undo log that rolls back on any error); the conflict check compares the on-disk MANIFEST bytes with the bytes this handle last wrote (no re-parse); the new MANIFEST is the previous bytes plus one line, still written to `MANIFEST.tmp`, fsynced, renamed, directory fsync best effort; per-commit stray cleanup only touches the names the next two versions could use (full scan stays in `recover()`). The hash chain and the atomic write are unchanged. |
| open / lookup | id index is `HashMap<[u8;32], ordinal>` (hex only at the API boundary). Segments are read, checksummed and decoded in parallel (rayon, already a dependency; helps format 1 too). For a log whose events segments are all v2, `open()` still reads and sha256-verifies every segment but decodes only the id column; the row snapshot is decoded on the first `open_latest()` and kept. New `get_event(id)` returns the row reading at most one segment; `event_count()`, `format()`, `try_open_latest()` added. |
| revision replay | format 2 only: a revision whose content digest already exists anywhere in the log is a no-op (rev id returned, no row, no version). Format 1 behaviour is unchanged (idempotent only against the current head). |
| tests | `versioned/tests_v2.rs`, `codec.rs`, `hz.rs` tests; fixture `kore-self/tests/data/vlog_v1/` written by the pre-change binary (`gen_vlog_fixture` example built from the old code, `.gitattributes` there disables line-ending conversion). |

## 2. Compatibility and safety checks (outcomes, not timings)

* Old fixture (13 versions: 5 event batches, revisions with causes, a prediction and outcome, one `forget` commit that resealed the chain) opens, `verify()` passes, every version's manifest hash equals the value recorded by the old binary, every version's `content_hash` and `recorded_at` match, the SQL group-by result and the id list match, forgotten ids are still refused. A new commit on that log stays format 1 (no `enc` appears in the MANIFEST: no silent upgrade). In-place `upgrade_to_v2` keeps all old manifest hashes and contents; `migrate_to_v2` reproduces every version's content hash and is smaller on disk.
* Tamper: unit test flips one bit at EVERY byte offset of every segment of a v2 log (more than 500 flips): each is reported by `verify()` as a checksum mismatch and each makes `open()` fail; edited `enc`, swapped-in v1 encoding of the same rows, truncated/reordered/edited MANIFEST are caught. Harness: 40 random single-bit flips per run at 10k and 100k, all 3 variants: see table (all detected by `verify`, none silent). The 1M run does not include the tamper step (as in the original benchmark).
* Query-hash equality across formats: the harness's lookup/range/full-aggregate/as-of checks produce identical hashes for old, format 1 and format 2 in every run (line under each table). A unit test also checks that identical operations give identical rows in v1 and v2 for every version.
* The forget, time-travel, append-only prefix, idempotent-ingest, crash, tamper, docs_match_registry, banned-words and controller tests all still pass (counts in section 6); the v1 scenarios are repeated on v2 logs in `tests_v2.rs`.

## 3. Results (realistic text mix, `rdata_N.jsonl`, batches of 1000, fsync per commit; same generator and harness as `docs/format_bench.md`)

"before" = binary built from the unchanged code (own target dir); "after, format 1" = new code writing the old encoding; "after, format 2" = new code, new encoding. Cell = median of per-run medians [min-max over all in-run repetitions] (runs = number of interleaved rounds). "open() + decode latest snapshot" does not exist for the old binary: the old `open()` always decoded everything, so compare its `open()` row with the "open() + decode" row of the new columns for like-for-like work, and with the plain `open()` row for the new lazy behaviour.


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



## 4. What dominates the v2 size (events table, 1M, bytes in the segments)

| column | v1 (old, single-column re-encode, 1M) | v2 (actual bytes in the segments, 1M) |
|---|---|---|
| text | 117.02 MB | 48.78 MB |
| event_id | 68.32 MB | 32.01 MB (32 B x 1,000,000 + headers) |
| tags | 9.34 MB | 2.07 MB |
| occurred_at | 10.03 MB | 0.83 MB |
| source | 6.55 MB | 0.44 MB |
| kind | 6.58 MB | 0.43 MB |
| recorded_at | 4.30 MB | 0.034 MB |
| redacted | not listed | 0.038 MB |
| events total (files) | 222.07 MB | 84.73 MB |

(v1 column figures are from `docs/format_bench.md`, each column re-encoded alone with the old writer; the v2 figures are the real block sizes read from the 3,000 events segments of the format 2 log, block headers included, so the two are comparable but not identical in method.)

The id column is now the incompressible 32 bytes per event (random sha256); the text column (370 B average raw) is the largest remaining part. The old per-column numbers (v1, 1M): text 117.02 MB, event_id 68.32 MB, occurred_at 10.03 MB, tags 9.34 MB, source 6.55 MB, kind 6.58 MB, recorded_at 4.30 MB (`docs/format_bench.md`).

## 5. Reading the numbers (including regressions)

**Size (deterministic, not load dependent).** 1M events: 229.82 MB -> 88.72 MB (-61.4 %), events segments 222.07 -> 84.73 MB, revisions 4.30 -> 1.98 MB, revision_causes 2.81 -> 1.34 MB; 100k: 22.98 -> 8.86 MB; 10k: 2.28 -> 0.88 MB. Bytes per event at 1M: 229.8 -> 88.7. The biggest single gain is the id (68.32 -> 32.01 MB) and text via the entropy stage (117.02 -> 48.78 MB); `occurred_at`, `source`, `kind`, `recorded_at` and `tags` together went from 36.8 MB to 3.8 MB. The MANIFEST grows slightly (0.65 -> 0.67 MB, the `enc` field). For reference, `docs/format_bench.md` recorded 138.43 MB for gzip -6 of the CSV of the same dataset; that figure was not re-measured here, so "smaller than gzip" is a comparison against an older measurement. The id cannot shrink below 32 bytes without storing a prefix (not done: collisions would then have to be excluded by a check; the full 32 bytes are kept).

**Single-event commit (item 2).** Per commit on an N-event log, before -> format 1 / format 2: 1M 120.6 ms -> 12.7 / 16.6 ms (an earlier harness run of the old binary gave 142.1 ms; the original benchmark doc 152.9 ms); 100k 21.8 -> 10.9 / 9.0 ms; 10k 61.5 -> 28.8 / 22.9 ms (the 10k rounds were the noisiest: ranges 29-139 ms for the old code and 7-48 ms for the new, see the [min-max] and `format_improvement_results/rdata_10000_*`; an earlier, not archived 10k run of the previous binaries measured 13.3 / 10.1 / 11.3 ms for old / format 1 / format 2, so at 10k the difference is within noise). The cost no longer grows with N the way it did (clone of the whole id index), what remains is mostly the fsyncs (segment, directory, MANIFEST) plus a MANIFEST rewrite that is still O(number of versions) (0.67 MB at 1M). `docs/format_bench.md` measured SQLite at about 5-7 ms per commit under the same kind of load; that was not re-run here, so KORE is still roughly 2x or more slower than that older SQLite figure at 1M. 1000-event batch at 1M: 119.8 -> 18.5 ms (format 1) but 42.3 ms [33.5-129.4] for format 2 (the entropy coder costs CPU per segment). Bulk build, events/s: 1M 11.9k -> 45.2k (format 1) / 29.3k (format 2); 100k 29.5k -> 36.9k / 28.4k; 10k 50.5k -> 51.5k / 34.3k. **So format 2 builds slower than format 1 and, at 10k, slower than the old code (-32 %)**; it is faster than the old code only where the old code's per-commit clone dominated (1M).

**Open and lookup (item 3).** `open()` at 1M: 3.83 s -> 3.00 s (format 1, parallel decode) / 0.37 s (format 2, ids only, -90 %); 100k 310 -> 143 / 36 ms; 10k 26.7 -> 15.3 / 4.1 ms (the original benchmark recorded 6.93 s at 1M under heavier load). Decoding the full latest snapshot afterwards costs what the old open did: format 2 `open() + open_latest()` 2.44 s at 1M vs 1.82 s for format 1 (format 2 is slower to decode because of the entropy stage) vs 3.83 s for the old `open()`. `verify()` at 1M: 2.29 s -> 1.22 s (format 1) / 1.71 s (format 2). **Id lookup:** `get_event` on a format 2 log opened lazily takes 2.00 ms at 1M (one segment read, checksum and decode), vs a 8-9 ms scan of the resident snapshot at 1M. **But at 10k and 100k `get_event` on format 2 (1.7 ms, 2.25 ms) is slower than scanning an already-resident snapshot (12.7 us, 0.77 ms)**; it only wins when the snapshot is not resident, and it never beats the resident format 1 path (0.47 us, since rows are in memory there). `has_event` (existence) got slower: 83 ns -> 84 / 98 ns at 1M, 43 -> 110 / 94 ns at 100k, 24 -> 66 / 64 ns at 10k (hex parse and a 32-byte key).

**As-of (time travel) reads.** `open_at(mid version)` at 1M: 2.21 s -> 2.23 s (format 1) / **4.02 s (format 2, +82 %)**; at 100k 118.8 -> 36.9 / 39.1 ms; at 10k 8.0 -> 5.1 / 7.2 ms. SQL lookups, range and full-aggregate queries run on the same in-memory cells and are unchanged within noise (e.g. 1M range 784 / 817 / 835 ms).

**Re-ingest of the same N events** (a no-op, no version, no file change): 1M 160.0 s -> 6.9 s (format 1) / 18.8 s (format 2); 100k 1.6 -> 0.56 / 0.51 s (median of 3). The old cost was the per-call index clone. Why format 2 took longer than format 1 at 1M in that one run is not explained by these measurements (single round, noisy machine).

**Revision replay (item 4).** Re-submitting the 20,000 benchmark revisions in order: before and format 1: 20,000 -> 40,000 revisions and 1,000 new versions; format 2: 20,000 -> 20,000, 0 new versions (same at 100k: 2,000 -> 2,000; 10k: 200 -> 200).

**Tamper.** 40 random bit flips per run (10k and 100k, 3 variants x 3 rounds): 40/40 detected by `verify()` in every run, none silent, none harmless; plus the exhaustive per-byte test (section 2).

**Consistency.** All query-result hashes match across old, format 1 and format 2 in every run at 10k, 100k and 1M.

## 6. Tests

`cargo test --offline -p kore-self` (debug, final code): **291 unit tests passed, 0 failed** (272 before this branch; +19: 4 codec, 2 hz, 13 in `tests_v2.rs`), and **68 integration tests passed, 0 failed** (cli_bind 4, e2e_advisor 4, e2e_being 4, e2e_chat 3, e2e_ingest 1, e2e_learn 51, e2e_versioned 1); 359 in total. The workspace-wide `cargo test --offline` stops at an unrelated failure in `kore-python --lib` (1 test) before reaching kore-self; that crate was not touched by this branch and the failure was not investigated.


## 7. Risks and behaviour changes

* **Format 2 logs cannot be read by older binaries** (they refuse the version-3 segments cleanly). Upgrade is explicit; there is no downgrade tool.
* **`open_latest()` can now panic** (for all-v2 logs, on first use) if the files were modified after `open()`; `try_open_latest()` returns the error instead. Before, rows were decoded at open so this could not happen. All-v1 logs still decode at open.
* **Revision replay (format 2 only):** identical content anywhere in the history is a no-op. A deliberate revert to an earlier stance with the same `valid_from` and the same cites is therefore ignored (use a later `valid_from` or an explicit `supersedes`). Format 1 keeps the old behaviour.
* **Stray cleanup per commit is narrower** (only the next two version names); other stray files remain until `recover()` and are still reported by `verify()` (`ExtraSegment`).
* **Conflict check is stricter** (byte equality of the MANIFEST instead of last hash + entry count). Any outside edit to the MANIFEST between commits is a `Conflict`.
* **Snapshot sharing:** snapshots share rows via `Arc`; a commit while an old snapshot is still alive copies the in-memory tables once (same order of cost the old per-call clone had).
* `has_event` is slower per call (hex parse + 32-byte hash key); `get_event` on a format 2 log costs one segment read + decode (milliseconds) because rows are not resident.
* The `hz` coder is new code written for this branch and fuzz-tested only lightly (truncation and bit-flip inputs never panic in the unit tests); segment checksums are verified before any decode, so corrupted files are rejected before they reach it.
* Compression ratio depends on the data: this is the synthetic realistic mix whose text reuses a pool of 400 sentences; real notes will compress differently.

## 8. Not measured

* Idle-machine timings; cold-cache reads; Windows Defender/indexing effects; any other OS.
* 1M with more than one interleaved round; tamper at 1M; the uniform (`data_N`) dataset on the final binaries.
* SQLite / CSV were not re-run (the comparison with them in `docs/format_bench.md` still refers to the old KORE numbers).
* Memory use (RSS) of the id index vs the old `HashSet<String>` (expected lower, not measured).
* Multi-process access (still single writer, no cross-process lock).
* Cost of `migrate_to_v2` on large logs (only the 13-version fixture was migrated in tests).

## 9. Recommendations for the kore-store level (not done here)

These belong on a branch off `fileformat`, not on this branch:

* a binary/bytes column type (so 32-byte ids need no custom container), per-column codecs chosen per segment (dictionary, delta, bit-packing), and an entropy stage (the own LZ77+Huffman coder here is a working reference: it took the text column from about 117 MB to the figure in section 4 without any new dependency);
* a column directory with offsets so a reader can fetch one column without scanning the others (here done by parsing block headers);
* a segment-level format version in the container header that old readers reject (used here: header version 3).

## 10. Reproduce

```
set CARGO_TARGET_DIR=C:/Users/skathera/kw/target-selfkorefmt
cargo build --release --offline -p kore-self --example bench_log_formats
# old binary: build the commit 4a62cba3 (or earlier) versioned code into its own target dir
sh kore-self/benches/run_format_compare.sh <outdir> "10000 100000" 3 rdata 3 5
python -I kore-self/benches/format_compare_tables.py <outdir> rdata 10000 100000
```

Raw JSON, load samples and the generated tables: `docs/format_improvement_results/`.
