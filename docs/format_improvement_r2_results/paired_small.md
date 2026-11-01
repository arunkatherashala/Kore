#### N=10,000 paired ratios (AFTER / BEFORE, same round)

| metric | f2 / r1 | f3 / r1 | rounds |
|---|---|---|---|
| bulk build events/s | 1.41 [1.18-1.48] | 1.51 [1.21-1.68] | 5 |
| open() | 0.50 [0.31-1.72] | 0.49 [0.20-1.14] | 5 |
| open() + decode latest | 0.98 [0.82-1.70] | 0.97 [0.39-1.17] | 5 |
| verify() | 0.95 [0.78-2.68] | 0.88 [0.74-2.18] | 5 |
| get_event(id) | 0.49 [0.47-0.59] | 0.50 [0.36-0.53] | 5 |
| append 1 event/commit | 0.98 [0.95-1.00] | 0.72 [0.64-1.13] | 5 |
| append 1000-event batch | 1.00 [0.87-1.01] | 0.72 [0.65-0.87] | 5 |
| re-ingest same events (s) | 0.66 [0.59-0.73] | 0.63 [0.55-0.70] | 5 |
| total size on disk | 1.00 [1.00-1.00] | 1.00 [1.00-1.00] | 5 |
| as-of mid version, open_at | 1.67 [0.41-2.24] | 1.01 [0.68-1.76] | 5 |

#### N=100,000 paired ratios (AFTER / BEFORE, same round)

| metric | f2 / r1 | f3 / r1 | rounds |
|---|---|---|---|
| bulk build events/s | 1.23 [1.12-1.28] | 1.33 [1.18-1.38] | 5 |
| open() | 1.25 [0.98-1.28] | 1.16 [0.87-1.27] | 5 |
| open() + decode latest | 0.97 [0.89-1.01] | 0.88 [0.76-1.26] | 5 |
| verify() | 0.95 [0.89-1.11] | 1.00 [0.92-1.12] | 5 |
| get_event(id) | 0.51 [0.49-0.61] | 0.53 [0.48-0.57] | 5 |
| append 1 event/commit | 1.02 [0.99-1.16] | 0.67 [0.66-0.70] | 5 |
| append 1000-event batch | 0.95 [0.89-1.03] | 0.69 [0.68-0.77] | 5 |
| re-ingest same events (s) | 0.66 [0.62-0.83] | 0.66 [0.63-0.76] | 5 |
| total size on disk | 1.00 [1.00-1.00] | 1.00 [1.00-1.00] | 5 |
| as-of mid version, open_at | 0.95 [0.72-1.74] | 0.95 [0.75-1.22] | 5 |

