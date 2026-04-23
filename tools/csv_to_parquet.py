import sys, os

def csv_to_parquet(csv_path, parquet_path=None):
    try:
        import pandas as pd
    except Exception as e:
        raise RuntimeError('pandas is required: pip install pandas pyarrow')
    if parquet_path is None:
        parquet_path = os.path.splitext(csv_path)[0] + '.parquet'
    try:
        # prefer pyarrow engine
        df = pd.read_csv(csv_path)
        df.to_parquet(parquet_path, engine='pyarrow', index=False)
    except Exception as e:
        raise
    return parquet_path, os.path.getsize(parquet_path)

if __name__ == '__main__':
    csvp = sys.argv[1] if len(sys.argv) > 1 else 'sample_10mb.csv'
    out = sys.argv[2] if len(sys.argv) > 2 else None
    p = csv_to_parquet(csvp, out)
    print(f'Wrote PARQUET: {p[0]} ({p[1]} bytes)')
