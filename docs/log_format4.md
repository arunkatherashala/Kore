# Digital Life log format 4: KORE v4 compact segments with shared dictionaries

Branch `log-v4` (from `main-Digital-Life-AI` 4feb8daf). `kore-store/` is a path-limited checkout of
`origin/fileformat` bbbefd2c (the owner's KORE v4 container and shared dictionaries, cd2806f4), committed
on its own; no kore-store source was edited. Everything else is in `kore-self/src/versioned/`,
`kore-self/examples/`, `kore-self/benches/`, `kore-self/tests/data/` and docs. Nothing was pushed.

**Read this first.** One shared Windows machine (8 logical CPUs), other jobs running the whole time: mean
total CPU 97-99 % during the runs (section 5). Sizes, query hashes, tamper and idempotence outcomes do not depend
on load; times do. Formats 1, 3 and 4 ran back to back inside every round with the same binary; use the
paired ratios (format 4 divided by format 1 or 3 of the same round).

## 1. Result in one paragraph

Opt-in log format 4 stores the realistic Digital Life dataset in **81.0 B/event at 100k** (format 3: 89.1,
format 1: 126.3) and **80.0 B/event at 1M** (format 3: 89.2, format 1: 126.2), i.e. -10 % against format 3
and -37 % against format 1. The owner's store-level measurement of the same container (docs/kore_size.md,
79.3 B/event at 1M) rewrote format-1 segments outside the log API; the 0.7 B/event difference was not
investigated. At 10k the one 128,960-byte dictionary object is 14 % of the log and format 4 is **larger than
format 3 (91.1 vs 88.2 B/event)**. SQL results, time travel (every AS OF version) and lookups give identical
hashes on formats 1, 3 and 4 in every run; every random bit flip (960 trials at 10k/100k, 320 of them on format 4) was
detected by `verify`; `forget` rebuilds a dictionary that held a forgotten text. The price is write time
(paired against format 3, 100k): bulk build rate 0.44 [0.28-0.85], a 1000-event commit 25-103 ms instead of
12-15 ms, a 1-event commit 12-42 ms instead of 7.5-7.9 ms; and `get_event` on a fresh handle is 2.4-2.9x
slower than format 3 at every size (the v4 reader decodes the whole segment). open, open_at, verify and
re-ingest are within the noise of format 3 (paired medians f4/f3 0.46-1.32 at 10k, 100k and 1M).

## 2. Design

### 2.1 On disk

```text
<root>/<owner>/log/
  MANIFEST                 header "KORE-VLOG-MANIFEST 3", journal (one appended line per commit, fsync)
  events/0000000003.kore   kore-store version-4 file ("enc":4 in the segment reference)
  revisions/...            version-4 files (no dictionary)
  dicts/<sha256>.kdic      kore-store KDIC object: "KDIC" | version | kind | len | content | sha256 (= id)
```

* Segments: `KoreWriter::to_bytes_compact` of the same `DataBlock` format 1 writes (`rows_to_block`), default
  `CompactOptions` (Auto transforms incl. raw 32-byte ids, `TsText`, `Tokens`; zstd level 19 below 256 KiB).
  `events.text` gets `with_dict("text", d)` when the log has a dictionary; kore-store uses it only when smaller.
* MANIFEST entry of the commit that first uses a dictionary:
  `"dicts":[{"id":"<sha256 hex>","file":"dicts/<id>.kdic","bytes":<len>}]`, hashed into the chain as
  `dict\t<id>\t<file>\t<bytes>` lines after the segment lines (entries without `dicts` hash exactly as before,
  so the old lines of an upgraded log keep their hashes). The segment header pins the dictionary id and the
  segment sha256 is in the MANIFEST, so each segment decodes only with the exact object it was written with.
* Header `KORE-VLOG-MANIFEST 3` = format 4. Older binaries know only `... 1` and `... 2` and refuse the log;
  a dictionary listing under an older header is refused by the new parser too.

### 2.2 How dictionaries are built (`DictPolicy`, `versioned/v4log.rs`)

* **Content = whole event texts**, newest that fit into `max_bytes` (default 128 KiB), stored oldest first and
  separated by the byte 0xFF (never part of UTF-8); kind 1 (raw content, used by zstd as history). Whole texts
  make the membership exact, which `forget` needs (2.4).
* **First dictionary**: built by the first commit after which the log holds `train_events` events (default
  1000); training texts are that commit's own texts first, then earlier stored ones. That commit's events
  segment already uses it (in the benchmark: the first batch of 1000).
* **Drift**: after `check_events` (default 100,000) events since the current dictionary was listed, the next
  commit with >= 16 KiB of event text is also encoded with a candidate built from the texts stored before it;
  the candidate becomes a new object (new id, listed by that commit) if the events segment gets >= 5 % smaller,
  otherwise the check is postponed by another `check_events`. The realistic dataset is stationary: at 100k and
  1M no second dictionary was created (one object in every run). The drift path is tested with a log whose
  content changes completely (`v4_drift_lists_a_new_dictionary_and_old_versions_stay_readable`).
* **Immutability**: an object is never modified. Old segments keep referencing the old object; every version
  stays readable. The policy is per handle and not stored: it only affects future writes.
* Only `events.text` uses a dictionary (53 % of the events payload at 100k; the 32-byte ids, 42 %, do not compress, section 4).

### 2.3 Checks

`verify()` (and `open`, which loads and verifies every listed object before streaming the log):

| check | error |
|---|---|
| listed object missing | `MissingSegment { file: "dicts/<id>.kdic" }` |
| object length differs from the listing, sha256 of the object != id, KDIC header invalid, or a different valid object under that name | `ChecksumMismatch { file }` |
| listing: file name not `dicts/<id>.kdic`, id not 64 lowercase hex, id listed twice | `ChainBroken` |
| a segment references an id not listed at or before its version | `SegmentUnreadable` ("not listed at or before version N") |
| file in `dicts/` that no entry lists (crash leftover) | `ExtraSegment` (`recover()` and the next format-4 commit delete it) |
| MANIFEST, segments | as for format 3 (hash chain, sha256 per segment, journal tail rules) |

No new `VerifyError` variant (existing callers keep compiling and matching).

### 2.4 forget and dictionaries

A dictionary that holds the text of a forgotten event is **rebuilt**: a new object from its remaining texts
(exact, because texts are whole and 0xFF-separated), every segment that used the old object is re-encoded with
the new one, the listing entry names the new object, the chain is re-sealed from the earliest affected entry,
and the old object file is deleted, all inside the existing staged, crash-safe forget (`FORGET_PENDING`). If no
text remains the dictionary is dropped and its segments are re-encoded without one. `ForgetReport` counts
`dictionaries_rebuilt` / `dictionaries_removed`. Tested: after the forget no file of the log contains the
forgotten text (byte scan of every file), every other row of every version is unchanged, re-ingest is
refused, a reopened handle keeps writing with the new object; a forget of an event that is not in any
dictionary leaves the dictionaries alone; forgetting every text of a dictionary drops it. Stated limit: a
forgotten text that also occurs inside another, still stored event's text stays there as part of that event;
copies outside the log are not touched (as for every format).

### 2.5 Upgrade and compatibility

* `VersionedLog::upgrade_to_v4()` (from 1, 2 or 3): one commit with `schema_meta.format_version = 4`, header
  switched in the same atomic file replacement, every earlier line byte-identical, every old version hash and
  content hash unchanged (tested on the committed format 1, 2 and 3 fixtures), old segments not rewritten. New
  logs: `open_or_create_with_format(.., LogFormat::V4)`. Never silent: ordinary commits keep the format;
  `upgrade_to_v2`/`upgrade_to_v3` on a format-4 log return false and write nothing (no downgrade).
* A binary built before format 4 (this branch at 15027f54, `examples/vlog_open_check.rs`, own copy of the exe)
  on the committed fixtures:
  ```
  vlog_v3: open: OK format=3 latest=12 verify=Ok(()) events=Ok(99)
  vlog_v4/fixture: open: ERR verify: MANIFEST is corrupt: manifest header missing or wrong
  vlog_v4/fixture_mixed: open: ERR verify: MANIFEST is corrupt: manifest header missing or wrong
  ```
* Formats 1, 2, 3 read and write exactly as before (all earlier tests unchanged and green).
* Fixtures (tracked; the existing `.gitignore` exception covers `kore-self/tests/data/**/*.kore`, `.kdic` is not
  ignored): `tests/data/vlog_v3/` written by the pre-format-4 binary (`examples/gen_vlog_fixture_v4.rs ... 3`),
  `tests/data/vlog_v4/fixture` (format 4 from the start) and `vlog_v4/fixture_mixed` (format 1, then
  `upgrade_to_v4`), each with a dictionary that the fixture's own forget had to rebuild.

### 2.6 Read paths

`open` streams as for formats 2/3 (events segments: projection of `event_id` only, which needs no dictionary;
the other tables folded chunk by chunk); `open_at`, `diff`, `for_each_event` (events-only read) and
`peek_events` read segments with the listed dictionaries; `get_event` verifies and decodes the one segment that
holds the row (whole segment: kore-store v4 has no row-level decode; format 3 decodes one row).

## 3. Tests (all in `kore-self/src/versioned/tests_v4.rs`, 14 tests)

round trip equal to formats 1 and 3 at every version (rows of every table, three SQL results), segments all
v4, dictionary listed once and used only from the listing commit on, streamed reopen; reopened handle keeps
using the dictionary, journal append only, no-op re-submission, no downgrade; no dictionary below
`train_events` and never with `train_events = 0`; drift creates a new object and older versions stay
readable; refusal by older readers (header, format-2 codec, kore-store reader without dictionaries); explicit
upgrade from the format 1, 2 and 3 fixtures (old lines and hashes, dictionary after upgrade, forget on the mixed
log); the committed format-4 fixtures (verify, recorded hashes per version, SQL, get_event, events-only read,
same rows as the format-3 fixture); fixture dirs present; tamper: one bit flip at **every byte** of the
dictionary object (verify and open), truncation/extension/replacement/removal of it, an unlisted object, every
byte of the MANIFEST (header included), every byte of an events segment that uses the dictionary, MANIFEST
truncation at every length; a dictionary used before it is listed (forged, re-sealed chain); crash leftover
object; forget (two tests, 2.4); streamed open = full read, events-only read, peek, and their tamper detection.

Full `cargo test --offline --no-fail-fast` in kore-self: 504 passed, 0 failed (8 test binaries incl. 404 unit tests; examples build), after the last code change.

## 4. Where the bytes are (100k, one run; sizes are deterministic)

| part | format 1 | format 3 | format 4 |
|---|---:|---:|---:|
| events segments | 11,766,548 | 8,509,354 | 7,568,781 |
| revisions + revision_causes | 800,578 | 329,092 | 334,721 |
| MANIFEST | 64,765 | 67,173 | 67,357 |
| dictionary objects | - | - | 128,960 (1) |
| total B/event | 126.3 | 89.1 | 81.0 |

Format 4 events payload by column at 100k: text 3,965,056, event_id 3,200,300 (32 raw bytes per id), tags
206,281, occurred_at 78,142, source 49,084, kind 40,318, the rest < 3 KB; directories/headers make up the
remaining 26 KB. At 1M: events segments 75,837,437 B (format 3: 85,166,503), text payload 39,797,339, event_id 32,003,000, still one dictionary object of 128,960 B (0.13 B/event).

Encode cost of one events segment without disk I/O (`examples/prof_v4_encode.rs`, realistic data, dictionary
from the first 1000 events, two runs at different machine load; ms per segment):

| segment | format-2 codec | v4 no dictionary | v4 + dictionary | bytes (f2 / v4 / v4+dict) |
|---|---:|---:|---:|---|
| 1 event (mean bytes of 500 different events, text mean 297 B) | 0.007-0.008 | 1.0-7.0 | 0.5-4.6 | 429.7 / 474.3 / 400.9 |
| 10 events | 0.12-0.13 | 1.4-12.0 | 3.4-24.6 | 2,468 / 2,499 / 1,242 |
| 1000 events | 7.6-13.7 | 12.3-55.6 | 12.4-63.1 | 82,487 / 81,392 / 73,440 |

The v4 writer searches transforms and stages in parallel (rayon) and codes payloads below 256 KiB at zstd
level 19, so its cost depends strongly on how many CPUs are free; the format-2 codec is single-threaded.

## 5. Measured (benches/run_log_format4.sh, benches/log_format4_tables.py)

Harness: `examples/bench_log_formats.rs` (the existing one, `--format 4` added) through the public log API:
bulk build = `commit_batch` of batches of 1000 (validation, sha256 ids, fsync per commit, MANIFEST), commit =
100 single-event commits and 5 commits of 1000 onto the built log, open = `VersionedLog::open`,
open_at = `open_at(AsOf::Version(mid))` in the harness process (holds a lot of live data, see
format_improvement_r2.md), get_event = 100 random ids on a fresh handle (format 1 opens eagerly, its snapshot
is in memory, so its get_event is a memory lookup and the ratio against it is meaningless), verify =
`verify()`, re-ingest = `append_events` of every batch again (no-op). Dataset: `format_bench_gen_realistic.py`
seed 20261101, the same files as docs/kore_size.md (sha256 rdata_10000 9063a507.., rdata_100000 2118d09a..,
rdata_1000000 f201354a..). Release build, rustc 1.96.0, Windows 11. Raw results are not committed (scratch
directory of this session). Median over rounds of the per-run median, [min-max over rounds].

### N=10,000 (5 rounds)

| metric | format 1 | format 3 | format 4 | f4/f1 paired | f4/f3 paired |
|---|---:|---:|---:|---:|---:|
| size, B/event | 123.9 [123.9-123.9] | 88.2 [88.2-88.2] | 91.1 [91.1-91.1] | 0.74 [0.74-0.74] | 1.03 [1.03-1.03] |
| bulk build, events/s (b1000) | 21,341 [5,227-27,325] | 30,399 [27,632-32,833] | 16,598 [6,680-21,557] | 0.89 [0.24-2.34] | 0.60 [0.21-0.66] |
| commit, 1 event, ms | 13.54 [10.54-32.93] | 7.62 [6.21-8.11] | 14.29 [11.62-20.24] | 0.86 [0.61-1.36] | 1.88 [1.43-3.26] |
| commit, 1000 events, ms | 19.8 [16.0-42.3] | 11.2 [10.4-12.4] | 27.7 [20.0-75.1] | 1.49 [0.63-2.02] | 2.47 [1.62-7.25] |
| open, s | 0.034 [0.025-0.167] | 0.041 [0.004-0.096] | 0.023 [0.008-0.063] | 0.32 [0.16-2.56] | 0.46 [0.18-3.05] |
| open + latest snapshot, s | 0.052 [0.031-0.179] | 0.125 [0.018-0.160] | 0.058 [0.026-0.172] | 0.83 [0.68-5.63] | 0.97 [0.20-2.63] |
| open_at(mid version), s | 0.012 [0.008-0.070] | 0.010 [0.005-0.066] | 0.020 [0.005-0.043] | 0.62 [0.47-2.49] | 1.12 [0.39-2.20] |
| get_event, ms/op (fresh handle) | 0.001 [0.000-0.001] | 2.715 [1.414-3.177] | 6.150 [2.362-7.774] | 12974.70 [4489.75-17131.71] | 2.45 [0.87-3.60] |
| verify, s | 0.041 [0.013-0.110] | 0.028 [0.017-0.071] | 0.025 [0.015-0.074] | 0.67 [0.44-3.15] | 1.06 [0.35-1.58] |
| re-ingest of all events (no-op), s | 0.05 [0.04-0.10] | 0.04 [0.03-0.05] | 0.04 [0.04-0.06] | 0.92 [0.64-1.17] | 0.94 [0.84-2.36] |

format 4 dictionaries: 1 object(s), 128,960 bytes; events payload bytes per column: {'event_id': 320030, 'kind': 4037, 'occurred_at': 7816, 'recorded_at': 230, 'redacted': 130, 'source': 4910, 'tags': 20598, 'text': 383301}
format 1 bytes by table: {'MANIFEST': 6804, 'events': 1157020, 'outcomes': 0, 'predictions': 0, 'revision_causes': 28970, 'revisions': 45423, 'schema_meta': 420}, files 32
format 3 bytes by table: {'MANIFEST': 7052, 'events': 844805, 'outcomes': 0, 'predictions': 0, 'revision_causes': 13400, 'revisions': 16654, 'schema_meta': 76}, files 32
format 4 bytes by table: {'MANIFEST': 7236, 'events': 743652, 'outcomes': 0, 'predictions': 0, 'revision_causes': 13600, 'revisions': 17011, 'schema_meta': 92}, files 33
query hashes (range, full aggregate, lookup, every AS OF version) equal across formats 1/3/4 in every round: MATCH
tamper (random bit flips): format 1: 200/200 detected by verify, 0 silent, 0 harmless; format 3: 200/200 detected by verify, 0 silent, 0 harmless; format 4: 200/200 detected by verify, 0 silent, 0 harmless
re-ingest of every event creates no version and no row in every run: True

### N=100,000 (3 rounds)

| metric | format 1 | format 3 | format 4 | f4/f1 paired | f4/f3 paired |
|---|---:|---:|---:|---:|---:|
| size, B/event | 126.3 [126.3-126.3] | 89.1 [89.1-89.1] | 81.0 [81.0-81.0] | 0.64 [0.64-0.64] | 0.91 [0.91-0.91] |
| bulk build, events/s (b1000) | 28,601 [19,723-28,663] | 29,481 [28,002-32,447] | 14,337 [8,258-23,690] | 0.50 [0.42-0.83] | 0.44 [0.28-0.85] |
| commit, 1 event, ms | 13.68 [11.98-24.00] | 7.74 [7.47-7.91] | 13.81 [12.10-41.84] | 1.15 [0.88-1.74] | 1.78 [1.53-5.60] |
| commit, 1000 events, ms | 21.3 [17.0-28.4] | 13.3 [12.0-15.3] | 77.0 [24.9-102.9] | 3.62 [1.17-4.54] | 6.43 [1.63-7.74] |
| open, s | 0.163 [0.161-0.268] | 0.060 [0.041-0.285] | 0.056 [0.041-0.134] | 0.35 [0.25-0.50] | 0.69 [0.47-1.37] |
| open + latest snapshot, s | 0.167 [0.162-0.232] | 0.209 [0.179-0.629] | 0.274 [0.197-0.351] | 1.51 [1.22-1.64] | 0.94 [0.56-1.53] |
| open_at(mid version), s | 0.062 [0.041-0.073] | 0.092 [0.072-0.233] | 0.090 [0.053-0.289] | 1.45 [1.30-3.99] | 1.24 [0.57-1.26] |
| get_event, ms/op (fresh handle) | 0.001 [0.001-0.001] | 3.339 [1.726-3.899] | 6.724 [4.931-16.679] | 10933.78 [7620.68-25581.99] | 2.86 [2.01-4.28] |
| verify, s | 0.071 [0.065-0.152] | 0.216 [0.090-0.288] | 0.098 [0.091-0.263] | 1.51 [1.29-1.73] | 0.91 [0.45-1.01] |
| re-ingest of all events (no-op), s | 0.46 [0.43-0.55] | 0.51 [0.47-0.75] | 0.62 [0.50-1.13] | 1.44 [0.90-2.47] | 1.32 [0.98-1.51] |

format 4 dictionaries: 1 object(s), 128,960 bytes; events payload bytes per column: {'event_id': 3200300, 'kind': 40318, 'occurred_at': 78142, 'recorded_at': 2300, 'redacted': 1300, 'source': 49084, 'tags': 206281, 'text': 3965056}
format 1 bytes by table: {'MANIFEST': 64765, 'events': 11766548, 'outcomes': 0, 'predictions': 0, 'revision_causes': 289690, 'revisions': 510888, 'schema_meta': 420}, files 302
format 3 bytes by table: {'MANIFEST': 67173, 'events': 8509354, 'outcomes': 0, 'predictions': 0, 'revision_causes': 134000, 'revisions': 195092, 'schema_meta': 76}, files 302
format 4 bytes by table: {'MANIFEST': 67357, 'events': 7568781, 'outcomes': 0, 'predictions': 0, 'revision_causes': 136000, 'revisions': 198721, 'schema_meta': 92}, files 303
query hashes (range, full aggregate, lookup, every AS OF version) equal across formats 1/3/4 in every round: MATCH
tamper (random bit flips): format 1: 120/120 detected by verify, 0 silent, 0 harmless; format 3: 120/120 detected by verify, 0 silent, 0 harmless; format 4: 120/120 detected by verify, 0 silent, 0 harmless
re-ingest of every event creates no version and no row in every run: True


### N=1,000,000 (1 round; round 2 skipped, see below)

| metric | format 1 | format 3 | format 4 | f4/f1 paired | f4/f3 paired |
|---|---:|---:|---:|---:|---:|
| size, B/event | 126.2 [126.2-126.2] | 89.2 [89.2-89.2] | 80.0 [80.0-80.0] | 0.63 [0.63-0.63] | 0.90 [0.90-0.90] |
| bulk build, events/s (b1000) | 27,824 [27,824-27,824] | 5,362 [5,362-5,362] | 4,333 [4,333-4,333] | 0.16 [0.16-0.16] | 0.81 [0.81-0.81] |
| commit, 1 event, ms | 43.49 [43.49-43.49] | 10.06 [10.06-10.06] | 173.26 [173.26-173.26] | 3.98 [3.98-3.98] | 17.21 [17.21-17.21] |
| commit, 1000 events, ms | 158.2 [158.2-158.2] | 106.0 [106.0-106.0] | 404.9 [404.9-404.9] | 2.56 [2.56-2.56] | 3.82 [3.82-3.82] |
| open, s | 4.334 [4.334-4.334] | 3.293 [3.293-3.293] | 3.969 [3.969-3.969] | 0.92 [0.92-0.92] | 1.21 [1.21-1.21] |
| open + latest snapshot, s | 5.373 [5.373-5.373] | 14.497 [14.497-14.497] | 17.194 [17.194-17.194] | 3.20 [3.20-3.20] | 1.19 [1.19-1.19] |
| open_at(mid version), s | 1.867 [1.867-1.867] | 10.090 [10.090-10.090] | 8.853 [8.853-8.853] | 4.74 [4.74-4.74] | 0.88 [0.88-0.88] |
| get_event, ms/op (fresh handle) | 0.001 [0.001-0.001] | 12.776 [12.776-12.776] | 35.481 [35.481-35.481] | 52487.20 [52487.20-52487.20] | 2.78 [2.78-2.78] |
| verify, s | 1.276 [1.276-1.276] | 3.419 [3.419-3.419] | 2.940 [2.940-2.940] | 2.30 [2.30-2.30] | 0.86 [0.86-0.86] |
| re-ingest of all events (no-op), s | 30.64 [30.64-30.64] | 30.46 [30.46-30.46] | 31.63 [31.63-31.63] | 1.03 [1.03-1.03] | 1.04 [1.04-1.04] |

format 4 dictionaries: 1 object(s), 128,960 bytes; events payload bytes per column: {'event_id': 32003000, 'kind': 403128, 'occurred_at': 781242, 'recorded_at': 23000, 'redacted': 13000, 'source': 490714, 'tags': 2066014, 'text': 39797339}
format 1 bytes by table: {'MANIFEST': 645266, 'events': 117510501, 'outcomes': 0, 'predictions': 0, 'revision_causes': 2896936, 'revisions': 5175939, 'schema_meta': 420}, files 3002
format 3 bytes by table: {'MANIFEST': 669274, 'events': 85166503, 'outcomes': 0, 'predictions': 0, 'revision_causes': 1340000, 'revisions': 1980736, 'schema_meta': 76}, files 3002
format 4 bytes by table: {'MANIFEST': 669458, 'events': 75837437, 'outcomes': 0, 'predictions': 0, 'revision_causes': 1360000, 'revisions': 2018014, 'schema_meta': 92}, files 3003
query hashes (range, full aggregate, lookup, every AS OF version) equal across formats 1/3/4 in every round: MATCH
re-ingest of every event creates no version and no row in every run: True


Machine load (typeperf total CPU, sampled before and after every run): 10k + 100k set 144 samples, mean 97 %
(min 40 %, max 100 %); 1M set 21 samples, mean 100 % (min 99 %). At 1M the load changed a lot between the
three runs of the one round (format 1's build ran at 27,824 events/s, format 3's at 5,362, format 4's at
4,333; the 1-event commit took 43 ms on format 1 and 173 ms on format 4): the 1M times, and the 1M paired ratios
against format 1, are dominated by load and are given only for completeness. **1M round 2 was skipped**: during
round 1 the free space on C: (shared with other jobs) fell to 1.7 GB, below the 12 GB rule for 1M runs.

Reading the tables: sizes are exact and the same in every round. "get_event" of format 1 is a memory lookup
(format 1 opens eagerly), so the f4/f1 ratio of that row is meaningless; compare with format 3, which, like
format 4, opens lazily and reads one segment per lookup.

## 6. Not measured

* Power loss / real disk faults (crash handling is tested only by simulated crash points and leftover files).
* Other platforms than Windows; other datasets than the realistic generator at 10k/100k/1M (the uniform
  dataset was not run through the log API).
* Content drift on real data: the drift check never fired on the realistic dataset (stationary); its gain
  threshold (5 %) and interval (100k events) are not tuned.
* A trained zstd dictionary (`SharedDict::train`, kind 2) instead of raw texts; other `max_bytes` than 128 KiB.
* The being itself on a format-4 log (long run, restart timings); only the log API was measured.
* Memory use of open/open_at.
* `migrate_to_v2` from a format-4 log (it reads with the dictionaries; not tested).
* 1M: 1 rounds, 1 build repetition and 3 repetitions per read metric (10k: 5 rounds, 100k: 3 rounds, 2 build
  repetitions, 5 read repetitions); no tamper trials at 1M.

## 7. What kore-store would need (for the owner; nothing was changed there)

* **Row access in the v4 reader** (decode one row of a projection, or a cheap per-row offset for `Str` with
  Plain/Dict transforms): `get_event` on format 4 decodes the whole segment and is 2.4-2.9x slower than format
  3's row-level decode.
* **Cheaper encode for small payloads**: payloads below 256 KiB are always zstd level 19 and every column runs
  a parallel transform/stage search; a configurable small-payload level and an option to skip the search for a
  known column shape would cut the 1-event and 1000-event commit cost (section 4/5). With a dictionary the
  prepared `EncoderDictionary` is built per level for an unknown source size; a size hint might make small
  dictionary-coded payloads cheaper (not verified).
* **A lower per-file overhead**: an 8-column v4 directory costs about 45 B more per 1-event segment than the
  format-2 codec without a dictionary (474 vs 430 B); with the dictionary v4 is still smaller (401 B).
* Nice to have: `v4::info` exposing the dictionary ids as hex, and `DictSet` iteration (kore-self keeps its own
  list).

## 8. Files

`kore-self/src/versioned/v4log.rs` (new: policy, dictionary build/members, encode/decode, listing checks,
`upgrade_to_v4`), `mod.rs` (LogFormat::V4, enc 4, read paths with dictionaries, verify, commit writes and
lists dictionaries, header level 3), `manifest.rs` (`HEADER_V4`, `DictRef`, `dicts` in the hashed entry,
level-aware parse/render), `forget.rs` (dictionary rebuild), `upgrade.rs` (struct fields), `tests_v4.rs`
(new); `examples/bench_log_formats.rs` (`--format 4`, dictionary bytes, v4 column bytes),
`examples/gen_vlog_fixture_v4.rs`, `examples/vlog_open_check.rs`, `examples/prof_v4_encode.rs`,
`benches/run_log_format4.sh`, `benches/log_format4_tables.py`; fixtures `kore-self/tests/data/vlog_v3/`,
`kore-self/tests/data/vlog_v4/`; `kore-self/docs/versioned_log.md` ("Log format 4").
