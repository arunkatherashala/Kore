#### N=10,000 paired ratios (AFTER / BEFORE, same round)

| metric | f2 / r1 | f3 / r1 | rounds |
|---|---|---|---|
| bulk build events/s | 1.38 [0.79-1.53] | 1.42 [1.02-1.59] | 5 |
| open() | 0.89 [0.51-1.94] | 1.00 [0.37-3.63] | 5 |
| open() + decode latest | 0.77 [0.28-1.54] | 1.14 [0.27-1.61] | 5 |
| verify() | 1.22 [0.59-1.66] | 1.16 [0.69-2.11] | 5 |
| get_event(id) | 0.58 [0.49-0.71] | 0.50 [0.46-0.81] | 5 |
| append 1 event/commit | 1.06 [0.92-1.12] | 0.74 [0.59-0.90] | 5 |
| append 1000-event batch | 0.95 [0.93-1.03] | 0.90 [0.70-1.03] | 5 |
| re-ingest same events (s) | 0.64 [0.55-0.79] | 0.72 [0.54-1.23] | 5 |
| total size on disk | 1.00 [1.00-1.00] | 1.00 [1.00-1.00] | 5 |
| as-of mid version, open_at | 0.57 [0.39-3.44] | 0.89 [0.43-1.38] | 5 |

#### N=100,000 paired ratios (AFTER / BEFORE, same round)

| metric | f2 / r1 | f3 / r1 | rounds |
|---|---|---|---|
| bulk build events/s | 1.29 [1.22-1.61] | 1.39 [1.16-1.57] | 5 |
| open() | 0.65 [0.47-1.68] | 1.07 [0.50-2.33] | 5 |
| open() + decode latest | 0.93 [0.85-1.55] | 0.98 [0.90-1.69] | 5 |
| verify() | 0.97 [0.75-1.38] | 1.29 [0.87-1.57] | 5 |
| get_event(id) | 0.45 [0.42-0.76] | 0.56 [0.52-0.88] | 5 |
| append 1 event/commit | 1.02 [0.95-1.05] | 0.64 [0.59-0.67] | 5 |
| append 1000-event batch | 0.97 [0.82-1.25] | 0.70 [0.56-0.82] | 5 |
| re-ingest same events (s) | 0.73 [0.63-0.84] | 0.68 [0.57-0.93] | 5 |
| total size on disk | 1.00 [1.00-1.00] | 1.00 [1.00-1.00] | 5 |
| as-of mid version, open_at | 1.00 [0.63-1.50] | 1.09 [0.76-1.86] | 5 |

