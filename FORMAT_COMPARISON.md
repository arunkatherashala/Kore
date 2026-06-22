Kore vs Common Columnar/Row Formats — Quick Comparison

- Kore (current prototype):
  - Storage: file-backed, supports newline-row and prototype block-encoded files (KORB).
  - Columnar features: planned; currently row-oriented payloads inside blocks; prototype block-aware streaming and compaction implemented.
  - Tombstones: manifest-level tombstones (row-range + predicate strings); compaction applies row-range and simple predicate tombstones (substring match).
  - Atomic commits: manifest write-with-temp-and-rename (POSIX atomic) implemented for filesystem.
  - Strengths: simple manifest snapshots, streaming decoders, compaction prototype, DDL/DML scaffolding.
  - Limitations: real KORE codecs (FOR/packed/RLE) not fully implemented; predicate evaluation is simple substring match; object-store atomicity not implemented.

- CSV (row):
  - Simple, ubiquitous, streamable, no schema or compression built-in.
  - Fast to write/read for small datasets; poor random access and column pruning.
  - Tombstones/ACID: not native — needs sidecar manifests or a table layer.

- Parquet (columnar):
  - Columnar, strongly typed, supports encodings (DICT, RLE, BIT PACKING), row-groups, column statistics for pruning.
  - Good compression, predicate pushdown via column stats, efficient column projection.
  - Supports block-aware reads and zero-copy use with Arrow.

- Avro (row/record):
  - Schema-first binary row format; good for RPC / streaming.
  - Less optimized for analytics than Parquet.

- ORC (columnar):
  - Columnar with advanced encodings and indexes, strong for Hadoop ecosystems.
  - Efficient compression and predicate pushdown via bloom filters and column stats.

- Feather / Arrow IPC:
  - In-memory columnar data on disk, zero-copy-friendly, great for cross-language exchange.
  - Focused on memory/IPC speed rather than long-term storage semantics.

Where Kore aligns and next steps to reach parity
- Columnar encodings: implement full KORE codecs (FOR, packed, RLE) with stateful, cross-block decoders. This enables compact storage and true column pruning like Parquet/ORC.
- Predicate evaluation: implement expression evaluator against column metadata + per-row decode streaming to apply predicate tombstones efficiently (not substring-based).
- Metadata: add row-group / column stats, checksums, and per-block indexes to support pruning and faster compaction.
- Object store atomicity: add manifest staging (WAL-like) or use object-store multi-part strategies to emulate atomic commits.
- Tooling: add readers/writers for CSV/Parquet/Avro to compare performance and interoperability; or provide adapters to Arrow/Parquet for direct comparison.

Immediate actionables
- Finish codec decoders (FOR/packed/RLE) — enables true column/block-aware compaction.
- Replace substring predicate tombstones with expression-evaluator applied during streaming decode.
- Add integration benchmark harness comparing Kore compaction/read vs Parquet on sample datasets.
