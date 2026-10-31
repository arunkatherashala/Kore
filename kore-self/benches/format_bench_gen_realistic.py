"""Deterministic generator of a more realistic 'memory' dataset (same JSONL schema as format_bench_gen.py).

Usage:  python -I format_bench_gen_realistic.py OUT.jsonl N [SEED]

Text mix per event (chosen with the seeded RNG; ASCII only so byte length == char length):
  60%  short note      5-25 words, Zipf-skewed over a 3000-word vocabulary
  10%  boilerplate     one of 40 fixed phrases (e.g. 'standup done, nothing blocked') + ' #i'
  22%  medium entry    3-8 sentences drawn (skewed) from a pool of 400 recurring sentences
   8%  document        1000-5000 bytes: sentences from the same pool, paragraph breaks every 4-7 sentences
Every text ends with ' #<i>' so all event ids are distinct. Tags 0-4 from 40 tags. Timestamps and revisions
as in format_bench_gen.py. This is a synthetic stand-in for real notes; real text will compress differently.
"""
import datetime
import json
import random
import sys

DEFAULT_SEED = 20261101
BASE = 1735689600
SOURCES = ["journal", "chat", "calendar", "email", "sensor", "web", "voice", "import"]
KINDS = ["note", "action", "decision", "outcome", "statement"]
STANCES = ["prefer office", "prefer remote", "mixed", "undecided", "strongly remote"]


def iso(secs):
    return datetime.datetime.fromtimestamp(secs, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def main():
    out, n = sys.argv[1], int(sys.argv[2])
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else DEFAULT_SEED
    rng = random.Random(seed)
    syl = ["ka", "to", "mi", "re", "su", "lo", "an", "vi", "de", "po", "ni", "ra", "el", "or", "ta", "ge", "bu", "fa", "ul", "ix"]
    vocab = []
    for _ in range(3000):
        vocab.append("".join(syl[rng.randrange(len(syl))] for _ in range(rng.randrange(1, 5))))
    common = "the of and to in a is that for it with as was on be at by this had not are but from or have an they which one you were her all she there would their we him been has when who will more no".split()

    def word():
        # Zipf-ish skew toward the front of the vocabulary; ~35% common English function words
        if rng.random() < 0.35:
            return common[int(rng.random() ** 2 * len(common))]
        return vocab[int(rng.random() ** 3 * len(vocab))]

    def sentence():
        w = [word() for _ in range(rng.randrange(8, 16))]
        w[0] = w[0].capitalize()
        return " ".join(w) + "."

    pool = [sentence() for _ in range(400)]
    boiler = [" ".join(word() for _ in range(rng.randrange(3, 7))) for _ in range(40)]
    tags_pool = [f"tag{k:02d}" for k in range(40)]

    def pick():
        return pool[int(rng.random() ** 2 * len(pool))]

    with open(out, "w", encoding="utf-8", newline="\n") as f:
        for i in range(n):
            ts = iso(BASE + 30 * i + rng.randrange(30))
            u = rng.random()
            if u < 0.60:
                text = " ".join(word() for _ in range(rng.randrange(5, 26)))
            elif u < 0.70:
                text = boiler[rng.randrange(len(boiler))]
            elif u < 0.92:
                text = " ".join(pick() for _ in range(rng.randrange(3, 9)))
            else:
                target = rng.randrange(1000, 5000)
                parts, size, k = [], 0, 0
                while size < target:
                    s = pick()
                    k += 1
                    sep = "\n" if k % rng.randrange(4, 8) == 0 else " "
                    parts.append(s + sep)
                    size += len(s) + 1
                text = "".join(parts).strip()
            text = text + f" #{i}"
            tags = ",".join(tags_pool[rng.randrange(40)] for _ in range(rng.randrange(0, 5)))
            ev = {"t": "e", "source": SOURCES[rng.randrange(8)], "occurred_at": ts, "kind": KINDS[rng.randrange(5)], "text": text, "tags": tags}
            f.write(json.dumps(ev, separators=(",", ":")) + "\n")
            if i % 50 == 49:
                rv = {"t": "r", "key": f"topic-{(i // 50) % 100:03d}", "stance": STANCES[rng.randrange(5)], "conf": rng.randrange(101) / 100.0,
                      "rationale": f"after event {i}", "valid_from": ts, "cause": i}
                f.write(json.dumps(rv, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
