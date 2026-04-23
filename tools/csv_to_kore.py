import csv, sys, os

# Produce a simple KORE textual file using the bracketed nested-list form for kore_write input
# Format: first line = schema string, second line = data as [[...],[...],...]

def csv_to_kore(csv_path, kore_path, schema=None):
    with open(csv_path, 'r', newline='') as f:
        reader = csv.reader(f)
        headers = next(reader)
        rows = list(reader)
    if not schema:
        # naive type inference: all strings except numeric-like
        types = []
        for h in headers:
            types.append('str')
        schema = ','.join([f"{h}:{t}" for h,t in zip(headers, types)])
    # build data string
    parts = []
    for r in rows:
        # escape inner commas/brackets by leaving as-is; keep simple
        escaped = [val.replace(']', '\\]').replace('[', '\\[') for val in r]
        parts.append('[' + ','.join(escaped) + ']')
    data = '[' + ','.join(parts) + ']'
    with open(kore_path, 'w', encoding='utf-8') as k:
        k.write(schema + '\n')
        k.write(data + '\n')
    return kore_path, os.path.getsize(kore_path)

if __name__ == '__main__':
    csvp = sys.argv[1] if len(sys.argv) > 1 else 'sample_10mb.csv'
    korep = sys.argv[2] if len(sys.argv) > 2 else 'sample_10mb.kore'
    schema = sys.argv[3] if len(sys.argv) > 3 else None
    out = csv_to_kore(csvp, korep, schema)
    print(f"Wrote KORE: {out[0]} ({out[1]} bytes)")
