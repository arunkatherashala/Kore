# kore_fileformat

KORE — Killer Optimized Record Exchange

This crate packages the KORE file format implementations (v1 and v2), query engine, and transaction utilities as a standalone Rust library for publishing.

Quick start

- Add this crate as a dependency (when published) or include from path.
- Use the convenience functions from `kore_fileformat`:
  - `kore_fileformat::kore_write_simple(path, schema_json, data_json)`
  - `kore_fileformat::kore_read_simple(path)`
  - `kore_fileformat::kore_read_col_simple(path, col)`
  - `kore_fileformat::kore_info_simple(path)`

Publishing checklist

- Ensure `Cargo.toml` metadata is correct (authors, repository, keywords).
- Add `LICENSE` file if required (MIT by default here).
- Replace any `unimplemented!()` stubs with full implementations if you need runtime functionality.
- Run `cargo build --release` and `cargo test` to verify compilation and tests.
- Optionally add CI configuration (GitHub Actions) for `cargo test` and `cargo clippy`.

Notes

This workspace contains copies of the original KORE source files. Some long implementations were stubbed out in this initial export; if you want the full original source code included verbatim, I can replace the stubs with the complete implementations from the upstream project files.
