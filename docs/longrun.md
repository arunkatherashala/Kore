# Long run of the being (kore-self), unattended, with restarts

Everything here is the same synthetic sandbox as Track B1 to B6: a world, tasks, rewards, solvers and rule
grammar written by one author, no real data, no language model. The numbers say how this code behaves when it is
left running for hours in that sandbox. They are not claims about anything outside it.

## Setup

* Code: branch `longrun`, built from commit `c7e5d8c3` (the being and the log are unchanged from that commit; the
  `kore-self.exe` used by every run was built from it, before the one small fix, item 6 of "Failures and bugs found"). Harness:
  `kore-self/examples/longrun_being.rs` (commit `5ae6b4f4` and later on `longrun`).
* Date: 2026-11-05 (UTC 04:50 to 09:35).
* Machine: Windows 11 Enterprise, 8 logical CPUs, 34.0 GB RAM. **The machine was shared**: two other agents were
  building and testing in parallel the whole time. The harness measured the share of CPU time busy over all cores
  between the start and end of every child process: 0.94 to 1.00 (0.98 or more for all but one). Available RAM was
  5.6 to 10.7 GB at the checkpoints. Free disk on C: was 11.9 to 20.3 GB (other agents' build directories grew while
  the runs went on). All wall times below are therefore on a saturated machine and are upper bounds, not speeds of
  the code alone; CPU seconds of the child processes are given where they matter.
* Release build, `CARGO_TARGET_DIR=C:\Users\skathera\kw\target-longrun`. Data roots under
  `C:\Users\skathera\kw\longrun-data\` (never the real home: `KORE_SELF_HOME`, `HOME` and `USERPROFILE` of every
  child point into the data root). Run data and raw results are not committed.

### What one run does

The being is driven through the **real binary** (`kore-self.exe <owner>` over stdio MCP, tool `being_run`), exactly as an
owner would drive it: `chooser model`, `pattern_families extended2`, discovery every 10 steps, composition and pruning
requested, budget 3000 energy units with refills (`stop_after_budget: false`), loop seed 1. `controller.txt` in the owner
directory holds the documented safe defaults with composition and pruning allowed (live rules 16, building blocks 8,
search candidates 2000, steps per run 5000, energy per run 100000). The log is created in format 3 (journal MANIFEST)
before the first call; the binary keeps the format.

Each child process gets 2 calls of 5000 steps (the hard maximum per call), then stdin is closed and the process exits.
So every 10000 steps is a cold restart: the next process re-opens the log and re-folds the being's state from it. After
the child exits the harness records its peak working set and CPU seconds (Win32 `K32GetProcessMemoryInfo`,
`GetProcessTimes` on the child handle) and the machine's CPU busy share (`GetSystemTimes`).

At checkpoints (every 50000 steps in world 1, every 25000 in world 4) the harness, with no child running, opens the log
in-process and:

* times `VersionedLog::open`, `verify()` (full hash chain and every segment checksum) and three one-event probe
  commits (source `longrun_probe`, which the being ignores);
* reads only the new versions (`diff`) and folds their events into its own copy of the being state, checking that
  outcome steps are consecutive (no gap, no repeat across restarts), counting audit rows, refusals, stops, rule notes
  and live rules after every rule change (cap check);
* re-derives up to 300 logged decisions per checkpoint from the folded log state alone (world from the genesis
  seed, the same planner) and compares them with the logged action;
* every 4th checkpoint and when memory allows ("heavy"): decodes the whole latest snapshot, re-folds it from scratch,
  checks it equals the incremental fold, and records its content hash and answers (steps, total reward, rule count,
  event count); at later checkpoints `open_at(Version(v))` of the first recorded version (and on heavy checkpoints two
  more) must give the same content hash and the same answers;
* computes an oracle in the same world: it knows the hidden success probability of every strategy and the real energy,
  given the being's live rule book; "skip oracle" also skips a menu when the best expected reward per energy is below a
  threshold (best of a grid 0 to 8 in steps of 0.25, 30000 energy units). The plain oracle never skips; the being does,
  so the skip oracle is the fair yardstick.

Learning per window of steps, from the logged outcomes only (plus the hidden truth for one column):

* reward per energy (rpe) and regret = skip-oracle rate minus the window rate;
* **repeated mistake (known bad)**: an attempt with a (domain, difficulty, strategy) whose own logged record BEFORE
  that attempt had at least 10 attempts and at most 10% right. "of them wrong": such attempts that were wrong again;
* **truly bad**: attempts whose hidden success probability (measurement only, strategies other than `discovered`) is at
  most 0.10;
* **rule wrong repeated**: an answer given by a rule that was wrong, where the same rule had already given a wrong
  answer earlier in the log.

### Commands

```
cargo build -p kore-self --release --offline
cargo build -p kore-self --release --offline --example longrun_being
E=target-longrun/release/examples/longrun_being.exe
$E run --root <data>/smoke --steps 20000 --checkpoint-every 5000 --window 2500 --kill-at 12000      # smoke
$E run --root <data>/w1 --world 1 --steps 1000000 --checkpoint-every 50000 --window 50000 --kill-at 300000 --heavy-every 4 --max-hours 9
$E run --root <data>/w4 --world 4 --steps 1000000 --checkpoint-every 25000 --window 25000 --kill-at 250000 --heavy-every 4 --max-hours 8
$E control --root <data>/smoke --steps 20000 --world 1     # in-process single run vs the chunked log
$E torn --src <data>/smoke --root <data>/torn              # damage on copies of the smoke log
$E authz --root <data>/authz                               # authorize() path of being_run (advisor_query)
$E prof --root <data>/prof --world 4 --steps 10000 --window 2500
$E memstat --root <data>/smoke
$E scan --root <data>/scan --world 8 --steps 6000           # which worlds find rules
$E rulecheck --root <data>/w4                              # wrong rule answers: wrong rule or execution slip
$E digest --root <data>/dig --world 4 --steps 20000 --chunk 5000   # sha256 of every being event and revision (bit-identity checks)
```

`run` resumes on an existing root (`--steps` and `--kill-at` count the being's total steps). Below the disk floor
(`--min-free-gb`, default 12) the harness starts no child; the first session stopped there, the later sessions pause
until 12.5 GB are free.

Worlds: a scan of worlds 1 to 5 in-process (6000 steps each) found rules only in worlds 3 (6 rules) and 4 (12 rules); in
worlds 1, 2 and 5 the being almost never attempts sequence tasks (4, 12 and 5 revealed examples in 6000 steps), so it
never has examples to search. World 1 is the long run (no rules: the log, restarts and planner alone); world 4 is the
run with discovery, composition and pruning.

## Run length

| run | world | steps reached | child processes (cold restarts) | `being_run` calls | wall time | why it ended |
|---|---|---|---|---|---|---|
| smoke | 1 | 20,000 | 3 | 5 + 1 refused | 0.04 h | target reached |
| long run | 1 | **390,000** | 40 | 79 + 1 refused | 3.65 h (05:34 to 09:13 UTC, 4 sessions) | memory guard: the next restart would need more than the free RAM allows (1.3 x 5.2 GB + 2 GB > 8.7 GB available) |
| rule world | 4 | 44,275 | 6 | 8 complete + 1 cut + 1 of 100 steps | 4.3 h (05:13 to 09:30 UTC) | stopped by hand: a step had become about 0.56 s (see bug 3) |
| format 1 control | 1 | 50,000 | 5 | 10 | 0.06 h | target reached |

The 1M-step target was **not reached**. The long run was interrupted once by the disk floor (free disk 11.9 GB at
100,000 steps, caused by the other agents' build directories) and resumed 20 minutes later from the same log; after
the kill test at 300,000 steps a harness bug stopped it (it treated a 0-step call as fatal; fixed, resumed); it ended
at 390,000 steps on the memory guard. Each resume starts a new harness session on the same log; the being's state
comes only from the log, so for the being a resume is one more cold restart.

## Long run, world 1: checkpoints

Format 3 log. "decode+fold" is only measured on heavy checkpoints. "open_at same" = earlier recorded versions
re-opened with `open_at` that gave the identical content hash and answers / versions checked. Wall times are on a
saturated machine.

| steps | verify | verify s | open s | decode+fold s | probe commit ms (median of 3) | log MB | files | bytes/event | bytes/step | full re-fold equals incremental fold | open_at same | cumulative reward per energy | RAM available GB | free disk GB |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 50,000 | ok | 2.1 | 3.0 | 2.1 | 9.4 | 30.9 | 10,006 | 206 | 618 | yes | - | 4.095 | 8.4 | 13.5 |
| 100,000 | ok | 3.8 | 9.7 | - | 7.8 | 61.6 | 20,009 | 205 | 616 | - | 1/1 | 4.096 | 8.6 | 12.1 |
| 150,000 | ok | 4.2 | 7.6 | 24.6 | 225.3 | 92.2 | 30,012 | 205 | 615 | yes | - | 4.095 | 7.4 | 16.4 |
| 200,000 | ok | 11.1 | 23.6 | - | 11.8 | 122.8 | 40,015 | 205 | 614 | - | 1/1 | 4.094 | 8.6 | 14.1 |
| 250,000 | ok | 11.9 | 37.4 | - | 10.8 | 153.5 | 50,018 | 205 | 614 | - | 1/1 | 4.093 | 10.7 | 14.0 |
| 300,000 | ok | 15.2 | 53.4 | - | 10.4 | 183.9 | 60,021 | 204 | 613 | - | 1/1 | 4.088 | 10.0 | 13.8 |
| 350,000 | ok | 15.1 | 61.4 | - | 17.7 | 214.2 | 70,027 | 204 | 612 | - | - | 4.083 | 5.6 | 13.6 |
| 390,000 | ok | 16.5 | 82.1 | - | 19.9 | 238.4 | 78,030 | 204 | 611 | - | - | 4.077 | 9.6 | 13.4 |

Heavy checkpoints after 150,000 steps were skipped by the memory rule of the harness (a full decode needs about the
child's peak again). The `open_at` re-check after a resume uses the first version recorded in that session.

Child process of each restart (the being itself):

| steps at the end of the process | peak working set MB | CPU s | wall s of the process (2 calls of 5000 steps) | wall ms per step in the call that ended there |
|---|---|---|---|---|
| 10,000 | 136 | 13 | 60 | 3.4 |
| 50,000 | 629 | 49 | 39 | 4.1 |
| 100,000 | 1,308 | 110 | 89 | 7.3 |
| 150,000 | 1,987 | 187 | 140 | 14.2 |
| 200,000 | 2,653 | 404 | 241 | 24.0 |
| 250,000 | 3,357 | 549 | 330 | 34.1 |
| 300,000 | 4,007 | 752 | 479 | 50.4 |
| 350,000 | 4,690 | 971 | 638 | 66.6 |
| 390,000 | 5,201 | 1,205 | 831 | 89.2 |

## Long run, world 1: learning per window of 50,000 steps

Skip oracle without rules (world 1 has none): 4.158 reward per energy (threshold 2.50); the never-skip oracle: 3.596.

| steps | reward per energy | regret vs skip oracle | attempts | skips | right share | known-bad attempts per 1000 | truly bad per 1000 | strategies (fast / careful / deep / discovered) |
|---|---|---|---|---|---|---|---|---|
| 0-50k | 4.095 | +0.063 | 21,345 | 28,655 | 0.944 | 0.0 | 0.3 | 21,345 / 0 / 0 / 0 |
| 50k-100k | 4.098 | +0.060 | 21,329 | 28,671 | 0.946 | 0.0 | 0.0 | 21,329 / 0 / 0 / 0 |
| 100k-150k | 4.093 | +0.065 | 20,988 | 29,012 | 0.949 | 0.0 | 0.0 | 20,988 / 0 / 0 / 0 |
| 150k-200k | 4.090 | +0.068 | 20,976 | 29,024 | 0.948 | 0.0 | 0.0 | 20,976 / 0 / 0 / 0 |
| 200k-250k | 4.088 | +0.070 | 21,141 | 28,859 | 0.948 | 0.0 | 0.0 | 21,141 / 0 / 0 / 0 |
| 250k-300k | 4.066 | +0.092 | 20,225 | 29,775 | 0.955 | 0.0 | 0.0 | 20,225 / 0 / 0 / 0 |
| 300k-350k | 4.047 | +0.111 | 20,003 | 29,997 | 0.956 | 0.0 | 0.0 | 20,003 / 0 / 0 / 0 |
| 350k-390k | 4.030 | +0.128 | 15,509 | 24,491 | 0.964 | 0.0 | 0.0 | 15,509 / 0 / 0 / 0 |

Reading:

* The being found its policy (fast strategy only, skip about 57% of menus) within the first window and **did not
  improve after that**. From about 200,000 steps on it got slowly **worse**: reward per energy 4.098 -> 4.030, regret
  +0.060 -> +0.128 (roughly doubled), with fewer attempts and more skips per window (attempt share 42.7% -> 38.8%) and
  a higher right share (0.944 -> 0.964). It becomes more selective than the skip oracle (which skips only below 2.50).
  A possible cause, NOT tested: the planner's score is optimistic by `beta x sd`, the sd shrinks as attempts pile up,
  and the skip threshold (`skip_frac` x the rate earned so far, about 4.09) stays; so offers that used to clear the bar
  stop clearing it. The B1 settings were tuned at budgets 600 and 3000 (about 150 and 700 attempts), not at 160,000.
* Repeated mistakes: **0** attempts on a (domain, difficulty, strategy) whose own record already showed at least 10
  attempts with at most 10% right, in every window. By hidden truth, 0.3 per 1000 attempts in the first window were on
  a choice with success probability at most 0.10, then 0. In this world the only strategy used (fast) is right about
  95% of the time, so a repeated known mistake had little chance to show; the rule world below is the harder case.

## Rule world (world 4): discovery, composition, pruning

Same harness and settings, world 4, checkpoints every 25,000 steps. The run was stopped by hand at 4.3 h: by then a
5,000-step call took 2,787 s (557 ms per step, most of it CPU: bug 3). Stopping the harness ended its child in the
middle of a call; the log was at 44,175 steps (the last 25-step commit), verified ok, steps consecutive, and a restart
ran 100 more steps from there (`verify` ok again, full re-fold equal to the incremental fold).

| steps | verify | open s | decode+fold s | probe commit ms | log MB | files | bytes/step | revealed examples | rules live / total | child peak MB (process ending here) | cumulative reward per energy | skip oracle with the live rules |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 10,000 | - | - | - | - | - | - | - | - | 12 / 12 | 161 | - | - |
| 20,000 | - | - | - | - | - | - | - | - | 12 / 12 | 304 | - | - |
| 30,000 | ok | 2.6 | 1.8 | 12.7 | 22.8 | 6,006 | 760 | 9,462 | 12 / 12 | 449 | 3.781 | 3.904 |
| 44,175 | ok | 2.2 | 1.9 | 10.5 | 33.5 | 8,844 | 758 | 14,122 | 12 / 12 | 588 (at 40,000) | 3.798 | 3.904 |
| 44,275 (after the restart) | ok | 2.4 | 1.7 | 10.3 | 33.6 | 8,867 | 758 | 14,157 | 12 / 12 | 717 | 3.799 | 3.904 |

Wall time of a 5,000-step call: 239 s (0-5,000), 648 s, 1,223 s, 1,527 s, 1,821 s, 2,139 s, 2,412 s, 2,787 s
(35,000-40,000): it grows about linearly with the step count, so the total time grows with its square.

Skip oracle without rules: 3.510 (threshold 1.75); with the being's 12 live rules: 3.904 (threshold 2.50; never-skip
3.805). Learning per window of 5,000 steps (from the log):

| steps | reward per energy | regret vs skip oracle with the live rules | attempts | skips | right share | known-bad attempts per 1000 | of them wrong | truly bad per 1000 | rule answers | wrong rule answers | of them by a rule already wrong before | rules accepted | retired |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 0-5k | 3.537 | +0.367 | 3,307 | 1,693 | 0.888 | 0.6 | 2 | 11.8 | 384 | 13 | 7 | 12 | 0 |
| 5k-10k | 3.838 | +0.066 | 3,175 | 1,825 | 0.944 | 0.0 | 0 | 0.0 | 679 | 18 | 16 | 0 | 0 |
| 10k-15k | 3.796 | +0.108 | 3,170 | 1,830 | 0.934 | 0.0 | 0 | 0.0 | 711 | 28 | 28 | 0 | 0 |
| 15k-20k | 3.838 | +0.066 | 3,156 | 1,844 | 0.944 | 0.0 | 0 | 0.0 | 660 | 16 | 16 | 0 | 0 |
| 20k-25k | 3.850 | +0.054 | 3,211 | 1,789 | 0.940 | 0.0 | 0 | 0.0 | 743 | 23 | 23 | 0 | 0 |
| 25k-30k | 3.817 | +0.087 | 3,137 | 1,863 | 0.943 | 0.0 | 0 | 0.0 | 636 | 21 | 21 | 0 | 0 |
| 30k-35k | 3.840 | +0.064 | 3,139 | 1,861 | 0.946 | 0.0 | 0 | 0.0 | 696 | 19 | 19 | 0 | 0 |
| 35k-40k | 3.837 | +0.067 | 3,179 | 1,821 | 0.941 | 0.0 | 0 | 0.0 | 678 | 21 | 21 | 0 | 0 |
| 40k-44.2k | 3.821 | +0.083 | 2,648 | 1,527 | 0.937 | 0.0 | 0 | 0.0 | 557 | 24 | 24 | 0 | 0 |

Strategy mix after the first window: about 71% fast and 29% `discovered` (rule answers), no careful or deep.

Reading:

* All 12 rules were accepted in the first 5,000 steps (the B5 smoke saw 7 by step 2,000); none was added, retired or
  merged in the next 39,000 steps, and no cap was reached. Reward per energy rose from 3.537 to about 3.84 after the
  first window and then stayed flat (3.80 to 3.85) with regret +0.05 to +0.11 against the skip oracle that has the same
  rules. No further improvement over 39,000 steps; no decline either in this shorter run.
* Known-bad repeats: 2 in the first window (0.6 per 1000 attempts), then 0. Truly bad choices (hidden success
  probability at most 0.10): 11.8 per 1000 attempts in the first window, then 0.
* "Wrong rule answers by a rule already wrong before" look like repeated mistakes (24 of 24 in the last window), but
  `rulecheck` shows that **none of the 183 wrong rule answers was a wrong rule**: on every one of them the rule's own
  prediction equals the revealed next term; the answer was wrong because the executor slipped (the `discovered` strategy
  has the same slip rate as `careful`). Over 5,744 rule answers the rules were never wrong on the task. Pruning (window
  6, retire below 0.5) correctly never retired them: their window success stays near 97%.

Where the time goes (in-process engine, no log, `prof`, world 4; the machine was saturated; ms per step):

| steps | revealed examples | live rules | the step itself | discovery pass (every 10 steps, per step) | housekeeping (pruning and merge check, every step) | total |
|---|---|---|---|---|---|---|
| 2,500 | 466 | 7 | 0.21 | 3.15 | 2.09 | 5.46 |
| 5,000 | 1,352 | 12 | 0.25 | 15.00 | 13.91 | 29.15 |
| 7,500 | 2,186 | 12 | 0.43 | 42.34 | 48.45 | 91.22 |
| 10,000 | 2,959 | 12 | 0.25 | 59.61 | 68.52 | 128.38 |

## Correctness

* `verify()` (hash chain plus every segment checksum, straight from disk): **ok at every checkpoint** of every run (8
  in the long run, up to 390,000 steps and 15,628 versions).
* Steps across restarts: the outcome step numbers in the long-run log are consecutive from 0 to 389,999, no gap and no
  repeat, across 40 cold restarts and 4 harness sessions.
* Replay from the log alone: 2,400 logged decisions of the long run re-derived (the first 300 after each checkpoint,
  plus the first 300 of the log at the start of every session): **0 mismatches**. Smoke run 900, world 4 900,
  format 1 run 1,500: 0 mismatches.
* Restart invariance (`control`): the same being run in-process in one go for 20,000 steps gives exactly the same
  60,016 being events (id, kind, world time, text, tags) as the smoke log, which the binary wrote in 4 calls over 3
  processes, interrupted once by the KILL file (audit notes excluded, since they mark run boundaries).
* Incremental fold equals a full re-fold of the whole snapshot at every heavy checkpoint (50,000 and 150,000 steps in
  the long run; every checkpoint of the smoke, world 4 and format 1 runs).
* `open_at`: every re-opened earlier version gave the identical content hash and identical answers (steps, total
  reward, rule count, event count): 4 of 4 in the long run (version 2,001 re-opened at 100,000 steps; version 6,007 at
  200,000, 250,000 and 300,000), 3 of 3 in the smoke run, 7 of 7 in the format 1 run. One `open_at` of version 6,007
  (about 150,000 steps of history) took about 31 s on the saturated machine.

## Safety and control

* Caps: no call ran more than 5,000 steps (the cap), no call spent more than 12,397 energy units in world 1 or 14,618 in world 4 (cap 100,000), live
  rules never above 16 (world 1: 0; world 4: max 12), composed rules never used more than 8 blocks (world 4:
  max 6). Checked both on the tool's answers and on the log after every rule change. Refusals logged:
  none in either long run except the two KILL refusals (no cap was reached).
* KILL file, long run (at 300,000 steps): the harness created `KILL` 126 s into a 5,000-step call. The call returned
  `stopped: kill switch` with **0 steps run**, 174 s after the file appeared: at that log size the call spent its first
  minutes opening and folding the log, and the check before the first step saw the file. A second call with the file
  present was refused before any step ("the KILL file exists ..."). Both are audit rows (source `authorize`, tool
  `controller:kill_switch`) and the log has the `controller_stop` note at step 300,000. After the file was removed the
  run went on from step 300,000.
* KILL file, smoke run (11,625 steps): the file appeared while steps were running; the call stopped after 1,625 of
  5,000 steps, 0.95 s after the file appeared (at about 3.4 ms per step). Refused while present; audited.
* `authorize()`: in `being_run` the being's steps are not tool calls; `authorize()` is reached only when the being has
  an advisor (`advisor_query`). Checked separately (`authz`, simulated advisor): without `autonomy_policy.txt` the run is
  refused, 0 steps, 1 audit row (deny); with `advisor_query` under `[L1]`, 10 calls of 200 steps gave 10 "allow" audit
  rows for `advisor_query`, one per call; with `KILL` present the call is refused with an audit row. In the long run the
  log holds exactly three sources: `being:loop` (1,170,305 events), `authorize` (2 rows, both from the KILL test) and
  the harness's `longrun_probe` (24): nothing else wrote to it.

## Crash and restart

Kill-free: every 10,000 steps the child exits normally and the next one starts from the log (40 times in the long
run). Torn writes were tested on copies of the 20,000-step smoke log (format 3), then the real binary was started on
the copy:

| damage (simulated crash) | `verify` before restart | after a `being_run` on the copy |
|---|---|---|
| half a MANIFEST journal line appended (crash during the append) | ManifestCorrupt: uncommitted partial tail | run ok, 20,000 -> 20,200 steps, verify ok |
| a segment of the next, uncommitted version on disk (crash before the MANIFEST line) | ExtraSegment | run ok, 20,000 -> 20,200, verify ok (the leftover is removed) |
| last 4 MANIFEST lines cut at a line boundary (the last being commit and 3 probe commits lost) | ExtraSegment (leftover segments of the lost versions) | the being is back at step 19,975 and redoes the 25 lost steps; the redone events are identical to the lost ones (60,016 of 60,016 tuples equal) |
| one flipped byte in the oldest events segment | ChecksumMismatch | the binary refuses the log ("checksum mismatch in segment"), nothing runs |

Closest to a real crash: the world 4 harness was stopped by hand in the middle of a call, and its child ended within
40 s without finishing the call (how it ended was not observed). The log was at the last 25-step commit (44,175
steps), `verify` was ok, steps consecutive, and the next process ran 100 more steps from there. A process killed
exactly during a commit, or a power loss, was not tested.

## Resources

* Log on disk, format 3: 611 to 618 bytes per step, 204 to 206 bytes per event, flat over 390,000 steps (238 MB). One
  segment file per table per commit: 78,030 files at 390,000 steps (5 per 25-step commit).
* Format 1, same world and seeds, 50,000 steps: 2,582 bytes per step (4.2 times format 3), 860 bytes per event, 10,018
  files, MANIFEST 1.9 MB rewritten on every commit (format 3's journal MANIFEST was 15 MB at 390,000 steps and is only
  appended). Child memory and open time were about the same as format 3 at 50,000 steps (child peak 639 MB vs 629 MB;
  open 2.2 s vs 3.0 s). Format 1 was not run longer.
* Commit time (one-event probe commit): 8 to 20 ms median at every log size up to 390,000 steps (one 225 ms value on
  the loaded machine): **no growth** in format 3.
* Open time grows with the log: 3.0 s at 50,000 steps, 82 s at 390,000; `verify` 2.1 s -> 16.5 s.
* **Restart memory grows linearly with the history: about 13 KB per step** (child peak 136 MB at 10,000 steps,
  5.2 GB at 390,000). Every `being_run` call folds the whole log, and the whole snapshot (all tables) is held in memory.
  `memstat` on the 20,000-step smoke log: in-memory revision strings (rationale, stance, key) 77 MB = 3.9 KB per step,
  event strings 19 MB = 1.0 KB per step, 13 revision-cause rows per step; on disk the whole log is 12.4 MB. The decision
  rationale JSON stored in every decision revision is the largest part, and it is decoded on every restart although
  the being's fold reads only events.
* **Restart time grows linearly with the history**: a 5,000-step call took 17 s at 10,000 steps and 446 s at 390,000
  (3.4 -> 89 ms per step), almost all of it opening and folding the log (the steps themselves take about 3 ms each in
  world 1). `being_run` opens and folds the log twice per call (once to read the genesis, once in the loop).

## Failures and bugs found

1. **Memory grows linearly with the history and is needed again at every restart** (about 13 KB per step; 5.2 GB at
   390,000 steps). This ended the long run. Extrapolated, not measured: about 13 GB at 1,000,000 steps. Cause:
   `VersionedLog::open` / `open_latest` materialise every table, `BeingState::from_snapshot` folds all events, and
   decision revisions (rationale JSON) dominate. Not fixed (needs an events-only or lazy read path, or a state
   checkpoint, in `versioned` and `being/loop.rs`; not small).
2. **Restart time grows linearly** (17 s -> 446 s per 5,000-step call), same cause, plus the double fold in
   `being_tools::run` (`load(owner)` and `run_full` each open and fold the log). Not fixed.
3. **Per-step cost grows with the number of revealed examples in a rule world** (world 4: 48 ms per step in the first
   5,000 steps, 557 ms per step at 35,000 to 40,000 steps; in-process profile: 0.25 ms for the step itself, 60 ms discovery and 69 ms housekeeping per step at 10,000 steps). `discover_pass` evaluates every candidate on every
   logged example of each signature every 10 steps, and `housekeeping` re-checks every live rule for a merge against
   all examples of its signature at every step (and every simpler rule against each of them), so the total time is
   quadratic in the run length. No cap bounds it (the candidate cap bounds candidates, not examples). **Fixed** on
   branch `rulescan-fix` by an exactly equivalent incremental scan, no cap; see "Rule scan fix" below.
4. **Reward per energy declines slowly in a stationary world** (world 1: 4.098 -> 4.030 per window, regret from +0.060
   to +0.128 between 50,000 and 390,000 steps; more skipping). Cause not established. Not fixed.
5. KILL latency on a large log: the kill check runs before each step, so a KILL file that appears while a call is still
   loading the log takes effect when loading ends (174 s at 300,000 steps). No step ran after the file appeared, so "no
   step after KILL" held; only the answer was slow. Not fixed (follows from 2).
6. With an advisor and a KILL file, the refusal told the owner to edit `autonomy_policy.txt`, which was not the reason.
   **Fixed** in `being_tools.rs` (the hint names the KILL file when it exists), test
   `kill_file_with_an_advisor_names_the_kill_file_not_the_policy`.
7. Harness bugs, fixed in the harness: a 0-step kill-stopped call was treated as fatal; no final checkpoint when a
   session ended between checkpoints; one run root deleted by a concurrent clean-up of my own (world 4 was restarted at
   0 steps).

Side observation: the binary also writes `becoming.kore`, `learning_history.kore` and `memories.kore` into the owner
directory (legacy files); their size was not tracked.

## Rule scan fix

Branch `rulescan-fix` (from `048855f2`), 2026-11-06, same machine, again shared with other agents' builds and tests
(wall times are upper bounds; where a table row ran while an 8-thread test suite was running it says so). Fixes bug 3.
Data under `C:\Users\skathera\kw\longrun-data3\` (not committed).

**Cause, confirmed.** `prof` (in-process engine, no log, world 4) with the code of `048855f2`: the step itself stays at
0.14 to 0.18 ms, while the discovery pass (every 10 steps, shown per step) and housekeeping (every step) grow with the
revealed examples:

| steps | revealed examples | step | discovery per step | housekeeping per step | total ms per step |
|---|---|---|---|---|---|
| 2,500 | 466 | 0.18 | 2.53 | 1.63 | 4.33 |
| 5,000 | 1,352 | 0.15 | 9.76 | 9.15 | 19.07 |
| 7,500 | 2,186 | 0.14 | 18.88 | 21.38 | 40.40 |
| 10,000 | 2,959 | 0.18 | 36.26 | 42.24 | 78.68 |

`discover::search_core` computes `predict_x` (an exact Gauss-Jordan fit) for every candidate on every example of each
signature on every pass, and the merge check in `housekeeping` predicts every live rule and every simpler rule on every
example of its signature at every step. A third, smaller scan: the planner counted the examples per signature on every
decision, for a curiosity bonus that is off by default.

**Fix (exactly equivalent, no cap).** New `being/discover_scan.rs`, `ScanCache`, held by the engine (`Engine.scan`,
`Engine2.scan` for World v2). The search and the merge check are pure functions of the example list, the list only grows
at its end, and the status of a candidate on one example depends on that example alone. So the cache keeps, per
signature: the positions of its examples and the held-out mask; per candidate `(transform, form)`: how far it has
scanned, a bitset of the examples it reproduced, its training / held-out counts, and whether it was ever
applicable-and-wrong (then its scan stops: such a candidate can never be accepted again, and the full search uses
nothing else of it); per live rule of the merge check: its prediction on every example, the number of examples where it
applies and the first 24 of them; per (rule, simpler rule) pair: whether they have agreed so far. A pass only predicts
the examples that arrived since the last pass, then runs the same loop as the full search (same candidate order,
transferred hypotheses first, same cap and truncation, same "new support" through the same `covered` set, now a bitset,
same cited examples in the same order). If the example list is not an extension of the scanned one, the cache starts
over. The planner counts examples only when the curiosity bonus is on. Between two `being_run` calls of one process the
cache is kept and reused only if the new run's examples begin with exactly the scanned ones (compared example for
example); a new process starts empty, and its first pass then scans every example once (on one thread: the being's
sources start no threads, a test checks it). The old code is kept as the reference (`Engine.reference_scan`, `discover_scan::with_reference`);
the tests run both. Pruning by low success was already incremental (latest uses per rule) and is unchanged.

**Bit-identical, shown four ways.**

* Digest (`longrun_being digest`: the being in-process with a real log, in calls of 5,000 steps with a new engine per
  call as `being_run` does; sha256 over every being event and every revision): world 4, 20,000 steps, old and new code
  give the same digest `b77b7b60...` (60,032 events, 20,012 revisions, 12 rules, same reward); world 3, 10,000 steps,
  `bdf12c04...` both.
* Against the old long run: `control` on a copy of the world 4 log of the first long run (written by the old binary in
  calls over 6 processes, 44,275 steps): the new code in-process for 44,000 steps gives the same 132,055 being event
  tuples (id, kind, world time, text, tags; audit notes excluded), first difference none.
* The measurement programs, old and new build, same seeds: `measure_being discover` (B2, seeds 1..20), `b6` (B6, seeds
  1..20, budget 3000), `b5-prune-wide` and `measure_world_v2 report` (World v2, seeds 1..20, all 7 conditions):
  the printed output is byte for byte the same, and so is `b5` (B5 composition, pruning, caps and kill switch, seeds
  1..20; the old build took 2.6 h for it, the new one 7 minutes, both on the loaded machine). These were run with the
  build just before the last change (the first pass of a new process then used threads, since removed); the digests,
  `control` and the tests were repeated with the final build.
* Tests (`being/tests_scan.rs`, 11): the incremental search equals the full rescan after every new example (3 example
  sets, 3 block sets, with and without a candidate cap, with and without transferred hypotheses), on a cold start over a
  long history (the first pass of a new engine), and when the list is replaced; no example is predicted twice; the merge check
  answers equal the full rescan for every pair of 9 rules after every 3rd example; housekeeping on a crowded rule book
  (about 115 rules) merges the same rules with the same records after every example; B5 runs (8 configurations: no
  controller, default caps, the long-run controller, tight pruning, live-rule and candidate caps, narrow and wide rule
  drift; they include low-success retirements, both caps and composed rules), B6 runs (3 arms, 2 seeds), World v2 runs
  (3 discovery arms, clean and hard) and logged runs through `run` in 4 calls (all events and revisions) are identical
  with the reference; a stashed cache is reused only for exactly the same examples.

**Per-step time after the fix.** `prof`, world 4, new code, one engine (no restarts), up to 200,000 steps and 64,411
revealed examples. The table is the run with the planner change and without the between-call reuse (which does not
apply to one engine); a second run with a later build gave 0.25 to 0.30 ms per step up to 50,000 steps and 1.0 to 2.8
ms in rows that ran while an 8-thread test suite ran (the unchanged step itself went from 0.15 to 1.39 ms there):

| steps | revealed examples | live rules | step | discovery per step | housekeeping per step | total ms per step | scan cache MB | process peak MB |
|---|---|---|---|---|---|---|---|---|
| 10,000 | 2,959 | 12 | 0.12 | 0.06 | 0.04 | 0.22 | 0.3 | 7 |
| 50,000 | 15,947 | 12 | 0.52 | 0.23 | 0.18 | 0.93 | 2.0 | 12 |
| 100,000 | 32,186 | 12 | 0.43 | 0.25 | 0.17 | 0.85 | 3.9 | 17 |
| 150,000 | 48,230 | 11 | 0.47 | 0.25 | 0.18 | 0.90 | 4.3 | 20 |
| 200,000 | 64,411 | 11 | 0.39 | 0.19 | 0.14 | 0.72 | 7.3 | 25 |

Before the fix, 10,000 steps took 78.68 ms per step and grew; after it, the cost per step does not grow with the
examples in this range (it moves with the machine's load). One rule was retired for low success between 100,000 and
110,000 steps (the same in every run).

In calls of 5,000 steps with a real log (`digest`, world 4, a new engine per call):

| steps at the end of the call | old code: ms per step | new code: ms per step |
|---|---|---|
| 5,000 | 12.44 | 1.92 |
| 10,000 | 61.28 | 1.86 |
| 15,000 | 205.87 | 1.61 |
| 20,000 | 358.59 | 2.58 |

Old: 3,191 s for 20,000 steps; new: 40 s, most of it the log (opening and folding it at every call, bug 2).

The real binary (`run`, world 4, same harness and settings as the first long run, a cold restart every 10,000 steps,
final build): **150,000 steps in 0.47 h** (the first long run stopped by hand at 44,275 steps after 4.3 h). Wall time of
a 5,000-step call, and the child process of the restart that ends there:

| steps at the end of the call | first long run (old code): s per call | this run: s per call | ms per step | child CPU s (2 calls) | child peak MB |
|---|---|---|---|---|---|
| 5,000 | 239 | 6.4 | 1.3 | - | - |
| 10,000 | 648 | 11.3 | 2.3 | 13 | 156 |
| 20,000 | 1,527 | 13.3 | 2.7 | 24 | 301 |
| 40,000 | 2,787 (35,000-40,000) | 22.6 | 4.5 | 49 | 584 |
| 60,000 | - | 29.8 | 6.0 | 76 | 893 |
| 80,000 | - | 28.6 | 5.7 | 103 | 1,209 |
| 100,000 | - | 34.3 | 6.9 | 134 | 1,530 |
| 120,000 | - | 55.2 | 11.0 | 172 | 1,864 |
| 150,000 | - | 89.7 | 17.9 | 255 | 2,318 |

Three calls took much longer than their neighbours (108.3 s at 55,000, 155.6 s and 115.0 s at 85,000 and 90,000
steps); their processes had a machine busy share of 0.84 and 0.65 (the others 0.96 to 1.00): the machine was waiting
on something else, the being did not compute more. What still grows per call is the restart: opening and folding the
log twice per call and the memory of bug 1 (about 15 KB per step here; bugs 1 and 2, not changed here), plus the first
scan of the examples in each new process (below). The being itself: checkpoints at 30,000 to 150,000 steps all `verify`
ok, 1,800 logged decisions re-derived with 0 mismatches, steps consecutive, no cap exceeded (live rules at most 12), no
refusal, no stop; one rule retired for low success at about 110,000 steps; cumulative reward per energy 3.781 (30,000),
3.801 (50,000), 3.812 (80,000), 3.816 (100,000), 3.809 (130,000), 3.808 (150,000) against 3.904 for the skip oracle with
the same rules. Two earlier builds of the fix (one without the planner change and the reuse between calls, to 120,000
steps, where one call took 1,360 s while the machine was 16% busy and its process used 238 s of CPU for both of its calls; one with a
threaded first pass, to 150,000 steps, 450,156 being events) gave the same checkpoint values at the
same steps and, for the second, the same number of events.

**What still grows (not changed here).**

* A new process (every restart) has an empty cache, so its first pass scans every example once: at 64,411 examples
  3.8 million predictions, 31.3 s on the loaded machine. Per restart, not per step; it adds to the restart cost of
  bugs 1 and 2. A second call in the same process reuses the cache.
* Per pass, the bitset work (AND and popcount over the reproduced examples of each live candidate) is linear in the
  examples with a small constant (64 examples per word); it is inside the discovery column above.
* The cache holds a few bytes to 16 bytes per example and tracked rule: 7.3 MB at 64,411 examples. Between calls it
  also keeps a copy of the examples.
* Not touched: the B6 transfer similarity (`transfer::rule_fit`, a scan of the `pattern_b` examples after attempts,
  only in transfer worlds), the advisor's rule-proposal verdict when a proposal is rejected (`judge_rule` evaluates the
  form on the signature's examples, once per proposal), and the replay check `replay::rederive_rules` (runs the full
  search once per rule note, measurement only).

No cap was needed, so none was added: the rules, decisions and events are those of the old code.

## Not measured

* 1,000,000 steps or more (the run ended at 390,000 on memory); anything beyond is extrapolation.
* A real crash during a commit (process killed, power loss); only simulated damage on copies.
* Torn-write tests on the large log (only on the 20,000-step log, to keep disk use down).
* Format 1 beyond 50,000 steps (its whole-MANIFEST rewrite per commit was not pushed to large version counts).
* Speeds on an idle machine: every wall time here is on a machine at 98-100% CPU from other work.
* Drift, advice and transfer worlds over a long run; more than one world seed per configuration (one long run each, so
  no intervals).
* The cause of the slow decline in world 1, and whether other `beta` / `skip_frac` values or forgetting remove it.
* The rule world past 150,000 steps with the real binary (200,000 in-process with `prof`, after the rule scan fix), and
  rule worlds other than world 4 over a long run (world 3 only in the 10,000-step bit-identity check).
