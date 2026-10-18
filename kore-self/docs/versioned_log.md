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
| `revisions` | rev_id, entity_type (belief/habit/decision), key, stance, confidence (0..1), rationale, supersedes (nullable), valid_from, author (`user`, `legacy` (imported from the old state, never cited) or `rule:<id>`), recorded_at |
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
  MANIFEST                      one JSON line per commit, hash-chained
  events/0000000003.kore        only the rows added by commit 3
  revisions/0000000003.kore
  revision_causes/...  predictions/...  outcomes/...  schema_meta/0000000000.kore
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

## Invariants (tested in `versioned/tests.rs`)

* Time travel: `snapshot_hash(v)` and the row content of version `v` are
  identical right after the commit, after later commits, and after reopening.
* Append-only: `manifest(v)` is a byte prefix of `manifest(v+1)`; segment files
  are never rewritten (checksum and mtime unchanged).
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
* Single writer per log. A second writer is detected at commit and refused, but
  there is no cross-process lock.
* fsync of the directory is best effort (not available on every platform).
* Durability against hardware or OS faults beyond what fsync gives.
* Whole log is loaded in memory when opened; fine for personal-scale data, not
  tuned for large volumes.
* `forget` (deleting or redacting stored content) is not implemented. When it is
  added it will be the only documented exception to append-only.
* Content is whatever the caller supplies; the log stores it, it does not judge
  whether it is true. No accuracy claim is made about anything derived from it.
