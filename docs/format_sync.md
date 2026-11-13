# Format sync: kore-store from fileformat (5bcb2d41) into the Digital Life workspace

Branch `format-sync`, started from local `dev` = d9edbcfc (after the engine sync, docs/engine_sync.md, which kept
dev's kore-store on purpose). Source of the format: `origin/fileformat` = 5bcb2d41, the home of the KORE file
format (read only; nothing was written there).

## 1. Inventory

### Where dev's kore-store came from

dev's kore-store is fileformat's kore-store at bbbefd2c (dev commit 15027f54 "use kore-store from fileformat
bbbefd2c"). Since then fileformat added, in `kore-store` only (`git diff d9edbcfc 5bcb2d41 -- kore-store`):

| | files | lines |
|---|---|---|
| all of kore-store | 50 (42 added, 8 modified, 0 deleted) | +7,410 / -19 |
| `src/` | 13 (6 added: `keyindex.rs`, `rules/{mod,cross,intseq,stream,within}.rs`; 7 modified) | +3,733 / -14 |
| `tests/` | 23 added (incl. 16 fixture files: `fixtures/dl_log4/*` = Digital Life format-4 segments, `rules_v6.kore`, `old_files.digest`) | |
| `examples/` | 14 (12 added, 2 modified) | |
| `Cargo.toml` | unchanged (same dependencies) | |

The 14 removed `src` lines are all replaced in place (the `CompactOptions` default, `stage_bytes` /
`encode_column` signatures that gained a `small` zstd level, the `pub use v4::CompactOptions` line). Nothing was
removed from the public API.

### Public API changes

Additions only:

* `v4`: `CompactOptions` gains three public fields `small_zstd_level` (default 19), `search: SearchMode` (default
  `Full`) and `compact_directory` (default false), with `with_small_zstd_level`, `with_search`,
  `with_compact_directory`, presets `fast()` and `quick()`; `SearchMode`; `read_rows`, `read_row`, `is_v5`,
  `is_compact`, `write_reference`, `write_threads`, `write_many`, `stage_payload`, `info_unchecked`,
  `Info::dict_ids_hex`, `Info::column_dicts_hex`.
* `KoreReader::from_bytes_rows_with_dicts`; `KoreWriter::to_bytes_compact_many`, `KoreWriter::to_bytes_rules`.
* `DictSet::iter`, `ids`, `ids_hex`, `contains`, `IntoIterator`, `FromIterator`, `Extend`.
* `codec::decode_transform_rows`.
* new modules `keyindex` (`KeyIndex`, `KeyIndexBuilder`, `Verify`; `KIDX` object) and `rules` (`RuleOptions`; rule
  mode).
* `VERSION_V5`.

The only source-incompatible change: code that builds `CompactOptions { codecs, dicts, zstd_level }` as a struct
literal no longer compiles (three new fields). No caller in this workspace does (grep `CompactOptions *\{` outside
kore-store: none); kore-self uses `CompactOptions::new().with_dict(..)`.

### On-disk format

| container | magic / version | dev (bbbefd2c) | fileformat (5bcb2d41) |
|---|---|---|---|
| v1/v2 default writer (`KoreWriter::to_bytes`, `write_file`) | `KORE` v1/v2 | yes | same code, same bytes |
| v3 codec writer (`to_bytes_with`) | `KORE` 3 | yes | unchanged |
| v4 compact (`to_bytes_compact`) | `KORE` 4 | yes | same bytes with default options (pruned search writes the bytes of the earlier search; the i64 "second finalist" only changes a large i64 column when it is strictly smaller) |
| v5 compact directory | `KORE` 5 | no | new, opt-in (`with_compact_directory(true)`); dev's reader refuses it ("unsupported version 5") |
| v6 rule mode | `KORE` 6 | no | new, opt-in (`to_bytes_rules`); dev's reader refuses it |
| key index object | `KIDX` 1 | no | new, opt-in (separate `.kidx` object, not a data file) |
| row groups, version log, `KDIC` dictionaries, encryption | | yes | unchanged |

Readers: the new reader reads everything the old one did, plus v5/v6. No default changes: a caller that does not
opt in writes the formats it wrote before.

### Who calls kore-store in the workspace

| crate | use |
|---|---|
| kore-self (the brain) | `aru_store.rs` (`write_file` / `read_file`: payload, plan, learning history), `kore_rows.rs` (`write_file` / `read_file`: harness rows with `<key>:json` columns), `machine_protocol.rs` (`to_bytes` / `from_bytes`), `ingest_tools.rs`, `main.rs` (import/export `.kore`), `versioned/mod.rs` (format-1 segments `to_bytes` / `from_bytes`; `DictSet`, `SharedDict`), `versioned/v4log.rs` (format-4 segments: `CompactOptions::new().with_dict`, `to_bytes_compact`, `from_bytes_with_dicts`, `from_bytes_columns_with_dicts`, `v4::is_v4`, `v4::info`), `versioned/checkpoint.rs` (checkpoint files, `to_bytes` / `from_bytes`), `versioned/index_runs.rs` (index-run groups, `to_bytes` / `from_bytes` inside kore-self's own row-group framing), `versioned/upgrade.rs` (`DictSet::new`), `versioned/manifest.rs`; examples `bench_log_formats.rs`, `prof_v4_encode.rs`; tests `tests_v4.rs`, `tests_d1c.rs`, `tests_v2.rs` |
| kore-sql | `executor.rs`: `KoreReader`, `KoreWriter`, `rowgroups` |
| kore-connect, kore-io, kore-iceberg, kore-object-store, kore-spill, kore-shuffle-store, kore-bench | `KoreReader` / `KoreWriter` (+ `compress`, `rowgroups`, `versioned` in a few) |
| kore-api | dependency only |

Log formats 2, 3 and 5 store their segments with kore-self's own codec (`kore-self/src/versioned/codec.rs`), not
with kore-store; the live brain's default log format is 3. kore-store is on the path of: format-1 and format-4
segments, the format-4 dictionaries, index runs (groups), checkpoints, and every non-log brain file
(`aru_store`, `kore_rows`, `said.kore`, health, ...) through `KoreWriter::to_bytes` / `write_file`.

## 2. Compatibility fixtures (written with dev's code before the replacement)

`kore-store/tests/fixtures_dev/` (201 files, 324,646 bytes), every file pinned in `SHA256SUMS`
(`.gitattributes`: `-text`, so a checkout never changes a byte, e.g. the MANIFEST text files under
`core.autocrlf`):

* `store/` (19 files), written by `kore-store/examples/gen_dev_fixtures.rs` from the datasets in
  `kore-store/tests/common/dev_data.rs`: every column type (i64 extremes, NaN, +-0.0, +-inf, subnormal, MIN/MAX;
  bool; unicode, empty and quoted strings; `StrDict`) with NULLs; the `kore_rows` shape with nested JSON in
  `<key>:json` text columns; an empty table (columns, 0 rows); a table with no columns; an all-NULL table; a
  6,000-row table; through `to_bytes`, `write_file`, row groups (2,048-row and 128-row groups), a kore-store
  version log (3 versions, one empty), version-4 compact with default options (5 tables) and with a shared
  dictionary (`.kdic` object + a segment that cannot be read without it). The generator reads every file back
  with dev's reader and stops unless the values are the written ones.
* `kore-self/` (182 files), written by the ignored kore-self test `tests_format_sync::gen_dev_format_fixtures`
  with dev's code: a format-5 log (47 versions, kore-self codec segments, one index run `index/0-40.kore`
  written through kore-store, a checkpoint), a format-4 log (30 versions, version-4 segments with a trained
  shared dictionary, a checkpoint with an empty payload), a `kore_rows` file (nested JSON values) and an
  `aru_store` payload file. `kore-self.expect` pins what dev read back: every version's snapshot hash, the
  number of index runs, the checkpoints (step, content hash, payload), the rows, the payload, and a value digest
  of each of the 179 `.kore` files as kore-store reads it (73 kore-store files, 1 row-group index run, 105
  kore-self codec segments that kore-store refuses).

One property of dev's default writer found while writing them: a NaN is stored as 0x7FF8_0000_0000_0001, so a
written 0x7FF8_0000_0000_0000 reads back with payload 1 (still NaN). The fixture comparison therefore treats every
NaN as one value (as fileformat's own `old_files.rs` does); all other floats are compared bit for bit.

Tests over the fixtures (they pass on dev before the replacement: kore-store 67 passed, 0 failed):

* `kore-store/tests/dev_fixtures.rs`: files unchanged (SHA-256, exact file set); every `store/` file reads to the
  written values (with and without the dictionary set, `read_file`, `rowgroups::read`, every version of the
  version log); every `kore-self/` `.kore` file reads as dev read it; writer round-trip of every dataset (default
  and version 4) and the same bytes as dev's writer for every API the brain uses.
* `kore-self/src/versioned/tests_format_sync.rs`: the logs open (full and fast open through the index run) with
  the pinned hashes, checkpoint and brain rows; a new commit on the old format-4 log verifies; re-writing the
  whole fixture set gives the bytes dev wrote, file for file.
