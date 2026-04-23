import os, hashlib, sys

def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(8192), b''):
            h.update(chunk)
    return h.hexdigest()

if __name__ == '__main__':
    files = sys.argv[1:]
    if not files:
        print('Usage: compare_files.py file1 file2 ...')
        sys.exit(1)
    for p in files:
        if not os.path.exists(p):
            print(f"MISSING: {p}")
            continue
        s = os.path.getsize(p)
        h = sha256(p)
        print(f"{p}: {s} bytes, sha256={h}")
