"""
KORE vs Parquet vs ORC vs CSV vs JSON — Full Format Benchmark
==============================================================
Run: python bench_all_formats.py
"""
import os, sys, time, json, csv, tempfile

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "kore-python"))

def bench(fn, repeats=10):
    times = []
    for _ in range(repeats):
        start = time.perf_counter()
        fn()
        times.append((time.perf_counter() - start) * 1000)
    return min(times)

def generate_data(n_rows=100_000):
    import random
    random.seed(42)
    return {
        "id": list(range(n_rows)),
        "price": [random.uniform(1.0, 1000.0) for _ in range(n_rows)],
        "qty": [random.randint(1, 10000) for _ in range(n_rows)],
        "region": [random.choice(["US", "EU", "APAC", "LATAM"]) for _ in range(n_rows)],
    }

def bench_kore(data, tmp_dir):
    import kore_fileformat as kf
    path = os.path.join(tmp_dir, "bench.kore")
    block = kf.DataBlock()
    # Columns are prebuilt as typed arrays (like the prebuilt Arrow table used for Parquet),
    # so the timing excludes list -> buffer conversion for both formats.
    import array
    block.add_column("id", kf.DataType.I64, array.array("q", data["id"]))
    block.add_column("price", kf.DataType.F64, array.array("d", data["price"]))
    block.add_column("qty", kf.DataType.I64, array.array("q", data["qty"]))
    block.add_column("region", kf.DataType.STR, data["region"])
    write_ms = bench(lambda: kf.write_file(path, block))
    read_ms = bench(lambda: kf.read_file(path))
    size_kb = os.path.getsize(path) / 1024
    try:
        arrow_ms = round(bench(lambda: kf.to_arrow(path)), 1)
    except ImportError:
        arrow_ms = "N/A"
    return {"format": "KORE", "write_ms": round(write_ms, 1), "read_ms": round(read_ms, 1),
            "read_arrow_ms": arrow_ms, "size_kb": round(size_kb)}

def bench_csv_fmt(data, tmp_dir):
    path = os.path.join(tmp_dir, "bench.csv")
    n = len(data["id"])
    def write():
        with open(path, "w", newline="") as f:
            w = csv.writer(f)
            w.writerow(data.keys())
            for i in range(n):
                w.writerow([data[k][i] for k in data])
    def read():
        with open(path) as f:
            list(csv.DictReader(f))
    write_ms = bench(write)
    read_ms = bench(read)
    size_kb = os.path.getsize(path) / 1024
    return {"format": "CSV", "write_ms": round(write_ms, 1), "read_ms": round(read_ms, 1), "size_kb": round(size_kb)}

def bench_json_fmt(data, tmp_dir):
    path = os.path.join(tmp_dir, "bench.json")
    n = len(data["id"])
    records = [{k: data[k][i] for k in data} for i in range(n)]
    write_ms = bench(lambda: open(path, "w").write(json.dumps(records)))
    read_ms = bench(lambda: json.loads(open(path).read()))
    size_kb = os.path.getsize(path) / 1024
    return {"format": "JSON", "write_ms": round(write_ms, 1), "read_ms": round(read_ms, 1), "size_kb": round(size_kb)}

def bench_parquet(data, tmp_dir):
    try:
        import pyarrow as pa, pyarrow.parquet as pq
    except ImportError:
        return {"format": "Parquet", "write_ms": "N/A", "read_ms": "N/A", "size_kb": "N/A"}
    path = os.path.join(tmp_dir, "bench.parquet")
    table = pa.table(data)
    write_ms = bench(lambda: pq.write_table(table, path, compression="zstd"))
    arrow_ms = bench(lambda: pq.read_table(path))
    read_ms = bench(lambda: pq.read_table(path).to_pydict())  # Python objects, like the other rows
    size_kb = os.path.getsize(path) / 1024
    return {"format": "Parquet (ZSTD)", "write_ms": round(write_ms, 1), "read_ms": round(read_ms, 1),
            "read_arrow_ms": round(arrow_ms, 1), "size_kb": round(size_kb)}

def main():
    n = 100_000
    print(f"=== KORE Format Benchmark: {n:,} rows × 4 cols ===\n")
    data = generate_data(n)
    with tempfile.TemporaryDirectory() as tmp:
        results = [bench_kore(data, tmp), bench_parquet(data, tmp), bench_csv_fmt(data, tmp), bench_json_fmt(data, tmp)]
    print("Read (Python) = columns materialised as Python objects; Read (Arrow) = Arrow table.\n")
    print(f"{'Format':<20} {'Write (ms)':>12} {'Read Python':>12} {'Read Arrow':>12} {'Size (KB)':>12}")
    print("-" * 72)
    for r in results:
        print(f"{r['format']:<20} {str(r['write_ms']):>12} {str(r['read_ms']):>12} {str(r.get('read_arrow_ms', '-')):>12} {str(r['size_kb']):>12}")
    with open("bench_all_formats_results.json", "w") as f:
        json.dump({"rows": n, "columns": 4, "results": results}, f, indent=2)
    print(f"\nSaved to bench_all_formats_results.json")

if __name__ == "__main__":
    main()
