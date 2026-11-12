# Track D1c design: an open whose cost follows the tail, not the number of files

Design only. Nothing here is built and nothing here is measured. Written on branch `d1c-design` from `4b39a150`.
Every cost statement is either a D1 measurement (named, with its source) or a reading of the code (with file:line).
Where the doc estimates, it says so.

## 1. Facts from Track D1 (measured, not re-measured here)

Source: `docs/longrun.md` "Track D1 results", raw rows in `docs/data/d1_timing_2026-11-10.jsonl`, targets in
`docs/d1_prereg.md`. Busy shared machine (busy share 0.63 to 0.99), so the numbers are rough.

| at 1,000,000 steps (40,064 versions, about 202,065 segment files, about 576 MB of segments) | run 1 / run 2 |
|---|---|
| in-process `open` (checkpoint off / on) | 72.7 / 52.7 s, 65.0 / 50.0 s |
| `manifest_json_parse_1_thread` (memfold rows) | 0.17 to 0.33 s |
| `read_and_sha_all_segments_1_thread` (memfold rows) | 26.5 to 48.0 s |
| fold with checkpoint (D1b) | 3.4 / 4.6 ms |
| whole 5,000-step restart call with checkpoint (T5: <= 10 s, <= 300 MB) | 72.3 / 56.2 s, about 1,575 MB: not met |
| KILL created during the call / present before the call (T6) | 74.0 s / 74.8 s: latency not met |

At 300,000 steps (59,942 files) the open was 11.5 to 12.1 s. Steps x3.3, open time x4 to x6: superlinear, cause
not isolated. The manifest parse is under 1 percent of the open; reading and hashing the files is the bulk. The rest
of the open (decode and index build, in parallel) was not split out by the harness.

## 2. Where the open spends its time (read from the code)

`VersionedLog::open` -> `open_existing` (`kore-self/src/versioned/mod.rs:911-935`), in order:

1. `recover_pending` (`mod.rs:913`, `forget.rs:143-156`): one `exists` check for `FORGET_PENDING` and one for
   `forget.staging`. O(1).
2. `read_manifest_raw` + `manifest::parse_ex` (`mod.rs:914-915`, `manifest.rs` `parse_ex`): reads the whole MANIFEST
   and `serde_json`-parses one line per version. O(versions); measured 0.17-0.33 s at 40,064 versions.
3. `check_chain` (`mod.rs:563-585`, called at `:920`): one sha256 per entry (`Entry::compute_hash`,
   `manifest.rs:82`), a path check per segment, `v4log::check_listings`. O(versions + segments), CPU only, no file I/O.
4. `load_state` (`mod.rs:703-713`, called at `:924`) -> `load_state_streamed` (`mod.rs:720-796`) for all-v2/v4 logs
   (the being's format 3 log), else `load_state_full` (`mod.rs:799-845`). **This is the O(files) part.**
   * `all_segs(entries)` (`mod.rs:620`) lists every segment of every version; the loop at `mod.rs:740` takes them
     512 at a time (`READ_CHUNK`, `:714`) with rayon.
   * For each segment: `read_verified` (`mod.rs:482-493`): one `fs::read` (open, read, close: one file-system
     path lookup per file in a table directory of about 40,000 files) and one sha256 over all bytes (`:489`).
   * Events segments: `load_ids` (`mod.rs:537-549`) decodes only the id column, then `Index::ev_add` per event
     (`:324`), about 3.0 M hash-map inserts at 1M steps.
   * Other tables are fully decoded (`load_seg`, `:525`) and folded: per revision `Index::add_rev` recomputes the
     revision content digest, a sha256 (`:386`); per prediction a `serde_json` parse of its options (`:394`); per
     outcome a set insert.
   * `forget::forgotten_set` (`mod.rs:793`, `forget.rs:50`) and the format from `schema_meta` rows: small.
5. `v4log::current` (`mod.rs:925`): format 4 only.

What the open builds and why appends need it: `Index` (`mod.rs:286-307`): every event id -> ordinal, every rev id,
every (entity, key) head, every revision content digest, every prediction id with options, every outcome id, the
forgotten set. `stage_batch` (`mod.rs:1380-1440`) uses all of it to reject duplicates, check revision chains and
causes, and validate outcomes. `ev_segs` (field at `mod.rs:453`) maps event ordinals to segments; it can
be derived from the MANIFEST alone (row counts are in each `SegRef`).

Not in the open path: directory listings. `stray_files` (`mod.rs:587-604`, `read_dir` per table) runs only in
`recover` (`:1101`, called by `write_commit` at version 0 only, `:1454`) and in `verify_dir` (`:626-656`). Per
commit `recover_next` (`:1124-1139`) removes at most 12 named files: O(1).

Other O(files) paths that a restart hits:
* **KILL present before the call**: the run is refused before the open, but `audit_refusals`
  (`kore-self/src/being_tools.rs:57-66`, from `:227-229`) calls `open(owner)` to append the audit row: a full open
  (explains the measured 74.8 s; inferred from the code, not split by timing).
* `genesis_peek` (`being/state.rs:452-464` -> `peek_events`, `mod.rs:937-957`): MANIFEST parse + `check_chain` +
  the first events segments. O(versions), not O(files).
* `forget` reads and hashes every segment (`forget.rs:195-260`) and ends with `reload()` (`forget.rs:384`,
  `mod.rs:960-984`): a second full open. Rare (owner action); left linear in this design.
* `open_at(v)` / `snapshot_from` (`mod.rs:1191-1208`, `:672-684`) read every segment up to v: inherent to a
  full snapshot at v and not on the restart path; unchanged.

Reading: the open is linear in files and bytes because (a) every segment is checksummed at every open and (b) the
whole index is rebuilt from rows at every open. Removing only (a) still leaves (b) reading every segment. Both
must go for an O(tail) open.

## 3. Candidate designs

### A. Sealed pack files (compaction), index still rebuilt

Concatenate the exact bytes of the segments of a sealed version range (for example 1,024 versions) into one pack file
`packs/<first>-<last>.kpk` with a trailer table (logical name, offset, length). MANIFEST entry lines are unchanged:
the logical names `<table>/<v>.kore` and per-segment sha256 stay, so every entry hash and `check_chain` are unchanged.
A new MANIFEST field (hashed additively, like `dicts`) on the next commit lists the pack and its sha256.

* Append-only: content unchanged; only the physical layout of sealed versions. Hash chain: unchanged bytes and hashes.
* Tamper detection: as today (every segment still hashed at open), plus the pack's own hash.
* `open_at(v)`: identical bytes, so identical snapshots for every v.
* `forget`: must rewrite each pack holding an affected segment (staged pack + `FORGET_PENDING` rename, the existing
  protocol in `forget.rs:1-24`).
* Crash safety: pack written temp + fsync + rename, committed by the MANIFEST line; loose files deleted only after the
  commit (idempotent; a crash leaves duplicates, readers prefer the listed pack, `recover` deletes the loose copy).
* Old logs: untouched unless packed. Cost: removes per-file overhead, files from about 202,000 to about 200 at 1M;
  the open still reads and hashes about 576 MB and rebuilds the index, so it stays linear and memory stays about
  1.5 GB. Expected not to meet T5 alone.

### B. Index snapshot in memory + verified-prefix anchor + tail

At a sealed version s, serialize the whole `Index` (plus format and `ev_segs` length) to `index/<s>.kidx`; the next
commit's MANIFEST entry carries `index: {v: s, file, sha256, chain_hash_s}` inside its hashed text. Open: parse
MANIFEST, `check_chain`, take the newest listed snapshot whose sha256 verifies and whose `chain_hash_s` equals entry s,
load it, then read and verify only the segments of versions > s (the tail) into the index.

* Verified prefix: a snapshot is built only from a full verified open (or from a verified older snapshot plus a
  verified tail), so it asserts "every segment <= s was read and checksummed when this was built". Segments <= s are
  **not re-hashed at open**; they stay hashed on every read (`read_verified` is used by every reader:
  `each_event_range` `mod.rs:1047`, `get_event` `:1008`, `snapshot_from` `:672`) and by `verify`.
* Tamper detection: a changed snapshot fails its listed sha256; a forged one is not listed (only a hash-chained
  commit can list it, which `verify_against(expected head)` catches); a truncated MANIFEST is caught as today by
  `verify_against`. **Change:** a flipped byte in an old segment no longer fails the open; it fails the first read
  of that segment and `verify`. This is the price; it needs an owner decision (section 6).
* `open_at(v)`: unchanged code, reads segments <= v as today: identical for every v.
* `forget`: re-seals entries >= `resealed_from`, so every snapshot with s >= that version fails the chain check and
  is ignored; `forget` already ends with a full `reload`, after which a new snapshot is written. Ordinals shift
  after forget, so reuse would be wrong; invalidation by chain hash makes it impossible.
* Crash safety: snapshot temp + fsync + rename, then listed by the next commit; a crash before that leaves an
  unlisted file, ignored and removed by `recover`.
* Cost: open reads the MANIFEST, one snapshot file and the tail. Memory: the whole index is still deserialized
  (about 1.5 GB process peak today; the split by field is not measured), so T5's 300 MB is expected not met.
  Writing a snapshot is O(index size) each time.

### C. On-disk index runs (sorted, immutable) + verified-prefix anchor + tail

As B, but the index is a list of immutable sorted runs, each covering a version range, each listed in the MANIFEST
(hashed field `index_runs: [{from, to, file, sha256, chain_hash_to}]`). A run holds fixed-width sorted tables:
event id (32 B) -> ordinal, rev id -> (entity, key) ref, (entity, key) hash -> head rev id + digest, content digest
-> first rev id, prediction id -> options blob offset, outcome ids; plus a small header. Lookups binary-search the runs
with positioned reads (newest run first for heads, oldest first for "first rev with this content"); the tail since the
last run is held in the existing in-memory `Index` and overlaid. A new run covers only the versions since the previous
run (O(delta) to write); when more than K runs exist (K = 8 proposed) the oldest are merged into one (temp + fsync +
rename, listed by the next commit, old runs deleted after the commit). Same verified-prefix, tamper, `open_at`,
`forget` and crash rules as B; the forgotten set stays in the MANIFEST as today.

* Cost: open = MANIFEST parse + `check_chain` (O(versions), 0.2-0.4 s measured at 1M) + run headers + the tail.
  Memory: the tail index plus read buffers; independent of the log size by construction (not measured).
* Risk: every append does up to K binary searches per id (estimated about 22 positioned reads per run for 3 M ids,
  mostly from the OS cache); the step phase may slow down. Measured by O7 below.

## 4. Recommendation

**C**, behind a key and a new format, with B's verified-prefix anchor as its trust rule. A alone keeps the open
linear in bytes; B meets the time target but not the memory target and rewrites the whole index per snapshot. C is
the only one whose open and memory are both bounded by the tail. A stays a later, separate option for the file
count (section 6), not part of D1c.

Implementation outline (later, not now): `index_runs.rs` (run format, writer, merge, lookup); `Index` gains a
`base: Vec<Run>` overlay consulted by `ev_has`/`ev_ord`, `revs`, `heads`, `content_ids`, `preds`, `outcomes`;
`open_existing` gets a fast path when the format is 5 and the key is on; `write_commit` writes a due run before the
MANIFEST append and lists it in the same entry; `verify_dir` also verifies every listed run against a rebuilt index;
`recover`/`stray_files` learn `index/`.

## 5. Rules

1. Additive only: no existing field, file, function behaviour or test changes meaning. New entry fields are absent
   on every old entry, so the hashed text of old entries is byte-identical (the `dicts`/`forget` pattern,
   `manifest.rs:64-70`, `:82-92`).
2. New format version: log format 5, MANIFEST header `KORE-VLOG-MANIFEST 4`. Binaries before D1c refuse the
   header (`parse_ex` rejects unknown headers), so they never misread a format-5 log. Upgrade is explicit and
   one-way (`upgrade_to_v5`, as `upgrade_to_v3`: new header line, every entry line kept byte for byte, one
   `schema_meta` commit). No automatic upgrade; new logs stay format 3 by default.
3. Old logs (formats 1-4) open by the unchanged code path, with the key on or off.
4. Default off behind a fail-closed controller key, `fast_open = on` in `controller.txt` (same parsing rules as
   `checkpoint_every`, `being/ckpt.rs:57-80`): missing file, unparseable file, missing key or any value but `on`
   = off = today's full open. On, a format-5 log uses the fast path; any failure of the fast path (no valid run,
   bad hash, chain mismatch, unknown run version) falls back to the full open with the same result and reports why.
5. Golden digests, decisions and appended rows unchanged: the being reads only events; runs add no event rows
   (unlike the D1b anchor note) and live only in MANIFEST fields and `index/`.
6. `verify` stays a full verification (every segment plus every listed run). The fast open is never called "verify".

## 6. Owner decisions needed

* D-1. Accept that a tampered old segment (<= the newest run) is detected on read and by `verify`, not at open.
  Alternative: the open also hashes N random old segments (sampling; N to be set), or a periodic `verify` run.
* D-2. Accept format 5 and an explicit `upgrade_to_v5` of the being's log (needed before any fast open).
* D-3. Run cadence (proposed: a new run when 2,000 versions or 100,000 events have passed, and at most K = 8 runs).
* D-4. Whether the KILL-present path should write its audit row via the fast open (proposed) or not open the log at
  all (would change what is audited where).
* D-5. Whether pack files (A) follow as a separate track for the file count (about 202,000 files at 1M remain on disk
  under C; only the open stops touching them).

## 6a. Owner decisions (2026-11-11)

* D-1: accepted, with sampling. The fast open also hashes 64 randomly chosen old segments (versions <= the newest
  usable run), chosen by a PRNG seeded from the MANIFEST head hash (reproducible for a given log state), in addition
  to the tail. Full detection stays in every read (`read_verified`) and in `verify`.
* D-2: format 5 with an explicit `upgrade_to_v5` is approved.
* D-3: cadence as proposed: a new run when 2,000 versions or 100,000 events have passed since the newest run, at most
  K = 8 runs; when there are more, the oldest are merged.
* D-4: the KILL-present audit row is written via the fast open.
* D-5: pack files (A) later, as a separate track; not part of D1c.

## 7. Pre-registered measurement plan (to be run later, in the D1 style)

Logs: the same copies as D1: cuts of the 1,000,000-step world-1 log (`kw/longrun-data2`) at version 12,001
(299,625 steps) and at the last version 40,064; fresh copy per variant; checkpoint on (`checkpoint_every = 5000`).
Variants: (off) format 3, key off; (on) the same copy after `upgrade_to_v5`, key on, one untimed priming call that
builds the first run(s) and the D1b checkpoint. Two runs of each timed measurement, both reported. Quiet machine:
no other build or test; the harness records the busy share and the run is marked rough if it exceeds 0.3. Disk guard:
the harness refuses to copy or run when free disk is below 4 GB. Page-cache state is not controlled; run order is
reported. The harness adds counters: files opened by the open, bytes read by the open.

| id | target (fixed now) |
|---|---|
| O1 | In-process open at 1M (on) <= 3.0 s in both runs, and <= max(1.5 x, 300k value + 0.5 s). |
| O2 | Whole 5,000-step restart call at 1M (on, real binary, cold process) <= 10 s and peak working set <= 300 MB (the T5 targets). |
| O3 | Files opened by the open (on) <= 6 x (versions after the newest run) + 20; at 1M right after the priming call <= 2,000. Total files on disk: reported, no target (unchanged by C). |
| O4 | KILL present before the call: refusal <= 5 s at 1M. KILL created 100 ms after the call: no step after it appears, latency <= open + 2 s and <= 7 s at 1M. |
| O5 | Writing one incremental run at the end of a 5,000-step call <= 2.0 s at 1M; a merge of K runs reported (no target); the priming call reported. |
| O6 | Step phase (call time minus open minus restore) at 1M, on <= 1.25 x off, same session. |
| O7 | Bit identity: after one timed call each, `compare` off vs on: events, revisions, causes, predictions, outcomes identical (no anchor difference expected, runs add no rows). |
| O8 | Old logs: the format-3 copy with the key on opens via the full path (counter = all files) and gives the same index and rows as key off. |

Expected but not promised: O2 memory depends on the tail index and OS cache accounting; if not met it is reported
as not met. A target is met only by its named measurement; nothing is extrapolated.

## 8. Tests (written with the code, run by the implementation)

* Time travel identical (property test, random logs of formats 3 and 5 with events, revisions, causes, predictions,
  outcomes, format-4 dictionaries, forgets, runs at random versions, merges): for every version v, `open_at(v)`
  `content_hash` from a fast open equals the full open; every index lookup (all present keys plus random absent
  ones) agrees; the same next batch after either open yields byte-identical MANIFEST and segment files (fixed clock).
* Tamper detected: a flipped byte, truncation, swapped run from another log, run listed at a re-sealed chain hash,
  unlisted run, listed-but-missing run: never used, fallback to the full open with an identical index, `verify`
  fails. A flipped byte in an old segment: fast open succeeds (documented, D-1), the first read of that segment and
  `verify` fail with `ChecksumMismatch`.
* Kill during run write and merge: crash before temp fsync, after rename before the MANIFEST append, during the append
  (torn tail), after the merge commit before old runs are deleted: the next open gives the same index as a full
  open, no unlisted run is used, `recover` removes leftovers.
* Forget: after `forget` of events before and after the newest run, runs at or after `resealed_from` are rejected;
  results equal the full open; the forgotten ids stay refused on re-ingest.
* Key and format: key missing, unparseable, `off`, other values = full open; key on with formats 1-4 = full open;
  existing format 1-4 fixtures and golden digests unchanged; a level-4 header is refused by the old-level parse.

## 9. Implementation (2026-11-11, branch `d1c-build`)

Built as design C with the owner decisions of section 6a. Code: `kore-self/src/versioned/index_runs.rs` (runs,
overlay, fast open, `maintain_index_runs`, `upgrade_to_v5`), `versioned/mod.rs` (format 5, `OpenOptions`,
`open_with`, `open_report`, io counters), `being/fast_open_ctl.rs` (key), `log_tools.rs` (`open` honours the key;
`maintain_runs` at the end of `being_run`). Tests: `versioned/tests_d1c.rs`. Harness: `longrun_being d1c-prep`,
`openfast`, `d1c-smoke`. Timing script (not run): `scripts/d1c_timing.sh`.

Deviations from the text above, and limits:

* Run files are KORE files (owner rule: storage is KORE only): `index/<from>-<to>.kore`, a kore-store row-group
  container whose 128-row groups are written by `KoreWriter`; not fixed-width binary tables. A lookup decodes one
  group (bloom filter and group keys in memory first).
* The open hashes every listed run file in full (sequential read of all run bytes) and decodes every group once
  to check order and build the bloom filter. So the open reads O(index bytes) in a few files, not only "run
  headers"; memory kept is the bloom filter (about 10 bits per index entry) plus about 50 bytes per 128-row group.
  Whether this meets O1/O2 is for the measurement.
* The run set is listed by an extra commit without rows (one more version per run write).
* `upgrade_to_v5` refuses format-4 logs; format 5 writes format-2 segments.
* After `forget`, runs whose `to` is at or after the re-sealed version are stale (ignored); the chain-valid prefix
  is still used; `forget` itself still ends with a full reload, and the next `maintain_index_runs` rewrites the set.
* The MANIFEST stays the existing JSON-lines journal (the `index_runs` field is a new hashed field in it).

Functional smoke (correctness only, no timing; `d1c-smoke --steps 50000 --chunk 5000`, in-process log of 50,000
steps, then 2 in-process calls and 2 calls through the real binary with `fast_open = on`): upgrade + first run
(`0000000000-0000002002.kore`); every check equal between full and fast open (180,648 events looked up, 0
mismatches, latest content equal); the fast open was used each time (64 sampled segments; files read by the open
2,545 fast against 12,045 full at 2,413 versions); KILL present: refused, one audit row written; `verify` Ok;
no `.json`/`.jsonl` file in the data root.
