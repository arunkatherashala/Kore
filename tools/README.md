This folder contains small scripts to exercise CSV→KORE conversion and packing for size/checksum comparison.

Usage examples (from repository root or this folder):

Generate ~10MB CSV:

```powershell
python tools\generate_csv.py sample_10mb.csv 10
```

Convert CSV to KORE:

```powershell
python tools\csv_to_kore.py sample_10mb.csv sample_10mb.kore
```

Gzip-pack the KORE file:

```powershell
python tools\pack_kore.py sample_10mb.kore
```

Compare sizes and checksums:

```powershell
python tools\compare_files.py sample_10mb.csv sample_10mb.kore sample_10mb.kore.gz
```
