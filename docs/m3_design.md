# M3 design: World M3, a World T-like world built from one real public file (design only)

Written 2026-11-11 before any M3 code, build or run. This is a design and a pre-registration draft. The owner approves
it (and the decisions listed at the end) before code; after that, nothing in "Measurement" changes after a report run.

M3 in `docs/milestones.md`: a World T-like world built from a real public dataset, with exact checkers, in which the
being beats tuned LinUCB, UCB1 and always-X, paired CI above 0. World T (`docs/world_t.md`) is the template: same
loop, learner, log, fold, replay, controller and authorize path. What changes is where the values come from.

## The source file and what is known about it

NYC TLC Yellow Taxi trip records, January 2024, one Parquet file supplied by the owner, 49,961,641 bytes, SHA-256
`c4d59da7bbc8abaeeeb1727947ee93d9891a71acb42854bd80db1571b2030510` (the hash of the file as supplied, not a
verification of its source), 2,964,624 rows x 19 columns. Facts below were computed on 2026-11-11 on a copy in an
empty scratch directory (hash checked after the copy), with DuckDB 1.5.6 from the existing read-only conda env
`engines`, `python -I`, script in another directory, path as an argument; nothing was installed; the copy and the
script were deleted afterwards; nothing derived was kept or committed.

* Columns: VendorID, tpep_pickup_datetime, tpep_dropoff_datetime, passenger_count, trip_distance, RatecodeID,
  store_and_fwd_flag, PULocationID, DOLocationID, payment_type, fare_amount, extra, mta_tax, tip_amount, tolls_amount,
  improvement_surcharge, total_amount, congestion_surcharge, Airport_fee (integers, timestamps, doubles, one text).
* NULLs: passenger_count, RatecodeID, store_and_fwd_flag, congestion_surcharge and Airport_fee are NULL on 140,162
  rows, exactly the rows with payment_type 0. No other column has NULLs.
* Real anomalies (kept, never cleaned): pickup outside January 2024 on 18 rows (earliest 2002-12-31, latest
  2024-02-01 00:01); dropoff before pickup 56; trips longer than 24 h 16; fare_amount < 0 on 37,448 rows (minimum
  -899.00, maximum 5000.00); total_amount < 0 on 35,504; trip_distance 0 on 60,371 and > 100 miles on 59 (maximum
  312,722.3); passenger_count 0 on 31,465; RatecodeID 99 on 28,663; VendorID 6 on 260 rows (1: 729,732, 2: 2,234,632);
  total_amount differs from the sum of its eight components by more than 1 cent on 761,838 rows (675,956 of them
  vendor 1). Every money value has at most 2 decimals (checked: `round(x, 2) = x` on all fare rows). Rows per pickup
  day: 67,543 to 110,515.

## The base table and the files (reproducible from (file hash, seed))

* Load: the run reads the Parquet file in Rust through `kore-parquet` (already a `kore-self` dependency); no Python at
  run time. The base table keeps the file's row order; `trip_id` = 0-based row index in the file (a real index, the
  only added column). Money becomes integer cents and trip_distance hundredths of a mile by `round(x * 100)`; the
  loader asserts `|x * 100 - round(x * 100)| < 1e-6` on every value and refuses the file otherwise (fail closed).
  Timestamps become seconds. NULL stays NULL.
* A world is a pure function of (source SHA-256, `M3Cfg`): `seed`, `budget` 6000, `start_kinds`, `intro_every`,
  `penalty_pm`, `cost_pm`, `ood`, and a code constant `M3_VERSION`. Genesis logs all of them (tags `m3_*`). The RNG
  is World T's `Rng`/`mix`.
* Slices (contexts, visible in the file name and the task): `day` (all trips picked up on day d), `vendor` (day d,
  one VendorID), `zone` (day d, PULocationID in a seeded set of 20 ids), `night` (day d, pickup 00:00 to 05:59),
  `airport` (PULocationID 1, 132 or 138: Newark, JFK, LaGuardia in the TLC zone list, from memory; the zone lookup
  file is not used), `misc` (a sample of day d plus all 18 out-of-month and all 56 dropoff-before-pickup rows).
* A file: slice spec drawn from the seed, then the n rows of the slice with the smallest `mix(seed, file_idx,
  trip_id)`, n by World T's size classes (40-120, 150-350, 400-700 rows; OOD 1000-2500), written in (pickup, trip_id)
  order with `trip_id` first and a seeded subset of 7 to 12 other columns (always those the file's templates need).
  About 200 table files per world, 6 per context and size as in World T.
* Rendering quirks (author-defined, per context a chance drawn from the seed in a fixed range, then per file; the
  same set as World T): delimiter (`,` `.csv`, tab `.tsv`, `|` `.txt`, `;` legacy), decimal commas with `;`, CRLF,
  byte-order mark, padding, one NULL token per file (empty, `NA`, `NULL`, `-`), and one taxi-specific quirk:
  timestamps as `MM/DD/YYYY hh:mm:ss AM` instead of ISO in some files. Values are never altered by rendering.
* Series files: `hourly_<ctx>.txt` (trips per pickup hour, 744 lines, contexts city / airport / night-zone set) and
  `daily_vendor<v>.txt` (31 lines). Computed from the base table, not from a rule.
* Measurement lives keep the rendered bytes in memory (as Gate 0 and W2); only the logged run writes them, to
  `<data root>/<owner>/world_m3/seed_<seed>/` via `safety::safe_path`, outside the repository.
* Reproducibility test: two builds of seed 101 give the same pool digest (sha256 over file names and bytes); that
  32-byte digest is pinned in a golden test (a hash, not data).

## Task kinds and exact checkers

The truth is computed from the base-table rows of the file (never from the rendered bytes), so a loader that misreads
a quirk is wrong. Money answers are canonical cents text (`1234.50`, sign kept). The prompt states the convention
("NULLs ignored", "values as recorded, negatives included", "ties: smallest id"). Columns drawn for a task have at
least one non-NULL value in the file; parameters that make a task ambiguous are redrawn.

| kind | templates | truth and tolerance | plans (energy as World T, r = rows) |
|---|---|---|---|
| colstat | sum, max, row count, mean, distinct of a column | exact; NULLs ignored (count = rows); negatives included; mean accepts either rounding of a half cent | naive/sniff x native/sql |
| filter | rows with col op k (e.g. distance > 10.00, payment_type = 2); total of col where cat = v | exact; NULL never satisfies a predicate | naive/sniff+sql, `native-filter` (new, 2 + r/200) |
| group | the cat with the largest total; top-3 PULocationID by trips | exact; ties broken by smallest id, answer `132,161,237` | naive/sniff+sql |
| quantile | median; p90 (nearest rank: value at rank ceil(0.9 n) ascending) of a money column | exact; median of even n = mean of the two middle values, either rounding of a half cent | sniff+native (sort), sniff+sql (`MEDIAN`; p90 by count then `LIMIT 1 OFFSET`) |
| lookup | column C of trip_id K | exact | the four table plans |
| anomaly (real) | rows with fare < 0; distance = 0; total != sum of components (cents, exact); pickup outside the file's day / outside January; first trip_id with dropoff before pickup | exact over the real values; the definitions are fixed in the prompt | naive/sniff+sql, `native-filter` |
| fix_row | the 1-based line of the one injected malformed line (extra field, missing field, bad number, impossible date such as 2024-01-32) | exact by construction; real but valid values (a 2002 date, a negative fare) are not malformed | fix-fieldcount, fix-typecheck |
| format | delimiter and header / no header | exact | fmt-extension, fmt-sniff |
| ts | next hourly or daily count from the first k values (hourly k = 168..743, daily k = 14..30) | `abs(a - v) <= max(3, round(0.05 v))`; the true value is revealed after an attempt | ts-last, drift, season7, season24, mean7, ts-rule |

No `derived` kind: total_amount is not the sum of its components on 25.7% of rows, so the World T column grammar has no
exact target here; that is reported, not patched. Listed rewards as World T (colstat and lookup 6, filter, group,
anomaly and fix 9, quantile 8, ts 7, format 4, plus 2 per size class); a skip costs 1.

**Checker cross-check (gate before any report run):** for every task of tuning seeds 101..120, the Rust truth is
compared with an independent DuckDB query over the rendered file read with an explicit, correct dialect (in a scratch
directory outside the repo, `python -I`, read-only env, nothing installed). Any disagreement stops the track until it
is explained and fixed. Cross-check outputs are deleted, not committed.

**Not 93.9% deterministic.** In World T 93.9% of (template, file, plan) outcomes are fixed by the author's generator,
which a per-file key exploits. Here the checker is still exact and every plan is still a deterministic function of
(file, parameter), but whether a plan is right depends on real values the author did not choose: whether this sample
has a NULL in this column, a negative fare, a tie at the top, an even n with a half cent, a non-January date. So the
determinism share is not known in advance; it is measured on tuning seeds and reported (secondary S5), with the
per-template table of which plan is right how often. The series are not generated by a rule, so `ts-rule` may never
hold a rule; that is a result, not a defect.

## Plans, tools, energy

The being's World T tools unchanged: naive and sniff loaders, `native`, KORE SQL through `tools::guarded_sql`
(read-only check, fresh `KqlContext` with only `t`, timeout and row limit from `world_t_sql_timeout_ms` /
`world_t_sql_max_rows`, digest logged), fix, format and series tools, its own series rules (B2 search). Two additions,
both small: `native-filter` (one predicate or group-by-one-column count/sum over the loaded table) and the sniffer
learns the US timestamp format (rewrites to ISO; the naive loader does not, so its SQL date comparisons on such files
are wrong). Energy formulas are World T's `Plan::energy` by rows; `cost_pm` and `penalty_pm` as in W1b. Known engine
limit kept: no `PERCENTILE_CONT` (item 4 of World T's engine needs), hence nearest-rank p90.

## Baselines (measurement only, same loop, same tasks)

| policy | what | tuned on 101..120 (6 values x 20 seeds, objective mean reward per energy, penalty 0, weight 1) |
|---|---|---|
| B0 | default being (`wt_file_key` off) | beta 0.5, 1, 2, 4, 8, 16 |
| B1 | being with `wt_file_key` on | beta 0.5, 1, 2, 4, 8, 16 |
| random | uniform over the template's plans, no skip | none |
| always SQL | sniff+sql where the template has it, else skip | none |
| always cheapest | the lowest listed energy plan, no skip | none |
| UCB1 | arms per (template, context, plan), skip on | c 0.001, 0.003, 0.01, 0.03, 0.1, 0.3 |
| LinUCB | W1b LinUCB, skip on | alpha 0.1, 0.3, 1, 3, 10, 30 |
| LinUCB+file | LinUCB plus a one-hot of the file (W2) | alpha 0.1, 0.3, 1, 3, 10, 30 |
| reward-best oracle | sees every plan's true result (World T definition) | reference only, no claim |

If a tuned optimum lies on a grid edge, the grid is extended once by two values on that side for that policy only and
the extension is reported. No tuning on report seeds.

## Measurement (pre-registered once approved)

* Seeds: tuning **101..120** only (all tuning, the cross-check, the B0/B1 choice). Report seeds **401..460** (n = 60),
  never used by any measurement (1..60 and 201..260 used; 261..320 reserved for W2b). The report runs **once**.
* Setting: World M3, penalty 0, energy weight 1, budget 6000, `ood` off.
* Candidate: **B0** (the default being), beta from tuning. (Owner decision 1.)
* **PRIMARY (one comparison):** candidate minus the strongest baseline, where "strongest" is the one of {random, always
  SQL, always cheapest, UCB1, LinUCB, LinUCB+file} with the highest tuned mean on 101..120, named in the log before the
  report run. Reward per energy, paired 95% t-CI (t, 59 df, 2.001): **lower bound > 0 AND at least 36 of 60 wins.**
* **M3 is met** only if the PRIMARY is met AND S1 holds. Otherwise M3 is recorded as not met.
* SECONDARY (all reported, W/T/L and paired CI for each):
  * S1: candidate minus each of the six baselines; one-sided paired t "candidate - baseline <= 0", Holm over the six
    at family-wise 0.05; S1 holds if all six are rejected.
  * S2: wrong answers per 100 tasks, coverage, skip share, for every policy; skip-off variants of B0, UCB1, LinUCB.
  * S3: share of the gap to the reward-best oracle closed by the candidate.
  * S4: B1 minus B0 (does the file key help on real slices?).
  * S5: determinism share of (template, file, plan) outcomes and plan-right rates per template (descriptive, tuning seeds).
  * S6: small sweep, penalty {0, 1, 10} x weight {0.316, 1, 3.16}, every policy re-tuned per point on 101..120; per
    point candidate minus the best baseline; Holm over 9 points for regressions.
  * S7: OOD (`ood = true`: size class 3, columns reversed; never tuned): candidate minus each baseline.
  * S8: wall time per life, peak memory, base-table load time; S9: 0 wrong rules accepted.
* Stop rule: one tuning pass (plus at most one edge extension), one cross-check pass, one report run of the PRIMARY and
  S1 to S5, S7 to S9, one of S6. Before the report run the track stops for: hash mismatch, cross-check disagreement,
  failed golden or replay test, free disk below 4 GB. After the report run no constant, template or checker changes;
  a fix found later is a new world version with new seeds. **Negative results are reported as negative**, including a
  PRIMARY or M3 failure, in `docs/measured.md` under "Track M3".

## Untrusted data handling

* The source file is untrusted. The owner (or a script) copies it into a new empty directory outside the repository
  (`<data root>/<owner>/world_m3/source/`); nothing there is executed; scripts live elsewhere and get paths as arguments;
  any Python (only the cross-check) runs with `python -I` from the read-only env, nothing installed.
* Before every run (logged run and each measurement process) the SHA-256 is recomputed and compared with the constant
  pinned in code; a mismatch refuses the run (`controller_refusal`, audit row). Also refused: file > 64 MB, row count
  != 2,964,624, more than 3,000,000 rows read, column names or types different from the list above, any value that
  fails the cents assertion. The Parquet reader runs with these caps before allocating the base table.
* Never committed: the Parquet file, the base table, rendered files, cross-check outputs, per-life TSVs that contain
  values from the file. Committed per-seed TSVs hold only policy, seed, reward, energy, counts. `.gitignore` gains
  `world_m3/` and `*.parquet` as a second guard; the data directory is outside the repo anyway.
* Controller key **`world_m3_run`** (default off, fail closed: missing or invalid `controller.txt` refuses), checked with
  `KILL` (also before every step) and `authorize()` of the internal tool `world_m3_run`, tier L2 (reads its sandbox,
  appends to its log; nothing leaves them). World T keys and golden tests are untouched: with M3 off, World T, the first
  world, World v2 and World O stay byte-identical (`tests_golden.rs`). Log source `being:worldm3`.
* Disk guard: before writing the sandbox and before each measurement batch, free disk on the data volume is read; below
  **4 GB** the run stops (logged), nothing is written. Peak memory target below 1 GB (base table with narrow columns,
  loaded once per process, shared read-only).

## Limits stated now

One month of one city's yellow-taxi records; one file. The templates, slices, rendering quirks, rewards and energies are
author-written; only the values and their anomalies are real. The injected malformed line of `fix_row` is synthetic.
Tuning and report worlds draw from the same month (different slices and samples, same population), so this is not a
test on new data. The zone-to-airport mapping is from memory. A pass shows that on this world the being's plan choice
earns more reward per energy than these tuned baselines; it is not a claim of thinking or understanding, and not a
claim about other datasets.

## Decisions for the owner

1. Candidate for the PRIMARY: B0 (default) as written, or B1, or "whichever of B0/B1 is better on tuning seeds".
2. PRIMARY threshold: lower bound > 0 and 36 of 60 wins, as written, or add a minimum mean (e.g. +0.05).
3. Whether the source hash is a code constant (as written: a different file needs a code change) or a controller key.
4. Approve the two tool additions (`native-filter`, US timestamps in the sniffer) and dropping the `derived` kind.
5. Approve report seeds 401..460.

## Owner decisions (2026-11-11)

Recorded before any M3 code. They replace the open points above where they differ.

1. PRIMARY candidate: **B0** (the default being), as written. B1 is reported as a secondary (S4).
2. PRIMARY threshold: paired 95% CI lower bound **> 0 AND mean >= +0.05 AND at least 36 of 60 wins**.
3. Source hash: a **code constant**, as written (a different file needs a code change).
4. The two tool additions (`native-filter`, US-timestamp sniffing) are approved; the `derived` kind is dropped.
5. Report seeds **401..460**, run once. Tuning seeds **101..120** only.

Airport zone ids, checked against the file itself before code (DuckDB over the copy, same handling as above). The
file has no zone names, so the ids are inferred from TLC fare rules visible in the data: rows with Airport_fee > 0
(the fee is charged on pickups at JFK and LaGuardia) are picked up in zone 132 (133,913 rows) and 138 (87,952), then
70 (10,131, a zone next to LaGuardia) and a few hundred elsewhere; RatecodeID 2 (the JFK flat fare) is picked up
mostly in 132 (71,855); RatecodeID 3 (Newark) is dropped off mostly in zone 1 (5,893). So 132 = JFK, 138 = LaGuardia,
1 = Newark is consistent with the file but not proven by it (no names; the fare rules are indirect evidence).
Pickups: zone 1 295, 132 145,240, 138 89,533. Timestamps are whole seconds on every row (0 with a sub-second part).
