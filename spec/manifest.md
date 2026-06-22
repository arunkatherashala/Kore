# Kore Manifest Schema (draft)

Purpose
- Describe a snapshot/manifest file listing data files, their metadata, schema, and commit id.

Top-level fields
- version: integer — manifest schema version (start at 1)
- commit_id: string — unique commit id (uuid or timestamp+rand)
- parent_commit_id: string|null — previous commit id for history
- created_at: string — ISO8601 timestamp
- author: { name: string, email: string } | null
- schema: { columns: [ { name: string, type: string, nullable: bool, metadata?: object } ], primary_key?: [string] }
- files: [ { path: string, row_count: integer, uncompressed_size: integer, checksum?: string, block_count?: integer } ]
- tombstones: [ { file_path: string, row_id_range?: [integer, integer], predicate?: string } ]
- properties: object — free-form key/value map

Atomic commit algorithm (simple)
- Write manifest to `manifest.tmp.<commit_id>` in same directory as final `manifest.json`.
- Flush/fsync the temp file.
- Atomically rename `manifest.tmp.<commit_id>` -> `manifest.json`.
- Optionally write a small `manifest.log` append entry for history.

Notes
- For object stores without atomic rename, write `manifest-<commit_id>.json` and update a pointer file using a conditional operation (e.g., S3 object copy with If-Match) or store pointers in a sequenced index.
