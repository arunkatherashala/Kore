#### N=1,000,000 paired ratios (AFTER / BEFORE, same round)

| metric | f2 / r1 | f3 / r1 | rounds |
|---|---|---|---|
| bulk build events/s | 1.35 [1.19-1.44] | 1.42 [1.33-1.56] | 3 |
| open() | 0.98 [0.89-1.05] | 1.20 [0.98-1.31] | 3 |
| open() + decode latest | 0.92 [0.89-1.00] | 1.02 [0.94-1.07] | 3 |
| verify() | 0.93 [0.88-1.61] | 0.93 [0.78-0.95] | 3 |
| get_event(id) | 0.44 [0.42-0.50] | 0.46 [0.45-0.57] | 3 |
| append 1 event/commit | 1.12 [0.93-1.17] | 0.41 [0.37-0.45] | 3 |
| append 1000-event batch | 0.89 [0.60-1.03] | 0.48 [0.27-0.69] | 3 |
| re-ingest same events (s) | 0.44 [0.41-0.49] | 0.51 [0.47-0.52] | 3 |
| total size on disk | 1.00 [1.00-1.00] | 1.00 [1.00-1.00] | 3 |
| as-of mid version, open_at | 0.76 [0.74-1.05] | 0.86 [0.79-0.88] | 3 |

