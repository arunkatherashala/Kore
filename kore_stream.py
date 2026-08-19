#!/usr/bin/env python3
"""
KORE Live Stream — append rows to .hkore files in real-time.
World's first human-readable streaming columnar format.

Usage:
  writer = KoreStream("metrics.hkore", schema={"ts": "I64", "cpu": "F64", "host": "STR"})
  writer.append({"ts": 1723680000, "cpu": 45.2, "host": "srv-01"})
  writer.append({"ts": 1723680001, "cpu": 67.8, "host": "srv-02"})
  writer.flush()  # write to disk

  # Read stream
  for batch in KoreStreamReader("metrics.hkore"):
      print(batch)
"""
import sys, os, struct, time, array, threading
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), 'kore-python'))
import kore_fileformat as kore

_STREAM_MAGIC = b'KSTR'
_BATCH_MARKER = b'KBAT'


class KoreStream:
    """Append-only streaming writer for .hkore files."""

    def __init__(self, path, schema, batch_size=1000):
        self.path = path
        self.schema = schema  # {"col": "I64"|"F64"|"STR"}
        self.batch_size = batch_size
        self._buffer = {col: [] for col in schema}
        self._row_count = 0
        self._batch_count = 0
        self._lock = threading.Lock()

        # Write stream header
        if not os.path.exists(path):
            with open(path, 'wb') as f:
                f.write(_STREAM_MAGIC)
                f.write(struct.pack('<I', len(schema)))
                for col, dtype in schema.items():
                    name_b = col.encode('utf-8')
                    dtype_b = {'I64': 0, 'F64': 1, 'STR': 2}[dtype]
                    f.write(struct.pack('<BH', dtype_b, len(name_b)))
                    f.write(name_b)

    def append(self, row):
        """Append a single row. Auto-flushes at batch_size."""
        with self._lock:
            for col in self.schema:
                self._buffer[col].append(row.get(col))
            self._row_count += 1
            if self._row_count % self.batch_size == 0:
                self._write_batch()

    def append_batch(self, rows):
        """Append multiple rows at once."""
        with self._lock:
            for row in rows:
                for col in self.schema:
                    self._buffer[col].append(row.get(col))
                self._row_count += 1
            self._write_batch()

    def flush(self):
        """Flush any remaining buffered rows to disk."""
        with self._lock:
            if any(len(v) > 0 for v in self._buffer.values()):
                self._write_batch()

    def _write_batch(self):
        n = len(next(iter(self._buffer.values())))
        if n == 0:
            return

        with open(self.path, 'ab') as f:
            f.write(_BATCH_MARKER)
            f.write(struct.pack('<I', n))
            ts = int(time.time() * 1000)
            f.write(struct.pack('<Q', ts))

            for col, dtype in self.schema.items():
                data = self._buffer[col]
                if dtype == 'I64':
                    buf = array.array('q', [v if v is not None else 0 for v in data])
                    f.write(buf.tobytes())
                elif dtype == 'F64':
                    buf = array.array('d', [v if v is not None else 0.0 for v in data])
                    f.write(buf.tobytes())
                elif dtype == 'STR':
                    for s in data:
                        sb = (str(s) if s is not None else '').encode('utf-8')
                        f.write(struct.pack('<I', len(sb)))
                        f.write(sb)

        self._batch_count += 1
        self._buffer = {col: [] for col in self.schema}

    @property
    def total_rows(self):
        return self._row_count

    @property
    def batches(self):
        return self._batch_count


class KoreStreamReader:
    """Read batches from a streaming .hkore file."""

    def __init__(self, path):
        self.path = path

    def __iter__(self):
        with open(self.path, 'rb') as f:
            magic = f.read(4)
            if magic != _STREAM_MAGIC:
                raise ValueError("Not a KORE stream file")

            ncols = struct.unpack('<I', f.read(4))[0]
            schema = []
            for _ in range(ncols):
                dtype_b, name_len = struct.unpack('<BH', f.read(3))
                name = f.read(name_len).decode('utf-8')
                dtype = {0: 'I64', 1: 'F64', 2: 'STR'}[dtype_b]
                schema.append((name, dtype))

            while True:
                marker = f.read(4)
                if len(marker) < 4:
                    break
                if marker != _BATCH_MARKER:
                    break

                nrows = struct.unpack('<I', f.read(4))[0]
                ts = struct.unpack('<Q', f.read(8))[0]

                batch = {"_timestamp_ms": ts, "_rows": nrows}
                for col_name, dtype in schema:
                    if dtype == 'I64':
                        a = array.array('q')
                        a.fromfile(f, nrows)
                        batch[col_name] = list(a)
                    elif dtype == 'F64':
                        a = array.array('d')
                        a.fromfile(f, nrows)
                        batch[col_name] = list(a)
                    elif dtype == 'STR':
                        strings = []
                        for _ in range(nrows):
                            slen = struct.unpack('<I', f.read(4))[0]
                            strings.append(f.read(slen).decode('utf-8'))
                        batch[col_name] = strings

                yield batch

    def read_all(self):
        """Read all batches into a single DataBlock."""
        all_data = {}
        total = 0
        for batch in self:
            for k, v in batch.items():
                if k.startswith('_'):
                    continue
                if k not in all_data:
                    all_data[k] = []
                all_data[k].extend(v)
            total += batch['_rows']

        block = kore.DataBlock()
        for name, values in all_data.items():
            if all(isinstance(v, int) for v in values):
                block.add_column(name, kore.DataType.I64, values)
            elif all(isinstance(v, (int, float)) for v in values):
                block.add_column(name, kore.DataType.F64, values)
            else:
                block.add_column(name, kore.DataType.STR, values)
        block.num_rows = total
        return block


if __name__ == "__main__":
    import time as _time

    print("=== KORE Live Streaming Demo ===")
    print()

    path = "C:/tmp/stream_demo.kore"
    schema = {"ts": "I64", "cpu": "F64", "host": "STR", "mem_mb": "I64"}

    writer = KoreStream(path, schema, batch_size=100)

    hosts = ["srv-01", "srv-02", "srv-03", "web-01", "web-02", "db-01"]
    import random

    print("Writing 1000 streaming events...")
    t0 = _time.perf_counter()
    for i in range(1000):
        writer.append({
            "ts": int(_time.time()) + i,
            "cpu": random.uniform(0, 100),
            "host": random.choice(hosts),
            "mem_mb": random.randint(1024, 32768),
        })
    writer.flush()
    w_ms = (_time.perf_counter() - t0) * 1000

    print(f"  Written: {writer.total_rows} rows in {writer.batches} batches ({w_ms:.1f}ms)")
    print(f"  File: {os.path.getsize(path)/1024:.1f} KB")

    print()
    print("Reading stream back...")
    t0 = _time.perf_counter()
    reader = KoreStreamReader(path)
    block = reader.read_all()
    r_ms = (_time.perf_counter() - t0) * 1000
    print(f"  Read: {block.num_rows} rows ({r_ms:.1f}ms)")
    print(f"  Columns: {[c.name for c in block.columns]}")
    print(f"  Sample: host={block.columns[2].data[0]}, cpu={block.columns[1].data[0]:.1f}%")

    print()
    print("Batch-by-batch streaming read:")
    for i, batch in enumerate(KoreStreamReader(path)):
        if i < 3:
            print(f"  Batch {i}: {batch['_rows']} rows, ts={batch['_timestamp_ms']}")
        elif i == 3:
            print(f"  ... ({writer.batches} total batches)")
            break
