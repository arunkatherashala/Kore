# World v2: design

A larger and more varied sandbox for the being (`kore-self`, Digital Life). It exists so that later research steps have a test bed in which learning from experience has more to do than in the first world (four or seven domains, one hidden profile). It is a simulation written by one author: the families, the hidden rules, the rewards, the noise and the drift are all author-defined. Measured results are in `docs/measured.md`, section "World v2".

## Opt-in and isolation

* Code: `kore-self/src/being/world2/` (new files only), tools in `kore-self/src/being_v2_tools.rs`, measurement in `kore-self/examples/measure_world_v2.rs`. The only edits to existing files are additive registration lines (`being/mod.rs`, `main.rs`, `log_tools.rs`, and one entry `world_v2_run` in `authorize::INTERNAL_TOOLS`).
* Log source `being:world2`. The original state fold, loop and replay read only `being:loop` (and `advisor`), so a World v2 being in the same log changes nothing for the original being; a test runs the original being with and without a World v2 being in its log and compares its rows, state and replay.
* The first world (`being/world.rs`) and every existing measurement are untouched.

## The world

A world is a pure function of `W2Cfg` (all integers, logged in the genesis event):

| parameter | meaning | range |
|---|---|---|
| `seed` | world seed | any u64 |
| `families` | number of task families | 1..=48 |
| `budget` | energy units of one life | 1..=1 000 000 |
| `noise_pm` | feedback noise, per mille; a family's rate is 0, 1 or 2 times this (hidden multiplier) | 0..=400 |
| `drift_per_10k` | expected hidden changes per 10 000 energy units | 0..=200 |
| `curve_pct` | difficulty curve: 0 flat, 100 = difficulty rises by up to 3 levels over the budget | 0..=100 |
| `menu` | offers per step | 2..=8 |

Family `i` has the type `TYPE_CYCLE[i % 6]` = seq, grid, knap, formula, seq, compose (so a third of the families are sequences). Each family has a hidden regime drawn from the seed: reward scale (0.6..1.6), base cost (2..6), difficulty mix, and the type's hidden rule; plus a hidden slip rate (3..10%) and feedback-noise multiplier.

| type | task | hidden rule (can drift) | exact checker |
|---|---|---|---|
| seq | next term of an integer sequence | one of 12 classes: arithmetic, geometric, quadratic, Fibonacci-like, affine recurrence, cubic, second-order linear recurrence, tribonacci, period 3, interleaved steps, and two negative controls (consecutive primes, an LCG mod 101) | the generator's next term |
| grid | shortest path length, top-left to bottom-right, in an n x n grid with walls (n = 3 + difficulty), reachable with 4-neighbour moves by construction | move rule (4 or 8 neighbours), wall density | breadth-first search under the hidden move rule |
| knap | best value of items (2 + difficulty items) under a capacity | none (pure strategy choice) | exact dynamic programme |
| formula | fall distance g t^2 / 2, or weight m g | g in {6, 8, 10, 12, 14}; textbook 10 | the formula with the hidden g |
| compose | the term h = 2..4 places after the prefix, for a sequence from the rule of another (seq) family | the base family's rule | the generator, h terms on |

Menu: each step offers `menu` tasks, the family of each slot drawn from the seed; listed reward = round(scale x (4 + 3 x difficulty)), listed cost = base cost + difficulty; skipping the whole menu costs 1 energy. World time advances 30 s per energy unit.

Feedback noise: with the family's noise rate, the being is told the opposite of the checker's verdict and shown a revealed answer consistent with the lie. The world pays, and the measurement scores, the true verdict; the being's log holds only what it was told.

Drift: change times are drawn from the seed (gaps uniform in 0.5..1.5 times the mean gap), up to 4 x the budget; each change picks a family and re-draws its whole regime (profile and hidden rule). A compose family follows its base family's rule. The being is never told.

## The being's means

The four strategies of the menu world, with the same energy factors (fast 1/2, careful 1, deep 2, discovered 1) and slip factors (3x, 1x, 1/2x, 1x):

| type | fast | careful | deep | discovered |
|---|---|---|---|---|
| seq | arithmetic check, else last difference | the B1 careful detector (arithmetic, geometric, quadratic, Fibonacci-like) | careful + cubic, period 2/3, interleaved | the being's own accepted rule for (family, difficulty), else careful |
| compose | each of the left, iterated h times | | | the base family's rule, iterated |
| grid | Manhattan distance | BFS, 4 moves | BFS, 8 moves | careful |
| knap | greedy | greedy or best single item | exact DP | careful |
| formula | textbook g = 10 | same | same | careful |

So: which strategy is right depends on hidden rules (grid move rule, sequence class), affine recurrences, second-order recurrences and tribonacci need a discovered rule, formula families with g != 10 cannot be solved by any strategy the being has, and the negative-control classes cannot be solved at all.

## The learner (existing, not new)

* Model: the B1 world model (`being/model.rs`, mode off) with the domain index replaced by the family index (`world2/model.rs`). A test feeds identical observations to both and compares every estimate bit for bit.
* Planner: the B1 planner (`being/planner.rs`, beta 1.5, skip fraction 1, no advisor) over (offer, strategy) (`world2/learner.rs`). A test compares 1500 consecutive decisions with `ModelChooser` on the same observations.
* Rule search: the B2 search (`being/discover.rs`) over the revealed examples of seq tasks, one signature per (family, difficulty), a pass every 10 steps; with the controller's composition switch, the B5 composed search. Accepting a rule resets the `discovered` cells of its (family, difficulty) and of the compose families that reuse that family.

## Log, controller, kill switch

Per step: a decision event (cites the genesis event, the last 5 outcomes and the last 5 outcomes of the chosen (family, strategy)), an action event, an outcome event (reported result, reward, cost, revealed answer), a `decision` revision whose causes are those cited events and whose rationale lists every alternative with its estimate, and a prediction with its resolved outcome. Rules: a note plus a revision citing the examples. Refusals of the controller (`max_live_rules`, `max_search_candidates`, `max_building_blocks`, `max_energy_per_run`) and kill or energy stops are notes with the same tags as in B5.

Tool `being_run_v2`: refused (and audited) if `controller.txt` is missing or unparseable, refused (and audited) if the `KILL` file exists, then asked through `authorize()` as the internal tool `world_v2_run` (tier L2, default deny: the owner lists it under `[L2]` in `autonomy_policy.txt`), then run with the controller's caps on steps and energy, the KILL check before every step, and every refusal audited. `being_status_v2` folds the state from the log and replays every decision and rule.

Replay: `replay_decisions2` re-derives every learner decision from the events before it and the offers rebuilt from the logged config; `rederive_rules2` re-runs the search over the examples logged before each rule.

## Reference policies

random (uniform over affordable (offer, fixed strategy)); always-fast / always-careful / always-deep (best listed reward per listed cost); UCB1 over (family, strategy) arms of the three fixed strategies (reported reward per energy, `EFF_NORM` normalisation, untried first); oracle-planner (knows the true success rate of each fixed strategy per (family, difficulty) in the current regime, 200 samples per cell and regime, and skips with the learner's rule on its expected rate); oracle-knows-rules (as oracle-planner, and can answer with the checker's own answer at the listed energy with careful slips: what a being that had learned every hidden rule and constant could do). The oracles are strong references, not proven optima (the skip rule is a heuristic).

## Known limits of the design

Author-defined families and grammar; the B2 grammar covers 10 of the 12 sequence classes by construction; feedback noise is independent per attempt; drift re-draws a whole regime at once (no gradual drift); the formula constant is a single integer; grid and knapsack have no learnable rule beyond strategy choice; no real data, no language model.

## Track B7 switches (opt-in)

`world2/rules_use.rs` adds five learner switches, all off by default and logged in the genesis event (`w2_use`) when on: a rule-record estimate for `discovered` (A), no `discovered` arm where no rule applies (A2), noise-robust acceptance (B), fitted formula constants (C) and fitted grid move rules (G). With all off every log and measurement is byte-identical to the design above. Results: `docs/measured.md`, section "Track B7".
