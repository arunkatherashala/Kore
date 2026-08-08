import subprocess, json, time

KORE = r"C:\Users\skathera\Downloads\asistent\kore\target\debug\kore-self.exe"
CWD  = r"C:\Users\skathera\Downloads\asistent\kore"

def run_tool(tool, args, timeout=30):
    init = json.dumps({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}})
    msg  = json.dumps({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":args}})
    t0 = time.perf_counter()
    p  = subprocess.run([KORE,"arun"], input=(init+"\n"+msg+"\n").encode(), capture_output=True, timeout=timeout, cwd=CWD)
    elapsed = (time.perf_counter()-t0)*1000
    for line in p.stdout.decode(errors="replace").split("\n"):
        try:
            r = json.loads(line)
            if r.get("id")==2:
                return r["result"]["content"][0]["text"], elapsed
        except: pass
    return "ERROR", elapsed

print("="*65)
print("  KORE Distributed SQL — Rayon vs TRUE TCP Cluster")
print("="*65)

sql = "SELECT kind, COUNT(*) AS cnt FROM memories GROUP BY kind ORDER BY cnt DESC"

# Mode 1: Rayon parallel
print(f"\n  Mode 1: Rayon (in-process parallel, all CPU cores)")
result, ms = run_tool("self_distributed_query", {"sql": sql, "cluster": False})
try:
    data = json.loads(result)
    print(f"  Result: {data['rows']} rows in {ms:.0f}ms")
    print(f"  Engine: {data['engine']}")
    print(f"  Mode:   {data['mode']}")
    print(f"  Data:\n    {data['data']}")
except:
    print(f"  {result[:200]}")

# Mode 2: TRUE TCP cluster
print(f"\n  Mode 2: TRUE TCP Cluster (coordinator + workers via TCP sockets)")
print(f"  [Starting coordinator + workers...]")
result, ms = run_tool("self_distributed_query", {"sql": sql, "cluster": True}, timeout=60)
try:
    data = json.loads(result)
    print(f"  Result: {data['rows']} rows in {ms:.0f}ms")
    print(f"  Engine: {data['engine']}")
    print(f"  Mode:   {data['mode']}")
    print(f"  Data:\n    {data['data']}")
except:
    print(f"  {result[:300]}")

print("\n" + "="*65)
print("  KORE multi-node status:")
print("  - TCP cluster: coordinator + workers use REAL TCP sockets")
print("  - Same code works on multi-machine: workers connect to coordinator IP")
print("  - Currently: workers on same machine (localhost)")
print("  - For true multi-machine: run workers on remote hosts pointing to")
print("    coordinator's IP instead of 127.0.0.1")
print("  Scorecard: KORE multi-node = PARTIAL (TCP infra ready, network WIP)")
print("="*65)
