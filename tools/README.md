Manifest writer prototype

Run with:

```bash
# install dependencies
cargo install serde_json chrono
# or use a small Cargo project. Quick run with rust-script or compile with rustc and add crates accordingly.

rustc tools/manifest_writer.rs -L . && ./manifest_writer
```

This prototype writes `manifest.tmp.<commit_id>.json` then atomically renames it to `manifest.json`.
