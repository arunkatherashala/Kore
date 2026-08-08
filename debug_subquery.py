import subprocess, json

KORE = r"C:\Users\skathera\Downloads\asistent\kore\target\debug\kore-self.exe"
CWD  = r"C:\Users\skathera\Downloads\asistent\kore"

def run(sqls):
    init = json.dumps({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}})
    msgs = [init] + [json.dumps({"jsonrpc":"2.0","id":i+2,"method":"tools/call","params":{"name":"self_query","arguments":{"sql":s}}}) for i,s in enumerate(sqls)]
    p = subprocess.run([KORE,"arun"], input="\n".join(msgs).encode(), capture_output=True, timeout=15, cwd=CWD)
    results = []
    for line in p.stdout.decode(errors="replace").split("\n"):
        try:
            r = json.loads(line)
            if r.get("id",0) >= 2:
                results.append(r["result"]["content"][0]["text"])
        except: pass
    return results

# Debug: what does MAX(importance) actually return?
sqls = [
    "SELECT MAX(importance) AS max_imp FROM memories",
    "SELECT AVG(importance) AS avg_imp FROM memories",
    "SELECT importance FROM memories WHERE importance = 1.0 LIMIT 3",
    "SELECT content FROM memories WHERE importance = (SELECT MAX(importance) FROM memories)",
    "SELECT content, importance FROM memories WHERE importance > (SELECT AVG(importance) FROM memories) LIMIT 5",
    "SELECT content FROM memories WHERE kind IN (SELECT DISTINCT kind FROM memories WHERE importance > 0.9)",
    "SELECT content FROM memories m1 WHERE importance > (SELECT AVG(importance) FROM memories m2 WHERE m2.kind = m1.kind) LIMIT 5",
    "SELECT content FROM memories WHERE EXISTS (SELECT 1 FROM memories m2 WHERE m2.kind = memories.kind AND m2.importance > 0.9)",
]

labels = [
    "MAX(importance)", "AVG(importance)", "WHERE = 1.0 literal",
    "WHERE = scalar subquery MAX", "WHERE > scalar subquery AVG",
    "IN subquery", "correlated subquery", "EXISTS subquery"
]

results = run(sqls)
for label, text in zip(labels, results):
    icon = "❌" if "error" in text.lower()[:40] else "✅"
    rows = "(no rows)" if "(no rows)" in text else text[:150]
    print(f"\n{icon} {label}\n   {rows}")
