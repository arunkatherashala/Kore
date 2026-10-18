# KORE and kore-self

KORE is a columnar SQL engine written in Rust (a workspace of crates). `kore-self` is a local tool built on it: a memory store you query with SQL, exposed as an MCP server.

**Full guide:** [GUIDE.md](GUIDE.md)

---

## kore-self

**Target positioning (the only claim this project makes):** A local, deterministic, append-only journal of how your recorded beliefs, habits and decisions change. Every change cites the events that caused it. You can query any past version with SQL. Its explanations can be ablation-tested, and its predictions are scored against outcomes.

**Status (milestone M1):** the versioned log is built: events, cited belief revisions, time-travel SQL and tamper-evident verification (see "Versioned self: quick start"). Nothing derives beliefs automatically yet, there is no predictor in the log yet, and no accuracy is claimed. Besides that log, kore-self is a local memory store with SQL over it, simple keyword/word-frequency counters, and a keyword-to-choice count predictor that has not been validated against outcomes. No accuracy number is claimed anywhere; any future number must have a row (n, baseline, interval, date) in `docs/measured.md`.

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
- It cannot forget: nothing can be deleted or redacted from the log once written. No forget yet.
- It does not judge whether recorded events or stances are true, and a hash chain detects edits but is not a signature (someone who can write the folder can rewrite the whole log consistently).
- The old author-written narration layers (needs, story, worldview and similar generated prose) are off by default. They are available only if you build with `--features legacy-narration`, and they are not evidence of anything.

### Data and privacy

- Data root: `$KORE_SELF_HOME`, otherwise `~/.kore-self`. Each owner has its own folder `<root>/<owner>/`.
- Files are **unencrypted at rest**. On Unix they are created with mode 0600 (directories 0700). On Windows they inherit the permissions of your profile folder. Encryption is planned (milestone M4). Do not commit the data folder.
- Owner names accept only letters, digits, `_`, `-` and `.` (max 64 characters).
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
