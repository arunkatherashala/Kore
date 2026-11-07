# KORE file format: smaller on Digital Life events (opt-in version 4), one-run comparison

Branch `fileformat-improve-2` (from `origin/fileformat` @ 6e2e6a7b). Everything here is additive and opt-in:
the default writer (`KoreWriter::to_bytes`), the version-3 writer (`to_bytes_with`) and every existing
reader path produce and accept exactly what they did at 6e2e6a7b. Nothing was pushed.

## 1. What changed

The earlier opt-in version-3 container (per-column transform + stage, `HexFixed` raw ids, `Dict`, `Delta`,
`hz` entropy stage; docs/format_codecs.md) was already in 6e2e6a7b and is reused. Version 4 adds what the
Digital Life measurements (kore-self docs/format_improvement*.md) said was still missing:

| idea (source) | in version 4 |
|---|---|
| raw binary ids instead of hex text | reused `HexFixed` (32 raw bytes per sha256 id), chosen by `Auto` |
| dictionary for low-cardinality strings | reused `Dict` + stage; new `Tokens` transform for comma lists (`tags`): token dictionary + varint codes |
| delta for timestamps | new `TsText` transform: canonical `YYYY-MM-DDTHH:MM:SSZ` strings (the log stores timestamps as text) become zigzag varint deltas of epoch seconds; checked by formatting back, anything else is refused (explicit) or not chosen (`Auto`) |
| shared dictionary across segments (round 2: up to 28.5 % of text, not built there) | new **shared dictionary object** (`SharedDict`, file `<sha256>.kdic`): stored once, referenced by id from every file that uses it; stage 4 = zstd with that dictionary |
| stronger entropy coding of text | zstd level 9 for large payloads (19 below 256 KiB) instead of level 3; `hz` still a candidate for payloads below 128 KiB |

Files (all new unless marked):

| file | content |
|---|---|
| `kore-store/src/v4.rs` | version-4 container: writer (`Auto` = cheap transform pre-selection by zstd-1 score, then stage search), reader, `info()` |
| `kore-store/src/dict.rs` | `SharedDict` (KDIC object, sha256 id, raw-content or zstd-trained), `DictSet` (lookup by id, `load_dir`) |
| `kore-store/src/codec.rs` (edited, additive) | `Transform::TsText` (id 5), `Transform::Tokens` (id 6), `from_id_v4`; `from_id` (used by v3) still accepts 0-4 only; the v3 writer refuses the new transforms |
| `reader.rs`, `writer.rs`, `lib.rs`, `versioned.rs` (edited, additive) | `KoreReader::from_bytes_with_dicts`, `from_bytes_columns_with_dicts`, early `is_v4` branch; `KoreWriter::to_bytes_compact`; `versioned::read_at_with_dicts`, `read_at_columns_with_dicts`, `read_latest_with_dicts`; `list_versions` reads a v4 entry's row count from its header |
| `kore-store/tests/compact.rs` + 2 fixtures | 13 tests (section 4) |
| `kore-store/examples/bench_kore_size.rs`, `write_v4_sample.rs` | benchmark store side, old-reader check input |
| `scripts/kore_size/{run.sh, dl_log_side.rs, py_side.py, tables.py}` | the single harness (section 3) |

No existing line of a reader/writer/segment format was removed (the diff against 6e2e6a7b removes 4 lines, all
replaced by a superset: a `matches!` list, a `fn` made `pub(crate)`, the `list_versions` closure, and the lockfile
version line). One dev-dependency was added (`serde_json`, workspace, for the benchmark example). `kore-core` is untouched.

### Byte layout

```text
version-4 file
"KORE" | version:u16 = 4 | num_cols:u32 | num_rows:u64
num_dicts:u16, per dictionary: dict_id:[32] (sha256 of the KDIC object) | content_len:u32
per column: name_len:u16 | name | dtype:u8 | transform:u8 | stage:u8 | dict:u8 (0xFF none) | payload_len:u64 | payload_crc32:u32
dir_crc32:u32  (CRC-32 of every byte before it)
payloads       (stage 4 = varint(raw_len) + one zstd frame with content checksum, coded with the dictionary)

dictionary object (<id>.kdic)
"KDIC" | version:u16 = 1 | kind:u8 (1 raw content, 2 zstd-trained) | len:u32 | content | sha256:[32] of all bytes before it (= the id)
```

### Features kept

* **Columnar reads / projection**: unchanged model; a projection decodes only the requested payloads and needs only
  the dictionaries of those columns (`kind` and `event_id` read with the plain `from_bytes_columns`, `text` needs
  the dictionary). Tested.
* **Versioning / time travel**: a dictionary is immutable and content-addressed; a better dictionary is a new
  object with a new id, older files keep their reference. `versioned::read_at_with_dicts` reads any version of
  a KVLG log with v4 entries; a version needs only its own dictionaries (tested with two dictionaries across three
  versions). In the Digital Life log the dictionary would be one more file listed in the MANIFEST (not wired
  into kore-self here, see Not measured).
* **Checksums / tamper detection**: header, dictionary ids and directory under `dir_crc32`, each payload under its
  CRC, the dictionary object under sha256 and pinned by id in every file that uses it; a different valid
  dictionary is refused; truncation and trailing bytes are errors. Tested at every byte of a v4 file and of a
  KDIC object (3 masks each), plus recomputed-CRC garbage payloads (no panics).
* **Compatibility**: v1/v2/v3 files read as before; the default and v3 writers reproduce fixtures written by the
  unmodified 6e2e6a7b code byte for byte; readers that predate v4 stop at the version field. Verified with a
  real old binary: a detached worktree at 6e2e6a7b built into its own target dir with a scratch `old_read`
  example (not committed) on the files of `examples/write_v4_sample.rs`:
  ```
  sample_v2.kore: OK rows=200 cols=3
  sample_v4.kore: ERR invalid argument: unsupported version 4
  sample_v4_dict.kore: ERR invalid argument: unsupported version 4
  ```
  A v4 file that uses a dictionary, read without it, fails with
  `column 'text' needs shared dictionary <id> (not supplied; use KoreReader::from_bytes_with_dicts)`.

## 2. Result in one paragraph

On the realistic Digital Life dataset at 1M events, version 4 shrinks the store from **126.2 to 79.3 B/event (-37 %)**
at the default zstd level 9, and to **76.3 B/event (-40 %)** at level 19. That is smaller than the Digital Life log
format 3 (89.2), Parquet zstd at default level (96.4) and at level 22 (80.4), CSV+bz2-9 (80.4), CSV+zstd-19 (83.0),
DuckDB (635) and SQLite (675), and **0.5 B/event larger than CSV+xz-9 (78.8)** at level 9 (level 19 beats it by
2.5 B/event). On the uniform dataset KORE v4 is 60.7 B/event at 1M: better than Parquet level 22 (61.5), xz-9
(63.9), log format 3 (68.1), Parquet default (71.4), but **CSV+bz2-9 is smaller (58.4)**. At 10k events the one
shared dictionary (131 KB) is a large share of the total and **KORE v4 (90.1 B/event realistic) loses to log
format 3 (88.2), bz2 (80.4), xz (82.0), zstd-19 (83.5) and Parquet-22 (84.8)**. The price is write time:
2.0-2.2x the default writer on realistic data, 5-8x on uniform data (paired); level 19 costs about 10x more
again. Reads stay close to the default writer: paired ratios v4/default between 0.80 and 1.18 for full read, projection and lookup at every size and dataset, except one lookup ratio of 1.64 at 10k realistic (millisecond-scale, range 1.03-1.78).

## 3. The benchmark (one harness, one machine, same files)

`scripts/kore_size/run.sh` runs, for every dataset and size, back to back on the same JSONL file:

1. `dl_log_side --format 1` (Digital Life `VersionedLog`, batches of 1000, fixed clock): log format 1, whose
   segments are default kore-store files. The directory is kept.
2. `dl_log_side --format 3`: Digital Life log format 3 (own column codec with hz + journal MANIFEST).
3. `bench_kore_size`: decodes every segment of the format 1 log and writes it with the default writer (checked:
   byte-identical to the file on disk, 3001/3001 at 1M) and with `to_bytes_compact`; events.`text` may use one
   shared dictionary = the last 128 KiB of the raw text of the first events segment (built inside the timed
   write). The v4 copy keeps the table directories, file names and the MANIFEST (copied as is; in a real v4
   log the hash values would differ, the lengths would not) and adds `dicts/<id>.kdic`. Its bytes on disk
   are the KORE v4 size. It also dumps the logical rows for step 4.
4. `py_side.py` (Python 3.12.14, pyarrow 25.0.0, duckdb 1.5.6, SQLite 3.53.4, read-only conda env): the same
   three-plus-one tables (events, revisions, revision_causes, schema_meta) and columns, plus a `version`
   column (the commit number; how a format without native versions carries it). Ids are 64-char hex text in
   every format except the explicit "ids as binary(32)" Parquet row (KORE v4 stores them as 32 raw bytes).

Datasets: realistic = `format_bench_gen_realistic.py` seed 20261101 (sha256 rdata_10000 9063a507.., rdata_100000
2118d09a.., rdata_1000000 f201354a..), uniform = `format_bench_gen.py` seed 20261031 (data_10000 0ab905ae..,
data_100000 a7c1611c.., data_1000000 a25a716e..), the same files and hashes as docs/format_bench.md. Both
generators were run from the Digital Life checkout.

Builds: Digital Life side = a scratch worktree at main-Digital-Life-AI ecf5f695 (`C:\Users\skathera\kw\ff2-dl`,
not committed, detached) with its `kore-store/` replaced by this branch's, release build; store side = this
branch, release. rustc 1.96.0, Windows 11, 8 logical CPUs.

What each time means:

* write: KORE rows = encode + write every segment file (no fsync), from decoded blocks in memory, 3 reps (2 at
  1M), paired default/v4 in each rep. Log-API rows = `commit_batch` of the whole dataset (validation, sha256 ids,
  fsync per commit, MANIFEST); not comparable with the store rows. Python rows = one run from in-memory rows
  (CSV writer, `executemany`) or an in-memory Arrow table (Parquet/ORC/IPC/DuckDB) to files; compressed CSV =
  CSV write + compression of the CSV files.
* full read: every table, every column into memory (KORE: read + decode every segment; log API: `open` (sha256
  of every segment) + latest snapshot; Python: csv rows / `fetchall` / Arrow tables).
* projection: the `kind` column of events, counted per value. KORE and Python counts include their language's
  own counting; the Python rows include `to_pylist`, so they are pessimistic for the Arrow-based formats.
* point lookup: text of an event by event_id, 20 ids (5 for plain-CSV scans at 1M). KORE store: scan of the
  `event_id` column of each segment (projection) until found, then the `text` column of that segment (there is
  no index in the store). Log API: `get_event` with the log's in-memory id index (built by `open`, not timed).
  SQLite: PRIMARY KEY B-tree. DuckDB: `WHERE event_id = ?`, no index. Parquet: `filters=` (row-group statistics
  cannot prune random ids). ORC/IPC: read event_id+text, `pc.index`. gzip/bz2/xz/zstd CSV: not available
  (would need full decompression).

**Machine load.** The machine was shared with other jobs the whole time: `typeperf` total CPU at the start,
middle and end of every dataset/size (54 samples) averaged 61 % (min 25 %, max 100 %). Sizes are deterministic
and load-independent. Times are single-machine, under load, mostly 2-3 repetitions: use the paired ratios for
KORE v4 vs default, and treat cross-format times as rough (different languages and APIs).

## 4. Single-run tables (generated by scripts/kore_size/tables.py)

Sizes are bytes on disk of all files of the store (MB = 10^6 bytes). "B/event" divides by the number of events (the files also hold the 2 % revisions and their causes). Times: median [min-max] over the repetitions; one value = one run.

Raw results (not committed): `C:SERSSKATHERAAPPDATAocaltempkff2es`

### Size, realistic (format_bench_gen_realistic.py) dataset

| format | N=10,000 | N=100,000 | N=1,000,000 |
|---|---:|---:|---:|
| **KORE v4 compact (opt-in, this branch)** | 0.90 MB, 90.1 B/event | 8.03 MB, 80.3 B/event | 79.31 MB, 79.3 B/event |
| KORE v4 compact, zstd level 19 (opt-in) | 0.87 MB, 87.1 B/event | 7.73 MB, 77.3 B/event | 76.29 MB, 76.3 B/event |
| KORE default writer (6e2e6a7b) = DL log format 1 | 1.24 MB, 123.9 B/event | 12.63 MB, 126.3 B/event | 126.22 MB, 126.2 B/event |
| Digital Life log format 3 (own codec) | 0.88 MB, 88.2 B/event | 8.91 MB, 89.1 B/event | 89.16 MB, 89.2 B/event |
| Digital Life log format 1 via log API | 1.24 MB, 123.9 B/event | 12.63 MB, 126.3 B/event | 126.22 MB, 126.2 B/event |
| CSV (3 files) | 5.00 MB, 500.3 B/event | 51.81 MB, 518.1 B/event | 521.24 MB, 521.2 B/event |
| CSV + gzip -6 | 1.39 MB, 138.9 B/event | 14.11 MB, 141.1 B/event | 141.43 MB, 141.4 B/event |
| CSV + gzip -9 | 1.38 MB, 137.5 B/event | 13.96 MB, 139.6 B/event | 139.97 MB, 140.0 B/event |
| CSV + bz2 -9 | 0.80 MB, 80.4 B/event | 8.03 MB, 80.3 B/event | 80.43 MB, 80.4 B/event |
| CSV + xz -9 (lzma preset 9) | 0.82 MB, 82.0 B/event | 8.00 MB, 80.0 B/event | 78.82 MB, 78.8 B/event |
| CSV + zstd -3 (pyarrow codec) | 0.99 MB, 99.0 B/event | 9.88 MB, 98.8 B/event | 98.92 MB, 98.9 B/event |
| CSV + zstd -19 (pyarrow codec) | 0.84 MB, 83.5 B/event | 8.32 MB, 83.2 B/event | 83.04 MB, 83.0 B/event |
| SQLite | 6.54 MB, 654.1 B/event | 67.98 MB, 679.8 B/event | 684.08 MB, 684.1 B/event |
| SQLite after VACUUM | 6.46 MB, 645.9 B/event | 67.09 MB, 670.9 B/event | 675.24 MB, 675.2 B/event |
| Parquet zstd (default level) | 1.00 MB, 100.1 B/event | 9.75 MB, 97.5 B/event | 96.41 MB, 96.4 B/event |
| Parquet zstd level 22 (max) | 0.85 MB, 84.8 B/event | 8.19 MB, 81.9 B/event | 80.44 MB, 80.4 B/event |
| Parquet snappy | 2.28 MB, 228.0 B/event | 22.84 MB, 228.4 B/event | 227.92 MB, 227.9 B/event |
| Parquet zstd (default), ids as binary(32) | 0.98 MB, 97.8 B/event | 9.56 MB, 95.6 B/event | 94.29 MB, 94.3 B/event |
| ORC zstd | 1.32 MB, 132.4 B/event | 13.49 MB, 134.9 B/event | 134.19 MB, 134.2 B/event |
| Arrow IPC / Feather v2 uncompressed | 5.24 MB, 524.5 B/event | 54.09 MB, 540.9 B/event | 542.98 MB, 543.0 B/event |
| Arrow IPC / Feather v2 lz4 | 2.20 MB, 220.5 B/event | 22.15 MB, 221.5 B/event | 221.46 MB, 221.5 B/event |
| Arrow IPC / Feather v2 zstd | 1.08 MB, 107.9 B/event | 10.92 MB, 109.2 B/event | 108.72 MB, 108.7 B/event |
| DuckDB .duckdb (before explicit CHECKPOINT) | 5.05 MB, 504.9 B/event | 63.41 MB, 634.1 B/event | 638.80 MB, 638.8 B/event |
| DuckDB .duckdb after CHECKPOINT | 7.35 MB, 735.2 B/event | 63.71 MB, 637.1 B/event | 635.19 MB, 635.2 B/event |

#### Times, realistic (format_bench_gen_realistic.py), N=10,000 (median [min-max]; machine under load)

| format | write | full read | projection (events.kind) | point lookup per op | how |
|---|---|---|---|---|---|
| **KORE v4 compact (opt-in, this branch)** | 128.5 ms [127.6 ms-137.1 ms] | 25.7 ms [12.5 ms-50.4 ms] | 2.6 ms [2.4 ms-2.6 ms] | 5.6 ms [3.2 ms-6.1 ms] | segments + MANIFEST + 1 dictionary object; lookup = scan of event_id columns (no index) |
| KORE v4 compact, zstd level 19 (opt-in) | 1.92 s | not available | not available | not available | same layout and decoder as the row above; one timed write; reads not repeated |
| KORE default writer (6e2e6a7b) = DL log format 1 | 63.6 ms [60.2 ms-115.0 ms] | 38.8 ms [11.8 ms-41.6 ms] | 3.0 ms [2.6 ms-4.4 ms] | 3.2 ms [3.1 ms-3.7 ms] | same files as the format 1 log (31/31 segments rewritten byte-identical) |
| Digital Life log format 3 (own codec) | 227.3 ms | 9.9 ms [9.2 ms-10.1 ms] | not available | 972 us [865 us-1.1 ms] | write = build via log API (fsync per commit, sha256, manifest); full read = open + latest snapshot; lookup = get_event (in-memory id index); projection not exposed by the API |
| Digital Life log format 1 via log API | 202.1 ms | 10.4 ms [8.7 ms-12.9 ms] | not available | 1 us [1 us-1 us] | same bytes as the KORE default row; times through the log API |
| CSV (3 files) | 80.5 ms | 36.5 ms [35.0 ms-48.6 ms] | 32.6 ms [30.3 ms-37.4 ms] | 24.7 ms [23.5 ms-28.2 ms] | lookup = scan of events.csv until found (20 ids) |
| CSV + gzip -6 | 366.1 ms | 79.1 ms [60.9 ms-79.5 ms] | 87.5 ms [86.2 ms-90.9 ms] | not available | write = CSV 0.081s + compress 0.286s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + gzip -9 | 427.3 ms | 74.9 ms [72.1 ms-94.4 ms] | 75.3 ms [62.3 ms-76.9 ms] | not available | write = CSV 0.081s + compress 0.347s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + bz2 -9 | 604.8 ms | 482.9 ms [456.2 ms-550.5 ms] | 379.4 ms [370.7 ms-381.7 ms] | not available | write = CSV 0.081s + compress 0.524s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + xz -9 (lzma preset 9) | 3.35 s | 133.3 ms [130.1 ms-140.5 ms] | 135.1 ms [133.4 ms-144.5 ms] | not available | write = CSV 0.081s + compress 3.270s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + zstd -3 (pyarrow codec) | 171.1 ms | 72.8 ms [64.6 ms-90.6 ms] | 76.6 ms [71.9 ms-83.4 ms] | not available | write = CSV 0.081s + compress 0.091s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + zstd -19 (pyarrow codec) | 4.38 s | 63.9 ms [58.4 ms-91.5 ms] | 46.7 ms [46.7 ms-48.1 ms] | not available | write = CSV 0.081s + compress 4.299s; reads decompress the whole file; point lookup not available without full decompression |
| SQLite | 284.3 ms | 45.7 ms [42.8 ms-45.9 ms] | 20.6 ms [19.7 ms-25.6 ms] | 131 us [38 us-139 us] | synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup) |
| SQLite after VACUUM | 465.3 ms | 45.7 ms [42.8 ms-45.9 ms] | 20.6 ms [19.7 ms-25.6 ms] | 131 us [38 us-139 us] | synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup); reads measured on the vacuumed file; VACUUM 0.181s |
| Parquet zstd (default level) | 86.0 ms | 19.4 ms [12.6 ms-26.7 ms] | 10.4 ms [9.8 ms-14.0 ms] | 9.0 ms [8.0 ms-9.3 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet zstd level 22 (max) | 3.58 s | 14.0 ms [12.3 ms-20.2 ms] | 26.4 ms [9.8 ms-27.2 ms] | 9.4 ms [9.0 ms-11.6 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet snappy | 80.0 ms | 19.8 ms [9.4 ms-23.4 ms] | 12.9 ms [8.9 ms-15.6 ms] | 9.0 ms [7.0 ms-9.0 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet zstd (default), ids as binary(32) | 48.8 ms | 10.6 ms [9.3 ms-23.4 ms] | 17.0 ms [11.2 ms-20.4 ms] | 9.1 ms [8.7 ms-9.3 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column); 64-hex id columns stored as fixed_size_binary(32), like KORE v4 HexFixed |
| ORC zstd | 99.9 ms | 21.2 ms [18.6 ms-22.2 ms] | 12.7 ms [7.9 ms-16.1 ms] | 10.3 ms [9.7 ms-10.7 ms] | lookup = read event_id+text columns, pc.index (no predicate pushdown in pyarrow.orc) |
| Arrow IPC / Feather v2 uncompressed | 37.5 ms | 4.5 ms [4.4 ms-4.8 ms] | 11.5 ms [9.6 ms-13.8 ms] | 4.7 ms [4.1 ms-5.9 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| Arrow IPC / Feather v2 lz4 | 28.3 ms | 5.7 ms [5.2 ms-7.4 ms] | 8.5 ms [7.6 ms-10.1 ms] | 3.4 ms [3.4 ms-3.5 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| Arrow IPC / Feather v2 zstd | 22.2 ms | 8.2 ms [7.1 ms-11.1 ms] | 8.7 ms [7.5 ms-10.5 ms] | 3.9 ms [3.8 ms-4.2 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| DuckDB .duckdb (before explicit CHECKPOINT) | 77.2 ms | 17.8 ms [15.2 ms-22.8 ms] | 9.8 ms [9.5 ms-10.1 ms] | 1.7 ms [1.7 ms-1.9 ms] | CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan); size = db file + WAL right after loading; reads measured on the checkpointed file |
| DuckDB .duckdb after CHECKPOINT | 172.0 ms | 17.8 ms [15.2 ms-22.8 ms] | 9.8 ms [9.5 ms-10.1 ms] | 1.7 ms [1.7 ms-1.9 ms] | CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan); CHECKPOINT 0.095s |

#### Times, realistic (format_bench_gen_realistic.py), N=100,000 (median [min-max]; machine under load)

| format | write | full read | projection (events.kind) | point lookup per op | how |
|---|---|---|---|---|---|
| **KORE v4 compact (opt-in, this branch)** | 2.28 s [1.38 s-3.43 s] | 534.1 ms [519.9 ms-651.9 ms] | 74.8 ms [61.0 ms-77.7 ms] | 31.1 ms [29.7 ms-34.0 ms] | segments + MANIFEST + 1 dictionary object; lookup = scan of event_id columns (no index) |
| KORE v4 compact, zstd level 19 (opt-in) | 31.60 s | not available | not available | not available | same layout and decoder as the row above; one timed write; reads not repeated |
| KORE default writer (6e2e6a7b) = DL log format 1 | 1.63 s [634.7 ms-2.15 s] | 603.9 ms [595.3 ms-838.8 ms] | 85.0 ms [74.2 ms-95.0 ms] | 38.6 ms [37.3 ms-43.8 ms] | same files as the format 1 log (301/301 segments rewritten byte-identical) |
| Digital Life log format 3 (own codec) | 2.33 s | 143.7 ms [140.4 ms-147.0 ms] | not available | 1.6 ms [1.5 ms-1.7 ms] | write = build via log API (fsync per commit, sha256, manifest); full read = open + latest snapshot; lookup = get_event (in-memory id index); projection not exposed by the API |
| Digital Life log format 1 via log API | 4.48 s | 221.8 ms [199.6 ms-222.7 ms] | not available | 3 us [3 us-4 us] | same bytes as the KORE default row; times through the log API |
| CSV (3 files) | 1.46 s | 1.86 s [1.42 s-2.15 s] | 1.49 s [1.21 s-1.74 s] | 514.1 ms [455.3 ms-521.9 ms] | lookup = scan of events.csv until found (20 ids) |
| CSV + gzip -6 | 6.48 s | 1.46 s [1.28 s-2.07 s] | 1.17 s [1.05 s-1.19 s] | not available | write = CSV 1.460s + compress 5.015s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + gzip -9 | 6.35 s | 836.7 ms [735.4 ms-1.21 s] | 556.2 ms [555.2 ms-616.9 ms] | not available | write = CSV 1.460s + compress 4.895s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + bz2 -9 | 5.88 s | 2.58 s [1.94 s-5.00 s] | 2.48 s [1.94 s-3.13 s] | not available | write = CSV 1.460s + compress 4.422s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + xz -9 (lzma preset 9) | 38.74 s | 1.56 s [1.22 s-1.79 s] | 2.35 s [1.67 s-3.01 s] | not available | write = CSV 1.460s + compress 37.285s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + zstd -3 (pyarrow codec) | 2.02 s | 829.2 ms [818.6 ms-902.8 ms] | 644.3 ms [593.4 ms-691.8 ms] | not available | write = CSV 1.460s + compress 0.565s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + zstd -19 (pyarrow codec) | 45.35 s | 861.5 ms [743.9 ms-921.7 ms] | 486.3 ms [391.2 ms-630.6 ms] | not available | write = CSV 1.460s + compress 43.891s; reads decompress the whole file; point lookup not available without full decompression |
| SQLite | 7.40 s | 385.2 ms [349.8 ms-409.0 ms] | 114.3 ms [111.9 ms-128.4 ms] | 38 us [37 us-41 us] | synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup) |
| SQLite after VACUUM | 9.42 s | 385.2 ms [349.8 ms-409.0 ms] | 114.3 ms [111.9 ms-128.4 ms] | 38 us [37 us-41 us] | synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup); reads measured on the vacuumed file; VACUUM 2.013s |
| Parquet zstd (default level) | 155.6 ms | 43.7 ms [40.9 ms-45.4 ms] | 75.0 ms [69.8 ms-75.7 ms] | 35.8 ms [33.6 ms-39.8 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet zstd level 22 (max) | 21.61 s | 49.9 ms [41.9 ms-54.0 ms] | 73.9 ms [61.0 ms-74.5 ms] | 34.9 ms [33.6 ms-43.5 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet snappy | 188.0 ms | 57.3 ms [41.9 ms-64.3 ms] | 87.8 ms [76.7 ms-97.6 ms] | 40.8 ms [35.7 ms-41.3 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet zstd (default), ids as binary(32) | 158.3 ms | 81.4 ms [68.8 ms-112.9 ms] | 104.4 ms [104.0 ms-142.8 ms] | 35.4 ms [30.7 ms-49.5 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column); 64-hex id columns stored as fixed_size_binary(32), like KORE v4 HexFixed |
| ORC zstd | 316.9 ms | 68.6 ms [62.5 ms-72.3 ms] | 71.4 ms [59.7 ms-76.9 ms] | 50.3 ms [42.7 ms-50.4 ms] | lookup = read event_id+text columns, pc.index (no predicate pushdown in pyarrow.orc) |
| Arrow IPC / Feather v2 uncompressed | 32.2 ms | 5.7 ms [4.9 ms-5.8 ms] | 91.4 ms [80.4 ms-97.9 ms] | 23.9 ms [23.7 ms-28.9 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| Arrow IPC / Feather v2 lz4 | 125.9 ms | 30.1 ms [29.7 ms-30.7 ms] | 122.4 ms [118.4 ms-167.2 ms] | 32.6 ms [31.2 ms-35.4 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| Arrow IPC / Feather v2 zstd | 123.6 ms | 36.3 ms [35.7 ms-39.3 ms] | 118.4 ms [70.0 ms-126.8 ms] | 32.5 ms [30.0 ms-41.7 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| DuckDB .duckdb (before explicit CHECKPOINT) | 565.4 ms | 96.0 ms [91.5 ms-101.7 ms] | 113.4 ms [97.4 ms-117.0 ms] | 7.5 ms [6.6 ms-7.9 ms] | CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan); size = db file + WAL right after loading; reads measured on the checkpointed file |
| DuckDB .duckdb after CHECKPOINT | 595.4 ms | 96.0 ms [91.5 ms-101.7 ms] | 113.4 ms [97.4 ms-117.0 ms] | 7.5 ms [6.6 ms-7.9 ms] | CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan); CHECKPOINT 0.030s |

#### Times, realistic (format_bench_gen_realistic.py), N=1,000,000 (median [min-max]; machine under load)

| format | write | full read | projection (events.kind) | point lookup per op | how |
|---|---|---|---|---|---|
| **KORE v4 compact (opt-in, this branch)** | 14.13 s [14.02 s-14.25 s] | 1.30 s [1.24 s-1.35 s] | 319.7 ms [280.4 ms-359.1 ms] | 158.3 ms [148.4 ms-168.3 ms] | segments + MANIFEST + 1 dictionary object; lookup = scan of event_id columns (no index) |
| KORE v4 compact, zstd level 19 (opt-in) | 139.36 s | not available | not available | not available | same layout and decoder as the row above; one timed write; reads not repeated |
| KORE default writer (6e2e6a7b) = DL log format 1 | 6.45 s [6.24 s-6.66 s] | 1.20 s [1.11 s-1.30 s] | 305.6 ms [287.4 ms-323.8 ms] | 155.8 ms [149.4 ms-162.2 ms] | same files as the format 1 log (3001/3001 segments rewritten byte-identical) |
| Digital Life log format 3 (own codec) | 19.94 s | 908.6 ms [869.8 ms-947.3 ms] | not available | 1.0 ms [946 us-1.1 ms] | write = build via log API (fsync per commit, sha256, manifest); full read = open + latest snapshot; lookup = get_event (in-memory id index); projection not exposed by the API |
| Digital Life log format 1 via log API | 22.53 s | 810.4 ms [715.8 ms-905.0 ms] | not available | 4 us [3 us-5 us] | same bytes as the KORE default row; times through the log API |
| CSV (3 files) | 4.97 s | 4.44 s [4.43 s-4.46 s] | 3.42 s [3.20 s-3.64 s] | 1.53 s [1.50 s-1.55 s] | lookup = scan of events.csv until found (5 ids) |
| CSV + gzip -6 | 22.70 s | 6.29 s [5.99 s-6.59 s] | 4.85 s [4.84 s-4.87 s] | not available | write = CSV 4.966s + compress 17.737s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + gzip -9 | 28.40 s | 6.31 s [5.94 s-6.69 s] | 4.57 s [4.41 s-4.73 s] | not available | write = CSV 4.966s + compress 23.434s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + bz2 -9 | 41.07 s | 17.78 s [17.71 s-17.85 s] | 15.77 s [15.47 s-16.06 s] | not available | write = CSV 4.966s + compress 36.104s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + xz -9 (lzma preset 9) | 347.09 s | 10.15 s [9.26 s-11.05 s] | 7.88 s [7.79 s-7.98 s] | not available | write = CSV 4.966s + compress 342.123s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + zstd -3 (pyarrow codec) | 8.25 s | 4.94 s [4.82 s-5.06 s] | 3.26 s [3.25 s-3.28 s] | not available | write = CSV 4.966s + compress 3.287s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + zstd -19 (pyarrow codec) | 350.90 s | 5.63 s [5.40 s-5.86 s] | 3.59 s [3.48 s-3.70 s] | not available | write = CSV 4.966s + compress 345.935s; reads decompress the whole file; point lookup not available without full decompression |
| SQLite | 91.63 s | 3.47 s [3.34 s-3.59 s] | 1.07 s [1.01 s-1.14 s] | 64 us [64 us-65 us] | synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup) |
| SQLite after VACUUM | 105.18 s | 3.47 s [3.34 s-3.59 s] | 1.07 s [1.01 s-1.14 s] | 64 us [64 us-65 us] | synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup); reads measured on the vacuumed file; VACUUM 13.551s |
| Parquet zstd (default level) | 1.36 s | 394.6 ms [314.3 ms-474.8 ms] | 763.9 ms [753.3 ms-774.5 ms] | 363.0 ms [356.7 ms-369.3 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet zstd level 22 (max) | 189.55 s | 433.7 ms [387.3 ms-480.2 ms] | 699.5 ms [610.0 ms-789.1 ms] | 362.7 ms [360.8 ms-364.7 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet snappy | 1.20 s | 368.3 ms [317.2 ms-419.5 ms] | 694.5 ms [684.4 ms-704.5 ms] | 330.7 ms [321.1 ms-340.2 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet zstd (default), ids as binary(32) | 1.18 s | 370.3 ms [321.4 ms-419.2 ms] | 727.8 ms [667.2 ms-788.3 ms] | 336.4 ms [326.3 ms-346.4 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column); 64-hex id columns stored as fixed_size_binary(32), like KORE v4 HexFixed |
| ORC zstd | 1.77 s | 670.8 ms [615.3 ms-726.2 ms] | 724.3 ms [701.4 ms-747.3 ms] | 513.1 ms [504.3 ms-521.9 ms] | lookup = read event_id+text columns, pc.index (no predicate pushdown in pyarrow.orc) |
| Arrow IPC / Feather v2 uncompressed | 431.2 ms | 47.6 ms [47.2 ms-48.0 ms] | 648.5 ms [606.3 ms-690.7 ms] | 286.2 ms [279.0 ms-293.4 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| Arrow IPC / Feather v2 lz4 | 680.9 ms | 180.7 ms [174.1 ms-187.2 ms] | 633.8 ms [630.2 ms-637.4 ms] | 206.9 ms [203.1 ms-210.8 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| Arrow IPC / Feather v2 zstd | 537.5 ms | 261.0 ms [243.8 ms-278.2 ms] | 702.4 ms [669.6 ms-735.1 ms] | 243.1 ms [241.5 ms-244.6 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| DuckDB .duckdb (before explicit CHECKPOINT) | 2.39 s | 776.7 ms [729.8 ms-823.6 ms] | 806.6 ms [734.0 ms-879.1 ms] | 18.2 ms [17.2 ms-19.2 ms] | CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan); size = db file + WAL right after loading; reads measured on the checkpointed file |
| DuckDB .duckdb after CHECKPOINT | 2.48 s | 776.7 ms [729.8 ms-823.6 ms] | 806.6 ms [734.0 ms-879.1 ms] | 18.2 ms [17.2 ms-19.2 ms] | CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan); CHECKPOINT 0.086s |

#### KORE v4 / KORE default, paired ratios (realistic (format_bench_gen_realistic.py); same repetition, below 1 = v4 faster)

| N | write | full read | projection | lookup |
|---|---|---|---|---|
| 10,000 | 2.01 [1.19-2.13] | 1.06 [0.66-1.21] | 0.86 [0.59-0.93] | 1.64 [1.03-1.78] |
| 100,000 | 2.10 [1.06-2.18] | 0.86 [0.78-0.90] | 0.82 [0.72-1.01] | 0.83 [0.68-0.88] |
| 1,000,000 | 2.20 [2.11-2.29] | 1.08 [1.04-1.12] | 1.04 [0.98-1.11] | 1.02 [0.99-1.04] |

#### KORE v4 per column bytes (realistic (format_bench_gen_realistic.py))

| column | N=10,000 | N=100,000 | N=1,000,000 | codec (transform/stage) at largest N |
|---|---:|---:|---:|---|
| events.event_id | 320,030 | 3,200,300 | 32,003,000 | {'t4/s0': 1000} |
| events.kind | 4,037 | 40,318 | 403,128 | {'t3/s3': 1000} |
| events.occurred_at | 7,816 | 78,142 | 781,242 | {'t5/s3': 1000} |
| events.recorded_at | 230 | 2,300 | 23,000 | {'t5/s1': 1000} |
| events.redacted | 130 | 1,300 | 13,000 | {'t0/s1': 1000} |
| events.source | 4,910 | 49,084 | 490,714 | {'t3/s3': 1000} |
| events.tags | 20,598 | 206,281 | 2,066,014 | {'t6/s2': 844, 't6/s3': 156} |
| events.text | 371,744 | 3,893,887 | 39,119,395 | {'t0/s4': 1000} |
| revision_causes.event_id | 6,430 | 64,300 | 643,000 | {'t4/s0': 1000} |
| revision_causes.rev_id | 6,430 | 64,300 | 643,000 | {'t4/s0': 1000} |
| revisions.author | 240 | 2,400 | 24,000 | {'t3/s1': 1000} |
| revisions.confidence | 1,263 | 12,380 | 124,370 | {'t0/s1': 204, 't0/s3': 796} |
| revisions.entity_type | 200 | 2,000 | 20,000 | {'t3/s1': 1000} |
| revisions.key | 630 | 6,300 | 63,000 | {'t0/s3': 1000} |
| revisions.rationale | 767 | 7,697 | 78,590 | {'t0/s3': 1000} |
| revisions.recorded_at | 180 | 1,800 | 18,000 | {'t5/s1': 1000} |
| revisions.rev_id | 6,430 | 64,300 | 643,000 | {'t4/s0': 1000} |
| revisions.stance | 766 | 7,739 | 77,250 | {'t3/s0': 24, 't3/s1': 783, 't3/s3': 193} |
| revisions.supersedes | 3,235 | 61,105 | 639,805 | {'t0/s0': 5, 't4/s0': 995} |
| revisions.valid_from | 440 | 4,400 | 43,999 | {'t5/s0': 999, 't5/s1': 1} |
| schema_meta.created_at | 6 | 6 | 6 | {'t5/s0': 1} |
| schema_meta.format_version | 2 | 2 | 2 | {'t1/s0': 1} |
| dictionary object | 131,115 | 131,115 | 131,115 | raw text of the first events segment (tail) |

### Size, uniform (format_bench_gen.py) dataset

| format | N=10,000 | N=100,000 | N=1,000,000 |
|---|---:|---:|---:|
| **KORE v4 compact (opt-in, this branch)** | 0.69 MB, 69.4 B/event | 6.15 MB, 61.5 B/event | 60.73 MB, 60.7 B/event |
| KORE v4 compact, zstd level 19 (opt-in) | 0.69 MB, 69.4 B/event | 6.15 MB, 61.5 B/event | 60.73 MB, 60.7 B/event |
| KORE default writer (6e2e6a7b) = DL log format 1 | 0.99 MB, 98.5 B/event | 9.89 MB, 98.9 B/event | 98.96 MB, 99.0 B/event |
| Digital Life log format 3 (own codec) | 0.68 MB, 67.8 B/event | 6.80 MB, 68.0 B/event | 68.06 MB, 68.1 B/event |
| Digital Life log format 1 via log API | 0.99 MB, 98.5 B/event | 9.89 MB, 98.9 B/event | 98.96 MB, 99.0 B/event |
| CSV (3 files) | 2.52 MB, 251.8 B/event | 25.41 MB, 254.1 B/event | 256.36 MB, 256.4 B/event |
| CSV + gzip -6 | 0.83 MB, 83.3 B/event | 8.34 MB, 83.4 B/event | 83.49 MB, 83.5 B/event |
| CSV + gzip -9 | 0.83 MB, 82.6 B/event | 8.26 MB, 82.6 B/event | 82.73 MB, 82.7 B/event |
| CSV + bz2 -9 | 0.58 MB, 58.1 B/event | 5.83 MB, 58.3 B/event | 58.43 MB, 58.4 B/event |
| CSV + xz -9 (lzma preset 9) | 0.66 MB, 65.9 B/event | 6.45 MB, 64.5 B/event | 63.86 MB, 63.9 B/event |
| CSV + zstd -3 (pyarrow codec) | 0.80 MB, 80.2 B/event | 8.02 MB, 80.2 B/event | 80.39 MB, 80.4 B/event |
| CSV + zstd -19 (pyarrow codec) | 0.72 MB, 71.6 B/event | 7.00 MB, 70.0 B/event | 69.58 MB, 69.6 B/event |
| SQLite | 3.52 MB, 351.8 B/event | 35.43 MB, 354.3 B/event | 356.12 MB, 356.1 B/event |
| SQLite after VACUUM | 3.44 MB, 344.1 B/event | 34.50 MB, 345.0 B/event | 347.27 MB, 347.3 B/event |
| Parquet zstd (default level) | 0.77 MB, 76.8 B/event | 7.20 MB, 72.0 B/event | 71.37 MB, 71.4 B/event |
| Parquet zstd level 22 (max) | 0.67 MB, 67.4 B/event | 6.26 MB, 62.6 B/event | 61.48 MB, 61.5 B/event |
| Parquet snappy | 1.23 MB, 122.9 B/event | 11.83 MB, 118.3 B/event | 117.10 MB, 117.1 B/event |
| Parquet zstd (default), ids as binary(32) | 0.74 MB, 74.3 B/event | 7.03 MB, 70.3 B/event | 69.23 MB, 69.2 B/event |
| ORC zstd | 0.73 MB, 72.6 B/event | 7.27 MB, 72.7 B/event | 72.11 MB, 72.1 B/event |
| Arrow IPC / Feather v2 uncompressed | 2.77 MB, 276.6 B/event | 27.76 MB, 277.6 B/event | 278.79 MB, 278.8 B/event |
| Arrow IPC / Feather v2 lz4 | 1.59 MB, 158.8 B/event | 15.75 MB, 157.5 B/event | 157.46 MB, 157.5 B/event |
| Arrow IPC / Feather v2 zstd | 0.87 MB, 87.0 B/event | 8.82 MB, 88.2 B/event | 87.97 MB, 88.0 B/event |
| DuckDB .duckdb (before explicit CHECKPOINT) | 2.57 MB, 257.1 B/event | 8.10 MB, 81.0 B/event | 80.87 MB, 80.9 B/event |
| DuckDB .duckdb after CHECKPOINT | 1.85 MB, 184.7 B/event | 8.14 MB, 81.4 B/event | 77.87 MB, 77.9 B/event |

#### Times, uniform (format_bench_gen.py), N=10,000 (median [min-max]; machine under load)

| format | write | full read | projection (events.kind) | point lookup per op | how |
|---|---|---|---|---|---|
| **KORE v4 compact (opt-in, this branch)** | 504.2 ms [487.8 ms-572.7 ms] | 12.6 ms [11.3 ms-13.2 ms] | 2.6 ms [2.5 ms-2.6 ms] | 2.2 ms [2.1 ms-2.4 ms] | segments + MANIFEST + 1 dictionary object; lookup = scan of event_id columns (no index) |
| KORE v4 compact, zstd level 19 (opt-in) | 509.8 ms | not available | not available | not available | same layout and decoder as the row above; one timed write; reads not repeated |
| KORE default writer (6e2e6a7b) = DL log format 1 | 60.1 ms [57.7 ms-344.0 ms] | 10.7 ms [9.1 ms-26.0 ms] | 3.0 ms [2.6 ms-3.7 ms] | 1.9 ms [1.7 ms-2.6 ms] | same files as the format 1 log (31/31 segments rewritten byte-identical) |
| Digital Life log format 3 (own codec) | 205.7 ms | 10.7 ms [10.6 ms-12.5 ms] | not available | 724 us [524 us-733 us] | write = build via log API (fsync per commit, sha256, manifest); full read = open + latest snapshot; lookup = get_event (in-memory id index); projection not exposed by the API |
| Digital Life log format 1 via log API | 166.2 ms | 10.0 ms [9.8 ms-11.3 ms] | not available | 1 us [1 us-1 us] | same bytes as the KORE default row; times through the log API |
| CSV (3 files) | 79.8 ms | 40.7 ms [33.1 ms-60.5 ms] | 26.8 ms [22.9 ms-30.6 ms] | 9.9 ms [9.5 ms-11.8 ms] | lookup = scan of events.csv until found (20 ids) |
| CSV + gzip -6 | 176.4 ms | 34.0 ms [30.4 ms-54.1 ms] | 34.4 ms [32.9 ms-44.7 ms] | not available | write = CSV 0.080s + compress 0.097s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + gzip -9 | 248.1 ms | 42.7 ms [32.7 ms-50.8 ms] | 29.9 ms [26.4 ms-33.8 ms] | not available | write = CSV 0.080s + compress 0.168s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + bz2 -9 | 234.3 ms | 74.5 ms [72.6 ms-81.1 ms] | 72.7 ms [70.8 ms-74.0 ms] | not available | write = CSV 0.080s + compress 0.155s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + xz -9 (lzma preset 9) | 1.14 s | 65.7 ms [58.2 ms-72.6 ms] | 58.4 ms [53.2 ms-62.1 ms] | not available | write = CSV 0.080s + compress 1.056s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + zstd -3 (pyarrow codec) | 106.1 ms | 24.8 ms [24.7 ms-25.5 ms] | 20.1 ms [19.6 ms-21.1 ms] | not available | write = CSV 0.080s + compress 0.026s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + zstd -19 (pyarrow codec) | 843.1 ms | 18.8 ms [18.1 ms-25.5 ms] | 18.8 ms [18.3 ms-24.2 ms] | not available | write = CSV 0.080s + compress 0.763s; reads decompress the whole file; point lookup not available without full decompression |
| SQLite | 139.4 ms | 20.1 ms [19.9 ms-25.4 ms] | 6.6 ms [6.4 ms-7.4 ms] | 27 us [25 us-27 us] | synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup) |
| SQLite after VACUUM | 264.4 ms | 20.1 ms [19.9 ms-25.4 ms] | 6.6 ms [6.4 ms-7.4 ms] | 27 us [25 us-27 us] | synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup); reads measured on the vacuumed file; VACUUM 0.125s |
| Parquet zstd (default level) | 33.8 ms | 7.9 ms [7.4 ms-8.2 ms] | 6.9 ms [6.8 ms-7.5 ms] | 4.5 ms [4.3 ms-4.5 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet zstd level 22 (max) | 829.0 ms | 12.2 ms [11.5 ms-15.2 ms] | 20.5 ms [18.9 ms-21.7 ms] | 7.7 ms [7.5 ms-7.8 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet snappy | 52.9 ms | 14.5 ms [13.5 ms-15.1 ms] | 15.9 ms [13.7 ms-17.3 ms] | 5.8 ms [5.5 ms-5.8 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet zstd (default), ids as binary(32) | 49.0 ms | 8.7 ms [7.9 ms-9.0 ms] | 8.5 ms [7.6 ms-9.3 ms] | 6.7 ms [5.6 ms-8.2 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column); 64-hex id columns stored as fixed_size_binary(32), like KORE v4 HexFixed |
| ORC zstd | 40.0 ms | 12.0 ms [9.5 ms-12.4 ms] | 8.4 ms [8.1 ms-8.5 ms] | 5.0 ms [4.4 ms-5.1 ms] | lookup = read event_id+text columns, pc.index (no predicate pushdown in pyarrow.orc) |
| Arrow IPC / Feather v2 uncompressed | 13.4 ms | 1.7 ms [1.7 ms-2.1 ms] | 7.9 ms [7.2 ms-13.8 ms] | 1.8 ms [1.7 ms-1.8 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| Arrow IPC / Feather v2 lz4 | 20.9 ms | 2.4 ms [2.4 ms-2.5 ms] | 6.3 ms [6.1 ms-7.3 ms] | 2.3 ms [2.0 ms-2.5 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| Arrow IPC / Feather v2 zstd | 35.3 ms | 4.3 ms [4.2 ms-5.8 ms] | 7.8 ms [7.8 ms-8.1 ms] | 2.8 ms [2.6 ms-3.0 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| DuckDB .duckdb (before explicit CHECKPOINT) | 31.5 ms | 9.6 ms [9.6 ms-36.0 ms] | 9.6 ms [8.7 ms-18.8 ms] | 1.3 ms [1.1 ms-1.4 ms] | CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan); size = db file + WAL right after loading; reads measured on the checkpointed file |
| DuckDB .duckdb after CHECKPOINT | 95.6 ms | 9.6 ms [9.6 ms-36.0 ms] | 9.6 ms [8.7 ms-18.8 ms] | 1.3 ms [1.1 ms-1.4 ms] | CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan); CHECKPOINT 0.064s |

#### Times, uniform (format_bench_gen.py), N=100,000 (median [min-max]; machine under load)

| format | write | full read | projection (events.kind) | point lookup per op | how |
|---|---|---|---|---|---|
| **KORE v4 compact (opt-in, this branch)** | 14.29 s [8.96 s-17.32 s] | 448.1 ms [350.6 ms-475.9 ms] | 91.6 ms [63.9 ms-109.3 ms] | 57.6 ms [54.7 ms-58.0 ms] | segments + MANIFEST + 1 dictionary object; lookup = scan of event_id columns (no index) |
| KORE v4 compact, zstd level 19 (opt-in) | 15.65 s | not available | not available | not available | same layout and decoder as the row above; one timed write; reads not repeated |
| KORE default writer (6e2e6a7b) = DL log format 1 | 2.69 s [2.29 s-2.80 s] | 499.6 ms [487.1 ms-594.9 ms] | 112.3 ms [102.1 ms-115.0 ms] | 52.0 ms [48.7 ms-58.6 ms] | same files as the format 1 log (301/301 segments rewritten byte-identical) |
| Digital Life log format 3 (own codec) | 3.44 s | 145.6 ms [141.0 ms-153.8 ms] | not available | 2.0 ms [1.7 ms-2.1 ms] | write = build via log API (fsync per commit, sha256, manifest); full read = open + latest snapshot; lookup = get_event (in-memory id index); projection not exposed by the API |
| Digital Life log format 1 via log API | 2.85 s | 120.6 ms [102.6 ms-123.0 ms] | not available | 2 us [2 us-3 us] | same bytes as the KORE default row; times through the log API |
| CSV (3 files) | 918.4 ms | 663.6 ms [626.0 ms-762.0 ms] | 417.8 ms [353.9 ms-505.9 ms] | 211.8 ms [197.4 ms-221.7 ms] | lookup = scan of events.csv until found (20 ids) |
| CSV + gzip -6 | 2.68 s | 819.4 ms [683.6 ms-909.5 ms] | 721.4 ms [635.8 ms-756.8 ms] | not available | write = CSV 0.918s + compress 1.765s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + gzip -9 | 3.09 s | 564.5 ms [558.7 ms-805.7 ms] | 508.3 ms [499.8 ms-519.8 ms] | not available | write = CSV 0.918s + compress 2.170s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + bz2 -9 | 3.65 s | 2.55 s [2.34 s-2.73 s] | 1.48 s [1.24 s-2.19 s] | not available | write = CSV 0.918s + compress 2.732s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + xz -9 (lzma preset 9) | 47.21 s | 2.15 s [1.99 s-2.23 s] | 1.86 s [1.85 s-1.98 s] | not available | write = CSV 0.918s + compress 46.296s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + zstd -3 (pyarrow codec) | 1.62 s | 1.04 s [861.9 ms-1.13 s] | 677.3 ms [615.2 ms-693.5 ms] | not available | write = CSV 0.918s + compress 0.701s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + zstd -19 (pyarrow codec) | 57.15 s | 1.09 s [1.09 s-1.13 s] | 782.8 ms [776.1 ms-1.05 s] | not available | write = CSV 0.918s + compress 56.231s; reads decompress the whole file; point lookup not available without full decompression |
| SQLite | 19.68 s | 553.7 ms [546.9 ms-564.9 ms] | 231.8 ms [216.1 ms-268.6 ms] | 80 us [63 us-646 us] | synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup) |
| SQLite after VACUUM | 22.75 s | 553.7 ms [546.9 ms-564.9 ms] | 231.8 ms [216.1 ms-268.6 ms] | 80 us [63 us-646 us] | synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup); reads measured on the vacuumed file; VACUUM 3.066s |
| Parquet zstd (default level) | 582.4 ms | 73.9 ms [69.9 ms-74.9 ms] | 209.9 ms [192.7 ms-252.6 ms] | 55.1 ms [49.5 ms-59.2 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet zstd level 22 (max) | 23.08 s | 49.3 ms [43.3 ms-52.8 ms] | 132.5 ms [124.5 ms-132.9 ms] | 36.3 ms [36.1 ms-38.0 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet snappy | 199.3 ms | 35.8 ms [30.4 ms-41.5 ms] | 139.5 ms [131.3 ms-140.5 ms] | 22.6 ms [22.5 ms-23.5 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet zstd (default), ids as binary(32) | 191.4 ms | 43.8 ms [41.5 ms-49.8 ms] | 136.4 ms [125.5 ms-137.7 ms] | 35.6 ms [35.2 ms-36.2 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column); 64-hex id columns stored as fixed_size_binary(32), like KORE v4 HexFixed |
| ORC zstd | 243.5 ms | 77.6 ms [50.8 ms-107.6 ms] | 91.0 ms [87.2 ms-105.7 ms] | 37.9 ms [35.7 ms-45.8 ms] | lookup = read event_id+text columns, pc.index (no predicate pushdown in pyarrow.orc) |
| Arrow IPC / Feather v2 uncompressed | 34.3 ms | 5.4 ms [5.0 ms-5.7 ms] | 142.2 ms [111.0 ms-144.4 ms] | 14.8 ms [13.0 ms-22.5 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| Arrow IPC / Feather v2 lz4 | 62.7 ms | 19.0 ms [18.5 ms-21.9 ms] | 96.8 ms [90.3 ms-100.2 ms] | 18.3 ms [18.1 ms-19.1 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| Arrow IPC / Feather v2 zstd | 71.3 ms | 23.6 ms [23.6 ms-26.7 ms] | 127.3 ms [90.1 ms-152.2 ms] | 24.4 ms [23.7 ms-25.9 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| DuckDB .duckdb (before explicit CHECKPOINT) | 508.5 ms | 76.4 ms [55.1 ms-76.9 ms] | 80.2 ms [70.6 ms-94.2 ms] | 6.5 ms [5.0 ms-7.4 ms] | CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan); size = db file + WAL right after loading; reads measured on the checkpointed file |
| DuckDB .duckdb after CHECKPOINT | 558.6 ms | 76.4 ms [55.1 ms-76.9 ms] | 80.2 ms [70.6 ms-94.2 ms] | 6.5 ms [5.0 ms-7.4 ms] | CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan); CHECKPOINT 0.050s |

#### Times, uniform (format_bench_gen.py), N=1,000,000 (median [min-max]; machine under load)

| format | write | full read | projection (events.kind) | point lookup per op | how |
|---|---|---|---|---|---|
| **KORE v4 compact (opt-in, this branch)** | 42.25 s [41.72 s-42.77 s] | 1.05 s [1.02 s-1.07 s] | 284.7 ms [274.0 ms-295.4 ms] | 142.2 ms [136.0 ms-148.5 ms] | segments + MANIFEST + 1 dictionary object; lookup = scan of event_id columns (no index) |
| KORE v4 compact, zstd level 19 (opt-in) | 41.86 s | not available | not available | not available | same layout and decoder as the row above; one timed write; reads not repeated |
| KORE default writer (6e2e6a7b) = DL log format 1 | 5.37 s [5.29 s-5.45 s] | 919.0 ms [855.9 ms-982.0 ms] | 257.3 ms [252.6 ms-262.0 ms] | 163.8 ms [159.8 ms-167.7 ms] | same files as the format 1 log (3001/3001 segments rewritten byte-identical) |
| Digital Life log format 3 (own codec) | 16.98 s | 816.1 ms [806.8 ms-825.4 ms] | not available | 752 us [748 us-756 us] | write = build via log API (fsync per commit, sha256, manifest); full read = open + latest snapshot; lookup = get_event (in-memory id index); projection not exposed by the API |
| Digital Life log format 1 via log API | 20.16 s | 735.5 ms [652.0 ms-819.0 ms] | not available | 2 us [2 us-2 us] | same bytes as the KORE default row; times through the log API |
| CSV (3 files) | 3.14 s | 3.11 s [3.03 s-3.20 s] | 1.93 s [1.74 s-2.12 s] | 780.0 ms [767.9 ms-792.0 ms] | lookup = scan of events.csv until found (5 ids) |
| CSV + gzip -6 | 9.95 s | 4.36 s [4.31 s-4.41 s] | 3.00 s [2.97 s-3.03 s] | not available | write = CSV 3.138s + compress 6.812s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + gzip -9 | 12.41 s | 4.25 s [3.88 s-4.62 s] | 2.65 s [2.61 s-2.68 s] | not available | write = CSV 3.138s + compress 9.276s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + bz2 -9 | 19.18 s | 10.14 s [9.85 s-10.43 s] | 8.94 s [8.59 s-9.29 s] | not available | write = CSV 3.138s + compress 16.039s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + xz -9 (lzma preset 9) | 313.26 s | 7.48 s [7.11 s-7.84 s] | 6.11 s [6.01 s-6.20 s] | not available | write = CSV 3.138s + compress 310.120s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + zstd -3 (pyarrow codec) | 5.32 s | 3.63 s [3.62 s-3.63 s] | 2.00 s [1.99 s-2.00 s] | not available | write = CSV 3.138s + compress 2.182s; reads decompress the whole file; point lookup not available without full decompression |
| CSV + zstd -19 (pyarrow codec) | 178.25 s | 3.92 s [3.68 s-4.16 s] | 2.53 s [2.48 s-2.59 s] | not available | write = CSV 3.138s + compress 175.111s; reads decompress the whole file; point lookup not available without full decompression |
| SQLite | 106.74 s | 6.63 s [6.12 s-7.15 s] | 2.24 s [2.16 s-2.31 s] | 78 us [76 us-81 us] | synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup) |
| SQLite after VACUUM | 126.31 s | 6.63 s [6.12 s-7.15 s] | 2.24 s [2.16 s-2.31 s] | 78 us [76 us-81 us] | synchronous=OFF, one transaction per version; event_id TEXT PRIMARY KEY (B-tree index used for the lookup); reads measured on the vacuumed file; VACUUM 19.567s |
| Parquet zstd (default level) | 2.86 s | 467.4 ms [428.4 ms-506.4 ms] | 1.92 s [1.84 s-1.99 s] | 421.3 ms [415.5 ms-427.0 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet zstd level 22 (max) | 156.38 s | 185.3 ms [182.8 ms-187.9 ms] | 701.7 ms [642.0 ms-761.4 ms] | 179.2 ms [177.4 ms-180.9 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet snappy | 763.1 ms | 113.6 ms [107.7 ms-119.5 ms] | 705.6 ms [700.5 ms-710.6 ms] | 102.0 ms [99.7 ms-104.4 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column) |
| Parquet zstd (default), ids as binary(32) | 865.4 ms | 245.0 ms [186.5 ms-303.6 ms] | 710.9 ms [701.2 ms-720.7 ms] | 224.0 ms [185.4 ms-262.6 ms] | lookup = pq.read_table(columns=['text'], filters=[event_id == id]) (row-group statistics cannot prune random ids: scans the id column); 64-hex id columns stored as fixed_size_binary(32), like KORE v4 HexFixed |
| ORC zstd | 943.8 ms | 925.2 ms [923.5 ms-927.0 ms] | 1.35 s [1.28 s-1.41 s] | 448.1 ms [412.3 ms-483.9 ms] | lookup = read event_id+text columns, pc.index (no predicate pushdown in pyarrow.orc) |
| Arrow IPC / Feather v2 uncompressed | 273.7 ms | 49.6 ms [45.2 ms-54.0 ms] | 1.19 s [1.18 s-1.20 s] | 167.8 ms [156.8 ms-178.8 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| Arrow IPC / Feather v2 lz4 | 332.1 ms | 179.2 ms [152.6 ms-205.8 ms] | 1.14 s [1.07 s-1.21 s] | 176.2 ms [173.6 ms-178.7 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| Arrow IPC / Feather v2 zstd | 511.8 ms | 237.5 ms [229.6 ms-245.4 ms] | 1.19 s [1.18 s-1.20 s] | 215.7 ms [215.5 ms-215.9 ms] | lookup = memory-mapped read of event_id+text, pc.index |
| DuckDB .duckdb (before explicit CHECKPOINT) | 3.89 s | 666.0 ms [659.5 ms-672.6 ms] | 1.15 s [1.05 s-1.26 s] | 17.3 ms [15.9 ms-18.8 ms] | CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan); size = db file + WAL right after loading; reads measured on the checkpointed file |
| DuckDB .duckdb after CHECKPOINT | 4.17 s | 666.0 ms [659.5 ms-672.6 ms] | 1.15 s [1.05 s-1.26 s] | 17.3 ms [15.9 ms-18.8 ms] | CREATE TABLE AS SELECT from Arrow; no index (lookup = WHERE event_id = ? using zone maps / scan); CHECKPOINT 0.277s |

#### KORE v4 / KORE default, paired ratios (uniform (format_bench_gen.py); same repetition, below 1 = v4 faster)

| N | write | full read | projection | lookup |
|---|---|---|---|---|
| 10,000 | 8.38 [1.66-8.45] | 1.18 [0.43-1.44] | 0.85 [0.72-0.97] | 1.11 [0.87-1.40] |
| 100,000 | 5.32 [3.92-6.18] | 0.80 [0.70-0.92] | 0.82 [0.56-1.07] | 1.12 [0.98-1.12] |
| 1,000,000 | 7.87 [7.85-7.89] | 1.15 [1.04-1.26] | 1.11 [1.05-1.17] | 0.87 [0.81-0.93] |

#### KORE v4 per column bytes (uniform (format_bench_gen.py))

| column | N=10,000 | N=100,000 | N=1,000,000 | codec (transform/stage) at largest N |
|---|---:|---:|---:|---|
| events.event_id | 320,030 | 3,200,300 | 32,003,000 | {'t4/s0': 1000} |
| events.kind | 4,034 | 40,292 | 403,129 | {'t3/s3': 1000} |
| events.occurred_at | 7,827 | 78,209 | 781,719 | {'t5/s3': 1000} |
| events.recorded_at | 230 | 2,300 | 23,000 | {'t5/s1': 1000} |
| events.redacted | 130 | 1,300 | 13,000 | {'t0/s1': 1000} |
| events.source | 4,915 | 49,084 | 490,611 | {'t3/s3': 1000} |
| events.tags | 11,261 | 112,645 | 1,127,446 | {'t6/s2': 1, 't6/s3': 999} |
| events.text | 194,839 | 2,131,971 | 21,493,872 | {'t0/s4': 1000} |
| revision_causes.event_id | 6,430 | 64,300 | 643,000 | {'t4/s0': 1000} |
| revision_causes.rev_id | 6,430 | 64,300 | 643,000 | {'t4/s0': 1000} |
| revisions.author | 240 | 2,400 | 24,000 | {'t3/s1': 1000} |
| revisions.confidence | 1,268 | 12,538 | 124,385 | {'t0/s1': 199, 't0/s3': 801} |
| revisions.entity_type | 200 | 2,000 | 20,000 | {'t3/s1': 1000} |
| revisions.key | 630 | 6,300 | 63,000 | {'t0/s3': 1000} |
| revisions.rationale | 767 | 7,697 | 78,590 | {'t0/s3': 1000} |
| revisions.recorded_at | 180 | 1,800 | 18,000 | {'t5/s1': 1000} |
| revisions.rev_id | 6,430 | 64,300 | 643,000 | {'t4/s0': 1000} |
| revisions.stance | 772 | 7,737 | 77,347 | {'t3/s0': 24, 't3/s1': 757, 't3/s3': 219} |
| revisions.supersedes | 3,235 | 61,105 | 639,805 | {'t0/s0': 5, 't4/s0': 995} |
| revisions.valid_from | 440 | 4,400 | 44,000 | {'t5/s0': 1000} |
| schema_meta.created_at | 6 | 6 | 6 | {'t5/s0': 1} |
| schema_meta.format_version | 2 | 2 | 2 | {'t1/s0': 1} |
| dictionary object | 111,081 | 111,081 | 111,081 | raw text of the first events segment (tail) |


## 5. Where KORE v4 wins and where it loses

Wins (sizes are exact):

* Against its own default (6e2e6a7b): -37 % (realistic) and -39 % (uniform) at 1M; -27 % / -30 % at 10k.
* Against the Digital Life log format 3: -11 % on both datasets at 1M, -10 % at 100k. At 10k it is larger (see
  losses).
* Against every format that keeps random access to columns: Parquet (default and level 22, text or binary ids),
  ORC, Arrow IPC, DuckDB, SQLite are all larger at 100k and 1M on both datasets (Parquet level 22 is the
  closest: +1.4 % realistic, +1.2 % uniform at 1M vs KORE v4 level 9).
* Where the per-column bytes go (realistic, 1M, 79.3 MB): text 39.1 MB, the 64-hex event ids 32.0 MB as raw
  bytes (incompressible random sha256; the floor for this id scheme), tags 2.1 MB, the other events columns (timestamps via `TsText`, low-cardinality strings via `Dict`, file headers) 2.0 MB,
  revisions 2.0 MB, revision causes 1.4 MB, the MANIFEST 0.65 MB, the dictionary 0.13 MB.
  Text is 21.5 MB on the uniform data.
* Reads: full read, projection and lookup cost the same as the default writer within noise (paired ratios
  0.80-1.18, one 1.64 at 10k). Projection keeps working and does not need the dictionary for columns that do
  not use it.

Losses (reported with the same weight):

* **CSV+xz-9 is 0.6 % smaller than KORE v4 at level 9 on realistic 1M (78.8 vs 79.3 B/event)**, and CSV+bz2-9 is
  **4 % smaller on uniform data at every size (58.4 vs 60.7 at 1M)**. Both compress the whole table as one
  stream; they cannot read one column or one row without decompressing everything, and they are much slower:
  xz-9 wrote for 347 s and bz2 read for 17.8 s at 1M realistic (KORE v4: 14.1 s write, 1.3 s read). KORE v4 at
  level 19 (76.3) is below xz on realistic data, not on uniform data (level 19 changes nothing there: the
  uniform text payloads are below 256 KiB per segment, which already use level 19).
* **At 10k events KORE v4 is not the smallest**: 90.1 B/event realistic vs log format 3 88.2, bz2 80.4, xz 82.0,
  zstd-19 83.5, Parquet-22 84.8. The dictionary object (131,115 bytes) is 13 B/event at 10k and 0.13 B/event at
  1M. A smaller dictionary for small logs was not tried in this run.
* **Write is 2.0-2.2x slower than the default writer on realistic data and 5.3-8.4x on uniform data** (paired).
  Cause: zstd level 9/19 instead of 3, and the stage search (Auto tries none/lz4/zstd/hz/zstd+dictionary).
  Level 19 at 1M realistic: 139 s vs 14 s for level 9. On uniform data the small payloads all go to level 19.
  The log format 3 build (19.9 s at 1M) and the format 1 build (22.5 s) include fsync per commit and are not
  comparable with the store write times.
* **Point lookup by id has no index in the store**: 158 ms per lookup at 1M (scan of up to 1000 id columns),
  the same as the default writer; SQLite 64 us, DuckDB 18 ms, Parquet 363 ms, log format 3 `get_event` 1.0 ms
  (in-memory index built at open). An id index is not part of this work.
* Full reads through the store example (1.2-1.3 s at 1M) are slower than the Digital Life log API on the same
  format 1 files (0.81 s) and than Arrow IPC (0.05-0.26 s) or Parquet (0.37-0.43 s); the store example reads
  segments one after the other.

## 6. Tests

* `cargo test --offline -p kore-store` (debug): **63 passed, 0 failed** (lib 18, codecs 14, compact 13 new,
  conformance 1, nulls 2, projection 2, shuffle 2, speed_compat 3, stress 6, strlen 2).
* New in `tests/compact.rs`: v4 Auto round trip and choice of `HexFixed`/`TsText`/`Tokens`, v4 < v3 < v2 size on
  the same block; every explicit transform x stage incl. edge values (year 0000/9999, leap day, pre-1970, "",
  ",", "a,,b", UTF-8, nulls, empty and all-null columns); non-canonical timestamps refused explicitly and kept
  verbatim under Auto; v3 writer refuses the v4-only transforms and v3 ids stay 0-4; shared dictionary round
  trip, smaller than without, clear error without it, wrong valid dictionary refused, trained dictionary;
  projection without the dictionary, corrupt unrequested payload not read; tamper at every byte (3 masks) and
  every truncation of a v4 file and of a KDIC object, mismatched `.kdic` file name refused; garbage payloads
  with recomputed CRCs never panic; earlier-version readers refuse v4 at the version field; fixtures written by
  the unmodified 6e2e6a7b code read and are reproduced byte for byte by the default and v3 writers; KVLG version
  log with v2 + two v4 entries using two different dictionaries, time travel, per-version dictionary needs,
  `list_versions` without dictionaries; determinism across runs and 1/2/7 threads.
* kore-self (Digital Life, main-Digital-Life-AI ecf5f695) built against this branch's kore-store in the scratch
  worktree (path-limited copy of `kore-store/`, nothing committed there): `cargo test --offline -p kore-self`
  **443 passed, 0 failed** (lib 357; integration 4 + 5 + 4 + 3 + 1 + 68 + 1), run after the benchmark with the final `kore-store/` sources (identical to this branch).
* The old-reader check of section 1 (real 6e2e6a7b binary).

## 7. Not measured / not done

* zstd command-line tool: not installed; CSV+zstd was measured with pyarrow's zstd codec (one frame per file).
  The `zstandard` Python package is not installed.
* DuckDB `tpch` extension: not installed and would need a download, so no standard public dataset was measured.
* Polars was available but not used (it writes the same Parquet/IPC formats through its own writer).
* The opt-in is not wired into kore-self: the Digital Life log does not yet write v4 segments, list the
  dictionary in its MANIFEST, or verify it in `verify()`. The KORE v4 size is the format 1 log re-encoded
  segment by segment with the MANIFEST copied unchanged (same length; its hash values would differ).
* Dictionary choices other than "last 128 KiB of the first segment's text" in this run (a 64 KiB and a 256 KiB
  raw dictionary, and zstd levels 6/12/15, were tried once at 100k during development: 128 KiB at level 9 was the
  best size/time point; those trial numbers are not part of the table). No dictionary for other columns; no
  re-training as the log grows.
* Idle machine, cold cache, fsync, peak memory; more than 2-3 repetitions; other machines.
* Other-language readers (C#, Go, Java, Python bindings, FFI, `conformance/`) on v4 files.
* Encryption of v4 files, row groups with v4, v4 entries written by `versioned::append_version` (only
  `append_raw` is supported for v4).
* Nulls in the benchmark data (tests cover nulls).

## 8. Reproduce

```
# store side (this branch)
set CARGO_TARGET_DIR=C:/Users/skathera/kw/target-ff2
cargo build --release --offline -p kore-store --example bench_kore_size
# Digital Life side: a Digital Life checkout whose kore-store/ is replaced by this branch's kore-store/,
# with scripts/kore_size/dl_log_side.rs copied to kore-self/examples/
cargo build --release --offline -p kore-self --example dl_log_side
# one run (raw JSON results are not committed)
sh scripts/kore_size/run.sh OUTDIR "10000 100000 1000000" "r u"
python -I scripts/kore_size/tables.py OUTDIR
```
The run above was executed as `run.sh OUTDIR "10000 100000" "r u"` followed, after the disk check, by
`run.sh OUTDIR "1000000" "r u"` into the same OUTDIR with the same binaries (the 1M part waited for free disk
space, see the coordinator's note; nothing was rebuilt in between).
