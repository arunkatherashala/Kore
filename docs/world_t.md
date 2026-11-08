# World T: a sandbox world of data work (Track W1, design)

An opt-in sandbox for the being (crate `kore-self`, module `being/world_t/`) in which the tasks are small pieces
of data work on realistic-looking files: answer a question about a table, find a value, say what format a file
has, predict the next value of a time series, find the rule behind a derived column, find the malformed line of
a file, count errors in a log. Each task has an exact checker. The being's actions are tools; it learns from its
own log which tool or plan works for which kind of task, from visible features only.

It is a simulation written by one author: the data generator, its quirks, the rules, the rewards and the tools
are all author-defined. No real data, no personal data, no network, nothing read from the user's files. Measured
results are in `docs/measured.md`, section "Track W1".

## Opt-in and isolation

* Code: `kore-self/src/being/world_t/` (new files only: `data.rs` the generator, `world.rs` the pool, the task
  kinds and the checkers, `tools.rs` the being's tools, `learner.rs`, `engine.rs` the logged loop, fold and
  replay, `ctl.rs` the controller keys, `baselines.rs` and `measure.rs` for the measurement), the tool layer
  `kore-self/src/world_t_sandbox.rs` (the sandbox directory) and `kore-self/src/world_t_tools.rs` (the gate),
  the harness `kore-self/examples/measure_world_t.rs`.
* Edits to shared files (one line each): `being/mod.rs` (module line), `being/controller.rs` (the World T keys
  are validated, read by `world_t::ctl`), `authorize.rs` (internal tool `world_t_run`, tier L2), `main.rs` (two
  module lines). No engine crate, no KORE file format, no kore-store code was changed.
* Log source `being:worldt`. The other worlds' folds read only their own sources; a test puts a World T being and
  a World O being in one log and checks that each folds to exactly its own state.
* With World T off, the first world, World v2 and World O are byte-identical: `world_t/tests_golden.rs` pins the
  digests of a logged run of each (and of the controller's parse of a full file without World T keys), computed
  at 8591da53 with only the module line added, before any other shared edit.
* There is no MCP tool: the run is in-process (`world_t_tools::run_world_t`, like World O).

## Controller, authorize, kill switch

A run is refused (and the refusal audited, source `authorize`) unless, in this order:

1. `controller.txt` exists and parses (missing or invalid: fail closed) and contains `world_t_run = on`
   (default off);
2. the `KILL` file does not exist (it is also checked before every step: `Policy::gate_cost`);
3. `authorize()` allows the internal tool `world_t_run` (tier L2: it appends to the being's own log and reads
   its sandbox; the owner lists it under `[L2]` in `autonomy_policy.txt`; default deny).

World T has no L1 or L3 action of its own: nothing it does leaves the sandbox and the log, so there is nothing
that would need a human's approval. The run obeys `max_steps_per_run`, `max_energy_per_run`, `max_live_rules`
and `max_search_candidates`; every cut or refusal is a `controller_refusal` note in the being's log and an audit
row.

### The SQL tool: read-only, timeout, row limit

Every query the being generates goes through `tools::guarded_sql`:

* it must pass `versioned::sql::check_readonly` (one SELECT/WITH statement; INSERT, UPDATE, DELETE, DROP,
  CREATE, several statements, ... are refused before the engine sees them, cap `sql_read_only`);
* it runs in a fresh `KqlContext` holding only an in-memory copy of the table the loader built from the file
  (`t`); nothing else is registered, so nothing else can be read or changed;
* timeout: controller key `world_t_sql_timeout_ms` (default 2000, hard maximum 60000); a query that took that
  long or longer has its result discarded and is refused (cap `sql_timeout_ms`); 0 refuses every query. The
  being has no threads (a source-scan test forbids them), so the engine call is not interrupted: the limit is
  enforced on the result;
* row limit: `world_t_sql_max_rows` (default 1000, hard maximum 1000000); a larger result is discarded and
  refused (cap `sql_max_rows`);
* fail closed: a missing or invalid `controller.txt` gives timeout 0 and 0 rows (no query runs; the run itself
  is refused anyway); a value above the hard maximum or not a number makes the whole file invalid.

Logged for every query: the action event's text has the SQL text, the limits, what happened and the result
digest (first 12 hex digits of sha256 over the result rows); its tags have `sql_timeout_ms`, `sql_max_rows`,
`sql_status` and `sql_digest`; the outcome event of the same step repeats `sql_status` and `sql_digest` next to
the checker's verdict (`result`). Each refusal is also a `controller_refusal` note (with the query text).

## The world

A world is a pure function of `WTCfg` (logged in the genesis event, tags `wt_*`): `seed`, `budget` (energy of the
life, default 6000), `start_kinds` (3), `intro_every` (budget / 10: energy between two newly opened kinds),
`files` (6 per context and pool), `ood` (off; see "Out of distribution").

The sandbox holds a pool of about 200 files per world, written to `<data root>/<owner>/world_t/seed_<seed>/`
through `safety::safe_path` (plain file names only; no `..`, no absolute paths, no symlink escape; each file is read
from disk once per run and then kept in memory, the pool does not change during a run) and read back by
the engine through the `Sandbox` trait. The `being/` code itself has no file access (a source-scan test).

Contexts (visible: in the file name and the task): six table contexts (`sales`, `orders`, `payments`,
`customers` as `.csv`, `inventory` as `.tsv`, `shipments` as pipe-delimited `.txt`), two derived-column contexts
(`invoices`, `payroll`), three exact series (`meter`, `cumsales`, `shifts`), two noisy series (`sensor` hourly,
`visits` daily), two log contexts (`applog`, `workerlog`).

Quirks (hidden; per context a chance drawn from the seed inside an author-defined range, then per file): a `;`
delimiter instead of `,` (legacy exports), decimal commas, quoted thousands separators (`"1,234.50"`), null
tokens (`""`, `NA`, `NULL`, `-`; each context mostly one), CRLF line ends, a byte-order mark, padding after the
delimiter, quoted text fields that contain the delimiter (`"Smith, A."`), skewed categories (Zipf-like),
log-normal money values, missing values. Logs: lower-case levels in some files, the word ERROR inside INFO
messages, stack traces after errors.

| kind | templates (visible) | checker | plans |
|---|---|---|---|
| colstat | total, maximum, mean (nulls ignored), row count, distinct values of a column | exact (money in cents; the mean accepts either rounding of an exact half cent) | naive+native, sniff+native, naive+sql, sniff+sql |
| group | total where a category = v, the category with the largest total, rows with a count above k | exact | naive+sql, sniff+sql (the native statistics have no filters or groups) |
| lookup | the value of a column in the first row with key K | exact | the four table plans |
| ts_exact | next value of an integer series from its first k values (k = 16..29 of 30) | exact | last, drift, season7, own rule |
| ts_noisy | next value within +-3% of the last value (k = 50..119 of 120) | tolerance | last, drift, season7, season24, mean of 7, own rule |
| derived | a missing value of a derived column (4 blanks per file) | exact | product of two columns, fitted ratio, verified column-rule search |
| fix_row | the 1-based line of the one malformed line (extra field, missing field, bad number, impossible date) | exact | field-count check, type check |
| format | delimiter and header / no header | exact | the extension's delimiter (header assumed), sniffer |
| logs | ERROR entries of a service; the service with most ERROR entries | exact | substring count, structured parse + native count (count only), parse + SQL |

Hidden rules: exact series are linear, quadratic, cubic, alternating (period 2 with growth) or weekly (period 7 with
growth; lag 7 is outside the B2 grammar); derived columns are `a*b`, `a*b + fee`, a discount, a rate (VAT),
discount and fee, discount and rate, or tiered overtime (outside the column grammar); the mix differs by context.

One task per step: the kind is drawn uniformly among the open kinds, then template, file and parameter. The kinds
open in a seed-shuffled order: 3 at the start, then one more every `intro_every` energy (the being is told, a
`wt_kind` note). Listed reward per template 4 to 9, plus 2 per size class; listed energy per plan grows with the
rows (e.g. naive+native 1 + rows/300, sniff+sql 3 + rows/150); a skip costs 1. World time is 30 s per energy unit.
For series the sandbox shows the tools only the first k lines (the task's "as of" view), and after an attempt
the true next value is revealed (an example for the rule search).

## The being's tools

* Loaders: `naive` splits on commas and newlines and nothing else; `sniff` strips a byte-order mark and CRLF,
  detects the delimiter (the one with a constant field count above 1 on the first lines), honours quotes, trims
  padding, maps the null tokens, and reads thousands separators and decimal commas.
* Executors: `native` (sum, maximum, mean, count, distinct, key lookup) and `sql` (the KORE SQL engine, read-only,
  see above; the being writes the query from the template: no language model).
* Series: last value, drift (last + last change), seasonal with a level shift (`x[-k] + mean(last k) - mean(the k
  before)`, k = 7 or 24), mean of the last 7, and `ts-rule`: the being's own rules, the B2 search
  (`being/discover.rs`, unchanged) over its revealed series examples (one signature per context; a pass every
  10 steps; duplicate examples count once).
* Derived columns: a B2-style verified search over a 6-form column grammar (one constant fitted on the first known
  row, every known row must be reproduced exactly, at least 3 training and 1 held-out row; never wrong on the
  file). Its energy is 2 + half the forms tried; forms the being has accepted more often in that context (counted
  from its log) are tried first, so it gets cheaper with experience.
* Fix: field count against the header (quote-aware); type check (column types by majority, then the first line
  whose field count or a value breaks them).
* Format: the extension's delimiter, or the sniffer plus a header test (first line without numbers, second with).
* Logs: substring count of " ERROR " and "[service]"; structured parse (level case-insensitive, continuation
  lines skipped) with a native count; the same parse into a table plus SQL.

## The learner

Per (template, context, plan) a Beta estimate of the chance that the plan's answer is right, folded from the
outcome events; the prior is pooled from what it has seen elsewhere: the same context and plan in other
templates if it has 3 attempts there, else the same template and plan in other contexts, else the plan anywhere,
else 0.5 (`wt_prior:pooled`; `flat` = always 0.5, the ablation). A series rule (`ts-rule`) instead starts from its
own verification record: (support + 1) / (support + 2) of its best live rule for the signature (the B7 "rule
record" idea). Without that, and with applying a rule priced at 2 energy, the being never tried its 7 rules in a
test life (a 0.5 prior at double energy never beat a known cheap plan; the same under-use Track B7 found); applying
a rule now costs 1 energy like the other series tools. Score of a plan: `min(1, p + beta x sd) x reward /
energy` (the B1 planner's rule; the energy of the column search is its observed mean); it takes the best and
skips when that score is below `rate x (E - 1) / E` (rate = the reward per energy realized so far; after 40
energy). `ts-rule` is an option only when the being holds a rule for the series' signature. Constants were tuned on seeds 101..120 only: beta 2.0 and prior weight 2
(first pass: beta 0.5 / 1 / 2 x weight 1 / 2 / 4, beta 2.0 at the edge; second pass: beta 0.5 to 16, beta 2.0 inside;
see "Tuning separation and budget"). They are the defaults of `LearnerTCfg`.

## Log, fold, replay

Per step: the `wt_kind` notes, a `decision` event (the task, the choice, the reason, p, sd, energy, score, the
number of cited events) with a revision whose causes are the genesis, the last 5 outcomes and the last 5 outcomes
of the chosen cell, and whose rationale lists every option with its attempts, prior (and where it came from), p,
sd, energy and score; an `action` event (loader, delimiter, rows, the SQL text and its digest, the rule or form
used, the answer); an `outcome` event (the checker's verdict, reward, energy, budget, clock, the revealed next
value and the terms for series, the accepted column form); a prediction with its resolved outcome. Rules: a note
plus a revision citing the examples. `StateT::from_events` folds the state; `replay_t` re-derives every decision
from the events before it and the world rebuilt from the logged config; tests check fold = live state, every
decision replays, a run of 173 + 227 steps with a cold restart equals one run of 400, KILL before the first step
and after 12 steps (stop logged), and the caps.

## Reference policies (measurement only)

random plan (uniform over the template's fixed plans, no skip); always SQL (sniff+sql or log-sql where the
template has one, else skip); a UCB1 bandit over plans (arms per (template, context, plan), reward per energy scaled
to [0, 1], shifted when wrong answers are penalized; optional skip; tuned); LinUCB (one ridge model per plan over
one-hot bias, template, context, size class and template x context, success as the target, plus a second target
"the answer was wrong" on the same design matrix for the penalty; score (UCB of success x reward - mean wrong x
penalty) / energy; optional skip; tuned). The references have no rule book of their own (`ts-rule` is the being's).

Two oracles, both SEE the true result of every plan on every task (and whether a perfect series rule would answer
it):

* the **reward-best oracle**, the yardstick of the gap: on each task the option, skip included, of highest true
  reward minus rho x energy (the penalty of a wrong answer included; a skip is worth -rho), with rho its own realized
  reward per energy so far (the renewal comparison of the World O oracle-planner); a strong reference, not a proven
  optimum;
* the **answer-everything oracle** (the reference of the first measurement): answers every task some plan answers
  right, with the right plan of highest reward per energy, and skips only when no plan is right. It never skips a task
  it can answer however expensive, so a policy that skips well can beat it; no claim is based on it.

## Reward design (Track W1 follow-up)

Two public, logged parameters of the world (tags only when not the default, so earlier logs keep their meaning):
`penalty_pm`, the reward of a wrong answer is minus this share of the task's listed reward (default 0: a wrong answer
earns 0); `cost_pm`, a plan's energy is `1 + weight x (base energy - 1)` (default weight 1; a skip always costs 1).
The being estimates per cell also the chance that a plan's answer is wrong (pooled like its right-answer prior; flat
0.25) and scores `(min(1, p_right + beta x sd) x reward - p_wrong x penalty) / energy`. With the default design every
decision is the one of the first measurement (the sweep's default point reproduces its numbers exactly).

Track W1b (2026-11-08, `value_ucb`, default on): that score put the optimism bonus on the right-answer chance only
(scaled by the reward) and used a point estimate for the penalty term, so with a harsh penalty an untried plan
scored `(0.5 + bonus) x reward - 0.25 x penalty` < 0 and lost to the skip bar for good; skipped tasks give no
outcomes, so the estimate never moved. Now the bonus is on the expected reward of the whole outcome (right +reward,
wrong -penalty, no answer 0; Dirichlet posterior with the same counts and prior): `min(reward, p_right x reward -
p_wrong x penalty + beta x sd) / energy`, `sd^2 = (p_r(1-p_r) R^2 + p_w(1-p_w) P^2 + 2 p_r p_w R P) / (w + n + 1)`. With
no penalty this is exactly the old score (a test runs both rules and compares every step). Logs without the
`wt_ucb` tag replay with the old rule. Measured in docs/measured.md, Track W1b.

## Tuning separation and budget

Seeds 101..120 are used only for tuning, seeds 1..20 only for reporting; budget 6000 in both. At every point of the
reward-design grid every tuned policy gets the same budget: 6 configurations, each run on the 20 tuning seeds, the
objective (mean reward per energy) fixed in advance. Being: beta 0.5, 1, 2, 4, 8, 16 (prior weight 2, pooled, skip
on). UCB1: c 0.001, 0.003, 0.01, 0.03, 0.1, 0.3 (arms per template x context, skip on). LinUCB: alpha 0.1, 0.3, 1, 3,
10, 30 (skip on). The first measurement had ended at grid edges (being beta 2 of 0.5 / 1 / 2; UCB1 c 0.03 of 0.03 to
1); the grids above extend both, and at the default point the optimum is inside (beta 2, c 0.003, alpha 1). Elsewhere
in the grid UCB1 still chose its lowest c at 15 of 30 points, and the being and LinUCB their edge at 1 point each.

## Skips and coverage

A skip costs 1 energy and earns nothing. The measurement reports, per policy, coverage (tasks answered / tasks
met), the skip share and the wrong answers per 100 tasks, and runs skip-off variants of the being, UCB1 and LinUCB
(the same constants, skip disabled) to see how much of a lead comes from skipping.

## Out of distribution (never used in tuning)

`ood = true` (tag `wt_ood:1`): every table has 1000 to 2500 rows (size class 3, never seen otherwise) with its
columns in reverse order; exact series are Fibonacci-like, doubling-plus-constant (both in the B2 grammar, neither
in the default world) or weekly; derived columns use fees and rates the default world never uses (1234, 777;
113%, 115%, 93%), discounts on payroll, and tiered rules on invoices.

## Engine needs

What the KORE SQL engine (`kore-sql`, `KqlContext`) lacks or does differently from standard SQL, found while
building the tasks. World T did not change the engine crates. Items 1, 2 and 5 were later fixed in this copy of the
engine (branch `dl-sql-fix`, a targeted fix that gives these three cases the NULL semantics of `master-kore-engine`;
tests in `kore-sql/tests/null_semantics.rs`). Each item, fixed or not, is pinned by `world_t/tests_engine.rs`.

1. Fixed. `COUNT(column)` counted NULLs (`COUNT(x)` over 1, NULL, 3, 4 gave 4; standard SQL gives 3); it now gives
   3, and `COUNT(*)` still counts rows. `AVG` ignores NULLs. The mean task needs the non-null count; the tool used
   to work around the bug with `COUNT(*) ... WHERE x IS NOT NULL` (without the workaround the SQL mean was wrong on
   26 of 41 probe tasks with nulls) and now uses plain `SUM(x), COUNT(x)`. The report on seeds 1..20 at the default
   reward design (`report --beta 2 --ucb-c 0.003 --lin-alpha 1`) is identical before and after the fix and the
   plan change, for every policy: the two queries return the same sum and count on every table.
2. Fixed. An unknown function was not an error: it returned NULL for every row (`NO_SUCH_FN(x)`,
   `REGEXP_LIKE(g, 'a')`), so a typo in a generated query was a silent wrong answer. It is now an error when the
   query is parsed, `unknown function: <name as written>`. There is still no regular expression function
   (`REGEXP_LIKE` is one of the unknown functions), so log parsing is done before SQL.
3. No quoted identifiers (`"date"` is a lexer error). A header with a space, a byte-order mark or a reserved word
   cannot be named in a query; the tools rely on the sniffing loader to clean headers (with the naive loader and
   a byte-order mark, SQL cannot address the first column).
4. `PERCENTILE_CONT(p) WITHIN GROUP (ORDER BY x)` does not parse (`MEDIAN` works). No percentile task kind was
   added for that reason.
5. Fixed. Window functions returned NaN instead of NULL for missing values (`LAG` on the first row); they now return
   NULL (`LAG` / `LEAD` past the edge, an aggregate with no non-NULL value; `COUNT` is then 0). This copy still
   ignores window frames (`ROWS` / `RANGE`): a window aggregate covers its whole partition. The series tasks do not
   use SQL windows (the native series tools are used), so a SQL series plan was not added.
6. `ORDER BY ... DESC` puts NULL groups first (as PostgreSQL); `NULLS LAST` works. The top-group query filters
   NULLs (`WHERE x IS NOT NULL`); not a defect, noted because it changed an answer during development.
7. No CSV reader is used from the engine side: tables reach the engine only as `DataBlock`s built by the being's
   loaders (by design here: the loader quirks are the point of the world).

## Known limits

Author-defined data, quirks, rules, grammar and rewards; deterministic plan outcomes (no noise in the checker or
the tools); one task per step (no menu); the sniffing loader is right on almost every table task by construction,
so much of the table learning is "which cheap plan is good enough here"; the timeout is enforced after the query
returns, not by interrupting it; the oracles are references that see hidden results, not competitors. With a harsh
wrong-answer penalty the being's prior for the wrong-answer chance makes it over-skip (docs/measured.md, Track W1).
