# KORE and kore-self

KORE is a columnar SQL engine written in Rust (a workspace of crates). `kore-self` is a local tool built on it: a memory store you query with SQL, exposed as an MCP server.

**Full guide:** [GUIDE.md](GUIDE.md)

---

## kore-self

**Target positioning (the only claim this project makes):** A local, deterministic, append-only journal of how your recorded beliefs, habits and decisions change. Every change cites the events that caused it. You can query any past version with SQL. Its explanations can be ablation-tested, and its predictions are scored against outcomes.

**Status (milestone M2b/M2c, plus the being loop S1/S2):** a deterministic simulation loop with its own log (see "Being loop quick start");  deterministic rule-based learning (habits, topic change points, explicit stance statements), a rule-parameter evolution loop and the start of a prediction ledger are built (see "Learning quick start"), measured only on synthetic data. The versioned log is built: events, cited belief revisions, time-travel SQL and tamper-evident verification (see "Versioned self: quick start"), plus opt-in ingestion of journal folders, CSV and .kore files with best-effort redaction, export, and a documented forget (see "Ingestion quick start"). Besides that log, kore-self is a local memory store with SQL over it, simple keyword/word-frequency counters, and a keyword-to-choice count predictor that has not been validated against outcomes. No accuracy number is claimed anywhere; any future number must have a row (n, baseline, interval, date) in `docs/measured.md`.

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
| `self_tag_rules_check` | Read-only: show how your hand-edited tag_rules.txt (keyword rules for topics, actions and contexts in free-text events) matches the log: events and days matched per rule, hits dropped by the negation guard, day-level occasions, and a sample of events no rule matched |
| `being_run` | Run the being loop for N steps in a deterministic simulated world (arithmetic, algebra, physics formulas, sequences) with a seeded budget; the being picks the domain by learned reward per energy (or, with chooser model, picks one of 4 offered tasks and a solver strategy from a world model learned from its own log) and every decision, action and outcome is appended to its own log |
| `being_status` | Read-only: the being's steps, budget, per-domain selections and reward per energy, its last decisions with their reasons, and a check that every decision can be re-derived from the log |
| `being_explain` | Read-only: for one decision event, show why the domain was chosen (counts, mean reward per energy, exploration bonus and score of every alternative), the events it cited as evidence, the outcome, and a replay that re-derives the choice from the log |
| `being_rules` | Read-only: the rules the being has discovered by search over its own logged examples (author-defined grammar): form, signature, training and held-out support, the cited example events, how often each was used, and a check that each rule is re-derived from the log alone |
| `chat_say` | Say something to the being and get its reply |
| `chat_history` | Read-only: the last messages of a conversation (yours, the being's replies and tool results), from the log |
| `chat_pending` | Read-only: tool calls the thinker proposed that need your approval (listed under L3 in autonomy_policy.txt) and that you have not decided yet |
| `chat_approve` | Approve or reject one pending tool call by id (from chat_pending) |
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

### Learning quick start

`self_learn` derives habits, topic change points and explicit stance statements from the event log with fixed rules. No language model, no randomness, no network, no automatic scheduling: it runs only when you call it. Every derived row is a revision authored `rule:<id>` that cites the events it came from; tools cannot choose that author. All accuracy numbers (`docs/measured.md`) come from synthetic data only.

Conventions the rules read: an event tagged `ctx:<context>` is one occasion of that context and its `act:<action>` tag is what was done (habits: Wilson 95% lower bound of the action's share, refused below `min_n` occasions); `topic:<name>` tags are counted per day and scanned for rate changes (Poisson likelihood ratio); sentences such as "I now prefer X for T", "I now think T is ...", "I changed my mind about T", "From now on I will ..." become candidate belief revisions (fixed confidence 0.6, not calibrated). Re-running changes nothing; new events add only the rows whose evidence changed.

Real output of `kore-self/tests/e2e_learn.rs` (temp data root, a synthetic 150-day life with seed 11 and 20% noise written to a CSV and ingested through `self_ingest_csv`; long values shortened with `...`):

```
self_ingest_csv -> {"added":2837,"duplicate":0,"errors":0,"redacted":0,"rejected_no_date":0,"rows":2837,...}
self_learn {"dry_run": true}
  -> {"counts":{"changes":{"derived":3,"would_add":3},"habits":{"derived":2,"would_add":2},"stances":{"derived":6,"would_add":6}},"derived":[{"author":"rule:stances-v1","confidence":0.6,"key":"stance:remote-work","stance":"now prefers tea","detail":{"cue":"prefer","evidence":["87d25640e722..."],...},"new":true,"rev_id":null},...
self_learn
  -> counts {"changes":{"added":3,"derived":3},"habits":{"added":2,"derived":2},"stances":{"added":6,"derived":6}}
     insufficient_data {"habit_contexts":[{"context":"conference","min_n":8,"occasions":3},{"context":"retreat","min_n":8,"occasions":4}],"topics":[]}
self_learn (second run)
  -> counts {"changes":{"added":0,"already_present":3},"habits":{"added":0,"already_present":2},"stances":{"added":0,"already_present":6}}
self_explain {"rev_id": "<the morning/run habit revision>"}
  -> {"author":"rule:habits-v1","cited_event_count":123,"cited_events":[{"event_id":"b0071742bc63...","occurred_at":"2024-01-01T06:00:00Z","tags":"ctx:morning,act:run","text":"context morning: did run"},...
```

Only 2 of the 3 planted habits were found in this run (the third falls under the 0.5 lower bound at 20% noise); that is the detector's measured behaviour, see `docs/measured.md`.

**Evolving the rules (never the code).** The learner's thresholds (`habit_min_n`, `habit_lb`, `habit_window_days`, plus the fixed change-point settings, `habit_lift` and `habit_max_share`; the loop only moves the first three) are a rule set stored as revisions with `entity_type = rule`, key `learner-params`; only the library writes them. `self_evolve_rules` proposes a neighbouring set (one parameter moved), backtests it with a rolling origin on the past of the log (events before the cut only choose the candidate; a test proves later events are invisible to that choice), and promotes it only if its next-period habit F1 beats both the current rules and a trivial most-frequent-action baseline by a margin of 0.02. Promotions and rejections are both recorded as a `decision` event plus a rule revision with the scores. Rollback is time travel: read an earlier rule set with `self_sql` `as_of_version` (the library function `rollback_rules` re-instates one as a new revision; no tool exposes it).

```
self_evolve_rules {"dry_run": true}
  -> {"candidate":{"habit_lb":0.4,"habit_min_n":8,...},"candidates_tried":13,"cut_date":"2024-04-14","evaluation":{"baseline":{"f1":0.491,"fn":0,"fp":27,"tp":13},"candidate":{"f1":0.929,"fn":0,"fp":2,"tp":13},"incumbent":{"f1":0.87,"fn":3,"fp":0,"tp":10},"origins":5},"reason":"candidate F1 0.929 beats incumbent 0.870 and baseline 0.491 by at least 0.020","recorded":null,"selection":{"candidate":{"f1":0.917,...},"incumbent":{"f1":0.7,...},"origins":4},"verdict":"promoted"}
self_evolve_rules   -> same decision, "recorded" now holds the event id, rule revision id and version
self_sql {"sql": "SELECT key, author, confidence FROM beliefs_current WHERE entity_type = 'rule'"}
beliefs_current.key | beliefs_current.author | beliefs_current.confidence
learner-params      | rule:evolve-v1         | 0.9285714285714286
```

That is one run on one synthetic life, not evidence that the loop helps in general.

**Prediction ledger (start only).** `self_ledger_snapshot` writes, for each habit with enough data, a frequency-baseline prediction "action X in context C within the next 7 days" (probability = Wilson-smoothed frequency), using only events before the window start, stamped with that cutoff and written once per habit and window. `self_ledger_outcome` attaches an outcome later. Nothing is scored and no accuracy is claimed.

```
self_ledger_snapshot
  -> {"cutoff":"2024-05-29T06:14:48Z","model":"rule:habit-frequency wilson-smoothed-v1","resolves_by":"2024-06-06T00:00:00Z","window_start":"2024-05-30","written":[{"action":"podcast","context":"commute","k":89,"n":155,"pred_id":"de14cfa54c4c...","probability_yes":...},...],"version":4,...}
self_ledger_snapshot (again)  -> {"already_present":3,"written":[],...}
self_verify                   -> OK: 5 version(s), latest 4, head hash b41357224d367baf384f935f8a6f021f4be92a1ed6a3263abfaa474f6b239804
(after flipping one byte in revisions/0000000002.kore)
self_verify                   -> VERIFY FAILED: checksum mismatch in segment: revisions/0000000002.kore
```

Limits: the change-point test assumes Poisson counts; real writing is burstier, so the measured false-positive rate is a floor. A habit that stops meeting the bound is not retracted by a new revision (re-learning from a log without the evidence simply does not reproduce it). A late-arriving event dated earlier than existing revisions gets a correct `valid_from` but its `supersedes` link follows write order. The stance grammar is small and English only. Changing parameters writes new revisions (the rationale names the parameters).

### Free-text journals: keyword rules (`tag_rules.txt`)

Journal files ingested with `self_ingest_folder` are plain text with no `ctx:`/`act:`/`topic:` tags, so habits and topic changes find nothing in them (stances work, they read the text). To bridge that without a language model you can write keyword rules by hand in `<owner dir>/tag_rules.txt`. Like `ingest_roots` and `redact_patterns.txt`, no tool can read or write this file, and it is never ingested.

```
# <owner dir>/tag_rules.txt
action:gym = gym, workout          # an action that can be taken
ctx:morning = morning              # a context that holds (keywords)
topic:cooking = cooking, recipes   # a mention of a topic
negation = actions                 # actions (default) | all | off
builtin_contexts = on              # on (default) | off
```

* Matching is case-insensitive and whole-word (hand-written, no regex; unicode letters and digits form words, `gym's` matches `gym`, a keyword may be several words). Rules are applied when `self_learn` reads the log; stored events and stored tags are never rewritten, so you can edit the file and run `self_learn` again. Derived rows carry a fingerprint of the rules in their rationale, so a changed rule set writes new rows (old ones stay, as always) and an unchanged one writes nothing. Without the file, or with no matching rule, `self_learn` behaves exactly as before (tested).
* Topics: an event whose text matches a `topic:` rule counts as one mention that day; the change scan is unchanged.
* Habits (what counts as an occasion): `ctx:weekday` and `ctx:weekend` are built in (from the UTC date of the event; no keywords) and are on whenever an `action:` rule exists. Their occasion is a calendar day with at least one event (days without events are not counted as "did nothing"); the action counts if any event that day matches it. A keyword context `ctx:C` has one occasion per event with a sentence matching C; the action counts if its keywords match in the same sentence (`Morning: went to the gym.`). Sentences end at `. ! ? ;` or a line break.
* Habits (what is refused). A context needs `habit_min_n` occasions. A rule-derived context that holds on more than `habit_max_share` (default 0.9) of its units (days for weekday/weekend, events for keyword contexts) is dropped as uninformative. A habit must also pass a **lift test**: the in-context Wilson lower bound must exceed the Wilson upper bound of the same action's rate in all other contexts of the same family (stored tags, built-in date contexts, keyword contexts) plus `habit_lift` (default 0.0; negative switches it off). For weekday that comparison is exactly the weekend. Without it, a frequent action such as `run` looked like a "weekday habit" in dense logs even though it happens just as often on weekends; with fewer than `habit_min_n` out-of-context occasions the test cannot be made and the habit is kept. The parameters live in the learner rule set (`self_learn` also accepts `habit_lift` and `habit_max_share` as overrides) and `self_learn` reports `uninformative_contexts` and `lift_rejected` (with both rates) next to `insufficient_data`.
* Negation guard (best effort): a keyword is ignored if one of the 4 words before it in the same sentence is not, no, never, without, skipped, missed, cancelled, avoided or an `...n't` word. It catches "did not go to the gym" and "skipped gym", it misses "the gym was closed" or "gym was cancelled", and it wrongly suppresses "did not quit going to gym". Default: actions only.
* `self_tag_rules_check` (read-only) shows, per rule, how many events and days matched, how many hits the guard dropped, the day-level occasions per context and action, and a sample of events no rule matched, so you can tune the file.
* Cited evidence: a derived habit cites the events that matched the action, a topic change cites all events that matched the topic.
* `self_evolve_rules` and `self_ledger_snapshot` read the log through the same rules as `self_learn` (predictions, the backtest truth and the baseline all use the virtual tags; there is a test, but their scores on free text are not measured).

Real output of a run of the demo (a fake, invented 90-day journal of 90 dated .md files written by a script: gym before work on about 85% of weekdays, never on weekends; "rust compiler work" in the evenings until day 45, then "cooking new recipes"; one sentence "I now prefer cooking over coding in the evenings."; fake data root and fake home in a temp folder; script at `kw/ab/demo_life_m2d.py` outside this repo). Before writing the rules file:

```
self_learn {"dry_run": true}
  counts: changes derived 0, habits derived 0, stances derived 1
  stances  stance:cooking | now prefers cooking over coding in the evenings | conf 0.6
```

After writing the three-line rules file `action:gym = gym, workout`, `topic:rust = rust, compiler`, `topic:cooking = cooking, recipes` by hand:

```
self_tag_rules_check -> action:gym matched 56 events on 56 days; weekday: 64 days, gym on 56 (0.875); weekend: 26 days, no gym;
                        83 of 90 events matched by some rule, 7 unmatched
self_learn
  -> counts {"changes":{"added":2,"derived":2},"habits":{"added":1,"derived":1},"stances":{"added":1,"derived":1}}
  changes topic:cooking | mentions of topic 'cooking' changed: 0.00/day to 0.76/day from 2026-08-15 (statistic 47.1) | conf 1.0
  changes topic:rust    | mentions of topic 'rust' changed: 0.78/day to 0.00/day from 2026-08-15 (statistic 48.5) | conf 1.0
  stances stance:cooking | now prefers cooking over coding in the evenings | conf 0.6
  habits  ctx:weekday|act:gym | in context 'weekday' does 'gym' (56 of 64 occasions, rate 0.875, Wilson 95% lower bound 0.772) | conf 0.772
          out_of_context {'k': 0, 'n': 26, 'wilson_upper': 0.129}   (the weekend: 0 gym days of 26)
self_learn (again) -> everything already_present, nothing added
self_verify -> OK: 3 version(s), latest 2
```

The change date 2026-08-15 is day 45 of the script, the planted change. The output is the same with and without the lift test here, because the weekend rate (0 of 26) is far below the weekday rate; the lift test matters on dense logs, see below. This is a script written by the same author as the rules, so it shows the mechanism works, not how well it works on your writing. What keyword rules cannot do: they do not know synonyms you did not list ("hit the weights" is not "gym"), they do not understand sarcasm or hypotheticals ("I should go to the gym" counts), they cannot tell a context from a keyword ("morning" in "good morning to the team" counts), they cannot see context beyond the sentence, and the negation guard is a short-window heuristic. On the synthetic free-text lives of `docs/measured.md` (templates and rules by the same author), habit precision was 1.00 at every seed after the lift test (before it, built-in weekday/weekend contexts gave 2 to 9 false habits per life and overall precision 0.50 / 0.29 / 0.00 at 0 / 20 / 50% noise), but habit recall fell to a mean of 0.93 / 0.70 / 0.07 at 0 / 20 / 50% noise, and at 50% noise that precision is mostly "nothing detected". On separate tuning seeds a residual false habit still appeared in some lives (details in `docs/measured.md`).

### Being loop quick start

The owner directory (`<root>/<owner>/`) holds the log of one independent being: a deterministic simulation loop, not a person's journal. A seeded simulated world offers verifiable tasks in four domains (arithmetic, linear equations, physics formulas, next term of a sequence); each task has a difficulty, an energy cost, a reward and an exact checker. The loop observes its own log, picks a domain with a UCB1 chooser over the reward per energy it has been paid so far, has a scripted solver answer, checks the answer and appends a decision, an action and an outcome event (source `being:loop`, time from the world clock), a decision revision that cites its evidence events, and a prediction with its resolved outcome. Everything the being "prefers" is derived from the log alone (a replay re-derives every decision) and produced by the author-defined rewards. Rewards, costs and solvers were written by the same author; see `docs/measured.md` for the measured numbers and their limits.

Real output of `kore-self/tests/e2e_being.rs` (real binary, temp data root; long values shortened with `...`):

```
being_run {steps:300, seed:1, world_seed:3, budget:3000}
  steps_run 300, stopped "step limit reached", total_reward 2438, total_energy_spent 1592, budget_left 1408, version_after 13
  arithmetic: selected 100, correct 79, reward 803, energy 526, reward_per_energy 1.527
  algebra:    selected  35, correct 28, reward 354, energy 252, reward_per_energy 1.405
  physics:    selected   5, correct  2, reward  21, energy  30, reward_per_energy 0.7
  pattern:    selected 160, correct 126, reward 1260, energy 784, reward_per_energy 1.607

being_status -> replay_check {"decisions": 300, "re_derived_from_log_and_equal": 300}

being_explain {decision_event_id: "9341509dcad0cdbf..."}
  decision: "step 299: chose pattern by ucb1 (highest score (mean + exploration bonus) among affordable domains); n=159 mean=1.602 reward/energy bonus=0.013 score=0.174; task 299-pattern difficulty 3 cost 5 reward 12; cites 21 events"
  alternatives (mean reward per energy, bonus, score):
    arithmetic n=100 1.527 0.0169 0.1695
    algebra    n=35  1.405 0.0285 0.1690
    physics    n=5   0.700 0.0755 0.1455
    pattern    n=159 1.602 0.0134 0.1736   <- chosen
  evidence_events_cited: genesis event; "step 2: task 2-physics answer 2 is correct (expected 2); reward 15 cost 8 budget left 2983"; "step 7: task 7-physics answer 29 is wrong (expected 28); reward 0 cost 4 budget left 2954"; ... (21 events)
  outcome_of_this_step: "step 299: task 299-pattern answer 118 is correct (expected 118); reward 12 cost 5 budget left 1408"
  replay_check {"re_derived_domain": "pattern", "equal_to_recorded": true}

self_sql: SELECT kind, COUNT(*) AS n FROM events WHERE source = 'being:loop' GROUP BY kind ORDER BY kind
  action 300 | decision 300 | note 1 | outcome 300

self_sql: SELECT r.stance, COUNT(*) AS decisions FROM revisions r WHERE r.entity_type = 'decision' GROUP BY r.stance ORDER BY decisions DESC
  choose pattern 160 | choose arithmetic 100 | choose algebra 35 | choose physics 5

being_run {steps:5, world_seed:4}  (the being already has world_seed 3)
  invalid: this being already has world_seed 3; the request says 4. A being keeps one world and one seed

self_verify
  OK: 14 version(s), latest 13, head hash f17b94384116dd8725e9c26c5fdb0c43a1da28a5736e506f56d188003b25aee5

self_verify after flipping one byte of events/0000000013.kore
  VERIFY FAILED: checksum mismatch in segment: events/0000000013.kore
```

Notes. A decision cites the genesis event and the latest 5 outcome events per domain; the counts it used come from all earlier outcomes. `being_run` with `stop_after_budget: false` lets the world refill the budget at the next world day (logged as a `refill` event). The loop can touch only the simulated world and this owner's log (no files, network or processes; a source-scan test checks this); it stops at the step limit, a stop flag, or when no task is affordable. The `Thinker` trait (`being/solver.rs`) is the seam where a language-model thinker could be plugged in later; only the scripted baseline exists. Under the defined rewards the learned chooser collected about 31% more reward per energy than random choice over 20 simulated worlds and about the same as the best single fixed domain (`docs/measured.md`).

Does NOT do: there is no language-model thinker yet (the solvers are fixed scripts); no access to the real world (no files, web or tools); no open-ended goals (the only goal is reward per energy in the simulation); its preferences are produced by the rewards the author defined, they are not discovered values; it is not shown to beat always choosing the best single domain; it does not run by itself between calls (no scheduler); no claim is made about understanding or intelligence.

### Track B: experience learner (research, step B1)

`being_run` with `chooser: "model"` creates a being in a menu world: each step the world offers 4 tasks, and the being picks one task and one of three solver strategies (fast, careful, deep, with different energy and reliability), or skips the menu for 1 energy unit. A world model built only from the being's own logged outcomes (success probability and energy per domain, difficulty and strategy, with shrinkage from broader levels and an uncertainty) feeds a planner that maximizes optimistic reward per energy. Every decision logs all 12 alternatives with their estimates (`being_explain` shows them), and a replay re-derives every decision from the log alone. The default (`chooser: "ucb"`) is the unchanged single-task being above; the chooser is fixed once the being exists.

Real output of `kore-self/tests/e2e_being.rs` (`being_model_chooser_end_to_end`, real binary, temp data root; values shortened):

```
being_run {steps:200, seed:1, world_seed:3, budget:3000, chooser:"model"}
  steps_run 200, chooser "model", total_reward 1907, total_energy_spent 577 (3.31 reward per energy)
being_status -> replay_check {"decisions": 200, "re_derived_from_log_and_equal": 200}
being_explain on the last decision -> action "0:fast", replay equal_to_recorded true
  alternative slot 0 arithmetic difficulty 2 fast: n_cell 21, p_success 0.821, uncertainty_sd 0.075, est_energy 2.985, score 3.44
  alternative slot 0 arithmetic difficulty 2 careful: n_cell 0, p_success 0.5, uncertainty_sd 0.224, est_energy 5.0, score 1.838
being_run {steps:1, chooser:"ucb"} on this being -> error: the being keeps its chooser
```

Measured (`docs/measured.md`, 20 simulated worlds, seeds 1..20, budget 600; tuned on seeds 101..120; author-defined rewards and strategies): mean reward per energy 3.12 for the model learner against 1.59 for the UCB1 domain chooser, 1.50 for greedy-by-listed-reward-per-cost, 2.01 for the best fixed strategy (greedy with fast), 1.19 for random and 3.49 for the oracle that knows the hidden success rates (which never skips). The learner is better than UCB1 in 20 of 20 worlds and than the best fixed strategy in 19 of 20; its predicted success has Brier 0.127 against 0.169 for the running base rate but is overconfident where it predicts 0.2-0.6.

Does NOT do: it does not discover new strategies or rules (the three strategies are fixed code; discovery is step B2); how it copes with change is measured in step B3 below; it is not compared with any language model (B4); the oracle and the baselines are weak by design (the old policies cannot choose a strategy), so the gap to UCB1 mostly measures strategy choice and skipping; the result holds only for this author-written world and its trade-offs; it makes no claim of understanding, thinking or intelligence, only the measured numbers above.

### Track B2: rule discovery

The being searches for new solving rules in its own log. The DSL and the sequence families are written by the author, so "discovery" here means a search inside a small defined space, not open-ended invention.

How it works. A being created with `pattern_families: "extended"` (needs `chooser: "model"`) lives in a world where sequence tasks also come from extra hidden families that the fixed solvers cannot solve (second-order recurrences, tribonacci-like, cubics, affine geometric, alternating add, a period-3 cycle) plus two families outside the grammar on purpose (a product recurrence and consecutive primes; negative controls). After each attempt the checker's exact next term is logged on the outcome event, so every attempt leaves one revealed example (prefix and next term). With `discover: true` the being runs a search every `discover_every` steps (default 10) over those examples only. The grammar has 17 forms: next = a weighted sum of the previous 1-3 terms (optional constant), or a polynomial of degree 1-3 in the term index; the coefficients are fitted per task from its own prefix with exact rational arithmetic. A rule applies to a prefix only if the fit is unique and at least 3 spare equations agree exactly. Smallest forms are tried first, per signature (pattern, difficulty); examples are split in log order and every third one is held out. A form is accepted only if it is never applicable-and-wrong on training or held-out examples and reproduces at least 3 training and 1 held-out example that no smaller accepted form already explains. An accepted rule is appended to the log as a `rule` note event and a `rule` revision that cites the example events it was proved on, with its content-derived id; searching again adds nothing. A fourth strategy, `discovered`, applies the being's own rule when one fits and otherwise behaves like `careful`; the model learner treats it as one more strategy. `being_rules` lists the rules (form, support, cited examples, how often used, and a check that the search over only the examples logged before the rule yields the same rule), `being_explain` names the rule used by a decision. Nothing reads the generator or the hidden family labels (a source-scan test checks this).

Real output of `kore-self/tests/e2e_being.rs` (`being_rule_discovery_end_to_end`, real binary, temp data root; values shortened):

```
being_run {steps:700, seed:1, world_seed:6, budget:60000, chooser:"model", pattern_families:"extended", discover:true, discover_every:10}
  steps_run 700, pattern_families "extended", total_reward 12685, total_energy_spent 3251 (3.90 reward per energy), rules_total 8
  rules accepted: poly.3 (difficulty 3), poly.1 (d1), lin.3 (d5), lin.1.c (d2), lin.123 (d4), lin.1 (d2), lin.2.c (d5), lin.12 (d4)

being_rules (first rule)
  rule_id 1a7343ebffd9, signature "pattern difficulty 3", dsl "next = b0 + b1*n + b2*n^2 + b3*n^3", accepted_at_step 10
  training_examples_reproduced 3, held_out_examples_reproduced 1, cited_example_events 4, times_used_in_attempts 25, rederived_from_log true
  cited sample: terms "2;17;50;107;194;317;482" revealed_next "695"

being_explain on the decision of step 70 (answered by a rule)
  decision: "step 70: chose slot 0 (pattern difficulty 5, listed cost 10 reward 56) with strategy discovered by model-ucb (...); p_success=0.500 sd=0.224 ..."
  discovered_rule_used {rule_id "3e88382ba97a", stance "pattern difficulty 5: next = a3*t[-3]"}
  outcome_of_this_step: "step 70: task 70-0-pattern is correct; reward 56 cost 10 budget left 59659"
  replay_check {"re_derived_domain": "0:discovered", "equal_to_recorded": true}

self_sql: SELECT entity_type, COUNT(*) AS n FROM revisions GROUP BY entity_type ORDER BY entity_type
  decision 700 | rule 8

self_verify
  OK: 30 version(s), latest 29, head hash b4148714fc5a08503da62810f8c68077e717141e3b3505053b609bcfcae8bc00
self_verify after flipping one byte of events/0000000029.kore
  VERIFY FAILED: checksum mismatch in segment: events/0000000029.kore
```

Measured (`docs/measured.md`, 20 simulated worlds, seeds 1..20, budget 3000; choices tuned on seeds 101..120; pattern puzzles pay 2x in this world so that rules are worth something): mean reward per energy 3.94 for the model learner with discovery against 3.78 for the same learner with fixed solvers only (difference +0.16, 95% interval +0.00 to +0.32; 8 wins, 7 losses, 5 ties), 1.20 for random and 4.27 for an oracle holding perfect rules. At budget 600 discovery did not help (-0.04, interval -0.16 to +0.08), and in a control world where pattern puzzles pay 1x it did not help either (-0.12, interval -0.29 to +0.05). The search found 52% of the (family, world) pairs by the end (median 60 to 170 steps to the first correct rule), accepted 99 rules of which all 99 were exactly correct on fresh tasks, applied a rule to none of the fresh tasks of the two families outside the grammar, and accepted nothing in a control with only those two families (30, 100 and 300 examples, 20 worlds). An earlier, looser guard (2 spare equations) accepted 4 wrong rules in 100 (consecutive primes can satisfy a lag-2 rule); the stricter guard removed them at the price of data.

Does NOT do: it is not open-ended invention (17 author-written forms, fitted per task, for author-written families); it does not help reliably (the gain at budget 3000 has an interval touching 0, none at budget 600 or when the puzzles pay 1x); it finds about half of the expressible families in 3000 energy units and only after 60 to 170 steps; a rule needs only 4 supporting examples (3 training, 1 held out); rules are per difficulty level and are not transferred between levels; a rule cannot be retired once accepted; the algebra domain is not covered; it uses no real data and no language model; no claim is made about understanding or intelligence, only the measured numbers above.

### Track B3: adapting to change

What happens when the world changes in the middle of the being's life, and does a learner built for change do better? Two creation-time options of `being_run` (both need `chooser: "model"`, both are fixed once the being exists, both default to off so every earlier result is unchanged):

* `drift: true` makes the simulated world change at hidden times that scale with the initial budget (about 25%, 50% and 75% of it): a new domain (`units`, unit conversion with an exact checker) starts to appear in the menu; the rewards and costs of physics and arithmetic shift; the `fast` strategy becomes much less reliable. The being is not told; it sees only outcomes and the visible features of the offers.
* `adapt: true` makes the world model forget old evidence exponentially (half-life in attempts), adds optimism for a (domain, strategy) pair it has hardly seen, and runs a change detector per (domain, strategy) on the prediction error of its own model (two-sided CUSUM). Everything is derived from the log, so replay is exact. Each flagged change is logged as a note event plus a `belief` revision (author `rule:being-detector`) that cites the outcome events it rests on; `being_status` lists them (`changes_detected`) and `being_explain` says how many were flagged before the decision. With `adapt: false` the model is the unchanged B1 model.

Real output of `kore-self/tests/e2e_being.rs` (`being_drift_adapt_end_to_end`, real binary, temp data root; values shortened):

```
being_run {steps:3500, seed:1, world_seed:6, budget:6000, chooser:"model", drift:true, adapt:true}
  steps_run 2214, stopped "budget exhausted", total_reward 23144, total_energy_spent 5999 (3.86 reward per energy)
  drift true, adapt "full", changes_flagged_total 2
being_status
  replay_check {"decisions": 2214, "re_derived_from_log_and_equal": 2214}
  changes_detected[0]: "change 0 detected at step 605: the success rate of strategy fast in domain units looks lower than the model
    predicted (observed 0 right of 5 attempts, predicted 4.5; CUSUM statistic 9.17); older evidence of this group is cut to 10% of its weight"
    revision key being/change/0000, author rule:being-detector, evidence_events_cited 12
  changes_detected[1]: "change 1 detected at step 1496: ... strategy fast in domain units ... (observed 1 right of 12 attempts, predicted 9.4; CUSUM statistic 8.38) ..."
being_explain on the last decision (step 2213, answered with units/fast)
  changes_flagged_before_this_decision {"count": 2, ...}, reasons {"adapt": "full", "novelty_bonus": 0.25, "changes_flagged_so_far": 2, ...}
  replay_check {"re_derived_domain": "3:fast", "equal_to_recorded": true}
self_sql: SELECT occurred_at, text FROM events WHERE tags LIKE '%change_index:%'
  2030-01-01T19:28:30Z | change 0 detected at step 605: the success rate of strategy~
  2030-01-02T14:07:30Z | change 1 detected at step 1496: the success rate of strateg~
self_verify
  OK: 91 version(s), latest 90, head hash 03432260a42253328abfd878d4a0ea7ea58e42fbcc53c4322e2aae4e9b6feecc
self_verify after flipping one byte of events/0000000090.kore
  VERIFY FAILED: checksum mismatch in segment: events/0000000090.kore
```

In this world the second alarm (energy about 4570) is the degradation of `fast`; the first (about 2340) is the new domain, where the model had predicted that `fast` would work as in the other domains and it did not.

Measured (`docs/measured.md`, 20 simulated worlds, seeds 1..20, budget 6000; settings chosen on seeds 101..120; author-defined schedule and rewards): mean reward per energy 3.81 for the B1 learner, 3.84 for the adaptive learner, 3.71 for B1 with forgetting only, 3.89 for an oracle that knows the schedule, 1.87 for UCB1 over domains and 1.33 for random. The adaptive learner minus B1 is +0.03 (95% interval -0.18 to +0.24, 10 wins and 10 losses): adaptation did not help measurably, because B1 is already within 0.08 of the oracle; the cold-start bonus alone gives the same gain and forgetting alone is worse than B1 in 20 of 20 worlds (-0.10). The detector flagged the degradation of `fast` in 13 of 20 worlds with a median delay of 265 energy units, with 0 false alarms in 60 stationary worlds, but that did not become reward. In the stationary world the adaptive learner is within noise of B1 (-0.04 to +0.05, interval about +-0.2) and lost in 14 to 15 of 20 worlds at each budget.

Does NOT do: it does not show that adaptation helps (the interval of its gain over B1 contains 0, and the gain is explained by the cold-start bonus alone); forgetting by itself hurts, and the tuning picked a half-life of 5000 attempts, which is almost no forgetting in a life of about 2500 attempts; it does not adapt to a change that hides in the visible features or to a change whose stale belief is expensive (neither was tested; the one hidden change here, `fast` degrading, costs little because `careful` is almost as good); the detector missed the degradation in 7 of 20 worlds and has a delay of about 100 steps; the schedule has three changes in a fixed order, one world family and one seed range; the oracle never skips, so it is not an upper bound on reward; it uses no real data and no language model; it makes no claim of understanding, thinking or intelligence, only the measured numbers above.

### Track B4: advice as an input

Research question: can an outside knowledge source (for example a language model) be ONE INPUT to a mind that learns from its own experience, does it help, and how far should the being trust it? B4 gives the menu-world being (`chooser: "model"`) an optional advisor and makes the being learn, from its own logged outcomes, how far to trust it. In every measurement and test the advisor is SIMULATED (a program with a quality knob); no real model is called and this crate contains no network code.

Creation-time options of `being_run` (fixed once the being exists, default off, so every earlier result is bit-identical):
* `advisor`: `"none"`, `"simulated"` (needs `advisor_quality`, 0 to 1: the probability that the simulated advisor names the oracle-correct option; a property of the simulation) or `"command"` (the program in `advisor.txt`).
* `trust`: `true` (default with an advisor) learns trust per (advisor, domain) from the log and mixes advice with the being's own model by it; `false` follows advice blindly (the comparison arm).

What the being does. Each step it sends the advisor only the visible task features (domain, difficulty, listed cost and reward of the 4 offers, strategy names; for sequence tasks the visible terms). The advisor names an option (slot and strategy, optionally a stated success probability) and, in the extended pattern world, may propose a rule of the B2 grammar. Everything is logged as events of source `advisor` (request, reply, what happened afterwards, with ids derived from the content). The advice is an extra input to the planner: with the learned trust weight `w` the advised option's score is lifted towards 1.1 times the best score by the fraction `w` (formula in `being/planner.rs` and `being/trust.rs`); a proposed rule is used only if the B2 verified search (training and held-out exact reproduction, never applicable-and-wrong) accepts that form on the being's own logged examples. Trust is the Beta posterior of "advice that was followed against the model's own choice earned at least the reward per energy the model alone had predicted"; it is folded from the log, so a replay re-derives every decision including the advice part. A new advisor is tried (optimism of one standard deviation), a useless one stops being followed as the tests accumulate, and a cumulative-sum detector flags an advisor that gets worse.

Authorization. Every `being_run` that asks an advisor first goes through `authorize` with the tool name `advisor_query` (level L1, default deny, audited, only the being loop may use it, the kill switch stops it). The owner allows it by hand in `autonomy_policy.txt`:

```
[L1]
advisor_query
```

(The check is made once per run, before the log is opened, not once per step: every authorization decision is an audit event.)

The command advisor (`advisor.txt`). Only for an owner who wants to try a program as advisor. The file `<owner dir>/advisor.txt` is written by hand (reserved: no tool can read or write it), same format and same safety rules as `thinker.txt`:

```
# one setting per line: key = value
command = C:\tools\my_advisor.exe      # absolute path, required
arg = --json                           # repeat for each argument, in order
timeout_secs = 30                      # 1..=600, default 30
max_output_bytes = 65536               # 1024..=4194304, default 65536
env_allow = MY_API_KEY                 # names of variables to pass through (the environment is otherwise cleared)
```

The program is run without a shell, with an absolute path, working directory `<owner dir>/scratch`, a cleared environment, no stderr, a timeout and an output cap. It gets ONE JSON request on stdin (the shareable features above, passed through the redactor first, so `redact_patterns.txt` applies) and must print ONE JSON object: `{"slot": 2, "strategy": "careful", "p_success": 0.8, "rule": {"signature": 3, "form": "poly.2"}, "note": "..."}` where `slot`+`strategy` go together, `rule` is optional and only for signatures the request asks for (`form` is a code like `lin.12.c` or the DSL text `next = a1*t[-1] + ...`), and every field is validated. The prompt and the reply go into the advice event (redacted). A failed request (timeout, bad JSON, over the cap, non-zero exit, invalid option) is logged as an error and the being goes on without advice; after 3 failures in a row the advisor is not asked again in that run. It is tested only with the helper example program `chat_helper` (modes `advise_best`, `advise_echo`, `advise_rule`, `sleep`, `flood`, `badjson`, `exit1`), never with a real model.

Real output (from `cargo test -p kore-self --offline --test e2e_advisor -- --nocapture`, the real binary on a temp root):

```
being_run with advisor simulated, no autonomy_policy.txt
advisor_query was not allowed (deny): no autonomy_policy.txt: default deny. The being will not ask its advisor. To allow it, list advisor_query under [L1] in autonomy_policy.txt (edit that file by hand)

being_run {steps:300, seed:1, world_seed:3, budget:3000, chooser:"model", advisor:"simulated", advisor_quality:0.9, trust:true}   (after allowing advisor_query)
  "advisor": "simulated", "advisor_quality": 0.9, "steps_run": 300, "total_reward": 3032, "trust_learning": true,
  "advice": [ { "advisor": "simulated:q0.90", "kind": "algebra", "advice_given": 78, "tests_followed_against_model": 17, "tests_won": 12, "trust_mean": 0.684, "trust_sd": 0.104, "mixing_weight_now": 0.576 },
              { "kind": "arithmetic", "advice_given": 122, "tests_followed_against_model": 3, "tests_won": 2, "trust_mean": 0.6 },
              { "kind": "pattern", "advice_given": 90, "tests_followed_against_model": 20, "tests_won": 14, "trust_mean": 0.682 },
              { "kind": "physics", "advice_given": 10, "tests_followed_against_model": 0, "trust_mean": 0.5 } ]

being_explain on the first decision, where the being followed advice against its own model:
  "decision": "step 0: chose slot 2 (algebra difficulty 4, listed cost 8 reward 16) with strategy careful by model-ucb (advice 593908f6a2 moved the choice from 2:fast to 2:careful (mode trust, weight 0.58); ...",
  "how_advice_changed_the_decision": { "advised": "2:careful", "model_alone_would_choose": "2:fast", "model_alone_predicted_reward_per_energy": 1.0,
      "decision_equals_model_alone": false, "advice_followed": true, "mixing_weight": 0.577, "trust_mean": 0.5, "trust_sd": 0.289, "mode": "trust" },
  "advice_for_this_step": [ { "kind": "advice", "text": "{... request, advice {slot 2, strategy careful, p_success 0.84} ...}" },
      { "kind": "result", "text": "advice 593908f6a2cc of simulated:q0.90 at step 0: followed advice 2:careful against the model's own choice 2:fast (the model alone predicted 1.000 reward per energy for it); realized 2.000 reward per energy, outcome correct (reward 16 for energy 8) ..." } ],
  "replay_check": { "equal_to_recorded": true, "re_derived_domain": "2:careful|adv f1 a0 r1.000000 w0.577" }

self_verify -> OK: 24 version(s), latest 23, head hash ffce4b05...
self_verify after flipping a byte -> VERIFY FAILED: checksum mismatch in segment: events/0000000023.kore

command advisor, helper program, first logged failure per mode (6 steps each, the run goes on without advice):
  sleep (timeout_secs = 1): {"advice":{"error":"the external command timed out after 1s ...
  flood (max_output_bytes = 1024): {"advice":{"error":"the external command printed more than 1024 bytes and was killed ...
  badjson: {"advice":{"error":"the reply is not valid JSON: ...
  exit1: {"advice":{"error":"the external command exited with exit code: 1 ...
  echo: {"advice":{"error":"the reply names no option, no rule and has no note ...
```

Measured (docs/measured.md, "Track B4": synthetic world, SIMULATED advisor; seeds 1..20, tuning seeds 101..120; mean reward per energy, paired against the B1 learner without advisor):
* Following advice blindly loses unless the advisor is almost perfect: at `q = 0.9` it is 0.18 (budget 600) and 0.39 (budget 3000) below no advisor, at `q = 0.5` 1.3 and 1.6 below, for the confidently wrong advisor (`q = 0.2`, states 0.95) 1.8 and 2.1 below; only at `q = 1.0` does it win (+0.38 [+0.11, +0.64] and +0.16 [-0.05, +0.37]). Break-even about `q` = 0.93 to 0.97.
* The trust learner is not significantly worse than no advisor at any `q` (every difference within noise of zero below `q = 0.9`; confidently wrong advisor +0.01 and +0.07; an advisor that degrades from 0.9 to 0.3 mid-life +0.09 and +0.13). Testing a useless advisor costs about 1% to 4% of the steps.
* Good advice helps modestly and mainly early: `q = 0.9` gives +0.15 [-0.02, +0.32] and +0.17 [-0.02, +0.36]; `q = 1.0` +0.23 [+0.02, +0.43] and +0.20 [+0.00, +0.39]; the step at which the being reaches 90% of the oracle's rate falls from 61 to 43 (budget 3000). The B1 learner alone already reaches 3.38 against the oracle's 3.54 at budget 3000.
* Learned trust ranks the advisors perfectly by quality (rank correlation 1.00) but is not a calibrated estimate of `q` (0.49 at `q` = 0 to 0.82 at `q` = 1).
* Rule proposals: every accepted rule passed the verified search (0 wrong rules), none arrived faster than discovery (median first rule at step 46 against 20).

What B4 does NOT do. It does not measure any real language model: the advisor in every number is simulated, and how good a real model is as an advisor on real tasks is unknown and would have to be measured separately through `advisor: "command"` on real tasks. It does not show that advice helps in general: the gains are small, early, and in one synthetic world with an author-defined reward. It does not separate the information in advice from the exploration a wrong suggestion causes. The command advisor can use the network, the disk and your account if the program you configure does (there is no sandbox); only configure a program you trust. A real model's reply can only choose among the options in the request and propose rules that the verified search then checks; it cannot execute anything. No claim of understanding, life or intelligence is made.

### Talking to the being (A1)

A chat scaffold with a pluggable thinker, off by default. You talk with `chat_say`; the reply comes from a thinker; the tool calls a thinker proposes run only if a policy file you wrote by hand allows them. Messages, the exact prompt and reply, every authorization decision and every tool result are events in the being's own log, so a conversation survives restarts and is queryable with `self_sql`. There is no model in this crate and the chat code makes no outgoing connections (there is no HTTP client in the crate): to use a model, you point `thinker.txt` at a program you trust (a local model CLI, or a script of your own that calls whatever service you choose).

Tools: `chat_say {text, session?}`, `chat_history {session?, limit?}`, `chat_pending {}`, `chat_approve {id, approve}`. They are for you (the MCP caller); a thinker cannot call them. The tools a thinker may propose in A1 are only `self_sql`, `self_belief_history`, `being_status` (read-only) and `self_event_add` (a note into the being's own log, stored with source `chat_note`; the thinker cannot choose the source, kind or time).

**Policy file** `<owner dir>/autonomy_policy.txt` (you create and edit it by hand; no tool can read, create or change it). Default with no file: every proposed tool call is denied.

```text
# comments start with '#'
[L1]            # suggest only: read-only tools, no effects
self_sql
being_status
self_belief_history
[L2]            # low-risk effects: a note into the being's own log
self_event_add
[L3]            # always needs your approval via chat_approve, never automatic
```

A tool may be listed under its built-in tier or a higher one (a read-only tool under `[L3]` means "ask me every time"). Any parse error (unknown section, unknown tool name, a tool listed twice or below its built-in tier, text before the first section, a file over 64 KiB, or on unix a group/world-writable file) makes the whole policy invalid and everything is denied. Every decision for a thinker's proposal is appended as an event with source `authorize` (actor, tool, decision, reason, redacted and truncated arguments); if that row cannot be written, the call is denied. Your own `chat_say` / `chat_approve` calls are audited too; `chat_history` and `chat_pending` reads are not. A file named `KILL` in the owner directory (create it by hand) is a kill switch: `chat_say`, `chat_approve` and every proposed tool call are refused and the thinker command is not started; only the read-only `being_status`, `chat_history` and `chat_pending` still work (a tool call still has to be listed in the policy). `self_event_add` refuses the sources `authorize`, `thinker` and `chat:*`, so audit and chat rows cannot be forged through it. The files `thinker.txt`, `autonomy_policy.txt` and `KILL` are reserved like `ingest_roots`: no tool path, ingest or export can read, create or overwrite them.

**Thinker file** `<owner dir>/thinker.txt` (by hand; there is no default). Without it the built-in `ScriptedChatThinker` answers: `echo <text>`, `help`, `status`, `beliefs`, `recall <word>`, `history <key>`, `remember <text>` (rules only, it proposes tool calls and the policy decides). With a broken `thinker.txt` the turn is refused and nothing runs (no silent fallback).

```text
# one setting per line, key = value; unknown keys are an error
command = C:\tools\my_model_cli.exe     # absolute path to an existing file, required
arg = --json                            # one line per argument, in order; no shell, no quoting
arg = --model
arg = small
timeout_secs = 60                       # 1..600, default 30
max_output_bytes = 65536                # 1024..4194304, default 65536
env_allow = MY_API_KEY                  # repeat; the only variables passed through besides PATH and SystemRoot
```

Protocol: one process per turn. stdin gets one JSON object (`protocol`, `session`, `user`, `messages` [{role,text}], `facts`, `tools`, `reply_format`), stdout must be one JSON object `{"reply": "...", "tool_calls": [{"tool": "...", "args": {...}}]}` (at most 5 tool calls are considered per turn). Before anything is sent, every text in the context goes through the redaction of `src/redact.rs` (emails, card numbers, SSNs, phone numbers and your `redact_patterns.txt`; best effort, not a guarantee). The exact request and the raw reply are logged as events with source `thinker` (the reply after redaction, as for every event).

**Security model.** `thinker.txt` makes this crate run a program of your choosing with your own rights. Only configure a program you would run yourself and keep `thinker.txt`, the program and the policy file writable by you only (on unix a group/world-writable file is refused; on Windows this is not checked). What this crate does: no shell; absolute program path required; working directory `<owner dir>/scratch`; a cleared environment (`PATH`, `SystemRoot`, `TEMP`/`TMP`/`TMPDIR` set to scratch, plus only the names in `env_allow`); stderr discarded; a timeout (the process is killed); an output cap (the process is killed); a reply that is not valid JSON fails the turn; each proposed tool call is checked by `authorize` and then by the normal argument validation of the tool. What it does not do: it cannot stop the program from using the network or your files (there is no sandbox), it cannot tell a good reply from a bad one, and redaction can miss secrets, so anything you type may reach the program in some form. A thinker can only propose; it cannot approve its own request (`chat_approve` is refused to every actor but the human) and cannot edit the policy, the kill switch or its own configuration through any tool.

Real output of the end-to-end test (`cargo test -p kore-self --test e2e_chat -- --nocapture`, run after `cargo test -p kore-self` or `cargo build -p kore-self --example chat_helper`, which builds the helper program the test uses as its "thinker"; it is not a model). Trimmed to the interesting fields.

```text
--- chat_say 'status' (no thinker.txt, no policy file)
  "reply": "Asking for the being loop status.",
  "thinker": "scripted-chat-1",
  "tools": [ { "reason": "no autonomy_policy.txt: default deny", "status": "denied", "tool": "being_status" } ]

--- chat_say with secrets, thinker = helper echo (the helper replies with what it received)
  input:  my email is bob.builder@example.com, ssn 123-45-6789, card 4111 1111 1111 1111
  the helper received text with these replaced by [REDACTED:...]; the three values appear nowhere in the log

--- thinker proposes self_forget (not in the policy, not a chat tool)
  "tools": [ { "status": "denied", ... } ]

--- self_sql: audit rows by decision/tool/actor
events.tags                                      | n
decision:allow,tool:being_status,actor:thinker   | 1
decision:allow,tool:chat_say,actor:human         | 9
decision:allow,tool:self_event_add,actor:thinker | 1
decision:allow,tool:self_sql,actor:thinker       | 2
decision:deny,tool:being_status,actor:thinker    | 1
decision:deny,tool:chat_approve,actor:thinker    | 1
decision:deny,tool:self_forget,actor:thinker     | 1

--- thinker proposes self_event_add (listed under [L3]), then chat_pending, then chat_approve {approve:true}
  status "needs_approval" -> "pending": 1 -> "approved": true, "ran": true (the note exists only after the approval)

--- chat_say with the KILL file present
chat_say refused: kill switch: the KILL file exists, only read-only status is allowed

--- self_sql: events by source after restart
authorize 32 | chat:being 9 | chat:human 3 | chat:pending 3 | chat:tool 14 | chat:user 13 | chat_note 3 | thinker 26

--- self_verify
OK: 104 version(s), latest 103, head hash 13abd8c2c9bd...

--- self_verify after flipping a byte
VERIFY FAILED: checksum mismatch in segment: events/0000000103.kore
```

A second test runs the helper in misbehaving modes: a program that sleeps is killed at the timeout, one that prints more than `max_output_bytes` is killed, bad JSON and a non-zero exit fail the turn (each logged as a `thinker` error event), a relative or missing command path and unknown keys refuse the turn, and only `env_allow` variables reach the program.

What the chat does NOT do:

- It has no built-in model: without `thinker.txt` only the fixed rules above answer, and they do not understand language.
- The chat code contains no network client; if your thinker program talks to a hosted service, that is your program's doing, under your account, and what you type (after best-effort redaction) is sent there.
- No autonomy beyond the policy: nothing runs unless you wrote it into `autonomy_policy.txt`, there is no scheduler or background loop, and in A1 a thinker cannot reach any tool outside the four listed above (nothing that writes outside the log, no files, no processes, no ingestion, no forgetting).
- No reply-quality claim: how good the replies are depends entirely on the program you plug in and is not measured here; there is no row for it in `docs/measured.md`.
- One thinker call per turn: tool results are logged and appear in the next turn's context, they are not fed back within the same turn.
- No sandbox for the thinker program, no check of file permissions on Windows, no encryption of the conversation (it is in the log, which is not encrypted), and the kill switch is a file check made before each decision, not a way to stop a call already running.

### What this does NOT do

- It does not think, understand, or know you. Its "profile" values are counters over the words in your memories.
- It does not predict reliably. The predictor counts keyword to choice matches and is not scored against outcomes yet.
- It does not run any language model and makes no network calls on its own (the chat can run a program you configure; see "Talking to the being (A1)").
- It does not write or modify source code. The former code-generating tool was removed.
- It does not push anything to GitHub or any remote (`self_pushback` only compares a decision with your own recorded history).
- It does not encrypt data at rest (see below).
- It does not learn automatically: `self_learn` and `self_evolve_rules` run only when called; there is no scheduling. Learning is rule-based (counts, Wilson bounds, a likelihood-ratio scan, a small phrase grammar, hand-written keyword rules) and has been measured only on synthetic data, not on real lives.
- It does not score predictions yet. The ledger only stores frequency-baseline predictions and lets you attach outcomes; no accuracy is claimed and no predictor is validated.
- The log is not encrypted. No encryption yet.
- Forgetting exists (`self_forget`) but only inside the log: copies in backups, earlier exports or disk snapshots are not removed. Redaction before storage is best-effort and will miss things.
- It does not judge whether recorded events or stances are true, and a hash chain detects edits but is not a signature (someone who can write the folder can rewrite the whole log consistently).
- The old author-written narration layers (needs, story, worldview and similar generated prose) are off by default. They are available only if you build with `--features legacy-narration`, and they are not evidence of anything.

### Data and privacy

- Data root: `$KORE_SELF_HOME`, otherwise `~/.kore-self`. Each owner has its own folder `<root>/<owner>/`, which holds that being's log (the owner is the being, not a person's journal).
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
