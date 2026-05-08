from pathlib import Path
p=Path('.github/workflows/ci.yml')
s=p.read_text(encoding='utf-8')
if not s.startswith('---\n'):
    s='---\n'+s
s=s.replace('\r\n','\n').replace('\r','\n')
p.write_text(s,encoding='utf-8', newline='\n')
print('normalized')
