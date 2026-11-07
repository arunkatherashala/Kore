# World O: an open-ended world (design)

An opt-in sandbox for the being (`kore-self`, Digital Life) in which new task families keep appearing, so that
there is always something the being has not seen yet. It is a simulation written by one author: the families, the
hidden rules, the rewards, the schedule and the drift are all author-defined. Measured results are in
`docs/measured.md`, section "Track B11".

## Opt-in and isolation

* Code: `kore-self/src/being/world_o/` (new files only: `world.rs`, `engine.rs`, `measure.rs`, `tests.rs`),
  measurement in `kore-self/examples/measure_b11.rs` (modes `worldo`, `worldo-log`). The only edits to existing
  files are registration lines in `being/mod.rs`.
* Log source `being:worldo`. The first world's fold and replay read only `being:loop`, World v2 reads only
  `being:world2`, World O reads only `being:worldo`; a test puts a World v2 being and a World O being in one log and
  checks that each folds to exactly its own live state.
* The tasks, checkers and solvers are those of World v2 (`world2/world.rs`: `seq_task`, `bfs`, `knap_exact`;
  `world2/solve.rs`: the four strategies), reused, not copied. Nothing in World v2 or the first world changed.

## The world

A world is a pure function of `WOCfg` (all integers, logged in the genesis event, tags `wo_*`):

| parameter | default | meaning |
|---|---|---|
| `seed` | | world seed |
| `budget` | | energy units of the life (no refills) |
| `menu` | 4 | offers per step (skipping a menu costs 1 energy) |
| `start` | 4 | families open at energy 0 (their retirements are staggered) |
| `intro_every` | 6000 | energy units between two new families |
| `life_intros` | 12 | life of a family in units of `intro_every` (so about 12 to 16 families are open at once) |
| `tier_every` | 8 | families per tier step |
| `drift_pm` | 250 | per mille of families whose hidden rule is re-drawn once, at mid-life (the being is not told) |
| `noise_pm` | 0 | feedback noise as in World v2 (0 in all measurements here) |

Family `j` (0, 1, 2, ...) opens at `intro(j)`, retires `life_intros x intro_every` energy units later, and lives in the
observable slot `j % 50` (two families sharing a slot never live at the same time). Its tier is
`min(4, j / tier_every)`:

| tier | family types (drawn from the seed) | what it takes |
|---|---|---|
| 0 | arithmetic sequences, knapsack, grid with 4-neighbour moves, formula with the textbook constant | a fixed strategy solves it; the being only has to learn which one is cheapest and reliable |
| 1 | geometric / quadratic / Fibonacci-like sequences, grid with a hidden move rule, knapsack, formula with a hidden constant | the right fixed strategy (careful for the sequences, deep for 8-neighbour grids, deep for the knapsack); formula families with g != 10 cannot be solved by the being's means |
| 2 | affine recurrence, second-order recurrence, tribonacci, cubic, period 3, interleaved steps | a rule discovered from the being's own examples (or deep for some) |
| 3, 4 | mostly `compose` (the term h = 2..4 places ahead, tier 4 one more, under the rule of an earlier open sequence family of tier >= 1), plus tier-2 sequences, grids, knapsacks, formulas | reusing a discovered rule of another family, iterated |

From tier 1 on, one sequence family in ten is a negative control (consecutive primes, a linear congruential
generator): nothing in the being's grammar describes it. Listed rewards grow with the tier (scale `(0.7 + 0.3 x tier)`
times a per-family factor 0.8..1.2), so a being that keeps up with the new families earns more per energy later in
its life; a being that does not, falls behind the reference. A family's difficulty rises by up to three levels over
its own life. Menus draw each slot uniformly from the open families.

Public facts (logged as `wo_family` notes before the step that follows them): a family opened (its name, slot, type
and, for `compose`, the slot whose rule it reuses) or closed. The world never shows a rule class, a constant, a move
rule, a retirement time or a drift.

## The being in World O

* The learning state is a World v2 `State2`: the B1 world model keyed by slot (`world2/model.rs`), the B2 rule book and
  its examples, the totals. Its planner is `Learner2` (the B1 planner) wrapped in `LearnerO`, which only adds the
  Track B11 skip test as a switch (`wo_skip_rule` in the genesis event; absent = the B1 test). A test checks that
  `LearnerO` with the default test returns exactly what `Learner2` returns, step for step.
* Rule search: the B2 search over the revealed examples of sequence tasks, one signature per (slot, difficulty), a
  pass every 10 steps, with the controller's caps. Accepting a rule resets the `discovered` cells of its (slot,
  difficulty) and of the open `compose` slots that reuse it.
* When a family closes, the evidence of its slot is moved into the model's archive cell (so the strategy-wide prior
  that new families start from keeps everything the being has seen), and its working examples, rules and rule
  records are set aside (the log keeps them; the slot is free for a later family). This is bookkeeping driven by the
  public announcement, not a new learning mechanism.
* The B7 switches A (rule-record estimate for `discovered`) and A2 (`discovered` only where a rule covers the offer)
  are available (logged as `w2_use`); B7 B and C (robust acceptance, fitted constants) and the B3b adaptation switches
  (which live in the first world's model, not in the family-keyed model) are not wired into World O.

## Log, controller, kill switch

Per step: the family notes, a decision event (with every alternative in its revision's rationale), an action event,
an outcome event (reported result, reward, cost, revealed answer), a `decision` revision citing its evidence, and a
prediction with its resolved outcome; rules are a note and a revision citing the examples. `run_o` runs the being on
its own log with `Policy::gate_cost` (kill switch, step cap, energy cap) checked before every step, and the
controller's caps (`max_live_rules`, `max_search_candidates`, `max_building_blocks`, `max_energy_per_run`) with every
refusal logged; a kill or energy stop is a note. `StateO::from_events` folds the state from the log; `replay_o`
re-derives every decision from the events before it and the world rebuilt from the logged config. There is no MCP
tool for World O; it is driven in-process (`measure_b11 worldo-log` runs it on a real log in chunks, each chunk a
cold restart from the log).

## Reference policies

* `oracle-planner`: knows the hidden success rate of every fixed strategy on every (family, difficulty) in the current
  regime (200 sampled tasks per cell), takes the option of highest expected reward per energy, and skips by the
  renewal comparison with its own realized rate.
* `oracle-knows-rules`: as `oracle-planner`, and may also answer with the checker's own answer at the listed energy
  with careful slips: what a being that had learned every hidden rule and constant could do. This is the yardstick
  of the regret columns. Strong references, not proven optima.

## Time to competence

For every attempt the measurement computes the ratio of the chosen option's true success per energy to the best the
being's means allow on that task (the fixed strategies, or a perfect rule of the grammar for sequence and compose
families whose hidden class the grammar describes). A family is learnable when the mean best success probability on
its attempted tasks is at least 0.5; it is reached competence after the attempt at which its last 10 attempts first
average at least 0.9. Attempts and energy are counted from the family's opening.

## Known limits of the design

Author-defined families, grammar and schedule; the types are World v2's; rewards rise with the tier by construction
(so reward per energy rising over time is not by itself learning; the regret against the reference is the measure);
a family's drift is a single re-draw; the slot is observable (the being knows when a family is new); no real data,
no language model.
