# Engine robustness: LIKE matcher and input-size recursion

Branch `engine-improve` (from `master-kore-engine` @ 8b69327e). Not pushed; for owner review.

## 1. LIKE: the engine line already has the fix

The Digital Life fix (`self-b6`, commit 802b446e: iterative matcher + 2 MB test) is **not needed as a port**.
`master-kore-engine` already carries an iterative matcher, introduced by 00915e81 ("fix ... LIKE escapes"):
`like_match` in `kore-sql/src/executor.rs`. It is a superset of the self-b6 version:

| | original (b7aec916) | self-b6 | engine line (this branch) |
|---|---|---|---|
| recursion | yes, 1 frame per byte | no | no |
| `_` | one byte | one byte | one UTF-8 character |
| `\x` escape | no | no | yes (`ESCAPE 'c'` is rewritten by the parser to `\`) |

So nothing was changed in `like_match`. Replacing it with the self-b6 matcher would be a regression (byte-wise `_`, no escapes).
Other LIKE paths: `vecexpr.rs` `LikePlan` (substring search, only for patterns without `_`/`\`) falls back to `like_match`; it already has an agreement test.
ILIKE lowercases both sides and calls `like_match`; NOT LIKE / NOT ILIKE negate; NULL operand gives NULL (so the row is excluded for both forms).

### Reproduction (old behaviour)
`executor::like_repro::old_recursive_matcher_overflows` (ignored; it kills the test binary by design) contains the original recursive matcher.
`KORE_OLD_LIKE_LEN=10000 cargo test -p kore-sql --lib like_repro -- --ignored --nocapture` -> `thread has overflowed its stack`,
STATUS_STACK_OVERFLOW (0xc00000fd) in a debug build on a 2 MB test thread; 6,000 bytes still passed, 10,000 crashed. The current
`like_match` runs the same input fine.

### Worst case
Single saved star position (standard two-pointer backtracking): O(len(text) * len(pattern)) worst case, O(len(text)+len(pattern)) typical, O(1) extra space.
It is never exponential. Measured: `%a%a%a%a%a%a%a%a%b` on 100,000 `a` finishes well under the 20 s test bound, and 20 stars on 20,000 chars via SQL under 10 s.

### Tests added
- `executor::like_robust_tests` (unit): differential against a recursive char-based reference with the current semantics
  (300,000 random value/pattern pairs over `a b é 日 😀 % _ \`, up to 7 symbols; plus an exhaustive sweep of every pattern
  over `{a,é,%,_,\}` up to length 5 against every value over `{a,b,é}` up to length 5; named edge cases: empty strings, `%%`, trailing
  backslash, multi-byte `_`); differential against the original recursive matcher (verbatim copy) on 200,000 ASCII pairs; 2 MB text
  and multi-byte text of 2.7 MB with several wildcards; adversarial star pattern.
- `tests/like_robust.rs` (SQL level, default 2 MB test-thread stack): 2 MB rows with LIKE/ILIKE/NOT LIKE/NOT ILIKE/ESCAPE, `_`, several `%`,
  NULL and empty rows, a 200 KB pattern, adversarial pattern timing.
- `executor::like_bench` (ignored micro-benchmark).

### Benchmark (release, typical short strings)
64 (value, pattern) combinations of 8 short values (0 to 24 chars) and 8 patterns, 12.8 M matches per run, 11 runs, ns per match:

| matcher | median | min | max |
|---|---|---|---|
| original recursive (b7aec916) | 45.9 | 32.8 | 53.1 |
| current iterative (unchanged on this branch) | 29.4 | 23.0 | 38.1 |

The machine was loaded (other builds running), so treat the absolute values as noisy; direction is consistent (iterative is faster).
Since `like_match` is unchanged there is no LIKE regression by construction. The parser guard below adds one stack-address comparison per
`parse_expr`/`parse_unary`/`parse_compound`/`parse_table_expr` call (not benchmarked separately; parse time is dominated by tokenising).

## 2. Audit of other input-size recursion (kore-sql, kore-sql-v2, kore-subquery)

Method: `tests/deep_input.rs` re-executes the test binary per probe (an overflow aborts the process) on the default 2 MB thread stack,
debug and release. Probe sizes: 5,000-deep nesting, 5,000-term chains, 100,000-element IN lists.

kore-sql-v2 and kore-subquery are thin wrappers (DISTINCT/EXCEPT/INTERSECT/ROLLUP helpers, semi/anti-join over value sets) over `KqlContext`;
they contain no recursion over input size and no LIKE/regex code. Their exposure is whatever kore-sql does with the SQL they pass in.

Results before the fix (release; debug is worse, parser overflows at about 50 parenthesis levels):

| probe | result before |
|---|---|
| `((((a))))` 5,000 deep (WHERE and SELECT) | STACK OVERFLOW (parser, ~500 levels in release) |
| `NOT NOT ... a=1`, `- - - a` x5,000 | STACK OVERFLOW (parser) |
| `abs(abs(...))`, nested CASE x5,000 | STACK OVERFLOW (parser) |
| nested subqueries / `IN (SELECT .. IN (SELECT ..))` x5,000 | STACK OVERFLOW (parser, ~200 levels) |
| `a AND a AND ...`, `OR`, `+`, `||`, `LIKE .. AND LIKE ..` x5,000 | STACK OVERFLOW (parser builds the chain in a loop, a later phase recurses on the left-deep tree; ~3,000 links in release, ~300 in debug); 100,000 chain also overflows |
| `IN (...)` / `NOT IN (...)` with 100,000 ints or strings | OK |
| `UNION ALL` x5,000, 2,000 chained CTEs | OK |

### Fixed (each had a failing probe first)
In `kore-sql/src/parser.rs`:
- **Stack-budget guard**: `Parser` records the address of a local at `parse_query`; `parse_expr`, `parse_unary`, `parse_compound`,
  `parse_table_expr` return `InvalidArgument("query is nested too deeply ...")` when more than `PARSER_STACK_BUDGET` bytes of stack are used
  (400 KB release, 1.2 MB debug, since debug frames are about 10x larger). This is profile independent in effect and covers parentheses, NOT/unary chains, function
  nesting, CASE and nested subqueries. Release allows roughly 100 paren levels, debug roughly 25.
- **Operator chain cap**: more than `MAX_OPERATOR_CHAIN` (1,000 release, 250 debug) left-associative operators in one expression level
  returns a clear error ("use IN (...) for long lists"). Release 2,000-term chains used to work on a 2 MB stack and now error; that is a
  deliberate behaviour change, because anything near 3,000 aborted the process. IN lists are unaffected (no cap).
- Regression tests: `nesting_returns_error_not_abort` (14 probes, run in child processes), `moderate_nesting_still_parses`.

### Not fixed / not covered
- The cap values are conservative heuristics, not exact: the real limit depends on the thread stack size. A thread with a 1 MB stack
  (Windows main thread) has less headroom than the 2 MB test threads the numbers were measured on. Where the MCP server runs queries
  (thread stack size) was not checked.
- The root cause of the chain overflow (which of bind/rewrite/eval/Drop recurses per link) was not isolated; the cap is a guard, not a
  structural fix. A structural fix would flatten AND/OR/+/|| into n-ary nodes or evaluate on a large-stack thread.
- Debug build chain cap (250) is lower than release (1,000); a debug test using a longer chain will get an error.
- ASTs built programmatically (not via the parser) are not protected.
- Regex-like helpers: `scalar.rs` `regex_of` / `maps.rs` use the `regex` crate (linear-time, no backtracking recursion; compiled-size
  limit applies); user patterns were not fuzzed.
- Other recursive walkers (`ast_walk`, `rewrite`, `desugar`) were only exercised indirectly through the probes, not individually.
- 100,000-term chains in the lexer/parser only tested up to the error path.

## 3. Test results
See the final report; the full `cargo test -p kore-sql --release --no-fail-fast` run (30 binaries) passed except my own first-draft
`like_robust` expectation (a miscounted row, fixed), then `kore-sql-v2` and `kore-subquery` passed. The debug-profile `cargo test -p kore-sql`
was started but the 900 s timeout cut it at `differential.rs` (cliff_window alone is 275 s in debug); every binary before that passed
(lib 141, cliff_join 4, cliff_window 5, date_accel 1, deep_input 4).

SQLite oracle gate (`kore-sql/tests/oracle/run_gate.sh`, reduced: 30 small seeds, 3 mid seeds, threshold sizes off, tests skipped):
`GATE PASS diff=0 oracle_mismatch=0 oracle_kore_err=0` (4,500 small + 300 mid fuzzed queries compared with SQLite, 26 skipped as unsupported by the oracle, 6 min).
