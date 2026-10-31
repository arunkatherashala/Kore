"""Deterministic dataset generator for the log-format benchmark.

Usage:  python -I format_bench_gen.py OUT.jsonl N [SEED]

Writes N events as JSON lines. After every 50th event (index i with i % 50 == 49)
one belief-revision line is written that cites that event. Both the Rust harness
(examples/bench_log_formats.rs) and the Python harness (format_bench_py.py) read
this same file. Event ids are NOT stored: each side derives them itself
(sha256 of source|occurred_at|text with whitespace collapsed), as an application would.

Generator facts (also asserted by the harnesses): occurred_at is strictly
increasing (2025-01-01T00:00:00Z + 30 s * i + jitter in 0..29 s), text is unique
per event (contains '#<i>'), so every event id is distinct.
"""
import json
import random
import sys

DEFAULT_SEED = 20261031

WORDS = ("alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec "
         "romeo sierra tango uniform victor whiskey xray yankee zulu worked home office meeting lunch commute "
         "remote tired focused coffee review deploy outage ticket budget travel family health reading running "
         "planning budgeting writing calling learning cooking cleaning shipping testing debugging sleeping").split()
SOURCES = ["journal", "chat", "calendar", "email", "sensor", "web", "voice", "import"]
KINDS = ["note", "action", "decision", "outcome", "statement"]
TAGS = ["work", "home", "health", "money", "family", "travel", "learning", "tools", "mood", "plan",
        "meeting", "code", "food", "sleep", "sport", "news", "chat", "todo", "idea", "misc"]
STANCES = ["prefer office", "prefer remote", "mixed", "undecided", "strongly remote"]
BASE = 1735689600  # 2025-01-01T00:00:00Z


def iso(secs):
    import datetime
    return datetime.datetime.fromtimestamp(secs, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def main():
    out, n = sys.argv[1], int(sys.argv[2])
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else DEFAULT_SEED
    rng = random.Random(seed)
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        for i in range(n):
            ts = iso(BASE + 30 * i + rng.randrange(30))
            words = [WORDS[rng.randrange(len(WORDS))] for _ in range(rng.randrange(6, 25))]
            tags = ",".join(TAGS[rng.randrange(len(TAGS))] for _ in range(rng.randrange(0, 3)))
            # tags may repeat; harmless
            ev = {"t": "e", "source": SOURCES[rng.randrange(len(SOURCES))], "occurred_at": ts,
                  "kind": KINDS[rng.randrange(len(KINDS))], "text": " ".join(words) + f" #{i}", "tags": tags}
            f.write(json.dumps(ev, separators=(",", ":")) + "\n")
            if i % 50 == 49:
                k = (i // 50) % 100
                rv = {"t": "r", "key": f"topic-{k:03d}", "stance": STANCES[rng.randrange(len(STANCES))],
                      "conf": rng.randrange(101) / 100.0, "rationale": f"after event {i}",
                      "valid_from": ts, "cause": i}
                f.write(json.dumps(rv, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
