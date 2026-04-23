kore_fileformat_killer
======================

Goal
----
Provide a pure-Killer implementation of the KORE fileformat and a canonical place
for Killer-native code and tests. This folder is the starting scaffold for that
work.

Contents
--------
- `implementation.killer` — minimal pure-Killer reference API (text serialization helper + `kore_write_text`).
- `test_kore_fileformat_killer.killer` — a small test that writes a sample KORE-like text file and attempts a `kore_info` check.

Notes & next steps
------------------
- This is intentionally small: the full KORE binary encoder (dictionary/RLE/delta/etc.) requires implementing column encoders in Killer. Next steps:
  1. Expand `implementation.killer` with columnar encoders (DictRLE, Delta, FOR, HuffDict).
  2. Implement chunking and metadata headers to match the Rust `kore_v2` format.
  3. Add parity tests that compare Killer-produced KORE to Rust `kore_bench` outputs on sample CSVs.
  4. Add benchmarks and CI that build and publish Killer artifacts.

How to run
----------
Run the test with the release Killer runtime (example):

```powershell
& "..\..\killer\SOURCE\src\v2-rust\killer\target\release\killer-native.exe" ".\kore_fileformat_killer\test_kore_fileformat_killer.killer"
```

If your runtime lacks file I/O builtins, use the Rust `kore_bench` to generate KORE files and use Killer scripts for read/verify.

Contributions
-------------
Pull requests welcome: focus on encoder cross-checks, deterministic header formats, and compatibility with `kore_info` produced by the runtime.
