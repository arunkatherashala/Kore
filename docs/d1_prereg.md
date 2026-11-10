# Track D1 pre-registration: default log format 3 (D1a) and a state checkpoint (D1b)

Written and committed on branch `brain-d1` (from `be348539`) **before any D1 code**. The targets below are fixed
now. Timing and memory are measured later by the owner's session, when no other build or test runs on the machine
(the earlier long runs were on a shared, saturated machine; see `docs/longrun.md`, "Setup"). Correctness tests are
part of the implementation and are run by the implementation itself.

## Baselines (measured earlier, with their sources)

| what | value | source |
|---|---|---|
| default format of a new log | format 1 | `kore-self/src/versioned/mod.rs:850` (`open_or_create_with_clock` passes `LogFormat::V1`); `kore-self/src/log_tools.rs:75-78` (`open` calls `open_or_create`), used by the being tools (`being_tools.rs:17`, `being_v2_tools.rs:18`) |
| log bytes per step, format 3, world 1 | 611 to 618 B/step, flat to 390,000 steps | `docs/longrun.md:294` |
| log bytes per step, format 1, world 1, 50,000 steps | 2,582 B/step (4.2 x format 3); MANIFEST 1.9 MB rewritten on every commit | `docs/longrun.md:296-297` |
| restart reads the whole history | `run_full` starts with `BeingState::from_log(log)`, a fold of every event | `kore-self/src/being/loop.rs:1077` |
| no state checkpoint exists | "A state checkpoint ... would remove the fold part only; not built" | `docs/longrun.md:381-384`, `:492` |
| open cost | every segment file read and sha256-checked at every open (200,066 files at 1,000,000 steps) | `docs/longrun.md:487`, `versioned/mod.rs` `load_state` |
| split of a restart at 390,000 steps (in-process, warm cache) | open 17.6 s, fold 4.3 s | `docs/longrun.md:381`, table at `:410-416` |
| child peak memory at 1,000,000 steps | 1,573 MB (the log's in-memory id index, about 1.5 KB per step) | `docs/longrun.md:459`, `:488` |
| KILL latency, file created during a call, 300,000 steps | 72 s (honoured only after the log load) | `docs/longrun.md:465` |

## What the design changes and what it cannot change

Read from the code before writing any: a restart has two linear parts.

1. **Open** (`VersionedLog::open`): reads and hashes every segment and builds the in-memory id index (event ids,
   revision ids and heads, content digests, predictions, outcomes). Appending needs that index (duplicate and
   revision-chain checks). This is most of the restart time and all of the 1,573 MB.
2. **Fold** (`BeingState::from_log`): reads every events segment again and folds the being's events.

D1b (a checkpoint of the folded being state) removes part 2 only: with a valid checkpoint at version v, the fold
reads only the events of versions after v. It does **not** change part 1. A restart whose time and memory are
proportional to the tail would also need an open that neither reads every segment nor holds every id in memory
(an index checkpoint plus an on-disk or bounded id index). That is not part of D1 and is named here as **D1c,
follow-up**, so that the whole-restart targets below are not read as promised by D1b.

## What will be measured later (owner's session, quiet machine)

* M-a. Log bytes per step after 50,000 steps of the world-1 long-run setup, created through the real binary with
  the new default (format 3) and with an explicit format-1 log: same seeds, same controller.
* M-b. Restart of one 5,000-step `being_run` call (cold process, real binary) on cut copies of the world-1 log at
  300,000 and 1,000,000 steps, with checkpoint off and with `checkpoint_every = 5000`: wall time of the call, process
  peak working set, CPU seconds; in-process split into open / restore (checkpoint load + tail fold) / full fold.
* M-c. KILL latency on the same copies: KILL file present before the call, and created 100 ms after the call is
  sent; with and without checkpoint.
* M-d. Bit identity: rows appended by the 5,000-step call with checkpoint on equal those with checkpoint off,
  except the checkpoint's own anchor note and wall-clock `recorded_at`.

## Targets (fixed now)

Tail = steps since the newest valid checkpoint. With `checkpoint_every = 5000` and calls of at most 5,000 steps
(the checkpoint is written at the end of a call when it is due), the tail at the start of a call is below 10,000
steps whatever the log size.

| id | target | justification from the design |
|---|---|---|
| T1 | A new log created by the being tools is format 3; M-a gives at most 650 B/step for it and at least 2,000 B/step for the format-1 copy (the old default). | Format 3 is already measured at 611-618 B/step; only the default changes. |
| T2 | Decisions identical: M-d equal; tests: a format-1 and a format-3 log of the same being fold to the same state and the same decision sequence; pinned golden digests unchanged. | No being code reads the format. |
| T3 | **Fold part** of a restart with checkpoint (checkpoint load + tail fold) at 1,000,000 steps: at most 1.0 s, and at most 1.5 x its value at 300,000 steps (flat in log size). Without checkpoint it stays linear (about 11 us per step: 4.3 s at 390,000). | Tail < 10,000 steps x about 11 us per step = about 0.1 s, plus decoding a world-1 state of well under 1 MB. |
| T4 | **Memory added by the fold** with checkpoint: at most 50 MB above the process peak after open alone, at 300,000 and at 1,000,000 steps. | The fold holds one state (world 1: bounded deques, counts, model cells) and one chunk of tail segments. |
| T5 | **Whole restart** with checkpoint at 1,000,000 steps: at most 10 s and at most 300 MB. **Expected NOT met by D1b** (predicted: the call time drops at most by the fold part, about 5 to 15 percent, and peak memory stays within 10 percent of the 1,573 MB baseline), because part 1 above is unchanged. Meeting T5 is the target of D1c. | See "What the design changes". |
| T6 | KILL file present before the call: refused before the history is loaded (unchanged, one open for the audit row). KILL created during the call: no step after the file appears (unchanged invariant), and latency at most open time + 2 s with checkpoint at both sizes (the fold no longer adds to it). A KILL honoured inside the open is D1c. | The kill check runs after open + restore; with checkpoint the restore is T3. |
| T7 | Tamper evidence (milestone M5) holds with checkpoints on: a corrupted, truncated, stale, chain-mismatched, forged (valid self-hash, not anchored in the log), unknown-version or crash-leftover checkpoint is never used; the fallback is the full fold with identical state. | Tests, written with the code. |

A target is met only by the measurement named for it; nothing is extrapolated. Measurements that cannot be run
quietly are reported as not measured.
