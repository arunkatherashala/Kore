import csv, random, string, os, sys

def rand_name():
    return ''.join(random.choices(string.ascii_letters, k=8))

def gen_row(i):
    return [rand_name(), str(random.randint(18,80)), f"{random.uniform(30000,150000):.2f}", random.choice(['Engineering','Sales','HR','Marketing','Ops','QA','Support'])]

def generate(path, target_mb=10):
    target_bytes = target_mb * 1024 * 1024
    with open(path, 'w', newline='') as f:
        writer = csv.writer(f)
        writer.writerow(['name','age','salary','dept'])
        total = f.tell()
        i = 0
        while total < target_bytes:
            writer.writerow(gen_row(i))
            if i % 1000 == 0:
                f.flush()
            total = f.tell()
            i += 1
    return path, os.path.getsize(path)

if __name__ == '__main__':
    out = sys.argv[1] if len(sys.argv) > 1 else 'sample_10mb.csv'
    mb = int(sys.argv[2]) if len(sys.argv) > 2 else 10
    path, size = generate(out, mb)
    print(f"Wrote CSV: {path} ({size} bytes)")
