# KORE rule mode: lossless compression by discovered rules (opt-in, container version 5)

Branch `kore-rule-mode` (from `fileformat` @ b44d2a91). Additive and opt-in: the default writer, the version-3
and version-4 writers and every existing reader path produce and accept exactly what they did at b44d2a91
(the 19 pinned version-4 hashes in `tests/v4_writer_bytes.txt` still match). Nothing was pushed.

## 1. Idea and guarantee

When a column, or a relation between columns, follows a rule, the file stores the rule plus its exceptions or
residuals instead of the values. The reader regenerates the values. It is lossless:

* every rule is evaluated with exact integer arithmetic (wrapping `i64`) or exact IEEE-754 operations that
  Rust performs identically on every platform (`+ - * /`, `round`, `as f64`; no fused multiply-add, no
  library math), and whatever the rule does not reproduce is stored as a residual or an exception;
* the writer decodes every rule payload it produces and keeps it only if the result equals the column bit
  for bit (floats compared by bits, nulls by position);
* the payload carries a CRC-64/XZ of the decoded column (`rules::digest`: type, row count, per row a null
  flag and the value bits). The reader recomputes it after decoding; a mismatch is the error
  `rule-mode checksum mismatch: decoded values differ from the written column`, never silent;
* the version-4 checksums stay: header and directory under `dir_crc32`, every payload under its CRC-32.

The writer never makes a file larger: every column is encoded by the version-4 writer and by every
applicable rule, and the smaller payload is kept (ties keep version 4). The header has the same size as in
version 4. If no column uses a rule, the output **is the version-4 file, byte for byte** (readable by
b44d2a91 readers); the cost in that case is only encode time.

API: `CompactOptions::with_rules() -> RuleOptions` (also `RuleOptions::default()`, `.within_only()`,
`.cross_only()`), `KoreWriter::to_bytes_rules(&block, &opts)`, `rules::write_explained` (bytes plus, per
column, the version-4 size and the rule that won), `rules::describe(&bytes)`. Reading is the usual
`KoreReader::from_bytes`, `from_bytes_columns`, `from_bytes_with_dicts`, `from_bytes_columns_with_dicts`.
A column with an explicit codec (`CompactOptions::with_column`) keeps it; shared dictionaries work as in
version 4 for the columns that keep a version-4 payload.

## 2. The rule DSL

Rules are searched in a small, fixed grammar (the idea of a small verifiable DSL with an exact checker comes
from Track B2 of the Digital Life being, `docs/measured.md` on `main-Digital-Life-AI`; none of its code is used
and kore-store does not depend on kore-self). The search is deterministic: same block and options, same bytes.

### Inside one column (families 1-6)

| family | applies to | rule | stored |
|---|---|---|---|
| 1 integer sequence | `i64` | one predictor over the non-null values `v[0..m)` (below) | predictor + residuals `v[i] - pred(i)` |
| 2 float decimal | `f64` | `x = k / 10^s` bit-exact, s = 0..15 (smallest s that fits 7/8 of a sample) | s, exceptions (rows + raw bits: NaN, -0.0, inf, values with more digits), `k` through family 1 |
| 3 float predicted | `f64` | prediction = previous value, or `a + i*b` | xor of the bits with the prediction, as 8 byte planes |
| 4 timestamp text | canonical `YYYY-MM-DDTHH:MM:SSZ` strings | epoch seconds through family 1 | as family 1 |
| 5 text template | strings | each maximal digit run that is a canonical integer of at most 18 digits becomes a slot | skeleton column (version-4 coded: dictionary + stage), per template slot a mode (plain or delta to the previous value of the same slot), the numbers |
| 6 categorical Markov | strings with at most 1024 distinct values | adaptive next-token ranking: per previous value, all values ordered by how often they followed it | dictionary + one rank per value |

Integer predictors (family 1, also used inside 2 and 4):

| predictor | prediction for index i | covers |
|---|---|---|
| `Rec` k = 0..3 | `c0 + sum c_j * v[i-j]`, `v[<0] = 0` | plain (k = 0), constant, delta (arithmetic progression), delta-of-delta (piecewise linear), third difference (polynomials up to degree 2 leave a constant residual), and fitted linear recurrences of order up to 3 with integer coefficients (geometric with integer ratio, affine-geometric `3*v+1`, Fibonacci-like) |
| `Affine a b` | `a + b*i` | fixed grids (timestamps with jitter: the residual is the jitter alone, delta would store the difference of two jitters); with b = 0 it is an offset (frame of reference) |
| `Lag p` | `v[i-p]` | periodic / seasonal (p = 2..2048 searched) |
| `LagTrend p` | `v[i-p] + v[i-1] - v[i-p-1]` | seasonal with trend |
| `Runs` | none | constant runs: run values (delta) + run lengths |

Recurrences are found as in B2: for each form (non-empty subset of lags {1,2,3}, with or without constant;
14 forms, smallest first) the coefficients are solved exactly (rational Gauss-Jordan) on q consecutive rows,
3 further rows must agree exactly, and only integer coefficients up to 2^20 are kept. Every candidate then
competes on size: candidates are scored on three sample windows of 16K values (zstd level 1 on the residual
varints), the two best are encoded in full, the smaller wins.

### Across columns (families 7-9)

| family | rule | stored |
|---|---|---|
| 7 integer relation | `t = a + b`, `a - b`, `a * b` (wrapping) or `alpha * a + beta` | residuals `t - pred` (0 where a source is null) |
| 8 float relation | `t = a * b`, `a + b`, `a - b`, `a / b` in f64 (integer sources converted with `as f64`), optionally rounded half away from zero to s decimals | xor of the bits with the prediction (8 byte planes) |
| 9 lookup | shift: `t[i] = a[i-k]`, k = 0..8; functional dependency: `t = map[a]` (one target value per distinct source value, the most frequent one) | the map (version-4 coded column), exception rows + exception values (version-4 coded column) |

Search: on a sample of 4096 rows (the first 2048 and 2048 spread over the rest) a relation must predict at least
half of the target rows exactly (lookups: at most 1/8 distinct keys in the sample and 80 % consistency); then it
is encoded in full. Candidates are accepted greedily by bytes saved; every column is derived at most once and
the derivation graph stays acyclic (a derived column may be the source of another: `total_cents =
subtotal_cents + shipping_cents`, `subtotal_cents = qty * unit_cents`, `unit_cents = lookup(unit_price)`).
Only the first 64 columns of a block are searched for relations.

Residuals and exceptions always go through the existing best stages: every residual or exception stream is
coded with the smallest of no stage, lz4, zstd (level as in version 4: `small_zstd_level` below 256 KiB,
`zstd_level` above) and hz (below 128 KiB); embedded value columns (skeletons, dictionaries, maps, exception
values) are coded by the version-4 column encoder itself (`Auto` transform + stage).

## 3. Format layout

```text
"KORE" | version:u16 = 5 | num_cols:u32 | num_rows:u64
num_dicts:u16, per dictionary: dict_id:[32] | content_len:u32                          (as version 4)
per column: name_len:u16 | name | dtype:u8 | transform:u8 | stage:u8 | dict:u8 | payload_len:u64 | payload_crc32:u32
dir_crc32:u32
payloads
```

The version-4 layout with version 5. A column with `transform` = `0x10` is a rule column (`stage` 0, `dict`
0xFF); every other column is a version-4 payload decoded by the version-4 code. A rule payload is

```text
family:u8 | digest:u64 (CRC-64/XZ of the decoded column) | n_deps:u8 (0, 1 or 2) | deps: varint column indices | body

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
```

Reader checks: the version-4 header and payload checks; rule family, model and op ids; every count against
the row count (or the bytes present) before any allocation; stream sizes declared before decompression; a
rule column may not refer to itself and the dependencies may not form a cycle; source columns must have the
row count; the decoded length, type and digest. Everything runs under `catch_unwind` like the version-4
reader, so a decoder bug on corrupt input is an error, not an abort. A projection decodes the requested
columns and, transitively, the sources of the derived ones (sources are not returned).

**Compatibility.** Readers that predate version 5 stop at the version field: the b44d2a91 `KoreReader`
returns `unsupported version 5`, as does `v4::read` (both checked, section 6). Version 1-4 files read as
before. The default writer, `to_bytes_with` (v3) and `to_bytes_compact` (v4) are unchanged; the 19 pinned
version-4 hashes match. Shared files touched: `lib.rs` (module + re-export), `reader.rs` (2 dispatch lines in
2 functions), `writer.rs` (`to_bytes_rules`), `v4.rs` (two `fn` made `pub(crate)`). Everything else is new:
`src/rules/{mod,stream,intseq,within,cross}.rs`, `tests/rules.rs`, `tests/fixtures/rules_v5.kore`,
`examples/bench_rules.rs`, `examples/support/rules_gen.rs`, `scripts/kore_rules/{py_side,tables}.py`.

## 4. Measurements

One run per dataset and size on one machine (Windows 11, 8 logical CPUs, rustc release build), 2026-11-07.
The machine was shared with other builds the whole time: `typeperf` total CPU was 99.9-100 % at the start
and at the end of the final run. Sizes are deterministic and do not depend on load. Times are not reliable in
absolute terms; use the paired ratios (version 4 and rule mode encoded/decoded back to back in the same
process, order alternating per repetition, 3 repetitions). As a noise reference: on the control without any
rule, both decoders read byte-identical files and the paired decode ratio still came out at 1.31 (1M rows)
and 1.50 (100k rows).

Commands: `cargo build --release -p kore-store --example bench_rules`, then
`bench_rules --out DIR --sizes 100000,1000000 --sets sensors,orders,logs,sequences,random --reps 3`,
`bench_rules --out DIR --sizes 100000 --sets events --events rdata_100000.jsonl --reps 3`,
`python -I scripts/kore_rules/py_side.py DIR/dump_<set>_<n> DIR/py_<set>_<n>.json` per dump (read-only conda env
`engines`: Python 3.12, pyarrow 25.0.0) and `python -I scripts/kore_rules/tables.py DIR`. Raw results (not
committed): `C:\Users\skathera\AppData\Local\Temp\claude\7f7596edcdf0d251\75ecfc96-88bb-4174-9290-83105f566c76\scratchpad\rules\run3`
(store side, final binary; the Python results copied in) and `...\scratchpad\rules\run1` (dumps and the Python
side, the same generated data).

Datasets (all deterministic):

* **events** (a): the realistic dataset of docs/kore_size.md, `format_bench_gen_realistic.py` seed 20261101,
  N = 100,000 lines (sha256 of the JSONL 2118d09a.., the same file as in kore_size.md). Its 98,040 event lines
  are turned into the 8 columns of the Digital Life events table (event_id = sha256 of
  source|occurred_at|normalised text as in the log, recorded_at = the benchmark's fixed clock, redacted =
  false); the 1,960 revision lines are not used. One block. kore_size.md measured 1000-row log segments plus the
  revision tables, so its B/event are not comparable with this table.
* **sensors**, **orders**, **logs**, **sequences** (b): `examples/support/rules_gen.rs`, seed 20261107.
  sensors: 1 s grid timestamps in ms with 0-19 ms jitter, the same as text on a 60 s grid with 0-2 s jitter, a
  temperature with drift + daily cycle + noise rounded to 2 decimals, a full-precision pressure with drift and
  noise, a counter, a battery level in long runs, a sorted device id, a period-96 pattern with noise. orders:
  sequential id, order time text, customer, city -> country (0.1 % exceptions), the previous row's city,
  product -> catalog price, qty, line total = round(qty * price, 2), and cents columns with exact relations
  (subtotal = qty * unit cents, total = subtotal + shipping). logs: time text, service -> host, a level from a
  4-state Markov chain, status from the level, a message from 6 templates with numeric fields, latency,
  request id. sequences: Fibonacci (wrapping), 3*v+1 (wrapping), a quadratic, an arithmetic progression, the
  same with 1 % of the rows replaced.
* **random** (c): uniform 64-bit integers, doubles with uniform random bits (any bit pattern, NaNs included),
  uniform doubles in [0, 1) (53 random bits each), 32-character random hex. `random_norule` is the same without
  the [0, 1) doubles (no rule wins on any of its columns).

Other formats are written from the same values (exact bits, via Arrow buffers): Parquet = pyarrow, zstd level
22, dictionary encoding on, one row group per 1M rows; CSV + xz-9 = pyarrow CSV writer, then lzma preset 9;
raw + xz-9 = the Arrow buffers (validity, values, offsets) concatenated, then lzma preset 9.

### Size (bytes per value = file bytes / (rows x columns); ratio = v4 default bytes / format bytes)

| dataset | rows x cols | v4 default | v4 level 19 | **rule mode** | rule mode, level 19 | Parquet zstd-22 | CSV + xz-9 | raw buffers + xz-9 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| events | 98,040 x 8 | 9.185 B (1.00x) | 8.537 B (1.08x) | **9.169 B (1.00x)** | 8.514 B (1.08x) | 9.862 B (0.93x) | 9.705 B (0.95x) | 9.272 B (0.99x) |
| sensors | 100,000 x 8 | 1.266 B (1.00x) | 1.099 B (1.15x) | **0.935 B (1.35x)** | 0.923 B (1.37x) | 2.285 B (0.55x) | 1.753 B (0.72x) | 1.284 B (0.99x) |
| sensors | 1,000,000 x 8 | 1.260 B (1.00x) | 1.046 B (1.20x) | **0.928 B (1.36x)** | 0.909 B (1.39x) | 1.598 B (0.79x) | 1.745 B (0.72x) | 1.236 B (1.02x) |
| orders | 100,000 x 14 | 1.320 B (1.00x) | 1.188 B (1.11x) | **0.374 B (3.53x)** | 0.391 B (3.37x) | 1.370 B (0.96x) | 0.861 B (1.53x) | 1.085 B (1.22x) |
| orders | 1,000,000 x 14 | 1.315 B (1.00x) | 1.138 B (1.16x) | **0.381 B (3.45x)** | 0.388 B (3.39x) | 1.090 B (1.21x) | 0.803 B (1.64x) | 1.048 B (1.25x) |
| logs | 100,000 x 8 | 1.040 B (1.00x) | 0.958 B (1.09x) | **0.663 B (1.57x)** | 0.663 B (1.57x) | 1.532 B (0.68x) | 1.302 B (0.80x) | 1.416 B (0.73x) |
| logs | 1,000,000 x 8 | 1.050 B (1.00x) | 0.925 B (1.14x) | **0.668 B (1.57x)** | 0.654 B (1.61x) | 1.169 B (0.90x) | 1.271 B (0.83x) | 1.394 B (0.75x) |
| sequences | 100,000 x 5 | 3.724 B (1.00x) | 3.724 B (1.00x) | **0.007 B (543.01x)** | 0.007 B (543.01x) | 6.278 B (0.59x) | 5.232 B (0.71x) | 3.892 B (0.96x) |
| sequences | 1,000,000 x 5 | 3.713 B (1.00x) | 3.346 B (1.11x) | **0.006 B (615.71x)** | 0.006 B (615.71x) | 4.604 B (0.81x) | 5.325 B (0.70x) | 3.630 B (1.02x) |
| random | 100,000 x 4 | 9.878 B (1.00x) | 9.879 B (1.00x) | **9.725 B (1.02x)** | 9.717 B (1.02x) | 11.590 B (0.85x) | 11.986 B (0.82x) | 10.140 B (0.97x) |
| random | 1,000,000 x 4 | 9.877 B (1.00x) | 9.878 B (1.00x) | **9.712 B (1.02x)** | 9.711 B (1.02x) | 10.211 B (0.97x) | 11.881 B (0.83x) | 10.119 B (0.98x) |
| random_norule | 100,000 x 3 | 10.667 B (1.00x) | 10.667 B (1.00x) | **10.667 B (1.00x)** | 10.667 B (1.00x) | not measured | not measured | not measured |
| random_norule | 1,000,000 x 3 | 10.667 B (1.00x) | 10.667 B (1.00x) | **10.667 B (1.00x)** | 10.667 B (1.00x) | not measured | not measured | not measured |

### Bytes

| dataset | rows | v4 default | v4 level 19 | rule mode | rule mode, level 19 | rule mode / v4 default | Parquet zstd-22 | CSV + xz-9 | raw + xz-9 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| events | 98,040 | 7,204,298 | 6,696,077 | 7,191,467 | 6,677,987 | 0.9982 | 7,735,068 | 7,611,964 | 7,272,148 |
| sensors | 100,000 | 1,012,868 | 879,413 | 747,898 | 738,004 | 0.7384 | 1,828,385 | 1,402,780 | 1,026,936 |
| sensors | 1,000,000 | 10,077,059 | 8,369,739 | 7,421,316 | 7,275,087 | 0.7365 | 12,787,541 | 13,962,104 | 9,888,260 |
| orders | 100,000 | 1,847,660 | 1,663,420 | 524,157 | 547,539 | 0.2837 | 1,918,535 | 1,205,736 | 1,518,520 |
| orders | 1,000,000 | 18,415,760 | 15,932,314 | 5,335,569 | 5,430,558 | 0.2897 | 15,258,676 | 11,243,680 | 14,675,080 |
| logs | 100,000 | 831,693 | 766,189 | 530,137 | 530,607 | 0.6374 | 1,225,440 | 1,041,704 | 1,133,088 |
| logs | 1,000,000 | 8,401,683 | 7,399,696 | 5,342,744 | 5,228,889 | 0.6359 | 9,352,987 | 10,165,100 | 11,150,976 |
| sequences | 100,000 | 1,861,997 | 1,861,997 | 3,429 | 3,429 | 0.0018 | 3,139,121 | 2,615,916 | 1,946,200 |
| sequences | 1,000,000 | 18,566,016 | 16,729,265 | 30,154 | 30,154 | 0.0016 | 23,020,822 | 26,623,036 | 18,147,520 |
| random | 100,000 | 3,951,075 | 3,951,528 | 3,890,104 | 3,886,642 | 0.9846 | 4,635,823 | 4,794,276 | 4,055,884 |
| random | 1,000,000 | 39,509,650 | 39,511,865 | 38,849,435 | 38,845,730 | 0.9833 | 40,842,983 | 47,524,636 | 40,476,628 |
| random_norule | 100,000 | 3,200,097 | 3,200,097 | 3,200,097 | 3,200,097 | 1.0000 | not measured | not measured | not measured |
| random_norule | 1,000,000 | 32,000,097 | 32,000,097 | 32,000,097 | 32,000,097 | 1.0000 | not measured | not measured | not measured |

### Speed (median over the repetitions; ratios are paired, same process, alternating order)

| dataset | rows | encode v4 | encode rule mode | encode ratio rule/v4 (per rep) | decode v4 | decode rule mode | decode ratio rule/v4 (per rep) | rule-mode encode MB/s (raw) | rule-mode decode MB/s (raw) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| events | 98,040 | 3868 ms | 4324 ms | 1.12 (1.02, 1.12, 1.32) | 321.5 ms | 265.2 ms | 0.82 (0.82, 0.93, 0.71) | 13 | 204 |
| sensors | 100,000 | 212 ms | 742 ms | 3.43 (2.73, 3.71, 3.43) | 32.0 ms | 37.2 ms | 1.24 (1.24, 1.12, 1.37) | 11 | 226 |
| sensors | 1,000,000 | 1249 ms | 3934 ms | 3.15 (3.63, 3.15, 2.98) | 184.4 ms | 294.3 ms | 1.68 (1.82, 1.60, 1.68) | 21 | 285 |
| orders | 100,000 | 351 ms | 1193 ms | 3.61 (3.79, 3.39, 3.61) | 50.3 ms | 64.6 ms | 1.27 (1.22, 1.42, 1.27) | 12 | 224 |
| orders | 1,000,000 | 1529 ms | 6147 ms | 4.10 (4.10, 3.45, 4.24) | 230.7 ms | 699.5 ms | 2.95 (2.95, 2.60, 3.09) | 24 | 207 |
| logs | 100,000 | 281 ms | 2054 ms | 6.72 (6.72, 5.17, 7.63) | 22.3 ms | 86.8 ms | 3.90 (4.70, 3.90, 3.57) | 6 | 149 |
| logs | 1,000,000 | 2354 ms | 7529 ms | 3.28 (3.33, 3.28, 2.64) | 580.8 ms | 1616.1 ms | 2.70 (3.58, 2.67, 2.70) | 17 | 80 |
| sequences | 100,000 | 111 ms | 307 ms | 2.75 (2.75, 3.13, 2.59) | 41.4 ms | 9.6 ms | 0.28 (0.30, 0.28, 0.23) | 13 | 416 |
| sequences | 1,000,000 | 369 ms | 858 ms | 2.32 (2.87, 2.32, 1.86) | 79.6 ms | 349.5 ms | 4.13 (4.13, 1.98, 4.39) | 47 | 114 |
| random | 100,000 | 137 ms | 380 ms | 2.60 (3.57, 2.60, 1.94) | 57.1 ms | 39.5 ms | 0.47 (0.47, 0.40, 3.09) | 17 | 162 |
| random | 1,000,000 | 1647 ms | 4788 ms | 2.89 (2.84, 2.89, 2.96) | 383.1 ms | 355.7 ms | 0.92 (0.92, 0.91, 1.39) | 13 | 180 |
| random_norule | 100,000 | 113 ms | 272 ms | 2.00 (1.30, 2.82, 2.00) | 25.2 ms | 33.8 ms | 1.50 (1.11, 1.50, 2.30) | 21 | 166 |
| random_norule | 1,000,000 | 978 ms | 2586 ms | 2.62 (2.21, 2.62, 2.80) | 240.6 ms | 286.1 ms | 1.31 (1.44, 0.78, 1.31) | 22 | 196 |

### Per column, largest size of each dataset (v4 default payload vs the rule that won)


**events, 98,040 rows**

| column | v4 payload bytes | rule-mode payload bytes | rule |
|---|---:|---:|---|
| event_id | 3,137,283 | - | none (v4 payload kept) |
| occurred_at | 69,507 | 60,556 | timestamp text: affine 1735689614+30*i |
| recorded_at | 30 | 25 | timestamp text: runs |
| source | 38,490 | 36,993 | categorical Markov ranks |
| kind | 31,968 | 29,590 | categorical Markov ranks |
| text | 3,742,564 | - | none (v4 payload kept) |
| tags | 184,212 | - | none (v4 payload kept) |
| redacted | 20 | - | none (v4 payload kept) |

**sensors, 1,000,000 rows**

| column | v4 payload bytes | rule-mode payload bytes | rule |
|---|---:|---:|---|
| ts_ms | 755,511 | 561,817 | int: affine 1767225600009+1000*i |
| ts_text | 266,979 | 240,452 | timestamp text: affine 1767225601+60*i |
| temp_c | 1,577,977 | 624,282 | float: k/10^2, 0 exceptions, k: delta |
| pressure | 6,723,420 | 5,372,424 | float: xor residual vs affine a+i*b |
| counter | 293,072 | - | none (v4 payload kept) |
| battery | 493 | 234 | int: runs |
| device | 126 | 46 | int: runs |
| cycle | 459,262 | 328,770 | int: lag 192 |

**orders, 1,000,000 rows**

| column | v4 payload bytes | rule-mode payload bytes | rule |
|---|---:|---:|---|
| order_id | 56 | - | none (v4 payload kept) |
| ordered_at | 674,838 | - | none (v4 payload kept) |
| customer | 2,051,146 | - | none (v4 payload kept) |
| city | 532,259 | 531,936 | categorical Markov ranks |
| country | 369,200 | 1,781 | = lookup(city) (23 keys) |
| prev_city | 532,291 | 14 | = city shifted by 1 |
| product | 1,342,877 | 1,331,452 | int: affine 256+0*i |
| qty | 444,894 | 443,948 | int: affine 5+0*i |
| unit_price | 2,222,470 | 1,714 | = lookup(product) (500 keys) |
| line_total | 2,933,267 | 288 | = qty * unit_price (f64), rounded to 2 decimals |
| unit_cents | 2,065,224 | 1,054 | = lookup(unit_price) (493 keys) |
| subtotal_cents | 2,420,949 | 68 | = qty * unit_cents |
| shipping_cents | 298,819 | 296,805 | int: affine 499+0*i |
| total_cents | 2,527,069 | 68 | = subtotal_cents + shipping_cents |

**logs, 1,000,000 rows**

| column | v4 payload bytes | rule-mode payload bytes | rule |
|---|---:|---:|---|
| ts | 137,933 | - | none (v4 payload kept) |
| service | 334,686 | 47 | = lookup(host) (5 keys) |
| host | 334,706 | 334,458 | categorical Markov ranks |
| level | 209,938 | 209,113 | categorical Markov ranks |
| message | 5,828,717 | 3,222,645 | text template + numeric slots |
| latency_ms | 1,194,138 | 1,162,100 | int: affine 76+0*i |
| status | 120,512 | 35,395 | = lookup(level) (4 keys) |
| request_id | 240,834 | - | none (v4 payload kept) |

**sequences, 1,000,000 rows**

| column | v4 payload bytes | rule-mode payload bytes | rule |
|---|---:|---:|---|
| fibonacci | 8,000,001 | 71 | int: recurrence v=+1*v[-1]+1*v[-2] |
| affine_geometric | 8,000,001 | 70 | int: recurrence v=+3*v[-1]+1 |
| quadratic | 2,524,648 | 72 | int: delta-of-delta |
| arithmetic | 55 | - | none (v4 payload kept) |
| arithmetic_1pct_exceptions | 41,127 | 29,702 | = arithmetic shifted by 0 |

**random, 1,000,000 rows**

| column | v4 payload bytes | rule-mode payload bytes | rule |
|---|---:|---:|---|
| u64 | 8,000,001 | - | none (v4 payload kept) |
| f64_bits | 8,000,001 | - | none (v4 payload kept) |
| f64_unit | 7,509,527 | 6,849,312 | float: xor residual vs previous value |
| hex | 16,000,003 | - | none (v4 payload kept) |

**random_norule, 1,000,000 rows**

| column | v4 payload bytes | rule-mode payload bytes | rule |
|---|---:|---:|---|
| u64 | 8,000,001 | - | none (v4 payload kept) |
| f64_bits | 8,000,001 | - | none (v4 payload kept) |
| hex | 16,000,003 | - | none (v4 payload kept) |

### Worst case: no rule found

`random_norule` (no column has a rule): the rule-mode file is the version-4 file byte for byte (also checked
by the test `no_rule_means_the_version4_bytes`), so the size overhead is 0 bytes. The cost is encode time:
paired encode ratio rule mode / version 4 = 2.00 at 100,000 rows (3,200,097 bytes both) and 2.62 at
1,000,000 rows (32,000,097 bytes both). Decode reads the same bytes with the same code.

When a rule wins, the per-column overhead over the rule body is 10 bytes (family, 8-byte digest, n_deps) plus
the source indices; the file header is the version-4 header.

## 5. What won, and where

In one paragraph: on the generated machine data rule mode is 1.36x (sensors), 1.57x (logs) and 3.45x (orders)
smaller than version 4 at its default setting at 1M rows, and smaller than Parquet zstd-22, CSV + xz-9 and the
raw buffers + xz-9 on every one of them. On the realistic events table it is 0.18 % smaller (9.169 against
9.185 B/value; at zstd level 19 8.514, against Parquet zstd-22 9.862 and CSV + xz-9 9.705). On the random
control it is 1.7 % smaller, all from one column whose values have fewer random bits than their width, and
0.0 % on the columns that are random over their full width. Encode costs 2-7x the version-4 time (paired).
Rule mode at zstd level 19 was larger than rule mode at the default levels on orders (5,430,558 against
5,335,569 bytes at 1M rows); the per-column choice depends on the stage sizes, which differ by level. Not
investigated further.

* **orders, 3.45x smaller than version 4 at 1M rows (0.381 B/value against 1.315).** Almost all of it is
  cross-column: `country = lookup(city)` (369,200 -> 1,781 bytes, the 0.1 % exceptions included),
  `prev_city = city shifted by 1` (532,291 -> 14), `unit_price = lookup(product)`, `unit_cents =
  lookup(unit_price)`, `line_total = qty * unit_price` rounded to 2 decimals (2,933,267 -> 288), `subtotal_cents = qty *
  unit_cents`, `total_cents = subtotal_cents + shipping_cents`. What is left is what the table really
  holds: customer, product, qty, city, shipping and the order times. Smaller than Parquet zstd-22 (1.090
  B/value) and CSV + xz-9 (0.803).
* **logs, 1.57x (0.668 B/value against 1.050).** The text template (5,828,717 -> 3,222,645 bytes: skeletons as
  a dictionary, numbers as varints, delta where it pays) and `service = lookup(host)`, `status = lookup(level)`
  with exceptions. The Markov family won on `host` and `level` by less than 1 %.
* **sensors, 1.36x (0.928 B/value against 1.260).** Decimal scale on the rounded temperature (1,577,977 ->
  624,282), affine grid on both timestamp columns (the residual is the jitter alone: -26 % and -10 %), lag
  (period 192 = 2 x 96) on the pattern, runs on battery and device. The full-precision pressure is 72 % of the
  rule-mode file: its xor residual saves 20 %, the noise bits remain.
* **sequences, 616x at 1M rows.** Exact recurrences (Fibonacci, 3*v+1) and the quadratic (delta-of-delta)
  become about 70 bytes each; the arithmetic progression is already 55 bytes in version 4 (delta) and stays
  there; with 1 % exceptions the column is `arithmetic shifted by 0` plus the exceptions (41,127 -> 29,702).
  This dataset is built for the rules: it shows that the families work, not a typical gain.
* **events (realistic), 0.18 % smaller (9.169 against 9.185 B/value).** The random sha256 ids and the text
  are 95 % of the bytes and no rule applies to them; the gain comes from the occurred_at grid (30 s, -13 %)
  and adaptive-ranking gains on the iid categorical columns `source` and `kind` (-4 % and -7 %; this is
  better entropy coding of iid symbols, not a discovered structure).
* **random, 1.7 % smaller, all of it from one column.** On `u64`, `f64_bits` and `hex` no rule wins (version-4
  payload kept). The uniform [0, 1) doubles (f64_unit) shrink 7,509,527 -> 6,849,312 bytes with the xor
  residual: such doubles have only 53 random bits but a skewed exponent byte, and the byte-plane layout codes
  that better than zstd on raw bits. This is an entropy-coding effect, not a rule.
* Slope-0 `affine` (an offset) won on some uniform integer columns by 0.2-2.7 % at 1M rows (product, qty,
  shipping_cents, latency_ms): centring the values shortens some varints. Not a structure either.

**The entropy limit.** No lossless method can store data below its entropy. For the random control the limit
is 8 + 8 + 53/8 + 16 = 38.625 bytes per row = 9.656 B/value (u64 8 B, random-bit doubles 8 B, [0, 1) doubles
6.625 B, 128 random bits of hex 16 B). Rule mode stores 9.712 B/value at 1M rows (0.6 % above the limit),
version 4 9.877. Rules can only remove redundancy that is there: a structured column becomes small because its
entropy given the other columns (or given its own past) is small. On random data, free text and hash ids the
possible gain is about zero, and the measurements show that.

**Families that did not win anywhere here:** `LagTrend`, the third difference and recurrences with lag 3,
integer `alpha * a + beta`, float `+ - /` relations, shifts other than 0 and 1. They are in the search and
covered by the tests; these datasets did not need them.

## 6. Tests

`kore-store/tests/rules.rs`, 13 tests. All of `cargo test -p kore-store --offline --no-fail-fast`: 89 passed,
0 failed (14 test targets).

* `every_family_round_trips_and_wins_where_built_for`: each family on data made for it (affine grid,
  timestamp text, decimal scale, lag, FD, shift, float product with rounding, integer product and sum chains,
  template, recurrences, delta-of-delta, exceptions) round-trips bit for bit and is the rule chosen;
* `markov_family_round_trips`; `random_streams_through_every_int_model`: every integer model decodes random
  residual streams with i64 extremes exactly (not only what the search picks);
* `random_property_cases`: 40 seeded random cases (0 to 5000 rows, random null rates, noise rates, slopes,
  periods, random-bit floats, unicode text, timestamps, products): exact round trip, never larger than
  version 4, deterministic bytes;
* `adversarial_values`: rules that almost fit (one wrong value at the start, middle and end, i64::MIN and
  MAX), NaN with two payloads, -0.0, infinities, a subnormal, 1e300, nulls, derived floats over NaN and -0.0,
  wrapping i64 relations with nulls, templates with leading zeros, 19-digit runs, empty strings, emoji, 0x00;
* `small_and_empty_columns`: 0, 1 and 2 rows of every type, all-null columns, an empty block;
* `no_rule_means_the_version4_bytes` (also with a shared dictionary); `rules_with_shared_dictionary_and_projection`
  (a projection of derived columns decodes their sources internally and returns only what was asked);
* `old_readers_refuse_rule_mode_files` (`v4::read`, `v3::read`, `v4::info`, also on the committed fixture
  `tests/fixtures/rules_v5.kore`, 237 bytes) and `fixture_bytes_are_stable`;
* `corruption_and_truncation_never_panic_or_return_wrong_data`: every byte flipped with 3 masks, every
  truncation, one trailing byte: all refused; `payload_garbage_with_valid_crcs_is_refused_or_exact`: every
  payload byte flipped with 3 masks and all CRCs recomputed, so the change reaches the rule decoders: every
  result is an error or the exact column, and a changed digest is reported as `checksum mismatch`;
* `explicit_codecs_are_respected_and_options_checked` (within-only, cross-only, unknown column option).

The 19 pinned version-4 hashes (`tests/v4_writer.rs`) and all earlier tests pass unchanged.

Old reader, real binary: a detached worktree at b44d2a91 built into its own target directory (removed
afterwards), its own `examples/read_hash.rs` run on a copy of `rules_v5.kore` and of the v2 fixture
`events_1000.kore`:

```
events_1000.kore rows=1000 hash=36efa5f5e7c183e0
rules_v5.kore ERROR invalid argument: unsupported version 5
```

## 7. Limits

* The entropy limit (section 5): rules only help data that has structure. Hash ids, random numbers and free
  text gain nothing; the realistic events table gained 0.18 %.
* Encode is 2-7x slower than version 4 (paired ratios above): every column is encoded twice, and the
  cross-column search tests O(columns^2) relations on samples. Only the first 64 columns are searched for
  relations.
* Decode is slower where rules win (paired ratios up to about 4 on these datasets, under the load noise
  described above): residual streams are decoded value by value, derived columns wait for their sources, and
  every rule column is hashed for the digest.
* Floats: decimal scale up to 15 digits; affine float prediction only from the end points or the first step; no
  Gorilla-style bit packing; relations only `* + - /` with optional decimal rounding (`tax = total * 0.08`
  against a constant is not in the DSL).
* Integers: recurrences only with integer coefficients up to 2^20 and order up to 3; one predictor per column
  (no switching between predictors inside a column).
* Strings: templates split only on canonical integer digit runs (no hex, decimals or signs); Markov ranks only
  up to 1024 distinct values; functional dependencies only from one source column.
* Rule mode is per block; rules are not shared between files (every segment of a log searches again).

## 8. Not measured

* Real (non-generated) machine data, real server logs, real order tables; only generators written here.
* Files of the Digital Life log itself (1000-row segments, revision tables); the events table was measured as
  one block.
* Sizes above 1M rows; tables with many columns (the search grows with columns^2); tables beyond 64 columns.
* Encode/decode speed on an idle machine; memory use of the writer and reader.
* Parquet with other encodings (delta, byte-stream-split), ORC, DuckDB, bz2 or zstd-19 CSV on these datasets.
* Rule-mode files inside the versioned log (`versioned::*`).
* Fuzzing beyond the byte-flip, truncation and recomputed-CRC sweeps in the tests.
* Readers in other languages (Java, C#, Python bindings): none of them read version 5.
