# KORE conformance fixtures

Every file here holds the **same logical table** (3000 rows: `i` i64, `f` f64, `s` string, `b` bool, `q` i64,
with nulls, NaN, `-0.0`, `i64::MIN/MAX`, empty and non-ASCII strings). A reader is conformant when each file
decodes to the digests in `expected.txt`.

| File | What it exercises |
|---|---|
| `default.kore` | default writer (LZ4/ZSTD per column, per-column CRC32) |
| `shuffle.kore` | codec 7: byte-shuffled ZSTD (`KORE_SHUFFLE=1`) |
| `strlen.kore` | codec 8: length-prefixed strings (`KORE_STR_LENGTHS=1`) |
| `shuffle_strlen.kore` | both |
| `encrypted.kore` | `KENC` wrapper, AES-256-GCM, PBKDF2-HMAC-SHA256 (100k iterations); password `kore-conformance` |
| `versions.kore` | `KVLG` version log: version @100 is a 2-row table, version @200 is the table above |

## Digest

`expected.txt` is `rows N` followed by `name type nulls fnv64hex` per column. The digest is FNV-1a 64
(offset `0xcbf29ce484222325`, prime `0x100000001b3`) over, for every row in order:

- null: byte `0x00`
- otherwise byte `0x01`, then: i64 = 8 bytes LE; f64 = 8 bytes LE of the IEEE bits (**every NaN is hashed as
  `0x7FF8000000000001`**); bool = 1 byte; string = u32 LE byte length + UTF-8 bytes.

## Regenerating

`KORE_REGEN_FIXTURES=1 cargo test -p kore-store --test conformance` rewrites the files and `expected.txt`.
Do that only when the format intentionally changes. Rust (`kore-store/tests/conformance.rs`) and Python
(`kore-python/test_conformance.py`) both check these files; a new language binding should add the same check.
