# Production lookup strategy: complete direct comparison

90 fresh processes; 3 rounds × 3 sizes × two key types × five arms. 7 samples per read/write case; 65,536 point queries and up to1,024 scan queries per workload. Seeds 73 through 75.

Machine: AMD EPYC 7702P 64-Core Processor; x86_64; Linux 6.8.0-147-generic; CPU 12. Shared host; no exclusive CPU reservation. Source manifest SHA-256 `a4aa92e6f057c9f4430ed8689a1e2d5df957ab424a26a63796ae7e2b1940a008`; executable SHA-256 `9cdddb8f4aa9f4d18aa0f038aa0c8dd96ea093c3fa84d8fca29def838a6bd540`. Pinned native reference: LMDB 1.0.2: (Aug 6, 2026).

All five arms execute sequentially in fresh processes with reversing/rotating group order. Native uses the original prepared KvRead and constructs no hash. Hash uses the completed production API with borrowed keys/values and exact owned slot capacity accounting. LMDB uses only NOTLS (0x200000), normal locking and synchronized durable commits. A held old reader exists in native, hash and LMDB pinned-write phases; hash additionally retains its side index. All initial, old, new and reopened values and forward/reverse ranges are checked against the same deterministic byte oracle. Every corresponding timed checksum and operation count is checked across all five arms.

Values are medians of within-process samples, then medians across fresh processes. Each ratio is baseline cost / hash cost; above1 is a lower hash cost. Brackets show the minimum and maximum paired process ratio, not confidence intervals. A >5% increase in any process is retained as an adverse observation, including construction, memory, retention and lifecycle costs; it is not automatically a statistically established regression. 'all' means every process exceeded5%. Missing or undefined zero-cost ratios are N/A.

## Read throughput costs

Nanoseconds per point query or one cursor seek plus up to100/64 complete key/value results. These are batch throughput averages, not individual request p99 latency.

| Keys | Key type | Phase / operation | Legacy native | Legacy hash | Resident native | Resident hash | LMDB | Legacy hash/native | Resident hash/native | Legacy hash/LMDB | Resident hash/LMDB | >5% hash cost increase vs native |
|---:|---|---|---:|---:|---:|---:|---:|---|---|---|---|---|
| 8192 | fixed16 | hit | 373.040 | 74.626 | 249.325 | 73.971 | 253.965 | 5.01x [4.93,5.07] | 3.37x [2.97,3.45] | 3.41x [3.36,3.45] | 3.44x [3.03,3.49] | None observed |
| 8192 | fixed16 | hot90 | 320.866 | 55.966 | 205.864 | 57.332 | 193.467 | 5.73x [5.73,5.73] | 3.59x [3.58,3.71] | 3.46x [3.44,3.47] | 3.37x [3.37,3.5] | None observed |
| 8192 | fixed16 | miss | 351.243 | 46.032 | 242.328 | 45.815 | 249.755 | 7.64x [7.62,7.65] | 5.29x [5.15,5.47] | 5.42x [5.39,5.47] | 5.42x [5.35,5.59] | None observed |
| 8192 | fixed16 | mixed50 | 367.881 | 60.049 | 250.041 | 60.116 | 253.199 | 6.12x [6.1,6.15] | 4.15x [4.15,4.26] | 4.22x [4.2,4.25] | 4.24x [4.2,4.29] | None observed |
| 8192 | fixed16 | range_forward64 | 4,087.559 | 4,083.243 | 3,974.335 | 4,108.085 | 3,235.547 | 1.01x [1,1.02] | 0.967x [0.952,0.981] | 0.797x [0.773,0.808] | 0.79x [0.774,0.799] | resident (some) |
| 8192 | fixed16 | range_reverse64 | 4,109.964 | 4,117.537 | 3,989.089 | 4,133.466 | 2,965.640 | 0.997x [0.977,0.999] | 0.965x [0.947,0.992] | 0.722x [0.717,0.723] | 0.714x [0.709,0.727] | resident (some) |
| 8192 | fixed16 | scan_forward100 | 5,451.079 | 5,403.243 | 5,397.793 | 5,641.343 | 4,262.040 | 1.01x [0.989,1.04] | 0.962x [0.943,0.964] | 0.787x [0.764,0.809] | 0.745x [0.738,0.773] | resident (some) |
| 8192 | fixed16 | scan_reverse100 | 5,548.354 | 5,419.838 | 5,466.088 | 5,667.839 | 4,036.641 | 1.02x [0.997,1.04] | 0.966x [0.964,0.976] | 0.744x [0.741,0.745] | 0.713x [0.706,0.714] | None observed |
| 8192 | hostname | hit | 373.141 | 86.301 | 389.124 | 85.180 | 274.769 | 4.37x [4.28,4.39] | 4.57x [4.55,4.65] | 3.18x [3.16,3.24] | 3.23x [3.2,3.3] | None observed |
| 8192 | hostname | hot90 | 305.247 | 64.410 | 309.417 | 63.777 | 195.518 | 4.74x [4.67,4.76] | 4.85x [4.8,4.88] | 3.03x [3.02,3.05] | 3.07x [3.04,3.09] | None observed |
| 8192 | hostname | miss | 352.035 | 54.188 | 376.010 | 54.811 | 268.708 | 6.46x [6.43,6.5] | 6.94x [6.78,6.97] | 4.96x [4.87,5] | 4.95x [4.84,5] | None observed |
| 8192 | hostname | mixed50 | 368.081 | 70.297 | 388.657 | 68.962 | 276.465 | 5.29x [5.19,5.31] | 5.64x [5.52,5.93] | 3.92x [3.92,3.99] | 4.01x [3.96,4.17] | None observed |
| 8192 | hostname | range_forward64 | 4,967.454 | 5,112.251 | 5,860.754 | 5,922.336 | 3,828.735 | 0.972x [0.945,1.02] | 0.993x [0.986,0.995] | 0.762x [0.727,0.789] | 0.645x [0.645,0.685] | legacy (some) |
| 8192 | hostname | range_reverse64 | 4,953.560 | 4,968.188 | 5,860.011 | 5,920.262 | 3,565.384 | 0.989x [0.941,1.02] | 0.983x [0.978,1.03] | 0.718x [0.677,0.73] | 0.605x [0.602,0.607] | legacy (some) |
| 8192 | hostname | scan_forward100 | 6,825.059 | 6,873.940 | 8,354.667 | 8,533.424 | 5,355.585 | 0.986x [0.958,0.993] | 0.983x [0.97,0.99] | 0.78x [0.743,0.788] | 0.627x [0.622,0.635] | None observed |
| 8192 | hostname | scan_reverse100 | 7,071.190 | 7,276.522 | 8,214.165 | 8,206.317 | 5,144.284 | 0.988x [0.961,1] | 0.995x [0.994,1.04] | 0.718x [0.699,0.738] | 0.633x [0.627,0.633] | None observed |
| 100000 | fixed16 | hit | 482.421 | 115.627 | 386.878 | 110.435 | 354.356 | 4.13x [4.1,4.32] | 3.5x [2.57,3.6] | 3.07x [3.02,3.17] | 3.21x [2.37,3.26] | None observed |
| 100000 | fixed16 | hot90 | 407.421 | 67.106 | 312.189 | 66.937 | 276.606 | 6.07x [6.07,6.18] | 4.68x [4.62,4.71] | 4.13x [4.11,4.22] | 4.14x [4.07,4.2] | None observed |
| 100000 | fixed16 | miss | 456.001 | 50.017 | 398.128 | 50.475 | 352.009 | 9.21x [8.87,9.22] | 7.89x [7.65,10.1] | 7.04x [6.82,7.12] | 6.97x [6.73,7.04] | None observed |
| 100000 | fixed16 | mixed50 | 474.026 | 96.637 | 388.375 | 93.985 | 352.882 | 4.86x [3.59,5.16] | 4.13x [3.84,4.23] | 3.65x [2.66,3.84] | 3.76x [3.48,3.83] | None observed |
| 100000 | fixed16 | range_forward64 | 4,209.312 | 4,235.789 | 4,167.760 | 4,294.514 | 3,389.199 | 0.998x [0.978,1.04] | 0.975x [0.964,0.985] | 0.799x [0.784,0.812] | 0.784x [0.773,0.807] | None observed |
| 100000 | fixed16 | range_reverse64 | 4,131.969 | 4,256.415 | 4,194.559 | 4,385.008 | 3,105.544 | 0.98x [0.97,1.05] | 0.952x [0.938,0.981] | 0.73x [0.728,0.737] | 0.708x [0.701,0.725] | resident (some) |
| 100000 | fixed16 | scan_forward100 | 5,596.062 | 5,540.047 | 5,626.236 | 5,790.151 | 4,504.425 | 1.02x [0.991,1.03] | 0.973x [0.911,0.976] | 0.799x [0.794,0.829] | 0.759x [0.734,0.791] | resident (some) |
| 100000 | fixed16 | scan_reverse100 | 5,509.627 | 5,608.947 | 5,712.015 | 5,912.680 | 4,151.067 | 0.991x [0.982,1.05] | 0.956x [0.95,0.985] | 0.743x [0.732,0.746] | 0.7x [0.691,0.708] | resident (some) |
| 100000 | hostname | hit | 483.851 | 160.513 | 580.786 | 171.189 | 366.443 | 3.05x [2.93,3.06] | 3.42x [3.38,3.94] | 2.3x [2.23,2.31] | 2.16x [2.13,2.48] | None observed |
| 100000 | hostname | hot90 | 388.848 | 78.398 | 433.438 | 79.908 | 254.340 | 4.98x [4.93,6.71] | 5.42x [5.35,5.64] | 3.23x [3.2,4.41] | 3.18x [3.12,3.21] | None observed |
| 100000 | hostname | miss | 466.966 | 61.678 | 562.046 | 63.143 | 362.644 | 7.57x [7.42,7.9] | 8.9x [8.29,9.26] | 5.88x [5.82,6.17] | 5.74x [5.35,5.95] | None observed |
| 100000 | hostname | mixed50 | 480.422 | 132.377 | 574.673 | 132.245 | 369.178 | 3.66x [3.47,3.68] | 4.55x [4.3,4.74] | 2.8x [2.68,2.8] | 2.8x [2.75,2.93] | None observed |
| 100000 | hostname | range_forward64 | 5,206.081 | 5,197.148 | 6,147.833 | 6,218.289 | 4,093.830 | 1x [0.975,1] | 0.987x [0.987,0.994] | 0.785x [0.765,0.805] | 0.661x [0.655,0.667] | None observed |
| 100000 | hostname | range_reverse64 | 5,233.517 | 5,157.229 | 6,137.530 | 6,211.089 | 3,761.754 | 0.995x [0.99,1.02] | 0.991x [0.979,0.994] | 0.729x [0.713,0.734] | 0.609x [0.603,0.609] | None observed |
| 100000 | hostname | scan_forward100 | 7,136.589 | 7,353.016 | 8,775.897 | 8,888.465 | 5,686.351 | 0.971x [0.956,0.999] | 0.988x [0.986,0.998] | 0.779x [0.756,0.795] | 0.64x [0.628,0.652] | None observed |
| 100000 | hostname | scan_reverse100 | 7,175.627 | 7,324.045 | 8,461.118 | 8,516.224 | 5,428.057 | 0.98x [0.947,1.04] | 0.995x [0.979,1] | 0.741x [0.733,0.754] | 0.637x [0.631,0.653] | legacy (some) |
| 1000000 | fixed16 | hit | 1,030.382 | 303.480 | 893.200 | 316.432 | 729.736 | 3.38x [3.19,3.51] | 2.83x [2.79,2.84] | 2.4x [2.27,2.41] | 2.31x [2.3,2.34] | None observed |
| 1000000 | fixed16 | hot90 | 566.628 | 93.771 | 462.899 | 97.122 | 374.131 | 6.04x [5.96,11.4] | 4.82x [4.76,4.83] | 3.99x [3.95,5.16] | 3.85x [3.83,3.92] | None observed |
| 1000000 | fixed16 | miss | 1,004.093 | 157.195 | 885.191 | 163.113 | 738.039 | 6.46x [6.37,6.51] | 5.41x [5.35,5.5] | 4.7x [4.68,4.75] | 4.52x [4.43,4.59] | None observed |
| 1000000 | fixed16 | mixed50 | 1,021.769 | 241.576 | 894.330 | 248.279 | 738.552 | 4.21x [4.18,4.28] | 3.59x [3.57,3.62] | 3.05x [3.05,3.08] | 2.98x [2.95,3.01] | None observed |
| 1000000 | fixed16 | range_forward64 | 4,435.397 | 4,431.845 | 4,398.031 | 4,538.621 | 3,446.730 | 0.983x [0.98,1.03] | 0.969x [0.955,0.979] | 0.772x [0.765,0.786] | 0.755x [0.754,0.764] | None observed |
| 1000000 | fixed16 | range_reverse64 | 4,433.401 | 4,424.146 | 4,347.477 | 4,512.301 | 3,229.354 | 0.998x [0.994,1] | 0.963x [0.953,0.971] | 0.726x [0.726,0.731] | 0.716x [0.71,0.716] | None observed |
| 1000000 | fixed16 | scan_forward100 | 5,944.811 | 5,785.475 | 5,984.828 | 6,179.809 | 4,503.192 | 1.02x [0.995,1.04] | 0.98x [0.962,0.981] | 0.783x [0.775,0.8] | 0.729x [0.725,0.759] | None observed |
| 1000000 | fixed16 | scan_reverse100 | 5,964.790 | 5,944.058 | 6,081.966 | 6,180.552 | 4,338.260 | 1x [0.981,1.05] | 0.987x [0.976,0.997] | 0.73x [0.727,0.739] | 0.703x [0.702,0.704] | None observed |
| 1000000 | hostname | hit | 1,118.679 | 330.741 | 1,313.312 | 346.385 | 810.780 | 3.38x [2.91,3.47] | 4x [3.77,4.09] | 2.45x [2.12,2.49] | 2.34x [2.32,2.48] | None observed |
| 1000000 | hostname | hot90 | 555.709 | 108.272 | 625.582 | 120.192 | 394.448 | 5.17x [5.11,6.53] | 5.2x [5.04,6.18] | 3.64x [3.63,4.56] | 3.23x [3.2,3.85] | None observed |
| 1000000 | hostname | miss | 1,082.400 | 166.743 | 1,291.186 | 176.078 | 821.521 | 6.49x [6.47,6.63] | 7.32x [7.27,7.37] | 4.95x [4.86,4.99] | 4.69x [4.58,4.73] | None observed |
| 1000000 | hostname | mixed50 | 1,106.541 | 254.193 | 1,305.004 | 268.749 | 828.328 | 4.34x [4.28,4.4] | 4.83x [4.79,4.96] | 3.26x [3.14,3.26] | 3.04x [3.03,3.09] | None observed |
| 1000000 | hostname | range_forward64 | 5,573.821 | 5,481.919 | 6,666.603 | 6,793.299 | 4,314.807 | 1.01x [1.01,1.02] | 0.984x [0.971,1.01] | 0.785x [0.785,0.788] | 0.635x [0.627,0.638] | None observed |
| 1000000 | hostname | range_reverse64 | 5,576.845 | 5,520.742 | 6,593.211 | 6,768.213 | 4,028.089 | 1.01x [0.998,1.09] | 0.98x [0.961,0.999] | 0.73x [0.72,0.749] | 0.594x [0.592,0.613] | None observed |
| 1000000 | hostname | scan_forward100 | 8,210.183 | 8,024.224 | 10,137.979 | 10,398.866 | 6,648.071 | 1.02x [1.01,1.03] | 0.967x [0.965,0.975] | 0.829x [0.795,0.839] | 0.639x [0.614,0.644] | None observed |
| 1000000 | hostname | scan_reverse100 | 7,985.546 | 7,887.411 | 9,459.796 | 9,537.912 | 6,037.623 | 1.01x [0.968,1.02] | 0.991x [0.969,0.996] | 0.758x [0.746,0.767] | 0.632x [0.623,0.636] | None observed |

## Durable writes in every pin state

Microseconds per synchronized transaction. Batch sizes are encoded in operation names; raw files also retain ns/update and exact operation counts. Fixtures are precomputed outside timing. No hash is automatically rebuilt on commit: this table measures writes with the same old snapshot held, plus the optional existing index; the explicit full rebuild is charged below.

| Keys | Key type | Phase / operation | Legacy native | Legacy hash | Resident native | Resident hash | LMDB | Legacy hash/native | Resident hash/native | Legacy hash/LMDB | Resident hash/LMDB | >5% hash cost increase vs native |
|---:|---|---|---:|---:|---:|---:|---:|---|---|---|---|---|
| 8192 | fixed16 | pinned / durable_batch1 | 1,915.968 | 1,970.943 | 79.551 | 77.367 | 112.699 | 1.03x [0.43,1.1] | 1.03x [0.829,1.04] | 0.0573x [0.0253,0.0691] | 1.28x [1.23,1.77] | legacy (some), resident (some) |
| 8192 | fixed16 | pinned / durable_batch1000 | 4,015.213 | 3,838.515 | 1,277.357 | 1,272.825 | 1,615.392 | 1.01x [0.99,1.05] | 1x [0.83,1.06] | 0.41x [0.41,0.427] | 1.24x [1.08,1.3] | resident (some) |
| 8192 | fixed16 | pinned / durable_batch64 | 2,532.859 | 2,407.461 | 226.438 | 231.007 | 674.568 | 1.06x [1.05,1.07] | 0.974x [0.795,1.04] | 0.295x [0.28,0.312] | 2.92x [2.71,2.97] | resident (some) |
| 8192 | fixed16 | unpinned_after / durable_batch1 | 1,857.370 | 1,890.127 | 78.049 | 77.550 | 124.404 | 1.01x [0.95,1.02] | 1.01x [0.84,1.02] | 0.0688x [0.0636,0.0841] | 1.6x [1.58,1.73] | legacy (some), resident (some) |
| 8192 | fixed16 | unpinned_after / durable_batch1000 | 3,761.668 | 3,810.636 | 1,252.815 | 1,257.959 | 1,048.521 | 0.987x [0.943,1.04] | 0.992x [0.806,1.04] | 0.275x [0.267,0.282] | 0.834x [0.671,0.878] | legacy (some), resident (some) |
| 8192 | fixed16 | unpinned_after / durable_batch64 | 2,508.443 | 2,519.852 | 231.991 | 231.748 | 550.670 | 0.995x [0.984,1.01] | 1.01x [0.79,1.02] | 0.22x [0.217,0.23] | 2.39x [1.91,2.39] | resident (some) |
| 8192 | fixed16 | unpinned_before / durable_batch1 | 1,910.814 | 1,892.556 | 75.661 | 76.849 | 85.793 | 0.997x [0.911,1.06] | 0.985x [0.65,1] | 0.0479x [0.0412,0.0497] | 1.01x [0.826,1.12] | legacy (some), resident (some) |
| 8192 | fixed16 | unpinned_before / durable_batch1000 | 3,862.495 | 3,879.202 | 1,330.748 | 1,334.142 | 1,197.820 | 0.947x [0.915,1.02] | 0.922x [0.667,1.06] | 0.316x [0.308,0.36] | 0.895x [0.732,0.958] | legacy (some), resident (some) |
| 8192 | fixed16 | unpinned_before / durable_batch64 | 2,424.192 | 2,432.165 | 234.316 | 239.786 | 555.521 | 0.969x [0.939,1.04] | 0.977x [0.582,0.992] | 0.229x [0.228,0.253] | 2.19x [1.5,2.32] | legacy (some), resident (some) |
| 8192 | hostname | pinned / durable_batch1 | 1,853.750 | 1,862.820 | 80.275 | 82.429 | 104.927 | 0.976x [0.943,1.03] | 0.982x [0.974,1.01] | 0.0559x [0.0476,0.0577] | 1.27x [1.12,1.33] | legacy (some) |
| 8192 | hostname | pinned / durable_batch1000 | 4,539.727 | 4,529.290 | 1,708.947 | 1,614.215 | 2,187.865 | 1x [0.985,1.06] | 1.03x [0.946,1.1] | 0.483x [0.454,0.485] | 1.27x [1.23,1.39] | resident (some) |
| 8192 | hostname | pinned / durable_batch64 | 2,572.772 | 2,522.134 | 271.421 | 268.671 | 777.887 | 1.01x [0.977,1.09] | 0.99x [0.85,1.09] | 0.306x [0.29,0.337] | 2.73x [2.44,3.16] | resident (some) |
| 8192 | hostname | unpinned_after / durable_batch1 | 1,898.447 | 1,786.357 | 79.313 | 79.684 | 129.927 | 1.07x [0.993,1.09] | 0.996x [0.962,0.996] | 0.0752x [0.0727,0.0878] | 1.63x [1.62,2.06] | None observed |
| 8192 | hostname | unpinned_after / durable_batch1000 | 4,638.453 | 4,632.920 | 1,675.524 | 1,708.111 | 1,392.365 | 1.01x [0.718,1.03] | 0.998x [0.952,0.998] | 0.299x [0.22,0.301] | 0.805x [0.791,0.845] | legacy (some), resident (some) |
| 8192 | hostname | unpinned_after / durable_batch64 | 2,506.845 | 2,524.280 | 266.550 | 266.397 | 616.991 | 0.993x [0.979,1.04] | 1x [0.94,1.05] | 0.244x [0.232,0.259] | 2.33x [2.15,2.34] | resident (some) |
| 8192 | hostname | unpinned_before / durable_batch1 | 1,855.095 | 1,844.442 | 78.106 | 80.007 | 89.909 | 0.994x [0.982,1.05] | 0.979x [0.643,0.994] | 0.0488x [0.0487,0.0501] | 1.11x [0.74,1.18] | resident (some) |
| 8192 | hostname | unpinned_before / durable_batch1000 | 4,543.482 | 4,471.723 | 1,689.823 | 1,754.213 | 1,596.481 | 1.02x [0.951,1.03] | 0.976x [0.658,1] | 0.361x [0.346,0.361] | 0.904x [0.631,0.961] | legacy (some), resident (some) |
| 8192 | hostname | unpinned_before / durable_batch64 | 2,438.407 | 2,459.693 | 288.549 | 274.442 | 633.520 | 0.971x [0.939,0.998] | 1.08x [0.525,1.17] | 0.245x [0.244,0.267] | 2.19x [1.26,2.36] | legacy (some), resident (some) |
| 100000 | fixed16 | pinned / durable_batch1 | 1,900.900 | 1,873.024 | 80.766 | 81.097 | 153.036 | 0.994x [0.98,1.01] | 0.988x [0.983,1.02] | 0.0783x [0.066,0.0822] | 1.86x [1.42,1.9] | None observed |
| 100000 | fixed16 | pinned / durable_batch1000 | 10,662.285 | 10,413.189 | 2,989.380 | 3,111.099 | 6,903.958 | 1.01x [0.941,1.04] | 0.962x [0.959,0.987] | 0.657x [0.655,0.666] | 2.23x [2.18,2.34] | legacy (some) |
| 100000 | fixed16 | pinned / durable_batch64 | 2,746.064 | 2,807.018 | 324.552 | 328.995 | 959.349 | 0.999x [0.977,1.02] | 0.986x [0.954,0.99] | 0.35x [0.342,0.352] | 2.86x [2.73,2.93] | None observed |
| 100000 | fixed16 | unpinned_after / durable_batch1 | 1,918.430 | 1,850.051 | 83.609 | 83.554 | 133.725 | 0.981x [0.956,1.05] | 1.01x [1,1.01] | 0.0723x [0.0716,0.111] | 1.6x [1.6,2.57] | None observed |
| 100000 | fixed16 | unpinned_after / durable_batch1000 | 10,528.984 | 10,465.171 | 2,988.964 | 3,015.384 | 6,845.517 | 1.01x [1.01,1.13] | 0.992x [0.957,1.04] | 0.645x [0.621,0.699] | 2.3x [2.22,2.37] | None observed |
| 100000 | fixed16 | unpinned_after / durable_batch64 | 2,875.425 | 2,840.164 | 335.896 | 338.704 | 858.360 | 0.984x [0.967,1.01] | 0.993x [0.977,1] | 0.301x [0.29,0.322] | 2.53x [2.48,2.58] | None observed |
| 100000 | fixed16 | unpinned_before / durable_batch1 | 1,900.228 | 1,871.519 | 80.871 | 80.934 | 97.157 | 1.02x [0.984,1.09] | 0.989x [0.868,0.999] | 0.0495x [0.0483,0.0878] | 1.12x [0.983,1.89] | resident (some) |
| 100000 | fixed16 | unpinned_before / durable_batch1000 | 10,859.548 | 10,355.587 | 3,020.013 | 3,040.031 | 7,226.870 | 0.968x [0.966,1.05] | 0.999x [0.759,1.01] | 0.631x [0.601,0.715] | 2.09x [1.83,2.38] | resident (some) |
| 100000 | fixed16 | unpinned_before / durable_batch64 | 2,838.726 | 2,704.215 | 345.109 | 354.382 | 792.019 | 0.999x [0.993,1.05] | 0.974x [0.798,0.997] | 0.283x [0.247,0.3] | 1.97x [1.86,2.23] | resident (some) |
| 100000 | hostname | pinned / durable_batch1 | 1,989.671 | 1,987.590 | 84.606 | 83.484 | 123.410 | 1x [0.993,1.06] | 1.02x [0.987,1.03] | 0.0625x [0.0617,0.0784] | 1.45x [1.41,1.87] | None observed |
| 100000 | hostname | pinned / durable_batch1000 | 12,023.075 | 11,743.892 | 6,502.244 | 6,520.532 | 7,952.898 | 1x [0.979,1.04] | 0.995x [0.992,1.05] | 0.664x [0.658,0.681] | 1.22x [1.18,1.23] | None observed |
| 100000 | hostname | pinned / durable_batch64 | 3,134.413 | 3,032.945 | 442.331 | 479.361 | 1,127.161 | 1.03x [1.03,1.07] | 0.92x [0.897,1.13] | 0.371x [0.336,0.389] | 2.35x [2.35,2.39] | resident (some) |
| 100000 | hostname | unpinned_after / durable_batch1 | 1,812.387 | 1,871.221 | 85.560 | 84.606 | 172.243 | 1.02x [0.966,1.03] | 1.01x [1.01,1.02] | 0.0914x [0.0889,0.092] | 2.03x [1.86,2.04] | None observed |
| 100000 | hostname | unpinned_after / durable_batch1000 | 11,949.150 | 11,615.450 | 3,991.513 | 3,993.124 | 8,474.610 | 1.02x [0.972,1.03] | 0.996x [0.989,1] | 0.73x [0.716,0.743] | 2.1x [2.1,2.12] | None observed |
| 100000 | hostname | unpinned_after / durable_batch64 | 3,197.931 | 2,969.469 | 447.180 | 483.510 | 1,032.135 | 1.06x [1,1.1] | 0.925x [0.87,1.07] | 0.348x [0.334,0.36] | 2.13x [2.02,2.17] | resident (some) |
| 100000 | hostname | unpinned_before / durable_batch1 | 1,872.378 | 1,810.335 | 83.319 | 82.206 | 101.696 | 0.962x [0.952,1.13] | 1.01x [1,1.02] | 0.06x [0.0513,0.0837] | 1.24x [1.2,1.84] | legacy (some) |
| 100000 | hostname | unpinned_before / durable_batch1000 | 11,870.192 | 11,693.576 | 4,142.232 | 4,165.917 | 8,443.313 | 1.02x [0.961,1.04] | 0.986x [0.971,0.994] | 0.708x [0.683,0.753] | 1.99x [1.97,2.04] | None observed |
| 100000 | hostname | unpinned_before / durable_batch64 | 2,883.133 | 2,948.649 | 436.693 | 444.658 | 990.135 | 1.06x [0.978,1.09] | 0.958x [0.882,1.02] | 0.347x [0.336,0.367] | 2.25x [1.95,2.32] | resident (some) |
| 1000000 | fixed16 | pinned / durable_batch1 | 1,911.355 | 1,911.793 | 84.864 | 84.293 | 163.373 | 1x [0.898,1.1] | 1.01x [0.994,1.01] | 0.0874x [0.0639,0.0913] | 1.91x [1.53,1.98] | legacy (some) |
| 1000000 | fixed16 | pinned / durable_batch1000 | 15,511.990 | 15,918.908 | 5,203.798 | 5,344.160 | 10,073.175 | 0.974x [0.925,1.02] | 0.984x [0.973,1] | 0.65x [0.633,0.651] | 1.94x [1.88,1.96] | legacy (some) |
| 1000000 | fixed16 | pinned / durable_batch64 | 3,309.811 | 3,267.413 | 509.534 | 505.311 | 1,349.421 | 1.01x [0.987,1.08] | 0.971x [0.95,1.05] | 0.413x [0.392,0.469] | 2.67x [2.39,2.99] | resident (some) |
| 1000000 | fixed16 | unpinned_after / durable_batch1 | 1,943.513 | 1,836.640 | 85.894 | 86.249 | 187.810 | 1.01x [0.984,1.15] | 0.993x [0.978,1.03] | 0.0974x [0.0829,0.112] | 2.17x [1.77,2.24] | None observed |
| 1000000 | fixed16 | unpinned_after / durable_batch1000 | 16,107.199 | 15,283.139 | 5,201.043 | 5,251.922 | 12,213.956 | 1.05x [1.01,1.11] | 0.992x [0.981,0.996] | 0.795x [0.787,0.835] | 2.33x [2.31,2.35] | None observed |
| 1000000 | fixed16 | unpinned_after / durable_batch64 | 3,343.964 | 3,289.450 | 526.772 | 518.090 | 1,302.003 | 0.992x [0.949,1.13] | 1.01x [0.951,1.09] | 0.395x [0.389,0.435] | 2.51x [2.35,2.53] | legacy (some), resident (some) |
| 1000000 | fixed16 | unpinned_before / durable_batch1 | 1,787.717 | 1,727.991 | 86.086 | 86.222 | 168.070 | 1.03x [1.02,1.14] | 0.992x [0.987,1.06] | 0.0994x [0.0582,0.101] | 1.95x [1.19,1.98] | None observed |
| 1000000 | fixed16 | unpinned_before / durable_batch1000 | 16,182.430 | 15,972.741 | 5,234.752 | 5,259.306 | 11,955.044 | 1.02x [0.912,1.08] | 0.983x [0.9,1] | 0.746x [0.741,0.794] | 2.25x [2.02,2.29] | legacy (some), resident (some) |
| 1000000 | fixed16 | unpinned_before / durable_batch64 | 3,386.061 | 3,342.321 | 507.788 | 551.657 | 1,437.844 | 1.04x [0.958,1.05] | 0.92x [0.906,0.933] | 0.429x [0.405,0.462] | 2.64x [2.45,2.7] | resident (all) |
| 1000000 | hostname | pinned / durable_batch1 | 1,931.405 | 1,957.926 | 88.075 | 89.090 | 166.088 | 0.998x [0.914,1.01] | 0.981x [0.915,0.989] | 0.0848x [0.0833,0.0851] | 1.85x [1.73,1.86] | legacy (some), resident (some) |
| 1000000 | hostname | pinned / durable_batch1000 | 17,512.151 | 17,810.366 | 32,176.251 | 32,484.194 | 11,924.956 | 0.985x [0.976,1.05] | 0.992x [0.991,0.996] | 0.669x [0.628,0.674] | 0.367x [0.344,0.373] | None observed |
| 1000000 | hostname | pinned / durable_batch64 | 3,465.934 | 3,517.664 | 652.035 | 665.207 | 1,541.491 | 0.997x [0.983,1] | 0.98x [0.944,1.06] | 0.437x [0.405,0.439] | 2.32x [2.22,2.32] | resident (some) |
| 1000000 | hostname | unpinned_after / durable_batch1 | 1,894.077 | 1,951.894 | 86.873 | 86.489 | 187.444 | 0.994x [0.963,0.999] | 0.999x [0.993,1.02] | 0.0949x [0.0932,0.0965] | 2.17x [2.07,2.2] | None observed |
| 1000000 | hostname | unpinned_after / durable_batch1000 | 17,484.378 | 17,550.241 | 7,193.327 | 7,327.361 | 14,453.778 | 0.999x [0.965,1.03] | 0.982x [0.977,0.997] | 0.822x [0.819,0.824] | 1.96x [1.95,2.03] | None observed |
| 1000000 | hostname | unpinned_after / durable_batch64 | 3,434.629 | 3,432.091 | 664.529 | 677.213 | 1,467.460 | 1.01x [0.96,1.06] | 0.985x [0.953,1.07] | 0.428x [0.421,0.431] | 2.17x [2.16,2.38] | None observed |
| 1000000 | hostname | unpinned_before / durable_batch1 | 1,762.196 | 1,812.228 | 89.147 | 87.299 | 176.180 | 0.972x [0.956,1.13] | 1.01x [0.997,1.02] | 0.0981x [0.0955,0.104] | 2.03x [1.94,2.07] | None observed |
| 1000000 | hostname | unpinned_before / durable_batch1000 | 17,581.325 | 17,862.575 | 7,221.525 | 7,163.605 | 14,161.933 | 0.984x [0.983,0.995] | 0.992x [0.987,1.04] | 0.784x [0.775,0.841] | 1.98x [1.91,2.01] | None observed |
| 1000000 | hostname | unpinned_before / durable_batch64 | 3,528.505 | 3,592.191 | 644.471 | 649.813 | 1,576.676 | 1x [0.964,1.02] | 0.993x [0.968,1.02] | 0.439x [0.421,0.516] | 2.42x [2.37,2.7] | None observed |

## Build, drop, snapshot and reopen lifecycle

Milliseconds per event. Each event is observed once per fresh process, rather than seven samples. Resident performs an explicitly charged ordinary Collect before retaining the old snapshot in both native and hash arms; Legacy and LMDB use a no-op. The total_snapshot_prepare row sums maintenance_before_pin + begin_snapshot + prepare_lookup for each process before aggregation. Maintenance is never treated as free. The capability does not collect automatically. Resident's common benchmark pool is ceil(keys/24)+8192 KV slots with256MiB metadata admission, providing room for both pinned generations; this is a disclosed benchmark setting, not a change to engine defaults. Rebuild includes acquiring the new snapshot and complete preparation. Final_validation measures the complete final byte verification. Initial loaders use different APIs and are labeled in raw reports; their costs do not establish equal-loader superiority. Reopen uses a warm OS cache. N/A identifies engine-specific events.

| Keys | Key type | Phase / operation | Legacy native | Legacy hash | Resident native | Resident hash | LMDB | Legacy hash/native | Resident hash/native | Legacy hash/LMDB | Resident hash/LMDB | >5% hash cost increase vs native |
|---:|---|---|---:|---:|---:|---:|---:|---|---|---|---|---|
| 8192 | fixed16 | begin_snapshot | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | 0.889x [0.818,1] | 0.833x [0.75,1] | 32.6x [28.7,47.4] | 48.9x [41.5,52.6] | legacy (some), resident (some) |
| 8192 | fixed16 | close_environment | 5.658 | 5.954 | 44.678 | 45.389 | 1.420 | 0.95x [0.896,1.17] | 0.984x [0.957,1.28] | 0.238x [0.234,0.293] | 0.0307x [0.0305,0.0321] | legacy (some) |
| 8192 | fixed16 | create_environment | 5.276 | 4.255 | 449.110 | 435.771 | 0.501 | 1.24x [1.19,1.35] | 1.02x [0.992,1.04] | 0.136x [0.107,0.166] | 0.00115x [0.00109,0.00161] | None observed |
| 8192 | fixed16 | drop_new_lookup | 0.000 | 0.000 | 0.000 | 0.001 | 0.000 | 0.805x [0.639,0.892] | 0.48x [0.42,0.51] | 0.128x [0.122,0.184] | 0.106x [0.096,0.123] | legacy (all), resident (all) |
| 8192 | fixed16 | drop_old_lookup | 0.000 | 0.000 | 0.000 | 0.116 | 0.000 | 0.464x [0.444,0.583] | 0.000663x [0.000602,0.00121] | 0.199x [0.167,0.25] | 0.000258x [0.000226,0.000331] | legacy (all), resident (all) |
| 8192 | fixed16 | final_validation | 2.745 | 0.661 | 1.918 | 0.726 | 1.638 | 4.15x [3.99,4.16] | 2.64x [2.58,2.68] | 2.48x [2.36,2.51] | 2.25x [2.25,2.28] | None observed |
| 8192 | fixed16 | fixture_load | 6.891 | 7.185 | 10.416 | 10.715 | 6.823 | 0.993x [0.959,1.04] | 0.964x [0.473,1.06] | 1.1x [0.95,1.43] | 0.598x [0.31,0.815] | resident (some) |
| 8192 | fixed16 | maintenance_before_pin | 0.002 | 0.002 | 2.274 | 2.252 | 0.001 | 1.05x [0.921,1.07] | 0.983x [0.723,1.58] | 0.636x [0.625,0.8] | 0.000509x [0.000489,0.000715] | legacy (some), resident (some) |
| 8192 | fixed16 | prepare_after_reopen | 0.002 | 0.822 | 0.003 | 1.364 | 0.000 | 0.00206x [0.00197,0.00224] | 0.00205x [0.00192,0.00227] | 4.86e-05x [4.82e-05,4.91e-05] | 2.93e-05x [2.86e-05,3e-05] | legacy (all), resident (all) |
| 8192 | fixed16 | prepare_final_validation | 0.001 | 0.820 | 0.001 | 0.999 | 0.000 | 0.00143x [0.00114,0.00163] | 0.000952x [0.000786,0.00101] | 0.000433x [0.000293,0.000448] | 0.000353x [0.0002,0.000482] | legacy (all), resident (all) |
| 8192 | fixed16 | prepare_lookup | 0.001 | 0.842 | 0.001 | 1.362 | 0.000 | 0.00147x [0.00115,0.0015] | 0.000799x [0.000736,0.00092] | 8.14e-05x [7.13e-05,8.33e-05] | 4.75e-05x [4.36e-05,5.14e-05] | legacy (all), resident (all) |
| 8192 | fixed16 | rebuild_after_pinned_writes | 0.002 | 0.793 | 0.001 | 0.916 | 0.002 | 0.00233x [0.00182,0.00252] | 0.00146x [0.00145,0.0016] | 0.00275x [0.0022,0.00283] | 0.00239x [0.0019,0.00247] | legacy (all), resident (all) |
| 8192 | fixed16 | release_old_snapshot | 0.052 | 0.050 | 0.007 | 0.008 | 0.000 | 1.05x [1.02,1.05] | 0.952x [0.811,1.04] | 0.00254x [0.00182,0.00398] | 0.0169x [0.0144,0.0245] | resident (some) |
| 8192 | fixed16 | reopen_database_handle | N/A | N/A | N/A | N/A | 0.008 | N/A | N/A | N/A | N/A | None observed |
| 8192 | fixed16 | reopen_environment | 48.726 | 48.967 | 365.462 | 386.050 | 0.165 | 0.997x [0.979,1.03] | 0.968x [0.892,1.03] | 0.00338x [0.00323,0.00369] | 0.000429x [0.00041,0.000464] | resident (some) |
| 8192 | fixed16 | preparation / total_snapshot_prepare | 0.003 | 0.844 | 2.275 | 3.614 | 0.005 | 0.00383x [0.00368,0.00403] | 0.613x [0.479,0.965] | 0.00549x [0.00488,0.00579] | 0.00114x [0.00105,0.00143] | legacy (all), resident (some) |
| 8192 | hostname | begin_snapshot | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | 1.22x [1.12,1.38] | 1x [0.75,1] | 52.9x [44.2,53.2] | 71x [44.2,79.3] | resident (some) |
| 8192 | hostname | close_environment | 7.112 | 7.433 | 44.609 | 45.428 | 2.077 | 0.993x [0.575,1.1] | 0.982x [0.979,1.04] | 0.27x [0.17,0.321] | 0.0447x [0.0446,0.0457] | legacy (some) |
| 8192 | hostname | create_environment | 4.098 | 3.839 | 436.795 | 451.565 | 0.453 | 1.06x [0.833,1.38] | 0.986x [0.819,0.997] | 0.092x [0.0807,0.149] | 0.000849x [0.000714,0.00119] | legacy (some), resident (some) |
| 8192 | hostname | drop_new_lookup | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.814x [0.794,1.41] | 0.684x [0.414,0.697] | 0.206x [0.162,0.222] | 0.171x [0.0951,0.182] | legacy (some), resident (all) |
| 8192 | hostname | drop_old_lookup | 0.000 | 0.000 | 0.000 | 0.123 | 0.000 | 0.317x [0.303,0.333] | 0.000568x [0.000556,0.000587] | 0.13x [0.125,0.136] | 0.000243x [0.000238,0.000294] | legacy (all), resident (all) |
| 8192 | hostname | final_validation | 2.645 | 0.743 | 2.845 | 0.892 | 1.598 | 3.52x [1.47,3.58] | 3.19x [3.15,3.22] | 2.15x [0.886,2.16] | 1.79x [1.75,1.81] | None observed |
| 8192 | hostname | fixture_load | 7.732 | 7.493 | 20.397 | 19.748 | 7.700 | 1.03x [0.949,1.15] | 0.997x [0.616,1.21] | 0.945x [0.787,1.17] | 0.301x [0.233,0.441] | legacy (some), resident (some) |
| 8192 | hostname | maintenance_before_pin | 0.002 | 0.002 | 1.189 | 1.179 | 0.001 | 1.05x [1.03,1.16] | 1.02x [0.545,1.29] | 0.597x [0.572,0.657] | 0.00111x [0.000689,0.00147] | resident (some) |
| 8192 | hostname | prepare_after_reopen | 0.002 | 0.932 | 0.003 | 1.607 | 0.000 | 0.00165x [0.00093,0.0018] | 0.00201x [0.00189,0.00202] | 4.29e-05x [2.43e-05,4.35e-05] | 2.49e-05x [2.43e-05,2.56e-05] | legacy (all), resident (all) |
| 8192 | hostname | prepare_final_validation | 0.001 | 0.889 | 0.001 | 1.107 | 0.000 | 0.00145x [0.000611,0.00182] | 0.000891x [0.000832,0.00126] | 0.00035x [0.00014,0.000791] | 0.000299x [0.000284,0.000623] | legacy (all), resident (all) |
| 8192 | hostname | prepare_lookup | 0.001 | 0.902 | 0.001 | 1.424 | 0.000 | 0.000977x [0.000976,0.00138] | 0.000541x [0.000403,0.000763] | 7.76e-05x [6.57e-05,7.87e-05] | 4.23e-05x [3.17e-05,4.92e-05] | legacy (all), resident (all) |
| 8192 | hostname | rebuild_after_pinned_writes | 0.001 | 0.908 | 0.001 | 1.096 | 0.002 | 0.0015x [0.00145,0.00168] | 0.000996x [0.000901,0.00117] | 0.00197x [0.00167,0.00206] | 0.00163x [0.00137,0.00175] | legacy (all), resident (all) |
| 8192 | hostname | release_old_snapshot | 0.052 | 0.052 | 0.058 | 0.060 | 0.000 | 0.985x [0.98,1.02] | 0.978x [0.965,0.994] | 0.00375x [0.00304,0.00598] | 0.0032x [0.00264,0.00521] | None observed |
| 8192 | hostname | reopen_database_handle | N/A | N/A | N/A | N/A | 0.008 | N/A | N/A | N/A | N/A | None observed |
| 8192 | hostname | reopen_environment | 63.881 | 63.802 | 402.895 | 413.130 | 0.255 | 1x [0.746,1.01] | 0.975x [0.953,0.977] | 0.00398x [0.00305,0.00399] | 0.000612x [0.000596,0.000631] | legacy (some) |
| 8192 | hostname | preparation / total_snapshot_prepare | 0.003 | 0.904 | 1.190 | 2.597 | 0.006 | 0.00364x [0.00338,0.0042] | 0.461x [0.237,0.507] | 0.00615x [0.00529,0.00695] | 0.00217x [0.00122,0.00264] | legacy (all), resident (all) |
| 100000 | fixed16 | begin_snapshot | 0.000 | 0.000 | 0.000 | 0.000 | 0.007 | 1x [0.661,1.25] | 1x [0.331,1.01] | 76.7x [69.2,87.5] | 115x [38.7,120] | legacy (some), resident (some) |
| 100000 | fixed16 | close_environment | 20.075 | 21.016 | 67.232 | 68.245 | 7.659 | 0.952x [0.925,1.13] | 0.996x [0.963,1.01] | 0.363x [0.34,0.365] | 0.112x [0.0934,0.114] | legacy (some) |
| 100000 | fixed16 | create_environment | 3.760 | 3.714 | 451.868 | 468.774 | 0.322 | 0.781x [0.725,1.04] | 1.01x [0.962,1.04] | 0.0946x [0.0867,0.0996] | 0.000734x [0.000681,0.00102] | legacy (some) |
| 100000 | fixed16 | drop_new_lookup | 0.001 | 1.272 | 0.000 | 1.245 | 0.000 | 0.000525x [0.000284,0.000527] | 0.000386x [0.00029,0.000395] | 0.000232x [0.00022,0.000348] | 0.000242x [0.000216,0.000357] | legacy (all), resident (all) |
| 100000 | fixed16 | drop_old_lookup | 0.000 | 1.123 | 0.000 | 1.168 | 0.000 | 6.42e-05x [5.34e-05,0.000161] | 0.000132x [6.93e-05,0.000188] | 2.67e-05x [1.58e-05,3.02e-05] | 2.57e-05x [1.65e-05,2.6e-05] | legacy (all), resident (all) |
| 100000 | fixed16 | final_validation | 40.315 | 11.232 | 28.726 | 11.379 | 26.060 | 3.56x [3.38,3.7] | 2.52x [2.51,2.54] | 2.31x [2.04,2.39] | 2.29x [2.26,2.3] | None observed |
| 100000 | fixed16 | fixture_load | 34.027 | 33.226 | 132.860 | 139.656 | 45.546 | 1.01x [0.985,1.02] | 0.962x [0.775,0.982] | 1.41x [1.31,1.66] | 0.335x [0.259,0.341] | resident (some) |
| 100000 | fixed16 | maintenance_before_pin | 0.003 | 0.003 | 5.044 | 4.961 | 0.001 | 1.17x [0.967,1.24] | 1x [0.716,1.02] | 0.561x [0.446,0.722] | 0.000251x [0.000202,0.000337] | resident (some) |
| 100000 | fixed16 | prepare_after_reopen | 0.002 | 12.500 | 0.003 | 17.717 | 0.000 | 0.000123x [0.000113,0.000124] | 0.000166x [0.000157,0.000169] | 3.31e-06x [3.2e-06,3.72e-06] | 2.26e-06x [2.19e-06,2.82e-06] | legacy (all), resident (all) |
| 100000 | fixed16 | prepare_final_validation | 0.001 | 15.949 | 0.001 | 18.285 | 0.000 | 9.36e-05x [8.45e-05,0.000124] | 7.75e-05x [7.45e-05,0.000101] | 2.69e-05x [1.31e-05,2.95e-05] | 2.54e-05x [1.1e-05,2.69e-05] | legacy (all), resident (all) |
| 100000 | fixed16 | prepare_lookup | 0.001 | 18.499 | 0.001 | 21.265 | 0.000 | 7.39e-05x [6.67e-05,7.85e-05] | 5.41e-05x [4.1e-05,0.000499] | 9.78e-06x [3.61e-06,1.15e-05] | 6.73e-06x [3.29e-06,8.8e-06] | legacy (all), resident (all) |
| 100000 | fixed16 | rebuild_after_pinned_writes | 0.003 | 19.070 | 0.003 | 19.471 | 0.004 | 0.000166x [0.000122,0.000173] | 0.000147x [0.000141,0.000147] | 0.000199x [0.000193,0.000244] | 0.000195x [0.000191,0.000238] | legacy (all), resident (all) |
| 100000 | fixed16 | release_old_snapshot | 0.115 | 0.108 | 0.367 | 0.371 | 0.000 | 1.09x [1.07,1.1] | 0.99x [0.722,1.02] | 0.00296x [0.00109,0.00449] | 0.000639x [0.000327,0.00116] | resident (some) |
| 100000 | fixed16 | reopen_database_handle | N/A | N/A | N/A | N/A | 0.011 | N/A | N/A | N/A | N/A | None observed |
| 100000 | fixed16 | reopen_environment | 207.271 | 206.349 | 755.479 | 762.433 | 0.259 | 1x [0.988,1.01] | 0.991x [0.984,1.01] | 0.00126x [0.00124,0.00128] | 0.00034x [0.000334,0.000355] | None observed |
| 100000 | fixed16 | preparation / total_snapshot_prepare | 0.004 | 18.502 | 5.045 | 26.227 | 0.009 | 0.000229x [0.00021,0.000266] | 0.19x [0.141,0.198] | 0.000496x [0.000424,0.000552] | 0.000313x [0.000234,0.000401] | legacy (all), resident (all) |
| 100000 | hostname | begin_snapshot | 0.000 | 0.000 | 0.000 | 0.000 | 0.007 | 0.889x [0.583,1.11] | 1x [1,1.16] | 84.6x [54.4,87.3] | 97.9x [93.2,112] | legacy (some) |
| 100000 | hostname | close_environment | 23.756 | 22.761 | 68.400 | 70.510 | 8.673 | 1.01x [0.887,1.06] | 0.97x [0.888,0.982] | 0.364x [0.285,0.389] | 0.124x [0.0826,0.126] | legacy (some), resident (some) |
| 100000 | hostname | create_environment | 3.031 | 2.564 | 448.280 | 448.642 | 0.295 | 1.03x [1.03,1.18] | 0.999x [0.965,1] | 0.0798x [0.0744,0.117] | 0.000637x [0.000428,0.000658] | None observed |
| 100000 | hostname | drop_new_lookup | 0.001 | 0.002 | 0.001 | 1.237 | 0.000 | 0.657x [0.534,0.959] | 0.000796x [0.000693,0.000826] | 0.139x [0.0919,0.343] | 0.000147x [0.000102,0.000478] | legacy (some), resident (all) |
| 100000 | hostname | drop_old_lookup | 0.000 | 0.000 | 0.000 | 1.175 | 0.000 | 0.233x [0.132,0.356] | 0.00012x [4.42e-05,0.000129] | 0.0713x [0.0248,0.0997] | 2.55e-05x [1.66e-05,2.55e-05] | legacy (all), resident (all) |
| 100000 | hostname | final_validation | 41.139 | 15.981 | 47.396 | 17.459 | 25.549 | 2.57x [2.47,2.65] | 2.71x [2.51,2.74] | 1.63x [1.6,1.63] | 1.45x [1.37,1.5] | None observed |
| 100000 | hostname | fixture_load | 38.845 | 38.689 | 247.958 | 243.627 | 49.772 | 1x [0.925,1.03] | 1.01x [0.999,1.1] | 1.29x [1.15,1.36] | 0.205x [0.199,0.205] | legacy (some) |
| 100000 | hostname | maintenance_before_pin | 0.003 | 0.003 | 10.559 | 10.791 | 0.001 | 1.07x [0.882,1.07] | 0.98x [0.978,0.988] | 0.515x [0.482,0.646] | 0.000132x [0.000126,0.000146] | legacy (some) |
| 100000 | hostname | prepare_after_reopen | 0.002 | 15.597 | 0.003 | 20.980 | 0.000 | 0.0001x [9.5e-05,0.000115] | 0.00016x [0.000156,0.000168] | 3.18e-06x [2.59e-06,2.44e-05] | 2.44e-06x [1.9e-06,1.82e-05] | legacy (all), resident (all) |
| 100000 | hostname | prepare_final_validation | 0.001 | 16.258 | 0.002 | 24.899 | 0.000 | 8.15e-05x [7.44e-05,9.92e-05] | 6.01e-05x [5.88e-05,6.54e-05] | 2.16e-05x [1.97e-05,2.46e-05] | 1.34e-05x [1.33e-05,1.65e-05] | legacy (all), resident (all) |
| 100000 | hostname | prepare_lookup | 0.001 | 18.475 | 0.001 | 24.217 | 0.000 | 7.88e-05x [7.1e-05,9.11e-05] | 4.43e-05x [4.35e-05,4.91e-05] | 3.25e-06x [2.91e-06,1.15e-05] | 2.49e-06x [2.46e-06,8.71e-06] | legacy (all), resident (all) |
| 100000 | hostname | rebuild_after_pinned_writes | 0.003 | 20.980 | 0.003 | 24.752 | 0.005 | 0.000164x [0.000149,0.000173] | 0.000123x [0.000116,0.000151] | 0.000259x [0.00024,0.00026] | 0.00021x [0.000209,0.000215] | legacy (all), resident (all) |
| 100000 | hostname | release_old_snapshot | 0.116 | 0.118 | 0.680 | 0.683 | 0.000 | 0.974x [0.939,1.02] | 0.996x [0.97,1.02] | 0.00258x [0.00202,0.0029] | 0.00047x [0.000333,0.00051] | legacy (some) |
| 100000 | hostname | reopen_database_handle | N/A | N/A | N/A | N/A | 0.012 | N/A | N/A | N/A | N/A | None observed |
| 100000 | hostname | reopen_environment | 234.920 | 235.060 | 903.344 | 907.596 | 0.266 | 0.999x [0.987,1] | 1.01x [0.942,1.01] | 0.00113x [0.00109,0.00114] | 0.000293x [0.000269,0.000299] | resident (some) |
| 100000 | hostname | preparation / total_snapshot_prepare | 0.004 | 18.477 | 10.560 | 35.027 | 0.008 | 0.000225x [0.000213,0.000236] | 0.303x [0.3,0.303] | 0.00045x [0.000446,0.000458] | 0.000242x [0.000233,0.000266] | legacy (all), resident (all) |
| 1000000 | fixed16 | begin_snapshot | 0.000 | 0.000 | 0.000 | 0.000 | 0.007 | 0.988x [0.362,1.44] | 0.636x [0.442,1.14] | 82.8x [34.5,85.8] | 63.2x [42.2,106] | legacy (some), resident (some) |
| 1000000 | fixed16 | close_environment | 32.665 | 30.474 | 278.315 | 280.518 | 15.139 | 1.09x [0.98,1.09] | 1x [0.992,1] | 0.497x [0.393,0.537] | 0.0538x [0.0467,0.0578] | None observed |
| 1000000 | fixed16 | create_environment | 2.590 | 2.613 | 600.112 | 598.892 | 0.307 | 0.967x [0.957,1.08] | 1x [0.998,1.04] | 0.124x [0.105,0.125] | 0.000514x [0.00045,0.000562] | None observed |
| 1000000 | fixed16 | drop_new_lookup | 0.001 | 10.201 | 0.001 | 9.762 | 0.000 | 0.0001x [8.38e-05,0.000119] | 9.24e-05x [8.18e-05,0.000123] | 2.74e-05x [2.61e-05,2.89e-05] | 2.87e-05x [2.64e-05,3.36e-05] | legacy (all), resident (all) |
| 1000000 | fixed16 | drop_old_lookup | 0.000 | 10.582 | 0.000 | 10.387 | 0.000 | 1.49e-05x [7.56e-06,1.88e-05] | 7.75e-06x [6.74e-06,1.82e-05] | 2.84e-06x [2.8e-06,2.97e-06] | 2.89e-06x [2.87e-06,2.91e-06] | legacy (all), resident (all) |
| 1000000 | fixed16 | final_validation | 466.213 | 211.069 | 362.262 | 214.094 | 291.193 | 2.23x [2.21,2.27] | 1.69x [1.69,1.87] | 1.39x [1.36,1.4] | 1.36x [1.36,1.38] | None observed |
| 1000000 | fixed16 | fixture_load | 252.346 | 249.823 | 1,361.063 | 1,356.258 | 458.293 | 1x [0.993,1.04] | 1x [0.981,1.02] | 1.83x [1.82,1.83] | 0.338x [0.327,0.339] | None observed |
| 1000000 | fixed16 | maintenance_before_pin | 0.003 | 0.003 | 37.090 | 37.150 | 0.002 | 0.936x [0.914,0.982] | 1.01x [0.99,1.01] | 0.536x [0.487,0.583] | 4.1e-05x [3.67e-05,4.43e-05] | legacy (some) |
| 1000000 | fixed16 | prepare_after_reopen | 0.002 | 268.713 | 0.003 | 284.788 | 0.000 | 5.85e-06x [5.33e-06,6.87e-06] | 1.19e-05x [1.13e-05,1.23e-05] | 1.86e-07x [1.48e-07,1.86e-07] | 1.76e-07x [1.39e-07,1.77e-07] | legacy (all), resident (all) |
| 1000000 | fixed16 | prepare_final_validation | 0.002 | 272.229 | 0.002 | 280.618 | 0.001 | 6.73e-06x [6.65e-06,7.25e-06] | 5.61e-06x [5.23e-06,6.46e-06] | 2.03e-06x [1.19e-06,2.06e-06] | 1.99e-06x [1.14e-06,2e-06] | legacy (all), resident (all) |
| 1000000 | fixed16 | prepare_lookup | 0.002 | 269.658 | 0.001 | 283.632 | 0.000 | 6.09e-06x [4.89e-06,6.43e-06] | 4.49e-06x [4.38e-06,5.99e-06] | 2.23e-07x [1.86e-07,2.53e-07] | 2.12e-07x [1.65e-07,2.47e-07] | legacy (all), resident (all) |
| 1000000 | fixed16 | rebuild_after_pinned_writes | 0.004 | 268.366 | 0.004 | 284.115 | 0.006 | 1.32e-05x [1.18e-05,1.5e-05] | 1.43e-05x [1.24e-05,1.52e-05] | 2.06e-05x [1.94e-05,2.25e-05] | 1.94e-05x [1.84e-05,2.25e-05] | legacy (all), resident (all) |
| 1000000 | fixed16 | release_old_snapshot | 0.117 | 0.111 | 3.426 | 3.553 | 0.001 | 1.17x [0.871,1.26] | 0.981x [0.964,1.2] | 0.00459x [0.00374,0.00519] | 0.000146x [0.000124,0.000146] | legacy (some) |
| 1000000 | fixed16 | reopen_database_handle | N/A | N/A | N/A | N/A | 0.011 | N/A | N/A | N/A | N/A | None observed |
| 1000000 | fixed16 | reopen_environment | 341.268 | 339.624 | 3,414.054 | 3,399.093 | 0.270 | 1x [0.999,1.01] | 0.998x [0.995,1.05] | 0.000791x [0.000738,0.000846] | 7.96e-05x [7.37e-05,8.36e-05] | None observed |
| 1000000 | fixed16 | preparation / total_snapshot_prepare | 0.004 | 269.661 | 37.091 | 320.308 | 0.009 | 1.57e-05x [1.55e-05,1.66e-05] | 0.116x [0.108,0.117] | 3.29e-05x [3.08e-05,3.48e-05] | 2.74e-05x [2.66e-05,2.78e-05] | legacy (all), resident (all) |
| 1000000 | hostname | begin_snapshot | 0.000 | 0.000 | 0.000 | 0.000 | 0.008 | 1x [0.546,1.5] | 0.778x [0.7,1.3] | 83.2x [64.7,96.1] | 96.1x [84.2,119] | legacy (some), resident (some) |
| 1000000 | hostname | close_environment | 37.297 | 38.485 | 279.284 | 279.282 | 17.992 | 1.01x [0.958,1.03] | 1x [0.984,1.02] | 0.467x [0.463,0.499] | 0.0644x [0.062,0.0687] | None observed |
| 1000000 | hostname | create_environment | 3.713 | 3.586 | 616.203 | 608.398 | 0.332 | 1.04x [0.872,1.04] | 1x [0.992,1.05] | 0.0925x [0.0558,0.0969] | 0.000541x [0.000381,0.000569] | legacy (some) |
| 1000000 | hostname | drop_new_lookup | 0.001 | 9.846 | 0.001 | 8.810 | 0.000 | 0.000106x [8.19e-05,0.000132] | 0.000122x [8.41e-05,0.000145] | 3.46e-05x [1.91e-05,4.1e-05] | 3.87e-05x [1.68e-05,4.58e-05] | legacy (all), resident (all) |
| 1000000 | hostname | drop_old_lookup | 0.000 | 10.373 | 0.000 | 10.133 | 0.000 | 6.75e-06x [6.57e-06,1.42e-05] | 1.42e-05x [1.38e-05,1.85e-05] | 2.89e-06x [2.83e-06,3.28e-06] | 2.96e-06x [2.77e-06,3.04e-06] | legacy (all), resident (all) |
| 1000000 | hostname | final_validation | 496.601 | 251.111 | 543.797 | 272.111 | 311.550 | 1.99x [1.94,1.99] | 2x [2,2] | 1.24x [1.21,1.26] | 1.14x [1.14,1.16] | None observed |
| 1000000 | hostname | fixture_load | 357.980 | 348.887 | 2,520.892 | 2,547.507 | 534.202 | 1.03x [0.974,1.05] | 0.99x [0.988,0.997] | 1.53x [1.52,1.55] | 0.21x [0.207,0.213] | None observed |
| 1000000 | hostname | maintenance_before_pin | 0.003 | 0.003 | 94.343 | 95.766 | 0.001 | 1.01x [0.858,1.1] | 0.99x [0.918,0.995] | 0.453x [0.421,0.495] | 1.37e-05x [1.29e-05,1.39e-05] | legacy (some), resident (some) |
| 1000000 | hostname | prepare_after_reopen | 0.002 | 307.002 | 0.003 | 340.936 | 0.000 | 4.95e-06x [4.3e-06,5.03e-06] | 9.7e-06x [9.61e-06,1.12e-05] | 1.3e-07x [1.3e-07,1.64e-07] | 1.17e-07x [1.17e-07,1.47e-07] | legacy (all), resident (all) |
| 1000000 | hostname | prepare_final_validation | 0.002 | 307.059 | 0.002 | 341.305 | 0.000 | 5.98e-06x [5.17e-06,6.59e-06] | 5.19e-06x [4.12e-06,5.58e-06] | 1.11e-06x [7.78e-07,2.09e-06] | 1.01e-06x [6.98e-07,1.88e-06] | legacy (all), resident (all) |
| 1000000 | hostname | prepare_lookup | 0.002 | 307.288 | 0.001 | 338.955 | 0.000 | 5.5e-06x [5.02e-06,5.55e-06] | 3.92e-06x [3.71e-06,4.2e-06] | 2.28e-07x [1.98e-07,4.85e-07] | 2.07e-07x [1.77e-07,4.48e-07] | legacy (all), resident (all) |
| 1000000 | hostname | rebuild_after_pinned_writes | 0.004 | 309.499 | 0.004 | 341.521 | 0.006 | 1.26e-05x [1.19e-05,1.4e-05] | 1.23e-05x [1.18e-05,1.32e-05] | 2.05e-05x [1.92e-05,2.14e-05] | 1.88e-05x [1.71e-05,1.93e-05] | legacy (all), resident (all) |
| 1000000 | hostname | release_old_snapshot | 0.114 | 0.110 | 5.525 | 5.623 | 0.000 | 1.08x [1.02,1.09] | 0.983x [0.977,0.992] | 0.00364x [0.00356,0.00371] | 6.95e-05x [6.83e-05,7.21e-05] | None observed |
| 1000000 | hostname | reopen_database_handle | N/A | N/A | N/A | N/A | 0.011 | N/A | N/A | N/A | N/A | None observed |
| 1000000 | hostname | reopen_environment | 410.104 | 411.628 | 5,372.515 | 5,417.075 | 0.262 | 0.996x [0.995,1.01] | 0.992x [0.984,1] | 0.000635x [0.000626,0.000654] | 4.83e-05x [4.8e-05,4.93e-05] | None observed |
| 1000000 | hostname | preparation / total_snapshot_prepare | 0.005 | 307.291 | 94.344 | 435.534 | 0.010 | 1.48e-05x [1.38e-05,1.64e-05] | 0.217x [0.214,0.218] | 3.19e-05x [3.16e-05,3.28e-05] | 2.24e-05x [2.22e-05,2.32e-05] | legacy (all), resident (all) |

## Actual per-process RSS at every checkpoint

MiB of Linux VmRSS. All input/query/model storage, mappings and allocator retention remain included. The after_full_validation checkpoint follows full source reads for every arm; after_prepare alone also reflects hash's eager source-page touching. Two-generations checkpoints retain both old and new lookup pairs. Dropping a lookup releases its table; the caller's old transaction continues retaining source pages until separately dropped. Allocators may retain freed storage. All other status counters, including RssAnon/RssFile/VmHWM/VmPTE, are retained in FULL_TABLE.csv and summary.json. VmHWM is cumulative, not isolated index peak usage.

| Keys | Key type | Phase / operation | Legacy native | Legacy hash | Resident native | Resident hash | LMDB | Legacy hash/native | Resident hash/native | Legacy hash/LMDB | Resident hash/LMDB | >5% hash cost increase vs native |
|---:|---|---|---:|---:|---:|---:|---:|---|---|---|---|---|
| 8192 | fixed16 | after_full_validation / vmrss_kib | 26.277 | 26.293 | 40.273 | 40.984 | 6.191 | 0.997x [0.995,1.01] | 0.983x [0.982,0.986] | 0.235x [0.234,0.238] | 0.151x [0.151,0.152] | None observed |
| 8192 | fixed16 | after_old_lookup_drop / vmrss_kib | 48.602 | 49.207 | 42.730 | 42.824 | 28.527 | 0.988x [0.987,0.988] | 0.999x [0.997,1] | 0.58x [0.577,0.581] | 0.667x [0.664,0.667] | None observed |
| 8192 | fixed16 | after_old_snapshot_release / vmrss_kib | 48.602 | 49.207 | 42.730 | 42.824 | 28.527 | 0.988x [0.987,0.988] | 0.999x [0.997,1] | 0.58x [0.577,0.581] | 0.667x [0.664,0.667] | None observed |
| 8192 | fixed16 | after_pinned_writes / vmrss_kib | 48.602 | 49.207 | 42.730 | 43.449 | 28.047 | 0.988x [0.987,0.988] | 0.984x [0.982,0.987] | 0.57x [0.567,0.571] | 0.646x [0.644,0.646] | None observed |
| 8192 | fixed16 | after_prepare / vmrss_kib | 26.277 | 26.293 | 40.273 | 40.984 | 6.191 | 0.997x [0.995,1.01] | 0.983x [0.982,0.986] | 0.235x [0.234,0.238] | 0.151x [0.151,0.152] | None observed |
| 8192 | fixed16 | after_reopen_full_validation / vmrss_kib | 68.574 | 69.160 | 42.133 | 42.848 | 9.113 | 0.991x [0.991,0.992] | 0.984x [0.982,0.987] | 0.132x [0.13,0.132] | 0.213x [0.21,0.214] | None observed |
| 8192 | fixed16 | before_prepare / vmrss_kib | 26.277 | 26.152 | 40.273 | 40.359 | 6.191 | 1x [0.999,1.02] | 1x [0.997,1] | 0.237x [0.235,0.239] | 0.154x [0.153,0.155] | None observed |
| 8192 | fixed16 | closed_environment / vmrss_kib | 8.293 | 8.914 | 8.773 | 8.867 | 7.828 | 0.932x [0.927,0.934] | 0.993x [0.985,1.01] | 0.878x [0.871,0.882] | 0.883x [0.873,0.886] | legacy (all) |
| 8192 | fixed16 | fixture_ready / vmrss_kib | 3.105 | 3.129 | 3.148 | 3.117 | 3.219 | 0.993x [0.971,1] | 0.999x [0.976,1.02] | 1.02x [0.973,1.03] | 1.02x [0.981,1.03] | None observed |
| 8192 | fixed16 | loaded_source_only / vmrss_kib | 5.031 | 5.031 | 38.836 | 38.930 | 4.070 | 1x [0.995,1.01] | 1x [0.997,1] | 0.809x [0.784,0.815] | 0.105x [0.102,0.105] | None observed |
| 8192 | fixed16 | pinned_batch1 / vmrss_kib | 29.254 | 29.859 | 42.730 | 43.449 | 9.281 | 0.98x [0.978,0.98] | 0.984x [0.982,0.987] | 0.311x [0.308,0.311] | 0.214x [0.212,0.214] | None observed |
| 8192 | fixed16 | pinned_batch1000 / vmrss_kib | 48.602 | 49.207 | 42.730 | 43.449 | 28.047 | 0.988x [0.987,0.988] | 0.984x [0.982,0.987] | 0.57x [0.567,0.571] | 0.646x [0.644,0.646] | None observed |
| 8192 | fixed16 | pinned_batch64 / vmrss_kib | 35.160 | 35.766 | 42.730 | 43.449 | 14.832 | 0.983x [0.982,0.984] | 0.984x [0.982,0.987] | 0.415x [0.412,0.416] | 0.342x [0.34,0.342] | None observed |
| 8192 | fixed16 | process_start_before_fixture / vmrss_kib | 2.312 | 2.312 | 2.371 | 2.375 | 2.438 | 1x [0.972,1.03] | 1x [0.997,1] | 1.05x [1,1.06] | 1.02x [0.974,1.08] | None observed |
| 8192 | fixed16 | resident_ballast_ready / vmrss_kib | 3.109 | 3.133 | 3.152 | 3.121 | 3.223 | 0.993x [0.971,1] | 0.999x [0.976,1.02] | 1.02x [0.973,1.03] | 1.02x [0.981,1.03] | None observed |
| 8192 | fixed16 | two_generations / vmrss_kib | 48.602 | 49.207 | 42.730 | 43.449 | 28.047 | 0.988x [0.987,0.988] | 0.984x [0.982,0.987] | 0.57x [0.567,0.571] | 0.646x [0.644,0.646] | None observed |
| 8192 | fixed16 | unpinned_after_batch1 / vmrss_kib | 49.148 | 49.754 | 42.730 | 42.824 | 28.527 | 0.988x [0.987,0.988] | 0.999x [0.997,1] | 0.573x [0.57,0.574] | 0.667x [0.665,0.667] | None observed |
| 8192 | fixed16 | unpinned_after_batch1000 / vmrss_kib | 68.574 | 69.160 | 42.730 | 42.824 | 28.527 | 0.991x [0.991,0.992] | 0.999x [0.997,1] | 0.413x [0.41,0.413] | 0.667x [0.665,0.667] | None observed |
| 8192 | fixed16 | unpinned_after_batch64 / vmrss_kib | 55.125 | 55.711 | 42.730 | 42.824 | 28.527 | 0.989x [0.988,0.989] | 0.999x [0.997,1] | 0.512x [0.509,0.513] | 0.667x [0.665,0.667] | None observed |
| 8192 | fixed16 | unpinned_before_batch1 / vmrss_kib | 5.676 | 5.676 | 38.852 | 38.945 | 4.590 | 1x [0.995,1.01] | 1x [0.997,1] | 0.809x [0.798,0.816] | 0.118x [0.116,0.118] | None observed |
| 8192 | fixed16 | unpinned_before_batch1000 / vmrss_kib | 26.277 | 26.152 | 40.176 | 40.266 | 6.191 | 1x [0.999,1.02] | 1x [0.997,1] | 0.237x [0.235,0.239] | 0.154x [0.153,0.155] | None observed |
| 8192 | fixed16 | unpinned_before_batch64 / vmrss_kib | 12.145 | 12.121 | 39.176 | 39.266 | 5.137 | 1x [0.998,1] | 1x [0.997,1] | 0.424x [0.414,0.425] | 0.131x [0.129,0.131] | None observed |
| 8192 | fixed16 | warm_hit / vmrss_kib | 28.559 | 29.164 | 42.586 | 43.309 | 9.062 | 0.979x [0.978,0.98] | 0.984x [0.982,0.987] | 0.311x [0.308,0.311] | 0.209x [0.208,0.209] | None observed |
| 8192 | fixed16 | warm_hot90 / vmrss_kib | 28.578 | 29.184 | 42.605 | 43.328 | 9.082 | 0.979x [0.978,0.98] | 0.984x [0.982,0.987] | 0.311x [0.308,0.312] | 0.21x [0.208,0.21] | None observed |
| 8192 | fixed16 | warm_miss / vmrss_kib | 28.562 | 29.168 | 42.590 | 43.312 | 9.066 | 0.979x [0.978,0.98] | 0.984x [0.983,0.987] | 0.311x [0.308,0.311] | 0.209x [0.208,0.21] | None observed |
| 8192 | fixed16 | warm_mixed50 / vmrss_kib | 28.570 | 29.176 | 42.598 | 43.320 | 9.078 | 0.979x [0.978,0.98] | 0.984x [0.982,0.987] | 0.311x [0.308,0.312] | 0.21x [0.208,0.21] | None observed |
| 8192 | hostname | after_full_validation / vmrss_kib | 33.688 | 33.605 | 41.207 | 41.848 | 7.422 | 1x [1,1.02] | 0.986x [0.983,0.987] | 0.221x [0.218,0.221] | 0.177x [0.175,0.178] | None observed |
| 8192 | hostname | after_old_lookup_drop / vmrss_kib | 64.609 | 65.219 | 45.195 | 45.246 | 38.855 | 0.991x [0.991,0.991] | 0.999x [0.998,1] | 0.596x [0.594,0.596] | 0.859x [0.857,0.861] | None observed |
| 8192 | hostname | after_old_snapshot_release / vmrss_kib | 64.609 | 65.219 | 45.195 | 45.246 | 38.855 | 0.991x [0.991,0.991] | 0.999x [0.998,1] | 0.596x [0.594,0.596] | 0.859x [0.857,0.861] | None observed |
| 8192 | hostname | after_pinned_writes / vmrss_kib | 64.609 | 65.219 | 45.195 | 45.871 | 38.141 | 0.991x [0.991,0.991] | 0.985x [0.985,0.986] | 0.585x [0.584,0.585] | 0.831x [0.83,0.833] | None observed |
| 8192 | hostname | after_prepare / vmrss_kib | 33.688 | 33.605 | 41.207 | 41.848 | 7.422 | 1x [1,1.02] | 0.986x [0.983,0.987] | 0.221x [0.218,0.221] | 0.177x [0.175,0.178] | None observed |
| 8192 | hostname | after_reopen_full_validation / vmrss_kib | 91.332 | 91.941 | 44.418 | 45.098 | 11.332 | 0.993x [0.993,0.994] | 0.985x [0.985,0.986] | 0.123x [0.122,0.124] | 0.251x [0.25,0.253] | None observed |
| 8192 | hostname | before_prepare / vmrss_kib | 33.688 | 33.605 | 41.207 | 41.105 | 7.422 | 1x [1,1.02] | 1x [1,1] | 0.221x [0.218,0.221] | 0.18x [0.178,0.181] | None observed |
| 8192 | hostname | closed_environment / vmrss_kib | 10.555 | 11.148 | 11.074 | 11.125 | 10.309 | 0.945x [0.945,0.95] | 0.996x [0.994,0.999] | 0.923x [0.921,0.926] | 0.927x [0.921,0.935] | legacy (all) |
| 8192 | hostname | fixture_ready / vmrss_kib | 3.375 | 3.398 | 3.375 | 3.379 | 3.473 | 1.02x [0.986,1.02] | 0.995x [0.993,1.02] | 1.02x [0.97,1.05] | 1.02x [0.977,1.05] | None observed |
| 8192 | hostname | loaded_source_only / vmrss_kib | 5.512 | 5.480 | 39.289 | 39.234 | 4.555 | 1x [1,1.01] | 1x [0.999,1] | 0.828x [0.812,0.832] | 0.116x [0.113,0.116] | None observed |
| 8192 | hostname | pinned_batch1 / vmrss_kib | 38.477 | 39.078 | 45.195 | 45.871 | 12.453 | 0.984x [0.984,0.986] | 0.985x [0.985,0.986] | 0.319x [0.318,0.319] | 0.271x [0.27,0.272] | None observed |
| 8192 | hostname | pinned_batch1000 / vmrss_kib | 64.609 | 65.219 | 45.195 | 45.871 | 38.141 | 0.991x [0.991,0.991] | 0.985x [0.985,0.986] | 0.585x [0.584,0.585] | 0.831x [0.83,0.833] | None observed |
| 8192 | hostname | pinned_batch64 / vmrss_kib | 44.996 | 45.586 | 45.195 | 45.871 | 18.695 | 0.987x [0.987,0.988] | 0.985x [0.985,0.986] | 0.41x [0.409,0.41] | 0.408x [0.406,0.408] | None observed |
| 8192 | hostname | process_start_before_fixture / vmrss_kib | 2.375 | 2.375 | 2.309 | 2.316 | 2.441 | 1x [0.998,1.05] | 0.997x [0.97,1] | 1.05x [0.974,1.05] | 1.05x [1,1.05] | None observed |
| 8192 | hostname | resident_ballast_ready / vmrss_kib | 3.379 | 3.402 | 3.379 | 3.383 | 3.477 | 1.02x [0.986,1.02] | 0.995x [0.993,1.02] | 1.02x [0.97,1.05] | 1.02x [0.977,1.05] | None observed |
| 8192 | hostname | two_generations / vmrss_kib | 64.609 | 65.219 | 45.195 | 45.871 | 38.141 | 0.991x [0.991,0.991] | 0.985x [0.985,0.986] | 0.585x [0.584,0.585] | 0.831x [0.83,0.833] | None observed |
| 8192 | hostname | unpinned_after_batch1 / vmrss_kib | 65.156 | 65.766 | 45.195 | 45.246 | 38.855 | 0.991x [0.991,0.992] | 0.999x [0.998,1] | 0.591x [0.59,0.591] | 0.859x [0.857,0.861] | None observed |
| 8192 | hostname | unpinned_after_batch1000 / vmrss_kib | 91.332 | 91.941 | 45.195 | 45.246 | 38.855 | 0.993x [0.993,0.994] | 0.999x [0.998,1] | 0.423x [0.422,0.423] | 0.859x [0.857,0.861] | None observed |
| 8192 | hostname | unpinned_after_batch64 / vmrss_kib | 71.727 | 72.336 | 45.195 | 45.246 | 38.855 | 0.992x [0.992,0.992] | 0.999x [0.998,1] | 0.537x [0.536,0.537] | 0.859x [0.857,0.861] | None observed |
| 8192 | hostname | unpinned_before_batch1 / vmrss_kib | 6.145 | 6.113 | 39.289 | 39.234 | 5.312 | 1x [1,1.01] | 1x [0.999,1] | 0.865x [0.851,0.87] | 0.135x [0.133,0.136] | None observed |
| 8192 | hostname | unpinned_before_batch1000 / vmrss_kib | 33.688 | 33.605 | 41.207 | 41.105 | 7.422 | 1x [1,1.02] | 1x [1,1] | 0.221x [0.218,0.221] | 0.18x [0.178,0.181] | None observed |
| 8192 | hostname | unpinned_before_batch64 / vmrss_kib | 13.215 | 13.199 | 39.613 | 39.559 | 5.906 | 1x [1,1.01] | 1x [0.999,1] | 0.447x [0.444,0.449] | 0.149x [0.148,0.15] | None observed |
| 8192 | hostname | warm_hit / vmrss_kib | 37.781 | 38.379 | 45.043 | 45.719 | 12.234 | 0.984x [0.984,0.985] | 0.985x [0.985,0.986] | 0.319x [0.318,0.319] | 0.268x [0.267,0.268] | None observed |
| 8192 | hostname | warm_hot90 / vmrss_kib | 37.797 | 38.398 | 45.059 | 45.738 | 12.254 | 0.984x [0.984,0.985] | 0.985x [0.985,0.986] | 0.319x [0.318,0.32] | 0.268x [0.267,0.269] | None observed |
| 8192 | hostname | warm_miss / vmrss_kib | 37.781 | 38.383 | 45.043 | 45.723 | 12.238 | 0.984x [0.984,0.985] | 0.985x [0.985,0.986] | 0.319x [0.318,0.319] | 0.268x [0.267,0.268] | None observed |
| 8192 | hostname | warm_mixed50 / vmrss_kib | 37.789 | 38.391 | 45.051 | 45.730 | 12.250 | 0.984x [0.984,0.985] | 0.985x [0.985,0.986] | 0.319x [0.318,0.319] | 0.268x [0.267,0.269] | None observed |
| 100000 | fixed16 | after_full_validation / vmrss_kib | 114.660 | 124.680 | 65.523 | 75.520 | 30.309 | 0.92x [0.92,0.94] | 0.868x [0.866,0.868] | 0.243x [0.243,0.249] | 0.401x [0.401,0.401] | legacy (all), resident (all) |
| 100000 | fixed16 | after_old_lookup_drop / vmrss_kib | 204.418 | 204.441 | 67.020 | 67.016 | 119.176 | 1x [1,1] | 1x [0.998,1] | 0.583x [0.583,0.585] | 1.78x [1.78,1.78] | None observed |
| 100000 | fixed16 | after_old_snapshot_release / vmrss_kib | 204.418 | 204.441 | 67.020 | 67.016 | 119.176 | 1x [1,1] | 1x [0.998,1] | 0.583x [0.583,0.585] | 1.78x [1.78,1.78] | None observed |
| 100000 | fixed16 | after_pinned_writes / vmrss_kib | 204.418 | 214.441 | 67.020 | 77.016 | 116.320 | 0.953x [0.953,0.956] | 0.87x [0.869,0.87] | 0.542x [0.542,0.544] | 1.51x [1.51,1.51] | resident (all) |
| 100000 | fixed16 | after_prepare / vmrss_kib | 114.660 | 124.680 | 65.523 | 75.520 | 30.309 | 0.92x [0.92,0.94] | 0.868x [0.866,0.868] | 0.243x [0.243,0.249] | 0.401x [0.401,0.401] | legacy (all), resident (all) |
| 100000 | fixed16 | after_reopen_full_validation / vmrss_kib | 294.504 | 301.527 | 66.102 | 79.691 | 40.113 | 0.977x [0.977,0.979] | 0.83x [0.828,0.83] | 0.133x [0.132,0.135] | 0.503x [0.497,0.51] | resident (all) |
| 100000 | fixed16 | before_prepare / vmrss_kib | 114.660 | 114.680 | 65.461 | 65.500 | 30.309 | 1x [1,1.02] | 0.999x [0.999,1] | 0.264x [0.264,0.271] | 0.463x [0.462,0.463] | None observed |
| 100000 | fixed16 | closed_environment / vmrss_kib | 19.609 | 26.555 | 17.562 | 26.082 | 20.738 | 0.737x [0.737,0.756] | 0.671x [0.67,0.675] | 0.781x [0.78,0.798] | 0.795x [0.792,0.797] | legacy (all), resident (all) |
| 100000 | fixed16 | fixture_ready / vmrss_kib | 10.754 | 10.836 | 10.832 | 10.816 | 10.758 | 0.992x [0.992,0.994] | 1x [1,1.01] | 0.994x [0.993,0.999] | 0.995x [0.992,1.01] | None observed |
| 100000 | fixed16 | loaded_source_only / vmrss_kib | 17.902 | 17.906 | 62.141 | 62.188 | 16.953 | 1x [0.999,1] | 0.999x [0.999,1] | 0.947x [0.946,0.95] | 0.273x [0.273,0.273] | None observed |
| 100000 | fixed16 | pinned_batch1 / vmrss_kib | 115.207 | 125.227 | 66.895 | 76.891 | 32.418 | 0.92x [0.92,0.94] | 0.87x [0.868,0.87] | 0.259x [0.259,0.265] | 0.422x [0.422,0.422] | legacy (all), resident (all) |
| 100000 | fixed16 | pinned_batch1000 / vmrss_kib | 204.418 | 214.441 | 67.020 | 77.016 | 116.320 | 0.953x [0.953,0.956] | 0.87x [0.869,0.87] | 0.542x [0.542,0.544] | 1.51x [1.51,1.51] | resident (all) |
| 100000 | fixed16 | pinned_batch64 / vmrss_kib | 123.949 | 133.969 | 66.895 | 76.891 | 37.957 | 0.925x [0.925,0.944] | 0.87x [0.868,0.87] | 0.283x [0.283,0.289] | 0.494x [0.493,0.494] | legacy (all), resident (all) |
| 100000 | fixed16 | process_start_before_fixture / vmrss_kib | 2.312 | 2.316 | 2.316 | 2.316 | 2.312 | 0.998x [0.997,1] | 1.01x [0.998,1.03] | 0.998x [0.998,1] | 0.998x [0.998,1] | None observed |
| 100000 | fixed16 | resident_ballast_ready / vmrss_kib | 10.758 | 10.840 | 10.836 | 10.820 | 10.762 | 0.992x [0.992,0.994] | 1x [1,1.01] | 0.994x [0.993,0.999] | 0.995x [0.992,1.01] | None observed |
| 100000 | fixed16 | two_generations / vmrss_kib | 204.418 | 224.445 | 67.020 | 87.016 | 116.320 | 0.911x [0.911,0.913] | 0.77x [0.769,0.77] | 0.518x [0.518,0.52] | 1.34x [1.34,1.34] | legacy (all), resident (all) |
| 100000 | fixed16 | unpinned_after_batch1 / vmrss_kib | 204.965 | 204.988 | 67.020 | 67.016 | 119.176 | 1x [1,1] | 1x [0.998,1] | 0.581x [0.581,0.584] | 1.78x [1.78,1.78] | None observed |
| 100000 | fixed16 | unpinned_after_batch1000 / vmrss_kib | 294.504 | 294.484 | 67.020 | 67.016 | 119.176 | 1x [1,1] | 1x [0.998,1] | 0.405x [0.404,0.406] | 1.78x [1.78,1.78] | None observed |
| 100000 | fixed16 | unpinned_after_batch64 / vmrss_kib | 213.684 | 213.707 | 67.020 | 67.016 | 119.176 | 1x [1,1] | 1x [0.998,1] | 0.557x [0.557,0.56] | 1.78x [1.78,1.78] | None observed |
| 100000 | fixed16 | unpinned_before_batch1 / vmrss_kib | 18.539 | 18.562 | 62.160 | 62.207 | 19.105 | 0.999x [0.998,1] | 0.999x [0.999,1] | 1.03x [1.03,1.03] | 0.307x [0.306,0.308] | None observed |
| 100000 | fixed16 | unpinned_before_batch1000 / vmrss_kib | 114.660 | 114.680 | 65.461 | 65.500 | 30.309 | 1x [1,1.02] | 0.999x [0.999,1] | 0.264x [0.264,0.271] | 0.463x [0.462,0.463] | None observed |
| 100000 | fixed16 | unpinned_before_batch64 / vmrss_kib | 28.062 | 28.094 | 62.629 | 62.676 | 23.492 | 0.999x [0.998,1] | 0.999x [0.999,1] | 0.836x [0.836,0.837] | 0.375x [0.375,0.375] | None observed |
| 100000 | fixed16 | warm_hit / vmrss_kib | 114.660 | 124.680 | 67.027 | 77.023 | 32.223 | 0.92x [0.92,0.94] | 0.87x [0.869,0.87] | 0.258x [0.258,0.264] | 0.418x [0.418,0.418] | legacy (all), resident (all) |
| 100000 | fixed16 | warm_hot90 / vmrss_kib | 114.660 | 124.680 | 66.895 | 76.891 | 32.230 | 0.92x [0.92,0.94] | 0.87x [0.868,0.87] | 0.259x [0.258,0.264] | 0.419x [0.419,0.419] | legacy (all), resident (all) |
| 100000 | fixed16 | warm_miss / vmrss_kib | 114.660 | 124.680 | 66.895 | 76.891 | 32.215 | 0.92x [0.92,0.94] | 0.87x [0.868,0.87] | 0.258x [0.258,0.264] | 0.419x [0.419,0.419] | legacy (all), resident (all) |
| 100000 | fixed16 | warm_mixed50 / vmrss_kib | 114.660 | 124.680 | 66.895 | 76.891 | 32.223 | 0.92x [0.92,0.94] | 0.87x [0.868,0.87] | 0.258x [0.258,0.264] | 0.419x [0.419,0.419] | legacy (all), resident (all) |
| 100000 | hostname | after_full_validation / vmrss_kib | 133.574 | 137.555 | 68.633 | 78.723 | 39.578 | 0.971x [0.953,0.972] | 0.872x [0.872,0.872] | 0.288x [0.282,0.288] | 0.503x [0.502,0.503] | resident (all) |
| 100000 | hostname | after_old_lookup_drop / vmrss_kib | 237.227 | 253.828 | 71.523 | 71.617 | 141.965 | 0.934x [0.934,0.935] | 0.999x [0.999,0.999] | 0.559x [0.559,0.56] | 1.98x [1.98,1.99] | legacy (all) |
| 100000 | hostname | after_old_snapshot_release / vmrss_kib | 237.227 | 253.828 | 71.523 | 71.617 | 141.965 | 0.934x [0.934,0.935] | 0.999x [0.999,0.999] | 0.559x [0.559,0.56] | 1.98x [1.98,1.99] | legacy (all) |
| 100000 | hostname | after_pinned_writes / vmrss_kib | 237.227 | 247.176 | 71.523 | 81.617 | 138.750 | 0.96x [0.959,0.96] | 0.876x [0.876,0.877] | 0.561x [0.561,0.561] | 1.7x [1.7,1.7] | resident (all) |
| 100000 | hostname | after_prepare / vmrss_kib | 133.574 | 137.555 | 68.633 | 78.723 | 39.578 | 0.971x [0.953,0.972] | 0.872x [0.872,0.872] | 0.288x [0.282,0.288] | 0.503x [0.502,0.503] | resident (all) |
| 100000 | hostname | after_reopen_full_validation / vmrss_kib | 339.684 | 356.285 | 70.254 | 83.070 | 60.750 | 0.953x [0.953,0.953] | 0.846x [0.846,0.846] | 0.171x [0.168,0.171] | 0.731x [0.722,0.732] | resident (all) |
| 100000 | hostname | before_prepare / vmrss_kib | 133.574 | 133.523 | 68.633 | 68.723 | 39.578 | 1x [1,1] | 0.999x [0.999,1] | 0.296x [0.296,0.296] | 0.576x [0.575,0.576] | None observed |
| 100000 | hostname | closed_environment / vmrss_kib | 24.398 | 41.043 | 21.930 | 32.023 | 28.531 | 0.594x [0.594,0.595] | 0.685x [0.685,0.686] | 0.696x [0.694,0.696] | 0.891x [0.89,0.892] | legacy (all), resident (all) |
| 100000 | hostname | fixture_ready / vmrss_kib | 13.887 | 13.871 | 13.895 | 13.801 | 13.887 | 1x [0.999,1] | 1.01x [1.01,1.01] | 1x [0.994,1] | 1.01x [1,1.01] | None observed |
| 100000 | hostname | loaded_source_only / vmrss_kib | 23.754 | 23.789 | 65.449 | 65.539 | 22.773 | 0.999x [0.998,1] | 0.999x [0.998,0.999] | 0.958x [0.957,0.958] | 0.347x [0.347,0.348] | None observed |
| 100000 | hostname | pinned_batch1 / vmrss_kib | 134.121 | 142.121 | 71.520 | 81.617 | 43.684 | 0.944x [0.943,0.944] | 0.876x [0.876,0.877] | 0.307x [0.307,0.308] | 0.535x [0.535,0.536] | legacy (all), resident (all) |
| 100000 | hostname | pinned_batch1000 / vmrss_kib | 237.227 | 247.176 | 71.523 | 81.617 | 138.750 | 0.96x [0.959,0.96] | 0.876x [0.876,0.877] | 0.561x [0.561,0.561] | 1.7x [1.7,1.7] | resident (all) |
| 100000 | hostname | pinned_batch64 / vmrss_kib | 144.555 | 152.555 | 71.520 | 81.617 | 50.672 | 0.947x [0.947,0.948] | 0.876x [0.876,0.877] | 0.332x [0.332,0.332] | 0.621x [0.621,0.621] | legacy (all), resident (all) |
| 100000 | hostname | process_start_before_fixture / vmrss_kib | 2.316 | 2.309 | 2.375 | 2.316 | 2.316 | 1x [0.975,1.03] | 1.03x [1,1.03] | 1x [0.974,1.03] | 1x [0.998,1.03] | None observed |
| 100000 | hostname | resident_ballast_ready / vmrss_kib | 13.891 | 13.875 | 13.898 | 13.805 | 13.891 | 1x [0.999,1] | 1.01x [1.01,1.01] | 1x [0.994,1] | 1.01x [1,1.01] | None observed |
| 100000 | hostname | two_generations / vmrss_kib | 237.227 | 253.828 | 71.523 | 91.617 | 138.750 | 0.934x [0.934,0.935] | 0.781x [0.781,0.781] | 0.547x [0.547,0.547] | 1.52x [1.51,1.52] | legacy (all), resident (all) |
| 100000 | hostname | unpinned_after_batch1 / vmrss_kib | 237.773 | 254.375 | 71.523 | 71.617 | 141.977 | 0.935x [0.935,0.935] | 0.999x [0.999,0.999] | 0.558x [0.558,0.558] | 1.98x [1.98,1.99] | legacy (all) |
| 100000 | hostname | unpinned_after_batch1000 / vmrss_kib | 339.684 | 356.285 | 71.523 | 71.617 | 141.977 | 0.953x [0.953,0.953] | 0.999x [0.999,0.999] | 0.399x [0.398,0.399] | 1.98x [1.98,1.99] | None observed |
| 100000 | hostname | unpinned_after_batch64 / vmrss_kib | 248.184 | 264.785 | 71.523 | 71.617 | 141.977 | 0.937x [0.937,0.937] | 0.999x [0.999,0.999] | 0.536x [0.536,0.537] | 1.98x [1.98,1.99] | legacy (all) |
| 100000 | hostname | unpinned_before_batch1 / vmrss_kib | 24.418 | 24.402 | 65.449 | 65.539 | 25.438 | 1x [0.999,1] | 0.999x [0.999,0.999] | 1.04x [1.04,1.05] | 0.388x [0.386,0.39] | None observed |
| 100000 | hostname | unpinned_before_batch1000 / vmrss_kib | 133.574 | 133.523 | 68.629 | 68.723 | 39.578 | 1x [1,1] | 0.999x [0.999,0.999] | 0.296x [0.296,0.296] | 0.576x [0.575,0.576] | None observed |
| 100000 | hostname | unpinned_before_batch64 / vmrss_kib | 35.738 | 35.715 | 65.973 | 66.066 | 32.152 | 1x [0.999,1] | 0.999x [0.999,0.999] | 0.901x [0.899,0.901] | 0.487x [0.486,0.487] | None observed |
| 100000 | hostname | warm_hit / vmrss_kib | 133.574 | 141.426 | 71.387 | 81.488 | 43.473 | 0.944x [0.944,0.944] | 0.876x [0.876,0.877] | 0.307x [0.307,0.307] | 0.533x [0.533,0.534] | legacy (all), resident (all) |
| 100000 | hostname | warm_hot90 / vmrss_kib | 133.574 | 141.449 | 71.402 | 81.500 | 43.492 | 0.944x [0.944,0.944] | 0.876x [0.876,0.877] | 0.307x [0.307,0.308] | 0.534x [0.533,0.534] | legacy (all), resident (all) |
| 100000 | hostname | warm_miss / vmrss_kib | 133.574 | 141.434 | 71.391 | 81.488 | 43.477 | 0.944x [0.944,0.944] | 0.876x [0.876,0.877] | 0.307x [0.307,0.308] | 0.534x [0.533,0.534] | legacy (all), resident (all) |
| 100000 | hostname | warm_mixed50 / vmrss_kib | 133.574 | 141.441 | 71.395 | 81.496 | 43.484 | 0.944x [0.944,0.944] | 0.876x [0.876,0.877] | 0.307x [0.307,0.308] | 0.534x [0.533,0.534] | legacy (all), resident (all) |
| 1000000 | fixed16 | after_full_validation / vmrss_kib | 288.906 | 368.887 | 296.301 | 376.316 | 211.559 | 0.783x [0.783,0.783] | 0.788x [0.787,0.788] | 0.574x [0.573,0.574] | 0.562x [0.562,0.562] | legacy (all), resident (all) |
| 1000000 | fixed16 | after_old_lookup_drop / vmrss_kib | 425.426 | 425.449 | 297.801 | 297.754 | 339.125 | 1x [1,1] | 1x [0.999,1] | 0.797x [0.797,0.797] | 1.14x [1.14,1.14] | None observed |
| 1000000 | fixed16 | after_old_snapshot_release / vmrss_kib | 425.426 | 425.449 | 297.801 | 297.754 | 339.125 | 1x [1,1] | 1x [0.999,1] | 0.797x [0.797,0.797] | 1.14x [1.14,1.14] | None observed |
| 1000000 | fixed16 | after_pinned_writes / vmrss_kib | 425.426 | 505.449 | 297.801 | 377.754 | 333.875 | 0.842x [0.842,0.842] | 0.788x [0.788,0.788] | 0.66x [0.66,0.661] | 0.884x [0.883,0.884] | legacy (all), resident (all) |
| 1000000 | fixed16 | after_prepare / vmrss_kib | 288.906 | 368.887 | 296.301 | 376.316 | 211.559 | 0.783x [0.783,0.783] | 0.788x [0.787,0.788] | 0.574x [0.573,0.574] | 0.562x [0.562,0.562] | legacy (all), resident (all) |
| 1000000 | fixed16 | after_reopen_full_validation / vmrss_kib | 560.082 | 640.062 | 295.848 | 375.879 | 336.117 | 0.875x [0.875,0.875] | 0.787x [0.787,0.787] | 0.525x [0.525,0.525] | 0.894x [0.894,0.894] | legacy (all), resident (all) |
| 1000000 | fixed16 | before_prepare / vmrss_kib | 288.906 | 288.887 | 296.301 | 296.316 | 211.559 | 1x [1,1] | 1x [0.999,1] | 0.732x [0.732,0.733] | 0.714x [0.714,0.714] | None observed |
| 1000000 | fixed16 | closed_environment / vmrss_kib | 99.871 | 99.867 | 98.672 | 98.629 | 147.773 | 1x [1,1] | 1x [0.998,1] | 1.48x [1.48,1.48] | 1.5x [1.5,1.5] | None observed |
| 1000000 | fixed16 | fixture_ready / vmrss_kib | 86.285 | 86.379 | 86.332 | 86.285 | 86.316 | 0.999x [0.999,1] | 1x [0.999,1] | 0.999x [0.999,1] | 1x [0.999,1] | None observed |
| 1000000 | fixed16 | loaded_source_only / vmrss_kib | 144.172 | 144.152 | 290.211 | 290.172 | 144.234 | 1x [1,1] | 1x [0.999,1] | 1x [1,1] | 0.497x [0.497,0.497] | None observed |
| 1000000 | fixed16 | pinned_batch1 / vmrss_kib | 289.562 | 369.543 | 297.801 | 377.754 | 213.457 | 0.784x [0.783,0.784] | 0.788x [0.788,0.788] | 0.578x [0.577,0.578] | 0.565x [0.565,0.565] | legacy (all), resident (all) |
| 1000000 | fixed16 | pinned_batch1000 / vmrss_kib | 425.426 | 505.449 | 297.801 | 377.754 | 333.875 | 0.842x [0.842,0.842] | 0.788x [0.788,0.788] | 0.66x [0.66,0.661] | 0.884x [0.883,0.884] | legacy (all), resident (all) |
| 1000000 | fixed16 | pinned_batch64 / vmrss_kib | 302.707 | 382.699 | 297.801 | 377.754 | 222.012 | 0.791x [0.791,0.791] | 0.788x [0.788,0.788] | 0.58x [0.58,0.58] | 0.588x [0.588,0.588] | legacy (all), resident (all) |
| 1000000 | fixed16 | process_start_before_fixture / vmrss_kib | 2.312 | 2.375 | 2.367 | 2.312 | 2.359 | 0.974x [0.972,1.03] | 1.02x [0.998,1.03] | 1x [0.972,1.02] | 1.02x [0.998,1.03] | None observed |
| 1000000 | fixed16 | resident_ballast_ready / vmrss_kib | 86.289 | 86.383 | 86.336 | 86.289 | 86.320 | 0.999x [0.999,1] | 1x [0.999,1] | 0.999x [0.999,1] | 1x [0.999,1] | None observed |
| 1000000 | fixed16 | two_generations / vmrss_kib | 425.426 | 585.449 | 297.801 | 457.758 | 333.875 | 0.727x [0.727,0.727] | 0.651x [0.65,0.651] | 0.57x [0.57,0.571] | 0.729x [0.729,0.73] | legacy (all), resident (all) |
| 1000000 | fixed16 | unpinned_after_batch1 / vmrss_kib | 426.082 | 426.105 | 297.801 | 297.754 | 339.125 | 1x [1,1] | 1x [0.999,1] | 0.796x [0.796,0.796] | 1.14x [1.14,1.14] | None observed |
| 1000000 | fixed16 | unpinned_after_batch1000 / vmrss_kib | 560.082 | 560.062 | 297.801 | 297.762 | 339.125 | 1x [1,1] | 1x [0.999,1] | 0.605x [0.605,0.606] | 1.14x [1.14,1.14] | None observed |
| 1000000 | fixed16 | unpinned_after_batch64 / vmrss_kib | 439.234 | 439.258 | 297.801 | 297.754 | 339.125 | 1x [1,1] | 1x [0.999,1] | 0.772x [0.772,0.772] | 1.14x [1.14,1.14] | None observed |
| 1000000 | fixed16 | unpinned_before_batch1 / vmrss_kib | 144.891 | 144.875 | 290.234 | 290.195 | 147.742 | 1x [1,1] | 1x [0.999,1] | 1.02x [1.02,1.02] | 0.509x [0.508,0.509] | None observed |
| 1000000 | fixed16 | unpinned_before_batch1000 / vmrss_kib | 288.906 | 288.887 | 296.301 | 296.254 | 211.559 | 1x [1,1] | 1x [0.999,1] | 0.732x [0.732,0.733] | 0.714x [0.714,0.714] | None observed |
| 1000000 | fixed16 | unpinned_before_batch64 / vmrss_kib | 158.270 | 158.570 | 290.871 | 290.832 | 195.402 | 1x [0.998,1] | 1x [0.999,1] | 1.23x [1.23,1.24] | 0.672x [0.671,0.674] | None observed |
| 1000000 | fixed16 | warm_hit / vmrss_kib | 288.906 | 368.887 | 297.805 | 377.820 | 213.266 | 0.783x [0.783,0.783] | 0.788x [0.788,0.788] | 0.578x [0.578,0.578] | 0.564x [0.564,0.565] | legacy (all), resident (all) |
| 1000000 | fixed16 | warm_hot90 / vmrss_kib | 288.906 | 368.887 | 297.801 | 377.754 | 213.273 | 0.783x [0.783,0.783] | 0.788x [0.788,0.788] | 0.578x [0.578,0.578] | 0.565x [0.564,0.565] | legacy (all), resident (all) |
| 1000000 | fixed16 | warm_miss / vmrss_kib | 288.906 | 368.887 | 297.801 | 377.754 | 213.254 | 0.783x [0.783,0.783] | 0.788x [0.788,0.788] | 0.578x [0.578,0.578] | 0.565x [0.564,0.565] | legacy (all), resident (all) |
| 1000000 | fixed16 | warm_mixed50 / vmrss_kib | 288.906 | 368.887 | 297.801 | 377.754 | 213.266 | 0.783x [0.783,0.783] | 0.788x [0.788,0.788] | 0.578x [0.578,0.578] | 0.565x [0.564,0.565] | legacy (all), resident (all) |
| 1000000 | hostname | after_full_validation / vmrss_kib | 372.258 | 451.098 | 328.023 | 408.027 | 298.426 | 0.825x [0.824,0.827] | 0.804x [0.803,0.804] | 0.662x [0.661,0.662] | 0.731x [0.731,0.731] | legacy (all), resident (all) |
| 1000000 | hostname | after_old_lookup_drop / vmrss_kib | 532.328 | 531.395 | 329.488 | 329.488 | 449.828 | 1x [0.997,1] | 1x [0.999,1] | 0.846x [0.846,0.847] | 1.37x [1.37,1.37] | None observed |
| 1000000 | hostname | after_old_snapshot_release / vmrss_kib | 532.328 | 531.395 | 329.488 | 329.488 | 449.828 | 1x [0.997,1] | 1x [0.999,1] | 0.846x [0.846,0.847] | 1.37x [1.37,1.37] | None observed |
| 1000000 | hostname | after_pinned_writes / vmrss_kib | 532.328 | 611.398 | 329.488 | 409.492 | 444.227 | 0.871x [0.867,0.871] | 0.805x [0.804,0.805] | 0.726x [0.726,0.727] | 1.08x [1.08,1.09] | legacy (all), resident (all) |
| 1000000 | hostname | after_prepare / vmrss_kib | 372.258 | 451.098 | 328.023 | 408.027 | 298.426 | 0.825x [0.824,0.827] | 0.804x [0.803,0.804] | 0.662x [0.661,0.662] | 0.731x [0.731,0.731] | legacy (all), resident (all) |
| 1000000 | hostname | after_reopen_full_validation / vmrss_kib | 689.906 | 768.969 | 327.008 | 406.938 | 448.461 | 0.897x [0.894,0.898] | 0.804x [0.803,0.804] | 0.583x [0.583,0.583] | 1.1x [1.1,1.1] | legacy (all), resident (all) |
| 1000000 | hostname | before_prepare / vmrss_kib | 372.258 | 371.094 | 328.023 | 328.023 | 298.426 | 1x [1,1] | 1x [0.999,1] | 0.804x [0.804,0.804] | 0.91x [0.91,0.91] | None observed |
| 1000000 | hostname | closed_environment / vmrss_kib | 132.797 | 131.859 | 130.488 | 130.488 | 207.668 | 1.01x [0.99,1.01] | 1x [0.999,1] | 1.57x [1.57,1.58] | 1.59x [1.59,1.59] | None observed |
| 1000000 | hostname | fixture_ready / vmrss_kib | 116.883 | 116.883 | 116.867 | 116.844 | 116.895 | 1x [1,1] | 1x [0.999,1] | 1x [0.999,1] | 1x [1,1] | None observed |
| 1000000 | hostname | loaded_source_only / vmrss_kib | 202.410 | 202.414 | 322.188 | 322.191 | 202.297 | 1x [1,1] | 1x [0.999,1] | 0.999x [0.999,1] | 0.628x [0.628,0.628] | None observed |
| 1000000 | hostname | pinned_batch1 / vmrss_kib | 373.727 | 454.305 | 329.246 | 409.250 | 302.316 | 0.822x [0.821,0.826] | 0.805x [0.804,0.805] | 0.665x [0.665,0.666] | 0.739x [0.739,0.739] | legacy (all), resident (all) |
| 1000000 | hostname | pinned_batch1000 / vmrss_kib | 532.328 | 611.398 | 329.488 | 409.492 | 444.227 | 0.871x [0.867,0.871] | 0.805x [0.804,0.805] | 0.726x [0.726,0.727] | 1.08x [1.08,1.09] | legacy (all), resident (all) |
| 1000000 | hostname | pinned_batch64 / vmrss_kib | 388.105 | 468.707 | 329.246 | 409.250 | 311.664 | 0.828x [0.826,0.831] | 0.805x [0.804,0.805] | 0.665x [0.665,0.665] | 0.762x [0.761,0.762] | legacy (all), resident (all) |
| 1000000 | hostname | process_start_before_fixture / vmrss_kib | 2.316 | 2.316 | 2.316 | 2.367 | 2.391 | 1x [0.998,1] | 0.995x [0.974,1.03] | 1.03x [1,1.05] | 1x [1,1.03] | None observed |
| 1000000 | hostname | resident_ballast_ready / vmrss_kib | 116.887 | 116.887 | 116.871 | 116.848 | 116.898 | 1x [1,1] | 1x [0.999,1] | 1x [0.999,1] | 1x [1,1] | None observed |
| 1000000 | hostname | two_generations / vmrss_kib | 532.328 | 691.398 | 329.488 | 489.496 | 444.227 | 0.77x [0.767,0.77] | 0.673x [0.673,0.673] | 0.642x [0.642,0.643] | 0.907x [0.907,0.908] | legacy (all), resident (all) |
| 1000000 | hostname | unpinned_after_batch1 / vmrss_kib | 532.984 | 532.051 | 329.488 | 329.488 | 449.828 | 1x [0.997,1] | 1x [0.999,1] | 0.845x [0.845,0.846] | 1.37x [1.37,1.37] | None observed |
| 1000000 | hostname | unpinned_after_batch1000 / vmrss_kib | 689.906 | 688.969 | 329.488 | 329.488 | 449.832 | 1x [0.998,1] | 1x [0.999,1] | 0.653x [0.652,0.653] | 1.37x [1.37,1.37] | None observed |
| 1000000 | hostname | unpinned_after_batch64 / vmrss_kib | 547.301 | 546.391 | 329.488 | 329.488 | 449.828 | 1x [0.998,1] | 1x [0.999,1] | 0.823x [0.823,0.823] | 1.37x [1.37,1.37] | None observed |
| 1000000 | hostname | unpinned_before_batch1 / vmrss_kib | 203.133 | 203.141 | 322.215 | 322.219 | 205.957 | 1x [1,1] | 1x [0.999,1] | 1.01x [1.01,1.01] | 0.639x [0.639,0.639] | None observed |
| 1000000 | hostname | unpinned_before_batch1000 / vmrss_kib | 372.258 | 371.094 | 328.023 | 328.023 | 298.426 | 1x [1,1] | 1x [0.999,1] | 0.804x [0.804,0.804] | 0.91x [0.91,0.91] | None observed |
| 1000000 | hostname | unpinned_before_batch64 / vmrss_kib | 218.664 | 218.125 | 322.965 | 322.973 | 270.516 | 1x [0.998,1] | 1x [0.999,1] | 1.24x [1.24,1.24] | 0.838x [0.837,0.84] | None observed |
| 1000000 | hostname | warm_hit / vmrss_kib | 372.258 | 451.098 | 329.527 | 409.531 | 302.105 | 0.825x [0.824,0.827] | 0.805x [0.804,0.805] | 0.67x [0.669,0.67] | 0.738x [0.738,0.738] | legacy (all), resident (all) |
| 1000000 | hostname | warm_hot90 / vmrss_kib | 373.070 | 453.648 | 329.246 | 409.250 | 302.125 | 0.822x [0.821,0.826] | 0.805x [0.804,0.805] | 0.666x [0.666,0.666] | 0.738x [0.738,0.738] | legacy (all), resident (all) |
| 1000000 | hostname | warm_miss / vmrss_kib | 373.070 | 453.648 | 329.246 | 409.250 | 302.109 | 0.822x [0.821,0.826] | 0.805x [0.804,0.805] | 0.666x [0.666,0.666] | 0.738x [0.738,0.738] | legacy (all), resident (all) |
| 1000000 | hostname | warm_mixed50 / vmrss_kib | 373.070 | 453.648 | 329.246 | 409.250 | 302.121 | 0.822x [0.821,0.826] | 0.805x [0.804,0.805] | 0.666x [0.666,0.666] | 0.738x [0.738,0.738] | legacy (all), resident (all) |

## Exact index slots and file space

MiB. Index values are actual owned slot-vector capacity bytes, rather than the previous prototype's HashMap estimate. Keys/values remain borrowed; source mappings, allocator bookkeeping and all retained snapshot pages are excluded from this slot count. File sizes include the complete retained engine directory after reopening validation.

| Keys | Key type | Phase / operation | Legacy native | Legacy hash | Resident native | Resident hash | LMDB | Legacy hash/native | Resident hash/native | Legacy hash/LMDB | Resident hash/LMDB | >5% hash cost increase vs native |
|---:|---|---|---:|---:|---:|---:|---:|---|---|---|---|---|
| 8192 | fixed16 | initial_preparation / owned_slot_bytes | 0.000 | 0.625 | 0.000 | 0.625 | 0.000 | N/A | N/A | N/A | N/A | legacy (all), resident (all) |
| 8192 | hostname | initial_preparation / owned_slot_bytes | 0.000 | 0.625 | 0.000 | 0.625 | 0.000 | N/A | N/A | N/A | N/A | legacy (all), resident (all) |
| 100000 | fixed16 | initial_preparation / owned_slot_bytes | 0.000 | 10.000 | 0.000 | 10.000 | 0.000 | N/A | N/A | N/A | N/A | legacy (all), resident (all) |
| 100000 | hostname | initial_preparation / owned_slot_bytes | 0.000 | 10.000 | 0.000 | 10.000 | 0.000 | N/A | N/A | N/A | N/A | legacy (all), resident (all) |
| 1000000 | fixed16 | initial_preparation / owned_slot_bytes | 0.000 | 80.000 | 0.000 | 80.000 | 0.000 | N/A | N/A | N/A | N/A | legacy (all), resident (all) |
| 1000000 | hostname | initial_preparation / owned_slot_bytes | 0.000 | 80.000 | 0.000 | 80.000 | 0.000 | N/A | N/A | N/A | N/A | legacy (all), resident (all) |
| 8192 | fixed16 | after_reopen_validation / allocated_bytes | 62.781 | 62.781 | 256.000 | 256.000 | 20.699 | 1x [1,1] | 1x [1,1] | 0.33x [0.328,0.33] | 0.0809x [0.0808,0.0809] | None observed |
| 8192 | fixed16 | after_reopen_validation / apparent_bytes | 62.289 | 62.289 | 256.000 | 256.000 | 20.703 | 1x [1,1] | 1x [1,1] | 0.332x [0.331,0.333] | 0.0809x [0.0808,0.0809] | None observed |
| 8192 | hostname | after_reopen_validation / allocated_bytes | 83.297 | 83.297 | 256.000 | 256.000 | 28.535 | 1x [1,1] | 1x [1,1] | 0.343x [0.342,0.343] | 0.111x [0.111,0.111] | None observed |
| 8192 | hostname | after_reopen_validation / apparent_bytes | 82.805 | 82.805 | 256.000 | 256.000 | 28.539 | 1x [1,1] | 1x [1,1] | 0.345x [0.344,0.345] | 0.111x [0.111,0.111] | None observed |
| 100000 | fixed16 | after_reopen_validation / allocated_bytes | 277.395 | 277.395 | 256.000 | 256.000 | 98.398 | 1x [1,1] | 1x [1,1] | 0.355x [0.354,0.355] | 0.384x [0.384,0.385] | None observed |
| 100000 | fixed16 | after_reopen_validation / apparent_bytes | 276.902 | 276.902 | 256.000 | 256.000 | 98.402 | 1x [1,1] | 1x [1,1] | 0.355x [0.355,0.356] | 0.384x [0.384,0.385] | None observed |
| 100000 | hostname | after_reopen_validation / allocated_bytes | 317.773 | 317.773 | 256.000 | 256.000 | 113.391 | 1x [1,1] | 1x [1,1] | 0.357x [0.357,0.357] | 0.443x [0.443,0.444] | None observed |
| 100000 | hostname | after_reopen_validation / apparent_bytes | 317.281 | 317.281 | 256.000 | 256.000 | 113.395 | 1x [1,1] | 1x [1,1] | 0.358x [0.357,0.358] | 0.443x [0.443,0.444] | None observed |
| 1000000 | fixed16 | after_reopen_validation / allocated_bytes | 462.695 | 462.695 | 256.000 | 256.000 | 191.094 | 1x [1,1] | 1x [1,1] | 0.413x [0.413,0.413] | 0.746x [0.746,0.747] | None observed |
| 1000000 | fixed16 | after_reopen_validation / apparent_bytes | 462.203 | 462.203 | 256.000 | 256.000 | 191.098 | 1x [1,1] | 1x [1,1] | 0.413x [0.413,0.413] | 0.746x [0.746,0.747] | None observed |
| 1000000 | hostname | after_reopen_validation / allocated_bytes | 559.609 | 559.609 | 256.000 | 256.000 | 241.695 | 1x [1,1] | 1x [1,1] | 0.432x [0.432,0.432] | 0.944x [0.944,0.945] | None observed |
| 1000000 | hostname | after_reopen_validation / apparent_bytes | 559.117 | 559.117 | 256.000 | 256.000 | 241.699 | 1x [1,1] | 1x [1,1] | 0.432x [0.432,0.432] | 0.944x [0.944,0.945] | None observed |

## Adverse observations are retained

494 rows have at least one >5% hash cost increase against the matching native arm, across all timing/memory/storage counters. Each appears in the complete CSV and typed summary, including cases that improve at other sizes. Read/write/lifecycle rows and main RSS rows are shown above; secondary memory counters are in the CSV. No universal no-regression claim follows from this campaign.

## Scope and remaining qualification

This is a synthetic bytewise fixed16/hostname KV benchmark with32-byte values and one read thread. It does not benchmark custom comparators, duplicate/prefix lookups, large values, controlled memory pressure, larger-than-RAM data, cold caches, concurrent request throughput, power-loss recovery or every production IPDon journey. Ballast, if configured, is a touched process allocation; it does not establish OS memory pressure. The production API deliberately rejects unsupported hash sources and preserves native ordered fallback. A separate frozen baseline/candidate full engine suite is required to assess unchanged/default paths.

## Evidence and reproduction

results.json retains every full raw report, exact command, source hashes, executable hash, LMDB build information and host observations. The raw directory retains successful and failed attempts; failures are never silently replaced. summary.json has typed rows and paired process values. FULL_TABLE.csv includes every metric and adverse flag. Existing public performance reports are untouched.

```sh
python3 benchmark/lookup-strategies/direct_campaign.py --source FROZEN_CANDIDATE --output NEW_OUTPUT --cpu 12 \
  --rounds 3 --sizes 8192,100000,1000000 --queries 65536 --samples 7 \
  --write-transactions 4 --seed 73 --ballast-mib 0
```

Use --build-only to stop before measurements; --resume continues the exact retained configuration and hashes.
