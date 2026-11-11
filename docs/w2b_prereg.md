# Track W2b pre-registration: B1 with a penalty-aware beta

Written before any W2b code (2026-11-11). No W2b code, build or run exists yet. The owner approves this text before
code; after that, nothing below is changed after a report run.

## Why

Track W2 (`docs/b1_default_prereg.md`, results in `docs/measured.md`, "Track W2") measured B1, the file-keyed
estimate with beta fixed at 2, in the World T being on report seeds 201..260:

* PRIMARY met: B1 minus B0 +0.225 [+0.192, +0.258] (59/0/1), penalty 0, energy weight 1.
* Stage-2 rule 2 not met: 6 regressions after Holm over the 30 sweep points, at penalty 1 x reward and weight 0.1
  (-0.071) and at penalty 10 x reward at all five weights (-0.67 to -1.55, 0/0/60 at each).
* Why: at penalty 10 B1 answers almost every task (skip share about 0.02) and is wrong 3 to 11 times per 100 tasks,
  against 0.1 to 2.4 for B0. B0 was re-tuned at every point (its beta was 1 or 0.5 at penalty 10); B1 was not, its
  beta stayed 2 everywhere. So part of the gap may be tuning, not the file key.
* At the main point the default being B0 is below the fair baseline LinUCB with the file feature: -0.135
  [-0.175, -0.096] (11/0/49); B1 is above it by +0.090 [+0.047, +0.132] (43/0/17).

W2b asks one question: with the same tuning as B0, does B1 stop regressing at the harsh-penalty points while keeping
the W2 PRIMARY?

## Candidate (B1p)

* B1 exactly as in W2 (`wt_file_key = on`; file-keyed Beta counts with the cell estimate as prior, prior weight 2,
  chooser, skip rule and `value_ucb` unchanged), except the optimism `beta` (the factor on the posterior sd in
  `learner.rs`, `beta_milli`) is chosen per reward design.
* The being reads the reward design from the task it sees (`penalty_pm`, `cost_pm` are public fields of every World
  T task view). It looks `beta` up in a table keyed by (penalty_pm, cost_pm), with one entry per sweep point. A design
  that is not in the table uses the nearest entry in log space, with ties going to the higher penalty. The table is
  part of the being's genesis settings (logged, fixed for the life, like `wt_file_key`).
* The table is filled by tuning on seeds 101..120 only. For each of the 30 sweep points, choose the beta from the
  6-value grid {0.5, 1, 2, 4, 8, 16} with the highest mean reward per energy on seeds 101..120. This is the same
  grid and the same selection rule that B0 gets in W1b and W2.
* Switch: a new controller key `wt_file_beta = table` (fail-closed parse; the only other value is `fixed`, which is
  the default and means W2's B1 with beta 2). It is read only when `wt_file_key = on`. With the key absent, every
  being is bit-for-bit what it is at commit 4b39a150 (both golden tests, files unchanged). Checkpoint: the table is in
  the genesis, not in the payload, so the World T checkpoint format stays 2. A test checks this.

## Fair tuning

B0, B1p, UCB1, LinUCB and LinUCB with the file feature each get 6 configurations per sweep point, tuned on seeds
101..120 only, with the selection rule "highest mean reward per energy". B1p has no other free constant: the prior
weight stays 2 and the grid is the B0 grid. The tuning table of every policy is printed and committed in docs/data
before the report run.

## Equivalence test (before any report run)

1. At every point where the table picks beta 2, B1p must be bit-identical to the W2 B1 on seeds 101..120: same
   decisions, same reward and energy.
2. At every point, B1p in the being must match a harness B1 with the same beta: bit-identical on seeds 101..120, on
   a logged run, and on a restart in mid-life, both from the full fold and from a checkpoint.

## Measurements (one report run each; seeds fixed now)

* Report seeds **261..320** (n = 60), never used by any World T measurement. Seeds 201..260 (W2) and 1..60 (Gate 0)
  are not reused. Tuning seeds 101..120 are not used for anything else in W2b.
* PRIMARY: B1p minus B0 at penalty 0, energy weight 1, budget 6000, default files, reward per energy. To pass, the
  95% paired t-CI lower bound must be **>= +0.05**, the mean **>= +0.10**, and B1p must win in **at least 45 of 60**
  worlds (the W2 thresholds). If the table picks beta 2 at that point, the PRIMARY is a second replication of W2. It is
  not a new finding.
* The real question is the sweep: rule 2 below.
* SECONDARY, all reported, none claimed as a win:
  * (a) wrong answers per 100 tasks, B1p minus B0, at every point.
  * (b) the full 6 x 5 sweep on seeds 261..320. At every point report B1p minus B0, B1p minus LinUCB, B1p minus
    LinUCB with the file feature, B1p minus UCB1, B1p minus always SQL, the skip share, and wrong answers per 100 for
    every policy. Claims against LinUCB are made only against the better of the two LinUCB variants at each point.
  * (c) out of distribution (`ood = true`, at penalty 0, weight 1): B1p minus B0.
  * (d) the same sweep for W2's B1 with beta 2, on seeds 261..320. This shows how much of any change comes from tuning
    alone.
  * (e) M4: is B1p dominated at any point (some baseline with higher reward per energy, CI above 0, and fewer wrong
    answers)?
  * (f) 0 wrong rules accepted. Cost: wall time per life and peak memory.

## Rule for making B1p the default

B1p (with `wt_file_key = on`, `wt_file_beta = table`) becomes the World T default only if ALL of these hold, and then
only with the owner's approval:

1. the PRIMARY is met;
2. there is no regression over the sweep, using the W2 definition: for each of the 30 points, take the one-sided
   paired t-test of "B1p minus B0 < 0" on seeds 261..320 and apply the Holm correction over the 30 points at
   family-wise level 0.05. A regression is any point rejected after Holm. Every raw CI is still reported, and every
   point with a negative mean is listed;
3. out of distribution: B1p minus B0 is not significantly negative (the 95% CI is not entirely below 0);
4. the equivalence tests pass.

Stage 2 is as in W2: new beings only, both keys recorded at genesis, the old default reachable and pinned by a
second golden test.

If 1 fails, the result is recorded as negative and nothing is re-tuned on the report seeds. If 1 holds but 2 or 3
fails, B1p stays opt-in and the failing points are reported. If B1p still regresses at penalty 10, the next candidate
is a change to the skip rule, not more beta tuning. That needs its own pre-registration.

## Limits stated now

* World T is the author's synthetic generator: 93.9% of (template, file, plan) outcomes are deterministic. A gain
  here is not a real-data result (that is M3, `docs/m3_design.md`).
* A beta table keyed by the reward design only works when the being is told the reward design. That is true in
  World T but may not be true elsewhere.
* Choosing one beta per point uses 30 x 6 tuning configurations. That is the same budget as B0, but it is still
  tuning, not learning by the being.
* Nothing here is a claim of thinking or understanding.

## Stop rule

One report run for the PRIMARY with (a), (c), (f), and one report run of the sweep (b), (d), (e). Every number is
reported as measured, including negative ones. No constant is changed after a report run.
