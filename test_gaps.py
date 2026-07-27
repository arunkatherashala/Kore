import subprocess, json

KORE = r"C:\Users\skathera\Downloads\asistent\kore\target\debug\kore-self.exe"

def q(id, tool, args):
    return json.dumps({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":tool,"arguments":args}})

msgs = [
    json.dumps({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
    q(2, "self_dml", {"sql": "CREATE TABLE decisions AS SELECT id, content, importance FROM memories WHERE kind='decision'"}),
    q(3, "self_dml", {"sql": "INSERT INTO decisions VALUES (999, 'KORE beats Spark - new confirmed benchmark', 0.99)"}),
    q(4, "self_dml", {"sql": "DELETE FROM decisions WHERE importance < 0.9"}),
    q(5, "self_save", {"path": r"C:\Users\skathera\Downloads\asistent\kore\memories_export.kore"}),
    q(6, "self_query", {"sql": "SELECT DISTINCT kind FROM memories ORDER BY kind"}),
    q(7, "self_query", {"sql": "SELECT kind, NTILE(4) OVER (ORDER BY importance DESC) AS quartile, importance FROM memories LIMIT 6"}),
]
inp = "\n".join(msgs).encode()
p = subprocess.run([KORE, "arun"], input=inp, capture_output=True, timeout=20,
                   cwd=r"C:\Users\skathera\Downloads\asistent\kore")
labels = {2:"CREATE TABLE AS SELECT (DML)",3:"INSERT INTO VALUES (DML)",4:"DELETE FROM WHERE (DML)",5:"self_save native .kore",6:"SELECT DISTINCT",7:"NTILE(4) quartile"}
for line in p.stdout.decode(errors="replace").strip().split("\n"):
    try:
        r = json.loads(line)
        if r.get("id",0) >= 2:
            text = r["result"]["content"][0]["text"][:300]
            icon = "❌" if "error" in text.lower()[:30] else "✅"
            print(f"\n{icon} {labels.get(r['id'], r['id'])}")
            print(text)
    except: pass

# Check file was created
import os
path = r"C:\Users\skathera\Downloads\asistent\kore\memories_export.kore"
if os.path.exists(path):
    print(f"\n✅ memories_export.kore: {os.path.getsize(path):,} bytes on disk")
else:
    print("\n❌ memories_export.kore NOT created")
