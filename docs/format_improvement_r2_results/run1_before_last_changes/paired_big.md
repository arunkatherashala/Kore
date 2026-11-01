#### N=1,000,000 paired ratios (AFTER / BEFORE, same round)

| metric | f2 / r1 | f3 / r1 | rounds |
|---|---|---|---|
| bulk build events/s | 1.30 [1.21-1.32] | 1.44 [1.37-1.55] | 3 |
| open() | 0.86 [0.86-0.92] | 1.14 [0.72-1.20] | 3 |
| open() + decode latest | 0.90 [0.88-0.96] | 0.99 [0.78-1.15] | 3 |
| verify() | 0.93 [0.62-1.20] | 1.20 [0.74-1.26] | 3 |
| get_event(id) | 0.50 [0.37-0.62] | 0.38 [0.37-0.57] | 3 |
| append 1 event/commit | 1.18 [1.03-1.42] | 0.41 [0.39-0.44] | 3 |
| append 1000-event batch | 0.93 [0.71-1.36] | 0.53 [0.47-0.57] | 3 |
| re-ingest same events (s) | 1.20 [1.01-1.57] | 0.58 [0.53-0.59] | 3 |
| total size on disk | 1.00 [1.00-1.00] | 1.00 [1.00-1.00] | 3 |
| as-of mid version, open_at | 0.94 [0.92-1.19] | 0.97 [0.96-1.03] | 3 |

