"""Final verification of JOIN + EXISTS fixes."""
import subprocess, json, time

KORE = 'target/release/kore-self.exe'

def q(sql, label, timeout=45):
    msg1 = json.dumps({'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'t','version':'1'}}})
    msg2 = json.dumps({'jsonrpc':'2.0','id':2,'method':'tools/call','params':{'name':'self_query','arguments':{'sql':sql}}})
    inp = (msg1 + '\n' + msg2 + '\n').encode()
    t0 = time.perf_counter()
    try:
        p = subprocess.run([KORE, 'arun'], input=inp, capture_output=True, timeout=timeout, cwd='.')
        ms = (time.perf_counter() - t0) * 1000
        out = p.stdout.decode(errors='replace')
        lines = [l for l in out.strip().split('\n') if l.startswith('{')]
        if len(lines) >= 2:
            r = json.loads(lines[1])
            text = r.get('result', {}).get('content', [{}])[0].get('text', '')
            ok = 'Query error' not in text and 'error' not in text.lower()[:40]
            status = 'PASS' if ok else 'FAIL'
            print(f'  {status} ({ms:.0f}ms): {label}')
        else:
            print(f'  NO RESPONSE: {label}')
    except subprocess.TimeoutExpired:
        print(f'  TIMEOUT ({timeout}s): {label}')

print('=== FINAL VERIFICATION (warm cache) ===')
q('SELECT m1.id, m1.kind FROM memories m1 INNER JOIN memories m2 ON m1.kind=m2.kind LIMIT 5',
  'INNER JOIN LIMIT 5')
q('SELECT m1.id, m2.id AS m2id FROM memories m1 LEFT JOIN memories m2 ON m1.kind=m2.kind LIMIT 5',
  'LEFT JOIN LIMIT 5')
q('SELECT m1.id, m2.id FROM memories m1 FULL OUTER JOIN memories m2 ON m1.kind=m2.kind LIMIT 5',
  'FULL OUTER JOIN LIMIT 5')
q('SELECT id FROM memories m1 WHERE EXISTS (SELECT 1 FROM memories m2 WHERE m2.kind=m1.kind AND m2.importance > 7) LIMIT 5',
  'EXISTS (correlated hash semi-join)')
print('Done.')
