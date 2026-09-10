# KORE Parity Test Report (Principal Architect)

Date: 2026-08-22
Scope: Functional parity checks + performance parity checks against Spark baselines used by kore-tpch.

## 1. Test Gates Executed

1. Functional correctness gate
- Command: python validate_kore.py
- Artifact: validate_kore_out_utf8.txt
- Result: PASS (15/15)

2. Scale-1 parity gate
- Command: cargo run --release -p kore-tpch -- --scale 1
- Artifact: tpch_scale1_parity.txt
- Result: PASS

3. Scale-5 parity gate
- Command: cargo run --release -p kore-tpch -- --scale 5
- Artifact: tpch_scale5_parity.txt
- Result: PASS

## 2. Functional Parity Results

From validate_kore_out_utf8.txt:
- Result: 15/15 PASSED, 0 FAILED
- IDs parity check passed
- SUM(amount) parity check passed (expected=1083.5, got=1083.5)

Interpretation:
- KORE read/write/query behavior is consistent for the validated dataset.
- Binary header/trailer integrity checks passed.

## 3. Performance Parity Results (Spark Baseline)

Scale 1 (tpch_scale1_parity.txt)
- D1 Distributed GROUP BY: KORE 72.0 ms vs Spark 11300.0 ms (157.1x)
- Total KORE time: 3076.3 ms
- Total Spark time: 234900.0 ms
- Average speedup: 360.9x

Scale 5 (tpch_scale5_parity.txt)
- D1 Distributed GROUP BY: KORE 294.9 ms vs Spark 11300.0 ms (38.3x)
- Total KORE time: 15677.9 ms
- Total Spark time: 234900.0 ms
- Average speedup: 75.4x

Interpretation:
- KORE remains faster than Spark baseline at both scales.
- D1 distributed path remains clearly faster than Spark baseline at SF-1 and SF-5.

## 4. Architecture Verdict

Parity status: PASS

- Functional parity gate: PASS
- Query-level performance parity gate: PASS
- Distributed D1 parity gate: PASS

KORE is parity-validated on the current harness for both correctness checks and benchmark-based Spark comparisons.

## 5. Risks and Caveats

1. Spark values are published baseline constants embedded in kore-tpch, not a live Spark execution in this specific run.
2. Some output files are UTF-16/console-encoded, which can complicate downstream parsing scripts.
3. Build emits many warnings; not blocking for parity, but should be reduced before GA for maintainability.

## 6. Recommended Follow-up

1. Add a live Spark/DuckDB same-data result equivalence harness (result-set equality with tolerances for floating point).
2. Add CI parity job that fails on:
- Any functional test failure
- D1 speedup regression below threshold
- Aggregate speedup regression below threshold
3. Normalize benchmark output encoding to UTF-8 in all scripts.
