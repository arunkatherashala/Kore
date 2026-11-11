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
| M1 No plateau below the ceiling | In a world with measured headroom: either (a) the least-squares slope of windowed reward per energy, minus the baseline's, has a paired 95% CI above 0, or (b) the gap to the known-parameter (KP) reference has a 95% CI upper bound below X% of the reference rate (X fixed in the track's pre-registration). The world is World T (Track N1). The first world is not applicable: its plateau is at the ceiling (see the notes). | first world: not applicable (at the ceiling). World T: open (Track N1) | First world, 200k steps, tuning seeds (N1 step 0): rule worlds level with the KP oracle, −0.047 [−0.178, +0.083]; rule answers 0.970 right = 1 − slip rate; simple worlds +0.249 [+0.172, +0.325] left (about 6%). Earlier slope results: B14 −0.008 [−0.026, +0.009], E1 −0.010 [−0.039, +0.018]. World T: reward-best oracle − being +0.632; a file-level KP ceiling of +0.554, an upper bound, not an expectation. |
| M2 Learning to learn | World O, default configuration, life 1,200,000, report seeds 1..20. PRIMARY: attempts to competence of the last third of a world's learnable top-tier families (families 32+) minus the first third, paired within world, 95% CI below 0. SECONDARY: top-tier Spearman of family order vs attempts, below 0. Minimum: at least 30 learnable top-tier families per world (10 per third); fewer and the world is excluded. | PRIMARY: not established. SECONDARY: met, by B12 `form`. | PRIMARY: `form` −1.08 [−5.11, +2.96]; `form+l2` +0.06 [−4.05, +4.18]. Both are wide, about ±4 attempts on a mean of about 23. SECONDARY: `form` −0.098 [−0.171, −0.026], mostly an immediate gain (level 2 over level 1: −0.014). Tracks B12 and L2. |
| M3 Real data | A World T-like world built from a real public dataset, for example the NYC taxi file, with exact checkers. The being beats tuned LinUCB, UCB1 and always-X, paired CI above 0. | **not met** (2026-11-11) | World M3 built from the January 2024 yellow-taxi file (`docs/m3_design.md`), exact checkers cross-checked by DuckDB (0 disagreements on 329,470 tuning-seed tasks). Report seeds 401..460, run once: B0 minus the strongest baseline LinUCB+file -0.129 [-0.145, -0.113] (1/0/59), minus LinUCB +0.008 [-0.005, +0.021]; ahead of UCB1, random, always SQL and always cheapest on every world. PRIMARY and S1 not met (`docs/measured.md`, Track M3). 96.5% of outcomes are still fixed per (template, file, plan): the rendering quirks, not the real values, decide which plan is right. |
| M4 Not dominated on reward and wrong answers | World T sweep (6 × 5 points, every policy re-tuned at every point). At every point, no baseline has both higher reward per energy (CI above 0) and fewer wrong answers per 100 tasks than the being. In other words, the being is on the Pareto front of (reward per energy, wrong answers). | not met | Penalty 10 × reward, energy weight 1: LinUCB has more reward (being −0.104 [−0.165, −0.043]) and fewer wrong answers (0.7 vs 1.6 per 100). Always SQL gives 0 wrong answers but is behind on reward at 29 of 30 points. Wrong answers per point for every baseline are not yet tabulated. |
| M5 Explainable and replayable | Every decision cites its evidence in the log. Runs replay exactly from the log. Tampering is detected. | met (by tests) | Replay, fold and tamper tests in every track. Full suite at 4a2f9126: kore-self 617/0, kore-store 63/0. |
| M6 Safe under the controller | Every new capability is off by default, behind a fail-closed controller key. KILL stops runs. Refusals are logged before the genesis. | met (by tests) | Controller tests in every track. The all-keys coexistence test (B14, E1, L2) at 4a2f9126. |

## Notes

### M1
- Track N1 step 0 (tuning seeds 101..120 only, branch `n1-diag`) shows the first-world plateau is a ceiling. In the 8 rule worlds the B14 being is level with a reference that knows the true parameters. What is left there is rules nobody has, plus the 3% execution slips that no learner can remove. Only the simple worlds keep about 6% headroom.
- So B14, E1 and L2 did not fail at M1. The first world cannot show M1: a flat curve at the ceiling is expected.
- M1 is therefore judged in World T. There the gap is mostly which plan works on which file. 93.9% of (template, file, plan) outcomes are deterministic by construction, which is the author's generator and not real data (that is M3).
- Headroom figures are ceilings. The KP references know true rates estimated from about 12,000 other tasks, and a real learner also pays to explore.
- The B14 "oracle with book" (regret 0.085) is a first-world reference. It is not the World T "reward-best oracle" (+0.632 on tuning seeds, −0.686 at the report-seed headline), and the two are not comparable.

### M2
- The primary is the thirds statistic, not the Spearman. B12 `form` already gives a negative Spearman, but almost all of it is an immediate gain at the first top-tier families, not faster learning later. The thirds statistic measures what is wanted: later families are learned faster.
- The current intervals are about ±4 attempts on a mean of about 23, so an effect of about 15% either way cannot be ruled out.
- More report seeds, with the count fixed by a power analysis before the run, or a less noisy statistic would be needed.
- The per-world count of learnable top-tier families is not printed by `measure_l2 report`. About 190 top-tier families open per world (families 32 to about 223), but how many of them are learnable must be reported before the minimum count above is applied.

### M4
- Track W2 (report seeds 201..260, docs/measured.md): with the file-keyed estimator B1 (opt-in, `wt_file_key`) the being is dominated at 5 of 30 points (penalty 10, every weight); the default being B0 at 14, mostly by the new baseline LinUCB with the file feature. Still not met.
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
