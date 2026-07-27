import subprocess, json

KORE = r"C:\Users\skathera\Downloads\asistent\kore\target\debug\kore-self.exe"
CWD  = r"C:\Users\skathera\Downloads\asistent\kore"

def run_tool(tool, args):
    init = json.dumps({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}})
    msg  = json.dumps({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":args}})
    p    = subprocess.run([KORE,"arun"], input=(init+"\n"+msg+"\n").encode(), capture_output=True, timeout=15, cwd=CWD)
    for line in p.stdout.decode(errors="replace").split("\n"):
        try:
            r = json.loads(line)
            if r.get("id")==2: return r["result"]["content"][0]["text"]
        except: pass
    return "NO RESPONSE"

print("Bug 1: AVG+MIN+MAX without AS keyword:")
r = run_tool("self_query", {"sql": "SELECT AVG(importance) avg, MIN(importance) mn, MAX(importance) mx FROM memories"})
print(r[:200])

print("\nBug 1b: AVG+MIN+MAX WITH AS keyword (should work):")
r = run_tool("self_query", {"sql": "SELECT AVG(importance) AS avg_imp, MIN(importance) AS min_imp, MAX(importance) AS max_imp FROM memories"})
print(r[:200])

print("\nBug 2: CREATE TABLE AS SELECT via self_dml:")
r = run_tool("self_dml", {"sql": "CREATE TABLE t1 AS SELECT id, importance FROM memories WHERE kind='decision'"})
print(r[:300])

print("\nBug 2b: INSERT into existing table:")
r = run_tool("self_dml", {"sql": "INSERT INTO decisions SELECT id, content FROM memories WHERE kind='decision'"})
print(r[:200])
