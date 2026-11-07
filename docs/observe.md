# Observing the World v2 being (Track B10)

The observer turns the being's own log into a readable report of what it did over time. It reads ONLY the log
(the events and revisions of `<data root>/<owner>/log`): no world, no hidden parameter, no measurement record. It
writes nothing to the log. Code: `kore-self/src/being/world2/observe.rs` (pure functions over log rows).

Everything it shows is a record of a simulation written by one author (World v2: author-defined task families,
rules, noise, drift and rewards). Rewards are the rewards the being was TOLD (feedback noise included), not the
checker's truth.

## Two ways to run it

**MCP tool `being_observe`** (read-only, like `being_status_v2`):

```json
{ "name": "being_observe", "arguments": { "window": 100, "max_items": 20, "format": "markdown" } }
```

* `window`: steps per window (0 or absent: about a tenth of the life, at least 25 steps).
* `max_items`: how many experiments, plans, model changes, refusals and stops are listed in full (default 20).
* `format`: `markdown` (default) or `json` (the same numbers, machine-readable).

A chat thinker can only call it through `authorize()`: it is a built-in L1 (read-only) tool, so the owner has to
list it under `[L1]` in `autonomy_policy.txt`; otherwise it is denied and the denial is audited. Like
`being_status`, it is still allowed while the `KILL` file exists (it only reads).

**Command** (the same report from a terminal):

```
cargo run -p kore-self --example being_observe --offline --release -- --root <data root> --owner <owner> \
    [--window 100] [--max-items 20] [--json] [--md report.md] [--html report.html]
```

`--md` / `--html` also write the report to that file (the HTML file is the markdown in a `pre` block). To try it
on a fresh directory, `--demo <empty directory> [--seed N]` first runs one all-skills World v2 life (12 families,
budget 3000, noise 100 per mille, strong drift, every skill allowed except lookahead) into
`<directory>/demo/log`, then reports on it. Never point `--demo` at a real owner directory.

## What the report contains

1. **Header**: the genesis note (world config and the learner's switches), every `skills_profile` note (what the
   all-skills profile turned on and what the controller refused), every `experiment_cfg` note, totals and the
   number of decisions per policy (`w2-model-ucb` = the learned planner, `w2-experiment`, `w2-lookahead-dN`).
2. **Over time**, one row per window of steps: attempts (reported right), skips, reported reward per energy, the
   being's own estimated regret, attempts per task type and per strategy, experiments, lookahead decisions, rules
   accepted / retired, answers given by a rule (reported right), model changes (B3b), controller refusals, stops.
   *Own regret* is the mean over decisions of (the best mean reward per energy among the affordable alternatives
   the model listed in the decision's rationale minus the chosen one): what the being itself believed it gave
   up. The true regret needs the hidden world and is not in the log; it is only measured in `docs/measured.md`.
3. **Choices per task type and strategy** over the whole log: attempts, reported right, reported reward per energy.
4. **Rules**: every sequence rule and fitted constant with the step it was accepted, its evidence (training and
   held-out examples reproduced, missed examples under robust acceptance, examples cited), how often it answered
   and was reported right, which experiments supplied its examples (`experiment_credit` revisions), and, if it
   was retired, when and why (window right / size, upper bound) and how many failing outcomes the retirement cites.
5. **Experiments**: what each predicted (hypotheses, expected bits, support gain), what was revealed, which
   hypotheses it kept and rejected.
6. **Plans of the lookahead planner**: each lookahead decision with its depth, nodes, score and future term.
7. **Model changes**: each B3b switch or prior conflict with its statistic.
8. **Controller refusals** (cap, limit, requested, what), **stops** (kill switch, energy cap) and the **audit rows**
   of the tool layer (refused runs, the kill switch before a run).

## Sample (from `--demo`, seed 2, window 200, abridged)

```
Steps: 980. Reported reward 7471 for 2997 energy (2.493 per energy; reported = what the being was told, feedback
noise included). Decisions by policy: w2-experiment 27, w2-model-ucb 953.

| steps | attempts (reported right) | skips | reward / energy | own regret | ... | experiments | rules +/- | rule answers (right) | model changes | refusals |
| 0-199   | 199 (124) |   1 | 2.568 | 0.175 | ... | 27 | +9 / -0 |  2 (2)  | 1 | 2 |
| 200-399 | 136 (114) |  64 | 3.629 | 0.036 | ... |  0 | +0 / -0 |  0 (0)  | 1 | 0 |
| 400-599 | 114 (60)  |  86 | 2.029 | 0.064 | ... |  0 | +5 / -0 | 15 (11) | 0 | 0 |
| 600-799 |  80 (55)  | 120 | 1.953 | 0.104 | ... |  0 | +1 / -1 | 59 (52) | 2 | 0 |
| 800-999 |  75 (62)  | 105 | 2.132 | 0.090 | ... |  0 | +1 / -1 | 69 (61) | 2 | 0 |

| rule | kind | what | accepted at step | evidence | answers (reported right) | experiment credit | retired |
| ede044a73c63 | fitted constant | f09 g/move rule = 10 | 120 | 4 training + 2 held-out examples reproduced, 1 missed, 6 examples cited | 5 (2) | - | at step 650: right on 0 of the last 6 revealed answers where it applies (95% upper bound 0.390); cites 6 failing outcomes |
| 4870af4d22cc | fitted constant | f09 g/move rule = 12 | 800 | 4 training + 2 held-out examples reproduced, 2 missed, 6 examples cited | 22 (19) | - | - |

- step 151: [max_experiment_energy_pm] controller refusal at step 151: an experiment was wanted at step 150 but the
  experiment budget is used up (27 experiments, 150 energy units so far); the planner's choice was kept
```

Here the formula family f09 changed its hidden constant (drift); the being retired g = 10 at step 650 after six
contradicting answers and fitted g = 12 from the answers that followed (step 800).

## Limits

* It reports what is in the log. A run made by an older binary has no profile, retirement or change notes; those
  sections are then empty.
* World v2 logs only (source `being:world2`); the first world's being has `being_status` and `being_explain`.
* Lookahead plans are shown as logged; their decisions are not re-derived (as in Track B8).
* The windows are by step count, not by energy.
