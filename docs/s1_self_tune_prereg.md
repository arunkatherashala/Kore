# Track S1 pre-registration: the being tunes its own settings during its life

Written before any S1 code (2026-11-11). Nothing has been coded, built or run. The owner approves this text before
code is written. After that, nothing below changes once a report run has happened.

## Why

In every World T track so far a human, the owner's harness, chose the being's settings by tuning on seeds 101..120:

* Track W2: B1 with beta fixed at 2 regressed at 6 of 30 sweep points (penalty 10 x every weight, and penalty 1 x
  weight 0.1). The re-tuned B0 used beta 1 or 0.5 there.
* Track W2b (`docs/w2b_prereg.md`): the owner's harness tunes a beta table per sweep point.

Both depend on someone outside the being knowing the reward design and running 30 x 6 tuning configurations. S1
asks whether the being can do that tuning itself, from its own experience, inside one life:

* no tuning seeds;
* every change is logged with its evidence;
* every change can be undone.

This is "improving itself" in a narrow, measurable sense. It does not change code.

## Candidate (S1)

* **Base.** B1 as in W2: `wt_file_key = on`, prior weight 2, chooser, skip rule and `value_ucb` unchanged.
* **Self-tuned setting.** One setting, the optimism `beta`, taken from the fixed 6-value grid
  {0.5, 1, 2, 4, 8, 16}.
* **Block loop.** The life is split into blocks of 200 steps.
  * At the start of each block the being picks one beta from the grid with a UCB1 rule over the grid. The arm value
    is the realized reward per energy of the blocks played with that beta; the bonus constant c is fixed at 0.1.
  * The first 6 blocks play each beta once, in grid order starting at 2.
  * The block's realized reward and energy come from the being's own outcome events. Nothing else is read.
* **Promotion, the "incumbent".** The being promotes a new incumbent beta only when both hold:
  * it has at least 3 blocks with that beta;
  * its mean block reward per energy is above the incumbent's by more than one pooled standard error.
* **Exploitation after promotion.** After a promotion, 3 of every 4 blocks use the incumbent. The fourth block keeps
  exploring by UCB1.
* **Logging.** Each block choice is a `decision` revision whose `caused_by` cites the outcome events of the blocks it
  used. Each promotion is a `rule` revision, key `being/worldt/self_tune/beta`, citing the same events. Rollback means
  re-instating an earlier revision; time travel shows every past incumbent.
* **Bounds.**
  * The being can only move within the fixed grid.
  * It cannot change the grid, the block length, c, or any other setting.
  * It cannot change code.
  * The controller's caps, the KILL file and `authorize()` apply as in every World T run.
* **Switch.** A new controller key `wt_self_tune` (fail-closed parse, values `off` (default) and `on`), read only
  when `wt_file_key = on`.
  * The setting is recorded at genesis and fixed for the life.
  * With the key absent, every being is bit-for-bit the being at commit eaa5fa13; both golden tests pass with their
    files unchanged.
  * The self-tune state is folded from the log. It goes into the World T checkpoint payload as a new snapshot format
    version 3; a version-2 checkpoint is refused and the log is folded instead (tested).
* **Fixed constants.** The block length 200, c = 0.1, the 3-block minimum and the 3-of-4 schedule are fixed now and
  are not tuned. The life of a World T being at budget 6000 is about 4,000 to 4,500 steps, so about 20 to 22 blocks.

## Equivalence and safety tests (before any report run)

1. With `wt_self_tune = on`, the decisions inside a block equal those of B1 run with that block's beta from the same
   state. The test runs on tuning seeds 101..120 and covers both a fold restart and a checkpoint restart.
2. Replay from the log alone reproduces every block choice and every promotion. The test expects 0 mismatches.
3. KILL in the middle of a block: no step runs after it, and the self-tune state folds correctly afterwards.

## Measurements (one report run each; seeds fixed now)

* **Seeds.** Report seeds **321..380** (n = 60), never used by any World T measurement. Seeds 1..60, 201..260,
  261..320 (W2b) and 401..460 (M3) are not reused. Tuning seeds 101..120 are not used at all: S1 has no tuned constant.
* **PRIMARY.** At the harsh point where W2 failed most, penalty 10 x reward and energy weight 1, the measure is S1
  minus B1 with fixed beta 2, reward per energy. To pass, the 95% paired t-CI lower bound must be **> 0** and S1 must
  win in **at least 36 of 60** worlds.
* **PRIMARY guard.** At the main point (penalty 0, weight 1), S1 minus B1 with fixed beta 2 must not be significantly
  negative (the 95% CI is not entirely below 0). Exploring costs something, and this measures how much.
* **SECONDARY.** All are reported; none is claimed as a win.
  * (a) The full 6 x 5 sweep. At every point, report S1 minus B1 (beta 2), S1 minus B0 (owner-tuned per point as in
    W2), S1 minus B1p (the owner-tuned table of W2b, if W2b has finished; otherwise left out and said so), and S1
    minus the better LinUCB variant. Report wrong answers per 100 tasks and skip share for every policy, and apply
    Holm over the 30 points for "S1 minus B1 (beta 2) < 0" and for "S1 minus B0 < 0".
  * (b) Self-tuning reference: S1 minus "B1 with the best fixed beta for this world in hindsight", which shows how
    much of the possible gain S1 finds inside one life.
  * (c) Choices: the share of blocks per beta, the final incumbent per point, and the number of promotions per life.
  * (d) Out of distribution (`ood = true`): S1 minus B1 (beta 2).
  * (e) Cost: wall time per life, and the number of extra revisions and cause links per life.
  * (f) 0 wrong rules accepted.

## Decision rule

S1 becomes the default only if ALL of the following hold, and then only with the owner's approval:

1. the PRIMARY is met and the guard holds;
2. there is no Holm-rejected regression of S1 minus B0 over the sweep;
3. OOD is not significantly negative;
4. the equivalence and safety tests pass.

If 1 fails, the result is recorded as negative. The constants are not changed on the report seeds, and any next
candidate needs its own pre-registration.

## What this is not

* It is not code self-modification, and the being cannot widen its own search space.
* It is not general self-improvement: it tunes one number from a grid of 6, within one life, in a synthetic world.
* World T is the author's generator (93.9% deterministic keys). A real-data test is M3.
* Nothing here is a claim of thinking or understanding.

## Stop rule

There is one report run for the PRIMARY with the guard, (d), (e) and (f), and one report run of the sweep with (a),
(b) and (c). Every number is reported as measured, including negative ones.
