# Track W2 pre-registration: the file-keyed estimator (B1) as the World T default

Written before any W2 code (2026-11-10). No W2 code, build or run exists yet. The owner approves this text before code;
after that, nothing below is changed after a report run.

## Why

N1 Gate 0 (branch `n1-diag`, harness at d34657b7, results in its `docs/measured.md`, "Gate 0 result") measured a
file-keyed baseline B1 against the current World T being B0, on report seeds 1..60, World T, penalty 0, energy weight
1, budget 6000:

* B1 minus B0: +0.220 [+0.193, +0.247] reward per energy (60/0/0), paired sd 0.104.
* Wrong answers per 100 tasks: B0 11.10, B1 7.77; B1 minus B0 -3.33 [-4.02, -2.64] (fewer in 57 of 60).
* B1 closes 0.38 [0.342, 0.416] of the gap to the known-parameter file reference (+0.359 remains, an upper bound).

B1 lives only in the measurement harness (`examples/measure_n1diag.rs`, `FileKeyed`). This track moves it into the
being, measures it again on fresh seeds and over the whole sweep, and makes it the default only if it holds there.
Seeds 1..60 are used up for this comparison and are not reused as report seeds here.

## Candidate

* B1 in the being: for each World T task, the estimate per plan is the being's Beta estimator over the counts of
  (template, file, plan), with the cell estimate (the current one) as its prior, prior weight from `beta`; the chooser,
  skip rule and `value_ucb` are unchanged. `beta` is fixed at 2.0, the value tuned on seeds 101..120 in Gate 0
  (tuning means: beta 0.5 +0.152, 1 +0.176, 2 +0.213, 4 +0.023, 8 -0.411, 16 -0.842); it is not re-tuned.
* Input: the file is already in every action event (`file:<name>` tag, `world_t/engine.rs:529`); the counts are
  folded from the log like the cell counts. No new event kind. If the fold needs a field that is not logged, it is
  added as a new tag (additive) and old logs fold with the file key empty (that is, exactly B0).
* Switch: controller key `wt_file_key` (fail-closed parse like the other keys). Stage 1: off by default; with it off
  the being is bit-for-bit the current being (golden test `world_t/tests_golden.rs` unchanged).
* Checkpoint (Track D1): the World T checkpoint payload gains the file-keyed counts. That is a new snapshot format
  version; a checkpoint of the old version is not used (fallback to the full fold), never misread. Test included.

## Equivalence test (before any report run)

The in-being B1 with `wt_file_key = true` and beta 2 reproduces the harness B1 exactly: same decision sequence and
same reward per energy on tuning seeds 101..120 (bit-identical), and the same on a restart in the middle of a life
(fold and checkpoint paths). If it does not, the difference is explained and fixed before any report run.

## Measurements (one report run each; seeds fixed now)

* Report seeds **201..260** (n = 60), never used by any World T measurement so far. Tuning seeds stay 101..120 and are
  not used again for W2 (beta is already fixed).
* PRIMARY: B1-in-being minus B0, World T, penalty 0, energy weight 1, budget 6000, default files: reward per energy,
  95% paired t-CI lower bound **>= +0.05** AND mean **>= +0.10** AND at least **45 of 60** wins. (Basis: Gate 0
  measured +0.220 with paired sd 0.104; at n = 60 the standard error is about 0.013, so a true effect of +0.15 or
  more passes almost surely, and a true effect at the threshold +0.10 passes about half the time.)
  **What the PRIMARY can and cannot show:** the threshold is far below the Gate 0 result, so passing it is expected; it
  is a replication on fresh seeds with the code moved into the being, not a new finding. The real decision about the
  default is in the secondaries that were never measured for B1: the sweep (b) (harsh penalties, where the being is
  weakest), out of distribution (c) and cold start (d).
* SECONDARY, all reported, none claimed as a win:
  * (a) wrong answers per 100 tasks, B1 minus B0, paired CI.
  * (b) the full World T sweep of Track W1b (6 penalties x 5 energy weights, every policy re-tuned at every point as
    in W1b, report seeds 201..260): at every point B1 minus B0, B1 minus tuned LinUCB, B1 minus tuned **LinUCB with
    the file feature**, B1 minus always-SQL, and wrong answers per 100 for every policy. "LinUCB with the file
    feature" = the W1b LinUCB with a one-hot of the task's file added to its context features (the same information
    B1 has), alpha tuned on seeds 101..120 at every sweep point as for plain LinUCB. Without it a B1-over-LinUCB gain
    could come only from the file information plain LinUCB never gets; claims against LinUCB are made against the
    better of the two LinUCB variants at each point. This is the M4 test: is the being, with B1, not dominated at any point (no
    baseline with higher reward per energy, CI above 0, and fewer wrong answers)?
  * (c) out of distribution: the config `ood = true` (never used in tuning): B1 minus B0, paired CI.
  * (d) cold start: reward per energy on each file's first 3 tasks, B1 minus B0.
  * (e) cost: wall time per life and peak memory; number of file keys per life (Gate 0: about 400 to 650).
  * (f) 0 wrong rules accepted (as in every World T track).

## Rule for making B1 the default (Stage 2)

B1 becomes the default World T estimator only if ALL hold, and then only with the owner's approval:

1. the PRIMARY is met;
2. no regression over the sweep (b), defined now: for each of the 30 points take the one-sided paired t-test of
   "B1 minus B0 < 0" on seeds 201..260 and apply the Holm correction over the 30 points at family-wise level 0.05; a
   regression is any point rejected after Holm. (So one point whose raw 95% CI lies below 0 by chance among 30 is not
   by itself a regression; every point's raw CI is still reported, and any point with a negative mean is listed);
3. out of distribution (c): B1 minus B0 is not significantly negative (95% CI entirely below 0 means it is);
4. the equivalence test passes.

Stage 2 changes the default of `wt_file_key` to true for new beings only. The golden test is then re-pinned for the new
default, and a second golden test keeps the old default reachable (`wt_file_key = false` gives the current being
bit-for-bit). Existing logs keep the behaviour they were created with: the setting is recorded at genesis like the
other World T learner settings (`world_t/engine.rs:308`) and a later run must not silently change it (a test checks
this; if World T genesis settings are not yet enforced on later runs, that enforcement is added for this key).

If 1 fails: the result is recorded as negative, the key stays off, nothing is re-tuned on the report seeds. If 1 holds
but 2 or 3 fails: B1 stays opt-in, and the failing points are reported as the next problem to solve.

## Milestones

* M4: decided by (b) under the existing M4 definition in `docs/milestones.md`. Currently not met (at penalty 10,
  weight 1, LinUCB has more reward and fewer wrong answers, 0.7 vs 1.6 per 100).
* N1: its PRIMARY baseline stays B1 with beta 2 (pre-registered in Track N1). If B1 becomes the default, N1 is measured
  against that default.
* M1 (World T): reported from the PRIMARY run, slope of windowed reward per energy, B1 minus B0, as a secondary only.

## Limits stated now

World T is the author's synthetic generator: each file is seen about 22 times per life and 93.9% of (template, file,
plan) outcomes are deterministic by construction, which is what a per-file key exploits. A gain here is not a
real-data result (milestone M3 stays separate). Nothing here is a claim of thinking or understanding.

## Stop rule

One report run for the PRIMARY and (a), (c) to (f); one report run of the sweep (b). Any number is reported as measured,
including a negative one. No constant is changed after a report run.
