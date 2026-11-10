# Digital Life milestones

Every milestone is a measured property of the being in an author-defined world. The protocol is the usual one:
- tuned on seeds 101..120 and reported on report seeds fixed before the run;
- paired 95% t-intervals with wins/ties/losses;
- criteria pre-registered.

None of these milestones is a claim of thinking, understanding or life.

Status words:
- **met**: the pre-registered interval is entirely on the wanted side, on report seeds, at a named commit.
- **not met**: the interval is entirely on the wrong side.
- **not established**: the interval includes 0, or the result is too wide to say either way.
- **met (by tests)**: a property checked by tests, not by a measured comparison.

| Milestone | Measured as | Status | Evidence |
|---|---|---|---|
| M1 No plateau below the ceiling | First world, long run of the B14 being: extended families, 200,000 steps, windows of 10,000, report seeds 1..20. Either (a) the least-squares slope of windowed reward per energy over steps 50k..200k, minus the baseline's slope, has a paired 95% CI above 0; or (b) the gap to the true-parameter oracle (see the note on M1) over steps 50k..200k has a 95% CI upper bound below X% of the oracle's rate. X is to be fixed before Track N1. | (a) not established. (b) open: X is not set. | (a) B14 slope minus base: −0.008 [−0.026, +0.009] (7/0/13). E1 slope minus B14: −0.010 [−0.039, +0.018] (10/1/9). (b) B14 regret vs the oracle with book: 0.085 [−0.003, +0.173] per energy, about 2.0% [−0.1%, +4.0%] of the oracle's rate. |
| M2 Learning to learn | World O, default configuration, life 1,200,000, report seeds 1..20. PRIMARY: attempts to competence of the last third of a world's learnable top-tier families (families 32+) minus the first third, paired within world, 95% CI below 0. SECONDARY: top-tier Spearman of family order vs attempts, below 0. Minimum: at least 30 learnable top-tier families per world (10 per third); fewer and the world is excluded. | PRIMARY: not established. SECONDARY: met, by B12 `form`. | PRIMARY: `form` −1.08 [−5.11, +2.96]; `form+l2` +0.06 [−4.05, +4.18]. Both are wide, about ±4 attempts on a mean of about 23. SECONDARY: `form` −0.098 [−0.171, −0.026], mostly an immediate gain (level 2 over level 1: −0.014). Tracks B12 and L2. |
| M3 Real data | A World T-like world built from a real public dataset, for example the NYC taxi file, with exact checkers. The being beats tuned LinUCB, UCB1 and always-X, paired CI above 0. | not started | World T uses synthetic data only. The taxi file has been used only for the file format, never as a world. A design is needed first: dataset, tasks and checkers. |
| M4 Not dominated on reward and wrong answers | World T sweep (6 × 5 points, every policy re-tuned at every point). At every point, no baseline has both higher reward per energy (CI above 0) and fewer wrong answers per 100 tasks than the being. In other words, the being is on the Pareto front of (reward per energy, wrong answers). | not met | Penalty 10 × reward, energy weight 1: LinUCB has more reward (being −0.104 [−0.165, −0.043]) and fewer wrong answers (0.7 vs 1.6 per 100). Always SQL gives 0 wrong answers but is behind on reward at 29 of 30 points. Wrong answers per point for every baseline are not yet tabulated. |
| M5 Explainable and replayable | Every decision cites its evidence in the log. Runs replay exactly from the log. Tampering is detected. | met (by tests) | Replay, fold and tamper tests in every track. Full suite at 4a2f9126: kore-self 617/0, kore-store 63/0. |
| M6 Safe under the controller | Every new capability is off by default, behind a fail-closed controller key. KILL stops runs. Refusals are logged before the genesis. | met (by tests) | Controller tests in every track. The all-keys coexistence test (B14, E1, L2) at 4a2f9126. |

## Notes

### M1
- A flat curve is a problem only below the ceiling.
- The ceiling used here is the B14 "oracle with book": a renewal oracle that knows the hidden success rates and the real energy, and holds the author's rule book (60 sampled tasks per hidden family).
- Its regret of 0.085 suggests the first-world long run may already be within a few percent of that ceiling. If so, Track N1 has little headroom in the first world. The N1 step 0 diagnosis must answer this before X is set.
- This oracle is not the World T "reward-best oracle" (−0.686 at the World T headline). That one sees every plan's realized result, and the two numbers are not comparable.

### M2
- The primary is the thirds statistic, not the Spearman. B12 `form` already gives a negative Spearman, but almost all of it is an immediate gain at the first top-tier families, not faster learning later. The thirds statistic measures what is wanted: later families are learned faster.
- The current intervals are about ±4 attempts on a mean of about 23, so an effect of about 15% either way cannot be ruled out.
- More report seeds, with the count fixed by a power analysis before the run, or a less noisy statistic would be needed.
- The per-world count of learnable top-tier families is not printed by `measure_l2 report`. About 190 top-tier families open per world (families 32 to about 223), but how many of them are learnable must be reported before the minimum count above is applied.

### M4
- The criterion is Pareto non-dominance, not "as few wrong answers as the best baseline". Always SQL answers nothing wrong, so matching its wrong-answer rate would make M4 impossible for any policy that attempts harder plans.

## Track N1 pre-registration template

Fill this in after the step 0 diagnosis. The reviewer approves it before any N1 code is written.

- **Question:** one sentence, from the diagnosis: which gap a learned world model is meant to close, and in which world.
- **Hypothesis:** the mechanism: what the model predicts, how the planner uses it, and why that should close the gap.
- **Headroom (from step 0):**
  - gap to the true-parameter oracle: X [CI];
  - irreducible part: Y;
  - share that N1 could close: Z.
- **Candidate:** learner name, switch, controller key (off by default).
- **Baselines:**
  - the current recommended being;
  - the same being with the model replaced by its prior (the ablation);
  - tuned LinUCB and UCB1 where applicable;
  - the true-parameter oracle as the ceiling.
- **Negative-control worlds:** worlds where a world model cannot help, for example i.i.d. outcomes with no structure to learn. The candidate must show no gain there; a gain there means an artifact.
- **World and configuration:** world, budget or steps, windows, controller.
- **Seeds:**
  - tune on 101..120;
  - report on 1..N, with N from the step 0 power analysis;
  - fixed now and never increased after a report run.
- **Power analysis:**
  - the assumed effect size, with its basis (headroom Z, tuning-seed estimate);
  - the paired standard deviation from the tuning seeds;
  - the resulting N at 80% power, two-sided α = 0.05.
- **PRIMARY:** one statistic, tied to M1 or M2, with the paired 95% CI direction, the threshold and the minimum wins. Only the PRIMARY decides the result.
- **SECONDARY** (all reported; no win is claimed on a secondary):
  - (a) model calibration: Brier score or log loss of its predictions on held-out steps, against the prior;
  - (b) wrong answers per 100;
  - (c) compute cost: simulated energy share and wall time, reported separately;
  - (d) no regression on the other worlds, by construction or measured.
- **ATTRIBUTION:** candidate minus the ablation (model off, everything else equal), paired CI.
- **Stop rule:**
  - one report run;
  - if the PRIMARY is not met, the result is recorded as negative and the code stays off by default;
  - no constant is re-tuned on report seeds.
- **If it fails:** the next track the diagnosis points to, for example N2 sleep/replay.
