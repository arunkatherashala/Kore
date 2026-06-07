# Master Tracking Sheet

## Releases and Changes

- **v1.3.4 (in-progress)** — 2026-06-06
  - **Change:** Implemented type-aware decoding fallback in `kore-reader` and added decoded per-column null counting and sample printing.
  - **Files modified:** `kore-reader/src/main.rs`
  - **Build status:** Local build succeeded (`cargo build -p kore-reader --release`)
  - **Notes:** Uses `KoreReader::read_all_columns()` to decode values for accurate null counts; will add unit tests and CLI `sample` subcommand next.
  - **Author:** automated agent


