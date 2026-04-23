import gzip, shutil, sys, os

def pack(kore_path, out_path=None):
    if not out_path:
        out_path = kore_path + '.gz'
    with open(kore_path, 'rb') as f_in:
        with gzip.open(out_path, 'wb') as f_out:
            shutil.copyfileobj(f_in, f_out)
    return out_path, os.path.getsize(out_path)

if __name__ == '__main__':
    korep = sys.argv[1] if len(sys.argv) > 1 else 'sample_10mb.kore'
    out = sys.argv[2] if len(sys.argv) > 2 else None
    p = pack(korep, out)
    print(f"Packed: {p[0]} ({p[1]} bytes)")
