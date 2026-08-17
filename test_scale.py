"""KORE vs Parquet — Scale Test: 10M rows (pure I/O, no Spark overhead)"""
import sys, os, time, array
os.environ['HADOOP_HOME'] = 'C:\\hadoop'
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), 'kore-python'))
import kore_fileformat as kore

os.makedirs('C:/tmp', exist_ok=True)

for N in [1_000_000, 10_000_000]:
    print(f"\n{'='*70}")
    print(f"  SCALE TEST: {N:,} rows x 2 numeric cols")
    print(f"{'='*70}")

    prices = array.array('d', (float(i) * 1.5 for i in range(N)))
    volumes = array.array('q', range(N))

    # .hkore write
    b = kore.DataBlock()
    b.add_column('price', kore.DataType.F64, prices)
    b.add_column('vol', kore.DataType.I64, volumes)
    t0 = time.perf_counter()
    kore.write_hybrid(f'C:/tmp/scale_{N}.hkore', b)
    hkore_w = (time.perf_counter() - t0) * 1000
    hkore_kb = os.path.getsize(f'C:/tmp/scale_{N}.hkore') / 1024

    # .hkore read
    t0 = time.perf_counter()
    kore.read_hybrid(f'C:/tmp/scale_{N}.hkore')
    hkore_r = (time.perf_counter() - t0) * 1000

    # .parquet write (via PyArrow)
    import pyarrow as pa, pyarrow.parquet as pq
    table = pa.table({'price': prices, 'vol': volumes})
    t0 = time.perf_counter()
    pq.write_table(table, f'C:/tmp/scale_{N}.parquet', compression='NONE')
    pq_w = (time.perf_counter() - t0) * 1000
    pq_kb = os.path.getsize(f'C:/tmp/scale_{N}.parquet') / 1024

    # .parquet read
    t0 = time.perf_counter()
    pq.read_table(f'C:/tmp/scale_{N}.parquet')
    pq_r = (time.perf_counter() - t0) * 1000

    # .parquet snappy
    t0 = time.perf_counter()
    pq.write_table(table, f'C:/tmp/scale_{N}_snappy.parquet', compression='SNAPPY')
    pqs_w = (time.perf_counter() - t0) * 1000
    pqs_kb = os.path.getsize(f'C:/tmp/scale_{N}_snappy.parquet') / 1024
    t0 = time.perf_counter()
    pq.read_table(f'C:/tmp/scale_{N}_snappy.parquet')
    pqs_r = (time.perf_counter() - t0) * 1000

    # .kore write (via pip KoreWriter if available)
    try:
        import csv
        csv_path = f'C:/tmp/_scale_{N}.csv'
        with open(csv_path, 'w', newline='') as f:
            cw = csv.writer(f); cw.writerow(['price','vol'])
            for i in range(N): cw.writerow([prices[i], volumes[i]])
        from kore_fileformat import KoreWriter, KoreReader
        t0 = time.perf_counter()
        w = KoreWriter(f'C:/tmp/scale_{N}.kore')
        w.write_csv(csv_path)
        kore_w = (time.perf_counter() - t0) * 1000
        kore_kb = os.path.getsize(f'C:/tmp/scale_{N}.kore') / 1024
        t0 = time.perf_counter()
        r = KoreReader(f'C:/tmp/scale_{N}.kore')
        r.read_columns()
        kore_r = (time.perf_counter() - t0) * 1000
        has_kore = True
    except:
        has_kore = False
        kore_w = kore_r = kore_kb = 0

    # Results
    print(f"\n  {'Format':<20} {'Write ms':>10} {'Read ms':>10} {'Size KB':>10} {'ns/row R':>10}")
    print(f"  {'-'*55}")
    if has_kore:
        print(f"  {'KORE .kore':<20} {kore_w:>10.1f} {kore_r:>10.1f} {kore_kb:>10.0f} {kore_r*1e6/N:>10.0f}")
    print(f"  {'KORE .hkore':<20} {hkore_w:>10.1f} {hkore_r:>10.1f} {hkore_kb:>10.0f} {hkore_r*1e6/N:>10.0f}")
    print(f"  {'Parquet (none)':<20} {pq_w:>10.1f} {pq_r:>10.1f} {pq_kb:>10.0f} {pq_r*1e6/N:>10.0f}")
    print(f"  {'Parquet (snappy)':<20} {pqs_w:>10.1f} {pqs_r:>10.1f} {pqs_kb:>10.0f} {pqs_r*1e6/N:>10.0f}")

    print(f"\n  KORE .hkore vs Parquet:")
    print(f"    Write: {'KORE' if hkore_w < pq_w else 'Parquet'} wins ({min(hkore_w,pq_w):.0f}ms vs {max(hkore_w,pq_w):.0f}ms)")
    print(f"    Read:  {'KORE' if hkore_r < pq_r else 'Parquet'} wins ({min(hkore_r,pq_r):.0f}ms vs {max(hkore_r,pq_r):.0f}ms)")
    print(f"    Size:  {'Parquet' if pq_kb < hkore_kb else 'KORE'} smaller ({min(pq_kb,hkore_kb):.0f}KB vs {max(pq_kb,hkore_kb):.0f}KB)")

print(f"\n{'='*70}")
print("  DONE!")
print(f"{'='*70}")
