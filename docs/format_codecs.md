# kore-store: opt-in per-column codecs (container version 3)

Branch `fileformat-improve` (from `origin/fileformat` @ a980727d). Nothing here changes what the existing
writer produces or how existing files are read; all of it is opt-in through new writer calls.

## 1. What changed

New code (all additive; `reader.rs` +3 lines, `writer.rs` +12, `lib.rs` +8, no existing line removed):

| file | content |
|---|---|
| `src/v3.rs` | version-3 container: write, read, directory, checksums |
| `src/codec.rs` | `Transform`, `Stage`, `ColumnCodec`, `WriteOptions`, encode/decode of every codec |
| `src/hz.rs` | deterministic LZ77 + canonical Huffman coder ported from the Digital Life log layer (`kore-self` `hz.rs`), plus 8 MiB segmented framing and bounded decoding |
| `tests/codecs.rs`, `tests/common/compat_data.rs`, `tests/fixtures/*.kore` | 14 new integration tests, 7 compatibility fixtures |
| `examples/bench_codecs.rs`, `write_v3_sample.rs`, `gen_compat_fixtures.rs` | benchmark, old-reader check input, fixture generator |

`kore-core` is untouched. There is no new dependency.

Public API (additive):

```rust
KoreWriter::to_bytes_with(&DataBlock, &WriteOptions) -> Result<Vec<u8>, KoreError>
KoreWriter::write_file_with(&Path, &DataBlock, &WriteOptions) -> Result<(), KoreError>
WriteOptions::{auto(), plain(), with_column(name, ColumnCodec)}   // default = auto()
ColumnCodec::new(Transform, Stage)                                 // ColumnCodec::AUTO, ColumnCodec::PLAIN
```

Reading is the existing `KoreReader::from_bytes / read_file / from_bytes_columns / read_file_columns`; they
detect version 3 themselves (also inside version logs). `KoreWriter::to_bytes` still writes version 2,
byte for byte as before.

### Column type for ids (what was and was not done)

`kore-core::ColumnData` has no bytes variant and adding one would break every exhaustive `match` in the
workspace, so it was not touched. The id case is covered by the `HexFixed` transform: a `Str` column whose
non-null values are all the same even length of lowercase hex (e.g. 64-char SHA-256 ids) is stored as
raw bytes (32 per id) and comes back as the identical `Str`. A caller holding raw 32-byte ids converts to hex
for the `DataBlock` and gets them stored raw. There is no way to store arbitrary non-UTF-8 bytes; that needs
a `ColumnData::Bytes` in `kore-core` and was left to the owner.

### Codecs

A column payload = transform (typed values to bytes) then stage (byte compression). Both ids are stored per
column in the directory. `Auto` is a writer-side request only; the concrete winner is stored.

| transform | valid for | layout |
|---|---|---|
| `Plain` | all | i64: 8 B LE per non-null; f64: 8 B IEEE bits (NaN payloads and -0.0 preserved); bool: bit-packed; str: varint lengths, then bytes; str_dict: the legacy dict layout |
| `Delta` | i64 | zigzag varint of the wrapping difference to the previous non-null value |
| `Varint` | i64 | zigzag varint of the value |
| `Dict` | str | dictionary of any size, first-appearance order, codes 1..=len (0 = NULL), code width 1/2/4 bytes |
| `HexFixed` | str | `width:u16` chars + `width/2` raw bytes per non-null value; only lowercase hex of one even length, otherwise the writer returns an error |

Null bitmap (all but `Dict`): `has_nulls:u8` then, if 1, `ceil(n/8)` bytes LSB first, 1 = NULL; only non-null
values follow.

| stage | payload |
|---|---|
| `None` | the transform bytes as they are |
| `Lz4` | `varint(raw_len)` + `lz4_flex` block |
| `Hz` | `varint(raw_len)` + segments `varint(clen) hz_stream`, one segment per 8 MiB of input, coded in parallel, output order fixed |
| `Zstd` | `varint(raw_len)` + zstd level 3 |

`Auto` tries the applicable transforms (`Dict` only if distinct values <= rows/2, `HexFixed` only if the data
qualifies) times the stages None/Lz4/Hz/Zstd and keeps the smallest result; ties go to the earlier candidate
in a fixed order. An explicit stage or transform is honored as requested, even if it is larger.

## 2. Byte layout (version 3)

```text
"KORE" | version:u16 = 3 | num_cols:u32 | num_rows:u64
directory, per column:
  name_len:u16 | name | dtype:u8 | transform:u8 | stage:u8 | payload_len:u64 | payload_crc32:u32
dir_crc32:u32     CRC-32 of every byte from offset 0 up to here
payloads          concatenated in directory order (offset = running sum of payload_len)
```

No readable text trailer. The file must end exactly after the last payload. dtype ids are the existing ones
(`DType`).

## 3. Compatibility and safety

* Version-2 files are read exactly as before: `reader.rs` only gains an early `if is_v3(data)` branch;
  nothing in the v2 path is edited.
* Old readers refuse v3. The version-2 reader checks `version != 2 && version != 1` right after the magic and
  returns `unsupported version 3` before any other field is interpreted.
  **Verified** (not just argued): a detached worktree at the unmodified a980727d was built into its own
  target dir, with a scratch `old_read` example (`KoreReader::from_bytes`, not committed). Result on the
  files from `examples/write_v3_sample.rs` (same data, written by the current branch):
  ```
  sample_v2.kore: OK rows=200 cols=3
  sample_v3.kore: ERR invalid argument: unsupported version 3
  ```
  Other-language readers (C#, Go, Java, Python bindings, `conformance/`) were not run against v3 files.
* Nothing is upgraded silently: `to_bytes`, `write_file`, row groups and version logs (`append_version`)
  still write version 2. A v3 file can be placed in a version log with `versioned::append_raw`; row groups
  have no v3 option.
* Checksums cover the new bytes: header, directory and every length by `dir_crc32`; each payload by its own
  CRC; trailing bytes and truncation are errors. The v2 format by comparison checksums only the compressed
  payload (not header, schema, codec byte or lengths); v3 is stricter. With column projection, only the
  requested columns' payload CRCs are checked (the directory CRC is always checked).
* Decoders are strict (exact lengths, UTF-8 validation, code range checks, size caps: hz 1032:1, lz4 255:1,
  2 GiB per column) and return errors; the container also wraps decoding in `catch_unwind` like the v2 reader.

## 4. Tests

`cargo test -p kore-store --offline` (debug): **42 passed, 0 failed** (baseline before the work: 23).

| suite | before | after |
|---|---:|---:|
| lib (unit) | 8 | 13 |
| conformance / nulls / projection / shuffle / stress / strlen | 1/2/2/2/6/2 | unchanged, all pass unchanged |
| `codecs` (new) | - | 14 |

(lib: +1 `hz::segmented_roundtrip`, +2 hz tests carried over from the port, +2 `codec` tests = +5.)

New tests in `tests/codecs.rs`:

* `v2_fixtures_still_read_identically`, `v2_writer_output_is_byte_identical_to_the_old_writer`: seven
  fixtures in `tests/fixtures/` were generated by the **unmodified** code (commit `ed5cb21d`, before any
  source change; generator `examples/gen_compat_fixtures.rs`): default and no-trailer files, sorted ints,
  low-cardinality strings and StrDict, text, a row-group file, a version log. The test reads them and
  compares to the logical data, and checks that the current v2 writer reproduces the same bytes (the version
  log was generated with the readable trailer off, so that one is checked structurally).
* `every_valid_transform_and_stage_round_trips`: every transform x stage on every dtype, with nulls, i64
  extremes, NaN payloads, -0.0, empty strings, UTF-8, empty and all-null columns.
* `invalid_codec_requests_are_errors_not_silent_fallbacks`, `auto_picks_the_intended_codecs`,
  `projection_...`, `v3_inside_version_log_and_file_roundtrip`, footer-marker text test.
* `output_is_deterministic_across_runs_and_thread_counts`: identical bytes on repeated runs and on rayon pools
  of 1, 2 and 7 threads.
* `tamper_at_every_byte_is_detected`: a 6-column v3 file; every byte flipped with three masks; all
  rejected, none panics. Plus truncation at every length and an appended byte.
* `consistent_checksums_over_garbage_payloads_still_fail_cleanly`: payloads mutated with valid CRCs fed to
  the decoders; no panics.
* `version_field_is_what_old_readers_check`: asserts the version bytes; a v3 body under a fake version is
  rejected.

## 5. Measurements

Generator (`examples/bench_codecs.rs`, seed `0x4B4F5245`): `id` 64 lowercase hex (random), `ts` i64 ms with
1..2000 ms gaps, `tag` (30 values), `source` (8), `kind` (12), `text` 40..5000 bytes log-uniform (average
1027) from a pool of 30000 recurring sentences plus numbers and hex tokens. No nulls. Release build; times
are in-memory `to_bytes`/`to_bytes_with` and `from_bytes` (no disk). Variants are interleaved round-robin
inside each repetition (write, read back, drop). Median with (min-max). The legacy writer was run with the
readable trailer off (`KORE_READABLE_MODE=none`). Each output was decoded and compared to the input in
repetition 0.

**Machine load: the machine was saturated by other jobs for the whole session.** `typeperf` showed
99.8-100 % total CPU before, during and after (samples in the bench output dir), with sibling
`bench_log_formats.exe`, cargo and python jobs running; my own process is part of that number, but it was
100 % even when idle. 8 logical cores, 32 GB RAM. Absolute times are therefore inflated and noisy
(the first 100k run gave v2 write 1.55 s, the later one 2.61 s for the same code); sizes are exact and
deterministic. Treat the ratios between variants as indicative only.

N = 100,000 (102.7 MB of text), 5 repetitions (second run, with the extra Hz variant):

| variant | size bytes | vs v2 | write median s (min-max) | read median s (min-max) |
|---|---:|---:|---|---|
| v2 (legacy writer) | 34,067,216 | 100.0% | 2.61 (1.67-5.43) | 1.44 (0.82-2.21) |
| v3 auto (lz4/hz/zstd chosen) | 32,526,372 | 95.5% | 14.38 (10.20-23.88) | 0.55 (0.48-1.09) |
| v3 transforms + lz4 | 86,792,598 | 254.8% | 1.80 (1.36-3.42) | 0.65 (0.52-2.10) |
| v3 transforms + zstd | 33,217,546 | 97.5% | 2.12 (1.66-6.52) | 0.86 (0.49-1.15) |
| v3 transforms + hz | 32,535,190 | 95.5% | 13.18 (9.16-25.89) | 1.11 (0.49-1.39) |

The first 100k run (4 variants, otherwise identical, lower load): v2 1.55 s (1.48-1.65) write,
0.83 s (0.76-0.94) read; v3 auto 9.21 s (7.31-11.04) write, 0.39 s (0.37-0.45) read; lz4 1.17 s / 0.48 s;
zstd 1.69 s / 0.54 s.

N = 1,000,000 (1.027 GB of text), 3 repetitions:

| variant | size bytes | vs v2 | write median s (min-max) | read median s (min-max) |
|---|---:|---:|---|---|
| v2 (legacy writer) | 340,011,138 | 100.0% | 26.13 (19.43-29.32) | 18.00 (11.30-22.51) |
| v3 auto (lz4/hz/zstd chosen) | 324,493,806 | 95.4% | 116.72 (84.35-125.08) | 5.81 (4.81-6.18) |
| v3 transforms + lz4 | 867,653,243 | 255.2% | 18.07 (14.42-29.31) | 6.99 (6.00-13.04) |
| v3 transforms + zstd | 331,559,150 | 97.5% | 22.95 (16.93-36.48) | 9.38 (5.58-11.44) |

(The hz-only variant was added after the 1M run and was not run at 1M.)

Per column, N = 1M (each column alone; v2 = single-column file, v3 = `Auto`):

| column | v2 bytes | v3 auto bytes | v3 auto choice |
|---|---:|---:|---|
| id | 36,069,565 | 32,000,003 | HexFixed / None |
| ts | 2,064,568 | 1,597,226 | Delta / Hz |
| tag | 584,358 | 585,034 | Dict / Zstd |
| source | 420,048 | 407,601 | Dict / Hz |
| kind | 485,046 | 483,615 | Dict / Hz |
| text | 300,387,663 | 289,420,182 | Plain / Hz |

Reading the numbers, including the bad ones:

* **Size: v3 is 4.5-4.6 % smaller than v2 on this data, not a large win.** The current v2 writer already
  tries zstd on every column, so it already entropy-codes hex ids (to about 36 bytes per id; the floor is
  32) and text. The big reductions in the Digital Life doc (229.8 -> 88.7 MB) were measured against a log
  layer that did not benefit from that; here the remaining gains are ids (-11 %), timestamps (-23 %) and
  text (-3.7 % with Hz vs zstd).
* **`tag` is 676 bytes (0.1 %) larger in v3 at 1M** (`Dict` + `Zstd` vs the v2 byte-code dictionary + zstd); at
  100k and 20k rows it was marginally smaller. A regression, immaterial in absolute size.
* **Write time: v3 `Auto` is about 4.5x slower than v2 at 1M and 5.5-5.9x at 100k (two runs).** Cause: `Auto` runs lz4,
  hz and zstd on every candidate payload; the fixed `transforms + hz` variant (13 s at 100k vs 14 s for
  auto) shows that Hz itself accounts for nearly all of it. The hz port is a single-threaded-per-segment
  LZ77 with hash chains (chain 32) and is roughly an order of magnitude slower than zstd level 3.
  Explicit `Stage::Zstd` or `Stage::Lz4` writes cost about the same as v2.
* **Read time is better**: v3 auto reads about 3.1x faster than v2 at 1M (5.8 s vs 18.0 s) and 2.6x at 100k
  (0.55 s vs 1.44 s). The v2 and v3 ranges do not overlap at 1M, but the three v3 variants overlap each other,
  so do not read an ordering into them. Both are inflated by the saturated machine.
* `transforms + lz4` is 2.55x the v2 size: LZ4 alone does poorly on this text; v2 picks zstd per column.
  Only `Auto` or explicit Hz/Zstd should be used for text.

## 6. Risks

* Hz is new code for this crate (ported, not independently audited); it is covered by round-trip,
  corruption and 8 MiB-boundary tests and the tamper test, not by a long fuzz run.
* Determinism is a property of this build: `Auto` includes zstd (level 3); a different zstd or lz4_flex
  version can produce different, still valid, bytes. Same build and same input give the same bytes
  (tested across thread counts). Hz and the transforms do not depend on any library.
* `Auto` write cost (above) and memory: a text column holds the raw payload plus the stage outputs at once.
* Files with `HexFixed` give back lowercase hex only; the writer refuses anything else rather than altering it.
* A v3 file's header text is not human readable (no readable trailer by design).
* Cargo note for anyone building the old and new trees side by side: two worktrees of this repo built into the
  **same** `CARGO_TARGET_DIR` share a `kore-store` artifact name, one overwrote the other during this work and
  produced misleading build errors until forced to rebuild. Use separate target dirs.

## 7. Not measured / not done

* Disk I/O (write_file / read_file), cold cache, fsync, mmap behavior; only in-memory times.
* Nulls in the benchmark data (tests cover nulls; sizes with nulls were not measured), data without recurring
  text, f64/bool columns, sorted vs shuffled integers beyond timestamps, row groups with v3.
* Memory use (peak RSS) was not measured.
* Another machine, an unloaded machine, or more than 3-5 repetitions.
* Other readers (C#, Go, Java, Python, FFI) on v3 files; `conformance/` fixtures were not extended with v3.
* A `ColumnData::Bytes` type in `kore-core` (arbitrary binary values), bit-packing transforms, v3 row groups,
  and encryption of v3 (it works through `encrypt_blob` on the output but has no dedicated test).
