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

**Status: the paired, interleaved benchmark (10k/100k/1M, old vs a980727d vs this branch, with load
recorded) has NOT been run yet.** It was held back while another long benchmark was using the machine.
The driver is `scripts/fileformat_speed_paired.sh` and the tables come from `scripts/fileformat_speed_tables.py`.
Only short development runs exist (section 6). They are noisy (total CPU was 92-99 % from other jobs).

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
  `cargo test --release --offline -p kore-self`: 381 tests passed, 0 failed (302 + 61 + 5 + 4 + 4 + 3 + 1 + 1).

## 5. Behaviour changes

- None in results or bytes. Timing/threading only: reads of files with 16 KiB or more of kept column data
  now checksum and decode columns on the rayon pool (a980727d decoded in parallel too, but checksummed
  serially first); large string columns and row groups use the pool as well. `read_file` of files under
  8 MiB reads the file instead of mapping it (a file being replaced while it is read is now read whole or
  not at all, instead of through a live mapping).
- With several columns damaged, a980727d never decoded any column; this branch may decode the undamaged
  columns in parallel before reporting the (same) first checksum mismatch. The returned error is the same.

## 6. Results of the paired runs

Not run yet (see the top of this document). Short development runs of `bench_store` follow: 3 rounds
interleaved, median [min-max] of the per-process medians in ms, total CPU 92-99 % from other jobs.

10k rows, 9 reps per process (`bench_store` numbers from before the last two reader commits):

| op | old | a980727d | this branch |
|---|---|---|---|
| seg write | 5.52 [5.29-5.77] | 31.67 [27.86-60.20] | 25.61 [15.58-32.76] |
| seg open (read_file x10) | 9.58 [8.53-11.58] | 29.56 [19.02-44.46] | 13.18 [12.94-18.13] (before the read_file change) |
| seg scan | 6.83 [6.82-7.18] | 19.14 [16.06-19.75] | 8.18 [7.50-10.01] |
| seg projection (1 col) | 7.27 | 1.81 | 0.73 |
| seg 20 point lookups | 106.7 | 110.4 | 41.5 |
| seg as-of (KVLG) | - | 1.59 | 0.92 |
| one write | 8.96 | 16.52 | 12.17 |
| one open | 10.25 | 11.70 | 9.93 |
| one scan | 9.62 | 11.85 | 7.83 |
| bytes seg / one | 1,707,712 / 1,680,274 | 971,120 / 867,306 | 971,120 / 867,306 |

100k rows, 2 rounds x 3 reps:

| op | old | a980727d | this branch |
|---|---|---|---|
| seg write | 57.5 | 424.8 | 153.3 |
| seg open | 101.4 | 302.1 | 99.5 |
| seg scan | 63.8 | 158.6 | 66.9 |
| seg projection | 72.0 | 15.1 | 7.0 |
| seg lookups | 919.6 | 607.9 | 225.8 |
| one write | 97.3 | 131.0 | 115.5 |
| one open | 98.7 | 105.2 | 64.7 |
| one scan | 92.7 | 87.6 | 52.7 |
| one projection | 99.7 | 2.45 | 0.22 |
| bytes seg / one | 17,095,874 / 16,805,932 | 9,734,053 / 8,585,453 | same as a980727d |

Row groups (`bench_rowgroups`, 100k rows, 10k-row groups, machine saturated): full read a980727d
543-570 ms vs this branch 332-375 ms.

## 7. Not measured / not done

- The paired interleaved runs at 10k/100k/1M (store level) and the kore-self log-format-1 runs at 10k/100k.
  None of them has been run, so there are no paired ratios yet.
- kore-self open / as-of / load through the Digital Life log: only a single 10k smoke run (open 18.9 ms,
  verify 8.9 ms, 10/10 tamper flips detected); no comparison yet.
- Log format 3 does not use kore-store, so it is unaffected and was not measured.
- 1M rows through the kore-self log was not planned (expected to be far too long).
- On a quiet machine the small-file (1000-row) full scan is expected to stay slower than the old store,
  because ZSTD decodes about 4x slower than LZ4 for these columns (about 300 vs 75-80 us per file).
  That is the price of the size advantage. In short runs the gap was 1.05x at 100k and 1.2x at 10k.
- `versioned::list_versions` still decodes every version fully to count rows (unchanged).
