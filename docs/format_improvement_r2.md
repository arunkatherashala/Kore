# KORE versioned log: round 2 (before / after)

Scope: branch `self-kore-fmt2` (from `self-kore-fmt` @ 6fc666ec, the round-1 result). Changes are confined to `kore-self`. Nothing under `kore-store/` or `kore-core/` was touched (ideas for them are in section 9); the branches `master-kore-engine` and `fileformat` were not touched. BEFORE = the round-1 binary (built from 6fc666ec into its own target dir, run on the same machine, interleaved), AFTER = this branch. Everything below is a measurement on one shared Windows machine; regressions and unexplained numbers are listed with the same prominence as gains.

**Read this first.**

* The machine was shared with other jobs. Mean total CPU (typeperf, sampled before and after every run, `docs/format_improvement_r2_results/*/load.txt`) was 78 % (samples 51-100 %) during the 10k/100k set and 75 % (samples 57-100 %) during the 1M set. BEFORE and AFTER binaries ran back to back inside every round (r1, f2, f3, repeat). The tables give the median over rounds of the per-run median with [min-max]; the *paired ratio* tables divide the AFTER value by the BEFORE value of the SAME round, which cancels slow phases of the machine to first order. 10k and 100k: 5 rounds; 1M: 3 rounds. Sizes, the tamper outcome and the idempotence outcomes do not depend on load.
* An earlier complete run of the same harness, taken with the binary as it was one commit before the last three small changes (decode micro-optimisations, conflict check only when something is written, see 3.2) and with the machine at about 100 % CPU the whole time, is kept in `docs/format_improvement_r2_results/run1_before_last_changes/`. Its direction agrees with the final run everywhere except the 1M re-ingest, where it showed no gain before the last change (section 5.2).
* The harness keeps a lot of live data (all events, the batches, a resident snapshot and an SQL context) while it times `open_at`, so its `open_at` numbers (seconds at 1M) are larger than the same call in a bare process (0.3 s, section 5.4). Compare columns, not the two kinds of numbers.
* Windows only, fsync as implemented by the OS; no other platform was run.

## 1. Targets and outcome

| # | target | outcome (paired, same round, median [min-max] of AFTER / BEFORE) | status |
|---|---|---|---|
| 1 | format 2 build/ingest speed | encoder (hz) time per 100k events 2.68 s -> 1.45 s (-46 %, paired profile, 5 rounds); bulk build events/s x1.41 (10k), x1.23 (100k), x1.35 (1M) with format 2 and x1.51 / x1.33 / x1.42 with format 3 | done; cost: +0.5 % bytes. Still below format 1: bulk build rate of format 2 / format 3 is 0.83 / 0.85 (10k) and 0.87 / 0.95 (100k) of format 1 (paired, section 5.6); 1M against format 1 not measured |
| 2 | re-ingest of identical events at 1M | cause is not format 2 (r1 binary: format 1 and 2 take the same 12-25 s under load); real costs found and cut: re-ingest x0.44 at 1M, x0.66 at 100k | done; the round-1 "18.8 s vs 6.9 s" is not reproduced |
| 3 | commit cost grows with versions | new format 3 (journal MANIFEST): 1 event/commit at 1M x0.41, at 100k x0.67; flat as versions grow (section 5.3) | done, explicit opt-in flag |
| 4 | `open_at` at 1M; `get_event` slower than scan | `get_event` x0.44 at 1M, x0.51 at 100k; `open_at(mid)` x0.76 at 1M (paired) and about -15 % in a bare process | partly: `get_event` clearly better; `open_at` modest |
| 5 | size: better text model | not implemented; measured bounds only (section 6): a stronger per-segment coder could save at most about 11 % of the text column (about 6 % of the log), a shared dictionary up to about 28 % of text | reported, not built |

## 2. What was built

| item | what changed (all inside `kore-self/`) |
|---|---|
| fixtures | `tests/data/vlog_v2_r1/` (`fixture`: pure format 2, `fixture_mixed`: format 1 then `upgrade_to_v2`) written by the round-1 binary BEFORE any change (`examples/gen_vlog_fixture_v2.rs`). Tests open, verify, query and commit on them (`tests_v2.rs`). **Round-1 defect found and fixed:** the root `.gitignore` ignores `*.kore`, so the segment files of `tests/data/vlog_v1/` were never committed in round 1 (a fresh checkout of `self-kore-fmt` fails 5 tests). They are now tracked (exception added to `.gitignore` for `kore-self/tests/data/`). |
| `hz` encoder | same stream grammar and same decoder. Lookahead result reused (identical output, about 10 % faster), 8-byte match compare, search limits (chain 16, stop at length 32, 4 more candidates once a match of 16 exists, no lookahead once a match of 32 exists), 14-bit hash. Streams written by the new encoder differ from round-1 streams for the same input (+0.5 % bytes) but every old stream decodes unchanged. |
| entropy stage | skipped for a column of distinct 32-byte ids (random sha256, the stage never won there, it only cost time); still tried when ids repeat (revision_causes). |
| journal MANIFEST = format 3 | `LogFormat::V3`, `schema_meta.format_version = 3`, MANIFEST header `KORE-VLOG-MANIFEST 2`. A commit appends its entry line and fsyncs; no rewrite, no rename, no re-read. Entry lines and the hash chain are unchanged. Explicit upgrade `VersionedLog::upgrade_to_v3()` (from format 1 or 2): one commit, atomic header switch, old lines byte-identical, old hashes unchanged. Never automatic. Details in `kore-self/docs/versioned_log.md`. |
| conflict check | format 1/2: unchanged full byte comparison, but now only when the commit has something to write (a no-op re-submission reads nothing). Format 3: length + last 512 bytes (O(1)). |
| `get_event` | for format 2/3 logs opened lazily: verified read of the segment, then only the requested row is materialised (`codec::decode_row`), not 1000 rows. |
| decode | timestamp and hex formatting without the `format!` machinery; columns assembled directly into cells. Whole-segment decode (100k events, 5 interleaved rounds): 0.371 s -> 0.310 s median of medians before the last two micro-changes, 0.18-0.21 s -> 0.15-0.16 s after them (separate, quieter runs). |
| duplicate ingest | an event that already exists no longer builds a row (no text clone, no tag join); the id normalisation no longer allocates a Vec of words. |
| tools | `examples/prof_codec.rs`, `prof_reingest.rs`, `prof_commit.rs`, `prof_openat.rs` (phase timers), `benches/run_format_compare_r2.sh`, `run_vs_format1_r2.sh`, `format_compare_tables_r2.py`, `format_paired_r2.py`. |

## 3. Causes found

### 3.1 Encoder speed (target 1)

Timer profile of `encode_segment` for 100k events in 1000-row segments (`prof_codec`, round-1 code): 2.0-2.5 s in total, of which the LZ77 search is 1.63 s (79 %), Huffman and bit output 0.14 s (7 %), everything else 0.29 s. The entropy attempt on the id bodies (3.2 MB of random bytes, never smaller) cost another 0.10-0.25 s. The search cost is not arithmetic: 3.2 MB of random bytes compressed in one call took 2.25 s, the same bytes in 100 calls of 32 KB took 0.13 s (18x per byte), so the time goes into cache misses while walking hash chains over a 370 KB window (each text column body) with a 1.5 MB `prev` table. Hence the fix is fewer probes, not faster probes. Search-limit sweep (100k events, minimum of 3, machine noisy, so read as order of magnitude):

| setting (chain / nice / good / after-good / lazy / hash bits) | encode time | events bytes |
|---|---|---|
| round 1 (32 / inf / inf / 32 / inf / 16) | 1.89 s | 8,466,812 |
| same, lookahead result reused | 10 % less, byte-identical | 8,466,812 |
| 32 / 32 / inf / 32 / inf / 16 | 2.04 s (noise) | 8,414,357 |
| 16 / 32 / 16 / 4 / 32 / 14 (**chosen**) | 1.04 s | 8,509,354 (+0.5 %) |
| 8 / 32 / inf / 32 / 32 / 16 | 0.96 s | 8,639,311 (+2.0 %) |
| 4 / 24 / - / - / no lookahead / 16 | 0.89 s | 9,491,417 (+12 %) |

Paired profile of the final encoder against the round-1 binary (`prof_codec`, 5 interleaved rounds): `encode_segment` 10k events 0.254 s -> 0.142 s, 100k events 2.684 s -> 1.451 s (medians of the round medians; ranges in `format_improvement_r2_results/codec_paired.txt`). Tried and dropped, both measured: encoding the columns of a segment in parallel (same bytes, but 1.70 s -> 2.00 s) and decoding them in parallel (0.40 s -> 1.30 s) were slower on this loaded machine, because segments are already processed in parallel and the text column dominates.

### 3.2 Re-ingest of identical events (target 2)

* Not a format 2 effect. Round-1 binary on the same 1M dataset, back to back, machine at 100 % CPU (`prof_reingest`, `format_improvement_r2_results/reingest_r1_1m.txt`): no-op pass format 1: 14.8, 12.9, 14.9, 25.2 s; format 2: 11.3, 12.8, 18.1, 16.0 s. At 100k both take about 2.3 s. The 18.8 s vs 6.9 s of round 1 was one noisy round; it is not reproducible as a format difference.
* Where the time goes (round-1 code, 1M): validating the events alone (sha256 over the normalised text, a Vec of words plus `join`, timestamp normalisation) takes 7.1-12.0 s of a 12.9-25.2 s pass (7-12 us per event); the rest is index work, cloning the text into a row that is then thrown away, and one full re-read plus byte comparison of the MANIFEST per batch (0.67 MB x 1000 batches).
* Changes: the id is computed without allocating a word vector; a duplicate no longer builds a row; the conflict check runs only if the commit writes (format 3 additionally makes it O(1)). Paired on the same 1M log (`prof_openat`, same files, round-1 binary vs new): 9.27 / 6.36 / 6.11 s -> 4.74 / 5.01 / 3.80 s. In the interleaved harness at 1M: x0.44 (format 2), x0.51 (format 3). The remaining cost is dominated by the sha256 of every event text.

## 4. Compatibility and safety checks (outcomes, not timings)

* Round-1 format 2 logs (`vlog_v2_r1/fixture`, `fixture_mixed`) written by the round-1 binary open, verify, give the recorded manifest hash for every version, the recorded content hash and `recorded_at`, the recorded SQL result and id list; `get_event` works for every id on a lazily opened handle; forgotten ids stay refused; a new commit keeps the log format 2 with a header-1 MANIFEST whose old lines are untouched (no silent upgrade).
* The format 1 fixture (`vlog_v1`) still opens, verifies, queries identically and stays format 1 on commit.
* Explicit upgrade to format 3 from the format 1 fixture and from the round-1 format 2 fixture: ordinary commits first leave the header at 1; after `upgrade_to_v3()` every old entry line is byte-identical, every old version's manifest hash and content hash is unchanged, later commits only append, `forget` works and keeps the journal header, reopening gives the same content hash.
* Tamper: flip one bit at EVERY byte of every segment of a format 2 log (as in round 1) and of a format 3 log; flip one bit at every byte of a format 3 MANIFEST (every entry byte is detected by `verify()`; a flip inside the header either is rejected or turns `...MANIFEST 2` into `...MANIFEST 1`, which changes no entry and is the one harmless outcome); truncation of the format 3 MANIFEST at every length never yields an apparently complete log. Harness: 40 random bit flips per run at 10k and 100k, 3 variants, 5 rounds: 40/40 detected by `verify`, none silent, none harmless, in every run.
* Crash during a journal append: an unterminated last line is ignored by `open()`, reported by `verify()`, removed by `recover()` or by the next commit (tested with the log left in that state); a format 1/2 manifest never tolerates a partial tail.
* Query-hash equality across BEFORE, f2, f3 in every run at 10k, 100k and 1M: asof, full aggregate, lookup and range hashes all MATCH (line under each table). `decode_row` equals `decode_segment` for every row of segments with nulls, non-hex ids, floats, tags and odd strings (unit test).
* All earlier tests (time travel, tamper, idempotence, forget, crash, docs_match_registry, banned words, controller) still pass; counts in section 7.

## 5. Results

"BEFORE" = round-1 binary writing format 2. "AFTER, format 2" = this branch, the same on-disk format. "AFTER, format 3" = this branch, journal MANIFEST. Format 1 is not in the main interleave (see 5.6).

### 5.1 Paired ratios (AFTER / BEFORE of the same round; for build events/s, above 1 is faster)

#### N=10,000 paired ratios (AFTER / BEFORE, same round)

| metric | f2 / r1 | f3 / r1 | rounds |
|---|---|---|---|
| bulk build events/s | 1.41 [1.18-1.48] | 1.51 [1.21-1.68] | 5 |
| open() | 0.50 [0.31-1.72] | 0.49 [0.20-1.14] | 5 |
| open() + decode latest | 0.98 [0.82-1.70] | 0.97 [0.39-1.17] | 5 |
| verify() | 0.95 [0.78-2.68] | 0.88 [0.74-2.18] | 5 |
| get_event(id) | 0.49 [0.47-0.59] | 0.50 [0.36-0.53] | 5 |
| append 1 event/commit | 0.98 [0.95-1.00] | 0.72 [0.64-1.13] | 5 |
| append 1000-event batch | 1.00 [0.87-1.01] | 0.72 [0.65-0.87] | 5 |
| re-ingest same events (s) | 0.66 [0.59-0.73] | 0.63 [0.55-0.70] | 5 |
| total size on disk | 1.00 [1.00-1.00] | 1.00 [1.00-1.00] | 5 |
| as-of mid version, open_at | 1.67 [0.41-2.24] | 1.01 [0.68-1.76] | 5 |

#### N=100,000 paired ratios (AFTER / BEFORE, same round)

| metric | f2 / r1 | f3 / r1 | rounds |
|---|---|---|---|
| bulk build events/s | 1.23 [1.12-1.28] | 1.33 [1.18-1.38] | 5 |
| open() | 1.25 [0.98-1.28] | 1.16 [0.87-1.27] | 5 |
| open() + decode latest | 0.97 [0.89-1.01] | 0.88 [0.76-1.26] | 5 |
| verify() | 0.95 [0.89-1.11] | 1.00 [0.92-1.12] | 5 |
| get_event(id) | 0.51 [0.49-0.61] | 0.53 [0.48-0.57] | 5 |
| append 1 event/commit | 1.02 [0.99-1.16] | 0.67 [0.66-0.70] | 5 |
| append 1000-event batch | 0.95 [0.89-1.03] | 0.69 [0.68-0.77] | 5 |
| re-ingest same events (s) | 0.66 [0.62-0.83] | 0.66 [0.63-0.76] | 5 |
| total size on disk | 1.00 [1.00-1.00] | 1.00 [1.00-1.00] | 5 |
| as-of mid version, open_at | 0.95 [0.72-1.74] | 0.95 [0.75-1.22] | 5 |

#### N=1,000,000 paired ratios (AFTER / BEFORE, same round)

| metric | f2 / r1 | f3 / r1 | rounds |
|---|---|---|---|
| bulk build events/s | 1.35 [1.19-1.44] | 1.42 [1.33-1.56] | 3 |
| open() | 0.98 [0.89-1.05] | 1.20 [0.98-1.31] | 3 |
| open() + decode latest | 0.92 [0.89-1.00] | 1.02 [0.94-1.07] | 3 |
| verify() | 0.93 [0.88-1.61] | 0.93 [0.78-0.95] | 3 |
| get_event(id) | 0.44 [0.42-0.50] | 0.46 [0.45-0.57] | 3 |
| append 1 event/commit | 1.12 [0.93-1.17] | 0.41 [0.37-0.45] | 3 |
| append 1000-event batch | 0.89 [0.60-1.03] | 0.48 [0.27-0.69] | 3 |
| re-ingest same events (s) | 0.44 [0.41-0.49] | 0.51 [0.47-0.52] | 3 |
| total size on disk | 1.00 [1.00-1.00] | 1.00 [1.00-1.00] | 3 |
| as-of mid version, open_at | 0.76 [0.74-1.05] | 0.86 [0.79-0.88] | 3 |

### 5.2 Medians and ranges

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

### 5.3 Commit cost as versions grow (target 3)

`prof_commit`: 3000 single-event commits (fsync per commit), median ms per commit in windows of 100 commits, 3 interleaved rounds, same machine state. MANIFEST 1.06 MB at the end.

| commits | BEFORE (round-1 binary, format 2) | AFTER, format 2 | AFTER, format 3 (journal) |
|---|---|---|---|
| 0..100 | 6.72 ms [6.55-6.92] | 6.54 ms [6.52-6.63] | 5.85 ms [5.54-6.05] |
| 500..600 | 7.30 ms [7.21-7.50] | 7.11 ms [7.04-7.73] | 5.51 ms [5.31-5.59] |
| 1000..1100 | 7.63 ms [7.52-8.06] | 7.83 ms [7.67-8.09] | 5.29 ms [5.08-5.38] |
| 1500..1600 | 7.96 ms [7.92-8.00] | 8.24 ms [8.21-8.83] | 5.01 ms [4.97-5.21] |
| 2000..2100 | 7.74 ms [7.66-8.07] | 8.53 ms [8.46-8.66] | 4.99 ms [4.74-5.19] |
| 2500..2600 | 8.25 ms [8.23-8.63] | 8.60 ms [8.23-8.88] | 5.13 ms [5.10-5.26] |
| all 3000 (median) | 7.90 ms [7.84-7.94] | 8.25 ms [8.08-8.27] | 4.96 ms [4.93-5.04] |

The round-1 and format 2 commit cost grows with the number of versions (the manifest is read for the conflict check and rewritten, fsynced and renamed each time); format 3 stays flat and is lower from the first commit. At 1M events (3002 versions, 0.67 MB manifest) the paired harness gives 1 event/commit x0.41 and a 1000-event batch x0.48 for format 3; format 2 is x1.12 and x0.89, i.e. unchanged within noise, as expected (its write path is unchanged).

### 5.4 Reads (target 4)

* `get_event` on a lazily opened log: x0.44 at 1M, x0.51 at 100k, x0.49 at 10k (both after-columns), from the row-level decode. In absolute terms it is still 1.0-1.2 ms: it reads and sha256-verifies the whole segment (about 150 KB) and decompresses the whole text column (370 KB) because the entropy stage is per column. **It is still slower than a scan of an already resident snapshot at 10k and 100k** (13 us at 10k, 0.8-1.0 ms at 100k) and only wins when the snapshot is not resident; at 1M the scan (9-10 ms) is about 8x slower than `get_event`. Making a random lookup cheaper than that needs smaller independently compressed blocks (a new column layout, not done).
* `open_at(mid)`: paired in the harness x0.76 at 1M (format 2) and x0.86 (format 3); at 100k x0.95; at 10k the range is [0.41-2.24] (ms-scale, noise). In a bare process on the same 1M log (`prof_openat`, 3 interleaved rounds, quieter machine): `open_at(mid)` 0.356 / 0.341 / 0.295 s -> 0.301 / 0.247 / 0.267 s, `open_at(latest)` 0.698 / 0.683 / 0.601 -> 0.680 / 0.563 / 0.590 s, `open()` + `open_latest()` 1.19 / 1.09 / 0.88 -> 0.95 / 0.85 / 0.84 s, `get_event` 11.3 / 6.0 / 2.5 ms -> 1.9 / 1.8 / 1.4 ms (the first-round BEFORE value includes cold files). So decode is 5-15 % faster, which is not the 2x that the round-1 gap to format 1 (2.21 s vs 4.02 s) would need; `hz` decode is about 60 % of the remaining decode time and was not changed.
* `open()` itself (ids only): not improved. Paired ratio at 100k 1.25 [0.98-1.28] for format 2 (slower, unexplained: nothing in the open path changed except parsing the manifest header), 0.98 at 1M; format 3 at 1M 1.20 [0.98-1.31]. With 3-5 rounds and ms-scale values I cannot rule out a small regression; I do not claim a gain.

### 5.5 Size

1M: 88.72 MB -> 89.16 MB (+0.5 %, events 84.73 -> 85.17 MB), the price of the cheaper encoder search (section 3.1). 100k: 8.86 -> 8.91 MB; 10k: 0.88 -> 0.88 MB. The text column is still about 49 MB and the ids 32 MB of the 1M log.

### 5.6 Against format 1 (same new binary, back to back)

#### N=10,000

| metric | format 1 | format 2 | format 3 |
|---|---|---|---|
| bulk build events/s | 55,093 [51,282-55,630] | 45,829 [42,438-48,149] | 47,339 [43,513-49,507] |
| total size MB | 2.28 [2.28-2.28] | 0.88 [0.88-0.88] | 0.88 [0.88-0.88] |
| 1 event/commit ms | 7.56 [7.12-7.69] | 7.54 [7.23-7.64] | 5.35 [5.28-5.43] |
| open() ms | 12.9 [11.2-13.1] | 3.2 [3.0-3.3] | 4.3 [3.1-8.6] |

Build events/s, format 2 / format 1 (same round): median 0.83 [0.82-0.87] over 3 rounds.
Build events/s, format 3 / format 1 (same round): median 0.85 [0.79-0.97] over 3 rounds.

#### N=100,000

| metric | format 1 | format 2 | format 3 |
|---|---|---|---|
| bulk build events/s | 51,968 [49,296-52,387] | 44,292 [43,675-45,427] | 47,777 [47,744-49,558] |
| total size MB | 22.98 [22.98-22.98] | 8.91 [8.91-8.91] | 8.91 [8.91-8.91] |
| 1 event/commit ms | 7.40 [6.98-8.05] | 7.83 [7.40-8.10] | 4.93 [4.74-5.08] |
| open() ms | 111.5 [101.2-143.1] | 26.5 [25.6-28.8] | 28.6 [26.1-50.5] |

Build events/s, format 2 / format 1 (same round): median 0.87 [0.85-0.89] over 3 rounds.
Build events/s, format 3 / format 1 (same round): median 0.95 [0.91-0.97] over 3 rounds.


## 6. Item 5: text model (measured bounds, not implemented)

Reference coders on the text column of the first 20 segments (20,000 events, 7,166,153 raw bytes, 1000-row segments, `hz` as built here: 931,795 bytes):

| coder | bytes | vs hz |
|---|---|---|
| zlib level 9 (32 KB window) | 1,355,900 | +45 % |
| bz2 level 9 | 835,434 | -10.3 % |
| lzma preset 9 extreme | 826,072 | -11.3 % |
| lzma, the 20 segments as one stream (cross-segment redundancy) | 665,944 | -28.5 % |

So even a strong per-segment coder (BWT or LZMA class) could take about 11 % off the text column, i.e. about 5.5 MB of the 89 MB log (about 6 %); only cross-segment redundancy (a shared dictionary of recurring sentences, stored once, referenced by every segment) is large, up to about 28 % of the text. I did not build either: an order-2 context-mixing coder in pure Rust decodes at tens of MB/s against about 380 MB/s for `hz`, which would multiply the CPU of every open and `open_at` on 370 MB of text; a shared dictionary is a new cross-segment object that `forget`, `verify` and the per-segment checksums would all have to know about, and its benefit on real notes (this data reuses a pool of 400 sentences) is unmeasured. Both belong behind a further explicit format flag if wanted; the ids (32 MB at 1M, random sha256) cannot shrink.

## 7. Tests

`cargo test --offline -p kore-self` (debug, final code): **301 unit tests passed, 0 failed** (291 at the end of round 1; +10 in this round: 1 codec `decode_row`, 3 round-1 fixture tests, 6 format 3 tests in `tests_v2.rs`, and the codec/hz tests are unchanged), and **78 integration tests passed, 0 failed** (cli_bind 4, e2e_advisor 4, e2e_being 4, e2e_chat 3, e2e_ingest 1, e2e_learn 61, e2e_versioned 1; `e2e_learn` compiles the versioned test modules too, which is why it grew from 51 to 61); 379 in total. The workspace-wide `cargo test --offline` stops at the unrelated failure in `kore-python --lib` (1 test) before reaching kore-self; that crate was not touched and the failure was not investigated.

New tests: `r1_v2_fixture_opens_verifies_and_queries_identically`, `r1_v2_mixed_fixture_opens_verifies_and_queries_identically`, `r1_v2_fixture_still_accepts_commits_and_keeps_its_shape`, `v3_commits_append_to_the_manifest_and_history_is_identical_to_v2`, `v3_interrupted_append_is_ignored_reported_and_cleaned`, `v3_tamper_every_manifest_byte_and_every_segment_byte_is_detected`, `v3_concurrent_writer_is_a_conflict`, `explicit_upgrade_to_v3_keeps_old_lines_and_hashes_and_never_happens_silently` (runs on both the format 1 and the round-1 format 2 fixture), `v3_get_event_reads_one_row_and_matches_the_snapshot`, codec `decode_row_equals_decode_segment_for_every_row`.


## 8. Risks and behaviour changes

* **Format 3 logs cannot be read by round-1 or older binaries** (header refused cleanly). Upgrade is explicit and one way; there is no downgrade tool. A one-bit change `...MANIFEST 2` -> `...MANIFEST 1` in the header of a format 3 log makes the file parse as a plain manifest (all lines intact, chain verified), so that single flip is not an error.
* **Journal conflict detection is weaker than the round-1 byte comparison**: length plus the last 512 bytes. A concurrent writer appends, which changes both, so it is detected; an in-place edit of the middle of the file that keeps the length is caught by `verify()` and by the checksums, not by the conflict check. Format 1/2 keep the full comparison.
* **No-op commits no longer touch the disk.** A stale handle that re-submits an event that its own index says exists returns "already stored" without checking the file, so after another writer's `forget` of that event it would report it as stored. Commits that write still fail with `Conflict` on a stale handle.
* **Recovery cuts bytes**: `recover()` and the next journal commit truncate an unterminated MANIFEST tail. Those bytes were never a commit (no newline); a committed line is never touched.
* **`verify()` is strict, `open()` lenient** about an unterminated journal tail (same split as stray segment files).
* **New hz streams differ from round-1 streams for the same input** (still decodable by every version since round 1; segments are never re-encoded by an upgrade). Format 2 is still lossless for arbitrary input; the unit tests are unchanged and pass.
* `get_event` has its own decode path for seven column modes; it is checked against `decode_segment` for every row of several segments, not fuzzed.
* Format 2/3 `open_latest()` can still panic if files change after `open` (use `try_open_latest`), as in round 1.
* Build caches: the disk filled up during this work; I deleted only build output of this task's lineage (`target-selfkorefmt/debug` of round 1 and my own first `target-selfkorefmt2`), no sources, no data.

## 9. Not measured

* An idle machine (mean CPU 75-78 % during the final runs, other jobs always present); cold cache reads; antivirus effects; any other OS or filesystem (a journal append may cost differently on other storage).
* Format 1 at 1M against the new code (5.6 covers 10k and 100k only); tamper at 1M; the uniform (`data_N`) dataset; memory use (RSS); multi-process access; `migrate_to_v2` on a large log (only the 13-version fixture, in tests).
* A real text model (section 6 gives bounds, nothing was built); `hz` decode speed work (about 60 % of the remaining decode time).
* Why `open()` at 100k looks 25 % slower for format 2 (5.4).

## 10. Recommendations for the kore-store level (not done here)

* An append-only manifest/journal as a first-class storage primitive (header version flag, newline-terminated records, torn-tail rule): it removes the O(versions) commit cost for every user of the format, not only this log.
* A column directory with offsets and a per-column (or per-64-row block) entropy stage, so a point lookup decompresses a block instead of a whole column; today `get_event` still decompresses 370 KB to return one row.
* A cross-segment shared dictionary object (ids in the manifest, referenced by segments) for text columns; the measurements in section 6 are the upper bound.
* A binary column type for 32-byte ids and a canonical-timestamp type, so they need no custom container.
* Search-limit presets for the entropy stage (the table in 3.1 is a ready trade-off curve).

## 11. Reproduce

```
set CARGO_TARGET_DIR=C:/Users/skathera/kw/target-selfkorefmt2
cargo build --release --offline -p kore-self --example bench_log_formats --example prof_codec --example prof_commit --example prof_openat
# BEFORE: build commit 6fc666ec (a detached worktree) into its own target dir and copy the examples out
sh kore-self/benches/run_format_compare_r2.sh <outdir> "10000 100000" 5 rdata 2 5
sh kore-self/benches/run_format_compare_r2.sh <outdir_big> "1000000" 3 rdata 1 3
sh kore-self/benches/run_vs_format1_r2.sh <outdir_f1> "10000 100000" 3
python -I kore-self/benches/format_compare_tables_r2.py <outdir> rdata 10000 100000
python -I kore-self/benches/format_paired_r2.py <outdir> rdata 10000 100000
```

Tables (`*.md`): `docs/format_improvement_r2_results/`. Raw JSON, load samples (`load.txt`) and profile outputs (`*.txt`) are kept locally by the harness and are not committed; re-run the scripts in section 11 to regenerate them.
