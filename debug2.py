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
                results.append((r["id"]-2, r["result"]["content"][0]["text"]))
        except: pass
    return results

sqls = [
    # Works:
    "SELECT importance FROM memories WHERE importance = 1.0 LIMIT 3",
    # Fails (should return same):
    "SELECT importance FROM memories WHERE importance = (SELECT MAX(importance) FROM memories)",
    # Does MAX work in subquery context?
    "SELECT (SELECT MAX(importance) FROM memories) AS max_val FROM memories LIMIT 1",
    # Maybe resolve_subqueries isn't running? Try explicit scalar in CTE:
    "WITH m AS (SELECT MAX(importance) AS mv FROM memories) SELECT content FROM memories, m WHERE importance = mv LIMIT 3",
    # Test with simple constant subquery
    "SELECT importance FROM memories WHERE importance > (SELECT 0.8 FROM memories LIMIT 1)",
]

results = run(sqls)
labels = [
    "literal 1.0 (control)", 
    "WHERE = (SELECT MAX)",
    "scalar subquery in projection",
    "CTE workaround for scalar subquery",
    "WHERE > (SELECT 0.8 constant)",
]

for i, text in results:
    print(f"\n{'✅' if 'error' not in text.lower()[:40] else '❌'} {labels[i]}")
    print(f"   {text[:200]}")
