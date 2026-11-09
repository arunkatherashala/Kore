# Version-4 writer: i64 transform selection (timestamp fix)

Date: 2026-11-08. Branch `v4-ts-fix`, based on `fileformat` @ d07924bf. Code commit: b596067b (this note
is in the commit after it). Nothing was pushed.

## 1. Problem

For an i64 column the version-4 writer has three layouts (transforms): `Plain` (8 bytes LE), `Delta`
(zigzag varint of the difference to the previous non-null value) and `Varint` (zigzag varint of the value).
It picked one by a pre-score, `min(raw, zstd level 1 + 4)` on the whole column
(`kore-store/src/v4.rs`, `encode_column` lines 181/186 and `encode_column_pruned` lines 306/316 at
d07924bf), and only the winner went through the stage search (none, lz4, zstd at level 9 or 19, hz).

zstd level 1 ranks integer layouts differently from the level that is written. On the NYC taxi file
(int64 microseconds, whole seconds, consecutive pickups within about +-1 h of each other):

| tpep_pickup_datetime | raw | zstd-1 (pre-score) | zstd-9 | zstd-19 |
|---|---:|---:|---:|---:|
| Plain | 23,716,993 | 11,826,983 | 10,243,363 | 10,054,722 |
| Delta | 14,556,642 | 12,218,965 | **7,179,270** | **6,216,354** |
| Varint | 23,716,993 | **10,080,164** | 9,561,627 | 10,533,559 |

The pre-score picked `Varint`; at the written level `Delta` is 25 % (level 9) and 41 % (level 19) smaller.
The same miss happened on `PULocationID` / `DOLocationID` (pre-score `Plain`, smaller: `Varint`) and on
several TPC-DS and generated columns (section 3).

## 2. Fix (writer only, no format change)

`kore-store/src/v4.rs`, `int_finalist` and its callers:

* For an i64 column under `Transform::Auto` (stage `Auto` or `Zstd`) where some candidate has at least
  256 KiB of raw bytes, every transform is also scored on a row sample: 4 windows of 16,384 consecutive
  rows spread over the column (first and last included; the whole column if it has at most 65,536 rows),
  compressed with the zstd level its full payload would get (`zstd_level` at 256 KiB and above,
  `small_zstd_level` below).
* If the sample ranking has another winner than the zstd-1 pre-score, both finalists go through the full
  stage search; the sampled one is kept only if it is **strictly smaller**.
* Consequences:
  * a column is never larger than with the earlier choice, and when the earlier choice is at least as small
    the bytes are exactly those of the earlier writer;
  * columns below 256 KiB raw, other types, explicit transforms, explicit non-zstd stages and
    `SearchMode::Quick` are unchanged;
  * the written transform ids are the existing ones (0 Plain, 1 Delta, 2 Varint), so every reader of
    version 4/5/6 reads the new files, and every old file reads as before. No id, byte or layout changed.
* The reference search (`write_reference`), the pruned search (`write`, on the calling thread and under
  rayon), `write_many`, and rule mode's version-4 baseline (`encode_for_write`) and embedded version-4
  columns all share the selection, so they still write identical bytes. Rule mode (container version 6)
  is therefore measured on the fixed version-4 encoder.

Tests (`kore-store/tests/v4_int_select.rs`, 6 tests): round trips of monotone, near-monotone, taxi-like,
random, NULL-heavy, all-NULL and i64-extreme columns through version 4 (default, fast, quick), version 5,
rule mode, a projection and a row range; odd sizes around the sample thresholds; the selector picks
`Delta` on a taxi-like column where zstd-1 prefers `Varint` (this test and one other fail on d07924bf);
random data keeps `Plain`; all writer paths byte-identical on large columns; explicit codecs and quick mode
unchanged. The 19 pinned version-4 and version-5 writer hashes, the conformance fixtures and every other
existing test pass unchanged: `cargo test --offline --no-fail-fast` in kore-store, 111 passed, 0 failed.

## 3. Measurements

One machine (Windows 11, 8 logical CPUs, release build), shared with other work the whole time (CPU
near 100 %): sizes are deterministic, times are noisy (the same binary gave 3.2 s and 3.9 s on the same
input). Before = d07924bf, after = this branch, both built from the same `examples/bench_v4_int_select.rs`;
every file was read back and compared (digest of every column). Write/read times are medians of 3
repetitions except the taxi file at level 19 (1 repetition).

Datasets: the NYC TLC Yellow Taxi trip records of January 2024 (SHA-256 c4d59da7..., 2,964,624 rows x 19
columns, copied into an empty scratch directory and exported with `scripts/kore_rules/export_parquet.py`
under `python -I`, as in docs/kore_rule_mode.md); TPC-DS generated locally by DuckDB dsdgen at SF1
(`scripts/kore_rules/export_tpcds.py`, same rows as in docs/kore_rule_mode.md; the before sizes equal
the version-4 sizes published there); the generated sets of `examples/support/rules_gen.rs` (seed 20261107).
Neither the data nor any result file is committed.

### Size (bytes)

| dataset | rows | v4 default before | after | change | v4 level 19 before | after | change |
|---|---:|---:|---:|---:|---:|---:|---:|
| NYC taxi Jan 2024 (real data) | 2,964,624 | 51,938,326 | 45,624,007 | -12.2 % | 45,758,509 | 36,892,694 | -19.4 % |
| TPC-DS store_sales (dsdgen) | 1,000,000 | 38,363,953 | 38,118,034 | -0.6 % | 34,351,503 | 34,154,679 | -0.6 % |
| TPC-DS web_sales (dsdgen) | 719,384 | 41,092,224 | 40,883,866 | -0.5 % | 37,823,671 | 37,755,149 | -0.2 % |
| TPC-DS customer (dsdgen) | 100,000 | 3,397,226 | 3,326,991 | -2.1 % | 3,417,888 | 3,315,395 | -3.0 % |
| TPC-DS customer_address (dsdgen) | 50,000 | 582,645 | 582,645 | 0 | 595,331 | 595,331 | 0 |
| TPC-DS item (dsdgen) | 18,000 | 944,113 | 944,113 | 0 | 870,249 | 870,249 | 0 |
| sensors (generated) | 1,000,000 | 10,077,059 | 10,019,102 | -0.6 % | 8,369,739 | 8,369,739 | 0 |
| orders (generated) | 1,000,000 | 18,415,760 | 17,940,066 | -2.6 % | 15,932,314 | 15,742,317 | -1.2 % |
| logs (generated) | 1,000,000 | 8,401,683 | 8,401,683 | 0 | 7,399,696 | 7,399,696 | 0 |
| random (control) | 1,000,000 | 39,509,650 | 39,509,650 | 0 | 39,511,865 | 39,511,865 | 0 |
| sensors (generated) | 100,000 | 1,012,868 | 1,010,653 | -0.2 % | 879,413 | 877,198 | -0.3 % |
| orders (generated) | 100,000 | 1,847,660 | 1,805,831 | -2.3 % | 1,663,420 | 1,636,514 | -1.6 % |
| logs (generated) | 100,000 | 831,693 | 831,693 | 0 | 766,189 | 766,189 | 0 |
| random (control) | 100,000 | 3,951,075 | 3,951,075 | 0 | 3,951,528 | 3,951,528 | 0 |

No dataset and no column got larger. Taxi at level 19, per column: pickup 10,533,559 -> 6,216,354
(Varint -> Delta), dropoff 10,650,855 -> 6,354,806 (Varint -> Delta), PULocationID 2,528,774 -> 2,402,266 and
DOLocationID 2,777,746 -> 2,651,693 (Plain -> Varint); the other 15 columns are byte-identical.

### Time (ms; write = `to_bytes_compact`, read = full `from_bytes_with_dicts`)

| dataset | setting | write before | write after | read before | read after |
|---|---|---:|---:|---:|---:|
| NYC taxi | default, run 1 / run 2 | 3,945 / 3,167 | 4,069 / 4,031 | 348 / 397 | 296 / 339 |
| NYC taxi | level 19, run 1 / run 2 | 41,340 / 93,067 | 67,186 / 107,794 | 400 / 504 | 258 / 867 |
| TPC-DS store_sales | default / level 19 | 4,083 / 37,915 | 4,219 / 44,876 | 448 / 385 | 414 / 376 |
| TPC-DS web_sales | default / level 19 | 3,378 / 44,618 | 3,563 / 42,812 | 387 / 552 | 350 / 447 |
| TPC-DS customer | default / level 19 | 1,051 / 6,411 | 1,470 / 7,036 | 207 / 271 | 173 / 159 |
| sensors 1M | default / level 19 | 1,466 / 11,697 | 2,285 / 14,158 | 287 / 274 | 348 / 365 |
| orders 1M | default / level 19 | 2,107 / 20,167 | 3,038 / 28,817 | 464 / 425 | 442 / 558 |
| logs 1M | default / level 19 | 3,877 / 94,098 | 3,557 / 40,058 | 508 / 833 | 518 / 256 |
| random 1M | default / level 19 | 1,946 / 15,443 | 532 / 8,601 | 529 / 503 | 104 / 135 |

* **Write gets slower** where a second finalist is encoded: on the taxi file about +3-27 % at the default
  level and +16-63 % at level 19 (two paired runs each; the second finalist at level 19 is a full zstd-19 pass
  over 14.6 MB per timestamp column). Datasets without a disagreement pay only the sample (4 x 16K rows x 3
  transforms); their differences here (logs, random: faster "after") are machine noise.
* **Read** is not affected by the selection itself (same decoder); delta decodes about as fast as varint.
  The differences in the table are within the noise of this machine.

### Rule mode and the other formats on the fixed version 4

Rule mode reuses the version-4 encoder, so it was re-measured on this branch (`bench_rules --sizes 0
--reps 1`), and Parquet / xz with `scripts/kore_rules/py_side.py` (same sizes as on 2026-11-07):

| dataset | rule mode L19 | v4 L19 fixed | Parquet zstd-22 | CSV + xz-9 | rule vs fixed v4 | rule vs Parquet |
|---|---:|---:|---:|---:|---:|---:|
| NYC taxi Jan 2024 | 29,506,067 | 36,892,694 | 45,674,181 | 34,640,960 | 1.25x | 1.55x |
| TPC-DS store_sales (dsdgen) | 18,499,338 | 34,154,679 | 36,998,186 | 37,044,892 | 1.85x | 2.00x |
| TPC-DS web_sales (dsdgen) | 17,850,212 | 37,755,149 | 40,697,025 | 40,965,588 | 2.12x | 2.28x |
| TPC-DS customer (dsdgen) | 3,098,025 | 3,315,395 | 3,983,727 | 3,995,844 | 1.07x | 1.29x |
| TPC-DS customer_address (dsdgen) | 573,848 | 595,331 | 776,300 | 882,936 | 1.04x | 1.35x |
| TPC-DS item (dsdgen) | 836,259 | 870,249 | 1,016,554 | 1,045,212 | 1.04x | 1.22x |

The earlier taxi figure "1.55x against v4 level 19" (released with 1.8.20/1.8.21) included about 8.6 MB of
this version-4 bug; against the fixed version 4 the rule-mode gain is 1.25x. docs/kore_rule_mode.md
section 4 is corrected accordingly.
