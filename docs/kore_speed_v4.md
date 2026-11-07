# KORE file format: point lookups and write speed of the opt-in version 4 (one-run comparison)

Branch `fileformat-improve-3` (from `origin/fileformat` @ bbbefd2c, which contains the opt-in version-4 compact
container of cd2806f4, docs/kore_size.md). Everything here is additive and opt-in. Nothing was pushed.

Starting point (docs/kore_size.md, same harness): a point lookup by event id took 158 ms at 1M events (no
index; the event_id column of up to 1000 segment files was scanned), and the version-4 writer was 2.0-2.2x
slower than the default writer on realistic data, 5.3-8.4x on uniform data, level 19 another ~10x.

## 0. Result in short

* **Point lookup with the opt-in key index**: realistic data, warm, per lookup **1.2 / 2.3 / 3.9 ms at 10k / 100k / 1M**,
  against 4.8 ms / 70 ms / 1.45 s for the scan in the same run (paired ratio 0.27 / 0.042 / **0.0027**, about 370x faster
  at 1M). Missing keys took 1-2 us with no file read (0 of 100 at every size). The index costs **2.0 / 2.3 / 2.6 bytes per
  event** (2.64 MB at 1M), so KORE v4 + index is 82.0 B/event at 1M realistic. That is still smaller than Digital Life log
  format 3 (89.2) and Parquet zstd (96.4), but no longer smaller than Parquet zstd level 22 (80.4) or CSV+xz-9 (78.8,
  docs/kore_size.md). Without the index nothing changed. **The ~1 ms target was not met in this run**: the machine was
  saturated (section 2) and every time is inflated. The same lookup, measured step by step on the 100k files, took
  0.84-0.85 ms (section 3.9). SQLite (B-tree, 50-113 us) is still 10-35x faster. DuckDB took 2.1 / 8.9 / 60 ms, Parquet
  9.4 ms / 48 ms / 3.65 s, and Digital Life format 3 get_event 1.2 / 2.0 / 5.0 ms.
* **Write, byte-identical**: the per-file version-4 encoder could not be made faster without changing bytes (section 1.2):
  context reuse and speculation gave nothing measurable, and the expensive zstd stages are the ones that win. The batch
  API `to_bytes_compact_many` writes the same bytes in **0.50-0.69x** the time of one file at a time (realistic) and
  0.60-0.73x (uniform). On realistic 10k/100k that is about the speed of the default writer one file at a time
  (1.04 / 1.08), but still 1.35-1.89x the default writer when the default writer is batched too.
* **Write, explicit faster settings** (other bytes, still ordinary version-4 files that the bbbefd2c reader reads):
  `CompactOptions::fast()` (small payloads at level 9 instead of 19) halves the uniform write time (0.41-0.51x) for
  +7.5 % / +9.3 % size (69.4 -> 74.6, 61.5 -> 67.2 B/event). It does nothing on realistic data, whose text payloads are
  already above 256 KiB. `fast().with_zstd_level(3)` gives 0.53 / 0.63 / 0.88x on realistic data for +4.1 / +5.1 / +5.3 %
  size (83.5 B/event at 1M); on uniform data it equals `fast()`.
* Compatibility: the default, version-3 and version-4 writers produce the bytes of bbbefd2c (19 digest cases, every
  benchmark segment), all v1-v4 files read as before, and the index is a separate object that every data reader refuses.

## 1. What changed

| change | bytes of existing writers | where |
|---|---|---|
| **key index object** (`.kidx`, new, opt-in): maps the values of one key column to the data file that holds them | unchanged (the index is a separate object; data files are not touched) | `kore-store/src/keyindex.rs` |
| clear error when a `.kidx` object is handed to the data reader | - | `reader.rs` (4 lines, error path only) |
| `KoreWriter::to_bytes_compact_many` / `v4::write_many`: several version-4 files encoded concurrently | each output byte-identical to `to_bytes_compact` of that block | `v4.rs`, `writer.rs` |
| `CompactOptions::small_zstd_level` (default 19 = bbbefd2c bytes) and `CompactOptions::fast()` (= small level 9): explicit, changes bytes | default unchanged; `fast()` is a different (still ordinary) version-4 file | `v4.rs` |
| harness: `PY_SET=ref` (only SQLite, Parquet zstd, DuckDB on the Python side), key-index and writer rows in `bench_kore_size`, `speed_tables.py` | - | `scripts/kore_size/`, `kore-store/examples/` |

The version-4 encoder itself is the code of bbbefd2c (one extra parameter: the level for payloads below 256 KiB,
default 19). The default writer (`to_bytes`), the version-3 writer (`to_bytes_with`) and every reader are
unchanged apart from the error message above. `kore-core` is untouched; no dependency was added.

### 1.1 Key index object (KIDX version 1)

```text
"KIDX" | version:u16 = 1 | hash:u8 = 1 (KH64) | flags:u8 = 0
column_len:u16 | column                       the key column (UTF-8)
num_files:u32, per file: name_len:u16 | name | file_len:u64 | file_sha256:[32] | rows:u32
num_keys:u64 | q:u8 | k:u8 | file_bits:u8 | block_keys:u16 (128)
num_blocks:u32, per block: first:u64 | bit_offset:u64
files_len:u64 | file numbers                  num_keys x file_bits bits, LSB first
rice_len:u64  | Rice stream                   per block: block_keys-1 deltas, unary(d >> k) as ones + a zero, then k low bits
sha256:[32]                                   of every byte before it = the index id (file name <id>.kidx)
```

* Every non-null key is hashed (KH64, 64 bit, specified in `keyindex.rs`; pinned by a test) and the top
  `q = ceil(log2(num_keys)) + 8` bits are kept. The sorted hashes are a Golomb-Rice coded set (Rice parameter `k`
  chosen exactly at build time), with the file number of each entry in a packed array. The key itself and the row
  are **not** stored: about `k + 2 + file_bits` bits per key, measured **2.3-2.6 bytes per event** here (section 3).
* Lookup: hash, binary search over the block fences, decode one block of 128 entries (and the next while the hash
  repeats), read only the files with an equal hash, check each file against its pin, decode its key column, compare
  the key, and only then decode the requested value columns of that one file. A hash collision can cost an extra
  file read, never a wrong row. A key that is not present finds an equal hash with probability about 2^-8 (no file
  is read otherwise).
* Integrity: `KeyIndex::from_bytes` verifies the whole object before use (sha256 = id, exact length, magic,
  version, hash id, flags, parameters consistent with the counts, fences, offsets, every block decodes to sorted
  hashes below 2^q, every file number in range, zero padding). Every data file is pinned by length and sha256:
  `Verify::Sha256` (default) hashes the file before using it and refuses a changed, truncated or replaced file
  ("file ... changed since the index was built"); `Verify::Length` checks the length only and relies on the file's
  own CRCs (which cover only what is decoded).
* Versioning / time travel: an index object is immutable. A new set of files (a new version of a store) gets a new
  index with a new id; old indexes keep pinning the exact bytes of their version. File names are opaque to the
  index; the caller resolves them (a path, or an entry of a KVLG version log, tested).
* Works for version 2, 3 and 4 data files (it only uses the public column projection); version-4 files need their
  shared dictionaries for the columns that use them, as before.
* Compatibility: data readers check the `KORE` magic first, so every reader, old or new, refuses a `.kidx`. The
  bbbefd2c reader (real binary, detached worktree, scratch example not committed) says
  `sample_index.kidx: ERR invalid argument: invalid KORE magic bytes`; the new reader says
  `this is a KORE key index object (.kidx), not a data file; open it with KeyIndex::from_bytes`. A `.kidx` with
  version 2 is refused with `unsupported key index version 2` even with a valid checksum.

API: `KeyIndexBuilder::new(column)`, `add_file_keys(name, bytes, keys)` (keys the writer already has) or
`add_file_bytes(name, bytes, dicts)` (decodes the key column), `build()`; `KeyIndex::from_bytes / read_file /
to_bytes / id / file_name / files / candidates(key) / lookup(key, columns, dicts, verify, read_fn)`.

### 1.2 Writer: what was tried

Profile of the version-4 writer on the 100k datasets (`examples/prof_v4_write.rs`, every column replayed
sequentially; machine 90-100 % busy, so only the proportions matter):

* realistic: zstd with the shared dictionary on the events `text` column (raw text of a 1000-event segment is above
  256 KiB, so level 9) is about half of all work and is the critical path of every segment; transform scoring
  (zstd level 1) about 17 %; zstd 19 on small columns about 15 %.
* uniform: the `text` payload is below 256 KiB, so plain zstd 19 and zstd 19 with the dictionary are both tried
  (in parallel); together about 90 % of all work, each one alone is the critical path.
* One segment at a time the writer used little parallelism: 3.3 s of a 3.5 s sequential replay (realistic),
  because one column dominates; all segments concurrently took 0.5 s for the same bytes.

| idea | result |
|---|---|
| reuse zstd contexts (thread-local compressor) | no measurable gain: zstd 19 on 100-250 KB took the same with a fresh and a reused context (12.9 vs 12.7 s summed over 100 segments); not kept |
| skip stages that cannot win | not possible without changing bytes: the stages that cost time (zstd 19, zstd + dictionary) are the ones that win; no cheap lower bound proves a loss |
| speculative stage search of `Plain` while the transforms are scored (byte-identical by construction; implemented in f8beb192) | no gain: on this data the free-text columns have a single candidate transform, so nothing is speculated; A/B over 4 alternating rounds 1.59-2.04 s (reference) vs 1.59-2.28 s; removed again in the next commit |
| cheaper transform detection | the probes (dictionary, hex, timestamp, tokens) are a few percent of the work and not on the critical path |
| **encode several files concurrently** (`to_bytes_compact_many`) | same bytes, the only byte-identical change with a large effect (section 3) |
| **lower level for small payloads** (`fast()`; explicit) | different bytes; size and speed in section 3 |

## 2. The benchmark

The single harness of docs/kore_size.md (`scripts/kore_size/run.sh`), one run over both datasets at 10k, 100k
and 1M events, with `PY_SET=ref` (Python side reduced to SQLite, Parquet zstd at the default level and DuckDB, the
reference rows asked for; the CSV/xz/bz2/ORC/IPC rows of docs/kore_size.md are not repeated). Same generators,
seeds and data hashes as docs/kore_size.md. The Digital Life side is the existing `dl_log_side.exe` (scratch
worktree at main-Digital-Life-AI ecf5f695 with the kore-store of fileformat-improve-2); it only writes the log
format 1/3 directories and their reference numbers, and format 1 segments are default-writer files whose bytes
this branch does not change (checked again: every segment is rewritten byte-identical, column
`default_rewrite_identical_files`).

Store side (`bench_kore_size`, this branch, release): per repetition and paired: default writer; version-4 writer
(one file at a time, as before); version-4 batch writer (all segments of the dataset in one
`to_bytes_compact_many` call, then written); default writer with all segments encoded concurrently (the fair
reference for the batch row); `fast()` and `fast().with_zstd_level(3)` (one file at a time). Reads from disk as in
docs/kore_size.md. Key index over events.event_id of the version-4 copy (and of the default files): built from the
keys in memory plus the sha256 of every file, written next to the store; lookups:

* **warm**: the index object and the dictionary are loaded once (like the Digital Life log's in-memory id index,
  which is built by `open` and not timed); each lookup = candidates, `std::fs::read` of one segment (OS file cache
  warm), sha256 check, decode of the event_id column, then of the `text` column of that file.
* **open each op**: every operation also reads and verifies the index object and loads (and sha256-verifies) the
  dictionary object from disk. This is the "first lookup in a fresh process" cost with a warm OS file cache.
* **cold** in the sense of an empty OS file cache (after reboot, or with the standby list flushed) was **not
  measured**: that needs administrator rights on this machine.
* missing keys: 5x as many random ids that are not in the store.

Times are per operation (20 ids, the same ids as docs/kore_size.md; 2 repetitions at 1M, 3 below).

**Machine load.** Machine load (typeperf total CPU, 48 samples at start/middle/end of every dataset and size): mean 100 %, min 89 %, max 100 %.
Skipped: uniform at 1M (the harness refuses 1M below 12 GB free; other jobs had filled the disk to 3 GB free). The machine was shared with other jobs and saturated the whole time; absolute times are inflated several-fold against docs/kore_size.md (e.g. the scan lookup at 1M realistic took 1.45 s here against 158 ms there, the default writer at 100k uniform 9.4 s against under 1 s). Use the paired ratios (same repetition, same process) and treat cross-format times as rough. Sizes are deterministic.

The realistic dataset ran first; the run was stopped once during the realistic 1M Python step (free disk fell to 3 GB) and resumed for the uniform dataset with the same binaries into the same output directory.

## 3. Results

### 3.1 Size, realistic dataset

| | N=10,000 | N=100,000 | N=1,000,000 |
|---|---:|---:|---:|
| KORE v4 (data files + dictionary + MANIFEST; same bytes before and after) | 0.90 MB, 90.1 B/event | 8.03 MB, 80.3 B/event | 79.31 MB, 79.3 B/event |
| key index object (.kidx) alone | 0.02 MB, 2.0 B/event | 0.23 MB, 2.3 B/event | 2.64 MB, 2.6 B/event |
| **KORE v4 + key index** | 0.92 MB, 92.0 B/event | 8.26 MB, 82.6 B/event | 81.95 MB, 82.0 B/event |
| KORE v4, `CompactOptions::fast()` (explicit, other bytes) | 0.90 MB, 90.1 B/event | 8.04 MB, 80.4 B/event | 79.39 MB, 79.4 B/event |
| KORE v4, `fast().with_zstd_level(3)` (explicit, other bytes) | 0.94 MB, 93.8 B/event | 8.44 MB, 84.4 B/event | 83.49 MB, 83.5 B/event |
| KORE default writer | 1.24 MB, 123.9 B/event | 12.63 MB, 126.3 B/event | 126.22 MB, 126.2 B/event |
| KORE v4, zstd level 19 | 0.87 MB, 87.1 B/event | 7.73 MB, 77.3 B/event | 76.29 MB, 76.3 B/event |
| Parquet zstd (default level) | 1.00 MB, 100.1 B/event | 9.75 MB, 97.5 B/event | 96.41 MB, 96.4 B/event |
| DuckDB .duckdb after CHECKPOINT | 7.35 MB, 735.2 B/event | 63.71 MB, 637.1 B/event | 635.19 MB, 635.2 B/event |
| SQLite | 6.54 MB, 654.1 B/event | 67.98 MB, 679.8 B/event | 684.08 MB, 684.1 B/event |
| Digital Life log format 3 | 0.88 MB, 88.2 B/event | 8.91 MB, 89.1 B/event | 89.16 MB, 89.2 B/event |

### 3.2 Write, realistic dataset (median [min-max] over repetitions)

| | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE default writer, one segment at a time | 116.8 ms [92.1 ms-157.3 ms] | 1.57 s [1.16 s-4.20 s] | 38.97 s [11.67 s-66.27 s] |
| **KORE v4** (`to_bytes_compact`; writer code and bytes as bbbefd2c), one segment at a time | 208.7 ms [198.6 ms-286.0 ms] | 3.40 s [2.42 s-3.63 s] | 86.09 s [73.47 s-98.71 s] |
| paired ratio v4 / default | 1.82 [1.79-2.16] | 2.08 [0.87-2.16] | 3.89 [1.49-6.29] |
| **KORE v4 batch API** (`to_bytes_compact_many`, all segments at once, same bytes) | 121.0 ms [108.6 ms-134.7 ms] | 1.56 s [1.19 s-1.70 s] | 56.16 s [41.49 s-70.83 s] |
| paired ratio v4 batch / v4 one at a time | 0.55 [0.47-0.58] | 0.50 [0.33-0.64] | 0.69 [0.42-0.96] |
| paired ratio v4 batch / default one at a time | 1.04 [0.86-1.18] | 1.08 [0.28-1.34] | 3.35 [0.63-6.07] |
| KORE default writer, all segments at once (fair reference for the batch row) | 74.8 ms [74.0 ms-76.1 ms] | 1.16 s [824.9 ms-1.45 s] | 33.03 s [8.74 s-57.32 s] |
| paired ratio v4 batch / default batch | 1.62 [1.43-1.82] | 1.35 [0.82-2.07] | 2.99 [1.24-4.75] |
| KORE v4 `fast()` (explicit, other bytes), one segment at a time | 241.1 ms [228.7 ms-242.2 ms] | 3.69 s [2.69 s-4.98 s] | 83.16 s [66.60 s-99.71 s] |
| paired ratio v4 fast / v4 | 1.10 [0.85-1.21] | 1.37 [0.79-1.52] | 1.02 [0.67-1.36] |
| paired ratio v4 fast / default | 1.96 [1.54-2.62] | 1.71 [1.19-3.17] | 4.77 [1.00-8.54] |
| KORE v4 `fast().with_zstd_level(3)` (explicit, other bytes), one at a time | 114.3 ms [106.0 ms-121.6 ms] | 1.53 s [1.07 s-2.37 s] | 67.20 s [18.39 s-116.01 s] |
| paired ratio v4 fast+level 3 / v4 | 0.53 [0.40-0.58] | 0.63 [0.31-0.65] | 0.88 [0.19-1.58] |
| paired ratio v4 fast+level 3 / default | 1.04 [0.73-1.15] | 0.68 [0.57-1.32] | 5.11 [0.28-9.94] |
| KORE v4 zstd level 19 (one run) | 2.52 s | 33.43 s | 903.66 s |
| key index build (keys in memory + sha256 of every events file + write) | 6.4 ms [4.7 ms-8.6 ms] | 75.2 ms [58.8 ms-82.5 ms] | 757.2 ms [692.7 ms-821.8 ms] |
| Parquet zstd (default level) | 59.6 ms | 482.5 ms | 10.17 s |
| DuckDB .duckdb after CHECKPOINT | 145.9 ms | 1.26 s | 20.36 s |
| SQLite | 406.9 ms | 27.35 s | 1055.53 s |
| Digital Life log format 3, build via log API (fsync per commit) | 338.7 ms | 4.21 s | 33.18 s |

### 3.3 Point lookup by event id, realistic dataset (text of one event; per operation)

| | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE v4 before: scan of the event_id column of each segment, then text | 4.8 ms [4.4 ms-5.2 ms] | 70.4 ms [48.9 ms-80.2 ms] | 1.45 s [1.39 s-1.50 s] |
| **KORE v4 + key index, warm** (index and dictionary in memory; one file read, sha256-checked) | 1.2 ms [991 us-1.4 ms] | 2.3 ms [1.2 ms-2.9 ms] | 3.9 ms [3.0 ms-4.7 ms] |
| paired ratio indexed / scan | 0.27 [0.19-0.29] | 0.0415 [0.0154-0.0467] | 0.0027 [0.0022-0.0031] |
| KORE v4 + key index, warm, `Verify::Length` (file CRCs only) | 1.2 ms [1.1 ms-1.4 ms] | 2.4 ms [1.4 ms-2.5 ms] | 2.5 ms [2.1 ms-2.9 ms] |
| KORE v4 + key index, index and dictionary loaded from disk and verified in every operation | 2.8 ms [2.3 ms-3.4 ms] | 12.1 ms [9.4 ms-12.2 ms] | 101.0 ms [100.4 ms-101.7 ms] |
| KORE v4 + key index, missing key (warm) | 1 us [1 us-2 us] (0 file reads for 100 keys) | 2 us [2 us-2 us] (0 file reads for 100 keys) | 1 us [1 us-2 us] (0 file reads for 100 keys) |
| KORE default files + key index, warm | 1.3 ms [1.2 ms-1.4 ms] | 1.5 ms [1.3 ms-2.8 ms] | 4.8 ms [3.7 ms-5.8 ms] |
| KORE default files, scan | 4.6 ms [3.9 ms-4.8 ms] | 71.2 ms [55.9 ms-75.1 ms] | 1.44 s [1.08 s-1.80 s] |
| key index load + verify (read file, sha256, structure) | 541 us [522 us-1.2 ms] | 3.3 ms [3.2 ms-7.7 ms] | 108.9 ms [102.6 ms-115.2 ms] |
| Parquet zstd (default level) | 9.4 ms [9.0 ms-9.4 ms] | 47.9 ms [45.9 ms-51.1 ms] | 3.65 s [3.51 s-3.79 s] |
| DuckDB .duckdb after CHECKPOINT | 2.1 ms [2.1 ms-2.4 ms] | 8.9 ms [8.7 ms-10.8 ms] | 59.9 ms [57.9 ms-61.9 ms] |
| SQLite | 63 us [61 us-63 us] | 50 us [48 us-51 us] | 113 us [103 us-123 us] |
| Digital Life log format 3, get_event (in-memory id index built by open) | 1.2 ms [1.2 ms-1.5 ms] | 2.0 ms [2.0 ms-2.1 ms] | 5.0 ms [4.6 ms-5.4 ms] |
| Digital Life log format 1, get_event | 2 us [1 us-2 us] | 15 us [3 us-28 us] | 5 us [4 us-6 us] |

### 3.4 Full read and projection, realistic dataset (reader code unchanged; measured for completeness)

| | N=10,000 | N=100,000 | N=1,000,000 |
|---|---|---|---|
| KORE v4 full read | 22.2 ms [21.4 ms-26.1 ms] | 895.0 ms [480.8 ms-1.24 s] | 20.18 s [17.50 s-22.85 s] |
| KORE default full read | 21.2 ms [19.3 ms-24.0 ms] | 540.1 ms [330.0 ms-682.3 ms] | 18.24 s [17.06 s-19.42 s] |
| paired ratio v4 / default, full read | 1.01 [0.92-1.35] | 1.66 [1.46-1.82] | 1.12 [0.90-1.34] |
| KORE v4 projection (events.kind) | 6.5 ms [5.5 ms-6.8 ms] | 82.6 ms [65.0 ms-194.7 ms] | 2.49 s [1.61 s-3.37 s] |
| KORE default projection | 4.2 ms [4.0 ms-5.6 ms] | 112.2 ms [93.5 ms-130.3 ms] | 2.68 s [1.72 s-3.65 s] |
| paired ratio v4 / default, projection | 1.62 [0.98-1.63] | 0.63 [0.58-2.08] | 0.93 [0.92-0.93] |
| Parquet zstd (default level) full read / projection | 12.9 ms [12.4 ms-19.5 ms] / 12.7 ms [12.1 ms-15.1 ms] | 57.4 ms [52.4 ms-67.5 ms] / 170.3 ms [169.2 ms-191.3 ms] | 3.84 s [3.44 s-4.25 s] / 5.29 s [5.20 s-5.38 s] |
| DuckDB .duckdb after CHECKPOINT full read / projection | 24.7 ms [18.3 ms-50.9 ms] / 13.7 ms [11.0 ms-21.3 ms] | 258.6 ms [243.3 ms-278.8 ms] / 205.2 ms [172.2 ms-265.5 ms] | 5.48 s [5.30 s-5.66 s] / 3.77 s [3.23 s-4.31 s] |
| SQLite full read / projection | 61.3 ms [58.9 ms-63.7 ms] / 17.9 ms [16.5 ms-19.1 ms] | 973.3 ms [898.6 ms-1.02 s] / 356.3 ms [354.9 ms-444.6 ms] | 25.98 s [23.08 s-28.87 s] / 9.33 s [9.03 s-9.62 s] |

Key index (realistic): N=10,000: 10,000 keys over 10 files, 19,670 bytes (1.97 B/key); N=100,000: 100,000 keys over 100 files, 229,371 bytes (2.29 B/key); N=1,000,000: 1,000,000 keys over 1000 files, 2,637,134 bytes (2.64 B/key).

### 3.5 Size, uniform dataset

| | N=10,000 | N=100,000 |
|---|---:|---:|
| KORE v4 (data files + dictionary + MANIFEST; same bytes before and after) | 0.69 MB, 69.4 B/event | 6.15 MB, 61.5 B/event |
| key index object (.kidx) alone | 0.02 MB, 2.0 B/event | 0.23 MB, 2.3 B/event |
| **KORE v4 + key index** | 0.71 MB, 71.4 B/event | 6.38 MB, 63.8 B/event |
| KORE v4, `CompactOptions::fast()` (explicit, other bytes) | 0.75 MB, 74.6 B/event | 6.72 MB, 67.2 B/event |
| KORE v4, `fast().with_zstd_level(3)` (explicit, other bytes) | 0.75 MB, 74.6 B/event | 6.72 MB, 67.2 B/event |
| KORE default writer | 0.99 MB, 98.5 B/event | 9.89 MB, 98.9 B/event |
| KORE v4, zstd level 19 | 0.69 MB, 69.4 B/event | 6.15 MB, 61.5 B/event |
| Parquet zstd (default level) | 0.77 MB, 76.8 B/event | 7.20 MB, 72.0 B/event |
| DuckDB .duckdb after CHECKPOINT | 2.37 MB, 237.2 B/event | 8.40 MB, 84.0 B/event |
| SQLite | 3.52 MB, 351.8 B/event | 35.43 MB, 354.3 B/event |
| Digital Life log format 3 | 0.68 MB, 67.8 B/event | 6.80 MB, 68.0 B/event |

### 3.6 Write, uniform dataset (median [min-max] over repetitions)

| | N=10,000 | N=100,000 |
|---|---|---|
| KORE default writer, one segment at a time | 1.24 s [1.05 s-1.43 s] | 9.35 s [8.57 s-9.89 s] |
| **KORE v4** (`to_bytes_compact`; writer code and bytes as bbbefd2c), one segment at a time | 5.08 s [4.93 s-5.16 s] | 42.68 s [40.22 s-47.47 s] |
| paired ratio v4 / default | 4.10 [3.61-4.70] | 4.57 [4.07-5.54] |
| **KORE v4 batch API** (`to_bytes_compact_many`, all segments at once, same bytes) | 2.96 s [2.66 s-3.12 s] | 31.31 s [24.58 s-39.25 s] |
| paired ratio v4 batch / v4 one at a time | 0.60 [0.52-0.61] | 0.73 [0.61-0.83] |
| paired ratio v4 batch / default one at a time | 2.52 [1.86-2.83] | 3.35 [2.49-4.58] |
| KORE default writer, all segments at once (fair reference for the batch row) | 1.57 s [1.21 s-1.83 s] | 13.71 s [13.56 s-13.83 s] |
| paired ratio v4 batch / default batch | 1.89 [1.70-2.20] | 2.31 [1.79-2.84] |
| KORE v4 `fast()` (explicit, other bytes), one segment at a time | 2.09 s [1.76 s-2.90 s] | 22.61 s [20.62 s-22.98 s] |
| paired ratio v4 fast / v4 | 0.41 [0.35-0.59] | 0.51 [0.48-0.53] |
| paired ratio v4 fast / default | 1.47 [1.42-2.77] | 2.42 [2.09-2.68] |
| KORE v4 `fast().with_zstd_level(3)` (explicit, other bytes), one at a time | 2.20 s [1.92 s-2.44 s] | 20.22 s [19.73 s-23.33 s] |
| paired ratio v4 fast+level 3 / v4 | 0.45 [0.38-0.47] | 0.49 [0.47-0.49] |
| paired ratio v4 fast+level 3 / default | 1.71 [1.55-2.10] | 2.16 [2.00-2.72] |
| KORE v4 zstd level 19 (one run) | 4.89 s | 41.81 s |
| key index build (keys in memory + sha256 of every events file + write) | 120.9 ms [61.3 ms-166.7 ms] | 146.0 ms [126.5 ms-216.4 ms] |
| Parquet zstd (default level) | 386.5 ms | 1.39 s |
| DuckDB .duckdb after CHECKPOINT | 940.8 ms | 3.83 s |
| SQLite | 3.59 s | 63.29 s |
| Digital Life log format 3, build via log API (fsync per commit) | 351.9 ms | 9.23 s |

### 3.7 Point lookup by event id, uniform dataset (text of one event; per operation)

| | N=10,000 | N=100,000 |
|---|---|---|
| KORE v4 before: scan of the event_id column of each segment, then text | 35.1 ms [27.8 ms-40.4 ms] | 202.2 ms [196.2 ms-224.9 ms] |
| **KORE v4 + key index, warm** (index and dictionary in memory; one file read, sha256-checked) | 10.1 ms [5.2 ms-20.4 ms] | 7.3 ms [4.9 ms-10.9 ms] |
| paired ratio indexed / scan | 0.29 [0.19-0.50] | 0.0361 [0.0249-0.0483] |
| KORE v4 + key index, warm, `Verify::Length` (file CRCs only) | 5.6 ms [5.5 ms-10.7 ms] | 3.6 ms [1.3 ms-4.2 ms] |
| KORE v4 + key index, index and dictionary loaded from disk and verified in every operation | 17.8 ms [16.5 ms-31.3 ms] | 39.8 ms [33.6 ms-40.3 ms] |
| KORE v4 + key index, missing key (warm) | 2 us [2 us-2 us] (0 file reads for 100 keys) | 2 us [2 us-2 us] (0 file reads for 100 keys) |
| KORE default files + key index, warm | 8.4 ms [3.5 ms-9.9 ms] | 6.4 ms [4.7 ms-8.7 ms] |
| KORE default files, scan | 35.5 ms [26.1 ms-40.6 ms] | 211.3 ms [210.8 ms-237.9 ms] |
| key index load + verify (read file, sha256, structure) | 909 us [660 us-1.1 ms] | 4.1 ms [3.5 ms-6.4 ms] |
| Parquet zstd (default level) | 27.1 ms [21.2 ms-28.6 ms] | 88.8 ms [87.2 ms-90.3 ms] |
| DuckDB .duckdb after CHECKPOINT | 10.2 ms [5.4 ms-11.3 ms] | 26.3 ms [23.5 ms-28.5 ms] |
| SQLite | 64 us [62 us-70 us] | 69 us [67 us-74 us] |
| Digital Life log format 3, get_event (in-memory id index built by open) | 4.4 ms [4.0 ms-6.3 ms] | 8.0 ms [7.9 ms-10.5 ms] |
| Digital Life log format 1, get_event | 2 us [2 us-3 us] | 3 us [3 us-6 us] |

### 3.8 Full read and projection, uniform dataset (reader code unchanged; measured for completeness)

| | N=10,000 | N=100,000 |
|---|---|---|
| KORE v4 full read | 279.6 ms [116.5 ms-284.3 ms] | 1.25 s [1.21 s-1.39 s] |
| KORE default full read | 651.9 ms [492.5 ms-696.6 ms] | 2.14 s [2.06 s-2.55 s] |
| paired ratio v4 / default, full read | 0.40 [0.18-0.58] | 0.57 [0.49-0.67] |
| KORE v4 projection (events.kind) | 83.4 ms [7.5 ms-102.1 ms] | 316.8 ms [293.8 ms-334.4 ms] |
| KORE default projection | 62.0 ms [7.5 ms-5.97 s] | 422.2 ms [324.3 ms-517.9 ms] |
| paired ratio v4 / default, projection | 0.1203 [0.0171-11.1602] | 0.79 [0.61-0.91] |
| Parquet zstd (default level) full read / projection | 67.8 ms [19.9 ms-71.2 ms] / 67.2 ms [17.2 ms-76.0 ms] | 146.0 ms [97.7 ms-157.7 ms] / 464.3 ms [454.7 ms-493.6 ms] |
| DuckDB .duckdb after CHECKPOINT full read / projection | 83.6 ms [78.3 ms-89.2 ms] / 81.1 ms [12.8 ms-84.6 ms] | 334.2 ms [331.3 ms-468.9 ms] / 506.8 ms [486.4 ms-584.6 ms] |
| SQLite full read / projection | 230.9 ms [196.0 ms-269.3 ms] / 86.3 ms [74.1 ms-88.2 ms] | 1.97 s [1.91 s-2.05 s] / 631.2 ms [591.9 ms-684.9 ms] |

Key index (uniform): N=10,000: 10,000 keys over 10 files, 19,674 bytes (1.97 B/key); N=100,000: 100,000 keys over 100 files, 229,377 bytes (2.29 B/key).


### 3.9 Supplementary measurements (separate runs, same machine, also under load)

Lookup step by step (`examples/prof_lookup.rs`): the 100k events segments re-encoded as version 4 with the dictionary,
200 ids x 2, medians, machine 95-100 % busy. These were taken after `find_in` was changed to decode the key column first
and the value column only on a hit, which is the code that was benchmarked.

| dataset | candidates | read file | sha256 | decode event_id | decode text | **lookup total** (`Verify::Length`) |
|---|---|---|---|---|---|---|
| realistic 100k | 2.8 us | 231 us | 26 us | 103 us | 411 us | **852 us** (748 us) |
| uniform 100k | 3.1 us | 305 us | 20 us | 121 us | 312 us | **840 us** (765 us) |

The time goes into the file read (Windows `std::fs::read` of one 60-80 KB segment) and into decompressing the whole
`text` column of that segment. The index itself takes a few microseconds.

Index load + verify, synthetic 1M keys over 1000 files (`examples/prof_index_load.rs`, 5 loads each):

* bit-by-bit Rice decoder (the one used in the benchmark run): 53.5-67.2 ms.
* after commit f8c11813 (56 bits at a time, same format): 14.9-79.7 ms, typically 15-22 ms.
* 100k keys: 1.0-1.1 ms. `candidates`: 1.9-2.1 us.

The benchmark's 1M "load + verify" (103-115 ms) and "open each op" (100-102 ms) rows were taken with the older decoder
and were not re-run.

## 4. Tests

* kore-store: all tests green, 76 = the 63 of bbbefd2c + 13 new:
  * `tests/keyindex.rs` (10): round trip over v2, v3, v4 and v4-with-dictionary files (every key found at its
    file and row, values right; deterministic; keys-in-memory and decode-the-file builds give the same object);
    2000 missing keys never hit and almost never read a file; duplicates across files, null keys, keys that differ
    only by a trailing NUL, empty and single-key indexes, non-string key column refused; 60 000 keys over 1200 files
    (11-bit file numbers); **every byte of the index flipped with 3 masks and truncated at every length: refused**;
    every byte flipped with the sha256 recomputed (a forger who knows the format): never a panic and never a row
    with another key; changed / truncated / replaced data files refused (sha256 pin), what `Verify::Length` catches
    and what it does not; data readers refuse the object clearly, unknown index version / hash / flags refused;
    the bbbefd2c-era fixtures `sample_v2_6e2e6a7b.kore` and `sample_v3_6e2e6a7b.kore` are indexed and still read;
    time travel: a KVLG log with three versions, one index per version, each returns its own version's text, an
    old index used against the newest entry is refused; KH64 values pinned.
  * `tests/v4_writer.rs` (3): `tests/v4_writer_bytes.txt` holds the length and sha256 of 19 version-4 cases
    written by the **unmodified bbbefd2c writer** (real build from a detached bbbefd2c worktree with a scratch
    example that includes `tests/common/v4_cases.rs`; not committed); the branch writer reproduces all 19 byte for
    byte (Auto, shared dictionaries, levels 3/9/19, payloads below and above 256 KiB, explicit transforms and
    stages, empty block), also with `small_zstd_level(19)` given explicitly; the batch API gives the same 19
    outputs; small levels 12/9/3/1 and `fast()` round-trip and are deterministic.
  * the existing fixtures tests (default and version-3 writers byte-identical to fixtures written by older code)
    still pass unchanged; in the benchmark every default-writer segment is rewritten byte-identical and every
    version-4 output of the batch writer equals the one-at-a-time output.
* kore-self against this branch's kore-store (path-limited copy `git checkout fileformat-improve-3 -- kore-store`
  into a scratch detached worktree, nothing committed there), at ecf5f695 (the Digital Life commit with the versioned
  log, used by the harness):
  * with kore-store at f8beb192: **443 passed, 0 failed** (357 unit + 4 cli_bind + 5 e2e_advisor + 4 e2e_being +
    3 e2e_chat + 1 e2e_ingest + 68 e2e_learn + 1 e2e_versioned);
  * with the final kore-store f8c11813 (v4 speculation removed and the index decoder changed since): 357 unit + cli_bind 4 + e2e_ingest 1 +
    e2e_versioned 1, **all passed**; the other e2e suites were not re-run.
  * at the current main-Digital-Life-AI tip 5df188ea (with kore-store at f8beb192): 38 passed, 0 failed.
* Old readers, real binaries: the bbbefd2c reader on the files of `examples/write_v4_sample.rs`:
  ```
  sample_index.kidx: ERR invalid argument: invalid KORE magic bytes
  sample_v2.kore: OK rows=200 cols=3
  sample_v4.kore: OK rows=200 cols=3
  sample_v4_fast.kore: OK rows=200 cols=3
  sample_v4_dict.kore: ERR ... needs shared dictionary <id> (not supplied; use KoreReader::from_bytes_with_dicts)
  <id>.kdic: ERR invalid argument: invalid KORE magic bytes
  ```
  (files written with `fast()` are ordinary version-4 files and read with the bbbefd2c reader.)

## 5. Not measured

* Cold lookups with an empty OS file cache (needs administrator rights to flush the standby list); a quiet
  machine (all times were taken while other jobs kept the CPU busy, section 2).
* The key index inside the Digital Life log (MANIFEST entry, incremental maintenance per commit): not wired into
  kore-self. A log that commits one segment at a time would either rebuild the index (it is built from the keys in
  memory plus one sha256 per file, see the build row) or keep several index objects and look up in each; neither
  was implemented or measured here.
* Range or prefix queries (the index is a hashed set: equality only) and non-string key columns.
* Lookups that return many columns or many rows (only `text` of one event was measured).
* zstd multithreading inside one payload (it changes the bytes and was not tried).
* The CSV, compressed CSV, ORC and Arrow IPC rows of docs/kore_size.md (not repeated in this run).
* Memory use of the index and of the batch writer (the batch writer holds all encoded files in memory).
