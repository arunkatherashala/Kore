# Versioned log (M1 core)

A local, deterministic, append-only journal of how recorded beliefs, habits and
decisions change. Every revision can cite the events that caused it, any past
version can be queried with SQL, and predictions are stored so they can later be
scored against outcomes. Code: `kore-self/src/versioned/`. Demo:
`cargo run -p kore-self --example versioned_demo --offline`.

This module is a storage and query layer. It does not interpret events, infer
beliefs, or predict anything; all stances, confidences and predictions are
supplied by the caller (a user or a named rule).

## Data model

All tables are typed `.kore` files (KORE columnar format).

| table | columns |
|---|---|
| `events` | event_id, occurred_at, recorded_at, source, kind (note/action/decision/outcome/statement), text, tags (comma-joined), redacted |
| `revisions` | rev_id, entity_type (belief/habit/decision, plus library-only `rule`), key, stance, confidence (0..1), rationale, supersedes (nullable), valid_from, author (`user`, `legacy` (imported from the old state, never cited) or `rule:<id>`), recorded_at |
| `revision_causes` | rev_id, event_id (link table; every event_id must exist) |
| `predictions` | pred_id, made_at, question_id, target, options (JSON), probs (JSON), model_id, model_version, features_cite (JSON event ids), resolves_by |
| `outcomes` | pred_id, resolved_at, actual (one of the options), outcome_event_id (nullable) |
| `schema_meta` | format_version, created_at |

* `event_id` = hex sha256 of `source|occurred_at|text`, with text trimmed and
  internal whitespace collapsed. Re-adding the same event is a no-op.
* `occurred_at` and `valid_from` are caller-supplied (`YYYY-MM-DD` or
  `YYYY-MM-DDTHH:MM:SS[Z]`, UTC, normalised to `...Z`); they are never defaulted
  to "now". `recorded_at` comes from the writer's clock and is forced to be
  non-decreasing. Two time axes therefore exist: when something happened
  (`occurred_at`, `valid_from`) and when the log learned it (`recorded_at`).
* Rule-authored revisions must cite at least one event. User revisions may be
  uncited; the registered `revisions` table and both belief views expose an
  `unsupported` column (1 = no cited event).
* `supersedes` defaults to the current head for (entity_type, key). Corrections
  are new rows; nothing is edited. Retrying an identical revision is a no-op.

## File layout

```
<root>/<owner>/log/
  MANIFEST                      one JSON line per commit, hash-chained (header 1, 2 = journal (format 3), 3 = format 4, 4 = format 5)
  events/0000000003.kore        only the rows added by commit 3
  revisions/0000000003.kore
  revision_causes/...  predictions/...  outcomes/...  schema_meta/0000000000.kore
  dicts/<sha256>.kdic           format 4 only: shared dictionary objects listed in the MANIFEST
  index/<from>-<to>.krun        format 5 only: index runs listed in the MANIFEST (Track D1c)
```

`owner` is restricted to `[A-Za-z0-9_-]{1,64}`. Version 0 is the genesis commit
(`schema_meta`). Version `v` is the manifest prefix `0..=v`, i.e. the first `v`
commits' segments. A manifest entry lists the segments of that commit with their
sha256 and row counts, plus `prev` and `hash` where
`hash = sha256(prev_hash_hex || canonical_entry_bytes)`.

## Commit protocol

Each mutating call is one commit and creates at most one version; a call that
adds no new rows (all duplicates) creates none. `commit_batch` groups events,
revisions, predictions and outcomes into one atomic version (revisions may cite
events of the same batch). A commit: validate everything (nothing is written if
any item is invalid) -> write each new segment to a temp name, fsync, rename ->
write `MANIFEST.tmp`, fsync, rename over `MANIFEST`, fsync the directory where
the platform allows. The commit exists only once the manifest rename succeeds.
Segments from a crash before that point are not listed, are ignored by readers,
and are deleted by `recover()` or the next commit. Before writing, a commit
re-reads the manifest and fails with `Conflict` if another writer changed it.
(Format 3 replaces the rename by an append, see below.) The check runs only when the commit has something to write: a call whose events all exist already reads nothing from disk.

## Log format 2 (opt-in) and compatibility

Two segment encodings exist; a manifest entry's segment records which one
(`"enc":2` for v2, absent = v1, part of the hashed entry text).

* **Format 1** (default for new logs): `kore-store` columns + LZ4. Unchanged;
  every log written before format 2 existed opens, verifies and queries exactly
  as before (`tests/data/vlog_v1/` is a fixture written by the old binary and
  checked in `versioned/tests_v2.rs`).
* **Format 2** (`LogFormat::V2`, implemented in `versioned/codec.rs` + `hz.rs`, no
  new dependency, nothing changed in `kore-store`): the same `.kore` container
  header but version 3, with per-column codecs: 64-hex ids as 32 raw bytes
  (the API and SQL still show the same hex strings), timestamps as delta varints,
  low-cardinality columns as dictionary + bit-packed codes, `tags` as a token
  dictionary, and an `hz` LZ77+Huffman stage over each column body when smaller.
  Old readers refuse these files (header version 3); they never misread them.
  Format 2 also makes revision replay idempotent by content hash: a revision whose
  content digest (type, key, stance, confidence, rationale, valid_from, author,
  causes) already exists anywhere in the log is a no-op, not only when it equals
  the current head. A deliberate revert to an earlier stance with an identical
  `valid_from` and cause list is therefore also a no-op; give it a later
  `valid_from` or an explicit `supersedes`.
* **Upgrade is explicit, never silent.** `open_or_create*` never changes the format
  of an existing log. `VersionedLog::upgrade_to_v2()` appends one commit with
  `schema_meta.format_version = 2` (old segments and hashes untouched; later
  segments are v2). `migrate_to_v2(src_root, dst_root, owner)` writes a fresh v2
  copy of every version and checks `content_hash` equality for each version (the
  chain hashes of the copy differ; forget records are carried over verbatim).
* **Open and lookup.** When every events segment is v2, `open()` still reads and
  sha256-verifies every segment (in parallel) but decodes only the event-id column;
  `open_latest()` decodes the rows on first use. `get_event(id)` reads one segment.
  Logs with any v1 events segment decode everything at open, as before.
* **Streamed open (restart fix).** For such an all-v2-events log, `open()` reads the other
  tables a chunk of segments at a time (512, in parallel) and folds each chunk into the
  index before reading the next, so open no longer holds every revision, cause,
  prediction and outcome row in memory at once (peak memory = the index plus one chunk).
  The index is the same as the full read gives, provided every `revision_causes` row is
  in the same commit as its revision and no `rev_id` is stored twice (always true for
  logs written through the API); a log where that does not hold is detected and read
  the old way (all at once), so it opens with the same index. Every segment is still
  read and sha256-verified at open; nothing about the files or the format changed.
* **Events-only read.** `for_each_event(f)` hands every event row of the latest version
  to `f`, in log order (the same rows as `open_latest().rows::<EventRow>()`), reading
  only the events segments, a chunk at a time, checksums verified, without keeping
  them (the snapshot is used if it is already in memory). `peek_events(root, owner, f)`
  does the same without opening the log (MANIFEST chain checked, events segments read
  in order until `f` stops), for a quick look at the first events. The being folds its
  state with `for_each_event` (`BeingState::from_log`) and reads its genesis with
  `peek_events`; revision text is not decoded on a restart except to build the index.
* **Commit cost.** A commit no longer clones the id index or re-parses/re-renders
  the MANIFEST: the new MANIFEST is the previous bytes plus one line, still written
  to `MANIFEST.tmp`, fsynced, renamed; the conflict check compares the on-disk
  bytes with the bytes this handle last wrote.

## Log format 3 (opt-in): journal MANIFEST

Format 3 is format 2 plus a different MANIFEST discipline; the segment encoding is the
format 2 one (`"enc":2`), the entry lines and the hash chain are unchanged.

* **Shape.** Header line `KORE-VLOG-MANIFEST 2` (format 1 and 2 logs have `... 1`). Older
  binaries refuse the header cleanly. Every entry line is exactly what it was, so the chain
  hashes of an upgraded log are unchanged.
* **Commit.** Segments are written, fsynced and renamed as before; then the one new entry
  line (with its `\n`) is appended to MANIFEST with a single write and fsynced. The commit
  exists once that fsync returns. Nothing is rewritten, no rename, no directory entry change,
  so the cost no longer grows with the number of versions (measured in
  `docs/format_improvement_r2.md`).
* **Crash during the append.** An unterminated last line is an uncommitted tail: `open()`
  ignores it (the log is at the previous version), `verify()` reports it as a manifest
  problem, and `recover()` or the next commit cuts it off before appending. A malformed line
  that is newline-terminated is corruption and fails `open()`. The segment files of the
  interrupted commit are stray files, handled as before.
* **Conflict check.** For a journal the check is the file length plus the last 512 bytes
  compared with what this handle last wrote (O(1)); a concurrent append changes both. Format 1
  and 2 logs keep the full byte comparison. An edit in the middle of a journal that keeps the
  length is not seen by the conflict check but is caught by `verify()` and by the checksum or
  chain check on the next read of the affected data.
* **Upgrade is explicit.** `VersionedLog::upgrade_to_v3()` (from format 1 or 2) appends one
  commit with `schema_meta.format_version = 3` and, in the same atomic file replacement,
  switches the header; all earlier lines are kept byte for byte. `open_or_create*` never does
  it; `LogFormat::V3` applies only when a log is created. There is no downgrade.
* `forget` rewrites the MANIFEST (the chain is re-sealed) with the log's own header.

## Log format 4 (opt-in): KORE v4 compact segments and shared dictionaries

Format 4 stores every segment as a kore-store version-4 ("compact") file and codes the
`events.text` column with a **shared dictionary**: a separate, immutable, content-addressed
object that many segments reference by id instead of each segment carrying its own vocabulary.
Measurements: `docs/log_format4.md`.

* **Shape.** MANIFEST header `KORE-VLOG-MANIFEST 3` (journal discipline, as format 3: a commit
  appends one line and fsyncs). Segment references carry `"enc":4`; the files are kore-store
  version-4 files (`KORE`, version 4: header, dictionary ids and column directory under one CRC,
  every payload under its CRC). Dictionary objects live in `dicts/<sha256>.kdic` (kore-store
  `KDIC`: magic, version, kind, length, content, sha256 of everything before it = the id).
* **Listing.** The commit that first uses a dictionary lists it in its entry:
  `"dicts":[{"id":<sha256 hex>,"file":"dicts/<id>.kdic","bytes":<object length>}]`. The listing
  is part of the hashed entry text (`dict\t<id>\t<file>\t<bytes>` lines after the segment lines;
  entries without `dicts` hash exactly as before), so it cannot be changed or dropped without
  breaking the chain. A segment names its dictionary by id in its checksummed header, and the
  segment's sha256 is in the MANIFEST, so a segment can only be decoded with exactly the object
  it was written with. Rules checked by `verify()` (and the listing rules also by `open`): the
  file name is `dicts/<id>.kdic`, an id is listed once, every listed object exists, has the listed
  length and sha256 = id, every id a segment references is listed at or before that segment's
  version, and `dicts/` holds no unlisted file. A dictionary listing under an older header is
  refused by the parser.
* **Which dictionary.** `DictPolicy` (per handle, not stored; it only affects future writes):
  the first dictionary is built by the first commit after which the log holds at least
  `train_events` events (default 1000), from the newest whole event texts that fit into
  `max_bytes` (default 128 KiB): that commit's own texts first, then earlier stored ones. The
  content is those texts in log order separated by the byte 0xFF (never part of UTF-8), stored
  as a raw-content zstd dictionary. That commit's events segment already uses it. Drift: once
  `check_events` events (default 100,000) have been stored since the current dictionary was
  listed, the next commit with at least `check_min_text` bytes of event text (default 16 KiB)
  is also coded with a candidate built from the texts stored before it; if the candidate makes
  that commit's events segment at least `min_gain_pct` (5 %) smaller it becomes a new object
  (new id) listed by that commit, otherwise the check is postponed by another `check_events`.
  A dictionary is never modified; older segments keep the object they reference, so every
  version stays readable (time travel needs the dictionaries of the versions read, and they
  are all kept). Only `events.text` uses a dictionary; every other column and table is plain
  v4 compact (raw 32-byte ids, delta timestamps, token/dictionary transforms, zstd level 19 for
  payloads below 256 KiB).
* **Reads.** `open` reads and verifies every listed dictionary object, then streams the log as
  for format 2/3 (ids only from events segments; v4 projection of `event_id`, which needs no
  dictionary). `get_event` decodes the one segment that holds the row (whole segment: the v4
  reader has no row-level access). `open_at`, `diff`, `for_each_event` and `peek_events` read
  segments with the listed dictionaries. Query results are identical to formats 1 and 3 for the
  same operations (tests compare the rows of every table at every version and three SQL results).
* **Upgrade is explicit.** `VersionedLog::upgrade_to_v4()` (from format 1, 2 or 3) appends one
  commit with `schema_meta.format_version = 4` and, in the same atomic file replacement, switches
  the header to `... 3`; all earlier entry lines are kept byte for byte, so every old version
  hash is unchanged and the old segments are not rewritten (a mixed log). `LogFormat::V4`
  applies only when a log is created. There is no downgrade: `upgrade_to_v2`/`upgrade_to_v3`
  on a format-4 log return false and write nothing.
* **Older binaries** refuse the log at the header (`MANIFEST is corrupt: manifest header missing
  or wrong`; checked with a binary built before format 4, `docs/log_format4.md`). Their segment
  readers would also refuse a v4 segment (`unsupported version 4`, or "needs shared dictionary").
* **Crash.** The dictionary object is written (temp file, fsync, rename) before the segments
  and the MANIFEST line; a crash before the line leaves an unlisted object, which `open`
  ignores, `verify()` reports as an extra file, and the next format-4 commit or `recover()`
  deletes.
* **forget and dictionaries.** Because a dictionary is made of whole event texts, the texts it
  holds are known exactly. `forget_events` replaces every listed dictionary that holds the text
  of a forgotten event by a new object built from its remaining texts (or drops it if none
  remain), re-encodes every segment that used it, lists the replacement in the entry that listed
  the old one, re-seals the chain from the earliest affected entry, and deletes the old object
  file (all inside the same staged, crash-safe forget). The report counts
  `dictionaries_rebuilt` / `dictionaries_removed`. Afterwards no file of the log contains the
  forgotten text (tested by scanning every file). Not covered: a forgotten text that also occurs
  inside the text of another, still stored event stays there as part of that event, and copies
  outside the log are not touched (as for every format).

## Log format 5 (opt-in): index runs and the fast open (Track D1c)

Format 5 is format 3 (format-2 segments, journal MANIFEST) plus **index runs**: immutable files that hold
the open's index (event id -> ordinal, revision ids, heads, content digests, predictions, outcomes), so an
open with `OpenOptions { fast_open: true }` reads the MANIFEST, the listed runs and the tail instead of every
segment. Design and owner decisions: `docs/d1c_design.md`. Code: `versioned/index_runs.rs`.

* **Shape.** MANIFEST header `KORE-VLOG-MANIFEST 4` (older binaries refuse it). A commit without rows lists the
  complete current run set in the hashed entry field `index_runs` (`from`, `to`, `file`, `sha256`, `bytes`,
  `chain` = hash of entry `to`); every older entry line is unchanged. Files: `index/<from>-<to>.krun`
  (magic `KORERUN1`, layout version 1; seven sorted tables of 44-byte records plus a blob area each).
* **Upgrade is explicit.** `upgrade_to_v5()` from format 1, 2 or 3 (one `schema_meta` commit, header switched in
  the same atomic replacement, earlier lines byte for byte). Format 4 logs are refused. No downgrade.
* **Writing runs.** `maintain_index_runs()`: a new run when there is no usable run in the handle, or 2,000
  versions or 100,000 events have passed since the newest run; above 8 runs the two oldest are merged. Temp
  file, fsync, rename, directory sync, then the listing commit; unlisted files are deleted afterwards and by
  `recover()`. The being tools call it at the end of `being_run` when `fast_open = on`.
* **Fast open.** Used only for a format-5 log; a run is used only if it is in the active listing, its `chain`
  equals the current hash of entry `to` (a `forget` that re-seals `to` makes it stale) and the whole file hashes
  to the listed sha256 (every 4 KB page digest is kept; later page reads are checked against it). 64 segments of
  versions <= the newest used run, chosen by a PRNG seeded from the head hash, are hashed (owner decision D-1);
  the others are checked on every read and by `verify()`, not at open. The tail after the newest run is read and
  verified as before. Any problem (no run, bad hash, unknown layout, irregular tail, sampled segment bad) = the
  full open, with the reason in `open_report().fallback_reason`.
* **verify()** also checks every chain-valid listed run (hash, layout) and that the runs, merged, equal the index
  rebuilt from the segments; an unlisted run file is an `ExtraSegment`.
* **Key.** `fast_open = on` in `controller.txt` (fail closed: anything else is off). Off = the open as before.

## Round 2 changes that do not alter the on-disk shape

The encoder search of the `hz` stage is cheaper (same stream grammar, same decoder; segment
bytes written by the new code differ slightly from those of round 1, old segments decode
unchanged), the entropy stage is skipped for columns of distinct 32-byte ids, `get_event` on a
lazily opened format 2/3 log materialises one row instead of one segment, and re-submitting
events that already exist builds no row. See `docs/format_improvement_r2.md`.

See `docs/format_improvement.md` (round 1) and `docs/format_improvement_r2.md` (round 2) for
measurements and limits.

## Invariants (tested in `versioned/tests.rs`)

* Time travel: `snapshot_hash(v)` and the row content of version `v` are
  identical right after the commit, after later commits, and after reopening.
* Append-only: `manifest(v)` is a byte prefix of `manifest(v+1)`; segment files
  are never rewritten (checksum and mtime unchanged). The one exception is `forget` (below).
* Idempotent ingest: repeating a batch changes neither version count nor hashes.
* `verify()` recomputes every checksum and the whole chain from disk and reports
  a flipped byte, missing or unlisted segment, reordered or edited entries, and a
  truncated manifest, naming the file or version.
* A crash between segment write and manifest rename leaves the log valid at the
  previous version.
* User SQL is limited to one SELECT/WITH statement over an in-memory copy.

## SQL

`register_into(ctx, snapshot)` registers `events`, `revisions`, `revision_causes`,
`predictions`, `outcomes`, `schema_meta` and two views over revisions:
`beliefs_current` (per entity_type+key, the latest revision no other revision
supersedes) and `beliefs_asof` (the latest revision with `valid_from <=
snapshot.valid_at`, not superseded by another such revision; `valid_at` defaults
to the snapshot's `recorded_at`, change it with `with_valid_at`). The views
contain all entity types; filter on `entity_type`. `run_readonly_sql` rejects
anything that is not a single SELECT/WITH (comments stripped, quotes respected,
`;` followed by another statement rejected, DML/DDL keywords rejected anywhere
outside string literals). The engine is the older KORE engine; joins are plain
equi-joins, which is why causes live in the `revision_causes` link table.

## MCP tool surface (M1b)

All tools use the owner of the running instance and operate on `<root>/<owner>/log`
(the owner and path are validated with `safety::validate_owner`/`safe_path`; nothing
is written outside the data root). Each call opens the log from disk and closes it
again; a process-wide mutex serialises log access between the MCP loop, the HTTP/TCP
servers and the heartbeat thread, and the MCP loop runs these tools without holding
the global state lock. A second process writing the same log is detected at commit
and reported as `Conflict` (nothing is written; retry). A damaged log makes every
tool except `self_verify` fail with the verification error.

| tool | arguments | result |
|---|---|---|
| `self_event_add` | `text`, `occurred_at` (required, ISO-8601 UTC), `source?` (default `mcp`), `kind?` (default `note`), `tags?` | `event_id`, `duplicate`, `version` |
| `self_belief_revise` | `key`, `stance`, `confidence` 0..1, `caused_by` (event ids, each must exist), `rationale?`, `valid_from?` (default: newest cited event's `occurred_at`, else now), `entity_type?` | `rev_id`, `version`, `duplicate`, `valid_from`, and a `note` when nothing is cited (stored as unsupported). Author is always `user`. |
| `self_belief_history` | `key`, `entity_type?` | revisions in order with cited events expanded and `supersedes` |
| `self_sql` | `sql`, `as_of_version?` or `as_of_time?` | text table; `as_of_time` picks the latest version recorded by then and also sets the `beliefs_asof` cut-off |
| `self_versions` | none | version, recorded_at, cumulative rows per table, short hash |
| `self_diff` | `v1`, `v2` | rows added in `(v1, v2]` |
| `self_verify` | none | `OK: ...` or `VERIFY FAILED: <exact error naming the file>` |
| `self_migrate_v1` | none | imports the old identity beliefs as `author=legacy` revisions with no causes (`unsupported=1`); topics that already have a belief revision are skipped, so it is safe to repeat |

`self_ingest` additionally appends the memory as an event (`kind=note`,
`occurred_at` = now, `source=self_ingest`, tag = the memory kind); duplicates inside
the same second are no-ops. `self_observe_outcome` is unchanged (it still writes the
old learning history; mapping it to `outcomes` needs a prediction id, which does not
exist yet). The former `self_delta_save` and `self_delta_history` tools were removed:
use `self_versions` and `self_sql` with `as_of_version`.

`author=legacy` is accepted by the library only for imports; tools cannot write
`rule:<id>` or `legacy` revisions.

Query notes for the older KORE SQL engine: single-table SELECTs name their columns
`<table>.<col>`, joins keep alias prefixes (`r.key`), `count(*)` is named `Count(*)`,
joins are equi-joins.

NULL semantics: this copy of the engine now matches `master-kore-engine` in three cases
(targeted fix on branch `dl-sql-fix`, tests in `kore-sql/tests/null_semantics.rs`):
`COUNT(col)` counts only non-NULL values (`COUNT(*)` and `COUNT(1)` still count rows),
an unknown function is an error naming the function as written (it was NULL on every
row), and a window function with no value returns NULL, not NaN. The rest of this copy
is unchanged and older than master (for example, window frames are ignored).

Long term (future work, not done): Digital Life should depend on the master engine
instead of keeping its own copy of `kore-sql`, so that fixes made there reach it.

## Ingestion, redaction and forget (M2a)

Tools `self_ingest_folder`, `self_ingest_csv`, `self_ingest_kore`, `self_export`
and `self_forget` live in `src/ingest_tools.rs`; redaction in `src/redact.rs`;
the CSV reader and writer in `src/csvio.rs`. See the README section "Ingestion
quick start" for real output.

* Allowed input: the owner directory plus folders listed in the hand-edited
  `<owner dir>/ingest_roots`. Paths are canonicalised before the check. The
  file, and `redact_patterns.txt`, are refused by `safety::safe_path`, so no
  tool can read or write them.
* Every event passes through `Redactor` before it reaches `NewEvent`, so the
  redacted text is what the event id is computed from (idempotent as long as the
  patterns do not change). The `redacted` column is set when anything was replaced.
* Dates come from the source (front-matter, heading, file name, CSV column);
  nothing is stamped with the current time. Undated files are skipped and listed;
  CSV rows without a parseable `occurred_at` are rejected and counted.
* `self_ingest_kore` imports an `events` table written by `self_export` (same
  columns as the log); a row whose `event_id` no longer matches its content is
  rejected.

### forget: the one exception to append-only

`VersionedLog::forget_events(ids)` (`versioned/forget.rs`):

1. Every segment of any version holding a row for a forgotten event is rewritten:
   `events` rows are removed; `revision_causes` rows citing the event are dropped
   (the revisions stay; one that lost its last citation reads `unsupported = 1`);
   the id is removed from `predictions.features_cite`; `outcomes.outcome_event_id`
   becomes null. A segment left empty is deleted and its manifest reference dropped.
2. Because the rewrite covers all versions, time travel to an older version no
   longer contains the forgotten text. Nothing can be resurrected from an old
   version, since the old segments are gone.
3. The manifest is re-sealed from the first rewritten version onward (their
   hashes change; the head hash before the re-seal is kept in the forget record),
   and one new commit is appended whose entry carries
   `forget: {removed: [sha256("forget1|" + event_id)], prior_head, resealed_from}`.
   The record is part of the entry hash, so editing it breaks the chain. No
   content and no plain event ids are recorded.
4. The hashes are also a tombstone: `append_events` refuses (counts as
   `suppressed`) an event whose id hash is in a forget record, so re-running an
   ingest does not bring the text back.
5. Semantics: unknown ids are an error and change nothing (all or nothing);
   ids already forgotten are counted and ignored (no new version); the tool
   needs `confirm: true`.

Crash safety: new segments and the new MANIFEST are written and fsynced under
`forget.staging/`, then a `FORGET_PENDING` marker (list of renames and deletes) is
created by temp file plus rename, then the renames, deletes and finally the
MANIFEST rename are applied (each step is idempotent). Opening the log rolls a
marked forget forward and discards an unmarked staging directory, so after a crash
the log is either exactly the old state or the completed new one. This is tested by
stopping at every step (before the marker, after it, and after each apply
operation); it is not tested against real power loss or disk faults. `verify_dir`
alone does not repair a half-applied forget (it reports the mismatch); opening the
log or calling `self_verify` does the recovery first.

What forget does not do: it cannot remove copies outside the log (backups,
filesystem snapshots, earlier exports, text still in unallocated disk blocks, or
anything an LLM client already received), and the manifest hashes of the affected
versions change, so a head hash you recorded elsewhere before the forget no longer
matches (use the new head hash afterwards).

## Learning, rule evolution and the ledger (M2b/M2c)

Code: `src/learn/` (pure detectors and writers), `src/learn_tools.rs` (tools),
`src/synth.rs` and `src/measure.rs` (synthetic ground truth and scoring; tests and the
`measure_learning` example only). Tools: `self_learn`, `self_explain`, `self_evolve_rules`,
`self_ledger_snapshot`, `self_ledger_outcome`. See the README section "Learning quick start".

Prediction (`src/predict/`): `predict_make`, `predict_resolve`, `predict_score` write and score forecast rows in the same predictions/outcomes tables (model id `predict:<predictor>`, the model version ends with `cutoff-v<N>`); refusals are events tagged `predict_refusal`. See the README section "Predictions and calibration (S4)".

* Rule-authored rows: `author` is `rule:habits-v1`, `rule:changes-v1`, `rule:stances-v1` or
  `rule:evolve-v1`; each cites at least one event (`revision_causes`). Only this code
  writes them; no tool argument selects an author.
* Idempotence: a derived row is identified by the content signature (type, key, stance,
  confidence, rationale, valid_from, author, cited events). A row whose signature already
  exists is not written; new events produce only the rows whose evidence changed (a new
  revision superseding the old head). Habit rows key `ctx:<C>|act:<A>`, topic change rows
  `topic:<T>`, stance rows `stance:<topic>`.
* Conventions: events tagged `ctx:`/`act:` (habits), `topic:` (change points); stance phrases
  are matched in the event text. Events tagged `evolve` are the loop's own records and are
  ignored by the learners.
* `entity_type = rule` (library only; `self_belief_revise` refuses it): key `learner-params`
  holds the current rule set as JSON in `stance`; the head is the current set, and
  `rejected:<hash>` keys hold rejected candidates. Each promotion or rejection is also an
  event (`source evolve`, `kind decision`, tags `evolve` and `promotion`/`rejection`/`rollback`)
  whose text holds the scores; the rule revision cites it.
* Rollback = time travel. `self_sql ... as_of_version N` shows the rule set as it was at version
  N; `learn::evolve::rollback_rules(log, N)` (library only) writes it again as a new revision.
  Nothing is deleted.
* The evolution loop reads only the log and writes only log revisions and events. It
  contains no file-system calls, and a test checks that the tool list has no code-writing tool.
* Ledger: `predictions` rows with `model_id rule:habit-frequency`, `question_id
  habit-window:ctx:<C>|act:<A>@<window start date>` (one per habit and window), options
  `yes`/`no`, `made_at` = newest event used (events at or after the window start are
  ignored). Outcomes are attached with the existing `outcomes` table; nothing is scored.

## What is and is not guaranteed

Guaranteed (when the files are only changed through this module): the properties
above, and that any change made by other means to a segment or to the manifest is
detected by `verify()`/open, except as noted below.

Not guaranteed:

* Not encrypted at rest. Files are plain; protect the directory with OS
  permissions. No permissions are set by this module yet.
* Detection is by checksum and hash chain, not signatures: someone who can write
  the directory can rewrite the whole log consistently. Rolling the manifest back
  by exactly whole commits is indistinguishable from a crash unless the head hash
  is recorded elsewhere; use `verify_against(head_hash)` with a hash kept
  externally (unlisted later segments are also reported by `verify()`).
* Single writer per log. A second writer is detected at commit and refused (full byte
  comparison of the MANIFEST for format 1/2, length plus tail for format 3), but
  there is no cross-process lock.
* fsync of the directory is best effort (not available on every platform).
* Durability against hardware or OS faults beyond what fsync gives.
* A format 1 log is loaded in memory when opened; a format 2 log keeps only an id
  index at open and loads rows on first snapshot use. Fine for personal-scale data,
  not tuned for large volumes. Open time and the index size still grow linearly with the
  log (every segment file is read and hashed at open; the index holds every event id,
  every revision id, key and content digest): see `docs/longrun.md`, "Restart fix".
* No automatic scheduling of learning, no prediction scoring yet, no encryption yet. Learning
  is rule-based and measured only on synthetic data (`docs/measured.md`). `forget` exists
  (see above) but only inside this log; redaction before storage is best-effort.
* Content is whatever the caller supplies; the log stores it, it does not judge
  whether it is true. No accuracy claim is made about anything derived from it.
