# kore-store read/write speed work (branch `fileformat-improve`)

Goal: reads of the owner's KORE format (`kore-store` on `fileformat`, a980727d) at least as fast as the old
Digital Life copy of `kore-store` (LZ4 only, branch `main-Digital-Life-AI`), without giving up the ~45 %
size advantage, compatibility or tamper detection. Everything below was measured on one shared Windows
machine that was busy with other jobs most of the time; load is reported next to the numbers.

Compared builds:

| name | what |
|---|---|
| old | `kore-store` 0.1.0 as on `origin/main-Digital-Life-AI` d8997ba8 (v2 container, Raw/Rle/Delta/Dict/NanRaw/Lz4, sequential, no checksums) |
| a980727d | `kore-store` 1.6.0 on `origin/fileformat` a980727d (adds Zstd, CRC32, row groups, version log, projection, rayon) |
| this branch | a980727d + the commits listed in section 3 (the v3 codec commits 274b6a58..5ffd975f do not touch the default v2 path) |

`kore-core` is identical in all three.

## Summary (paired runs, section 6)

Paired ratios are this branch / reference, per round, median [min-max]. Below 1 means faster or smaller.
For build events/s, below 1 means slower.

| | 10k | 100k | 1M |
|---|---|---|---|
| store: full scan, 1000-row files vs old | 1.00 [0.77-1.37] | 1.04 [0.89-1.07] | 0.98 [0.93-1.03] |
| store: open (read_file) of 1000-row files vs old | 1.02 [0.88-1.62] | 1.02 [0.99-1.37] | 1.05 [1.04-1.05] |
| store: single-file open / scan vs old | 0.68 / 0.62 | 0.59 / 0.57 | 0.54 / 0.59 |
| store: projection / point lookup (1000-row files) vs old | 0.12 / 0.38 | 0.12 / 0.25 | 0.11 / 0.26 |
| store: write, 1000-row files vs a980727d | 0.40 | 0.44 | 0.40 |
| log format 1: open() vs old | 1.11 [0.92-1.37] | 1.21 [1.09-1.31] | 1.06 [0.95-1.26] |
| log format 1: verify() vs old | 1.28 [0.91-1.99] | 1.23 [0.73-1.58] | 0.56 [0.49-0.66] |
| log format 1: open_at(mid) vs old | 0.96 [0.59-1.28] | 1.10 [0.88-1.42] | 0.54 [0.44-0.56] |
| log format 1: open_at(latest) vs old | 1.26 [0.74-1.73] | 1.00 [0.83-1.19] | 0.66 [0.55-0.68] |
| log format 1: open() / verify() / open_at(mid) vs a980727d | 0.80 / 0.69 / 0.60 | 0.62 / 0.55 / 0.38 | 0.22 / 0.09 / 0.09 |
| log format 1: bulk build events/s vs old / vs a980727d | 0.91 / 1.16 | 0.83 / 1.17 | 0.92 / 1.15 |
| size vs a980727d | 1.00 (byte-identical files) | 1.00 | 1.00 |
| size vs old (store files; log dir) | 0.57; 0.61 | 0.57; 0.61 | 0.57; 0.61 |

Targets:
- **Size not larger than a980727d: met** (identical bytes).
- **Writes not slower than a980727d: met.** Writes are still slower than the old store: 1.1-1.9x at store level, about 0.83-0.92x of its build rate in the log. The writer must still try both LZ4 and ZSTD to produce the same bytes as a980727d.
- **Reads not slower than the old store:**
  - Met for the single-file layout, projection, point lookups, store-level as-of and all 1M log reads.
  - The full scan of 1000-row files is a tie (about 1.00x).
  - **Not met for log format 1 `open()` at 10k and 100k** (1.11x and 1.21x the old store) **and `verify()` at 10k and 100k** (1.28x and 1.23x, ranges straddle 1). `open_at(latest)` at 10k is 1.26x (range 0.74-1.73).
  - Why: each 1000-row segment's ZSTD columns decode about 4x slower than the same bytes with LZ4 (about 300 vs 80 us per segment, single thread). That is the cost of the 40 % smaller files and is not removable without changing the writer's codec choice.
- **The 1M slowdown found by the other benchmark (a980727d: open 5.2x, verify 6.4x, open_at 5.9x the old store) is fixed.** Here a980727d was 4.5-9x slower than old at 1M and this branch is 0.54-1.06x the old store; see C9.

Load: total CPU sampled around every process (typeperf, 3 x 1 s) was median 67 %, min 59 %, max 100 % (156 samples). All 78 processes exited 0.

## 1. Where read time went (causes, with evidence)

Profiles: `kore-store/examples/prof_read.rs` (Digital Life `events` shape from `examples/support/events_gen.rs`:
7 string columns incl. a 64-char hex id, two ISO timestamps and free text, plus a bool) and
`examples/bench_store.rs`. "seg" = files of 1,000 rows (what kore-self log format 1 writes per batch,
about 97 KB each with ZSTD, 170 KB with the old LZ4 store); "one" = all rows in one file. Single-run
numbers in this section are from short runs on the busy machine; they show where time goes, the paired
tables above are the comparison.

| # | cause | evidence (before -> after) | fixed by |
|---|---|---|---|
| C1 | CRC-32 computed with the `crc` crate's default byte-at-a-time table (~370 MB/s here) | CRC of all column payloads of the 100k-row file: 21-23 ms -> 2.0-2.1 ms | slice-by-16 table, same checksum |
| C2 | the CRC check of all columns ran on one thread before the parallel decode started | the 21-23 ms of C1 were serial on the critical path of every read | checksum inside the per-column parallel task |
| C3 | every ZSTD column used `zstd::decode_all`: a new decompression context, a 128 KB read buffer, an extra copy and a doubling output `Vec` per column | per seg file (sum over its columns, single thread, min of 300): 547-683 us -> 298-320 us; 100k one-file: 59-63 ms -> 48-53 ms | per-thread reused `DCtx`, output decoded straight into one pre-sized buffer, `decode_all` fallback for anything unusual |
| C4 | string columns: UTF-8 validated per value; large columns built on one thread (allocation-bound) | per seg file: 368-418 us -> 270-312 us; 100k text-heavy file: 45 ms -> 27 ms | validate the column's bytes once; build columns of 32k+ values with rayon |
| C5 | readable-trailer marker searched with `windows().rposition`: a byte-by-byte scan of the whole file whenever the file has no trailer (row-group parts, `KORE_READABLE_MODE=none`, every `read_at_version` of a plain file via the `KVER` search) | a980727d, 100k-row file without trailer: projection of one column 26.3 ms vs 0.57 ms with trailer; this branch 1.97 ms | `memchr::memmem::rfind` (same last-occurrence result) |
| C6 | `read_file` memory-maps every file; on Windows mapping + faulting a small file costs more than one read | 10 seg files, `open - scan` (file cost only): old (fs::read) 2.7-3.4 ms, a980727d 5.1-10.4 ms | files < 8 MiB are read, larger ones still mapped |
| C7 | row-group files: groups decoded one after another, and the merge cloned every value | 100k rows in 10k-row groups, full read: 543-570 ms -> 332-375 ms (machine saturated by builds) | groups decoded in parallel, columns moved when all groups agree on type/dictionary |
| C9 | **the 1M log slowdown**: every ZSTD column of every segment was decompressed into a freshly allocated, growing buffer that was dropped right after its values were copied out; kore-self loads ~1000 segments on 8 threads, and under that concurrency the allocations/page faults dominated | a980727d at 1M, `open_at(latest)`: 5.96 s on 8 threads vs 3.74 s with `RAYON_NUM_THREADS=1` (parallelism made it slower). Not nested rayon: disabling the inner parallel decode changed nothing (1.84 vs 1.90 s). Decoding into a per-thread scratch buffer: 1.6-1.9 s -> 0.90-0.99 s (old store 1.02-1.13 s, same files, interleaved). The store-only decode of the 1000 events segments was already at parity before this fix (sequential over files: old 716 ms, branch 678 ms, a980727d 1487 ms) | per-thread reused output buffer (`zstd_decode_into`), released if it grew past 8 MiB |
| C8 | small files: thread hand-off for files of a few KB | not measurable above noise; kept columns under 16 KiB stay on the calling thread | threshold |
| W1 | writer: a new ZSTD compression context per column | part of the write-time gain below | per-thread reused `CCtx` driven through the same streaming calls (byte-identical frames) |

Not a cause that could be fixed without changing bytes or size:

- **ZSTD vs LZ4 decode speed.** The same decompressed bytes of one seg file decode in about 75-80 us with
  LZ4 and 300-320 us with ZSTD (after C3). The writer keeps whichever of LZ4/ZSTD is smaller, and for these
  columns that is always ZSTD; that choice is where the size advantage comes from. For small files this is
  the remaining gap to the old store (section 5).
- **Strings are one heap allocation each** (`ColumnData::Str(Vec<Option<String>>)`), the same in all three
  builds; the old store pays it too.
- The writer still tries both LZ4 and ZSTD per column (needed to produce the same bytes as a980727d).

## 2. What did not change

- The default writer (`KoreWriter::to_bytes`, `write_to`, `write_file`), `rowgroups::write` and
  `versioned::append_*` produce byte-identical output to a980727d (tests below, and the cross-check in
  section 4).
- File formats, codecs, version numbers: unchanged. No reader/writer/segment format code was removed.
- Tamper detection: a column whose CRC does not match is reported as `checksum mismatch in column '<name>'`
  (first damaged column in file order) and is never decoded, as before.
- Public API: unchanged; `compress::zstd_decode_all(src, size_hint)` is new (additive).

## 3. Commits (on `fileformat-improve` only, nothing pushed)

```
8c9a7ff4 scripts: 40 tamper trials at every size
7559ba17 kore-store reader: decompress ZSTD columns into a per-thread scratch buffer
371d9bfc docs: fileformat_speed.md (causes, fixes, compatibility; paired runs not yet done)
5b4c3f11 scripts: tamper counts in the log tables
b92b4d9e kore-store: read_hash example (cross-store read check, builds against every store copy)
a6706e19 kore-store: bench_rowgroups example (row-group container read time)
faa5d7e1 scripts: paired interleaved benchmark driver and tables for the store speed work
47477f51 kore-store reader: checksum and decode each column in one parallel task
62992a6c kore-store: bench_store (builds against old/a980727d/new stores) and prof_read examples
a677da8a kore-store: events-shaped fixtures from a980727d and speed-work compatibility tests
031a6415 kore-store: track the v2 compatibility fixtures that ed5cb21d's tests read
9b060b65 kore-store: read_file reads files under 8 MiB instead of memory-mapping them
68d6a59c kore-store: build large string columns on several threads
77d335b9 kore-store rowgroups: decode groups in parallel, move columns when concatenating
bf99240c kore-store reader: checksum+decode columns together in parallel, vectorised marker search
acc25186 kore-store: faster CRC32, ZSTD and string decode primitives with identical results
(+ this docs commit)
```

## 4. Compatibility proof

Tests (`cargo test --release --offline -p kore-store -p kore-core`, also `cargo test` in debug for
`kore-store`): all green, see section 6 for counts.

- `tests/codecs.rs` (from ed5cb21d): the 7 v2 fixtures written by the unmodified a980727d writer still read
  identically, and `KoreWriter::to_bytes` / `rowgroups::write` reproduce them byte for byte. **These fixture
  files had never been committed** (the repository-wide `*.kore` ignore rule hid them); commit 031a6415 adds
  them unchanged after re-generating them from an untouched a980727d checkout: all 7 byte-identical.
- `tests/speed_compat.rs` (new) with three larger fixtures written by the unmodified a980727d writer
  (`events_1000.kore`, `events_log.kore`, `events_rowgroups.kore`; every column ZSTD, text spanning several
  ZSTD blocks): decode to the generated data, the default writer and row-group writer reproduce them
  byte for byte, as-of reads of the version log, projection, and every sampled payload byte flip (every
  997th byte of every column) reported as a checksum mismatch; a damaged column that a projection does not
  request is not touched.
- `compress.rs` unit tests compare each new primitive with the code it replaces: CRC on 0..65,537 byte
  inputs plus the standard check value; ZSTD encode byte-identical to `zstd::encode_all` (incl. context
  reuse on one thread and fresh contexts on others, inputs crossing the 128 KB block size); ZSTD decode equal
  to `zstd::decode_all` on valid, two-frame, truncated, trailing-junk, bit-flipped, content-size-in-header
  and empty input (same value or both errors); string decoders equal to the old ones on valid input, every
  truncation and 3 flips of each of the first 600 bytes, invalid UTF-8 and a char split across two values,
  and on the parallel path.
- `rowgroups` unit test: moving merge equals the value-copying merge.
- Cross-store read check (`examples/read_hash.rs`, built against each store; 10k rows, seg and one layouts,
  default and `KORE_READABLE_MODE=none`):

  - this branch's writer produces the same bytes as a980727d's (identical SHA-256 over all files) for
    seg/one, in both the default and `none` trailer modes;
  - files written by the old store, a980727d and this branch read on this branch with the same content
    hashes as on their own store; the 10 fixtures read identically on a980727d and this branch;
  - the old store cannot read anything a980727d or this branch writes (`unknown compression 6`). That was
    already true of a980727d and is unchanged.

- kore-self (Digital Life) built against this branch's `kore-store` in a scratch worktree of
  `origin/main-Digital-Life-AI` d8997ba8 (path-limited copy of `kore-store/`, nothing committed there):
  `cargo test --release --offline -p kore-self`: 381 tests passed, 0 failed (302 + 61 + 5 + 4 + 4 + 3 + 1 + 1), re-run on the final store (7559ba17).

## 5. Behaviour changes

- None in results or bytes. Timing/threading only: reads of files with 16 KiB or more of kept column data
  now checksum and decode columns on the rayon pool (a980727d decoded in parallel too, but checksummed
  serially first); large string columns and row groups use the pool as well. `read_file` of files under
  8 MiB reads the file instead of mapping it (a file being replaced while it is read is now read whole or
  not at all, instead of through a live mapping).
- Each thread that has read a ZSTD column keeps one decompression context and a scratch buffer of up to 8 MiB (larger buffers are released after use), so steady-state memory is slightly higher.
- With several columns damaged, a980727d never decoded any column; this branch may decode the undamaged
  columns in parallel before reporting the (same) first checksum mismatch. The returned error is the same.

## 6. Results of the paired runs

Commands: `sh scripts/fileformat_speed_paired.sh store OUT "10000 100000" 5`, then `"1000000" 3`, and the same with
`log OUT ... DATADIR`. Tables: `python -I scripts/fileformat_speed_tables.py store|log OUT`.

Within a round the three builds ran back to back, and the order rotated every round. A cell is the median [min-max] over rounds of each process's own median. Per process, bench_store used 9/5/3 repetitions at 10k/100k/1M and 20/20/5 lookups; bench_log_formats used `--build-reps 2 --reps 5 --tamper-trials 40` (the harness skips tamper above 100k). Raw JSON was not committed. Every query `check` in every log run (12 per run, 39 runs) was identical across the three builds.

Data: store level, `examples/support/events_gen.rs` (seed 7). Log level, `kore-self/benches/format_bench_gen.py` (default seed). SHA-256 prefixes: data_10000 0ab905aef9d9, data_100000 a7c1611c5486, data_1000000 a25a716ecb1d. The exes were built from: old = d8997ba8; a980727d = d8997ba8 with `kore-store` from a980727d; this branch = d8997ba8 with `kore-store` from this branch at 7559ba17. All are release builds.

### Store level (`bench_store`; ms; seg = 1000-row files, one = single file)

### n = 10,000 (5 rounds)

| metric | old store | a980727d | this branch | new/old | new/a98 |
|---|---|---|---|---|---|
| seg.write_ms | 6.3 [5.23-6.87] | 27.6 [24.6-57.2] | 11 [9.95-11.2] | 1.74 [1.48-2.14] | 0.395 [0.192-0.451] |
| seg.open_ms | 10.9 [8.25-11.4] | 22.1 [21.5-22.4] | 10.5 [9.61-13.4] | 1.02 [0.88-1.62] | 0.489 [0.434-0.597] |
| seg.scan_ms | 6.55 [6.11-8.76] | 16.3 [15.2-18.2] | 6.86 [6.11-8.41] | 1 [0.774-1.37] | 0.447 [0.373-0.484] |
| seg.proj_ms | 7.3 [6.71-8.87] | 2.27 [1.71-2.62] | 0.879 [0.814-0.98] | 0.12 [0.0936-0.143] | 0.375 [0.365-0.559] |
| seg.lookup_ms | 119 [105-123] | 104 [101-106] | 44.4 [38.6-49.3] | 0.381 [0.317-0.422] | 0.42 [0.372-0.489] |
| seg.asof_ms | - | 1.57 [1.48-1.71] | 0.71 [0.658-0.984] | - | 0.439 [0.416-0.648] |
| one.write_ms | 9.85 [9.38-10.9] | 15.1 [13.6-15.7] | 12.8 [10.5-13.1] | 1.17 [1.12-1.34] | 0.813 [0.694-0.947] |
| one.open_ms | 11.7 [10.5-12.6] | 13.2 [12.4-19.4] | 7.54 [7.33-8.45] | 0.677 [0.581-0.735] | 0.593 [0.378-0.637] |
| one.scan_ms | 9.92 [9.54-11.2] | 12.1 [11.1-12.8] | 6.16 [5.35-6.76] | 0.621 [0.482-0.65] | 0.512 [0.419-0.599] |
| one.proj_ms | 10.9 [10.5-11.8] | 0.164 [0.0953-0.183] | 0.0354 [0.033-0.0525] | 0.00338 [0.003-0.00486] | 0.256 [0.19-0.549] |
| one.lookup_ms | 442 [435-450] | 338 [321-357] | 169 [159-174] | 0.381 [0.353-0.393] | 0.492 [0.457-0.536] |
| one.asof_ms | - | - | - | - | - |
| seg.bytes | 1.71e+06 [1.71e+06-1.71e+06] | 9.71e+05 [9.71e+05-9.71e+05] | 9.71e+05 [9.71e+05-9.71e+05] | 0.569 [0.569-0.569] | 1 [1-1] |
| one.bytes | 1.68e+06 [1.68e+06-1.68e+06] | 8.67e+05 [8.67e+05-8.67e+05] | 8.67e+05 [8.67e+05-8.67e+05] | 0.516 [0.516-0.516] | 1 [1-1] |

### n = 100,000 (5 rounds)

| metric | old store | a980727d | this branch | new/old | new/a98 |
|---|---|---|---|---|---|
| seg.write_ms | 60.8 [55.5-64.4] | 257 [251-277] | 112 [104-116] | 1.89 [1.71-2.02] | 0.438 [0.373-0.463] |
| seg.open_ms | 102 [96.5-106] | 211 [195-224] | 104 [98.2-132] | 1.02 [0.987-1.37] | 0.511 [0.47-0.627] |
| seg.scan_ms | 60.3 [59.9-73.6] | 137 [136-187] | 64.6 [62.6-68.4] | 1.04 [0.889-1.07] | 0.475 [0.343-0.503] |
| seg.proj_ms | 70.7 [67-76.6] | 24.7 [24-24.9] | 8.92 [8.23-9.35] | 0.122 [0.113-0.14] | 0.363 [0.333-0.376] |
| seg.lookup_ms | 973 [960-1.01e+03] | 688 [685-740] | 239 [232-249] | 0.246 [0.234-0.255] | 0.338 [0.319-0.364] |
| seg.asof_ms | - | 1.81 [1.4-2.5] | 0.819 [0.612-1.91] | - | 0.448 [0.245-1.37] |
| one.write_ms | 119 [113-143] | 146 [142-159] | 126 [123-141] | 1.06 [0.941-1.09] | 0.841 [0.798-0.903] |
| one.open_ms | 119 [117-135] | 109 [108-113] | 73.8 [63.9-89.9] | 0.588 [0.546-0.731] | 0.65 [0.588-0.817] |
| one.scan_ms | 103 [101-108] | 103 [94.3-111] | 61.5 [51.1-63] | 0.571 [0.5-0.611] | 0.579 [0.466-0.668] |
| one.proj_ms | 113 [108-122] | 0.617 [0.603-0.701] | 0.279 [0.201-0.313] | 0.00248 [0.00178-0.00274] | 0.457 [0.287-0.492] |
| one.lookup_ms | 4.15e+03 [4.04e+03-4.37e+03] | 2.85e+03 [2.74e+03-2.9e+03] | 1.68e+03 [1.68e+03-1.71e+03] | 0.408 [0.386-0.416] | 0.6 [0.579-0.614] |
| one.asof_ms | - | - | - | - | - |
| seg.bytes | 1.71e+07 [1.71e+07-1.71e+07] | 9.73e+06 [9.73e+06-9.73e+06] | 9.73e+06 [9.73e+06-9.73e+06] | 0.569 [0.569-0.569] | 1 [1-1] |
| one.bytes | 1.68e+07 [1.68e+07-1.68e+07] | 8.59e+06 [8.59e+06-8.59e+06] | 8.59e+06 [8.59e+06-8.59e+06] | 0.511 [0.511-0.511] | 1 [1-1] |

### n = 1,000,000 (3 rounds)

| metric | old store | a980727d | this branch | new/old | new/a98 |
|---|---|---|---|---|---|
| seg.write_ms | 658 [643-733] | 2.9e+03 [2.87e+03-3e+03] | 1.15e+03 [1.14e+03-1.19e+03] | 1.79 [1.56-1.8] | 0.401 [0.381-0.408] |
| seg.open_ms | 1.1e+03 [1.08e+03-1.13e+03] | 2.17e+03 [2.17e+03-2.21e+03] | 1.15e+03 [1.13e+03-1.18e+03] | 1.05 [1.04-1.05] | 0.531 [0.51-0.545] |
| seg.scan_ms | 678 [673-724] | 1.57e+03 [1.47e+03-1.6e+03] | 673 [662-700] | 0.984 [0.929-1.03] | 0.446 [0.421-0.45] |
| seg.proj_ms | 817 [769-821] | 240 [229-243] | 92.8 [91.4-95.3] | 0.113 [0.112-0.124] | 0.387 [0.376-0.416] |
| seg.lookup_ms | 1.72e+03 [1.66e+03-1.72e+03] | 1.13e+03 [1.13e+03-1.14e+03] | 443 [407-444] | 0.257 [0.236-0.268] | 0.39 [0.36-0.392] |
| seg.asof_ms | - | 1.46 [1.35-1.52] | 0.582 [0.564-0.672] | - | 0.397 [0.371-0.496] |
| one.write_ms | 1.18e+03 [1.17e+03-1.21e+03] | 1.49e+03 [1.45e+03-1.5e+03] | 1.26e+03 [1.24e+03-1.5e+03] | 1.07 [1.02-1.29] | 0.855 [0.847-1] |
| one.open_ms | 1.26e+03 [1.23e+03-1.52e+03] | 1.12e+03 [1.1e+03-1.14e+03] | 755 [682-795] | 0.539 [0.496-0.648] | 0.675 [0.619-0.696] |
| one.scan_ms | 1.21e+03 [1.15e+03-1.25e+03] | 1.08e+03 [1.06e+03-1.2e+03] | 705 [680-715] | 0.589 [0.546-0.615] | 0.64 [0.594-0.656] |
| one.proj_ms | 1.2e+03 [1.19e+03-1.27e+03] | 4.6 [4.21-5.28] | 2.68 [2.64-2.7] | 0.00224 [0.00207-0.00226] | 0.583 [0.51-0.627] |
| one.lookup_ms | 1.07e+04 [1.05e+04-2.28e+04] | 7.39e+03 [7.39e+03-7.52e+03] | 4.51e+03 [4.48e+03-4.61e+03] | 0.425 [0.198-0.43] | 0.611 [0.607-0.614] |
| one.asof_ms | - | - | - | - | - |
| seg.bytes | 1.71e+08 [1.71e+08-1.71e+08] | 9.74e+07 [9.74e+07-9.74e+07] | 9.74e+07 [9.74e+07-9.74e+07] | 0.57 [0.57-0.57] | 1 [1-1] |
| one.bytes | 1.68e+08 [1.68e+08-1.68e+08] | 8.59e+07 [8.59e+07-8.59e+07] | 8.59e+07 [8.59e+07-8.59e+07] | 0.512 [0.512-0.512] | 1 [1-1] |

CPU total % around runs: median 66.0, min 59, max 100 (78 samples); non-zero exit codes: 0

### kore-self log format 1 (`bench_log_formats --format 1`; seconds)

### n = 10,000 (5 rounds)

| metric | old store | a980727d | this branch | new/old | new/a98 |
|---|---|---|---|---|---|
| build events/s | 3.94e+04 [3.84e+04-4.11e+04] | 3.23e+04 [2.46e+04-3.35e+04] | 3.48e+04 [3.35e+04-3.92e+04] | 0.907 [0.816-0.992] | 1.16 [1.01-1.47] |
| open_s | 0.0145 [0.0141-0.0162] | 0.0212 [0.0204-0.0222] | 0.0165 [0.0149-0.0199] | 1.11 [0.918-1.37] | 0.803 [0.684-0.94] |
| open+latest snapshot_s | 0.0153 [0.0152-0.0217] | 0.0258 [0.021-0.0396] | 0.0174 [0.0153-0.0221] | 1.04 [0.811-1.45] | 0.765 [0.404-0.854] |
| verify_s | 0.0109 [0.00829-0.0119] | 0.0194 [0.0157-0.0388] | 0.0109 [0.0105-0.017] | 1.28 [0.914-1.99] | 0.69 [0.27-1.02] |
| get_event per op_s | 5.71e-07 [5.27e-07-6.7e-07] | 5.48e-07 [5.21e-07-6.1e-07] | 6.33e-07 [5.57e-07-7.96e-07] | 1.19 [0.838-1.51] | 1.08 [1.04-1.45] |
| as-of v1 open_at_s | 0.00245 [0.00176-0.00333] | 0.00286 [0.00273-0.00329] | 0.00225 [0.00209-0.00336] | 0.965 [0.674-1.38] | 0.803 [0.637-1.23] |
| as-of v5 open_at_s | 0.00467 [0.00389-0.0156] | 0.00835 [0.00702-0.0145] | 0.00469 [0.00449-0.00954] | 0.963 [0.591-1.28] | 0.596 [0.324-1.18] |
| as-of v10 open_at_s | 0.00844 [0.00757-0.0151] | 0.0154 [0.0132-0.0188] | 0.0101 [0.00877-0.0146] | 1.26 [0.742-1.73] | 0.729 [0.495-1] |
| size_bytes | 1.62e+06 [1.62e+06-1.62e+06] | 9.85e+05 [9.85e+05-9.85e+05] | 9.85e+05 [9.85e+05-9.85e+05] | 0.609 [0.609-0.609] | 1 [1-1] |
| events dir bytes | 1.55e+06 [1.55e+06-1.55e+06] | 9.04e+05 [9.04e+05-9.04e+05] | 9.04e+05 [9.04e+05-9.04e+05] | 0.584 [0.584-0.584] | 1 [1-1] |
| tamper detected | 40 [40-40] | 40 [40-40] | 40 [40-40] | 1 [1-1] | 1 [1-1] |
| tamper silent changes | 0 [0-0] | 0 [0-0] | 0 [0-0] | - | - |

### n = 100,000 (5 rounds)

| metric | old store | a980727d | this branch | new/old | new/a98 |
|---|---|---|---|---|---|
| build events/s | 3.65e+04 [3.58e+04-3.78e+04] | 2.66e+04 [2.28e+04-2.8e+04] | 3.11e+04 [2.96e+04-3.18e+04] | 0.831 [0.798-0.889] | 1.17 [1.08-1.37] |
| open_s | 0.134 [0.128-0.143] | 0.261 [0.244-0.287] | 0.163 [0.156-0.168] | 1.21 [1.09-1.31] | 0.621 [0.587-0.676] |
| open+latest snapshot_s | 0.158 [0.149-0.181] | 0.215 [0.21-0.234] | 0.162 [0.136-0.195] | 1.03 [0.795-1.27] | 0.756 [0.608-0.921] |
| verify_s | 0.0743 [0.0667-0.0873] | 0.153 [0.143-0.165] | 0.0874 [0.059-0.108] | 1.23 [0.732-1.58] | 0.553 [0.388-0.701] |
| get_event per op_s | 5.8e-07 [5.52e-07-6.36e-07] | 5.78e-07 [5.19e-07-9e-07] | 5.94e-07 [5.63e-07-6.8e-07] | 0.971 [0.951-1.21] | 0.974 [0.756-1.1] |
| as-of v1 open_at_s | 0.00267 [0.00201-0.00434] | 0.00397 [0.00368-0.00907] | 0.00274 [0.00228-0.00471] | 1.13 [0.829-1.76] | 0.671 [0.302-1.28] |
| as-of v50 open_at_s | 0.0413 [0.0372-0.0502] | 0.125 [0.118-0.131] | 0.0444 [0.041-0.0587] | 1.1 [0.884-1.42] | 0.376 [0.313-0.488] |
| as-of v100 open_at_s | 0.0985 [0.0872-0.116] | 0.253 [0.222-0.261] | 0.0963 [0.0876-0.109] | 1 [0.828-1.19] | 0.38 [0.338-0.445] |
| size_bytes | 1.62e+07 [1.62e+07-1.62e+07] | 9.89e+06 [9.89e+06-9.89e+06] | 9.89e+06 [9.89e+06-9.89e+06] | 0.611 [0.611-0.611] | 1 [1-1] |
| events dir bytes | 1.54e+07 [1.54e+07-1.54e+07] | 9.03e+06 [9.03e+06-9.03e+06] | 9.03e+06 [9.03e+06-9.03e+06] | 0.586 [0.586-0.586] | 1 [1-1] |
| tamper detected | 40 [40-40] | 40 [40-40] | 40 [40-40] | 1 [1-1] | 1 [1-1] |
| tamper silent changes | 0 [0-0] | 0 [0-0] | 0 [0-0] | - | - |

### n = 1,000,000 (3 rounds)

| metric | old store | a980727d | this branch | new/old | new/a98 |
|---|---|---|---|---|---|
| build events/s | 3.22e+04 [3.13e+04-3.33e+04] | 2.53e+04 [2.52e+04-2.58e+04] | 2.93e+04 [2.88e+04-2.97e+04] | 0.922 [0.882-0.922] | 1.15 [1.14-1.17] |
| open_s | 2.38 [2.02-2.64] | 11.5 [9.96-13.3] | 2.53 [2.5-2.55] | 1.06 [0.946-1.26] | 0.221 [0.188-0.254] |
| open+latest snapshot_s | 1.7 [1.68-1.92] | 3.53 [3.35-3.82] | 1.67 [1.63-1.77] | 0.961 [0.922-0.995] | 0.463 [0.439-0.529] |
| verify_s | 1.55 [1.43-1.75] | 9.74 [9.51-9.79] | 0.86 [0.855-0.949] | 0.556 [0.488-0.664] | 0.0899 [0.0878-0.0975] |
| get_event per op_s | 7.33e-07 [4.93e-07-8.59e-07] | 5.89e-07 [5.48e-07-6.59e-07] | 5.97e-07 [4.96e-07-5.99e-07] | 0.814 [0.697-1.01] | 0.906 [0.905-1.02] |
| as-of v1 open_at_s | 0.00279 [0.00276-0.00495] | 0.0183 [0.0172-0.0264] | 0.00381 [0.00351-0.00404] | 1.38 [0.709-1.45] | 0.204 [0.144-0.221] |
| as-of v500 open_at_s | 1.64 [1.61-1.81] | 9.79 [9.48-10.5] | 0.866 [0.796-0.912] | 0.537 [0.44-0.555] | 0.0913 [0.0759-0.0932] |
| as-of v1000 open_at_s | 3.89 [3.81-4.54] | 22.9 [21.9-24.5] | 2.56 [2.5-2.57] | 0.658 [0.552-0.676] | 0.112 [0.105-0.114] |
| size_bytes | 1.62e+08 [1.62e+08-1.62e+08] | 9.9e+07 [9.9e+07-9.9e+07] | 9.9e+07 [9.9e+07-9.9e+07] | 0.612 [0.612-0.612] | 1 [1-1] |
| events dir bytes | 1.54e+08 [1.54e+08-1.54e+08] | 9.02e+07 [9.02e+07-9.03e+07] | 9.03e+07 [9.02e+07-9.03e+07] | 0.586 [0.586-0.586] | 1 [1-1] |
| tamper detected | - | - | - | - | - |
| tamper silent changes | - | - | - | - | - |

CPU total % around runs: median 69.0, min 61, max 100 (78 samples); non-zero exit codes: 0

## 7. Not measured / not done

- Log format 3 (it does not go through kore-store, so it is unaffected).
- Tamper at 1M: the harness skips it above 100k. At 10k and 100k, 40/40 flips were detected for every build, with 0 silent changes.
- Commit latency and no-op re-ingest: `bench_log_formats` reports them under `append` and `reingest_*`, but they are not tabulated here.
- A quiet machine: CPU was 59-100 % busy with other jobs throughout, so absolute times are inflated and the paired ratios are the more trustworthy numbers.
- `versioned::list_versions` still decodes every version fully to count rows (unchanged).
- The ZSTD-shuffle (`KORE_SHUFFLE=1`) and LZ4 read paths do not use the scratch buffer (no measured problem there).
