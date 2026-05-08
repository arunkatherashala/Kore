from pathlib import Path
p=Path('.github/workflows/ci.yml')
b=p.read_bytes()
print('bytes length:', len(b))
if b.find(b'\r')!=-1:
    print('Found CR (\r) at positions (first 20):', [i for i,c in enumerate(b) if c==13][:20])
else:
    print('No CR found')

s=b.decode('utf-8', errors='replace')
for i,line in enumerate(s.splitlines()[:50], start=1):
    print(f'{i:03}: {repr(line)}')

# show any lines containing 'True' or 'TRUE' or 'Yes' or 'yes' or 'on:'
for i,line in enumerate(s.splitlines(), start=1):
    if any(tok in line for tok in ['True','TRUE','Yes','YES','On','ON','on:']):
        print('MATCH', i, repr(line))
