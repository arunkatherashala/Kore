# Tombstones & Delete Semantics (draft)

Purpose
- Describe how deletes are represented in manifests via tombstone entries, enabling logical deletes without rewriting data files immediately.

Tombstone entry
- file_path: string — data file affected
- row_id_range: [start_row, end_row] | null — optional contiguous row id range
- predicate: string | null — optional boolean expression describing rows deleted (for complex conditions)
- created_at: string — ISO8601 timestamp
- commit_id: string — commit that created the tombstone

Behavior
- A reader applies tombstones from the active manifest to filter out deleted rows during read.
- Tombstones are additive: multiple tombstones may overlap; the union is considered deleted.

Compaction
- Background compaction process reads active manifest, applies tombstones to data files and writes new compacted data file(s), then publishes a new manifest commit that replaces files and clears corresponding tombstones.

Atomicity
- Tombstones are part of the manifest and included in the atomic commit; readers that load the manifest see tombstones consistently with the snapshot.
