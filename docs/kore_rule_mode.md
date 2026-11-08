# KORE rule mode: lossless compression by discovered rules (opt-in, container version 6)

Branch `kore-rule-mode`: started from `fileformat` @ b44d2a91, then `fileformat` @ 118f80df was merged in (a
merge, no rebase). Everything is additive and opt-in. The default writer and the version-3, version-4 and
version-5 writers write the same bytes as at 118f80df, and every existing reader path accepts the same files.
The 19 pinned version-4 hashes (`tests/v4_writer_bytes.txt`) and the 19 pinned version-5 hashes
(`tests/v5_writer_bytes.txt`) still match. Nothing was pushed.

## 1. Idea and guarantee

When a column, or a relation between columns, follows a rule, the file stores the rule plus its exceptions or
residuals instead of the values. The reader regenerates the values. Decoding is lossless:

* Every rule is evaluated with exact integer arithmetic (wrapping `i64`) or with IEEE-754 operations that Rust
  performs the same way on every platform (`+ - * /`, `round`, `as f64`; no fused multiply-add, no library
  math). Whatever the rule does not reproduce is stored as a residual or an exception.
* The writer decodes every rule payload it produces. It keeps the payload only if the result equals the
  column bit for bit (floats compared by their bits, nulls by position).
* Each rule payload carries a CRC-64/XZ of the decoded column (`rules::digest`: the type, the row count, and per
  row a null flag and the value bits). The reader recomputes it after decoding. A mismatch gives the error
  `rule-mode checksum mismatch: decoded values differ from the written column`; it is never silent.
* The version-4/5 checksums still apply: the header and directory are covered by `dir_crc32` and every payload
  by its CRC-32.

A file is never larger than the version-4 output with the same options. Every column is encoded exactly as
`v4::write` encodes it with the same `CompactOptions` (`v4::encode_for_write`) and also by every applicable
rule. The smaller payload is kept; on a tie the version-4 payload is kept. The version-6 directory is the
varint directory of version 5, which is never larger than the version-4 directory.

If no column uses a rule, the output **is the file that `to_bytes_compact` writes with the same options**,
byte for byte. That is version 4, or version 5 with `with_compact_directory(true)`. A test checks this for the
default options, `quick()`, `fast()`, level 19 and the compact directory. In that case the only cost is encode
time.

API:

* Writing: `CompactOptions::with_rules() -> RuleOptions` (also `RuleOptions::default()`, `.within_only()` and
  `.cross_only()`), then `KoreWriter::to_bytes_rules(&block, &opts)`.
* Inspection: `rules::write_explained` returns the bytes plus, per column, the version-4 size and the rule
  that won. `rules::describe(&bytes)` lists per column the rule, its exception count and its
  exception/residual bytes (the `rules_describe` example prints it).
* Reading: the usual `KoreReader::from_bytes`, `from_bytes_columns`, `from_bytes_with_dicts`,
  `from_bytes_columns_with_dicts` and `from_bytes_rows_with_dicts`.
* A column with an explicit codec (`CompactOptions::with_column`) keeps that codec. Shared dictionaries work
  as in version 4 for the columns that keep a version-4 payload.

## 2. The rule DSL

Rules are searched in a small, fixed grammar. The idea of a small, verifiable DSL with an exact checker comes
from Track B2 of the Digital Life being (`docs/measured.md` on `main-Digital-Life-AI`). None of its code is
used, and kore-store does not depend on kore-self. The search is deterministic: the same block and options
always give the same bytes.

### Inside one column (families 1-6)

| family | applies to | rule | stored |
|---|---|---|---|
| 1 integer sequence | `i64` | one predictor over the non-null values `v[0..m)` (see below) | the predictor and the residuals `v[i] - pred(i)` |
| 2 float decimal | `f64` | `x = k / 10^s`, bit-exact; s = 0..15, the smallest s that fits 7/8 of a sample | s; the exceptions (row + raw bits: NaN, -0.0, inf, values with more digits); `k` coded by family 1 |
| 3 float predicted | `f64` | prediction = previous value, or `a + i*b` | xor of the bits with the prediction, as 8 byte planes |
| 4 timestamp text | canonical `YYYY-MM-DDTHH:MM:SSZ` strings | epoch seconds coded by family 1 | as family 1 |
| 5 text template | strings | each maximal digit run that is a canonical integer of at most 18 digits becomes a slot | a skeleton column (version-4 coded: dictionary + stage), a mode per template slot (plain, or delta to the previous value of the same slot), the numbers |
| 6 categorical Markov | strings with at most 1024 distinct values | adaptive next-token ranking: for each previous value, all values ordered by how often they followed it | the dictionary plus one rank per value |

Integer predictors (family 1; also used inside families 2 and 4):

| predictor | prediction for index i | covers |
|---|---|---|
| `Rec`, k = 0..3 | `c0 + sum c_j * v[i-j]`, with `v[<0] = 0` | plain (k = 0); constant; delta (arithmetic progression); delta-of-delta (piecewise linear); third difference (a polynomial up to degree 2 leaves a constant residual); fitted linear recurrences of order up to 3 with integer coefficients (geometric with an integer ratio, affine-geometric `3*v+1`, Fibonacci-like) |
| `Affine a b` | `a + b*i` | fixed grids (for timestamps with jitter the residual is the jitter alone, while delta would store the difference of two jitters); with b = 0 it is an offset (frame of reference) |
| `Lag p` | `v[i-p]` | periodic or seasonal data (p = 2..2048 is searched) |
| `LagTrend p` | `v[i-p] + v[i-1] - v[i-p-1]` | seasonal data with a trend |
| `Runs` | none | constant runs: the run values (delta coded) plus the run lengths |

Recurrences are found as in B2. Each form (a non-empty subset of the lags {1,2,3}, with or without a constant:
14 forms, smallest first) is solved exactly (rational Gauss-Jordan) on q consecutive rows, and 3 further rows
must agree exactly. Only integer coefficients up to 2^20 are kept.

Every candidate then competes on size. Candidates are scored on three sample windows of 16K values (zstd
level 1 on the residual varints), the two best are encoded in full, and the smaller one wins.

### Across columns (families 7-10)

| family | rule | stored |
|---|---|---|
| 7 integer relation | `t = a + b`, `a - b` or `a * b` (wrapping); `t = alpha * a + beta`; offset `t = a + r` (an order rule such as `ship_date = sold_date + lag`) | the residuals `t - pred` (pred = 0 where a source is null) |
| 8 float relation | `t = a * b`, `a + b`, `a - b` or `a / b` in f64 (integer sources converted with `as f64`), optionally rounded half away from zero to s decimals | xor of the bits with the prediction (8 byte planes) |
| 9 lookup | shift: `t[i] = a[i-k]`, k = 0..8; functional dependency: `t = map[a]` (one target value per distinct source value, the most frequent one) | the map (version-4 coded column) plus the exception rows and values (version-4 coded column) |
| 10 sum | `t = a_1 + ... + a_k` over 3 to 8 float columns. With a decimal scale s of the target (money: s = 2) the sum is taken in integer units, `(sum of round(a_j * 10^s)) / 10^s`, so float noise is not an exception | per value the residual in integer units, `round(t * 10^s) - prediction` (a missing or extra fee becomes a small integer), plus the raw bits of the values that are not exactly `units / 10^s` |

The search:

* **Sample test.** On a sample of 4096 rows (the first 2048 and 2048 spread over the rest), a relation must
  predict at least half of the target rows exactly. Lookups need at most 1/8 distinct keys in the sample and
  80 % consistency. An offset relation must code to less than 80 % of the target's own deltas on the sample.
  Sums are built greedily: at each step the column that makes the most sample rows exact is added (ties: the
  smaller absolute error); then the integer-unit sum and the plain f64 sum of the chosen columns are tested.
* **Full encoding.** Per target and family group, only the 8 relations with the best sample hit rate are
  encoded in full.
* **Acceptance.** Candidates are accepted greedily by bytes saved. Every column is derived at most once, and
  the derivation graph stays acyclic; a derived column may be the source of another one, for example
  `total_cents = subtotal_cents + shipping_cents`, `subtotal_cents = qty * unit_cents`,
  `unit_cents = lookup(unit_price)`.
* **Width limit.** Only the first 64 columns of a block are searched for relations.

Residuals and exceptions always go through the existing best stages:

* Every residual or exception stream is coded with the smallest of: no stage, lz4, zstd and hz. The zstd level
  is the version-4 one: `small_zstd_level` below 256 KiB, `zstd_level` above. hz is only tried below 128 KiB.
* Embedded value columns (skeletons, dictionaries, maps, exception values) are coded by the version-4 column
  encoder itself (`Auto` transform + stage).

## 3. Format: container version 6

Container version 5 (118f80df, `v4.rs`) is version 4 with a varint header and directory. **Version 6 is the
version-5 layout plus the rule marker**:

```text
"KORE" | version:u16 = 6 | varint(num_cols) | varint(num_rows) | varint(num_dicts)
per dictionary: dict_id:[32] | varint(content_len)
per column: varint(name_len) | name | dtype:u8 | codec:u8 | [dict:u8 if stage == 4] | varint(payload_len) | payload_crc32:u32
  codec with bit 7 = 0: transform id (bits 0-3) | stage id << 4 (bits 4-6); a version-4 payload, exactly as in version 5
  codec = 0x80 exactly: a rule payload (no dict byte); any other value with bit 7 set is refused
dir_crc32:u32
payloads
```

Why a new version and not a flag:

* Bit 7 of the codec byte is reserved (it must be 0) in version 5. A version-5 reader therefore cannot be
  handed a rule column under its own version number, and no transform or stage id of version 4 or 5 can
  collide with the marker.
* Version 6 always uses the compact varint directory: a file only becomes version 6 when a rule wins, so it
  never needs the version-4 directory.
* Varints must be minimal, as in version 5.

A rule payload is:

```text
family:u8 | digest:u64 (CRC-64/XZ of the decoded column) | n_deps:u8 (0; 1-2 for families 7-9; 2-8 for 10) | deps: varint column indices | body

stream      := stage:u8 (0 none, 1 lz4, 2 hz, 3 zstd) | varint(len) | version-4 stage payload
nulls       := 0 | 1 stream(bitmap, bit = 1 means NULL)
v4 column   := transform:u8 | stage:u8 | varint(len) | version-4 column payload
int body    := model | stream(zigzag varint residuals)          model: 0 Rec k c_1..c_k c0 | 1 Affine a b | 2 Lag p | 3 LagTrend p
             | 4 Runs | varint(runs) | stream(run values, delta) | stream(run lengths - 1)
family 1    := nulls | int body
family 2    := nulls | s:u8 | varint(n_exc) | [stream(exception gaps) | stream(exception bits, byte planes)] | int body
family 3    := nulls | 0 | stream(xor planes)   or   nulls | 1 | a:u64 | b:u64 | stream(xor planes)
family 4    := nulls | int body (epoch seconds)
family 5    := nulls | v4 column (skeletons, 0x00 per slot) | varint(n_slots) | stream(slot modes) | varint(n_numbers) | stream(numbers)
family 6    := nulls | varint(D) | v4 column (dictionary) | stream(ranks)
family 7    := nulls | op (0 +, 1 -, 2 *, 3 affine alpha beta) | stream(residuals)
family 8    := nulls | op:u8 (0 *, 1 +, 2 -, 3 /) | round:u8 (0xFF none) | stream(xor planes)
family 9    := kind (0 shift k | 1 FD varint(n_keys) v4 column(map)) | varint(n_exc) | [stream(exception gaps) | v4 column(values)]
family 10   := nulls | round:u8 | 0xFF: stream(xor planes) | s: stream(unit residuals) varint(n_exc) [stream(gaps) | stream(raw bits)]
```

Reader checks:

* the version-5 header checks: minimal varints, the reserved codec bits, the CRCs, the dictionaries;
* the rule family, model and op ids;
* every count against the row count (or against the bytes present) before any allocation, and every stream
  size before decompression;
* a rule column may not refer to itself, and the dependencies may not form a cycle;
* the source columns must have the row count;
* the decoded length, the type and the digest.

Everything runs under `catch_unwind`, like the version-4 reader, so a decoder bug on corrupt input becomes an
error, not an abort. A projection decodes the requested columns and, transitively, the sources of the derived
ones; the sources are not returned.

**Row-level partial reads** (`KoreReader::from_bytes_rows_with_dicts`, `rules::read_rows`) work on version 6.
They decode the requested columns and their sources in full and then cut the rows: a rule column needs its
whole sequence (predictors, lags, sources). The result is the same as a full read followed by a cut, and it is
not faster; a test compares partial reads with the full read. Version 4/5 files keep the row-level decode of
118f80df.

**Compatibility.** Readers that predate version 6 stop at the version field. Real builds of both earlier
readers, made in temporary worktrees (removed afterwards), each running its own `examples/read_hash.rs` on
the files of `examples/write_v6_sample.rs` plus the v2/v3 fixtures:

```text
--- reader 118f80df (fileformat, version 5)
events_1000.kore rows=1000 hash=36efa5f5e7c183e0
logs_v6.kore ERROR invalid argument: unsupported version 6
orders_v4.kore rows=2000 hash=2ad77ade3e1c952e
orders_v5.kore rows=2000 hash=2ad77ade3e1c952e
orders_v6.kore ERROR invalid argument: unsupported version 6
orders_v6_flipped.kore ERROR invalid argument: unsupported version 6
orders_v6_level19.kore ERROR invalid argument: unsupported version 6
orders_v6_truncated.kore ERROR invalid argument: unsupported version 6
rules_v6.kore ERROR invalid argument: unsupported version 6
sample_v2_6e2e6a7b.kore rows=200 hash=21a460494cd07082
sample_v3_6e2e6a7b.kore rows=200 hash=21a460494cd07082
sensors_v6.kore ERROR invalid argument: unsupported version 6
--- reader b44d2a91
(the same, and orders_v5.kore ERROR invalid argument: unsupported version 5)
--- this branch
orders_v4 / orders_v5 / orders_v6 / orders_v6_level19: rows=2000 hash=2ad77ade3e1c952e (the same values)
orders_v6_flipped.kore ERROR invalid argument: checksum mismatch in column 'qty'
orders_v6_truncated.kore ERROR invalid argument: truncated .kore data
```

Shared files touched, all additions:

* `lib.rs`: the module and a re-export;
* `reader.rs`: dispatch lines in 3 functions;
* `writer.rs`: `to_bytes_rules`;
* `v4.rs`: `encode_column`, `decode_payload` and `serialize_v5` made `pub(crate)`, plus the new
  `encode_for_write` (5 lines).

One upstream test line changed: `tests/v4_rows.rs` checked that a version-5 file relabelled 6 gives
"unsupported version 6". Version 6 is assigned now, so that check uses version 7 ("unsupported version 7"),
and a second assertion checks that the relabelled file is still refused.

New files:

* `src/rules/{mod,stream,intseq,within,cross}.rs`;
* `tests/rules.rs` and `tests/fixtures/rules_v6.kore` (187 bytes). It replaces the never-published
  `rules_v5.kore`, which would now be read as version-5 garbage;
* `examples/bench_rules.rs`, `bench_rules_sweep.rs`, `write_v6_sample.rs`, `rules_describe.rs`,
  `support/rules_gen.rs`;
* `scripts/kore_rules/{py_side,py_baselines,tables,export_tpcds,export_parquet,analyze_taxi}.py`.

## 4. Result in short

All numbers are from one run on one machine (section 5). Rule mode is compared at zstd level 19 with the
maximum-level baselines: v4 level 19, Parquet zstd-22 and xz-9.

* **Real data: NYC TLC Yellow Taxi trip records, January 2024** (one Parquet file supplied by the user,
  SHA-256 c4d59da7..., 2,964,624 rows x 19 columns).
  * Sizes: rule mode at level 19 is 29,518,146 bytes (0.524 B/value). v4 level 19 is 45,758,509 (1.55x larger),
    Parquet zstd-22 45,674,181 (1.55x), CSV + xz-9 34,640,960 (1.17x), and the user's original Parquet file
    49,961,641 bytes.
  * About half of the gain is not a rule. For the two timestamp columns the version-4 writer's cheap transform
    pre-selection (a zstd level 1 score) picks `Varint` (10.5 MB each), while plain delta gives 6.2 MB; rule
    mode's integer search tries delta in full. That accounts for 8.6 MB of the 16.2 MB difference.
  * The other 7.6 MB come from rules: the fare sum (4.8 MB -> 0.34 MB), the decimal scale on the money and
    distance columns, and lookups (`mta_tax`, `improvement_surcharge`, `Airport_fee`, `RatecodeID`).
* **TPC-DS (standard benchmark, generated locally by DuckDB's dsdgen).** store_sales (1M rows) is 1.86x
  smaller than v4 level 19 and web_sales (all 719,384 rows) 2.12x: the `ext_*`/`net_*` money columns are
  products, quotients and sums of each other. The dimension tables customer, customer_address and item gain
  1.04-1.10x.
* **Generated data from this branch** (written for the rules, so they show what the families can do, not what
  real data gives), at 1M rows against v4 level 19: orders 2.93x, logs 1.42x, sensors 1.15x.
* **The realistic events generator** (docs/kore_size.md): 1.00x (8.514 against 8.537 B/value). The hash ids and
  the free text are 95 % of the bytes and no rule applies to them.
* **Controls.**
  * random: 1.02x. All of it comes from the uniform [0, 1) doubles, which have only 53 random bits; the
    full-width random columns gain nothing.
  * random_norule: exactly 1.00x, byte-identical to version 4.
  * sequences is a **capability demo built for the rules** (Fibonacci, 3v+1, a quadratic: about 70 bytes per
    million values). It is not a benchmark and takes no part in any claim.
* **Costs.**
  * Encode takes 2-12x the version-4 time at the default level (paired); on the taxi file at level 19 it took
    567 s against 60 s.
  * Full reads are 1-5x slower than version 4. A single derived column is 1.5-40x slower, because its
    sources have to be decoded too.
* **TPC-H** was not measured. DuckDB's `tpch` extension is not installed in the conda env, and installing it
  would need a download, which was not done. The `tpch_1m.parquet` file in the repository root is not dbgen
  output: its `l_extendedprice` does not equal quantity x the part retail price on any of its 1M rows and its
  comments are `comment_<i>`, so it was not used as TPC-H.

## 5. Measurements

Setup:

* One run per dataset and size, on one machine (Windows 11, 8 logical CPUs, release build), on 2026-11-07/08.
* The machine was shared with other builds the whole time: `typeperf` total CPU was 99-100 % at the start
  and at the end of the store-side run.
* Sizes are deterministic and do not depend on load. Times are not reliable in absolute terms; use the paired
  ratios. Encode: version 4 and rule mode back to back in one process, order alternating per repetition,
  3 repetitions. Read: the 4 files read in rotating order, 3 repetitions.
* Noise reference: in the first run, two byte-identical files read by the same code gave paired ratios of 1.31
  and 1.50.

Commands (store side, then the other formats on the same values):

```text
cargo build --release -p kore-store --example bench_rules --example bench_rules_sweep --example rules_describe
bench_rules --out DIR --sizes 100000,1000000 --sets sensors,orders,logs,sequences,random,random_norule --reps 3
bench_rules --out DIR --sizes 100000 --sets events --events rdata_100000.jsonl --reps 3
bench_rules --out DIR --sizes 0 --sets dump:<dir> --reps 3            (TPC-DS tables, the taxi file; 0 = all rows)
bench_rules_sweep --out sweep.json --rows 200000
python -I scripts/kore_rules/py_side.py <dump> py_<name>.json          (Parquet zstd-22, CSV + xz-9, raw + xz-9)
python -I scripts/kore_rules/py_baselines.py <dump> pyb_<name>.json    (DuckDB, SQLite, Parquet variants)
python -I scripts/kore_rules/tables.py DIR sweep.json
```

Python is the read-only conda env `engines` (Python 3.12, pyarrow 25.0.0, duckdb 1.5.6, SQLite 3.53.4). Nothing
was installed. The raw results are not committed; they are in
`C:\Users\skathera\AppData\Local\Temp\claude\7f7596edcdf0d251\75ecfc96-88bb-4174-9290-83105f566c76\scratchpad\rules\final2`,
with the per-column rule descriptions in `...\scratchpad\rules\described`.

Datasets:

* **NYC TLC Yellow Taxi trip records, Jan 2024, SHA-256 c4d59da7...** (the hash of the file as supplied, not a
  verification of its source).
  * Handling: copied into an empty scratch directory, hash checked after the copy, read only with pyarrow
    (`scripts/kore_rules/export_parquet.py`); scripts live elsewhere and get the path as an argument. Neither
    the file nor anything derived from it is committed.
  * Conversion: timestamps become int64 microseconds, int32 becomes int64, the rest is unchanged. All rows
    are used (2,964,624).
  * Every format in the tables is written from these converted values. The user's original Parquet file
    (49,961,641 bytes, native timestamp types) is listed separately.
* **TPC-DS (standard benchmark, generated).**
  * Generated with `CALL dsdgen(sf=1)` through DuckDB's `tpcds` extension, which was already installed in the
    user's DuckDB extension directory; it was loaded with autoinstall and autoload off, so nothing was
    downloaded (`scripts/kore_rules/export_tpcds.py`).
  * Conversion: DECIMAL becomes float64 and DATE becomes 'YYYY-MM-DD' text.
  * Rows: store_sales has its first 1,000,000 rows in dsdgen order (2,880,404 at SF1); web_sales (719,384),
    customer (100,000), customer_address (50,000) and item (18,000) have all their rows.
* **Generated by this branch** (`examples/support/rules_gen.rs`, seed 20261107): sensors, orders and logs, as
  described in the source.
* **events**: the realistic generator of docs/kore_size.md (seed 20261101, 100,000 lines, the same JSONL file
  as there), turned into the 8 columns of the Digital Life events table, 98,040 event rows.
* **sequences**: capability demo only.
* **random**: uniform 64-bit integers, doubles with random bits, uniform doubles in [0, 1), and 32-character
  random hex. **random_norule** is the same without the [0, 1) doubles.

Other formats are written from the same values, with exact bits, via Arrow buffers:

* Parquet: pyarrow, zstd level 22, one row group per 1M rows; dictionary encoding as stated.
* CSV + xz-9: the pyarrow CSV writer, then lzma preset 9.
* raw + xz-9: the Arrow buffers concatenated, then lzma preset 9.
* DuckDB: `CREATE TABLE AS` plus `CHECKPOINT`, the size of the file.
* SQLite: one table, inserted in one transaction, then VACUUM; once without and once with an index on the
  key column.

#### Headline: size against the maximum-level baselines (B/value = file bytes / (rows x columns); ratio = baseline bytes / rule-mode bytes)

Rule mode at zstd level 19 (`CompactOptions::default().with_zstd_level(19).with_rules()`), so every column is compared with v4 at the same level.

| dataset | rows x cols | **rule mode, level 19** | v4 level 19 | Parquet zstd-22 | CSV + xz-9 | raw buffers + xz-9 |
|---|---:|---:|---:|---:|---:|---:|
| NYC TLC Yellow Taxi trip records, Jan 2024, SHA-256 c4d59da7... (real data) | 2,964,624 x 19 | **0.524 B, 29,518,146 bytes** | 0.812 B, 45,758,509 bytes (1.55x) | 0.811 B, 45,674,181 bytes (1.55x) | 0.615 B, 34,640,960 bytes (1.17x) | 0.679 B, 38,227,492 bytes (1.30x) |
| orders (generated) | 100,000 x 14 | **0.391 B, 547,405 bytes** | 1.188 B, 1,663,420 bytes (3.04x) | 1.370 B, 1,918,535 bytes (3.50x) | 0.861 B, 1,205,736 bytes (2.20x) | 1.085 B, 1,518,520 bytes (2.77x) |
| orders (generated) | 1,000,000 x 14 | **0.388 B, 5,430,426 bytes** | 1.138 B, 15,932,314 bytes (2.93x) | 1.090 B, 15,258,676 bytes (2.81x) | 0.803 B, 11,243,680 bytes (2.07x) | 1.048 B, 14,675,080 bytes (2.70x) |
| logs (generated) | 100,000 x 8 | **0.663 B, 530,530 bytes** | 0.958 B, 766,189 bytes (1.44x) | 1.532 B, 1,225,440 bytes (2.31x) | 1.302 B, 1,041,704 bytes (1.96x) | 1.416 B, 1,133,088 bytes (2.14x) |
| logs (generated) | 1,000,000 x 8 | **0.654 B, 5,228,815 bytes** | 0.925 B, 7,399,696 bytes (1.42x) | 1.169 B, 9,352,987 bytes (1.79x) | 1.271 B, 10,165,100 bytes (1.94x) | 1.394 B, 11,150,976 bytes (2.13x) |
| sensors (generated) | 100,000 x 8 | **0.922 B, 737,927 bytes** | 1.099 B, 879,413 bytes (1.19x) | 2.285 B, 1,828,385 bytes (2.48x) | 1.753 B, 1,402,780 bytes (1.90x) | 1.284 B, 1,026,936 bytes (1.39x) |
| sensors (generated) | 1,000,000 x 8 | **0.909 B, 7,275,012 bytes** | 1.046 B, 8,369,739 bytes (1.15x) | 1.598 B, 12,787,541 bytes (1.76x) | 1.745 B, 13,962,104 bytes (1.92x) | 1.236 B, 9,888,260 bytes (1.36x) |
| TPC-DS store_sales (benchmark, generated) | 1,000,000 x 23 | **0.804 B, 18,499,338 bytes** | 1.494 B, 34,351,503 bytes (1.86x) | 1.609 B, 36,998,186 bytes (2.00x) | 1.611 B, 37,044,892 bytes (2.00x) | 1.215 B, 27,947,348 bytes (1.51x) |
| TPC-DS web_sales (benchmark, generated) | 719,384 x 34 | **0.730 B, 17,850,212 bytes** | 1.546 B, 37,823,671 bytes (2.12x) | 1.664 B, 40,697,025 bytes (2.28x) | 1.675 B, 40,965,588 bytes (2.29x) | 1.275 B, 31,178,688 bytes (1.75x) |
| TPC-DS customer (benchmark, generated) | 100,000 x 18 | **1.721 B, 3,098,025 bytes** | 1.899 B, 3,417,888 bytes (1.10x) | 2.213 B, 3,983,727 bytes (1.29x) | 2.220 B, 3,995,844 bytes (1.29x) | 2.024 B, 3,643,304 bytes (1.18x) |
| TPC-DS customer_address (benchmark, generated) | 50,000 x 13 | **0.883 B, 573,848 bytes** | 0.916 B, 595,331 bytes (1.04x) | 1.194 B, 776,300 bytes (1.35x) | 1.358 B, 882,936 bytes (1.54x) | 1.504 B, 977,436 bytes (1.70x) |
| TPC-DS item (benchmark, generated) | 18,000 x 22 | **2.112 B, 836,259 bytes** | 2.198 B, 870,249 bytes (1.04x) | 2.567 B, 1,016,554 bytes (1.22x) | 2.639 B, 1,045,212 bytes (1.25x) | 2.627 B, 1,040,352 bytes (1.24x) |
| events (realistic generator, kore_size.md) | 98,040 x 8 | **8.514 B, 6,677,912 bytes** | 8.537 B, 6,696,077 bytes (1.00x) | 9.862 B, 7,735,068 bytes (1.16x) | 9.705 B, 7,611,964 bytes (1.14x) | 9.272 B, 7,272,148 bytes (1.09x) |
| sequences (capability demo, built for the rules) | 100,000 x 5 | **0.007 B, 3,371 bytes** | 3.724 B, 1,861,997 bytes (552.36x) | 6.278 B, 3,139,121 bytes (931.21x) | 5.232 B, 2,615,916 bytes (776.01x) | 3.892 B, 1,946,200 bytes (577.34x) |
| sequences (capability demo, built for the rules) | 1,000,000 x 5 | **0.006 B, 30,097 bytes** | 3.346 B, 16,729,265 bytes (555.84x) | 4.604 B, 23,020,822 bytes (764.89x) | 5.325 B, 26,623,036 bytes (884.57x) | 3.630 B, 18,147,520 bytes (602.97x) |
| random (control) | 100,000 x 4 | **9.717 B, 3,886,601 bytes** | 9.879 B, 3,951,528 bytes (1.02x) | 11.590 B, 4,635,823 bytes (1.19x) | 11.986 B, 4,794,276 bytes (1.23x) | 10.140 B, 4,055,884 bytes (1.04x) |
| random (control) | 1,000,000 x 4 | **9.711 B, 38,845,693 bytes** | 9.878 B, 39,511,865 bytes (1.02x) | 10.211 B, 40,842,983 bytes (1.05x) | 11.881 B, 47,524,636 bytes (1.22x) | 10.119 B, 40,476,628 bytes (1.04x) |
| random_norule (control, no rule) | 100,000 x 3 | **10.667 B, 3,200,097 bytes** | 10.667 B, 3,200,097 bytes (1.00x) | 12.301 B, 3,690,169 bytes (1.15x) | 13.048 B, 3,914,504 bytes (1.22x) | 11.041 B, 3,312,428 bytes (1.04x) |
| random_norule (control, no rule) | 1,000,000 x 3 | **10.667 B, 32,000,097 bytes** | 10.667 B, 32,000,097 bytes (1.00x) | 11.022 B, 33,065,353 bytes (1.03x) | 13.115 B, 39,344,336 bytes (1.23x) | 11.078 B, 33,234,136 bytes (1.04x) |

#### Rule mode against both version-4 settings (bytes and B/value; ratio = version-4 bytes / rule-mode bytes)

| dataset | rows | v4 default | rule mode, default | ratio | v4 level 19 | rule mode, level 19 | ratio | v4-19 / rule default | v4 default / rule-19 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| NYC TLC Yellow Taxi trip records, Jan 2024, SHA-256 c4d59da7... (real data) | 2,964,624 | 51,938,326 (0.922 B) | 32,368,578 (0.575 B) | 1.60x | 45,758,509 (0.812 B) | 29,518,146 (0.524 B) | 1.55x | 1.41x | 1.76x |
| orders (generated) | 100,000 | 1,847,660 (1.320 B) | 524,023 (0.374 B) | 3.53x | 1,663,420 (1.188 B) | 547,405 (0.391 B) | 3.04x | 3.17x | 3.38x |
| orders (generated) | 1,000,000 | 18,415,760 (1.315 B) | 5,335,436 (0.381 B) | 3.45x | 15,932,314 (1.138 B) | 5,430,426 (0.388 B) | 2.93x | 2.99x | 3.39x |
| logs (generated) | 100,000 | 831,693 (1.040 B) | 530,060 (0.663 B) | 1.57x | 766,189 (0.958 B) | 530,530 (0.663 B) | 1.44x | 1.45x | 1.57x |
| logs (generated) | 1,000,000 | 8,401,683 (1.050 B) | 5,342,670 (0.668 B) | 1.57x | 7,399,696 (0.925 B) | 5,228,815 (0.654 B) | 1.42x | 1.39x | 1.61x |
| sensors (generated) | 100,000 | 1,012,868 (1.266 B) | 747,821 (0.935 B) | 1.35x | 879,413 (1.099 B) | 737,927 (0.922 B) | 1.19x | 1.18x | 1.37x |
| sensors (generated) | 1,000,000 | 10,077,059 (1.260 B) | 7,421,241 (0.928 B) | 1.36x | 8,369,739 (1.046 B) | 7,275,012 (0.909 B) | 1.15x | 1.13x | 1.39x |
| TPC-DS store_sales (benchmark, generated) | 1,000,000 | 38,363,953 (1.668 B) | 20,690,248 (0.900 B) | 1.85x | 34,351,503 (1.494 B) | 18,499,338 (0.804 B) | 1.86x | 1.66x | 2.07x |
| TPC-DS web_sales (benchmark, generated) | 719,384 | 41,092,224 (1.680 B) | 18,989,668 (0.776 B) | 2.16x | 37,823,671 (1.546 B) | 17,850,212 (0.730 B) | 2.12x | 1.99x | 2.30x |
| TPC-DS customer (benchmark, generated) | 100,000 | 3,397,226 (1.887 B) | 3,111,945 (1.729 B) | 1.09x | 3,417,888 (1.899 B) | 3,098,025 (1.721 B) | 1.10x | 1.10x | 1.10x |
| TPC-DS customer_address (benchmark, generated) | 50,000 | 582,645 (0.896 B) | 554,481 (0.853 B) | 1.05x | 595,331 (0.916 B) | 573,848 (0.883 B) | 1.04x | 1.07x | 1.02x |
| TPC-DS item (benchmark, generated) | 18,000 | 944,113 (2.384 B) | 910,123 (2.298 B) | 1.04x | 870,249 (2.198 B) | 836,259 (2.112 B) | 1.04x | 0.96x | 1.13x |
| events (realistic generator, kore_size.md) | 98,040 | 7,204,298 (9.185 B) | 7,191,392 (9.169 B) | 1.00x | 6,696,077 (8.537 B) | 6,677,912 (8.514 B) | 1.00x | 0.93x | 1.08x |
| sequences (capability demo, built for the rules) | 100,000 | 1,861,997 (3.724 B) | 3,371 (0.007 B) | 552.36x | 1,861,997 (3.724 B) | 3,371 (0.007 B) | 552.36x | 552.36x | 552.36x |
| sequences (capability demo, built for the rules) | 1,000,000 | 18,566,016 (3.713 B) | 30,097 (0.006 B) | 616.87x | 16,729,265 (3.346 B) | 30,097 (0.006 B) | 555.84x | 555.84x | 616.87x |
| random (control) | 100,000 | 3,951,075 (9.878 B) | 3,890,063 (9.725 B) | 1.02x | 3,951,528 (9.879 B) | 3,886,601 (9.717 B) | 1.02x | 1.02x | 1.02x |
| random (control) | 1,000,000 | 39,509,650 (9.877 B) | 38,849,398 (9.712 B) | 1.02x | 39,511,865 (9.878 B) | 38,845,693 (9.711 B) | 1.02x | 1.02x | 1.02x |
| random_norule (control, no rule) | 100,000 | 3,200,097 (10.667 B) | 3,200,097 (10.667 B) | 1.00x | 3,200,097 (10.667 B) | 3,200,097 (10.667 B) | 1.00x | 1.00x | 1.00x |
| random_norule (control, no rule) | 1,000,000 | 32,000,097 (10.667 B) | 32,000,097 (10.667 B) | 1.00x | 32,000,097 (10.667 B) | 32,000,097 (10.667 B) | 1.00x | 1.00x | 1.00x |

#### More baselines (bytes; ratio = baseline / rule mode level 19)

| dataset | rows | rule mode, level 19 | DuckDB file | SQLite | SQLite + key index | Parquet zstd-22 dictionary off | dictionary on | sorted by key (dictionary on), key |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| NYC TLC Yellow Taxi trip records, Jan 2024, SHA-256 c4d59da7... (real data) | 2,964,624 | 29,518,146 | 84,422,656 (2.86x) | 385,421,312 (13.06x) | 436,043,776 (14.77x) | 49,911,240 (1.69x) | 45,674,181 (1.55x) | 41,672,004 (1.41x), `tpep_pickup_datetime` |
| orders (generated) | 100,000 | 547,405 | 3,158,016 (5.77x) | 8,720,384 (15.93x) | 9,904,128 (18.09x) | 2,018,221 (3.69x) | 1,918,535 (3.50x) | 1,918,535 (3.50x), `order_id` |
| orders (generated) | 1,000,000 | 5,430,426 | 28,061,696 (5.17x) | 87,322,624 (16.08x) | 99,356,672 (18.30x) | 20,237,096 (3.73x) | 15,258,676 (2.81x) | 15,258,676 (2.81x), `order_id` |
| logs (generated) | 100,000 | 530,530 | 2,371,584 (4.47x) | 8,810,496 (16.61x) | 11,706,368 (22.07x) | 968,650 (1.83x) | 1,225,440 (2.31x) | 1,225,440 (2.31x), `ts` |
| logs (generated) | 1,000,000 | 5,228,815 | 21,508,096 (4.11x) | 88,625,152 (16.95x) | 117,878,784 (22.54x) | 9,675,884 (1.85x) | 9,352,987 (1.79x) | 9,352,987 (1.79x), `ts` |
| sensors (generated) | 100,000 | 737,927 | 2,109,440 (2.86x) | 6,594,560 (8.94x) | 8,077,312 (10.95x) | 1,229,179 (1.67x) | 1,828,385 (2.48x) | 1,828,385 (2.48x), `ts_ms` |
| sensors (generated) | 1,000,000 | 7,275,012 | 18,886,656 (2.60x) | 65,695,744 (9.03x) | 80,740,352 (11.10x) | 12,123,853 (1.67x) | 12,787,541 (1.76x) | 12,787,541 (1.76x), `ts_ms` |
| TPC-DS store_sales (benchmark, generated) | 1,000,000 | 18,499,338 | 45,887,488 (2.48x) | 148,942,848 (8.05x) | 160,575,488 (8.68x) | 37,944,304 (2.05x) | 36,998,186 (2.00x) | 36,998,186 (2.00x), `ss_ticket_number` |
| TPC-DS web_sales (benchmark, generated) | 719,384 | 17,850,212 | 45,625,344 (2.56x) | 147,709,952 (8.27x) | 155,963,392 (8.74x) | 40,825,430 (2.29x) | 40,697,025 (2.28x) | 40,697,025 (2.28x), `ws_order_number` |
| TPC-DS customer (benchmark, generated) | 100,000 | 3,098,025 | 5,255,168 (1.70x) | 11,587,584 (3.74x) | 12,738,560 (4.11x) | 3,883,147 (1.25x) | 3,983,727 (1.29x) | 3,983,727 (1.29x), `c_customer_sk` |
| TPC-DS customer_address (benchmark, generated) | 50,000 | 573,848 | 1,585,152 (2.76x) | 6,082,560 (10.60x) | 6,623,232 (11.54x) | 815,863 (1.42x) | 776,300 (1.35x) | 776,300 (1.35x), `ca_address_sk` |
| TPC-DS item (benchmark, generated) | 18,000 | 836,259 | 2,895,872 (3.46x) | 5,394,432 (6.45x) | 5,578,752 (6.67x) | 984,524 (1.18x) | 1,016,554 (1.22x) | 1,016,554 (1.22x), `i_item_sk` |
| events (realistic generator, kore_size.md) | 98,040 | 6,677,912 | 61,616,128 (9.23x) | 57,352,192 (8.59x) | 64,667,648 (9.68x) | 7,621,793 (1.14x) | 7,735,068 (1.16x) | 7,923,878 (1.19x), `event_id` |
| sequences (capability demo, built for the rules) | 100,000 | 3,371 | 2,371,584 (703.53x) | 3,964,928 (1176.19x) | 5,648,384 (1675.58x) | 2,147,719 (637.12x) | 3,139,121 (931.21x) | 3,738,324 (1108.97x), `fibonacci` |
| sequences (capability demo, built for the rules) | 1,000,000 | 30,097 | 21,770,240 (723.34x) | 41,242,624 (1370.32x) | 58,306,560 (1937.29x) | 21,705,849 (721.20x) | 23,020,822 (764.89x) | 30,554,030 (1015.19x), `fibonacci` |
| random (control) | 100,000 | 3,886,601 | 4,468,736 (1.15x) | 6,742,016 (1.73x) | 8,425,472 (2.17x) | 4,003,207 (1.03x) | 4,635,823 (1.19x) | 4,633,508 (1.19x), `u64` |
| random (control) | 1,000,000 | 38,845,693 | 42,217,472 (1.09x) | 67,325,952 (1.73x) | 84,389,888 (2.17x) | 40,017,120 (1.03x) | 40,842,983 (1.05x) | 40,432,668 (1.04x), `u64` |
| random_norule (control, no rule) | 100,000 | 3,200,097 | 3,682,304 (1.15x) | 5,865,472 (1.83x) | 7,548,928 (2.36x) | 3,250,952 (1.02x) | 3,690,169 (1.15x) | 3,687,885 (1.15x), `u64` |
| random_norule (control, no rule) | 1,000,000 | 32,000,097 | 35,139,584 (1.10x) | 58,658,816 (1.83x) | 75,722,752 (2.37x) | 32,496,278 (1.02x) | 33,065,353 (1.03x) | 32,655,161 (1.02x), `u64` |

#### Encode (paired: v4 default and rule mode default, alternating order; medians of 3; one run each at level 19)

| dataset | rows | v4 default | rule mode default | ratio per repetition | v4 level 19 | rule mode level 19 |
|---|---:|---:|---:|---:|---:|---:|
| NYC TLC Yellow Taxi trip records, Jan 2024, SHA-256 c4d59da7... (real data) | 2,964,624 | 3563 ms | 36634 ms | 11.88, 8.94, 11.13 | 60119 ms | 567292 ms |
| orders (generated) | 100,000 | 179 ms | 862 ms | 4.75, 4.35, 5.04 | 546 ms | 1245 ms |
| orders (generated) | 1,000,000 | 1490 ms | 5596 ms | 4.11, 5.68, 2.62 | 9617 ms | 23060 ms |
| logs (generated) | 100,000 | 225 ms | 1991 ms | 10.68, 7.10, 7.12 | 2758 ms | 3704 ms |
| logs (generated) | 1,000,000 | 2685 ms | 7638 ms | 3.30, 2.84, 2.13 | 43804 ms | 54751 ms |
| sensors (generated) | 100,000 | 167 ms | 485 ms | 3.53, 2.54, 2.90 | 355 ms | 1885 ms |
| sensors (generated) | 1,000,000 | 673 ms | 2683 ms | 4.43, 4.16, 3.78 | 5461 ms | 23444 ms |
| TPC-DS store_sales (benchmark, generated) | 1,000,000 | 2168 ms | 13028 ms | 5.87, 5.65, 6.61 | 23619 ms | 120136 ms |
| TPC-DS web_sales (benchmark, generated) | 719,384 | 2921 ms | 17482 ms | 8.15, 6.96, 5.80 | 25711 ms | 98408 ms |
| TPC-DS customer (benchmark, generated) | 100,000 | 640 ms | 1692 ms | 1.80, 2.46, 2.64 | 2974 ms | 3567 ms |
| TPC-DS customer_address (benchmark, generated) | 50,000 | 272 ms | 1023 ms | 1.73, 3.76, 3.26 | 759 ms | 1531 ms |
| TPC-DS item (benchmark, generated) | 18,000 | 177 ms | 373 ms | 1.55, 1.98, 2.64 | 1162 ms | 1219 ms |
| events (realistic generator, kore_size.md) | 98,040 | 755 ms | 1221 ms | 1.62, 1.83, 1.49 | 54399 ms | 64741 ms |
| sequences (capability demo, built for the rules) | 100,000 | 74 ms | 261 ms | 3.50, 3.37, 4.48 | 188 ms | 304 ms |
| sequences (capability demo, built for the rules) | 1,000,000 | 146 ms | 392 ms | 2.69, 2.91, 2.08 | 1729 ms | 3217 ms |
| random (control) | 100,000 | 37 ms | 95 ms | 3.38, 2.40, 2.37 | 275 ms | 473 ms |
| random (control) | 1,000,000 | 644 ms | 1465 ms | 2.94, 2.27, 2.38 | 5308 ms | 9312 ms |
| random_norule (control, no rule) | 100,000 | 54 ms | 91 ms | 2.16, 1.45, 1.68 | 231 ms | 510 ms |
| random_norule (control, no rule) | 1,000,000 | 471 ms | 1456 ms | 3.34, 2.94, 2.75 | 4902 ms | 7700 ms |

#### Read (full read and one derived column; the 4 files read in rotating order per repetition; medians of 3; ratios = rule mode level 19 / version 4, per repetition)

Noise floor: in the first run two byte-identical files read by the same code gave paired ratios of 1.31 and 1.50 on this machine (`random_norule`).

| dataset | rows | full read v4 default / v4-19 / rule-19 | ratio vs v4 default | ratio vs v4-19 | column | one column v4 default / v4-19 / rule-19 | ratio vs v4 default | ratio vs v4-19 |
|---|---:|---:|---:|---:|---|---:|---:|---:|
| NYC TLC Yellow Taxi trip records, Jan 2024, SHA-256 c4d59da7... (real data) | 2,964,624 | 295.0 / 460.0 / 1544.0 ms | 3.58, 5.83, 5.23 | 4.69, 2.77, 3.23 | `VendorID` | 36.4 / 36.9 / 452.9 ms | 13.70, 13.82, 8.56 | 13.51, 10.79, 11.39 |
| orders (generated) | 100,000 | 42.4 / 57.3 / 141.1 ms | 1.01, 4.14, 10.17 | 2.95, 1.58, 2.46 | `country` | 5.6 / 5.5 / 40.3 ms | 5.13, 6.06, 8.46 | 6.09, 9.18, 7.05 |
| orders (generated) | 1,000,000 | 117.6 / 149.5 / 463.4 ms | 4.14, 2.45, 4.00 | 4.41, 2.69, 2.61 | `country` | 104.0 / 96.3 / 353.2 ms | 3.03, 3.13, 4.05 | 3.27, 3.57, 4.21 |
| logs (generated) | 100,000 | 75.6 / 77.4 / 172.3 ms | 7.19, 2.22, 1.42 | 2.13, 3.41, 2.22 | `service` | 5.8 / 5.5 / 82.5 ms | 13.09, 13.87, 15.69 | 15.14, 14.80, 15.22 |
| logs (generated) | 1,000,000 | 165.9 / 171.5 / 654.3 ms | 4.67, 3.08, 4.17 | 3.40, 3.42, 3.82 | `service` | 72.4 / 76.6 / 287.3 ms | 3.97, 4.08, 3.74 | 2.78, 4.04, 3.96 |
| sensors (generated) | 100,000 | 11.6 / 12.4 / 25.4 ms | 2.19, 1.02, 1.53 | 1.44, 2.38, 1.08 | `ts_ms` | 1.1 / 1.1 / 1.9 ms | 1.90, 2.04, 1.45 | 1.96, 2.04, 1.54 |
| sensors (generated) | 1,000,000 | 64.3 / 64.9 / 124.0 ms | 1.17, 2.22, 1.93 | 1.70, 2.00, 1.57 | `ts_ms` | 12.2 / 11.4 / 19.5 ms | 1.53, 1.60, 1.68 | 1.45, 1.72, 1.79 |
| TPC-DS store_sales (benchmark, generated) | 1,000,000 | 167.1 / 185.8 / 544.5 ms | 3.98, 3.26, 2.60 | 4.46, 2.93, 2.31 | `ss_sales_price` | 35.5 / 50.3 / 353.7 ms | 10.72, 9.77, 7.66 | 6.21, 13.45, 7.03 |
| TPC-DS web_sales (benchmark, generated) | 719,384 | 380.5 / 293.4 / 604.4 ms | 1.44, 1.74, 2.01 | 1.74, 2.26, 2.78 | `ws_ship_date_sk` | 18.8 / 44.9 / 84.9 ms | 4.40, 9.97, 1.96 | 1.16, 3.80, 2.14 |
| TPC-DS customer (benchmark, generated) | 100,000 | 101.7 / 23.6 / 105.2 ms | 1.21, 1.03, 1.47 | 2.76, 4.46, 2.20 | `c_first_sales_date_sk` | 2.2 / 2.2 / 35.7 ms | 7.08, 16.64, 16.83 | 7.66, 20.26, 1.15 |
| TPC-DS customer_address (benchmark, generated) | 50,000 | 55.7 / 68.7 / 112.9 ms | 2.97, 1.47, 2.06 | 3.93, 1.19, 1.35 | `ca_country` | 3.3 / 3.0 / 36.6 ms | 12.15, 12.65, 9.40 | 14.44, 11.05, 11.43 |
| TPC-DS item (benchmark, generated) | 18,000 | 64.9 / 91.5 / 151.0 ms | 3.56, 0.80, 2.49 | 1.65, 0.58, 2.04 | `i_category_id` | 0.2 / 0.1 / 4.1 ms | 41.37, 21.31, 21.50 | 44.18, 17.74, 40.33 |
| events (realistic generator, kore_size.md) | 98,040 | 147.8 / 204.7 / 170.3 ms | 1.20, 1.26, 1.15 | 0.75, 1.30, 1.37 | `occurred_at` | 22.9 / 42.2 / 29.3 ms | 1.28, 0.52, 4.56 | 4.00, 0.29, 0.82 |
| sequences (capability demo, built for the rules) | 100,000 | 2.6 / 5.7 / 7.2 ms | 0.64, 6.00, 2.94 | 3.15, 1.27, 0.62 | `arithmetic_1pct_exceptions` | 0.7 / 0.7 / 1.7 ms | 2.54, 3.35, 2.16 | 2.37, 3.74, 2.51 |
| sequences (capability demo, built for the rules) | 1,000,000 | 14.6 / 22.0 / 83.9 ms | 4.63, 6.15, 5.70 | 3.67, 4.33, 3.77 | `arithmetic_1pct_exceptions` | 7.0 / 8.0 / 22.6 ms | 3.49, 2.68, 2.83 | 3.55, 2.69, 2.48 |
| random (control) | 100,000 | 17.5 / 7.1 / 12.8 ms | 0.73, 0.71, 1.02 | 1.55, 1.87, 1.05 | `f64_unit` | 1.6 / 1.5 / 2.3 ms | 1.63, 0.19, 1.50 | 1.54, 1.54, 1.43 |
| random (control) | 1,000,000 | 80.2 / 82.5 / 80.7 ms | 0.99, 0.88, 1.55 | 1.07, 0.85, 1.07 | `f64_unit` | 21.0 / 23.4 / 49.3 ms | 2.35, 1.81, 2.20 | 2.33, 1.47, 2.16 |
| random_norule (control, no rule) | 100,000 | 11.4 / 11.6 / 7.0 ms | 0.85, 0.70, 0.60 | 0.71, 0.61, 0.97 | `-` | - | - | - |
| random_norule (control, no rule) | 1,000,000 | 118.3 / 83.3 / 109.8 ms | 0.91, 1.14, 0.89 | 1.72, 0.89, 1.05 | `-` | - | - | - |

#### Per column, rule mode level 19 (largest size of each dataset)


**NYC TLC Yellow Taxi trip records, Jan 2024, SHA-256 c4d59da7... (real data), 2,964,624 rows** (v4 level 19 file 45,758,509 bytes, rule mode level 19 29,518,146 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| VendorID | 303,030 | 243,930 | = lookup(extra) (48 keys) |
| tpep_pickup_datetime | 10,533,559 | 6,216,444 | int: delta |
| tpep_dropoff_datetime | 10,650,855 | 6,366,795 | int: runs |
| passenger_count | 451,720 | - | none (v4 payload kept) |
| trip_distance | 4,721,215 | 3,913,977 | float: k/10^2, 0 exceptions, k: affine 165+0*i |
| RatecodeID | 172,331 | 38,557 | = lookup(fare_amount) (8970 keys) |
| store_and_fwd_flag | 23,660 | 13,459 | = lookup(Airport_fee) (4 keys) |
| PULocationID | 2,528,774 | 2,337,808 | int: affine 162+0*i |
| DOLocationID | 2,777,746 | 2,595,671 | int: affine 162+0*i |
| payment_type | 309,005 | 278,465 | = lookup(congestion_surcharge) (7 keys) |
| fare_amount | 3,024,578 | 2,890,529 | float: k/10^1, 111629 exceptions, k: affine 128+0*i |
| extra | 713,168 | 522,731 | float: k/10^1, 41439 exceptions, k: plain (no rule) |
| mta_tax | 159,985 | 18,296 | = lookup(fare_amount) (8970 keys) |
| tip_amount | 3,462,073 | 2,952,835 | float: k/10^2, 0 exceptions, k: plain (no rule) |
| tolls_amount | 352,542 | 169,642 | = lookup(RatecodeID) (8 keys) |
| improvement_surcharge | 109,204 | 4,257 | = lookup(fare_amount) (8970 keys) |
| total_amount | 4,816,308 | 339,309 | = improvement_surcharge + fare_amount + mta_tax + extra + tip_amount + congestion_surcharge + tolls_amount + Airport_fee (sum), in units of 10^-2 |
| congestion_surcharge | 336,360 | 136,669 | = lookup(improvement_surcharge) (5 keys) |
| Airport_fee | 311,780 | 26,591 | = lookup(PULocationID) (260 keys) |

**orders (generated), 1,000,000 rows** (v4 level 19 file 15,932,314 bytes, rule mode level 19 5,430,426 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| order_id | 56 | - | none (v4 payload kept) |
| ordered_at | 675,368 | - | none (v4 payload kept) |
| customer | 2,423,034 | 2,284,728 | int: affine 24887+0*i |
| city | 488,428 | 488,112 | categorical Markov ranks |
| country | 313,370 | 1,781 | = lookup(city) (23 keys) |
| prev_city | 488,460 | 14 | = city shifted by 1 |
| product | 1,361,840 | 1,337,827 | int: affine 256+0*i |
| qty | 405,524 | 403,755 | int: affine 5+0*i |
| unit_price | 1,486,997 | 1,714 | = lookup(product) (500 keys) |
| line_total | 2,209,482 | 287 | = qty * unit_price (f64), rounded to 2 decimals |
| unit_cents | 1,540,498 | 1,054 | = lookup(unit_price) (493 keys) |
| subtotal_cents | 2,058,590 | 68 | = qty * unit_cents |
| shipping_cents | 238,605 | 235,325 | int: affine 499+0*i |
| total_cents | 2,241,661 | 68 | = subtotal_cents + shipping_cents |

**logs (generated), 1,000,000 rows** (v4 level 19 file 7,399,696 bytes, rule mode level 19 5,228,815 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| ts | 110,911 | - | none (v4 payload kept) |
| service | 302,858 | 47 | = lookup(host) (5 keys) |
| host | 302,851 | 300,834 | categorical Markov ranks |
| level | 171,504 | - | none (v4 payload kept) |
| message | 5,018,259 | 3,231,012 | text template + numeric slots |
| latency_ms | 1,197,014 | 1,164,395 | int: affine 76+0*i |
| status | 81,508 | 35,395 | = lookup(level) (4 keys) |
| request_id | 214,572 | - | none (v4 payload kept) |

**sensors (generated), 1,000,000 rows** (v4 level 19 file 8,369,739 bytes, rule mode level 19 7,275,012 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| ts_ms | 693,794 | 550,632 | int: affine 1767225600009+1000*i |
| ts_text | 240,575 | 209,349 | timestamp text: affine 1767225601+60*i |
| temp_c | 1,185,741 | 619,363 | float: k/10^2, 0 exceptions, k: delta |
| pressure | 5,642,850 | 5,332,748 | float: xor residual vs affine a+i*b |
| counter | 251,321 | - | none (v4 payload kept) |
| battery | 600 | 234 | int: runs |
| device | 137 | 46 | int: runs |
| cycle | 354,502 | 311,175 | int: lag 192 |

**TPC-DS store_sales (benchmark, generated), 1,000,000 rows** (v4 level 19 file 34,351,503 bytes, rule mode level 19 18,499,338 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| ss_sold_date_sk | 216,922 | 210,924 | int: runs |
| ss_sold_time_sk | 308,632 | 265,384 | int: runs |
| ss_item_sk | 373,396 | 364,855 | int: affine 8948+0*i |
| ss_customer_sk | 330,125 | 279,621 | int: runs |
| ss_cdemo_sk | 396,916 | 328,029 | int: runs |
| ss_hdemo_sk | 246,566 | 238,326 | int: plain (no rule) |
| ss_addr_sk | 271,385 | 268,312 | int: runs |
| ss_store_sk | 158,258 | 102,565 | int: runs |
| ss_promo_sk | 1,251,183 | 1,216,197 | int: affine 154+0*i |
| ss_ticket_number | 44,108 | 34,825 | int: runs |
| ss_quantity | 1,085,818 | 839,169 | int: affine 51+0*i |
| ss_wholesale_cost | 2,295,940 | - | none (v4 payload kept) |
| ss_list_price | 2,423,404 | - | none (v4 payload kept) |
| ss_sales_price | 2,338,985 | 608,597 | = ss_ext_sales_price / ss_quantity (f64) |
| ss_ext_discount_amt | 872,723 | 702,444 | float: k/10^2, 25270 exceptions, k: plain (no rule) |
| ss_ext_sales_price | 3,020,397 | 504,307 | = ss_ext_discount_amt + ss_net_paid (f64) |
| ss_ext_wholesale_cost | 3,057,060 | 607,493 | = ss_quantity * ss_wholesale_cost (f64) |
| ss_ext_list_price | 3,232,490 | 607,672 | = ss_quantity * ss_list_price (f64) |
| ss_ext_tax | 2,312,720 | 2,039,694 | float: k/10^2, 112595 exceptions, k: plain (no rule) |
| ss_coupon_amt | 876,939 | 60,507 | = ss_ext_discount_amt shifted by 0 |
| ss_net_paid | 3,028,370 | 803,344 | = ss_ext_wholesale_cost + ss_net_profit (f64) |
| ss_net_paid_inc_tax | 3,115,765 | 604,518 | = ss_ext_tax + ss_net_paid (f64) |
| ss_net_profit | 3,092,643 | - | none (v4 payload kept) |

**TPC-DS web_sales (benchmark, generated), 719,384 rows** (v4 level 19 file 37,823,671 bytes, rule mode level 19 17,850,212 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| ws_sold_date_sk | 131,219 | 126,030 | int: runs |
| ws_sold_time_sk | 206,235 | 172,668 | int: runs |
| ws_ship_date_sk | 817,654 | 749,599 | = ws_sold_date_sk + offset residual |
| ws_item_sk | 288,805 | 281,621 | int: affine 9131+0*i |
| ws_bill_customer_sk | 212,310 | 176,121 | int: runs |
| ws_bill_cdemo_sk | 255,355 | 218,764 | int: recurrence v=+874343 |
| ws_bill_hdemo_sk | 146,210 | 144,312 | int: plain (no rule) |
| ws_bill_addr_sk | 168,333 | 167,730 | int: runs |
| ws_ship_customer_sk | 212,556 | 176,217 | int: runs |
| ws_ship_cdemo_sk | 255,222 | 210,527 | int: runs |
| ws_ship_hdemo_sk | 146,585 | 143,754 | int: plain (no rule) |
| ws_ship_addr_sk | 168,448 | 167,760 | int: runs |
| ws_web_page_sk | 524,153 | 519,283 | int: plain (no rule) |
| ws_web_site_sk | 445,554 | 429,530 | int: affine 14+0*i |
| ws_ship_mode_sk | 405,968 | 396,679 | int: affine 11+0*i |
| ws_warehouse_sk | 233,286 | 217,078 | int: affine 3+0*i |
| ws_promo_sk | 909,934 | 888,848 | int: affine 151+0*i |
| ws_order_number | 31,864 | 24,293 | int: runs |
| ws_quantity | 613,317 | 604,475 | int: affine 50+0*i |
| ws_wholesale_cost | 1,709,252 | - | none (v4 payload kept) |
| ws_list_price | 1,850,488 | 141,337 | = ws_ext_list_price / ws_quantity (f64) |
| ws_sales_price | 1,805,484 | 141,132 | = ws_ext_sales_price / ws_quantity (f64) |
| ws_ext_discount_amt | 2,349,596 | - | none (v4 payload kept) |
| ws_ext_sales_price | 2,353,705 | 49,029 | = ws_coupon_amt + ws_net_paid (f64) |
| ws_ext_wholesale_cost | 2,323,483 | 149,865 | = ws_quantity * ws_wholesale_cost (f64) |
| ws_ext_list_price | 2,498,451 | 132,241 | = ws_ext_discount_amt + ws_ext_sales_price (f64) |
| ws_ext_tax | 1,784,508 | 1,545,463 | float: k/10^2, 85332 exceptions, k: plain (no rule) |
| ws_coupon_amt | 654,034 | 513,062 | float: k/10^2, 19104 exceptions, k: plain (no rule) |
| ws_ext_ship_cost | 2,225,100 | - | none (v4 payload kept) |
| ws_net_paid | 2,355,003 | 266,974 | = ws_ext_wholesale_cost + ws_net_profit (f64) |
| ws_net_paid_inc_tax | 2,419,993 | 131,613 | = ws_ext_tax + ws_net_paid (f64) |
| ws_net_paid_inc_ship | 2,415,683 | 135,423 | = ws_ext_ship_cost + ws_net_paid (f64) |
| ws_net_paid_inc_ship_tax | 2,491,330 | 130,561 | = ws_ext_tax + ws_net_paid_inc_ship (f64) |
| ws_net_profit | 2,413,390 | - | none (v4 payload kept) |

**TPC-DS customer (benchmark, generated), 100,000 rows** (v4 level 19 file 3,417,888 bytes, rule mode level 19 3,098,025 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| c_customer_sk | 25 | - | none (v4 payload kept) |
| c_customer_id | 103,300 | - | none (v4 payload kept) |
| c_current_cdemo_sk | 301,379 | 293,153 | int: affine 1253635+-6*i |
| c_current_hdemo_sk | 211,351 | 181,990 | int: plain (no rule) |
| c_current_addr_sk | 251,535 | 228,578 | int: affine 24905+0*i |
| c_first_shipto_date_sk | 188,820 | 170,092 | int: affine 2450869+0*i |
| c_first_sales_date_sk | 206,592 | 9,971 | = 1*c_first_shipto_date_sk -30 |
| c_salutation | 34,365 | 33,941 | categorical Markov ranks |
| c_first_name | 161,175 | - | none (v4 payload kept) |
| c_last_name | 181,022 | - | none (v4 payload kept) |
| c_preferred_cust_flag | 19,367 | 15,451 | categorical Markov ranks |
| c_birth_day | 64,765 | 63,240 | int: affine 15+0*i |
| c_birth_month | 51,792 | 47,458 | int: affine 7+0*i |
| c_birth_year | 99,999 | 77,392 | int: affine 1958+0*i |
| c_birth_country | 97,885 | - | none (v4 payload kept) |
| c_login | 3,192 | 2,883 | = lookup(c_preferred_cust_flag) (3 keys) |
| c_email_address | 1,303,945 | - | none (v4 payload kept) |
| c_last_review_date_sk | 136,759 | 126,062 | int: affine 2452462+0*i |

**TPC-DS customer_address (benchmark, generated), 50,000 rows** (v4 level 19 file 595,331 bytes, rule mode level 19 573,848 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| ca_address_sk | 23 | - | none (v4 payload kept) |
| ca_address_id | 51,697 | - | none (v4 payload kept) |
| ca_street_number | 71,880 | - | none (v4 payload kept) |
| ca_street_name | 103,773 | 101,022 | text template + numeric slots |
| ca_street_type | 28,295 | - | none (v4 payload kept) |
| ca_suite_number | 39,233 | - | none (v4 payload kept) |
| ca_city | 59,973 | 56,660 | categorical Markov ranks |
| ca_county | 86,337 | - | none (v4 payload kept) |
| ca_state | 33,700 | - | none (v4 payload kept) |
| ca_zip | 87,284 | - | none (v4 payload kept) |
| ca_country | 1,714 | 1,397 | = lookup(ca_gmt_offset) (7 keys) |
| ca_gmt_offset | 18,199 | 4,232 | = lookup(ca_state) (52 keys) |
| ca_location_type | 12,811 | 11,794 | categorical Markov ranks |

**TPC-DS item (benchmark, generated), 18,000 rows** (v4 level 19 file 870,249 bytes, rule mode level 19 836,259 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| i_item_sk | 22 | - | none (v4 payload kept) |
| i_item_id | 11,677 | - | none (v4 payload kept) |
| i_rec_start_date | 231 | - | none (v4 payload kept) |
| i_rec_end_date | 52 | - | none (v4 payload kept) |
| i_item_desc | 447,424 | - | none (v4 payload kept) |
| i_current_price | 40,655 | 33,191 | float: k/10^2, 2342 exceptions, k: affine 420+0*i |
| i_wholesale_cost | 32,630 | - | none (v4 payload kept) |
| i_brand_id | 27,517 | 25,481 | int: plain (no rule) |
| i_brand | 25,989 | 20,547 | text template + numeric slots |
| i_class_id | 8,437 | 8,307 | int: affine 4+0*i |
| i_class | 15,261 | - | none (v4 payload kept) |
| i_category_id | 8,074 | 131 | = lookup(i_category) (11 keys) |
| i_category | 8,030 | 487 | = lookup(i_class) (100 keys) |
| i_manufact_id | 23,334 | 22,626 | int: runs |
| i_manufact | 24,849 | 23,456 | categorical Markov ranks |
| i_size | 4,675 | - | none (v4 payload kept) |
| i_formulation | 121,785 | - | none (v4 payload kept) |
| i_color | 14,654 | - | none (v4 payload kept) |
| i_units | 10,376 | 10,337 | categorical Markov ranks |
| i_container | 172 | 110 | = lookup(i_rec_end_date) (4 keys) |
| i_manager_id | 15,920 | 14,891 | int: affine 28+0*i |
| i_product_name | 27,825 | - | none (v4 payload kept) |

**events (realistic generator, kore_size.md), 98,040 rows** (v4 level 19 file 6,696,077 bytes, rule mode level 19 6,677,912 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| event_id | 3,137,283 | - | none (v4 payload kept) |
| occurred_at | 69,507 | 60,556 | timestamp text: affine 1735689614+30*i |
| recorded_at | 30 | 25 | timestamp text: runs |
| source | 38,490 | 36,993 | categorical Markov ranks |
| kind | 31,968 | 29,590 | categorical Markov ranks |
| text | 3,234,745 | - | none (v4 payload kept) |
| tags | 183,810 | 178,551 | text template + numeric slots |
| redacted | 20 | - | none (v4 payload kept) |

**sequences (capability demo, built for the rules), 1,000,000 rows** (v4 level 19 file 16,729,265 bytes, rule mode level 19 30,097 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| fibonacci | 8,000,001 | 71 | int: recurrence v=+1*v[-1]+1*v[-2] |
| affine_geometric | 8,000,001 | 70 | int: recurrence v=+3*v[-1]+1 |
| quadratic | 691,749 | 72 | int: delta-of-delta |
| arithmetic | 55 | - | none (v4 payload kept) |
| arithmetic_1pct_exceptions | 37,275 | 29,702 | = arithmetic shifted by 0 |

**random (control), 1,000,000 rows** (v4 level 19 file 39,511,865 bytes, rule mode level 19 38,845,693 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| u64 | 8,000,001 | - | none (v4 payload kept) |
| f64_bits | 8,000,001 | - | none (v4 payload kept) |
| f64_unit | 7,511,742 | 6,845,607 | float: xor residual vs previous value |
| hex | 16,000,003 | - | none (v4 payload kept) |

**random_norule (control, no rule), 1,000,000 rows** (v4 level 19 file 32,000,097 bytes, rule mode level 19 32,000,097 bytes)

| column | v4-19 payload | rule-mode payload | rule |
|---|---:|---:|---|
| u64 | 8,000,001 | - | none (v4 payload kept) |
| f64_bits | 8,000,001 | - | none (v4 payload kept) |
| hex | 16,000,003 | - | none (v4 payload kept) |

### NYC taxi: per column, with exception counts (rule mode level 19 against v4 level 19)

"Exceptions" are the rows the rule does not predict exactly:

* lookups: rows whose value differs from the mapped value;
* the sum: rows whose total differs from the sum of the terms by a non-zero number of cents;
* the decimal family: values that are not exactly k/10^s.

The bytes are those of the streams that carry them (for zero-heavy residual streams this includes the coded
zero runs).

| column | type | v4 level 19 payload | rule-mode payload (level 19) | rule | exceptions | exception / residual bytes |
|---|---|---:|---:|---|---:|---:|
| VendorID | I64 | 303,030 | 243,930 | = lookup(extra) (48 keys, map 33 bytes) | 498,806 (16.83 %) | 243,881 |
| tpep_pickup_datetime | I64 | 10,533,559 | 6,216,444 | int: delta | - | - |
| tpep_dropoff_datetime | I64 | 10,650,855 | 6,366,795 | int: runs | - | - |
| passenger_count | I64 | 451,720 | 451,720 | v4 payload kept | - | - |
| trip_distance | F64 | 4,721,215 | 3,913,977 | float: k/10^2, 0 exceptions, k: affine 165+0*i | 0 (0.00 %) not k/10^2 | - |
| RatecodeID | I64 | 172,331 | 38,557 | = lookup(fare_amount) (8970 keys, map 1065 bytes) | 38,360 (1.29 %) | 37,474 |
| store_and_fwd_flag | Str | 23,660 | 13,459 | = lookup(Airport_fee) (4 keys, map 8 bytes) | 11,336 (0.38 %) | 13,435 |
| PULocationID | I64 | 2,528,774 | 2,337,808 | int: affine 162+0*i | - | - |
| DOLocationID | I64 | 2,777,746 | 2,595,671 | int: affine 162+0*i | - | - |
| payment_type | I64 | 309,005 | 278,465 | = lookup(congestion_surcharge) (7 keys, map 8 bytes) | 487,492 (16.44 %) | 278,441 |
| fare_amount | F64 | 3,024,578 | 2,890,529 | float: k/10^1, 111629 exceptions, k: affine 128+0*i | 111,629 (3.77 %) not k/10^1 | - |
| extra | F64 | 713,168 | 522,731 | float: k/10^1, 41439 exceptions, k: plain (no rule) | 41,439 (1.40 %) not k/10^1 | - |
| mta_tax | F64 | 159,985 | 18,296 | = lookup(fare_amount) (8970 keys, map 1394 bytes) | 11,185 (0.38 %) | 16,884 |
| tip_amount | F64 | 3,462,073 | 2,952,835 | float: k/10^2, 0 exceptions, k: plain (no rule) | 0 (0.00 %) not k/10^2 | - |
| tolls_amount | F64 | 352,542 | 169,642 | = lookup(RatecodeID) (8 keys, map 26 bytes) | 153,926 (5.19 %) | 169,600 |
| improvement_surcharge | F64 | 109,204 | 4,257 | = lookup(fare_amount) (8970 keys, map 794 bytes) | 1,817 (0.06 %) | 3,445 |
| total_amount | F64 | 4,816,308 | 339,309 | = improvement_surcharge + fare_amount + mta_tax + extra + tip_amount + congestion_surcharge + tolls_amount + Airport_fee (sum), in units of 10^-2 | 761,838 (25.70 %) off, 0 raw | 339,289 |
| congestion_surcharge | F64 | 336,360 | 136,669 | = lookup(improvement_surcharge) (5 keys, map 28 bytes) | 357,456 (12.06 %) | 136,625 |
| Airport_fee | F64 | 311,780 | 26,591 | = lookup(PULocationID) (260 keys, map 63 bytes) | 154,820 (5.22 %) | 26,511 |
| **sum of payloads** | | 45,757,893 | 29,517,685 | | | |

What the rules found on the taxi data, checked independently in integer cents with DuckDB
(`scripts/kore_rules/analyze_taxi.py`):

* **total_amount.**
  * The full formula `fare_amount + extra + mta_tax + tip_amount + tolls_amount + improvement_surcharge +
    congestion_surcharge + Airport_fee` (nulls as 0) holds exactly in cents on 2,202,786 of 2,964,624 rows
    (74.30 %). In plain f64 arithmetic only 1,938,930 rows (65.40 %) are exactly equal, which is why the
    family works in cents.
  * The search found the same 8 terms (in another order) and stores the other 761,838 rows (25.70 %) as cents
    differences. The column costs 339,309 bytes against 4,816,308 in version 4.
  * No term is missing from the formula: leaving out any one term lowers the hold count (the best 7-term
    subset, without tolls, holds on 2,033,310 rows).
  * The misses are fixed amounts. -2.50 on 591,045 rows (the components sum to 2.50 more than the total),
    +2.50 on 129,332, -4.25 on 22,451, -1.75 on 18,149, +4.50 on 792, and 70 rows with other differences.
    2.50 is the congestion surcharge and 1.75 the airport fee, so these rows record a fee in one place and not
    in the other. That is a real inconsistency in the data, not float noise. 35,504 rows have a negative
    total.
* **dropoff >= pickup.**
  * 56 rows have dropoff < pickup and 814 rows have dropoff = pickup. 18 pickups and 611 dropoffs fall
    outside January 2024. The trip duration is 36 s / 698 s / 3,627 s at the 1st / 50th / 99th percentile.
  * No family expresses an inequality. The offset relation `dropoff = pickup + residual` was tested but did not
    win: a duration in microseconds needs about as many bits as dropoff's own delta. Both timestamp columns
    are stored as their own sequence (delta, runs).
* **RatecodeID -> fare.**
  * Code 2 (JFK flat fare): fare 70.00 on 96,608 of 98,713 rows (97.87 %). Code 1: the most common fare covers
    only 5.25 % of 2,663,350 rows; code 99 3.83 %; code 5 5.92 %; code 3 6.14 %; code 4 1.79 %; null 2.78 %
    (140,162 rows); code 6 3 of 7 rows.
  * So `fare = lookup(RatecodeID)` is not a usable rule (far below the 80 % consistency gate). The search
    found the reverse instead, `RatecodeID = lookup(fare_amount)` (8,970 keys, 1.29 % exceptions,
    172,331 -> 38,557 bytes).
* **Other lookups that won.**
  * Strong: `mta_tax`, `improvement_surcharge` and `RatecodeID` from `fare_amount` (0.06-1.29 % exceptions),
    and `Airport_fee` from `PULocationID` (5.22 %).
  * Weaker: `congestion_surcharge` from `improvement_surcharge` (12.06 %), `payment_type` from
    `congestion_surcharge` (16.44 %) and `VendorID` from `extra` (16.83 %). They still won by 10-60 %, because
    their exception lists code small. They are correlations, not rules of the business.
* **Decode speed.** Full read of the taxi file: v4 default 295 ms, v4 level 19 460 ms, rule mode level 19
  1,544 ms (median of 3; paired ratios against v4 default 3.58, 5.83 and 5.23 per repetition). One derived
  column (`VendorID = lookup(extra)`): 36 ms against 453 ms, because the source column `extra` and its
  decimal-scale decode come along.
* **Nulls.** passenger_count, RatecodeID, store_and_fwd_flag, congestion_surcharge and Airport_fee are null on
  the same 140,162 rows. The round trip is exact, null positions included (`bench_rules` compares the digest of
  every column after every read; the rule-mode file is read back in full).

### Exception-rate sweep, and where rule mode falls back

The sweep has 200,000 generated rows. `line_total = round(qty * price, 2)` and `country = lookup(city)` get 0 to
50 % of their rows replaced by unrelated values. Rule mode runs at level 19 and the sizes are column payload
bytes.

| data | column | exceptions | v4 default | v4 level 19 | best rule candidate (encoded in full) | stored by rule mode | stored as |
|---|---|---:|---:|---:|---:|---:|---|
| plain | line_total | 0 % | 637,931 | 602,838 | 89 | 89 | = qty * price (f64), rounded to 2 decimals |
| plain | country | 0 % | 71,134 | 71,134 | 41 | 41 | = lookup(city) (8 keys) |
| plain | line_total | 0.1 % | 638,027 | 602,893 | 6,016 | 6,016 | = qty * price (f64), rounded to 2 decimals |
| plain | country | 0.1 % | 72,729 | 72,729 | 673 | 673 | = lookup(city) (8 keys) |
| plain | line_total | 1 % | 638,112 | 602,898 | 35,686 | 35,686 | = qty * price (f64), rounded to 2 decimals |
| plain | country | 1 % | 82,528 | 82,528 | 4,244 | 4,244 | = lookup(city) (8 keys) |
| plain | line_total | 5 % | 639,848 | 603,495 | 135,558 | 135,558 | = qty * price (f64), rounded to 2 decimals |
| plain | country | 5 % | 82,383 | 82,383 | 15,992 | 15,992 | = lookup(city) (8 keys) |
| plain | line_total | 10 % | 640,752 | 603,974 | 243,747 | 243,747 | = qty * price (f64), rounded to 2 decimals |
| plain | country | 10 % | 92,150 | 92,150 | 29,196 | 29,196 | = lookup(city) (8 keys) |
| plain | line_total | 20 % | 643,024 | 604,758 | 419,466 | 419,466 | = qty * price (f64), rounded to 2 decimals |
| plain | country | 20 % | 108,618 | 108,618 | not encoded (sample test failed) | 108,618 | v4 payload |
| plain | line_total | 25 % | 643,942 | 604,954 | 472,699 | 472,699 | = qty * price (f64), rounded to 2 decimals |
| plain | country | 25 % | 115,961 | 115,961 | not encoded (sample test failed) | 115,961 | v4 payload |
| plain | line_total | 30 % | 644,424 | 605,127 | 533,690 | 508,014 | float: k/10^2, 0 exceptions, k: affine 51711+0*i |
| plain | country | 30 % | 122,418 | 122,418 | not encoded (sample test failed) | 122,418 | v4 payload |
| plain | line_total | 35 % | 645,207 | 604,910 | 594,451 | 509,582 | float: k/10^2, 0 exceptions, k: affine 55585+0*i |
| plain | country | 35 % | 128,204 | 128,204 | not encoded (sample test failed) | 128,204 | v4 payload |
| plain | line_total | 40 % | 645,713 | 605,230 | 654,175 (larger than v4-19) | 510,070 | float: k/10^2, 0 exceptions, k: affine 59005+0*i |
| plain | country | 40 % | 133,727 | 133,727 | not encoded (sample test failed) | 133,727 | v4 payload |
| plain | line_total | 45 % | 646,122 | 604,901 | 719,910 (larger than v4-19) | 512,445 | float: k/10^2, 0 exceptions, k: affine 60076+0*i |
| plain | country | 45 % | 138,934 | 138,934 | not encoded (sample test failed) | 138,934 | v4 payload |
| plain | line_total | 50 % | 645,971 | 605,172 | 779,199 (larger than v4-19) | 512,664 | float: k/10^2, 0 exceptions, k: affine 61417+0*i |
| plain | country | 50 % | 143,842 | 143,842 | not encoded (sample test failed) | 143,842 | v4 payload |
| ties | line_total | 0 % | 570,881 | 499,120 | 89 | 89 | = qty * price (f64), rounded to 2 decimals |
| ties | country | 0 % | 71,134 | 71,134 | 41 | 41 | = lookup(city) (8 keys) |
| ties | line_total | 1 % | 574,178 | 503,318 | 36,628 | 36,628 | = qty * price (f64), rounded to 2 decimals |
| ties | country | 1 % | 82,528 | 82,528 | 4,244 | 4,244 | = lookup(city) (8 keys) |
| huge | line_total | 0 % | 820,693 | 780,566 | 89 | 89 | = qty * price (f64), rounded to 2 decimals |
| huge | country | 0 % | 71,134 | 71,134 | 41 | 41 | = lookup(city) (8 keys) |
| huge | line_total | 1 % | 826,638 | 777,524 | 41,361 | 41,361 | = qty * price (f64), rounded to 2 decimals |
| huge | country | 1 % | 82,528 | 82,528 | 4,244 | 4,244 | = lookup(city) (8 keys) |

* `line_total`:
  * The relation stays smaller than v4 level 19 up to 35 % exceptions (594,451 against 604,910 bytes).
  * At 40 % the relation would be 654,175 bytes, larger than v4 level 19 (605,230). The writer drops it and
    stores the column with the decimal-scale family instead (510,070 bytes), which is smaller than both.
  * So the crossover with v4 level 19 lies between 35 % and 40 %, and the stored column is never larger than
    version 4 at any rate.
* `country`: the lookup is cheap (29,196 bytes at 10 %, against 92,150). From 20 % on the sample test (80 %
  consistency) rejects it before the size comparison, and the version-4 payload is stored. The gate drops the
  rule earlier than size alone would.
* Float edge cases: prices on 2-decimal rounding ties (`ties`, odd multiples of 0.005) and prices near 1e13
  (100 x price close to 2^53, `huge`). The relation still holds exactly (89 bytes at 0 %). At 1 % exceptions
  the sizes are 36,628 and 41,361 bytes. Every case round-trips bit for bit.

### The orders level-19 anomaly (rule mode at level 19 larger than at the default level)

On orders at 1M rows, rule mode at level 19 writes 5,430,426 bytes, against 5,335,436 at the default level.
This is not a rule-mode bug; zstd is not monotonic in its level:

* The `customer` column (uniform random integers) as version 4 at level 9 is 205,129 bytes (100k rows), at
  level 12 205,048, at level 15 227,897, at level 17 265,081, and at level 19 and 22 251,416. In every case
  the transform is Plain and the stage zstd.
* Version 4 itself therefore stores `customer` 23 % larger at level 19. Rule mode compares every column with the
  version-4 payload at the same level, so at level 19 a slope-0 affine rule (228,511 bytes) wins there, and the
  file is still smaller than v4 level 19.

Rule mode at level 19 is never larger than v4 level 19; this holds by construction and in every row of the
tables. It can be larger than rule mode at the default level, as version 4 can.

## 6. Tests

Totals: `cargo test -p kore-store --offline --no-fail-fast` has 15 test targets, **105 passed, 0 failed**. The
same 105 also pass on a clean `git archive` export of commit 426a2ac3 (the code and docs of this branch) built in its own target directory.

`kore-store/tests/rules.rs` has 16 tests:

* **Every family on data made for it.** Each family round-trips bit for bit and is the rule chosen: the affine
  grid, timestamp text, decimal scale, lag, functional dependency, shift, float product with rounding, integer
  product and sum chains, template, recurrences, delta-of-delta and exceptions.
* **More families:** `markov_family_round_trips`, and `float_sum_and_offset_families` (a 5-term
  money sum in cents with a missing fee on 1/11 of the rows, nulls in a term and in the target, NaN, -0.0 and
  1/3 as targets; the offset relation `dropoff = pickup + lag`; corrupted payloads with recomputed CRCs).
* **Random inputs:** `random_streams_through_every_int_model` decodes random residual streams with i64
  extremes exactly. `random_property_cases` runs 40 seeded random tables (0-5000 rows, random null and noise
  rates); every one gives an exact round trip, is never larger than version 4, and has deterministic bytes.
* **Adversarial values (`adversarial_values`):**
  * rules that almost fit (one wrong value at the start, the middle and the end, including i64::MIN and MAX);
  * NaN with two payloads, -0.0, infinities, a subnormal, 1e300 and nulls;
  * derived floats over NaN and -0.0, and wrapping i64 relations with nulls;
  * template edge cases: leading zeros, 19-digit runs, empty strings, emoji, 0x00.
* **Small and empty columns (`small_and_empty_columns`):** 0, 1 and 2 rows of every type, all-null columns and
  an empty block.
* **No rule, same bytes:** `no_rule_means_the_version4_bytes` (also with a shared dictionary), and
  `no_rule_output_equals_every_v4_option_set` (default, compact directory, quick(), fast(), level 19, quick
  with the compact directory; with rules winning each of these gives a version-6 file that reads back exactly).
* **Reading:** `rules_with_shared_dictionary_and_projection`, and `partial_row_reads_equal_the_full_read` (6
  row ranges x 3 column sets, plus out-of-range ranges refused).
* **Old readers:** `old_readers_refuse_rule_mode_files` (`v4::read` gives "unsupported version 6"; `v3::read`;
  `v4::info`; the committed fixture), and `fixture_bytes_are_stable`.
* **Corruption:**
  * `corruption_and_truncation_never_panic_or_return_wrong_data`: every byte flipped with 3 masks, every
    truncation and one trailing byte are all refused.
  * `payload_garbage_with_valid_crcs_is_refused_or_exact`: every payload byte flipped with 3 masks, with all CRCs
    recomputed so the change reaches the rule decoders. Every result is an error or the exact column, and a
    changed digest is reported as `checksum mismatch`.
* **Options:** `explicit_codecs_are_respected_and_options_checked`.

Version-6 checklist (all passed):

| item | result |
|---|---|
| full kore-store test suite on a clean `git archive` export | 105 passed, 0 failed (15 targets) |
| bit-for-bit round trip of every dataset through the v6 writer and reader | all datasets of this document, 100k and 1M rows, TPC-DS and the taxi file: every column's digest equal after every full read and every one-column read (`bench_rules` asserts it), at both levels |
| v2, v3, v4 fixtures and v5 files open with the new reader | the earlier test targets (compact, conformance, speed_compat, v4_rows, v4_writer) pass unchanged; `orders_v4`/`orders_v5` of `write_v6_sample` read with the same hash as the v6 file |
| the 118f80df and b44d2a91 readers refuse v6 cleanly | real builds: `unsupported version 6` for every v6 file (including corrupted and truncated ones), no panic |
| corrupted or truncated v6 files give a checksum or format error | tests above; the flipped sample gives `checksum mismatch in column 'qty'`, the truncated one `truncated .kore data` |

## 7. Limits

* **The entropy limit.** No lossless method stores data below its entropy. For the random control the limit is
  8 + 8 + 53/8 + 16 = 38.625 bytes per row, which is 9.656 B/value. Rule mode stores 9.711 B/value at 1M rows.
  Rules only remove redundancy that is there: hash ids, random numbers and free text gain nothing.
* **Only structure that the DSL can express:**
  * no inequalities (dropoff >= pickup);
  * relations only between columns of one block (TPC-H/TPC-DS cross-table relations such as extended price =
    quantity x part retail price need the part table, so they are out of reach);
  * functional dependencies from one source column only;
  * sums of at most 8 float terms;
  * integer recurrences with coefficients up to 2^20 and order up to 3;
  * one predictor per column;
  * templates only on canonical integers;
  * Markov ranks only up to 1024 values.
* **Wrong wins.** Some wins are not rules: the delta choice on the taxi timestamps, slope-0 affine offsets and
  Markov ranks on iid categoricals. Those are better choices of transform or entropy coding. Some lookups are
  correlations with 12-17 % exceptions.
* **Encode cost.** Encode is 2-12x slower than version 4 at the default level, and far more at level 19 on
  large files (taxi: 567 s against 60 s). The relation search grows with columns^2, and only the first 64
  columns are searched.
* **Decode cost.** Full reads are up to about 5x slower than version 4. A single derived column reads its
  sources too (up to about 40x slower for small tables, where the version-4 read takes about a millisecond).
  Partial row reads on version 6 are full decodes.
* **Per block.** Rule mode works per block; rules are not shared between files.

## 8. Not measured

* TPC-H (the `tpch` extension is not installed and was not downloaded).
* Real data other than the single taxi file. Other months, other taxi types and other real tables are
  unmeasured. The taxi result is one file, not a benchmark of real data in general.
* Row counts above 2.96M; tables wider than 64 columns.
* Speed on an idle machine; memory use of the writer and the reader.
* ORC and bz2 for these datasets. Parquet only with pyarrow's encodings (no explicit delta or
  byte-stream-split settings).
* Rule-mode files inside the versioned log (`versioned::*`); readers in other languages (none of them read
  version 6).
* Fuzzing beyond the byte-flip, truncation and recomputed-CRC sweeps in the tests.

