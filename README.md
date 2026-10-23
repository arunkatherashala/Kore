# KORE and kore-self

KORE is a columnar SQL engine written in Rust (a workspace of crates). `kore-self` is a local tool built on it: a memory store you query with SQL, exposed as an MCP server.

**Full guide:** [GUIDE.md](GUIDE.md)

---

## kore-self

**Target positioning (the only claim this project makes):** A local, deterministic, append-only journal of how your recorded beliefs, habits and decisions change. Every change cites the events that caused it. You can query any past version with SQL. Its explanations can be ablation-tested, and its predictions are scored against outcomes.

**Status (milestone M2a):** the versioned log is built: events, cited belief revisions, time-travel SQL and tamper-evident verification (see "Versioned self: quick start"), plus opt-in ingestion of journal folders, CSV and .kore files with best-effort redaction, export, and a documented forget (see "Ingestion quick start"). Nothing derives beliefs automatically yet, there is no predictor in the log yet, and no accuracy is claimed. Besides that log, kore-self is a local memory store with SQL over it, simple keyword/word-frequency counters, and a keyword-to-choice count predictor that has not been validated against outcomes. No accuracy number is claimed anywhere; any future number must have a row (n, baseline, interval, date) in `docs/measured.md`.

### What it does today

- Stores memories (content, kind, importance, tags) on your machine.
- Runs KORE SQL over them (`self_query`, `self_dml`, `self_save`, `self_load`).
- Keeps simple counters: word frequency and trend, keyword to choice counts, recorded belief changes.
- Serves all of this over MCP (stdio), an optional local HTTP API and an optional local TCP JSON-RPC daemon.
- Starts **empty**. The author's own biography is available only as demo data with `--demo-seed` (or `KORE_SELF_DEMO_SEED=1`).

### Tools

This table is generated from the tool registry (`kore-self --tool-table`); a test fails if it drifts from the code.

<!-- tools:begin -->
| Tool | Description |
|---|---|
| `self_ingest` | Store a memory (content, kind, importance) |
| `self_research_claim` | Store a source-grounded research claim; evidence, source, confidence, and status are mandatory |
| `self_plan_problem` | Create a verification-gated life plan: understand, research, hypothesize, test, conclude |
| `self_plan_verify_step` | Advance the active life-plan step only when verification evidence is supplied |
| `self_recall` | Search memories by keyword relevance |
| `self_reason` | Retrieve evidence from your memory and return confidence, uncertainty, and a next action |
| `self_context` | Build a plain-text context block from your memories and keyword-count profile, to paste into any LLM |
| `self_stats` | Memory count, kinds breakdown, analysis counters, profile summary, disk usage |
| `self_world_events` | Query events recorded in the world model, filtered by actor |
| `self_world_causality` | Query the causal chain of recorded events starting from an event ID |
| `self_observe_outcome` | Record an action outcome to learn from experience |
| `self_update_belief` | Update confidence in a belief based on evidence |
| `self_learning_accuracy` | Show prediction accuracy, learning cycles, and belief confidence drift (simple counters over recorded outcomes) |
| `self_identity` | Keyword-count profile: top values, thinking-style counters, tracked beliefs |
| `self_belief` | Track a belief |
| `self_dream` | Word-frequency and trend analysis over all memories: repeated topics, early-versus-late shifts, clusters, time patterns |
| `self_shadow` | Passive usage report: tool usage counts, implicit interests, knowledge gaps |
| `self_patterns` | Patterns found by the word-frequency analysis (self_dream) |
| `self_predict` | Keyword-to-choice count predictor: given a context, returns the most frequent past choice |
| `self_contradictions` | List recorded belief changes (moments when a decision or belief was reversed) |
| `self_decisions` | Learned keyword-to-choice counts from your decision memories |
| `self_query` | Run KQL (KORE SQL) on your memories: DISTINCT, CTEs, window functions, joins, CASE WHEN, HAVING, UNION ALL |
| `self_dml` | INSERT INTO / UPDATE / DELETE FROM / CREATE TABLE AS SELECT against named in-session tables |
| `self_save` | Save memories to a native .kore file inside your data directory ([data root]/[owner]/) |
| `self_load` | Load a .kore file from your data directory ([data root]/[owner]/) into a named table for self_query |
| `self_distributed_query` | Run SQL with the Rayon-parallel engine |
| `self_context_sync` | Write a generated copilot-instructions.md (from your memories and profile) into your data directory (default context/copilot-instructions.md) |
| `self_brief` | Template-based briefing: recent work, tracked goals, observed patterns, suggestions |
| `self_pushback` | Decision pushback (does NOT push to GitHub or any remote): compares a decision you describe with your own recorded past decisions, beliefs and keyword-choice counts and lists matches |
| `self_remind` | Set a reminder that is listed in self_brief, mark one done, or list all |
| `self_goals` | Show tracked goals with importance levels |
| `self_read_source` | Read-only: list kore-self's own source files, tool names and line counts |
| `self_plan_feature` | Read-only text suggestion: compares topics in your memories with existing tools and suggests a feature |
| `self_tables` | List SQL tables in this session: memories plus tables created via self_dml or loaded via self_load |
| `self_describe` | Show the schema of a SQL table: columns, types, sample values, row count |
| `self_metrics` | Engine status: tables loaded, total rows, memory count, SQL capabilities |
| `self_version` | Version, tool count and pointer to the engine benchmark documentation |
| `self_sql_explain` | Run SQL and get a short template-based description of the result shape |
| `self_event_add` | Append a dated event to the versioned log (idempotent: the same source, time and text is stored once) |
| `self_belief_revise` | Record a new revision of a belief, habit or decision, citing the events that caused it (caused_by) |
| `self_belief_history` | Show every revision of a key in order, with the cited events expanded (time, source, text) and the supersedes chain |
| `self_sql` | Run one read-only SELECT/WITH over a snapshot of your versioned log (latest, or as_of_version / as_of_time) |
| `self_versions` | List the commits of the versioned log: version, recorded_at, cumulative row counts, short hash |
| `self_diff` | Show the rows added between two versions of the versioned log (v1 exclusive, v2 inclusive) |
| `self_verify` | Recompute every checksum and the hash chain of the versioned log from disk; reports OK or the exact failure and file |
| `self_migrate_v1` | Import beliefs from the pre-log identity state as author=legacy revisions marked unsupported (no causes are invented) |
| `self_ingest_folder` | Read .md and .txt files under an allow-listed folder and store one dated note event per file (idempotent, redacted before storing) |
| `self_ingest_csv` | Import events from a CSV file inside an allow-listed folder; rows without a parseable occurred_at are rejected and counted, never dated now |
| `self_ingest_kore` | Import events from a .kore file written by self_export (idempotent, validated, redacted again before storing) |
| `self_forget` | Permanently remove events from every version of the log, the one documented exception to append-only; requires confirm true |
| `self_export` | Write all events to a CSV or .kore file under the exports folder of your data directory |
| `self_learn` | Derive habits, topic change points and explicit stance statements from the event log with deterministic rules; each derived revision is authored by a rule and cites its events (idempotent, incremental) |
| `self_explain` | Show one revision with the events it cites (the provenance); rev_id may be a unique prefix of at least 8 characters |
| `self_evolve_rules` | Propose a neighbouring set of learner parameters, backtest it on the past of the log (rolling origin) against the current set and a trivial baseline, and promote it only if it wins; every promotion or rejection is logged with its scores |
| `self_ledger_snapshot` | Record frequency-baseline predictions (action in context within the next 7 days) for each habit with enough data, using only events before the window start; one row per habit and window, never scored here |
| `self_ledger_outcome` | Attach an outcome (yes or no) to a stored prediction, optionally citing the event that shows it; no score is computed |
<!-- tools:end -->

### Versioned self: quick start

The versioned log lives in `<root>/<owner>/log/` (see `kore-self/docs/versioned_log.md`). Every call below is an MCP `tools/call`; shown here as tool name, arguments and the real output of the end-to-end test (`kore-self/tests/e2e_versioned.rs`, run from an empty temp data root). Events are added with a required past `occurred_at`; a belief revision cites event ids.

```
self_event_add {"text": "Reviewed a pull request that mixed tabs and spaces", "occurred_at": "2024-01-03", "source": "e2e", "kind": "note"}
  -> {"duplicate":false,"event_id":"c271b5ed...","version":1}
  (20 such events were added; adding the same one again returns "duplicate":true and creates no version)

self_belief_revise {"key": "indent_style", "stance": "tabs", "confidence": 0.55, "caused_by": [<3 event ids>]}
  -> {"cited_events":3,"duplicate":false,"rev_id":"20375df3...","valid_from":"2024-01-29T00:00:00Z","version":8}
self_belief_revise {"key": "indent_style", "stance": "spaces", "confidence": 0.8, "caused_by": [<3 event ids>]}
  -> {"cited_events":3,"duplicate":false,"rev_id":"71914910...","valid_from":"2024-03-11T00:00:00Z","version":16}
self_belief_revise {"key": "indent_style", "stance": "spaces, enforced by the formatter", "confidence": 0.9, "caused_by": [<3 event ids>]}
  -> {"cited_events":3,"duplicate":false,"rev_id":"cb9b8662...","valid_from":"2024-05-06T00:00:00Z","version":23}
```

Query the same question at three past versions:

```
self_sql {"sql": "SELECT key, stance, confidence FROM beliefs_current", "as_of_version": 8}
beliefs_current.key | beliefs_current.stance | beliefs_current.confidence
indent_style        | tabs                   | 0.55

self_sql {..., "as_of_version": 16}
indent_style        | spaces                 | 0.8

self_sql {..., "as_of_version": 23}
indent_style        | spaces, enforced by the formatter | 0.9

self_sql {"sql": "SELECT count(*) FROM events", "as_of_version": 8}    -> 7
self_sql {..., "as_of_version": 16}                                   -> 14
self_sql {..., "as_of_version": 23}                                   -> 20
```

Which events support the "spaces" revision (join output columns keep their alias prefix):

```
self_sql {"sql": "SELECT r.stance, e.occurred_at, e.text FROM revisions r JOIN revision_causes c ON r.rev_id = c.rev_id JOIN events e ON c.event_id = e.event_id WHERE r.stance = 'spaces'"}
r.stance | e.occurred_at        | e.text
spaces   | 2024-03-11T00:00:00Z | Editorconfig merged, spaces enforced
spaces   | 2024-02-14T00:00:00Z | Merge conflict caused by mixed indentation
spaces   | 2024-02-20T00:00:00Z | Team agreed on spaces in the contributing guide
(3 rows)
```

Integrity:

```
self_verify {}
  -> OK: 24 version(s), latest 23, head hash 705f7bfa571e1692e71bae8dc6fa8b3847e2ac36fffda960d4868d4e97220ec1
(after flipping one byte in events/0000000003.kore)
self_verify {}
  -> VERIFY FAILED: checksum mismatch in segment: events/0000000003.kore
```

Other tools: `self_belief_history {key}` (revisions in order, cited events expanded, supersedes chain), `self_versions`, `self_diff {v1, v2}`, `self_migrate_v1` (imports old identity beliefs as `author=legacy`, unsupported, safe to repeat). `self_ingest` also appends a `note` event (source `self_ingest`, occurred_at = now). Tools can only write `author=user` revisions. Run the whole flow with `kore-self/demo/run_demo.sh` or `run_demo.ps1`.

### Ingestion quick start

Local files can be turned into dated events. Nothing is read unless you allow it, nothing is dated "now", and text is redacted before it is stored.

**1. Allow a folder (by hand).** Input may only come from your owner directory (`<root>/<owner>/`) or from folders listed, one absolute path per line, in the file `<root>/<owner>/ingest_roots`. Edit that file yourself in a text editor. No tool can read or write it (`safe_path` refuses it for `self_save`, `self_load`, `self_context_sync`, `self_export` and the others). Optional: `<root>/<owner>/redact_patterns.txt`, one plain substring per line (case-insensitive, `#` comments), is also hand-edited and also unreachable by tools.

**2. Ingest.** Real output of `kore-self/tests/e2e_ingest.rs` (temp data root, 10 dated journal files, 1 file with an e-mail address and a phone number, 2 undated files; temp paths and long note texts shortened with `...`):

```
self_ingest_folder {"path": "<other folder not on the allow-list>"}
  -> Rejected path '...': it is not inside an allowed ingest root. Allowed: ...\e2e | ...(journal dir) | ...(csv dir). To allow another folder, add its absolute path as a line in ...\e2e\ingest_roots (edit that file by hand; no tool can change it).

self_ingest_folder {"path": "<journal dir>", "dry_run": true}
  -> {"added":0,"dry_run":true,"duplicate":0,"errors":0,"files_scanned":13,"redacted":2,"skipped_symlinks":0,"skipped_undated":2,"suppressed_forgotten":0,"truncated_at_file_cap":false,"undated_files":["ideas/loose-notes.txt","scratch.md"],"version":0,"would_add":11,...}

self_ingest_folder {"path": "<journal dir>"}
  -> {"added":11,"duplicate":0,"errors":0,"files_scanned":13,"redacted":2,"skipped_undated":2,"undated_files":["ideas/loose-notes.txt","scratch.md"],"version":1,...}
self_ingest_folder (second run)
  -> {"added":0,"duplicate":11,"errors":0,"files_scanned":13,"redacted":2,"skipped_undated":2,"version":1,...}

self_sql {"sql": "SELECT source, text, redacted FROM events WHERE redacted"}
events.source                 | events.text                                                  | events.redacted
journal:2024-03-05-entry.md   | # Day 5 The secret plan about [REDACTED:custom]-7731 was di~ | true
journal:2024-03-20-contact.md | Asked [REDACTED:email] to call back on [REDACTED:phone].     | true
(2 rows)
```

A file is dated by, in order: a front-matter `date:` line, the first heading that contains an ISO date, a file name starting with `YYYY-MM-DD`. Undated files are skipped and listed in the reply, never stamped with today's date. One event per file (`kind` note, source `journal:<relative path>`, tag `journal`). Limits: .md and .txt only, 2000 files, 512 KiB per file, hidden folders skipped, symlinks that leave the folder skipped. `self_ingest_csv` takes a mapping (`occurred_at` and `text` required, `source`, `kind`, `tags` optional; header names, or column numbers with `has_header: false`) and rejects rows without a parseable date:

```
self_ingest_csv {"path": "<csv dir>/backfill.csv", "mapping": {"occurred_at": "when", "text": "what", "source": "where", "kind": "type", "tags": "labels"}}
  -> {"added":3,"duplicate":0,"error_details":["row 3: occurred_at '' is missing or not a date (YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ); row rejected"],"errors":0,"redacted":0,"rejected_no_date":1,"rows":4,"version":2,...}
(second run) -> {"added":0,"duplicate":3,...,"rejected_no_date":1,"rows":4,"version":2,...}
```

**3. Forget one event.** `self_forget {event_id | source_prefix, confirm: true}` is the one documented exception to append-only. It removes the event rows from every segment (so old versions no longer contain the text), drops the citations that pointed at them (those revisions stay and show `unsupported`), re-seals the hash chain and appends a `forget` record that holds only hashes of the removed event ids. Re-ingesting the same text is refused. Copies outside the log (backups, earlier exports, disk snapshots) are not touched.

```
self_forget {"source_prefix": "journal:2024-03-05"}
  -> self_forget permanently removes 1 event(s) from every version of the log (the one exception to append-only) and cannot be undone. Call again with confirm: true to proceed.
self_forget {"event_id": "0000", "confirm": true}
  -> not found: event 0000
self_forget {"event_id": "<id>", "confirm": true}
  -> {"already_forgotten":0,"causes_dropped":0,"forgotten":1,"outcomes_edited":0,"predictions_edited":0,"revisions_now_unsupported":[],"segments_removed":0,"segments_rewritten":1,"version":3,...}
self_forget (again)   -> {"already_forgotten":1,"forgotten":0,"version":3,...}
self_verify           -> OK: 4 version(s), latest 3, head hash 753121ae50f48e64958392a52e424e22c1ebe1919533de4723a07e29bb141d7b
self_ingest_folder (after the forget)
  -> {"added":0,"duplicate":10,"suppressed_forgotten":1,...,"version":3}
(after flipping one byte in events/0000000001.kore)
self_verify           -> VERIFY FAILED: checksum mismatch in segment: events/0000000001.kore
```

**4. Export and re-import.** `self_export {path, format: csv|kore}` writes all events (not beliefs or predictions) under `<root>/<owner>/exports/` only; `..` and absolute paths elsewhere are rejected. `self_ingest_kore {path}` reads a `.kore` export back (rows whose id no longer matches their content are rejected), idempotent. The e2e test imports the exports into a second owner:

```
self_export {"path": "events.csv", "format": "csv"}
  -> {"bytes":2546,"events":13,"format":"csv","path":"...\e2e\exports\events.csv","relative":"events.csv","sha256":"3e11c646...","version":3,...}
self_export {"path": "events.kore", "format": "kore"}
  -> {"bytes":1992,"events":13,"format":"kore","relative":"events.kore","sha256":"e2fb7610...","version":3,...}
self_export {"path": "../outside.csv", "format": "csv"}
  -> Rejected path '../outside.csv': exports are written only under ...\e2e\exports
(second owner, allow-list lists the first owner's exports/)
self_ingest_kore {"path": ".../exports/events.kore"}   -> {"added":13,"duplicate":0,"errors":0,"rows":13,"version":1,...}
self_ingest_kore (again)                                -> {"added":0,"duplicate":13,"errors":0,...}
self_ingest_csv of the CSV export (same content)        -> {"added":0,"duplicate":13,"errors":0,"rejected_no_date":0,"rows":13,...}
```

**Redaction is best-effort.** Before storage, event text from `self_event_add`, `self_ingest` (log event only), the folder, CSV and .kore doors goes through hand-written matchers for e-mail addresses, phone-like numbers, card-like digit runs that pass the Luhn check, US SSN patterns (`123-45-6789`) and your `redact_patterns.txt` substrings; matches become `[REDACTED:email|phone|card|ssn|custom]` and the event's `redacted` flag is set. It will miss names, addresses, unusual number layouts and anything written in words, and it can redact harmless text. It is not a compliance tool. Changing `redact_patterns.txt` only affects events stored afterwards. Memories stored by the older `self_ingest` memory store are not redacted. In `self_sql`, filter the flag with `WHERE redacted` (the older SQL engine does not accept `redacted = true`).

Not tested or not done: very large folders or files beyond the caps, concurrent writers while a forget runs (a second process is detected as a conflict, not tested under load), crash safety of `self_forget` on real hardware faults (tested only with simulated crashes at each step), Unicode normalisation differences between file systems, and symlink handling on Windows without the privilege to create links (the escape test skips itself there).

### What this does NOT do

- It does not think, understand, or know you. Its "profile" values are counters over the words in your memories.
- It does not predict reliably. The predictor counts keyword to choice matches and is not scored against outcomes yet.
- It does not run any language model and makes no network calls on its own.
- It does not write or modify source code. The former code-generating tool was removed.
- It does not push anything to GitHub or any remote (`self_pushback` only compares a decision with your own recorded history).
- It does not encrypt data at rest (see below).
- It does not learn automatically: beliefs enter the log only when you (or a tool call) record them. No automatic learning yet.
- It makes no predictions from the log yet (the tables exist; nothing writes them). No prediction yet.
- The log is not encrypted. No encryption yet.
- Forgetting exists (`self_forget`) but only inside the log: copies in backups, earlier exports or disk snapshots are not removed. Redaction before storage is best-effort and will miss things.
- It does not judge whether recorded events or stances are true, and a hash chain detects edits but is not a signature (someone who can write the folder can rewrite the whole log consistently).
- The old author-written narration layers (needs, story, worldview and similar generated prose) are off by default. They are available only if you build with `--features legacy-narration`, and they are not evidence of anything.

### Data and privacy

- Data root: `$KORE_SELF_HOME`, otherwise `~/.kore-self`. Each owner has its own folder `<root>/<owner>/`.
- Files are **unencrypted at rest**. On Unix they are created with mode 0600 (directories 0700). On Windows they inherit the permissions of your profile folder. Encryption is planned (milestone M4). Do not commit the data folder.
- Owner names accept only letters, digits, `_`, `-` and `.` (max 64 characters).
- Ingestion reads only from your owner directory and the folders you list by hand in `ingest_roots`; exports are written only under `<root>/<owner>/exports/`. `ingest_roots` and `redact_patterns.txt` cannot be read or written by any tool.
- Every path argument (`self_save`, `self_load`, `self_context_sync`, `/load` in the HTTP API) is resolved inside `<root>/<owner>/`. `..`, absolute paths elsewhere and symlink escapes are rejected.
- SQL `COPY` and `LOAD TABLE` (which read arbitrary files) are disabled in the tools and the HTTP API.

### Network safety

- `api` and `live` modes bind `127.0.0.1` by default.
- A non-loopback address needs both `--bind <addr>` and the environment variable `KORE_SELF_TOKEN` (at least 16 characters); otherwise the process refuses to start.
- When `KORE_SELF_TOKEN` is set, every HTTP request needs `Authorization: Bearer <token>`, and every JSON-RPC line on the TCP daemon needs `"authorization": "Bearer <token>"`. Tokens are compared in constant time.
- No CORS headers are sent.

```bash
cargo run -p kore-self -- arun                    # MCP over stdio (default owner "arun")
cargo run -p kore-self -- arun api 8080           # HTTP API on 127.0.0.1:8080
KORE_SELF_TOKEN=... cargo run -p kore-self -- arun api 8080 --bind 0.0.0.0
cargo run -p kore-self -- arun --demo-seed        # opt-in demo data
```

MCP configuration:

```json
{
  "mcpServers": {
    "kore-self": { "command": "C:/path/to/kore-self.exe", "args": ["arun"] }
  }
}
```

---

## KORE SQL engine

The SQL engine supports SELECT with joins, GROUP BY/HAVING, ROLLUP/CUBE, CTEs, subqueries, window functions, set operations, date and string functions, DML (INSERT, UPDATE, DELETE, CREATE TABLE AS, MERGE), COPY from CSV/Parquet/.kore, native `.kore` files and a Delta-style log.

Engine benchmark results are documented on branch `master-kore-engine`. This branch makes no performance claims.

---

## Build and test

```bash
cargo build -p kore-self
cargo test  -p kore-self                       # uses a temporary data root; never touches ~/.kore-self
cargo build -p kore-self --features legacy-narration   # optional, off by default
```

Tests set `KORE_SELF_HOME` to a temporary directory; in test builds the data root refuses to resolve to the real `~/.kore-self`.

## Author

Sai Arun Kumar Katherashala, GitHub [@arunkatherashala](https://github.com/arunkatherashala)

## License

MIT
