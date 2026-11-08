"""One Parquet file -> a bench_rules dump (docs/kore_rule_mode.md), for data files supplied by the user.

  python -I export_parquet.py FILE.parquet OUT_DUMP_DIR KEY_COLUMN

The file is only read (pyarrow). Types as in export_tpcds.py: timestamps -> int64 in the file's unit,
integers -> int64, floats -> float64, decimals -> float64, dates -> 'YYYY-MM-DD', strings -> string.
"""
import os
import sys

import pyarrow.parquet as pq

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from export_tpcds import dump  # noqa: E402


def main():
    src, out, key = sys.argv[1], sys.argv[2], sys.argv[3]
    t = pq.read_table(src)
    print(t.num_rows, t.num_columns)
    print(t.schema)
    dump(t, out, key)


if __name__ == "__main__":
    main()
