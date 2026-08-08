import subprocess, json

KORE = r"C:\Users\skathera\Downloads\asistent\kore\target\debug\kore-self.exe"
CWD  = r"C:\Users\skathera\Downloads\asistent\kore"

def run(msgs):
    inp = "\n".join(msgs).encode()
    p   = subprocess.run([KORE,"arun"], input=inp, capture_output=True, timeout=15, cwd=CWD)
    return p.stdout.decode(errors="replace")

def q(id, sql):
    return json.dumps({"jsonrpc":"2.0","id":id,"method":"tools/call",
                       "params":{"name":"self_query","arguments":{"sql":sql}}})

init = json.dumps({"jsonrpc":"2.0","id":1,"method":"initialize",
    "params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}})

tests = [
    (2, "scalar subquery (WHERE = MAX)",
        "SELECT content FROM memories WHERE importance = (SELECT MAX(importance) FROM memories)"),
    (3, "scalar subquery (WHERE > AVG)",
        "SELECT content, importance FROM memories WHERE importance > (SELECT AVG(importance) FROM memories) LIMIT 5"),
    (4, "IN subquery",
        "SELECT content FROM memories WHERE kind IN (SELECT DISTINCT kind FROM memories WHERE importance > 0.9)"),
    (5, "NOT IN subquery",
        "SELECT content FROM memories WHERE kind NOT IN (SELECT kind FROM memories WHERE importance < 0.7)"),
    (6, "correlated subquery (row > group avg)",
        "SELECT content, importance FROM memories m1 WHERE importance > (SELECT AVG(importance) FROM memories m2 WHERE m2.kind = m1.kind) LIMIT 5"),
    (7, "EXISTS subquery",
        "SELECT content FROM memories WHERE EXISTS (SELECT 1 FROM memories m2 WHERE m2.kind = memories.kind AND m2.importance > 0.9)"),
]

msgs = [init] + [q(id, sql) for id, _, sql in tests]
out  = run(msgs)

for line in out.split("\n"):
    try:
        r = json.loads(line)
        if r.get("id",0) >= 2:
            text = r["result"]["content"][0]["text"]
            icon = "❌" if "error" in text.lower()[:40] or "Query error" in text else "✅"
            label = next(lbl for i, lbl, _ in tests if i == r["id"])
            print(f"\n{icon} {label}")
            print(text[:180])
    except: pass
