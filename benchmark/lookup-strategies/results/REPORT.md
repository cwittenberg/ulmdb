# Lookup strategy benchmark

This campaign evaluates optional read accelerators. The experiment adds no production engine, dependency, default, file format or published performance-report changes.

Machine: AMD EPYC 7702P 64-Core Processor; Linux 6.8.0-147-generic; Rust 1.94.0; CPU 12. Shared host, no exclusive CPU reservation. 
3 fresh processes per configuration; 7 samples per process; seeds 73 through 75. In-process candidates alternate order; the ordered bbtree helper runs as a block before/after native samples, rotating by seed/key kind. All samples are retained. Values summarize process medians; ranges are observed process speedups, not confidence intervals.

Speedup is baseline time / candidate time. Values below 1 are slower. The prespecified material-improvement criterion is at least 1.10x in every fresh process. This is a screening criterion, not statistical equivalence or a universal no-regression guarantee.

## Comparable scope

- Prefix: actual prepared scalar binary radix versus existing checked 4-bit multibit implementation, in the same checked contiguous arena. Index-only, warm, single thread; real persistence and transactions are outside these measurements. Exact, LPM, and complete ancestor results match byte-for-byte, including interior/exterior misses and nested hot chains.
- Exact: actual file-backed prepared Legacy and Resident readers versus snapshot-bound std HashMap with randomized hashing, copied keys and borrowed values. Both backends are built with the accepted Resident feature set; Legacy here is feature-enabled. Complete cursor enumeration, allocation and copying are charged to index construction. Range scans use the original ordered reader.
- Ordered: default-feature public Legacy, std BTreeMap and actual bbtree 0.2.0. Bbtree uses a separate helper over an exact snapshot export because its Verus dependency pins conflict with ULMDB. Helper startup/IPC is outside lookup samples; export/read/decode is listed in build costs. Its storage differs from a live ULMDB mmap, so this does not qualify in-process integration.
- Exact and ordered use different key distributions, value sizes and compilation features. Compare each candidate with its own baseline, not absolute times between those two harnesses.
- Fixed synthetic fixtures, warm snapshots and one core do not establish cold-cache, larger-than-RAM, concurrency, write/recovery or real IPDon end-to-end performance. PlanB's published benchmark was not reproduced.

The hash comparison and scan fallback controls run with the side index already resident. They do not measure cache/RSS effects against a source-only process. Old/new snapshot checks prove byte isolation, not bounded retained generations or unchanged writer/reclamation cost. Build timings exclude later destruction of the old index, and a rebuild can retain both indexes plus their snapshots. Eager full rebuild after every commit would add the reported O(N) rebuild cost. Automatic promotion is not qualified by these results.

## Lookup timings

| Suite | Records | Backend/family | Keys | Operation | Distribution | Candidate | Baseline ns/query | Candidate ns/query | Speedup | Process range | Material in every process |
|---|---:|---|---|---|---|---|---:|---:|---:|---|---|
| exact | 8192 | legacy | fixed16 | hit |  | hash | 337.9 | 50.1 | 6.79x | 6.75–6.80x | Yes |
| exact | 8192 | legacy | fixed16 | hot90 |  | hash | 279.1 | 36.1 | 7.65x | 7.60–7.75x | Yes |
| exact | 8192 | legacy | fixed16 | miss |  | hash | 327.9 | 26.5 | 12.37x | 12.24–12.54x | Yes |
| exact | 8192 | legacy | fixed16 | mixed50 |  | hash | 338.3 | 46.4 | 7.23x | 7.21–7.30x | Yes |
| exact | 8192 | legacy | fixed16 | scan100 |  | hash_ordered_fallback | 5151.8 | 5156.9 | 1.00x | 1.00–1.00x | No |
| exact | 8192 | legacy | hostname | hit |  | hash | 340.5 | 62.1 | 5.48x | 5.40–5.51x | Yes |
| exact | 8192 | legacy | hostname | hot90 |  | hash | 266.9 | 45.7 | 5.85x | 5.71–5.94x | Yes |
| exact | 8192 | legacy | hostname | miss |  | hash | 334.3 | 35.9 | 9.23x | 9.21–9.30x | Yes |
| exact | 8192 | legacy | hostname | mixed50 |  | hash | 341.3 | 56.8 | 6.09x | 5.93–6.12x | Yes |
| exact | 8192 | legacy | hostname | scan100 |  | hash_ordered_fallback | 5514.6 | 5509.1 | 1.00x | 1.00–1.00x | No |
| exact | 8192 | resident | fixed16 | hit |  | hash | 244.3 | 48.9 | 5.00x | 4.92–5.10x | Yes |
| exact | 8192 | resident | fixed16 | hot90 |  | hash | 206.0 | 36.9 | 5.63x | 5.38–5.69x | Yes |
| exact | 8192 | resident | fixed16 | miss |  | hash | 251.8 | 26.3 | 9.51x | 9.24–9.64x | Yes |
| exact | 8192 | resident | fixed16 | mixed50 |  | hash | 255.7 | 46.6 | 5.27x | 5.26–5.66x | Yes |
| exact | 8192 | resident | fixed16 | scan100 |  | hash_ordered_fallback | 3822.7 | 3842.0 | 1.00x | 0.99–1.00x | No |
| exact | 8192 | resident | hostname | hit |  | hash | 383.8 | 61.3 | 6.24x | 5.23–6.26x | Yes |
| exact | 8192 | resident | hostname | hot90 |  | hash | 307.9 | 45.4 | 6.78x | 6.50–6.78x | Yes |
| exact | 8192 | resident | hostname | miss |  | hash | 378.2 | 36.5 | 10.36x | 10.32–10.57x | Yes |
| exact | 8192 | resident | hostname | mixed50 |  | hash | 385.8 | 56.0 | 6.89x | 6.70–6.96x | Yes |
| exact | 8192 | resident | hostname | scan100 |  | hash_ordered_fallback | 6035.4 | 6076.4 | 0.99x | 0.97–1.00x | No |
| exact | 100000 | legacy | fixed16 | hit |  | hash | 451.7 | 125.1 | 3.61x | 2.78–3.95x | Yes |
| exact | 100000 | legacy | fixed16 | hot90 |  | hash | 374.7 | 52.8 | 7.10x | 6.14–7.41x | Yes |
| exact | 100000 | legacy | fixed16 | miss |  | hash | 443.5 | 36.4 | 12.18x | 9.82–12.27x | Yes |
| exact | 100000 | legacy | fixed16 | mixed50 |  | hash | 454.6 | 90.0 | 5.05x | 3.49–5.17x | Yes |
| exact | 100000 | legacy | fixed16 | scan100 |  | hash_ordered_fallback | 5186.4 | 5182.6 | 1.00x | 1.00–1.00x | No |
| exact | 100000 | legacy | hostname | hit |  | hash | 489.4 | 202.7 | 2.37x | 2.30–2.45x | Yes |
| exact | 100000 | legacy | hostname | hot90 |  | hash | 355.4 | 68.2 | 5.25x | 5.12–5.50x | Yes |
| exact | 100000 | legacy | hostname | miss |  | hash | 474.3 | 55.8 | 8.59x | 8.50–9.37x | Yes |
| exact | 100000 | legacy | hostname | mixed50 |  | hash | 488.9 | 140.7 | 3.48x | 3.28–3.59x | Yes |
| exact | 100000 | legacy | hostname | scan100 |  | hash_ordered_fallback | 5465.0 | 5461.3 | 1.00x | 0.99–1.00x | No |
| exact | 100000 | resident | fixed16 | hit |  | hash | 377.6 | 109.7 | 3.43x | 2.52–3.44x | Yes |
| exact | 100000 | resident | fixed16 | hot90 |  | hash | 306.7 | 49.0 | 6.15x | 5.76–6.27x | Yes |
| exact | 100000 | resident | fixed16 | miss |  | hash | 379.4 | 36.3 | 10.46x | 9.65–11.28x | Yes |
| exact | 100000 | resident | fixed16 | mixed50 |  | hash | 383.1 | 87.4 | 4.36x | 4.23–4.72x | Yes |
| exact | 100000 | resident | fixed16 | scan100 |  | hash_ordered_fallback | 4004.9 | 4004.5 | 1.00x | 1.00–1.00x | No |
| exact | 100000 | resident | hostname | hit |  | hash | 583.4 | 209.2 | 2.79x | 2.75–2.80x | Yes |
| exact | 100000 | resident | hostname | hot90 |  | hash | 427.4 | 68.9 | 6.22x | 6.12–6.24x | Yes |
| exact | 100000 | resident | hostname | miss |  | hash | 563.3 | 50.8 | 11.14x | 11.01–11.33x | Yes |
| exact | 100000 | resident | hostname | mixed50 |  | hash | 589.7 | 148.4 | 3.97x | 3.93–4.06x | Yes |
| exact | 100000 | resident | hostname | scan100 |  | hash_ordered_fallback | 6585.4 | 6538.5 | 1.00x | 0.99–1.01x | No |
| exact | 1000000 | legacy | fixed16 | hit |  | hash | 980.9 | 338.1 | 2.88x | 2.87–2.90x | Yes |
| exact | 1000000 | legacy | fixed16 | hot90 |  | hash | 542.1 | 120.5 | 4.50x | 4.37–4.68x | Yes |
| exact | 1000000 | legacy | fixed16 | miss |  | hash | 983.5 | 76.3 | 12.93x | 12.76–12.94x | Yes |
| exact | 1000000 | legacy | fixed16 | mixed50 |  | hash | 992.6 | 219.4 | 4.52x | 4.50–4.53x | Yes |
| exact | 1000000 | legacy | fixed16 | scan100 |  | hash_ordered_fallback | 5936.7 | 5929.8 | 1.00x | 1.00–1.07x | No |
| exact | 1000000 | legacy | hostname | hit |  | hash | 1067.4 | 362.9 | 2.96x | 2.92–2.97x | Yes |
| exact | 1000000 | legacy | hostname | hot90 |  | hash | 519.7 | 145.8 | 3.54x | 3.22–3.56x | Yes |
| exact | 1000000 | legacy | hostname | miss |  | hash | 1055.9 | 86.4 | 12.23x | 12.08–12.40x | Yes |
| exact | 1000000 | legacy | hostname | mixed50 |  | hash | 1069.1 | 234.7 | 4.55x | 4.55–4.85x | Yes |
| exact | 1000000 | legacy | hostname | scan100 |  | hash_ordered_fallback | 7795.0 | 7794.5 | 1.00x | 1.00–1.00x | No |
| exact | 1000000 | resident | fixed16 | hit |  | hash | 862.5 | 340.1 | 2.53x | 2.53–2.57x | Yes |
| exact | 1000000 | resident | fixed16 | hot90 |  | hash | 462.9 | 135.1 | 3.41x | 3.40–3.65x | Yes |
| exact | 1000000 | resident | fixed16 | miss |  | hash | 886.9 | 75.1 | 11.72x | 11.68–12.36x | Yes |
| exact | 1000000 | resident | fixed16 | mixed50 |  | hash | 884.9 | 220.2 | 4.04x | 4.00–4.18x | Yes |
| exact | 1000000 | resident | fixed16 | scan100 |  | hash_ordered_fallback | 4717.8 | 4787.1 | 1.00x | 0.99–1.00x | No |
| exact | 1000000 | resident | hostname | hit |  | hash | 1277.1 | 364.0 | 3.50x | 3.50–3.55x | Yes |
| exact | 1000000 | resident | hostname | hot90 |  | hash | 611.3 | 150.0 | 4.07x | 3.78–4.10x | Yes |
| exact | 1000000 | resident | hostname | miss |  | hash | 1268.6 | 85.4 | 14.86x | 14.68–15.29x | Yes |
| exact | 1000000 | resident | hostname | mixed50 |  | hash | 1278.5 | 234.4 | 5.46x | 5.44–5.47x | Yes |
| exact | 1000000 | resident | hostname | scan100 |  | hash_ordered_fallback | 9210.7 | 9161.3 | 1.00x | 0.99–1.01x | No |
| ordered | 8192 | legacy_default_features | fixed16 | hit |  | bbtree_0_2_0_unsync | 270.0 | 84.0 | 3.29x | 3.06–3.31x | Yes |
| ordered | 8192 | legacy_default_features | fixed16 | hit |  | std_btreemap | 270.0 | 190.6 | 1.41x | 1.39–1.42x | Yes |
| ordered | 8192 | legacy_default_features | fixed16 | miss |  | bbtree_0_2_0_unsync | 270.1 | 70.6 | 3.84x | 3.28–4.08x | Yes |
| ordered | 8192 | legacy_default_features | fixed16 | miss |  | std_btreemap | 270.1 | 176.1 | 1.52x | 1.40–1.53x | Yes |
| ordered | 8192 | legacy_default_features | fixed16 | mixed_50_50 |  | bbtree_0_2_0_unsync | 279.3 | 90.9 | 3.12x | 2.98–4.54x | Yes |
| ordered | 8192 | legacy_default_features | fixed16 | mixed_50_50 |  | std_btreemap | 279.3 | 192.7 | 1.46x | 1.45–1.53x | Yes |
| ordered | 8192 | legacy_default_features | fixed16 | range64 |  | bbtree_0_2_0_unsync | 2312.6 | 440.5 | 5.26x | 4.40–5.41x | Yes |
| ordered | 8192 | legacy_default_features | fixed16 | range64 |  | std_btreemap | 2312.6 | 456.5 | 5.07x | 4.83–5.19x | Yes |
| ordered | 8192 | legacy_default_features | hostname | hit |  | bbtree_0_2_0_unsync | 283.2 | 141.8 | 2.01x | 1.99–2.02x | Yes |
| ordered | 8192 | legacy_default_features | hostname | hit |  | std_btreemap | 283.2 | 175.6 | 1.61x | 1.59–1.64x | Yes |
| ordered | 8192 | legacy_default_features | hostname | miss |  | bbtree_0_2_0_unsync | 284.0 | 130.7 | 2.17x | 2.17–2.19x | Yes |
| ordered | 8192 | legacy_default_features | hostname | miss |  | std_btreemap | 284.0 | 170.0 | 1.67x | 1.66–1.69x | Yes |
| ordered | 8192 | legacy_default_features | hostname | mixed_50_50 |  | bbtree_0_2_0_unsync | 295.8 | 147.6 | 1.99x | 1.99–2.02x | Yes |
| ordered | 8192 | legacy_default_features | hostname | mixed_50_50 |  | std_btreemap | 295.8 | 179.1 | 1.65x | 1.64–1.66x | Yes |
| ordered | 8192 | legacy_default_features | hostname | range64 |  | bbtree_0_2_0_unsync | 2419.7 | 510.5 | 4.77x | 4.71–4.77x | Yes |
| ordered | 8192 | legacy_default_features | hostname | range64 |  | std_btreemap | 2419.7 | 466.9 | 5.21x | 5.18–5.24x | Yes |
| ordered | 100000 | legacy_default_features | fixed16 | hit |  | bbtree_0_2_0_unsync | 394.6 | 208.3 | 1.89x | 1.88–1.95x | Yes |
| ordered | 100000 | legacy_default_features | fixed16 | hit |  | std_btreemap | 394.6 | 348.2 | 1.15x | 1.10–1.16x | Yes |
| ordered | 100000 | legacy_default_features | fixed16 | miss |  | bbtree_0_2_0_unsync | 402.4 | 113.2 | 3.56x | 3.54–4.74x | Yes |
| ordered | 100000 | legacy_default_features | fixed16 | miss |  | std_btreemap | 402.4 | 280.0 | 1.46x | 1.44–1.50x | Yes |
| ordered | 100000 | legacy_default_features | fixed16 | mixed_50_50 |  | bbtree_0_2_0_unsync | 405.7 | 193.8 | 2.08x | 2.07–2.13x | Yes |
| ordered | 100000 | legacy_default_features | fixed16 | mixed_50_50 |  | std_btreemap | 405.7 | 319.6 | 1.27x | 1.20–1.29x | Yes |
| ordered | 100000 | legacy_default_features | fixed16 | range64 |  | bbtree_0_2_0_unsync | 2464.6 | 757.8 | 3.23x | 3.21–3.31x | Yes |
| ordered | 100000 | legacy_default_features | fixed16 | range64 |  | std_btreemap | 2464.6 | 680.9 | 3.62x | 3.60–3.71x | Yes |
| ordered | 100000 | legacy_default_features | hostname | hit |  | bbtree_0_2_0_unsync | 387.8 | 355.4 | 1.09x | 1.07–1.14x | No |
| ordered | 100000 | legacy_default_features | hostname | hit |  | std_btreemap | 387.8 | 447.8 | 0.87x | 0.85–0.87x | No |
| ordered | 100000 | legacy_default_features | hostname | miss |  | bbtree_0_2_0_unsync | 391.4 | 228.2 | 1.68x | 1.67–1.72x | Yes |
| ordered | 100000 | legacy_default_features | hostname | miss |  | std_btreemap | 391.4 | 288.3 | 1.36x | 1.31–1.36x | Yes |
| ordered | 100000 | legacy_default_features | hostname | mixed_50_50 |  | bbtree_0_2_0_unsync | 399.0 | 332.3 | 1.20x | 1.19–1.23x | Yes |
| ordered | 100000 | legacy_default_features | hostname | mixed_50_50 |  | std_btreemap | 399.0 | 384.8 | 1.04x | 1.02–1.07x | No |
| ordered | 100000 | legacy_default_features | hostname | range64 |  | bbtree_0_2_0_unsync | 2578.4 | 1066.5 | 2.42x | 2.39–2.48x | Yes |
| ordered | 100000 | legacy_default_features | hostname | range64 |  | std_btreemap | 2578.4 | 910.0 | 2.86x | 2.80–2.88x | Yes |
| ordered | 1000000 | legacy_default_features | fixed16 | hit |  | bbtree_0_2_0_unsync | 774.4 | 533.4 | 1.46x | 1.45–1.54x | Yes |
| ordered | 1000000 | legacy_default_features | fixed16 | hit |  | std_btreemap | 774.4 | 1021.9 | 0.76x | 0.76–0.80x | No |
| ordered | 1000000 | legacy_default_features | fixed16 | miss |  | bbtree_0_2_0_unsync | 766.4 | 381.6 | 2.03x | 1.99–2.10x | Yes |
| ordered | 1000000 | legacy_default_features | fixed16 | miss |  | std_btreemap | 766.4 | 756.3 | 1.01x | 1.01–1.02x | No |
| ordered | 1000000 | legacy_default_features | fixed16 | mixed_50_50 |  | bbtree_0_2_0_unsync | 779.0 | 514.3 | 1.53x | 1.51–1.53x | Yes |
| ordered | 1000000 | legacy_default_features | fixed16 | mixed_50_50 |  | std_btreemap | 779.0 | 902.6 | 0.86x | 0.86–0.86x | No |
| ordered | 1000000 | legacy_default_features | fixed16 | range64 |  | bbtree_0_2_0_unsync | 3084.8 | 1969.7 | 1.57x | 1.49–2.37x | Yes |
| ordered | 1000000 | legacy_default_features | fixed16 | range64 |  | std_btreemap | 3084.8 | 2124.7 | 1.45x | 1.40–1.79x | Yes |
| ordered | 1000000 | legacy_default_features | hostname | hit |  | bbtree_0_2_0_unsync | 862.2 | 646.5 | 1.34x | 1.32–1.35x | Yes |
| ordered | 1000000 | legacy_default_features | hostname | hit |  | std_btreemap | 862.2 | 1107.0 | 0.78x | 0.78–0.78x | No |
| ordered | 1000000 | legacy_default_features | hostname | miss |  | bbtree_0_2_0_unsync | 871.8 | 473.9 | 1.83x | 1.82–1.86x | Yes |
| ordered | 1000000 | legacy_default_features | hostname | miss |  | std_btreemap | 871.8 | 832.3 | 1.04x | 1.04–1.05x | No |
| ordered | 1000000 | legacy_default_features | hostname | mixed_50_50 |  | bbtree_0_2_0_unsync | 889.2 | 596.7 | 1.48x | 1.45–1.50x | Yes |
| ordered | 1000000 | legacy_default_features | hostname | mixed_50_50 |  | std_btreemap | 889.2 | 989.2 | 0.88x | 0.88–0.90x | No |
| ordered | 1000000 | legacy_default_features | hostname | range64 |  | bbtree_0_2_0_unsync | 3642.5 | 2194.8 | 1.65x | 1.65–1.74x | Yes |
| ordered | 1000000 | legacy_default_features | hostname | range64 |  | std_btreemap | 3642.5 | 2383.4 | 1.53x | 1.53–1.58x | Yes |
| prefix | 8192 | ipv4 |  | all_covering | hit | multibit4_checked | 1507.9 | 6506.3 | 0.23x | 0.23–0.23x | No |
| prefix | 8192 | ipv4 |  | all_covering | mixed50 | multibit4_checked | 1188.6 | 6183.3 | 0.19x | 0.19–0.19x | No |
| prefix | 8192 | ipv4 |  | all_covering | nested_chain | multibit4_checked | 1734.0 | 6903.7 | 0.25x | 0.25–0.27x | No |
| prefix | 8192 | ipv4 |  | exact | hit | multibit4_checked | 263.7 | 292.3 | 0.90x | 0.90–0.91x | No |
| prefix | 8192 | ipv4 |  | exact | mixed50 | multibit4_checked | 150.8 | 160.6 | 0.94x | 0.92–0.95x | No |
| prefix | 8192 | ipv4 |  | exact | nested_chain | multibit4_checked | 279.1 | 481.6 | 0.58x | 0.58–0.62x | No |
| prefix | 8192 | ipv4 |  | lpm | hit | multibit4_checked | 271.2 | 313.1 | 0.87x | 0.85–0.88x | No |
| prefix | 8192 | ipv4 |  | lpm | mixed50 | multibit4_checked | 154.2 | 171.6 | 0.91x | 0.88–0.91x | No |
| prefix | 8192 | ipv4 |  | lpm | nested_chain | multibit4_checked | 285.2 | 489.5 | 0.59x | 0.58–0.62x | No |
| prefix | 8192 | ipv6 |  | all_covering | hit | multibit4_checked | 1496.5 | 6488.9 | 0.23x | 0.23–0.23x | No |
| prefix | 8192 | ipv6 |  | all_covering | mixed50 | multibit4_checked | 1189.7 | 6197.5 | 0.19x | 0.19–0.19x | No |
| prefix | 8192 | ipv6 |  | all_covering | nested_chain | multibit4_checked | 2514.7 | 8221.0 | 0.31x | 0.30–0.31x | No |
| prefix | 8192 | ipv6 |  | exact | hit | multibit4_checked | 353.5 | 344.5 | 1.02x | 1.01–1.03x | No |
| prefix | 8192 | ipv6 |  | exact | mixed50 | multibit4_checked | 195.2 | 188.5 | 1.02x | 1.02–1.04x | No |
| prefix | 8192 | ipv6 |  | exact | nested_chain | multibit4_checked | 623.5 | 1222.4 | 0.51x | 0.51–0.51x | No |
| prefix | 8192 | ipv6 |  | lpm | hit | multibit4_checked | 351.8 | 366.0 | 0.95x | 0.95–0.97x | No |
| prefix | 8192 | ipv6 |  | lpm | mixed50 | multibit4_checked | 194.7 | 199.3 | 0.97x | 0.97–0.98x | No |
| prefix | 8192 | ipv6 |  | lpm | nested_chain | multibit4_checked | 638.6 | 1249.2 | 0.51x | 0.51–0.51x | No |
| prefix | 100000 | ipv4 |  | all_covering | hit | multibit4_checked | 1856.8 | 6762.8 | 0.27x | 0.27–0.27x | No |
| prefix | 100000 | ipv4 |  | all_covering | mixed50 | multibit4_checked | 1363.7 | 6405.6 | 0.21x | 0.20–0.22x | No |
| prefix | 100000 | ipv4 |  | all_covering | nested_chain | multibit4_checked | 1929.4 | 6852.6 | 0.28x | 0.26–0.28x | No |
| prefix | 100000 | ipv4 |  | exact | hit | multibit4_checked | 478.8 | 530.8 | 0.91x | 0.89–0.94x | No |
| prefix | 100000 | ipv4 |  | exact | mixed50 | multibit4_checked | 273.7 | 289.4 | 0.96x | 0.95–0.96x | No |
| prefix | 100000 | ipv4 |  | exact | nested_chain | multibit4_checked | 343.0 | 504.1 | 0.68x | 0.64–0.69x | No |
| prefix | 100000 | ipv4 |  | lpm | hit | multibit4_checked | 488.9 | 561.5 | 0.90x | 0.87–0.91x | No |
| prefix | 100000 | ipv4 |  | lpm | mixed50 | multibit4_checked | 279.9 | 299.0 | 0.94x | 0.93–0.94x | No |
| prefix | 100000 | ipv4 |  | lpm | nested_chain | multibit4_checked | 334.6 | 510.9 | 0.66x | 0.63–0.67x | No |
| prefix | 100000 | ipv6 |  | all_covering | hit | multibit4_checked | 1830.4 | 6872.5 | 0.27x | 0.26–0.28x | No |
| prefix | 100000 | ipv6 |  | all_covering | mixed50 | multibit4_checked | 1364.0 | 6377.6 | 0.21x | 0.21–0.22x | No |
| prefix | 100000 | ipv6 |  | all_covering | nested_chain | multibit4_checked | 2645.7 | 8206.4 | 0.32x | 0.31–0.33x | No |
| prefix | 100000 | ipv6 |  | exact | hit | multibit4_checked | 621.7 | 581.0 | 1.06x | 1.02–1.07x | No |
| prefix | 100000 | ipv6 |  | exact | mixed50 | multibit4_checked | 337.0 | 320.8 | 1.05x | 1.05–1.05x | No |
| prefix | 100000 | ipv6 |  | exact | nested_chain | multibit4_checked | 685.8 | 1242.1 | 0.55x | 0.55–0.55x | No |
| prefix | 100000 | ipv6 |  | lpm | hit | multibit4_checked | 605.7 | 627.6 | 1.00x | 0.96–1.04x | No |
| prefix | 100000 | ipv6 |  | lpm | mixed50 | multibit4_checked | 343.5 | 337.7 | 1.01x | 1.00–1.02x | No |
| prefix | 100000 | ipv6 |  | lpm | nested_chain | multibit4_checked | 700.7 | 1268.9 | 0.55x | 0.55–0.65x | No |
| prefix | 1000000 | ipv4 |  | all_covering | hit | multibit4_checked | 2396.7 | 7385.3 | 0.32x | 0.32–0.33x | No |
| prefix | 1000000 | ipv4 |  | all_covering | mixed50 | multibit4_checked | 1635.5 | 6708.7 | 0.24x | 0.24–0.25x | No |
| prefix | 1000000 | ipv4 |  | all_covering | nested_chain | multibit4_checked | 2028.4 | 7108.1 | 0.28x | 0.28–0.30x | No |
| prefix | 1000000 | ipv4 |  | exact | hit | multibit4_checked | 882.2 | 822.2 | 1.07x | 1.07–1.13x | No |
| prefix | 1000000 | ipv4 |  | exact | mixed50 | multibit4_checked | 466.5 | 433.6 | 1.07x | 1.07–1.08x | No |
| prefix | 1000000 | ipv4 |  | exact | nested_chain | multibit4_checked | 381.3 | 511.2 | 0.74x | 0.74–0.79x | No |
| prefix | 1000000 | ipv4 |  | lpm | hit | multibit4_checked | 912.4 | 851.6 | 1.07x | 1.06–1.09x | No |
| prefix | 1000000 | ipv4 |  | lpm | mixed50 | multibit4_checked | 483.5 | 446.7 | 1.08x | 1.08–1.09x | No |
| prefix | 1000000 | ipv4 |  | lpm | nested_chain | multibit4_checked | 380.3 | 519.0 | 0.73x | 0.73–0.76x | No |
| prefix | 1000000 | ipv6 |  | all_covering | hit | multibit4_checked | 2314.7 | 7262.9 | 0.32x | 0.32–0.33x | No |
| prefix | 1000000 | ipv6 |  | all_covering | mixed50 | multibit4_checked | 1609.2 | 6654.5 | 0.24x | 0.24–0.25x | No |
| prefix | 1000000 | ipv6 |  | all_covering | nested_chain | multibit4_checked | 2807.6 | 8486.6 | 0.33x | 0.33–0.34x | No |
| prefix | 1000000 | ipv6 |  | exact | hit | multibit4_checked | 1041.1 | 900.2 | 1.15x | 1.15–1.16x | Yes |
| prefix | 1000000 | ipv6 |  | exact | mixed50 | multibit4_checked | 547.1 | 472.3 | 1.16x | 1.15–1.16x | Yes |
| prefix | 1000000 | ipv6 |  | exact | nested_chain | multibit4_checked | 771.3 | 1316.2 | 0.59x | 0.57–0.59x | No |
| prefix | 1000000 | ipv6 |  | lpm | hit | multibit4_checked | 1037.2 | 926.4 | 1.13x | 1.11–1.13x | Yes |
| prefix | 1000000 | ipv6 |  | lpm | mixed50 | multibit4_checked | 545.9 | 484.5 | 1.13x | 1.13–1.15x | Yes |
| prefix | 1000000 | ipv6 |  | lpm | nested_chain | multibit4_checked | 781.7 | 1341.4 | 0.58x | 0.57–0.58x | No |

Range-query times include consuming up to 64 (ordered) or 100 (exact fallback) records. Prefix all-covering consumes every matching ancestor; these are not single-winner timings.

## Construction and memory

Memory is requested or estimated storage, not RSS. Hash estimates exclude allocator overhead, source mappings and retained old snapshots. Prefix arena high-water includes abandoned private allocations; reachable size excludes them. Binary construction uses its sorted bulk loader; multibit uses its available incremental builder.

| Suite | Records | Variant | Algorithm/keys | Build ms | Arena/additional MiB | Reachable MiB / hash rebuild ms |
|---|---:|---|---|---:|---:|---:|
| exact | 8192 | legacy | fixed16 | 1.22 | 0.64 | 1.28 |
| exact | 8192 | legacy | hostname | 0.95 | 0.85 | 1.53 |
| exact | 8192 | resident | fixed16 | 1.06 | 0.64 | 1.17 |
| exact | 8192 | resident | hostname | 1.38 | 0.85 | 1.64 |
| exact | 100000 | legacy | fixed16 | 11.11 | 5.65 | 16.48 |
| exact | 100000 | legacy | hostname | 14.24 | 8.16 | 21.08 |
| exact | 100000 | resident | fixed16 | 13.10 | 5.65 | 14.46 |
| exact | 100000 | resident | hostname | 19.76 | 8.16 | 20.91 |
| exact | 1000000 | legacy | fixed16 | 367.42 | 81.26 | 296.41 |
| exact | 1000000 | legacy | hostname | 422.45 | 106.35 | 341.75 |
| exact | 1000000 | resident | fixed16 | 359.44 | 81.26 | 277.35 |
| exact | 1000000 | resident | hostname | 441.44 | 106.35 | 369.95 |
| ordered | 8192 | legacy | fixed16 | 2.28 | 1.17 | N/A |
| ordered | 8192 | legacy | hostname | 2.58 | 1.59 | N/A |
| ordered | 100000 | legacy | fixed16 | 28.92 | 14.23 | N/A |
| ordered | 100000 | legacy | hostname | 33.28 | 19.39 | N/A |
| ordered | 1000000 | legacy | fixed16 | 389.38 | 142.30 | N/A |
| ordered | 1000000 | legacy | hostname | 481.52 | 193.91 | N/A |
| prefix | 8192 | ipv4 | binary_scalar | 2.95 | 1.55 | 1.55 |
| prefix | 8192 | ipv4 | multibit4_checked | 9.13 | 1.97 | 1.09 |
| prefix | 8192 | ipv6 | binary_scalar | 2.00 | 1.75 | 1.75 |
| prefix | 8192 | ipv6 | multibit4_checked | 8.34 | 1.93 | 1.21 |
| prefix | 100000 | ipv4 | binary_scalar | 19.75 | 18.66 | 18.66 |
| prefix | 100000 | ipv4 | multibit4_checked | 125.24 | 24.27 | 13.30 |
| prefix | 100000 | ipv6 | binary_scalar | 35.98 | 21.36 | 21.36 |
| prefix | 100000 | ipv6 | multibit4_checked | 129.23 | 23.79 | 14.94 |
| prefix | 1000000 | ipv4 | binary_scalar | 196.25 | 185.70 | 185.70 |
| prefix | 1000000 | ipv4 | multibit4_checked | 1332.21 | 249.25 | 133.02 |
| prefix | 1000000 | ipv6 | binary_scalar | 226.83 | 213.62 | 213.62 |
| prefix | 1000000 | ipv6 | multibit4_checked | 1387.28 | 242.58 | 150.95 |

## Hash construction break-even

Estimated lookups required to amortize one complete hash build on the mixed workload. This excludes extra-memory cost and generation retention, and assumes unchanged timing after integration.

| Records | Backend | Keys | Build ms | Saved ns/query | Queries to break even |
|---:|---|---|---:|---:|---:|
| 8192 | legacy | fixed16 | 1.22 | 291.9 | 4,173 |
| 8192 | legacy | hostname | 0.95 | 284.5 | 3,330 |
| 8192 | resident | fixed16 | 1.06 | 209.1 | 5,067 |
| 8192 | resident | hostname | 1.38 | 329.9 | 4,172 |
| 100000 | legacy | fixed16 | 11.11 | 364.6 | 30,471 |
| 100000 | legacy | hostname | 14.24 | 348.3 | 40,892 |
| 100000 | resident | fixed16 | 13.10 | 295.7 | 44,292 |
| 100000 | resident | hostname | 19.76 | 441.2 | 44,777 |
| 1000000 | legacy | fixed16 | 367.42 | 773.3 | 475,152 |
| 1000000 | legacy | hostname | 422.45 | 834.4 | 506,321 |
| 1000000 | resident | fixed16 | 359.44 | 664.6 | 540,809 |
| 1000000 | resident | hostname | 441.44 | 1044.1 | 422,790 |

## Provenance

Per-run source revisions and executable hashes are retained in results.json. A setup correction capped the million-row Resident fixture's WAL request at its existing supported 256 MiB limit; smaller-fixture settings and all lookup algorithms stayed unchanged. Completed samples were retained under their original hashes, while the remaining jobs used rebuilt harnesses. The frozen engine sources are identical across both harness revisions. The rejected setup attempt and earlier development pilots remain recorded separately and contribute no successful timing rows.

## Reproduce

```sh
python3 benchmark/lookup-strategies/run.py --suite all \
  --sizes 8192,100000,1000000 --queries 65536 --samples 7 \
  --prefix-iterations 1 \
  --processes 3 --cpu 12 --seed 73 \
  --output benchmark/work/NEW_LOOKUP_CAMPAIGN
```

Use a fresh output path and an available CPU. The runner freezes the dirty source, builds locked dependencies, pins every process, rejects mismatched/missing results and retains all raw samples. Existing benchmark reports are untouched. Source and executable hashes are retained in results.json.
