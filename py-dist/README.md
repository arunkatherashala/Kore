# kore

Python bindings for the KORE columnar engine and the native `.kore` file format.

## Install

```bash
pip install kore-0.1.0-py3-none-any.whl
```

## Usage

```python
import kore

with kore.KoreSession() as sess:
    # Read a native .kore file
    sess.load_kore("orders", "orders.kore")

    rows = sess.query("SELECT id, total FROM orders WHERE total > 100")
    print(rows)

    # Write a table back out as .kore
    sess.save_kore("orders", "orders_copy.kore")
```

Also supported: `load_csv(table, path)`, `load_parquet(table, path)`, `row_count(table)`.

The Windows native library (`kore_ffi.dll`) is bundled inside the wheel, so no
Rust toolchain is required on the target machine.
