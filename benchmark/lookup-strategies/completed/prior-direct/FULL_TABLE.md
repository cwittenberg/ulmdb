# Production lookup strategy: complete direct comparison

90 fresh processes; 3 rounds × 3 sizes × two key types × five arms. 7 samples per read/write case; 65,536 point queries and up to1,024 scan queries per workload. Seeds 73 through 75.

Machine: AMD EPYC 7702P 64-Core Processor; x86_64; Linux 6.8.0-147-generic; CPU 12. Shared host; no exclusive CPU reservation. Source manifest SHA-256 `a8e33e87ba363fa038e109c7d0f07a126aca8922dc84c7de1aa9e364815de26c`; executable SHA-256 `54506833c9952e7c87c951c73ce5652c9c870d7c6fb6e367512a6e7cd9f278d5`. Pinned native reference: LMDB 1.0.2: (Aug 6, 2026).

All five arms execute sequentially in fresh processes with reversing/rotating group order. Native uses the original prepared KvRead and constructs no hash. Hash uses the completed production API with borrowed keys/values and exact owned slot capacity accounting. LMDB uses only NOTLS (0x200000), normal locking and synchronized durable commits. A held old reader exists in native, hash and LMDB pinned-write phases; hash additionally retains its side index. All initial, old, new and reopened values and forward/reverse ranges are checked against the same deterministic byte oracle. Every corresponding timed checksum and operation count is checked across all five arms.

Values are medians of within-process samples, then medians across fresh processes. Each ratio is baseline cost / hash cost; above1 is a lower hash cost. Brackets show the minimum and maximum paired process ratio, not confidence intervals. A >5% increase in any process is retained as an adverse observation, including construction, memory, retention and lifecycle costs; it is not automatically a statistically established regression. 'all' means every process exceeded5%. Missing or undefined zero-cost ratios are N/A.

## Read throughput costs

Nanoseconds per point query or one cursor seek plus up to100/64 complete key/value results. These are batch throughput averages, not individual request p99 latency.

| Keys | Key type | Phase / operation | Legacy native | Legacy hash | Resident native | Resident hash | LMDB | Legacy hash/native | Resident hash/native | Legacy hash/LMDB | Resident hash/LMDB | >5% hash cost increase vs native |
|---:|---|---|---:|---:|---:|---:|---:|---|---|---|---|---|
| 8192 | fixed16 | hit | 359.653 | 72.729 | 251.067 | 72.490 | 254.967 | 4.82x [4.66,4.97] | 3.46x [3.44,3.51] | 3.5x [3.3,3.7] | 3.51x [3.49,3.74] | None observed |
| 8192 | fixed16 | hot90 | 299.493 | 50.469 | 207.248 | 57.104 | 191.854 | 5.99x [5.24,6.16] | 3.63x [3.57,4.94] | 3.81x [3.37,3.95] | 3.35x [3.32,4.56] | None observed |
| 8192 | fixed16 | miss | 340.984 | 45.948 | 242.984 | 45.417 | 249.119 | 7.32x [6.52,7.47] | 5.37x [5.34,5.4] | 5.39x [4.76,5.78] | 5.52x [5.44,5.81] | None observed |
| 8192 | fixed16 | mixed50 | 355.687 | 58.068 | 251.220 | 59.325 | 254.123 | 5.98x [5.56,6.16] | 4.24x [4.22,4.39] | 4.32x [3.97,4.38] | 4.28x [4.21,4.44] | None observed |
| 8192 | fixed16 | range_forward64 | 4,380.517 | 4,280.288 | 4,080.122 | 4,425.671 | 3,069.372 | 1x [1,1.02] | 0.921x [0.92,0.926] | 0.717x [0.701,0.756] | 0.694x [0.694,0.728] | resident (all) |
| 8192 | fixed16 | range_reverse64 | 4,146.419 | 4,282.832 | 4,175.137 | 4,485.199 | 2,908.606 | 0.965x [0.957,0.993] | 0.922x [0.921,0.938] | 0.682x [0.667,0.685] | 0.644x [0.638,0.659] | resident (all) |
| 8192 | fixed16 | scan_forward100 | 5,632.058 | 5,687.015 | 5,599.760 | 6,115.223 | 4,333.914 | 0.986x [0.985,0.991] | 0.906x [0.902,0.916] | 0.754x [0.739,0.786] | 0.698x [0.687,0.731] | resident (all) |
| 8192 | fixed16 | scan_reverse100 | 5,676.066 | 5,839.405 | 5,685.077 | 6,139.810 | 3,902.949 | 0.978x [0.971,0.984] | 0.91x [0.91,0.931] | 0.671x [0.668,0.678] | 0.637x [0.625,0.639] | resident (all) |
| 8192 | hostname | hit | 360.164 | 85.563 | 402.886 | 69.143 | 272.960 | 4.21x [4.19,5.2] | 5.84x [5.8,5.87] | 3.17x [3.17,3.98] | 3.95x [3.93,3.98] | None observed |
| 8192 | hostname | hot90 | 287.971 | 65.154 | 314.186 | 50.570 | 192.496 | 4.43x [4.42,5.63] | 6.21x [6.2,6.23] | 2.97x [2.93,3.8] | 3.81x [3.8,3.81] | None observed |
| 8192 | hostname | miss | 341.727 | 54.246 | 381.596 | 54.543 | 270.854 | 6.3x [6.27,6.3] | 7.01x [7,7.08] | 5.01x [4.89,5.02] | 4.97x [4.87,5.05] | None observed |
| 8192 | hostname | mixed50 | 356.255 | 70.718 | 391.976 | 66.334 | 277.097 | 5.04x [5.04,5.26] | 5.91x [5.91,6.01] | 3.92x [3.83,4.14] | 4.18x [4.11,4.19] | None observed |
| 8192 | hostname | range_forward64 | 5,234.661 | 5,219.378 | 6,054.472 | 5,777.383 | 3,785.685 | 1x [0.95,1.01] | 1.05x [1.02,1.05] | 0.725x [0.719,0.731] | 0.65x [0.647,0.657] | legacy (some) |
| 8192 | hostname | range_reverse64 | 5,161.681 | 5,206.071 | 5,985.131 | 5,804.856 | 3,624.235 | 0.991x [0.955,1.01] | 1.04x [1.02,1.04] | 0.696x [0.692,0.716] | 0.621x [0.62,0.635] | None observed |
| 8192 | hostname | scan_forward100 | 7,175.256 | 7,192.172 | 8,521.341 | 8,196.475 | 5,433.966 | 0.989x [0.953,1] | 1.04x [1.03,1.05] | 0.751x [0.746,0.756] | 0.661x [0.656,0.663] | None observed |
| 8192 | hostname | scan_reverse100 | 7,451.445 | 7,239.743 | 8,018.079 | 8,320.999 | 5,145.791 | 1.03x [0.941,1.04] | 0.968x [0.962,0.986] | 0.708x [0.707,0.729] | 0.627x [0.617,0.627] | legacy (some) |
| 100000 | fixed16 | hit | 461.488 | 106.021 | 385.528 | 109.444 | 351.302 | 4.35x [4.03,4.82] | 3.53x [3.52,4.31] | 3.32x [3.12,3.66] | 3.21x [2.61,3.23] | None observed |
| 100000 | fixed16 | hot90 | 383.565 | 65.155 | 311.662 | 64.745 | 272.232 | 5.86x [5.75,7.97] | 4.81x [4.77,4.85] | 4.17x [4.14,5.66] | 4.2x [4.2,4.23] | None observed |
| 100000 | fixed16 | miss | 443.738 | 49.579 | 377.142 | 52.303 | 349.660 | 8.74x [8.41,8.95] | 7.21x [5.59,7.55] | 7.02x [6.63,7.07] | 6.7x [5.18,7.03] | None observed |
| 100000 | fixed16 | mixed50 | 458.312 | 99.555 | 385.153 | 98.720 | 353.476 | 4.5x [3.83,4.92] | 3.9x [3.33,4] | 3.53x [2.95,3.79] | 3.58x [3.03,3.68] | None observed |
| 100000 | fixed16 | range_forward64 | 4,498.368 | 4,438.910 | 4,351.135 | 4,596.260 | 3,210.177 | 1.01x [1.01,1.02] | 0.948x [0.937,0.949] | 0.724x [0.72,0.733] | 0.7x [0.687,0.708] | resident (all) |
| 100000 | fixed16 | range_reverse64 | 4,247.472 | 4,417.854 | 4,287.439 | 4,738.444 | 3,064.421 | 0.966x [0.953,0.981] | 0.905x [0.894,0.969] | 0.694x [0.667,0.707] | 0.653x [0.647,0.66] | resident (some) |
| 100000 | fixed16 | scan_forward100 | 5,795.776 | 5,888.287 | 5,829.356 | 6,367.382 | 4,411.387 | 0.985x [0.98,0.989] | 0.91x [0.904,0.965] | 0.747x [0.745,0.777] | 0.689x [0.689,0.718] | resident (some) |
| 100000 | fixed16 | scan_reverse100 | 5,811.030 | 5,940.378 | 5,950.094 | 6,399.649 | 4,060.143 | 0.976x [0.973,1] | 0.93x [0.913,0.959] | 0.689x [0.679,0.691] | 0.635x [0.63,0.643] | resident (some) |
| 100000 | hostname | hit | 491.590 | 158.805 | 601.896 | 168.736 | 366.581 | 3.14x [3.06,3.16] | 3.62x [3.23,4.13] | 2.31x [2.14,2.32] | 2.17x [1.97,2.52] | None observed |
| 100000 | hostname | hot90 | 374.612 | 79.269 | 455.910 | 79.482 | 253.397 | 4.77x [4.71,4.78] | 5.74x [5.73,5.76] | 3.22x [3.16,3.22] | 3.19x [3.17,3.2] | None observed |
| 100000 | hostname | miss | 466.641 | 61.274 | 578.139 | 62.750 | 364.427 | 7.62x [7.45,7.72] | 9.21x [7.6,9.3] | 5.92x [5.84,5.99] | 5.77x [4.35,5.83] | None observed |
| 100000 | hostname | mixed50 | 480.660 | 129.360 | 599.845 | 133.877 | 370.520 | 3.71x [3.69,3.98] | 4.45x [3.79,4.48] | 2.86x [2.84,3.76] | 2.77x [2.32,3.65] | None observed |
| 100000 | hostname | range_forward64 | 5,293.229 | 5,478.229 | 6,275.703 | 6,154.750 | 3,963.103 | 0.954x [0.863,0.967] | 1.02x [0.991,1.02] | 0.707x [0.655,0.736] | 0.629x [0.624,0.653] | legacy (some) |
| 100000 | hostname | range_reverse64 | 5,148.139 | 5,474.336 | 6,192.879 | 6,170.483 | 3,763.974 | 0.952x [0.849,0.969] | 1.01x [0.978,1.03] | 0.688x [0.63,0.692] | 0.612x [0.591,0.619] | legacy (some) |
| 100000 | hostname | scan_forward100 | 7,935.979 | 7,527.214 | 8,838.595 | 8,634.378 | 5,684.550 | 0.958x [0.92,1.59] | 1.02x [0.958,1.03] | 0.737x [0.659,0.765] | 0.644x [0.626,0.658] | legacy (some) |
| 100000 | hostname | scan_reverse100 | 7,854.213 | 7,718.329 | 8,472.194 | 8,806.600 | 5,296.537 | 0.919x [0.892,1.58] | 0.966x [0.933,1.01] | 0.676x [0.613,0.698] | 0.603x [0.583,0.613] | legacy (some), resident (some) |
| 1000000 | fixed16 | hit | 1,015.402 | 314.060 | 898.789 | 309.150 | 751.740 | 3.23x [2.98,3.46] | 2.95x [2.61,3.05] | 2.39x [2.25,2.45] | 2.47x [2.15,2.47] | None observed |
| 1000000 | fixed16 | hot90 | 549.648 | 79.101 | 470.453 | 93.440 | 373.325 | 6.95x [6.08,7.3] | 5.08x [4.95,6.36] | 4.66x [4.08,5.06] | 4.02x [3.94,5.05] | None observed |
| 1000000 | fixed16 | miss | 1,005.660 | 155.064 | 891.949 | 162.149 | 760.457 | 6.44x [6.4,6.76] | 5.5x [5.5,6.89] | 4.9x [4.75,4.93] | 4.63x [4.61,4.69] | None observed |
| 1000000 | fixed16 | mixed50 | 1,018.341 | 239.371 | 918.266 | 246.260 | 770.039 | 4.25x [4.17,4.28] | 3.72x [3.63,3.73] | 3.23x [3.06,3.23] | 3.08x [2.98,3.13] | None observed |
| 1000000 | fixed16 | range_forward64 | 4,783.158 | 4,582.698 | 4,635.123 | 4,902.115 | 3,365.766 | 1.04x [1.01,1.06] | 0.946x [0.931,0.961] | 0.735x [0.732,0.738] | 0.697x [0.669,0.7] | resident (some) |
| 1000000 | fixed16 | range_reverse64 | 4,482.088 | 4,596.523 | 4,559.305 | 4,846.315 | 3,197.672 | 0.975x [0.964,1] | 0.943x [0.922,0.946] | 0.694x [0.691,0.707] | 0.66x [0.642,0.669] | resident (all) |
| 1000000 | fixed16 | scan_forward100 | 6,081.535 | 6,082.964 | 6,130.955 | 6,715.211 | 4,526.107 | 0.999x [0.994,1] | 0.917x [0.896,0.922] | 0.742x [0.739,0.756] | 0.669x [0.663,0.691] | resident (all) |
| 1000000 | fixed16 | scan_reverse100 | 6,096.613 | 6,138.822 | 6,133.548 | 6,639.911 | 4,359.540 | 0.986x [0.974,1.01] | 0.921x [0.881,1.02] | 0.71x [0.685,0.717] | 0.648x [0.626,0.663] | resident (some) |
| 1000000 | hostname | hit | 1,104.555 | 325.400 | 1,313.284 | 350.711 | 816.089 | 3.39x [3.26,3.57] | 3.76x [3.68,3.84] | 2.53x [2.42,2.62] | 2.33x [2.27,2.41] | None observed |
| 1000000 | hostname | hot90 | 541.451 | 89.938 | 639.899 | 115.096 | 388.905 | 6.02x [4.87,6.39] | 5.56x [5.36,6.73] | 4.32x [3.51,4.61] | 3.42x [3.26,4.03] | None observed |
| 1000000 | hostname | miss | 1,080.720 | 167.183 | 1,294.924 | 181.344 | 823.743 | 6.49x [6.36,6.49] | 7.14x [7.01,7.5] | 4.87x [4.85,5.04] | 4.54x [4.41,4.77] | None observed |
| 1000000 | hostname | mixed50 | 1,090.694 | 255.958 | 1,319.581 | 270.824 | 822.798 | 4.24x [4.2,4.29] | 4.93x [4.67,4.94] | 3.23x [3.16,3.24] | 3.03x [2.95,3.1] | None observed |
| 1000000 | hostname | range_forward64 | 5,478.503 | 5,744.067 | 6,836.290 | 6,796.469 | 4,391.318 | 0.954x [0.946,0.957] | 1.01x [1,1.02] | 0.764x [0.734,0.764] | 0.646x [0.632,0.651] | legacy (some) |
| 1000000 | hostname | range_reverse64 | 5,488.238 | 5,852.319 | 6,736.795 | 6,673.188 | 4,185.743 | 0.953x [0.938,0.979] | 1.01x [0.955,1.03] | 0.719x [0.715,0.726] | 0.627x [0.58,0.655] | legacy (some) |
| 1000000 | hostname | scan_forward100 | 8,255.171 | 8,490.030 | 10,246.086 | 10,072.415 | 6,558.879 | 0.989x [0.946,1.01] | 1.03x [1.01,1.06] | 0.773x [0.773,0.774] | 0.651x [0.649,0.659] | legacy (some) |
| 1000000 | hostname | scan_reverse100 | 7,993.511 | 8,176.261 | 9,543.401 | 9,643.220 | 6,147.480 | 0.986x [0.952,0.987] | 0.988x [0.951,0.993] | 0.752x [0.729,0.761] | 0.637x [0.614,0.645] | legacy (some), resident (some) |

## Durable writes in every pin state

Microseconds per synchronized transaction. Batch sizes are encoded in operation names; raw files also retain ns/update and exact operation counts. Fixtures are precomputed outside timing. No hash is automatically rebuilt on commit: this table measures writes with the same old snapshot held, plus the optional existing index; the explicit full rebuild is charged below.

| Keys | Key type | Phase / operation | Legacy native | Legacy hash | Resident native | Resident hash | LMDB | Legacy hash/native | Resident hash/native | Legacy hash/LMDB | Resident hash/LMDB | >5% hash cost increase vs native |
|---:|---|---|---:|---:|---:|---:|---:|---|---|---|---|---|
| 8192 | fixed16 | pinned / durable_batch1 | 1,970.966 | 2,080.023 | 76.808 | 77.567 | 99.056 | 0.962x [0.944,1.03] | 0.995x [0.982,1.02] | 0.0527x [0.0397,0.0674] | 1.28x [1.06,1.81] | legacy (some) |
| 8192 | fixed16 | pinned / durable_batch1000 | 3,870.778 | 3,921.833 | 1,271.924 | 1,287.631 | 1,666.759 | 0.987x [0.984,0.994] | 0.992x [0.947,1] | 0.425x [0.421,0.425] | 1.29x [1.28,1.3] | resident (some) |
| 8192 | fixed16 | pinned / durable_batch64 | 2,485.825 | 2,523.378 | 227.966 | 223.710 | 709.672 | 0.985x [0.945,1.01] | 1x [0.854,1.03] | 0.292x [0.267,0.293] | 3.2x [2.53,3.49] | legacy (some), resident (some) |
| 8192 | fixed16 | unpinned_after / durable_batch1 | 1,939.653 | 1,903.442 | 78.347 | 77.936 | 160.139 | 1.02x [1,1.06] | 0.994x [0.987,1.02] | 0.0836x [0.065,0.0841] | 2.02x [1.56,2.05] | None observed |
| 8192 | fixed16 | unpinned_after / durable_batch1000 | 3,973.736 | 3,935.296 | 1,264.289 | 1,264.044 | 1,054.931 | 1.01x [0.999,1.01] | 1.01x [0.977,1.01] | 0.267x [0.266,0.271] | 0.835x [0.825,0.852] | None observed |
| 8192 | fixed16 | unpinned_after / durable_batch64 | 2,491.280 | 2,543.539 | 237.103 | 230.759 | 542.697 | 0.984x [0.961,0.993] | 1.02x [1.01,1.06] | 0.213x [0.208,0.219] | 2.35x [2.31,2.42] | None observed |
| 8192 | fixed16 | unpinned_before / durable_batch1 | 1,946.001 | 1,874.392 | 76.315 | 76.824 | 81.470 | 0.978x [0.64,1.63] | 0.992x [0.985,1] | 0.0426x [0.0269,0.0435] | 1.06x [1.04,1.06] | legacy (some) |
| 8192 | fixed16 | unpinned_before / durable_batch1000 | 3,852.659 | 3,884.877 | 1,304.816 | 1,241.538 | 1,202.910 | 0.996x [0.982,1] | 1.01x [0.996,1.07] | 0.31x [0.306,0.319] | 0.983x [0.93,0.993] | None observed |
| 8192 | fixed16 | unpinned_before / durable_batch64 | 2,444.318 | 2,463.404 | 239.275 | 233.700 | 556.040 | 0.978x [0.96,1.02] | 1.02x [1.01,1.07] | 0.229x [0.215,0.234] | 2.38x [2.31,2.47] | None observed |
| 8192 | hostname | pinned / durable_batch1 | 1,899.272 | 1,924.875 | 80.701 | 80.142 | 106.813 | 0.996x [0.97,1.01] | 1x [0.986,1.01] | 0.0555x [0.0461,0.0566] | 1.32x [1.1,1.4] | None observed |
| 8192 | hostname | pinned / durable_batch1000 | 4,522.983 | 4,511.224 | 1,722.270 | 1,607.797 | 2,139.709 | 1x [1,1.01] | 1.04x [1.01,1.07] | 0.476x [0.474,0.52] | 1.32x [1.27,1.48] | None observed |
| 8192 | hostname | pinned / durable_batch64 | 2,549.869 | 2,480.469 | 278.545 | 270.131 | 736.007 | 1.03x [1.01,1.04] | 1.03x [1.02,1.04] | 0.298x [0.291,0.326] | 2.72x [2.66,3.09] | None observed |
| 8192 | hostname | unpinned_after / durable_batch1 | 1,897.142 | 1,890.327 | 78.862 | 78.945 | 139.783 | 0.992x [0.986,1.05] | 0.999x [0.999,1] | 0.0739x [0.0681,0.0935] | 1.77x [1.66,2.07] | None observed |
| 8192 | hostname | unpinned_after / durable_batch1000 | 4,512.245 | 4,579.240 | 1,680.678 | 1,745.502 | 1,375.774 | 0.987x [0.985,1] | 0.976x [0.908,0.983] | 0.306x [0.3,0.31] | 0.799x [0.75,0.803] | resident (some) |
| 8192 | hostname | unpinned_after / durable_batch64 | 2,547.003 | 2,550.087 | 285.137 | 271.945 | 624.147 | 1.01x [0.947,1.03] | 1.08x [0.977,1.18] | 0.247x [0.232,0.248] | 2.27x [2.18,2.3] | legacy (some) |
| 8192 | hostname | unpinned_before / durable_batch1 | 1,897.724 | 1,899.432 | 78.276 | 79.474 | 95.725 | 0.998x [0.992,1.03] | 0.982x [0.84,0.985] | 0.0522x [0.05,0.0541] | 1.2x [0.962,1.25] | resident (some) |
| 8192 | hostname | unpinned_before / durable_batch1000 | 4,534.813 | 4,489.279 | 1,704.341 | 1,707.797 | 1,561.291 | 1.01x [0.982,1.03] | 1.01x [0.947,1.02] | 0.342x [0.336,0.357] | 0.915x [0.9,0.926] | resident (some) |
| 8192 | hostname | unpinned_before / durable_batch64 | 2,511.260 | 2,446.645 | 287.274 | 274.743 | 606.022 | 1.03x [1,1.04] | 1.06x [0.976,1.07] | 0.249x [0.246,0.249] | 2.22x [2.18,2.24] | None observed |
| 100000 | fixed16 | pinned / durable_batch1 | 2,012.707 | 1,963.296 | 80.328 | 81.773 | 114.841 | 1.05x [0.974,1.08] | 0.988x [0.979,0.999] | 0.0644x [0.0556,0.0753] | 1.4x [1.39,1.84] | None observed |
| 100000 | fixed16 | pinned / durable_batch1000 | 10,601.827 | 10,649.300 | 2,926.590 | 3,020.912 | 6,928.375 | 0.993x [0.958,0.997] | 0.969x [0.955,1.01] | 0.651x [0.622,0.666] | 2.29x [2.27,2.35] | None observed |
| 100000 | fixed16 | pinned / durable_batch64 | 2,845.140 | 2,884.074 | 340.971 | 332.029 | 951.507 | 0.995x [0.961,1] | 1.03x [0.949,1.14] | 0.325x [0.322,0.34] | 2.87x [2.72,2.92] | resident (some) |
| 100000 | fixed16 | unpinned_after / durable_batch1 | 1,931.931 | 1,880.461 | 83.334 | 85.445 | 170.910 | 1.05x [1.03,1.06] | 0.987x [0.971,1.01] | 0.0855x [0.0822,0.0947] | 1.99x [1.81,2.08] | None observed |
| 100000 | fixed16 | unpinned_after / durable_batch1000 | 10,681.133 | 10,687.766 | 3,035.211 | 3,035.104 | 6,828.783 | 1.01x [0.957,1.02] | 0.999x [0.99,1.01] | 0.644x [0.617,0.759] | 2.28x [2.23,2.67] | None observed |
| 100000 | fixed16 | unpinned_after / durable_batch64 | 2,782.341 | 2,945.731 | 383.582 | 389.135 | 863.164 | 0.986x [0.942,0.994] | 0.996x [0.935,1.09] | 0.295x [0.293,0.304] | 2.24x [2.22,2.41] | legacy (some), resident (some) |
| 100000 | fixed16 | unpinned_before / durable_batch1 | 1,923.547 | 1,884.398 | 80.390 | 80.213 | 98.097 | 1.01x [0.821,1.06] | 1.01x [0.972,1.25] | 0.0539x [0.0406,0.0776] | 1.23x [1.21,1.81] | legacy (some) |
| 100000 | fixed16 | unpinned_before / durable_batch1000 | 10,563.312 | 10,764.352 | 3,048.820 | 2,945.802 | 7,215.957 | 0.979x [0.957,0.989] | 1.01x [0.991,1.05] | 0.657x [0.576,0.675] | 2.36x [2.11,2.45] | None observed |
| 100000 | fixed16 | unpinned_before / durable_batch64 | 2,771.728 | 2,769.449 | 390.408 | 343.463 | 795.222 | 0.998x [0.923,1.02] | 1.17x [0.964,1.21] | 0.27x [0.241,0.287] | 2.27x [2,2.36] | legacy (some) |
| 100000 | hostname | pinned / durable_batch1 | 2,054.307 | 1,996.093 | 83.383 | 84.513 | 134.548 | 1.04x [1.02,1.1] | 0.997x [0.975,1.03] | 0.0674x [0.0647,0.0848] | 1.61x [1.54,1.74] | None observed |
| 100000 | hostname | pinned / durable_batch1000 | 11,857.510 | 11,927.035 | 6,476.843 | 6,494.207 | 7,969.279 | 1.01x [0.979,1.07] | 0.997x [0.994,0.997] | 0.668x [0.657,0.725] | 1.23x [1.22,1.23] | None observed |
| 100000 | hostname | pinned / durable_batch64 | 3,026.583 | 3,060.016 | 458.116 | 427.909 | 1,151.207 | 1.01x [0.989,1.02] | 0.999x [0.971,1.07] | 0.388x [0.374,0.399] | 2.69x [2.32,2.77] | None observed |
| 100000 | hostname | unpinned_after / durable_batch1 | 2,023.728 | 1,936.510 | 84.869 | 85.405 | 136.379 | 1.05x [1.02,1.1] | 0.994x [0.933,1.36] | 0.0765x [0.0683,0.0871] | 1.6x [1.5,1.98] | resident (some) |
| 100000 | hostname | unpinned_after / durable_batch1000 | 11,877.180 | 11,932.114 | 4,045.135 | 4,109.266 | 8,415.843 | 0.995x [0.892,1.06] | 0.984x [0.963,0.992] | 0.705x [0.62,0.754] | 2.05x [1.93,2.11] | legacy (some) |
| 100000 | hostname | unpinned_after / durable_batch64 | 3,071.012 | 2,972.617 | 493.333 | 497.942 | 1,026.727 | 1.06x [0.993,1.1] | 1.02x [0.968,1.05] | 0.346x [0.345,0.365] | 2.15x [1.97,2.17] | None observed |
| 100000 | hostname | unpinned_before / durable_batch1 | 1,911.159 | 1,898.836 | 81.663 | 83.364 | 158.919 | 0.97x [0.939,1.1] | 0.98x [0.98,1.03] | 0.0837x [0.0589,0.0884] | 1.91x [1.39,1.94] | legacy (some) |
| 100000 | hostname | unpinned_before / durable_batch1000 | 11,892.349 | 11,816.793 | 4,281.687 | 4,272.244 | 8,349.394 | 1.01x [0.994,1.04] | 1x [0.999,1.01] | 0.707x [0.704,0.718] | 1.96x [1.92,2.01] | None observed |
| 100000 | hostname | unpinned_before / durable_batch64 | 2,975.132 | 2,930.147 | 456.380 | 454.094 | 970.323 | 1.01x [0.999,1.03] | 1.01x [0.915,1.01] | 0.331x [0.326,0.336] | 2.1x [1.96,2.26] | resident (some) |
| 1000000 | fixed16 | pinned / durable_batch1 | 1,885.686 | 2,016.407 | 84.145 | 84.836 | 146.586 | 0.949x [0.908,0.999] | 0.995x [0.982,1.01] | 0.0727x [0.0653,0.0784] | 1.73x [1.53,1.87] | legacy (some) |
| 1000000 | fixed16 | pinned / durable_batch1000 | 16,150.797 | 16,392.304 | 5,352.518 | 5,281.077 | 10,058.787 | 0.982x [0.921,0.993] | 1.01x [1.01,1.02] | 0.611x [0.601,0.632] | 1.94x [1.86,1.96] | legacy (some) |
| 1000000 | fixed16 | pinned / durable_batch64 | 3,315.863 | 3,492.055 | 511.996 | 503.665 | 1,357.757 | 0.955x [0.921,0.967] | 1.03x [0.932,1.06] | 0.38x [0.377,0.409] | 2.7x [2.45,2.75] | legacy (some), resident (some) |
| 1000000 | fixed16 | unpinned_after / durable_batch1 | 2,034.018 | 2,010.300 | 86.119 | 86.038 | 174.507 | 0.962x [0.953,1.08] | 1x [0.991,1.03] | 0.0822x [0.0815,0.0948] | 2.03x [1.82,2.2] | None observed |
| 1000000 | fixed16 | unpinned_after / durable_batch1000 | 16,192.052 | 16,133.098 | 5,312.845 | 5,278.523 | 12,200.482 | 1.01x [0.985,1.01] | 1.01x [1,1.06] | 0.754x [0.742,0.795] | 2.37x [2.3,2.39] | None observed |
| 1000000 | fixed16 | unpinned_after / durable_batch64 | 3,394.129 | 3,437.176 | 513.817 | 520.485 | 1,317.007 | 1x [0.954,1.02] | 0.983x [0.923,1.02] | 0.385x [0.375,0.396] | 2.48x [2.37,2.61] | resident (some) |
| 1000000 | fixed16 | unpinned_before / durable_batch1 | 1,795.326 | 1,863.163 | 87.712 | 85.856 | 108.654 | 0.979x [0.95,0.98] | 1.01x [1,1.04] | 0.0593x [0.0559,0.0875] | 1.24x [1.21,1.93] | legacy (some) |
| 1000000 | fixed16 | unpinned_before / durable_batch1000 | 15,804.736 | 16,016.045 | 5,259.922 | 5,283.654 | 11,899.816 | 0.987x [0.957,1.05] | 0.996x [0.964,1.02] | 0.739x [0.733,0.748] | 2.25x [2.19,2.3] | None observed |
| 1000000 | fixed16 | unpinned_before / durable_batch64 | 3,412.113 | 3,429.108 | 504.697 | 545.282 | 1,433.531 | 0.992x [0.974,1.02] | 0.968x [0.918,0.981] | 0.418x [0.385,0.425] | 2.61x [2.61,2.66] | resident (some) |
| 1000000 | hostname | pinned / durable_batch1 | 1,941.550 | 1,917.025 | 88.210 | 89.377 | 129.792 | 0.972x [0.962,1.06] | 0.987x [0.952,1] | 0.0668x [0.065,0.0904] | 1.47x [1.4,1.92] | resident (some) |
| 1000000 | hostname | pinned / durable_batch1000 | 17,630.823 | 17,733.576 | 32,100.771 | 32,066.085 | 11,957.413 | 1.01x [0.991,1.03] | 1x [0.63,1.06] | 0.674x [0.629,0.703] | 0.373x [0.221,0.377] | resident (some) |
| 1000000 | hostname | pinned / durable_batch64 | 3,453.620 | 3,492.243 | 679.444 | 674.555 | 1,497.312 | 0.989x [0.966,1.03] | 1.01x [0.853,1.13] | 0.42x [0.414,0.463] | 2.32x [1.85,2.39] | resident (some) |
| 1000000 | hostname | unpinned_after / durable_batch1 | 2,007.034 | 2,001.098 | 88.285 | 88.400 | 188.669 | 0.928x [0.864,1.04] | 0.999x [0.991,1] | 0.0943x [0.0809,0.0977] | 2.13x [2.13,2.19] | legacy (some) |
| 1000000 | hostname | unpinned_after / durable_batch1000 | 17,808.128 | 17,943.180 | 7,181.734 | 7,637.741 | 14,368.765 | 0.977x [0.961,1] | 0.94x [0.927,0.993] | 0.796x [0.791,0.808] | 1.87x [1.87,2] | resident (some) |
| 1000000 | hostname | unpinned_after / durable_batch64 | 3,618.062 | 3,493.541 | 638.281 | 674.433 | 1,457.506 | 0.972x [0.898,1.05] | 1x [0.912,1.03] | 0.416x [0.362,0.42] | 2.16x [2.16,2.35] | legacy (some), resident (some) |
| 1000000 | hostname | unpinned_before / durable_batch1 | 1,828.489 | 1,812.349 | 88.055 | 88.473 | 177.558 | 0.999x [0.967,1.05] | 0.989x [0.973,1.02] | 0.102x [0.0977,0.104] | 2.05x [1.98,2.15] | None observed |
| 1000000 | hostname | unpinned_before / durable_batch1000 | 18,547.073 | 17,940.533 | 7,180.189 | 7,187.923 | 14,103.053 | 1.04x [0.987,1.05] | 0.999x [0.966,1.05] | 0.796x [0.764,0.798] | 1.96x [1.9,2.02] | None observed |
| 1000000 | hostname | unpinned_before / durable_batch64 | 3,678.991 | 3,602.586 | 649.808 | 647.133 | 1,592.639 | 1.04x [1.02,1.16] | 1x [0.949,1.06] | 0.442x [0.431,0.463] | 2.5x [2.4,2.6] | resident (some) |

## Build, drop, snapshot and reopen lifecycle

Milliseconds per event. Each event is observed once per fresh process, rather than seven samples. Resident performs an explicitly charged ordinary Collect before retaining the old snapshot in both native and hash arms; Legacy and LMDB use a no-op. The total_snapshot_prepare row sums maintenance_before_pin + begin_snapshot + prepare_lookup for each process before aggregation. Maintenance is never treated as free. The capability does not collect automatically. Resident's common benchmark pool is ceil(keys/24)+8192 KV slots with256MiB metadata admission, providing room for both pinned generations; this is a disclosed benchmark setting, not a change to engine defaults. Rebuild includes acquiring the new snapshot and complete preparation. Final_validation measures the complete final byte verification. Initial loaders use different APIs and are labeled in raw reports; their costs do not establish equal-loader superiority. Reopen uses a warm OS cache. N/A identifies engine-specific events.

| Keys | Key type | Phase / operation | Legacy native | Legacy hash | Resident native | Resident hash | LMDB | Legacy hash/native | Resident hash/native | Legacy hash/LMDB | Resident hash/LMDB | >5% hash cost increase vs native |
|---:|---|---|---:|---:|---:|---:|---:|---|---|---|---|---|
| 8192 | fixed16 | begin_snapshot | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | 1x [0.989,1.14] | 1x [1,1.17] | 44.7x [38.8,48.8] | 52.1x [48.8,58.8] | None observed |
| 8192 | fixed16 | close_environment | 5.650 | 6.007 | 44.972 | 45.949 | 1.434 | 0.979x [0.892,0.996] | 1.02x [0.968,1.02] | 0.238x [0.213,0.248] | 0.0312x [0.0278,0.033] | legacy (some) |
| 8192 | fixed16 | create_environment | 4.228 | 5.429 | 445.903 | 426.716 | 0.514 | 1.02x [0.713,1.13] | 1.01x [1.01,1.05] | 0.119x [0.0662,0.123] | 0.00122x [0.000811,0.00153] | legacy (some) |
| 8192 | fixed16 | drop_new_lookup | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.74x [0.576,0.893] | 0.53x [0.502,0.58] | 0.111x [0.0878,0.185] | 0.111x [0.102,0.121] | legacy (all), resident (all) |
| 8192 | fixed16 | drop_old_lookup | 0.000 | 0.000 | 0.000 | 0.109 | 0.000 | 0.538x [0.533,0.538] | 0.000556x [0.000548,0.000601] | 0.231x [0.2,0.231] | 0.000274x [0.000257,0.000278] | legacy (all), resident (all) |
| 8192 | fixed16 | final_validation | 2.716 | 0.712 | 1.836 | 0.758 | 1.638 | 3.82x [3.69,3.95] | 2.42x [2.41,2.47] | 2.3x [2.19,2.43] | 2.18x [2.14,2.19] | None observed |
| 8192 | fixed16 | fixture_load | 7.564 | 7.230 | 10.979 | 10.958 | 7.016 | 0.969x [0.957,1.25] | 1x [0.934,1.9] | 0.913x [0.888,1.16] | 0.658x [0.602,0.682] | resident (some) |
| 8192 | fixed16 | maintenance_before_pin | 0.002 | 0.002 | 2.223 | 2.046 | 0.001 | 1.04x [0.954,1.07] | 1.03x [0.987,1.16] | 0.702x [0.697,0.789] | 0.000708x [0.000627,0.000769] | None observed |
| 8192 | fixed16 | prepare_after_reopen | 0.001 | 0.899 | 0.003 | 1.365 | 0.000 | 0.0014x [0.00121,0.00181] | 0.00228x [0.00206,0.0024] | 4.45e-05x [4.39e-05,5.57e-05] | 3.06e-05x [2.93e-05,3.52e-05] | legacy (all), resident (all) |
| 8192 | fixed16 | prepare_final_validation | 0.001 | 0.903 | 0.001 | 0.984 | 0.000 | 0.00137x [0.0013,0.0017] | 0.00111x [0.00105,0.00118] | 0.000225x [0.000221,0.000577] | 0.000213x [0.000203,0.000523] | legacy (all), resident (all) |
| 8192 | fixed16 | prepare_lookup | 0.001 | 0.931 | 0.001 | 1.312 | 0.000 | 0.00125x [0.00107,0.00167] | 0.000787x [0.000644,0.000916] | 7.52e-05x [7.26e-05,7.74e-05] | 5.39e-05x [5.17e-05,5.41e-05] | legacy (all), resident (all) |
| 8192 | fixed16 | rebuild_after_pinned_writes | 0.001 | 0.870 | 0.001 | 0.925 | 0.002 | 0.00123x [0.00122,0.00185] | 0.00105x [0.00103,0.00121] | 0.00253x [0.00177,0.00281] | 0.00242x [0.00172,0.00264] | legacy (all), resident (all) |
| 8192 | fixed16 | release_old_snapshot | 0.051 | 0.050 | 0.006 | 0.007 | 0.000 | 1x [0.996,1.02] | 0.867x [0.735,1.1] | 0.00217x [0.0018,0.00418] | 0.0152x [0.011,0.0327] | resident (some) |
| 8192 | fixed16 | reopen_database_handle | N/A | N/A | N/A | N/A | 0.008 | N/A | N/A | N/A | N/A | None observed |
| 8192 | fixed16 | reopen_environment | 48.352 | 48.698 | 371.019 | 371.157 | 0.161 | 0.995x [0.985,1] | 1.01x [0.981,1.01] | 0.00331x [0.00326,0.00386] | 0.000437x [0.000429,0.000507] | None observed |
| 8192 | fixed16 | preparation / total_snapshot_prepare | 0.003 | 0.933 | 2.224 | 3.345 | 0.005 | 0.00353x [0.00319,0.00402] | 0.633x [0.624,0.683] | 0.00534x [0.00471,0.00558] | 0.00151x [0.00129,0.00151] | legacy (all), resident (all) |
| 8192 | hostname | begin_snapshot | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | 1.14x [0.889,1.25] | 0.857x [0.541,1.02] | 49.9x [36.5,50.5] | 59x [26.3,64.1] | legacy (some), resident (some) |
| 8192 | hostname | close_environment | 7.194 | 7.273 | 44.303 | 44.263 | 1.897 | 0.977x [0.951,1.18] | 0.988x [0.975,1.06] | 0.261x [0.246,0.332] | 0.044x [0.0425,0.0447] | legacy (some) |
| 8192 | hostname | create_environment | 3.774 | 3.578 | 437.944 | 430.803 | 0.485 | 1.12x [0.842,1.15] | 0.996x [0.989,1.05] | 0.128x [0.0983,0.154] | 0.00114x [0.000736,0.00128] | legacy (some) |
| 8192 | hostname | drop_new_lookup | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.608x [0.254,0.65] | 0.579x [0.559,0.627] | 0.135x [0.0978,0.146] | 0.142x [0.116,0.193] | legacy (all), resident (all) |
| 8192 | hostname | drop_old_lookup | 0.000 | 0.000 | 0.000 | 0.112 | 0.000 | 0.271x [0.222,0.333] | 0.00067x [0.000628,0.000715] | 0.143x [0.136,0.185] | 0.000269x [0.000248,0.000447] | legacy (all), resident (all) |
| 8192 | hostname | final_validation | 2.579 | 0.757 | 2.852 | 0.840 | 1.597 | 3.39x [3.37,3.53] | 3.37x [3.35,3.42] | 2.11x [2.09,2.11] | 1.9x [1.87,1.91] | None observed |
| 8192 | hostname | fixture_load | 7.662 | 7.825 | 20.266 | 19.561 | 7.576 | 0.981x [0.962,1.03] | 0.971x [0.946,1.04] | 0.968x [0.933,1] | 0.391x [0.183,0.407] | resident (some) |
| 8192 | hostname | maintenance_before_pin | 0.002 | 0.002 | 1.190 | 1.173 | 0.001 | 0.913x [0.877,1.09] | 1.01x [1.01,1.05] | 0.603x [0.495,0.675] | 0.00113x [0.000932,0.00116] | legacy (some) |
| 8192 | hostname | prepare_after_reopen | 0.001 | 1.014 | 0.003 | 1.682 | 0.000 | 0.00108x [0.00104,0.00109] | 0.0019x [0.00154,0.00203] | 4.07e-05x [3.93e-05,6.02e-05] | 2.44e-05x [2.38e-05,3.6e-05] | legacy (all), resident (all) |
| 8192 | hostname | prepare_final_validation | 0.001 | 0.978 | 0.001 | 1.179 | 0.000 | 0.0014x [0.0013,0.00172] | 0.001x [0.000857,0.00103] | 0.000315x [0.000308,0.000329] | 0.000263x [0.00026,0.000271] | legacy (all), resident (all) |
| 8192 | hostname | prepare_lookup | 0.001 | 0.980 | 0.001 | 1.501 | 0.000 | 0.00108x [0.000979,0.00119] | 0.00055x [0.000541,0.000842] | 6.16e-05x [5.03e-05,7.15e-05] | 4e-05x [3.34e-05,4.37e-05] | legacy (all), resident (all) |
| 8192 | hostname | rebuild_after_pinned_writes | 0.002 | 0.978 | 0.001 | 1.169 | 0.002 | 0.00164x [0.00121,0.00187] | 0.000892x [0.000784,0.000986] | 0.00161x [0.00151,0.00165] | 0.00126x [0.00117,0.0014] | legacy (all), resident (all) |
| 8192 | hostname | release_old_snapshot | 0.051 | 0.051 | 0.060 | 0.060 | 0.000 | 0.997x [0.989,1] | 0.995x [0.963,1] | 0.0035x [0.00313,0.00436] | 0.00297x [0.00266,0.00369] | None observed |
| 8192 | hostname | reopen_database_handle | N/A | N/A | N/A | N/A | 0.008 | N/A | N/A | N/A | N/A | None observed |
| 8192 | hostname | reopen_environment | 63.523 | 63.742 | 398.857 | 403.108 | 0.258 | 0.997x [0.988,1.02] | 0.989x [0.975,0.989] | 0.00405x [0.00399,0.00467] | 0.000649x [0.000605,0.000722] | None observed |
| 8192 | hostname | preparation / total_snapshot_prepare | 0.003 | 0.982 | 1.192 | 2.681 | 0.005 | 0.00331x [0.003,0.00345] | 0.445x [0.426,0.461] | 0.00502x [0.00445,0.00566] | 0.00178x [0.00162,0.00211] | legacy (all), resident (all) |
| 100000 | fixed16 | begin_snapshot | 0.000 | 0.000 | 0.000 | 0.000 | 0.007 | 1.12x [0.468,1.25] | 1x [0.75,1] | 84.3x [38.1,89.3] | 96.3x [81.5,102] | legacy (some), resident (some) |
| 100000 | fixed16 | close_environment | 19.169 | 20.087 | 68.660 | 68.815 | 7.211 | 0.93x [0.891,0.989] | 1x [0.987,1] | 0.335x [0.325,0.441] | 0.104x [0.0948,0.129] | legacy (some) |
| 100000 | fixed16 | create_environment | 3.855 | 3.501 | 441.204 | 431.808 | 0.496 | 1.1x [0.988,1.13] | 1x [1,1.05] | 0.142x [0.125,0.15] | 0.00113x [0.00113,0.00121] | None observed |
| 100000 | fixed16 | drop_new_lookup | 0.001 | 1.494 | 0.001 | 1.230 | 0.000 | 0.000496x [0.000356,0.000566] | 0.000538x [0.000375,0.000584] | 0.000202x [8.74e-05,0.000221] | 0.00022x [0.000118,0.000274] | legacy (all), resident (all) |
| 100000 | fixed16 | drop_old_lookup | 0.000 | 1.198 | 0.000 | 1.167 | 0.000 | 5.91e-05x [4.74e-05,8.34e-05] | 6.29e-05x [5.88e-05,0.000429] | 2.5e-05x [2.37e-05,2.53e-05] | 2.57e-05x [2.52e-05,3.15e-05] | legacy (all), resident (all) |
| 100000 | fixed16 | final_validation | 39.046 | 11.706 | 28.756 | 11.814 | 25.410 | 3.34x [3.3,3.6] | 2.43x [2.4,2.48] | 2.17x [2.15,3.44] | 2.22x [2.15,3.19] | None observed |
| 100000 | fixed16 | fixture_load | 33.299 | 34.877 | 138.854 | 132.237 | 47.942 | 0.955x [0.904,0.961] | 1.05x [0.994,1.11] | 1.37x [1.33,1.41] | 0.363x [0.358,0.375] | legacy (some) |
| 100000 | fixed16 | maintenance_before_pin | 0.003 | 0.003 | 4.989 | 5.148 | 0.002 | 0.97x [0.744,1.11] | 0.962x [0.962,1.05] | 0.573x [0.426,0.611] | 0.00029x [0.000284,0.000302] | legacy (some) |
| 100000 | fixed16 | prepare_after_reopen | 0.001 | 13.429 | 0.003 | 17.535 | 0.000 | 8.28e-05x [7.41e-05,8.51e-05] | 0.000198x [0.000196,0.000203] | 3.83e-06x [2.98e-06,7.25e-06] | 2.97e-06x [2.24e-06,5.76e-06] | legacy (all), resident (all) |
| 100000 | fixed16 | prepare_final_validation | 0.002 | 17.743 | 0.002 | 17.976 | 0.000 | 9.88e-05x [8.8e-05,0.0001] | 8.65e-05x [6.48e-05,0.000101] | 1.79e-05x [1.18e-05,2e-05] | 1.79e-05x [1.19e-05,1.83e-05] | legacy (all), resident (all) |
| 100000 | fixed16 | prepare_lookup | 0.002 | 19.125 | 0.001 | 19.412 | 0.000 | 8.8e-05x [5.7e-05,9.34e-05] | 4.57e-05x [4.26e-05,0.000725] | 3.77e-06x [2.31e-06,7.32e-06] | 3.55e-06x [2.59e-06,7.21e-06] | legacy (all), resident (all) |
| 100000 | fixed16 | rebuild_after_pinned_writes | 0.003 | 19.791 | 0.003 | 19.050 | 0.004 | 0.000141x [0.0001,0.000154] | 0.000133x [0.000131,0.000172] | 0.000197x [0.000121,0.000261] | 0.000205x [0.000186,0.000258] | legacy (all), resident (all) |
| 100000 | fixed16 | release_old_snapshot | 0.120 | 0.112 | 0.368 | 0.374 | 0.000 | 1.07x [1.01,1.1] | 0.988x [0.97,0.993] | 0.00258x [0.00197,0.00285] | 0.00077x [0.000594,0.000855] | None observed |
| 100000 | fixed16 | reopen_database_handle | N/A | N/A | N/A | N/A | 0.011 | N/A | N/A | N/A | N/A | None observed |
| 100000 | fixed16 | reopen_environment | 202.861 | 205.030 | 755.329 | 735.997 | 0.263 | 0.992x [0.975,0.994] | 1x [1,1.03] | 0.00127x [0.00125,0.00147] | 0.000359x [0.000349,0.000399] | None observed |
| 100000 | fixed16 | preparation / total_snapshot_prepare | 0.004 | 19.128 | 4.990 | 24.561 | 0.008 | 0.000232x [0.000182,0.000236] | 0.204x [0.202,0.211] | 0.000436x [0.000374,0.000469] | 0.00034x [0.00033,0.000353] | legacy (all), resident (all) |
| 100000 | hostname | begin_snapshot | 0.000 | 0.000 | 0.000 | 0.000 | 0.007 | 1.29x [1.12,1.43] | 1x [0.875,1.13] | 98.5x [90,102] | 102x [86.2,103] | resident (some) |
| 100000 | hostname | close_environment | 23.056 | 23.818 | 69.527 | 69.636 | 7.728 | 0.951x [0.775,1.03] | 1.02x [0.995,1.07] | 0.318x [0.264,0.324] | 0.111x [0.109,0.122] | legacy (some) |
| 100000 | hostname | create_environment | 2.781 | 2.779 | 452.990 | 441.658 | 0.302 | 0.968x [0.948,1.16] | 1.02x [0.98,1.03] | 0.109x [0.0923,0.126] | 0.000678x [0.0006,0.000743] | legacy (some) |
| 100000 | hostname | drop_new_lookup | 0.001 | 0.002 | 0.001 | 1.218 | 0.000 | 0.488x [0.23,0.868] | 0.000781x [0.00068,0.000833] | 0.123x [0.11,0.175] | 0.00023x [0.000221,0.000293] | legacy (all), resident (all) |
| 100000 | hostname | drop_old_lookup | 0.000 | 0.000 | 0.000 | 1.145 | 0.000 | 0.245x [0.162,0.258] | 5.91e-05x [5.73e-05,0.000175] | 0.0982x [0.0809,0.111] | 2.86e-05x [2.5e-05,5.24e-05] | legacy (all), resident (all) |
| 100000 | hostname | final_validation | 39.867 | 15.769 | 49.436 | 17.112 | 25.820 | 2.53x [2.08,2.54] | 2.91x [2.74,3.09] | 1.63x [1.32,1.7] | 1.52x [1.48,1.54] | None observed |
| 100000 | hostname | fixture_load | 37.860 | 38.301 | 244.904 | 246.946 | 50.740 | 0.988x [0.98,1.04] | 0.986x [0.979,1.05] | 1.32x [1.31,1.39] | 0.203x [0.203,0.217] | None observed |
| 100000 | hostname | maintenance_before_pin | 0.003 | 0.003 | 10.809 | 10.610 | 0.002 | 1.07x [1.01,1.08] | 1.01x [0.988,1.02] | 0.587x [0.573,0.638] | 0.000146x [0.000144,0.000163] | None observed |
| 100000 | hostname | prepare_after_reopen | 0.001 | 16.095 | 0.004 | 21.459 | 0.000 | 6.87e-05x [6.31e-05,8.34e-05] | 0.000171x [0.000171,0.00019] | 2.49e-06x [2.47e-06,2.52e-06] | 1.86e-06x [1.85e-06,1.87e-06] | legacy (all), resident (all) |
| 100000 | hostname | prepare_final_validation | 0.002 | 18.258 | 0.001 | 25.510 | 0.000 | 9.13e-05x [7.74e-05,0.000101] | 5.23e-05x [5.1e-05,7.26e-05] | 1.82e-05x [1.32e-05,2.06e-05] | 1.34e-05x [9.45e-06,1.73e-05] | legacy (all), resident (all) |
| 100000 | hostname | prepare_lookup | 0.002 | 21.394 | 0.001 | 25.170 | 0.000 | 8.66e-05x [6.25e-05,0.000116] | 6.05e-05x [4.82e-05,0.000488] | 3.27e-06x [3.21e-06,3.82e-06] | 2.85e-06x [2.75e-06,3.18e-06] | legacy (all), resident (all) |
| 100000 | hostname | rebuild_after_pinned_writes | 0.003 | 20.222 | 0.003 | 25.773 | 0.005 | 0.000171x [0.000156,0.000188] | 0.000125x [0.000115,0.00014] | 0.000234x [0.000225,0.000259] | 0.000193x [0.000177,0.000203] | legacy (all), resident (all) |
| 100000 | hostname | release_old_snapshot | 0.114 | 0.117 | 0.701 | 0.690 | 0.000 | 0.97x [0.963,1.02] | 1.03x [0.968,1.12] | 0.00304x [0.00256,0.00332] | 0.000523x [0.000428,0.000545] | None observed |
| 100000 | hostname | reopen_database_handle | N/A | N/A | N/A | N/A | 0.011 | N/A | N/A | N/A | N/A | None observed |
| 100000 | hostname | reopen_environment | 233.702 | 235.213 | 917.698 | 908.198 | 0.260 | 0.994x [0.977,1] | 1.02x [0.994,1.06] | 0.0011x [0.00108,0.00113] | 0.000286x [0.000285,0.000288] | None observed |
| 100000 | hostname | preparation / total_snapshot_prepare | 0.005 | 21.397 | 10.810 | 35.974 | 0.009 | 0.000225x [0.0002,0.00025] | 0.304x [0.291,0.308] | 0.000403x [0.000397,0.000437] | 0.00025x [0.000235,0.000254] | legacy (all), resident (all) |
| 1000000 | fixed16 | begin_snapshot | 0.000 | 0.000 | 0.000 | 0.000 | 0.007 | 1.12x [1,1.56] | 1.38x [1.01,1.44] | 90.7x [82.5,97.4] | 92.8x [90.7,111] | None observed |
| 1000000 | fixed16 | close_environment | 31.601 | 33.999 | 280.536 | 284.596 | 15.083 | 0.918x [0.891,0.959] | 0.985x [0.979,0.998] | 0.425x [0.413,0.452] | 0.053x [0.0466,0.054] | legacy (some) |
| 1000000 | fixed16 | create_environment | 2.991 | 2.599 | 606.114 | 598.412 | 0.301 | 1.15x [1.01,1.5] | 1.02x [0.971,1.03] | 0.102x [0.086,0.129] | 0.000496x [0.000374,0.000551] | None observed |
| 1000000 | fixed16 | drop_new_lookup | 0.001 | 10.077 | 0.001 | 10.197 | 0.000 | 0.000117x [9.87e-05,0.000122] | 5.71e-05x [5.54e-05,0.000116] | 3.42e-05x [2.77e-05,4.28e-05] | 2.87e-05x [2.63e-05,4.23e-05] | legacy (all), resident (all) |
| 1000000 | fixed16 | drop_old_lookup | 0.000 | 10.294 | 0.000 | 10.621 | 0.000 | 1.4e-05x [6.9e-06,1.73e-05] | 1.42e-05x [1.42e-05,2.37e-05] | 3e-06x [2.89e-06,3.89e-06] | 2.83e-06x [2.64e-06,3.77e-06] | legacy (all), resident (all) |
| 1000000 | fixed16 | final_validation | 471.690 | 212.471 | 365.664 | 219.582 | 288.506 | 2.23x [2.18,2.31] | 1.67x [1.66,2.02] | 1.36x [1.33,1.42] | 1.31x [1.31,1.35] | None observed |
| 1000000 | fixed16 | fixture_load | 249.657 | 248.990 | 1,382.870 | 1,375.674 | 462.703 | 0.998x [0.988,1.04] | 1.01x [0.986,1.02] | 1.86x [1.84,1.87] | 0.335x [0.331,0.338] | None observed |
| 1000000 | fixed16 | maintenance_before_pin | 0.003 | 0.003 | 36.953 | 37.063 | 0.001 | 1.04x [0.927,1.07] | 0.996x [0.992,0.997] | 0.554x [0.504,0.63] | 3.74e-05x [3.54e-05,4.25e-05] | legacy (some) |
| 1000000 | fixed16 | prepare_after_reopen | 0.001 | 274.853 | 0.004 | 280.855 | 0.000 | 5.43e-06x [3.74e-06,5.43e-06] | 1.39e-05x [1.31e-05,1.4e-05] | 1.46e-07x [1.45e-07,1.85e-07] | 1.42e-07x [1.39e-07,1.78e-07] | legacy (all), resident (all) |
| 1000000 | fixed16 | prepare_final_validation | 0.002 | 276.749 | 0.002 | 284.073 | 0.000 | 7.16e-06x [5.87e-06,7.68e-06] | 7.37e-06x [6.2e-06,7.46e-06] | 1.71e-06x [1.27e-06,1.78e-06] | 1.68e-06x [1.24e-06,1.73e-06] | legacy (all), resident (all) |
| 1000000 | fixed16 | prepare_lookup | 0.002 | 273.735 | 0.001 | 285.536 | 0.000 | 5.77e-06x [5.59e-06,7.39e-06] | 5.19e-06x [4.57e-06,6.21e-06] | 2.56e-07x [2.52e-07,6.24e-07] | 2.45e-07x [2.42e-07,5.99e-07] | legacy (all), resident (all) |
| 1000000 | fixed16 | rebuild_after_pinned_writes | 0.004 | 274.573 | 0.004 | 283.635 | 0.006 | 1.39e-05x [1.3e-05,1.57e-05] | 1.3e-05x [1.26e-05,1.36e-05] | 2.25e-05x [2.07e-05,2.32e-05] | 2.22e-05x [2e-05,2.23e-05] | legacy (all), resident (all) |
| 1000000 | fixed16 | release_old_snapshot | 0.108 | 0.109 | 3.444 | 3.565 | 0.000 | 1.09x [0.951,1.19] | 0.966x [0.965,0.974] | 0.00467x [0.00333,0.00609] | 0.000129x [0.000101,0.000188] | legacy (some) |
| 1000000 | fixed16 | reopen_database_handle | N/A | N/A | N/A | N/A | 0.012 | N/A | N/A | N/A | N/A | None observed |
| 1000000 | fixed16 | reopen_environment | 342.118 | 342.094 | 3,547.098 | 3,467.196 | 0.254 | 1x [0.99,1.01] | 1.02x [1.01,1.03] | 0.000742x [0.000737,0.000781] | 7.38e-05x [7.28e-05,7.61e-05] | None observed |
| 1000000 | fixed16 | preparation / total_snapshot_prepare | 0.004 | 273.738 | 36.955 | 322.561 | 0.009 | 1.59e-05x [1.49e-05,1.72e-05] | 0.114x [0.114,0.115] | 3.28e-05x [3.21e-05,3.38e-05] | 2.78e-05x [2.72e-05,2.87e-05] | legacy (all), resident (all) |
| 1000000 | hostname | begin_snapshot | 0.000 | 0.000 | 0.000 | 0.000 | 0.007 | 1x [1,1.12] | 1x [0.389,2.48] | 83.8x [69.3,93.2] | 77.8x [46.6,82.7] | resident (some) |
| 1000000 | hostname | close_environment | 38.120 | 39.943 | 279.664 | 288.671 | 18.211 | 0.954x [0.89,0.96] | 0.973x [0.925,0.997] | 0.452x [0.432,0.498] | 0.0611x [0.0595,0.0709] | legacy (some), resident (some) |
| 1000000 | hostname | create_environment | 2.595 | 2.855 | 614.909 | 596.850 | 0.276 | 0.878x [0.828,1.18] | 1.03x [0.984,1.05] | 0.103x [0.0793,0.114] | 0.00046x [0.00038,0.000546] | legacy (some) |
| 1000000 | hostname | drop_new_lookup | 0.001 | 10.537 | 0.001 | 9.205 | 0.000 | 9.43e-05x [7.61e-05,0.000123] | 0.000108x [7.81e-05,0.000123] | 2.88e-05x [2.86e-05,3.72e-05] | 3.43e-05x [2.86e-05,4.46e-05] | legacy (all), resident (all) |
| 1000000 | hostname | drop_old_lookup | 0.000 | 10.550 | 0.000 | 10.290 | 0.000 | 7.59e-06x [5.69e-06,1.2e-05] | 1.47e-05x [7.81e-06,1.51e-05] | 2.84e-06x [2.25e-06,2.84e-06] | 2.92e-06x [2.82e-06,2.93e-06] | legacy (all), resident (all) |
| 1000000 | hostname | final_validation | 484.997 | 251.966 | 540.695 | 269.856 | 302.079 | 1.92x [1.89,1.94] | 2.01x [1.96,2.03] | 1.2x [1.19,1.2] | 1.12x [1.12,1.13] | None observed |
| 1000000 | hostname | fixture_load | 342.755 | 351.915 | 2,537.947 | 2,536.923 | 535.717 | 0.972x [0.947,0.994] | 0.998x [0.998,1.01] | 1.54x [1.48,1.54] | 0.211x [0.211,0.215] | legacy (some) |
| 1000000 | hostname | maintenance_before_pin | 0.003 | 0.003 | 94.560 | 94.199 | 0.001 | 0.957x [0.929,1.01] | 1.01x [0.979,1.01] | 0.518x [0.509,0.529] | 1.45e-05x [1.45e-05,1.55e-05] | legacy (some) |
| 1000000 | hostname | prepare_after_reopen | 0.001 | 313.746 | 0.003 | 352.222 | 0.000 | 3.19e-06x [3.16e-06,4.28e-06] | 9.48e-06x [9.36e-06,1.01e-05] | 1.27e-07x [1.18e-07,1.29e-07] | 1.14e-07x [1.13e-07,1.15e-07] | legacy (all), resident (all) |
| 1000000 | hostname | prepare_final_validation | 0.002 | 319.487 | 0.002 | 355.175 | 0.000 | 5.05e-06x [4.56e-06,6.22e-06] | 4.75e-06x [4.65e-06,5.6e-06] | 1.51e-06x [1.22e-06,1.68e-06] | 1.32e-06x [1.1e-06,1.61e-06] | legacy (all), resident (all) |
| 1000000 | hostname | prepare_lookup | 0.002 | 313.912 | 0.002 | 345.515 | 0.000 | 5.55e-06x [3.98e-06,5.87e-06] | 4.81e-06x [3.77e-06,4.14e-05] | 2.18e-07x [1.91e-07,2.25e-07] | 2.03e-07x [1.69e-07,2.03e-07] | legacy (all), resident (all) |
| 1000000 | hostname | rebuild_after_pinned_writes | 0.004 | 317.905 | 0.004 | 349.222 | 0.006 | 1.29e-05x [8.83e-06,1.45e-05] | 1.11e-05x [1.08e-05,1.15e-05] | 1.85e-05x [1.85e-05,1.88e-05] | 1.69e-05x [1.67e-05,1.7e-05] | legacy (all), resident (all) |
| 1000000 | hostname | release_old_snapshot | 0.114 | 0.109 | 5.539 | 5.737 | 0.001 | 1.03x [0.832,1.09] | 0.973x [0.945,0.983] | 0.00426x [0.00379,0.00496] | 8.89e-05x [8.09e-05,9.43e-05] | legacy (some), resident (some) |
| 1000000 | hostname | reopen_database_handle | N/A | N/A | N/A | N/A | 0.012 | N/A | N/A | N/A | N/A | None observed |
| 1000000 | hostname | reopen_environment | 410.360 | 411.678 | 5,362.109 | 5,370.229 | 0.269 | 0.996x [0.995,0.997] | 0.993x [0.97,1] | 0.000651x [0.000634,0.000654] | 4.86e-05x [4.85e-05,5.02e-05] | None observed |
| 1000000 | hostname | preparation / total_snapshot_prepare | 0.004 | 313.915 | 94.564 | 439.714 | 0.008 | 1.35e-05x [1.27e-05,1.49e-05] | 0.216x [0.209,0.216] | 2.7e-05x [2.63e-05,3.06e-05] | 1.88e-05x [1.87e-05,2.23e-05] | legacy (all), resident (all) |

## Actual per-process RSS at every checkpoint

MiB of Linux VmRSS. All input/query/model storage, mappings and allocator retention remain included. The after_full_validation checkpoint follows full source reads for every arm; after_prepare alone also reflects hash's eager source-page touching. Two-generations checkpoints retain both old and new lookup pairs. Dropping a lookup releases its table; the caller's old transaction continues retaining source pages until separately dropped. Allocators may retain freed storage. All other status counters, including RssAnon/RssFile/VmHWM/VmPTE, are retained in FULL_TABLE.csv and summary.json. VmHWM is cumulative, not isolated index peak usage.

| Keys | Key type | Phase / operation | Legacy native | Legacy hash | Resident native | Resident hash | LMDB | Legacy hash/native | Resident hash/native | Legacy hash/LMDB | Resident hash/LMDB | >5% hash cost increase vs native |
|---:|---|---|---:|---:|---:|---:|---:|---|---|---|---|---|
| 8192 | fixed16 | after_full_validation / vmrss_kib | 26.180 | 26.281 | 40.367 | 40.977 | 6.184 | 0.996x [0.994,0.997] | 0.985x [0.983,0.986] | 0.235x [0.233,0.236] | 0.151x [0.15,0.151] | None observed |
| 8192 | fixed16 | after_old_lookup_drop / vmrss_kib | 48.590 | 49.246 | 42.832 | 42.816 | 28.516 | 0.987x [0.987,0.987] | 1x [0.998,1] | 0.579x [0.576,0.579] | 0.666x [0.665,0.666] | None observed |
| 8192 | fixed16 | after_old_snapshot_release / vmrss_kib | 48.590 | 49.246 | 42.832 | 42.816 | 28.516 | 0.987x [0.987,0.987] | 1x [0.998,1] | 0.579x [0.576,0.579] | 0.666x [0.665,0.666] | None observed |
| 8192 | fixed16 | after_pinned_writes / vmrss_kib | 48.590 | 49.246 | 42.832 | 43.441 | 28.035 | 0.987x [0.987,0.987] | 0.986x [0.984,0.987] | 0.569x [0.567,0.57] | 0.645x [0.644,0.645] | None observed |
| 8192 | fixed16 | after_prepare / vmrss_kib | 26.180 | 26.281 | 40.367 | 40.977 | 6.184 | 0.996x [0.994,0.997] | 0.985x [0.983,0.986] | 0.235x [0.233,0.236] | 0.151x [0.15,0.151] | None observed |
| 8192 | fixed16 | after_reopen_full_validation / vmrss_kib | 68.555 | 69.211 | 42.234 | 42.840 | 9.105 | 0.991x [0.991,0.991] | 0.986x [0.983,0.987] | 0.132x [0.129,0.132] | 0.213x [0.209,0.214] | None observed |
| 8192 | fixed16 | before_prepare / vmrss_kib | 26.180 | 26.215 | 40.367 | 40.348 | 6.184 | 0.998x [0.998,1] | 1x [1,1] | 0.236x [0.234,0.237] | 0.153x [0.153,0.154] | None observed |
| 8192 | fixed16 | closed_environment / vmrss_kib | 8.273 | 8.930 | 8.875 | 8.859 | 7.801 | 0.927x [0.926,0.927] | 1x [0.991,1.01] | 0.873x [0.871,0.874] | 0.88x [0.877,0.883] | legacy (all) |
| 8192 | fixed16 | fixture_ready / vmrss_kib | 3.145 | 3.133 | 3.133 | 3.059 | 3.148 | 1.01x [0.97,1.03] | 1.02x [0.976,1.02] | 0.999x [0.978,1.03] | 1.02x [0.976,1.03] | None observed |
| 8192 | fixed16 | loaded_source_only / vmrss_kib | 5.008 | 5.027 | 38.938 | 38.922 | 3.996 | 0.993x [0.993,1.01] | 1x [1,1] | 0.799x [0.795,0.801] | 0.103x [0.102,0.104] | None observed |
| 8192 | fixed16 | pinned_batch1 / vmrss_kib | 29.242 | 29.898 | 42.832 | 43.441 | 9.254 | 0.978x [0.978,0.978] | 0.986x [0.984,0.987] | 0.31x [0.307,0.31] | 0.213x [0.212,0.213] | None observed |
| 8192 | fixed16 | pinned_batch1000 / vmrss_kib | 48.590 | 49.246 | 42.832 | 43.441 | 28.035 | 0.987x [0.987,0.987] | 0.986x [0.984,0.987] | 0.569x [0.567,0.57] | 0.645x [0.644,0.645] | None observed |
| 8192 | fixed16 | pinned_batch64 / vmrss_kib | 35.148 | 35.805 | 42.832 | 43.441 | 14.824 | 0.982x [0.982,0.982] | 0.986x [0.984,0.987] | 0.414x [0.411,0.415] | 0.341x [0.34,0.341] | None observed |
| 8192 | fixed16 | process_start_before_fixture / vmrss_kib | 2.316 | 2.316 | 2.375 | 2.312 | 2.375 | 1x [0.926,1.03] | 1x [0.95,1.04] | 0.998x [0.953,1.03] | 1x [0.952,1.03] | legacy (some), resident (some) |
| 8192 | fixed16 | resident_ballast_ready / vmrss_kib | 3.148 | 3.137 | 3.137 | 3.062 | 3.152 | 1.01x [0.97,1.03] | 1.02x [0.976,1.02] | 0.999x [0.978,1.03] | 1.02x [0.976,1.03] | None observed |
| 8192 | fixed16 | two_generations / vmrss_kib | 48.590 | 49.246 | 42.832 | 43.441 | 28.035 | 0.987x [0.987,0.987] | 0.986x [0.984,0.987] | 0.569x [0.567,0.57] | 0.645x [0.644,0.645] | None observed |
| 8192 | fixed16 | unpinned_after_batch1 / vmrss_kib | 49.137 | 49.793 | 42.832 | 42.816 | 28.516 | 0.987x [0.987,0.987] | 1x [0.998,1] | 0.573x [0.57,0.573] | 0.666x [0.665,0.666] | None observed |
| 8192 | fixed16 | unpinned_after_batch1000 / vmrss_kib | 68.555 | 69.211 | 42.832 | 42.816 | 28.516 | 0.991x [0.991,0.991] | 1x [0.998,1] | 0.412x [0.41,0.413] | 0.666x [0.665,0.666] | None observed |
| 8192 | fixed16 | unpinned_after_batch64 / vmrss_kib | 55.105 | 55.762 | 42.832 | 42.816 | 28.516 | 0.988x [0.988,0.988] | 1x [0.998,1] | 0.511x [0.508,0.512] | 0.666x [0.665,0.666] | None observed |
| 8192 | fixed16 | unpinned_before_batch1 / vmrss_kib | 5.656 | 5.691 | 38.953 | 38.938 | 4.562 | 0.994x [0.994,0.994] | 1x [1,1] | 0.802x [0.797,0.802] | 0.117x [0.116,0.118] | None observed |
| 8192 | fixed16 | unpinned_before_batch1000 / vmrss_kib | 26.180 | 26.215 | 40.273 | 40.254 | 6.184 | 0.998x [0.998,1] | 1x [1,1] | 0.236x [0.234,0.237] | 0.154x [0.153,0.154] | None observed |
| 8192 | fixed16 | unpinned_before_batch64 / vmrss_kib | 12.152 | 12.160 | 39.273 | 39.258 | 5.105 | 0.999x [0.998,1] | 1x [1,1] | 0.421x [0.413,0.423] | 0.13x [0.129,0.131] | None observed |
| 8192 | fixed16 | warm_hit / vmrss_kib | 28.547 | 29.203 | 42.691 | 43.301 | 9.039 | 0.978x [0.978,0.978] | 0.986x [0.984,0.987] | 0.31x [0.307,0.31] | 0.209x [0.208,0.209] | None observed |
| 8192 | fixed16 | warm_hot90 / vmrss_kib | 28.562 | 29.223 | 42.711 | 43.316 | 9.059 | 0.978x [0.977,0.978] | 0.986x [0.984,0.987] | 0.31x [0.308,0.311] | 0.209x [0.208,0.209] | None observed |
| 8192 | fixed16 | warm_miss / vmrss_kib | 28.547 | 29.207 | 42.695 | 43.305 | 9.039 | 0.978x [0.977,0.978] | 0.986x [0.984,0.987] | 0.31x [0.307,0.31] | 0.209x [0.208,0.209] | None observed |
| 8192 | fixed16 | warm_mixed50 / vmrss_kib | 28.559 | 29.215 | 42.703 | 43.312 | 9.051 | 0.978x [0.978,0.978] | 0.986x [0.984,0.987] | 0.31x [0.308,0.31] | 0.209x [0.208,0.209] | None observed |
| 8192 | hostname | after_full_validation / vmrss_kib | 33.652 | 33.613 | 41.168 | 41.836 | 7.344 | 1x [0.983,1] | 0.983x [0.982,0.985] | 0.217x [0.215,0.221] | 0.175x [0.174,0.178] | None observed |
| 8192 | hostname | after_old_lookup_drop / vmrss_kib | 64.613 | 65.230 | 45.199 | 45.195 | 38.770 | 0.991x [0.99,0.991] | 1x [0.997,1] | 0.594x [0.594,0.597] | 0.858x [0.856,0.861] | None observed |
| 8192 | hostname | after_old_snapshot_release / vmrss_kib | 64.613 | 65.230 | 45.199 | 45.195 | 38.770 | 0.991x [0.99,0.991] | 1x [0.997,1] | 0.594x [0.594,0.597] | 0.858x [0.856,0.861] | None observed |
| 8192 | hostname | after_pinned_writes / vmrss_kib | 64.613 | 65.230 | 45.199 | 45.820 | 38.059 | 0.991x [0.99,0.991] | 0.987x [0.983,0.988] | 0.583x [0.583,0.586] | 0.831x [0.829,0.834] | None observed |
| 8192 | hostname | after_prepare / vmrss_kib | 33.652 | 33.613 | 41.168 | 41.836 | 7.344 | 1x [0.983,1] | 0.983x [0.982,0.985] | 0.217x [0.215,0.221] | 0.175x [0.174,0.178] | None observed |
| 8192 | hostname | after_reopen_full_validation / vmrss_kib | 91.336 | 91.973 | 44.426 | 45.043 | 11.348 | 0.994x [0.993,0.994] | 0.986x [0.983,0.988] | 0.123x [0.121,0.124] | 0.252x [0.248,0.254] | None observed |
| 8192 | hostname | before_prepare / vmrss_kib | 33.652 | 33.613 | 41.168 | 41.207 | 7.344 | 1x [0.983,1] | 0.998x [0.997,1] | 0.217x [0.215,0.221] | 0.178x [0.177,0.18] | None observed |
| 8192 | hostname | closed_environment / vmrss_kib | 10.559 | 11.145 | 11.078 | 11.074 | 10.281 | 0.949x [0.943,0.95] | 1x [0.987,1.01] | 0.923x [0.914,0.926] | 0.924x [0.923,0.938] | legacy (all) |
| 8192 | hostname | fixture_ready / vmrss_kib | 3.383 | 3.391 | 3.371 | 3.309 | 3.383 | 0.998x [0.976,1.02] | 1.02x [0.97,1.03] | 1x [0.992,1.01] | 1.02x [0.982,1.02] | None observed |
| 8192 | hostname | loaded_source_only / vmrss_kib | 5.500 | 5.480 | 39.293 | 39.285 | 4.465 | 0.999x [0.997,1.01] | 1x [0.996,1] | 0.809x [0.803,0.833] | 0.114x [0.112,0.116] | None observed |
| 8192 | hostname | pinned_batch1 / vmrss_kib | 38.480 | 39.074 | 45.199 | 45.820 | 12.367 | 0.985x [0.984,0.986] | 0.987x [0.983,0.988] | 0.316x [0.316,0.319] | 0.27x [0.27,0.272] | None observed |
| 8192 | hostname | pinned_batch1000 / vmrss_kib | 64.613 | 65.230 | 45.199 | 45.820 | 38.059 | 0.991x [0.99,0.991] | 0.987x [0.983,0.988] | 0.583x [0.583,0.586] | 0.831x [0.829,0.834] | None observed |
| 8192 | hostname | pinned_batch64 / vmrss_kib | 45.000 | 45.582 | 45.199 | 45.820 | 18.598 | 0.987x [0.986,0.988] | 0.987x [0.983,0.988] | 0.408x [0.407,0.411] | 0.406x [0.405,0.409] | None observed |
| 8192 | hostname | process_start_before_fixture / vmrss_kib | 2.316 | 2.324 | 2.309 | 2.316 | 2.363 | 0.998x [0.997,1.05] | 0.997x [0.946,1.03] | 1.02x [0.974,1.02] | 1x [0.976,1.02] | resident (some) |
| 8192 | hostname | resident_ballast_ready / vmrss_kib | 3.387 | 3.395 | 3.375 | 3.312 | 3.387 | 0.998x [0.976,1.02] | 1.02x [0.97,1.03] | 1x [0.992,1.01] | 1.02x [0.982,1.02] | None observed |
| 8192 | hostname | two_generations / vmrss_kib | 64.613 | 65.230 | 45.199 | 45.820 | 38.059 | 0.991x [0.99,0.991] | 0.987x [0.983,0.988] | 0.583x [0.583,0.586] | 0.831x [0.829,0.834] | None observed |
| 8192 | hostname | unpinned_after_batch1 / vmrss_kib | 65.160 | 65.777 | 45.199 | 45.195 | 38.770 | 0.991x [0.99,0.991] | 1x [0.997,1] | 0.589x [0.589,0.592] | 0.858x [0.856,0.861] | None observed |
| 8192 | hostname | unpinned_after_batch1000 / vmrss_kib | 91.336 | 91.973 | 45.199 | 45.195 | 38.770 | 0.994x [0.993,0.994] | 1x [0.997,1] | 0.422x [0.421,0.423] | 0.858x [0.856,0.861] | None observed |
| 8192 | hostname | unpinned_after_batch64 / vmrss_kib | 71.730 | 72.332 | 45.199 | 45.195 | 38.770 | 0.992x [0.991,0.992] | 1x [0.997,1] | 0.536x [0.536,0.537] | 0.858x [0.856,0.861] | None observed |
| 8192 | hostname | unpinned_before_batch1 / vmrss_kib | 6.164 | 6.125 | 39.293 | 39.285 | 5.219 | 1.01x [0.997,1.01] | 1x [0.996,1] | 0.847x [0.842,0.871] | 0.133x [0.131,0.136] | None observed |
| 8192 | hostname | unpinned_before_batch1000 / vmrss_kib | 33.652 | 33.613 | 41.168 | 41.207 | 7.344 | 1x [0.983,1] | 0.998x [0.997,1] | 0.217x [0.215,0.221] | 0.178x [0.177,0.18] | None observed |
| 8192 | hostname | unpinned_before_batch64 / vmrss_kib | 13.227 | 13.207 | 39.613 | 39.605 | 5.824 | 1.01x [0.999,1.01] | 1x [0.997,1] | 0.44x [0.44,0.449] | 0.147x [0.147,0.15] | None observed |
| 8192 | hostname | warm_hit / vmrss_kib | 37.785 | 38.379 | 45.047 | 45.664 | 12.148 | 0.985x [0.983,0.985] | 0.987x [0.983,0.988] | 0.316x [0.316,0.319] | 0.266x [0.266,0.268] | None observed |
| 8192 | hostname | warm_hot90 / vmrss_kib | 37.801 | 38.395 | 45.066 | 45.680 | 12.168 | 0.985x [0.983,0.985] | 0.987x [0.983,0.988] | 0.317x [0.317,0.319] | 0.266x [0.266,0.269] | None observed |
| 8192 | hostname | warm_miss / vmrss_kib | 37.785 | 38.379 | 45.051 | 45.668 | 12.152 | 0.985x [0.983,0.985] | 0.986x [0.983,0.988] | 0.317x [0.316,0.319] | 0.266x [0.266,0.268] | None observed |
| 8192 | hostname | warm_mixed50 / vmrss_kib | 37.793 | 38.387 | 45.059 | 45.676 | 12.160 | 0.985x [0.983,0.985] | 0.986x [0.983,0.988] | 0.317x [0.317,0.319] | 0.266x [0.266,0.269] | None observed |
| 100000 | fixed16 | after_full_validation / vmrss_kib | 114.656 | 124.641 | 65.512 | 75.508 | 30.320 | 0.92x [0.92,0.939] | 0.868x [0.866,0.868] | 0.243x [0.243,0.248] | 0.402x [0.401,0.402] | legacy (all), resident (all) |
| 100000 | fixed16 | after_old_lookup_drop / vmrss_kib | 204.410 | 204.332 | 67.008 | 67.004 | 119.168 | 1x [1,1] | 1x [0.998,1] | 0.584x [0.583,0.584] | 1.78x [1.78,1.78] | None observed |
| 100000 | fixed16 | after_old_snapshot_release / vmrss_kib | 204.410 | 204.332 | 67.008 | 67.004 | 119.168 | 1x [1,1] | 1x [0.998,1] | 0.584x [0.583,0.584] | 1.78x [1.78,1.78] | None observed |
| 100000 | fixed16 | after_pinned_writes / vmrss_kib | 204.410 | 214.332 | 67.008 | 77.004 | 116.324 | 0.953x [0.953,0.956] | 0.87x [0.868,0.871] | 0.543x [0.542,0.544] | 1.51x [1.51,1.51] | resident (all) |
| 100000 | fixed16 | after_prepare / vmrss_kib | 114.656 | 124.641 | 65.512 | 75.508 | 30.320 | 0.92x [0.92,0.939] | 0.868x [0.866,0.868] | 0.243x [0.243,0.248] | 0.402x [0.401,0.402] | legacy (all), resident (all) |
| 100000 | fixed16 | after_reopen_full_validation / vmrss_kib | 294.484 | 301.246 | 66.047 | 79.664 | 40.133 | 0.977x [0.976,0.979] | 0.829x [0.825,0.835] | 0.133x [0.132,0.135] | 0.504x [0.496,0.51] | resident (all) |
| 100000 | fixed16 | before_prepare / vmrss_kib | 114.656 | 114.641 | 65.469 | 65.508 | 30.320 | 1x [1,1.02] | 1x [0.998,1] | 0.265x [0.264,0.27] | 0.463x [0.462,0.464] | None observed |
| 100000 | fixed16 | closed_environment / vmrss_kib | 19.574 | 26.547 | 17.555 | 26.051 | 20.754 | 0.737x [0.736,0.755] | 0.674x [0.666,0.675] | 0.783x [0.781,0.799] | 0.797x [0.791,0.798] | legacy (all), resident (all) |
| 100000 | fixed16 | fixture_ready / vmrss_kib | 10.863 | 10.848 | 10.832 | 10.809 | 10.836 | 1x [1,1] | 1x [0.991,1.01] | 0.991x [0.991,1] | 0.995x [0.992,1.01] | None observed |
| 100000 | fixed16 | loaded_source_only / vmrss_kib | 17.883 | 17.914 | 62.141 | 62.195 | 16.969 | 1x [0.998,1] | 1x [0.998,1] | 0.948x [0.945,0.952] | 0.273x [0.272,0.274] | None observed |
| 100000 | fixed16 | pinned_batch1 / vmrss_kib | 115.203 | 125.188 | 66.883 | 76.879 | 32.441 | 0.92x [0.92,0.94] | 0.87x [0.868,0.87] | 0.259x [0.259,0.264] | 0.422x [0.421,0.422] | legacy (all), resident (all) |
| 100000 | fixed16 | pinned_batch1000 / vmrss_kib | 204.410 | 214.332 | 67.008 | 77.004 | 116.324 | 0.953x [0.953,0.956] | 0.87x [0.868,0.871] | 0.543x [0.542,0.544] | 1.51x [1.51,1.51] | resident (all) |
| 100000 | fixed16 | pinned_batch64 / vmrss_kib | 123.945 | 133.887 | 66.883 | 76.879 | 37.984 | 0.925x [0.925,0.944] | 0.87x [0.868,0.87] | 0.284x [0.284,0.288] | 0.494x [0.493,0.494] | legacy (all), resident (all) |
| 100000 | fixed16 | process_start_before_fixture / vmrss_kib | 2.379 | 2.375 | 2.312 | 2.375 | 2.316 | 1x [0.998,1.03] | 0.997x [0.926,1] | 0.974x [0.927,1.03] | 0.974x [0.928,1.03] | resident (some) |
| 100000 | fixed16 | resident_ballast_ready / vmrss_kib | 10.867 | 10.852 | 10.836 | 10.812 | 10.840 | 1x [1,1] | 1x [0.991,1.01] | 0.991x [0.991,1] | 0.995x [0.992,1.01] | None observed |
| 100000 | fixed16 | two_generations / vmrss_kib | 204.410 | 224.332 | 67.008 | 87.004 | 116.324 | 0.911x [0.911,0.913] | 0.77x [0.769,0.77] | 0.519x [0.518,0.519] | 1.34x [1.34,1.34] | legacy (all), resident (all) |
| 100000 | fixed16 | unpinned_after_batch1 / vmrss_kib | 204.957 | 204.879 | 67.008 | 67.004 | 119.168 | 1x [1,1] | 1x [0.998,1] | 0.582x [0.581,0.583] | 1.78x [1.78,1.78] | None observed |
| 100000 | fixed16 | unpinned_after_batch1000 / vmrss_kib | 294.484 | 294.152 | 67.008 | 67.004 | 119.168 | 1x [1,1] | 1x [0.998,1] | 0.405x [0.404,0.406] | 1.78x [1.78,1.78] | None observed |
| 100000 | fixed16 | unpinned_after_batch64 / vmrss_kib | 213.676 | 213.547 | 67.008 | 67.004 | 119.168 | 1x [1,1] | 1x [0.998,1] | 0.558x [0.558,0.559] | 1.78x [1.78,1.78] | None observed |
| 100000 | fixed16 | unpinned_before_batch1 / vmrss_kib | 18.531 | 18.562 | 62.160 | 62.215 | 19.133 | 1x [0.998,1] | 1x [0.998,1] | 1.03x [1.03,1.03] | 0.308x [0.306,0.309] | None observed |
| 100000 | fixed16 | unpinned_before_batch1000 / vmrss_kib | 114.656 | 114.641 | 65.469 | 65.508 | 30.320 | 1x [1,1.02] | 1x [0.998,1] | 0.265x [0.264,0.27] | 0.463x [0.462,0.464] | None observed |
| 100000 | fixed16 | unpinned_before_batch64 / vmrss_kib | 28.062 | 28.086 | 62.629 | 62.684 | 23.504 | 1x [0.999,1] | 1x [0.998,1] | 0.837x [0.835,0.84] | 0.375x [0.374,0.375] | None observed |
| 100000 | fixed16 | warm_hit / vmrss_kib | 114.656 | 124.641 | 67.016 | 77.012 | 32.246 | 0.92x [0.92,0.939] | 0.87x [0.868,0.871] | 0.259x [0.259,0.264] | 0.419x [0.418,0.419] | legacy (all), resident (all) |
| 100000 | fixed16 | warm_hot90 / vmrss_kib | 114.656 | 124.641 | 66.883 | 76.879 | 32.254 | 0.92x [0.92,0.939] | 0.87x [0.868,0.87] | 0.259x [0.259,0.264] | 0.42x [0.419,0.42] | legacy (all), resident (all) |
| 100000 | fixed16 | warm_miss / vmrss_kib | 114.656 | 124.641 | 66.883 | 76.879 | 32.238 | 0.92x [0.92,0.939] | 0.87x [0.868,0.87] | 0.259x [0.259,0.264] | 0.419x [0.419,0.419] | legacy (all), resident (all) |
| 100000 | fixed16 | warm_mixed50 / vmrss_kib | 114.656 | 124.641 | 66.883 | 76.879 | 32.246 | 0.92x [0.92,0.939] | 0.87x [0.868,0.87] | 0.259x [0.259,0.264] | 0.419x [0.419,0.419] | legacy (all), resident (all) |
| 100000 | hostname | after_full_validation / vmrss_kib | 133.625 | 140.113 | 68.684 | 78.660 | 39.570 | 0.954x [0.952,0.955] | 0.873x [0.872,0.874] | 0.282x [0.282,0.283] | 0.503x [0.503,0.503] | resident (all) |
| 100000 | hostname | after_old_lookup_drop / vmrss_kib | 237.277 | 253.879 | 71.570 | 71.570 | 141.949 | 0.934x [0.934,0.935] | 1x [0.999,1] | 0.559x [0.559,0.559] | 1.98x [1.98,1.99] | legacy (all) |
| 100000 | hostname | after_old_snapshot_release / vmrss_kib | 237.277 | 253.879 | 71.570 | 71.570 | 141.949 | 0.934x [0.934,0.935] | 1x [0.999,1] | 0.559x [0.559,0.559] | 1.98x [1.98,1.99] | legacy (all) |
| 100000 | hostname | after_pinned_writes / vmrss_kib | 237.277 | 247.227 | 71.570 | 81.570 | 138.723 | 0.959x [0.959,0.96] | 0.877x [0.877,0.878] | 0.561x [0.561,0.561] | 1.7x [1.7,1.7] | resident (all) |
| 100000 | hostname | after_prepare / vmrss_kib | 133.625 | 140.113 | 68.684 | 78.660 | 39.570 | 0.954x [0.952,0.955] | 0.873x [0.872,0.874] | 0.282x [0.282,0.283] | 0.503x [0.503,0.503] | resident (all) |
| 100000 | hostname | after_reopen_full_validation / vmrss_kib | 339.734 | 356.336 | 70.301 | 83.020 | 60.789 | 0.953x [0.953,0.953] | 0.847x [0.846,0.847] | 0.171x [0.168,0.171] | 0.732x [0.723,0.733] | resident (all) |
| 100000 | hostname | before_prepare / vmrss_kib | 133.625 | 133.477 | 68.621 | 68.660 | 39.570 | 1x [1,1] | 1x [0.999,1] | 0.297x [0.296,0.297] | 0.576x [0.576,0.577] | None observed |
| 100000 | hostname | closed_environment / vmrss_kib | 24.383 | 41.051 | 21.977 | 31.977 | 28.539 | 0.594x [0.593,0.596] | 0.687x [0.685,0.689] | 0.695x [0.695,0.695] | 0.893x [0.892,0.893] | legacy (all), resident (all) |
| 100000 | hostname | fixture_ready / vmrss_kib | 13.898 | 13.875 | 13.879 | 13.812 | 13.812 | 1x [0.995,1.01] | 1x [0.999,1.01] | 1x [0.995,1] | 1x [0.993,1.01] | None observed |
| 100000 | hostname | loaded_source_only / vmrss_kib | 23.766 | 23.793 | 65.434 | 65.496 | 22.762 | 0.999x [0.999,1] | 1x [0.999,1] | 0.957x [0.954,0.957] | 0.348x [0.347,0.348] | None observed |
| 100000 | hostname | pinned_batch1 / vmrss_kib | 134.172 | 142.172 | 71.570 | 81.566 | 43.668 | 0.944x [0.943,0.944] | 0.877x [0.877,0.878] | 0.307x [0.307,0.308] | 0.535x [0.535,0.536] | legacy (all), resident (all) |
| 100000 | hostname | pinned_batch1000 / vmrss_kib | 237.277 | 247.227 | 71.570 | 81.570 | 138.723 | 0.959x [0.959,0.96] | 0.877x [0.877,0.878] | 0.561x [0.561,0.561] | 1.7x [1.7,1.7] | resident (all) |
| 100000 | hostname | pinned_batch64 / vmrss_kib | 144.582 | 152.605 | 71.570 | 81.566 | 50.648 | 0.947x [0.947,0.948] | 0.877x [0.877,0.878] | 0.332x [0.332,0.333] | 0.621x [0.621,0.622] | legacy (all), resident (all) |
| 100000 | hostname | process_start_before_fixture / vmrss_kib | 2.375 | 2.309 | 2.312 | 2.312 | 2.316 | 1.03x [1,1.03] | 1x [0.998,1.05] | 1x [1,1] | 1x [0.974,1] | None observed |
| 100000 | hostname | resident_ballast_ready / vmrss_kib | 13.902 | 13.879 | 13.883 | 13.816 | 13.816 | 1x [0.995,1.01] | 1x [0.999,1.01] | 1x [0.995,1] | 1x [0.993,1.01] | None observed |
| 100000 | hostname | two_generations / vmrss_kib | 237.277 | 253.879 | 71.570 | 91.574 | 138.723 | 0.934x [0.934,0.935] | 0.782x [0.781,0.782] | 0.547x [0.546,0.547] | 1.52x [1.51,1.52] | legacy (all), resident (all) |
| 100000 | hostname | unpinned_after_batch1 / vmrss_kib | 237.824 | 254.426 | 71.570 | 71.570 | 141.949 | 0.935x [0.934,0.935] | 1x [0.999,1] | 0.558x [0.558,0.558] | 1.98x [1.98,1.99] | legacy (all) |
| 100000 | hostname | unpinned_after_batch1000 / vmrss_kib | 339.734 | 356.336 | 71.570 | 71.570 | 141.949 | 0.953x [0.953,0.953] | 1x [0.999,1] | 0.399x [0.398,0.399] | 1.98x [1.98,1.99] | None observed |
| 100000 | hostname | unpinned_after_batch64 / vmrss_kib | 248.234 | 264.836 | 71.570 | 71.570 | 141.949 | 0.937x [0.937,0.937] | 1x [0.999,1] | 0.536x [0.536,0.536] | 1.98x [1.98,1.99] | legacy (all) |
| 100000 | hostname | unpinned_before_batch1 / vmrss_kib | 24.379 | 24.406 | 65.434 | 65.496 | 25.398 | 0.999x [0.999,1] | 1x [0.999,1] | 1.04x [1.04,1.05] | 0.388x [0.387,0.39] | None observed |
| 100000 | hostname | unpinned_before_batch1000 / vmrss_kib | 133.625 | 133.477 | 68.633 | 68.668 | 39.570 | 1x [1,1] | 1x [0.999,1] | 0.297x [0.296,0.297] | 0.576x [0.576,0.577] | None observed |
| 100000 | hostname | unpinned_before_batch64 / vmrss_kib | 35.707 | 35.742 | 65.957 | 66.020 | 32.156 | 0.999x [0.999,1] | 1x [0.999,1] | 0.899x [0.898,0.9] | 0.487x [0.486,0.488] | None observed |
| 100000 | hostname | warm_hit / vmrss_kib | 133.625 | 141.477 | 71.438 | 81.434 | 43.457 | 0.944x [0.944,0.945] | 0.877x [0.876,0.878] | 0.307x [0.307,0.308] | 0.534x [0.534,0.534] | legacy (all), resident (all) |
| 100000 | hostname | warm_hot90 / vmrss_kib | 133.625 | 141.496 | 71.453 | 81.445 | 43.480 | 0.944x [0.944,0.944] | 0.877x [0.876,0.878] | 0.307x [0.307,0.308] | 0.534x [0.534,0.534] | legacy (all), resident (all) |
| 100000 | hostname | warm_miss / vmrss_kib | 133.625 | 141.480 | 71.441 | 81.434 | 43.461 | 0.944x [0.944,0.944] | 0.877x [0.876,0.878] | 0.307x [0.307,0.308] | 0.534x [0.534,0.534] | legacy (all), resident (all) |
| 100000 | hostname | warm_mixed50 / vmrss_kib | 133.625 | 141.492 | 71.445 | 81.441 | 43.469 | 0.944x [0.944,0.944] | 0.877x [0.876,0.878] | 0.307x [0.307,0.308] | 0.534x [0.534,0.534] | legacy (all), resident (all) |
| 1000000 | fixed16 | after_full_validation / vmrss_kib | 288.926 | 368.902 | 296.203 | 376.273 | 211.516 | 0.783x [0.783,0.783] | 0.787x [0.787,0.787] | 0.573x [0.573,0.573] | 0.562x [0.562,0.562] | legacy (all), resident (all) |
| 1000000 | fixed16 | after_old_lookup_drop / vmrss_kib | 425.434 | 425.465 | 297.703 | 297.762 | 339.090 | 1x [1,1] | 1x [1,1] | 0.797x [0.797,0.797] | 1.14x [1.14,1.14] | None observed |
| 1000000 | fixed16 | after_old_snapshot_release / vmrss_kib | 425.434 | 425.465 | 297.703 | 297.762 | 339.090 | 1x [1,1] | 1x [1,1] | 0.797x [0.797,0.797] | 1.14x [1.14,1.14] | None observed |
| 1000000 | fixed16 | after_pinned_writes / vmrss_kib | 425.434 | 505.469 | 297.703 | 377.766 | 333.840 | 0.842x [0.842,0.842] | 0.788x [0.788,0.788] | 0.66x [0.66,0.661] | 0.884x [0.884,0.884] | legacy (all), resident (all) |
| 1000000 | fixed16 | after_prepare / vmrss_kib | 288.926 | 368.902 | 296.203 | 376.273 | 211.516 | 0.783x [0.783,0.783] | 0.787x [0.787,0.787] | 0.573x [0.573,0.573] | 0.562x [0.562,0.562] | legacy (all), resident (all) |
| 1000000 | fixed16 | after_reopen_full_validation / vmrss_kib | 560.098 | 640.105 | 295.801 | 375.820 | 336.082 | 0.875x [0.875,0.875] | 0.787x [0.787,0.787] | 0.525x [0.525,0.525] | 0.894x [0.894,0.895] | legacy (all), resident (all) |
| 1000000 | fixed16 | before_prepare / vmrss_kib | 288.926 | 288.898 | 296.203 | 296.207 | 211.516 | 1x [1,1] | 1x [1,1] | 0.732x [0.732,0.732] | 0.714x [0.713,0.714] | None observed |
| 1000000 | fixed16 | closed_environment / vmrss_kib | 99.891 | 99.906 | 98.578 | 98.633 | 147.754 | 1x [1,1] | 0.999x [0.999,1] | 1.48x [1.48,1.48] | 1.5x [1.5,1.5] | None observed |
| 1000000 | fixed16 | fixture_ready / vmrss_kib | 86.363 | 86.359 | 86.371 | 86.371 | 86.371 | 1x [1,1] | 1x [0.999,1] | 1x [1,1] | 1x [1,1] | None observed |
| 1000000 | fixed16 | loaded_source_only / vmrss_kib | 144.145 | 144.145 | 290.121 | 290.125 | 144.164 | 1x [1,1] | 1x [1,1] | 1x [1,1] | 0.497x [0.497,0.497] | None observed |
| 1000000 | fixed16 | pinned_batch1 / vmrss_kib | 289.582 | 369.559 | 297.703 | 377.766 | 213.422 | 0.784x [0.783,0.784] | 0.788x [0.788,0.788] | 0.577x [0.577,0.578] | 0.565x [0.565,0.565] | legacy (all), resident (all) |
| 1000000 | fixed16 | pinned_batch1000 / vmrss_kib | 425.434 | 505.469 | 297.703 | 377.766 | 333.840 | 0.842x [0.842,0.842] | 0.788x [0.788,0.788] | 0.66x [0.66,0.661] | 0.884x [0.884,0.884] | legacy (all), resident (all) |
| 1000000 | fixed16 | pinned_batch64 / vmrss_kib | 302.727 | 382.723 | 297.703 | 377.766 | 221.996 | 0.791x [0.791,0.791] | 0.788x [0.788,0.788] | 0.58x [0.58,0.58] | 0.588x [0.588,0.588] | legacy (all), resident (all) |
| 1000000 | fixed16 | process_start_before_fixture / vmrss_kib | 2.312 | 2.312 | 2.312 | 2.312 | 2.312 | 1x [0.997,1] | 1x [0.998,1] | 1x [1,1.03] | 1x [1,1] | None observed |
| 1000000 | fixed16 | resident_ballast_ready / vmrss_kib | 86.367 | 86.363 | 86.375 | 86.375 | 86.375 | 1x [1,1] | 1x [0.999,1] | 1x [1,1] | 1x [1,1] | None observed |
| 1000000 | fixed16 | two_generations / vmrss_kib | 425.434 | 585.469 | 297.703 | 457.770 | 333.840 | 0.727x [0.727,0.727] | 0.65x [0.65,0.65] | 0.57x [0.57,0.571] | 0.729x [0.729,0.73] | legacy (all), resident (all) |
| 1000000 | fixed16 | unpinned_after_batch1 / vmrss_kib | 426.090 | 426.121 | 297.703 | 297.762 | 339.090 | 1x [1,1] | 1x [1,1] | 0.796x [0.796,0.796] | 1.14x [1.14,1.14] | None observed |
| 1000000 | fixed16 | unpinned_after_batch1000 / vmrss_kib | 560.098 | 560.105 | 297.711 | 297.762 | 339.090 | 1x [1,1] | 1x [1,1] | 0.605x [0.605,0.605] | 1.14x [1.14,1.14] | None observed |
| 1000000 | fixed16 | unpinned_after_batch64 / vmrss_kib | 439.223 | 439.273 | 297.703 | 297.762 | 339.090 | 1x [1,1] | 1x [1,1] | 0.772x [0.772,0.772] | 1.14x [1.14,1.14] | None observed |
| 1000000 | fixed16 | unpinned_before_batch1 / vmrss_kib | 144.906 | 144.914 | 290.145 | 290.148 | 147.707 | 1x [1,1] | 1x [1,1] | 1.02x [1.02,1.02] | 0.509x [0.507,0.51] | None observed |
| 1000000 | fixed16 | unpinned_before_batch1000 / vmrss_kib | 288.926 | 288.898 | 296.203 | 296.207 | 211.516 | 1x [1,1] | 1x [1,1] | 0.732x [0.732,0.732] | 0.714x [0.714,0.714] | None observed |
| 1000000 | fixed16 | unpinned_before_batch64 / vmrss_kib | 158.605 | 158.594 | 290.781 | 290.785 | 195.367 | 1x [1,1] | 1x [1,1] | 1.23x [1.23,1.24] | 0.672x [0.671,0.674] | None observed |
| 1000000 | fixed16 | warm_hit / vmrss_kib | 288.926 | 368.902 | 297.707 | 377.777 | 213.227 | 0.783x [0.783,0.783] | 0.788x [0.788,0.788] | 0.578x [0.578,0.578] | 0.564x [0.564,0.565] | legacy (all), resident (all) |
| 1000000 | fixed16 | warm_hot90 / vmrss_kib | 288.926 | 368.902 | 297.703 | 377.766 | 213.234 | 0.783x [0.783,0.783] | 0.788x [0.788,0.788] | 0.578x [0.578,0.578] | 0.564x [0.564,0.565] | legacy (all), resident (all) |
| 1000000 | fixed16 | warm_miss / vmrss_kib | 288.926 | 368.902 | 297.703 | 377.766 | 213.219 | 0.783x [0.783,0.783] | 0.788x [0.788,0.788] | 0.578x [0.578,0.578] | 0.564x [0.564,0.565] | legacy (all), resident (all) |
| 1000000 | fixed16 | warm_mixed50 / vmrss_kib | 288.926 | 368.902 | 297.703 | 377.766 | 213.230 | 0.783x [0.783,0.783] | 0.788x [0.788,0.788] | 0.578x [0.578,0.578] | 0.564x [0.564,0.565] | legacy (all), resident (all) |
| 1000000 | hostname | after_full_validation / vmrss_kib | 371.027 | 451.445 | 328.035 | 408.004 | 298.395 | 0.822x [0.821,0.822] | 0.804x [0.804,0.804] | 0.661x [0.661,0.662] | 0.731x [0.731,0.731] | legacy (all), resident (all) |
| 1000000 | hostname | after_old_lookup_drop / vmrss_kib | 531.336 | 531.754 | 329.500 | 329.461 | 449.738 | 1x [0.999,1] | 1x [1,1] | 0.846x [0.845,0.847] | 1.37x [1.37,1.37] | None observed |
| 1000000 | hostname | after_old_snapshot_release / vmrss_kib | 531.336 | 531.754 | 329.500 | 329.461 | 449.738 | 1x [0.999,1] | 1x [1,1] | 0.846x [0.845,0.847] | 1.37x [1.37,1.37] | None observed |
| 1000000 | hostname | after_pinned_writes / vmrss_kib | 531.336 | 611.754 | 329.500 | 409.465 | 444.148 | 0.869x [0.868,0.869] | 0.805x [0.805,0.805] | 0.726x [0.726,0.727] | 1.08x [1.08,1.09] | legacy (all), resident (all) |
| 1000000 | hostname | after_prepare / vmrss_kib | 371.027 | 451.445 | 328.035 | 408.004 | 298.395 | 0.822x [0.821,0.822] | 0.804x [0.804,0.804] | 0.661x [0.661,0.662] | 0.731x [0.731,0.731] | legacy (all), resident (all) |
| 1000000 | hostname | after_reopen_full_validation / vmrss_kib | 689.125 | 769.371 | 326.906 | 406.766 | 448.434 | 0.896x [0.895,0.896] | 0.804x [0.803,0.804] | 0.583x [0.582,0.583] | 1.1x [1.1,1.1] | legacy (all), resident (all) |
| 1000000 | hostname | before_prepare / vmrss_kib | 371.027 | 371.441 | 327.984 | 327.938 | 298.395 | 0.999x [0.998,1] | 1x [1,1] | 0.803x [0.803,0.804] | 0.91x [0.91,0.91] | None observed |
| 1000000 | hostname | closed_environment / vmrss_kib | 131.809 | 132.281 | 130.500 | 130.473 | 207.680 | 0.998x [0.995,0.999] | 1x [1,1] | 1.57x [1.57,1.57] | 1.59x [1.59,1.59] | None observed |
| 1000000 | hostname | fixture_ready / vmrss_kib | 116.898 | 116.895 | 116.879 | 116.906 | 116.898 | 1x [1,1] | 1x [1,1] | 1x [1,1] | 1x [1,1] | None observed |
| 1000000 | hostname | loaded_source_only / vmrss_kib | 202.367 | 202.375 | 322.148 | 322.109 | 202.301 | 1x [1,1] | 1x [1,1] | 1x [0.999,1] | 0.628x [0.628,0.628] | None observed |
| 1000000 | hostname | pinned_batch1 / vmrss_kib | 374.246 | 454.723 | 329.258 | 409.223 | 302.285 | 0.824x [0.823,0.824] | 0.805x [0.804,0.805] | 0.665x [0.664,0.666] | 0.739x [0.739,0.739] | legacy (all), resident (all) |
| 1000000 | hostname | pinned_batch1000 / vmrss_kib | 531.336 | 611.754 | 329.500 | 409.465 | 444.148 | 0.869x [0.868,0.869] | 0.805x [0.805,0.805] | 0.726x [0.726,0.727] | 1.08x [1.08,1.09] | legacy (all), resident (all) |
| 1000000 | hostname | pinned_batch64 / vmrss_kib | 388.648 | 469.078 | 329.258 | 409.223 | 311.656 | 0.829x [0.828,0.829] | 0.805x [0.804,0.805] | 0.664x [0.664,0.665] | 0.762x [0.761,0.762] | legacy (all), resident (all) |
| 1000000 | hostname | process_start_before_fixture / vmrss_kib | 2.375 | 2.375 | 2.312 | 2.379 | 2.379 | 1x [0.989,1.05] | 0.998x [0.972,0.998] | 1x [0.972,1.05] | 1x [0.972,1.05] | None observed |
| 1000000 | hostname | resident_ballast_ready / vmrss_kib | 116.902 | 116.898 | 116.879 | 116.910 | 116.902 | 1x [1,1] | 1x [0.999,1] | 1x [1,1] | 1x [1,1] | None observed |
| 1000000 | hostname | two_generations / vmrss_kib | 531.336 | 691.758 | 329.500 | 489.469 | 444.148 | 0.768x [0.768,0.768] | 0.673x [0.673,0.673] | 0.642x [0.642,0.643] | 0.907x [0.907,0.908] | legacy (all), resident (all) |
| 1000000 | hostname | unpinned_after_batch1 / vmrss_kib | 531.992 | 532.410 | 329.500 | 329.461 | 449.738 | 1x [0.999,1] | 1x [1,1] | 0.845x [0.844,0.846] | 1.37x [1.37,1.37] | None observed |
| 1000000 | hostname | unpinned_after_batch1000 / vmrss_kib | 689.125 | 689.371 | 329.500 | 329.461 | 449.742 | 1x [0.999,1] | 1x [1,1] | 0.652x [0.652,0.653] | 1.37x [1.37,1.37] | None observed |
| 1000000 | hostname | unpinned_after_batch64 / vmrss_kib | 546.379 | 546.832 | 329.500 | 329.461 | 449.738 | 1x [0.999,1] | 1x [1,1] | 0.822x [0.822,0.823] | 1.37x [1.37,1.37] | None observed |
| 1000000 | hostname | unpinned_before_batch1 / vmrss_kib | 203.086 | 203.094 | 322.176 | 322.141 | 205.961 | 1x [1,1] | 1x [1,1] | 1.01x [1.01,1.01] | 0.639x [0.639,0.64] | None observed |
| 1000000 | hostname | unpinned_before_batch1000 / vmrss_kib | 371.027 | 371.441 | 327.984 | 327.938 | 298.395 | 0.999x [0.998,1] | 1x [1,1] | 0.803x [0.803,0.804] | 0.91x [0.91,0.91] | None observed |
| 1000000 | hostname | unpinned_before_batch64 / vmrss_kib | 218.074 | 218.102 | 322.926 | 322.887 | 270.594 | 1x [1,1] | 1x [1,1] | 1.24x [1.24,1.24] | 0.838x [0.837,0.84] | None observed |
| 1000000 | hostname | warm_hit / vmrss_kib | 371.027 | 451.445 | 329.539 | 409.508 | 302.078 | 0.822x [0.821,0.822] | 0.805x [0.805,0.805] | 0.669x [0.669,0.67] | 0.738x [0.738,0.738] | legacy (all), resident (all) |
| 1000000 | hostname | warm_hot90 / vmrss_kib | 373.590 | 454.066 | 329.258 | 409.223 | 302.098 | 0.823x [0.822,0.823] | 0.805x [0.804,0.805] | 0.665x [0.665,0.666] | 0.738x [0.738,0.738] | legacy (all), resident (all) |
| 1000000 | hostname | warm_miss / vmrss_kib | 373.590 | 454.066 | 329.258 | 409.223 | 302.082 | 0.823x [0.822,0.823] | 0.805x [0.804,0.805] | 0.665x [0.665,0.666] | 0.738x [0.738,0.738] | legacy (all), resident (all) |
| 1000000 | hostname | warm_mixed50 / vmrss_kib | 373.590 | 454.066 | 329.258 | 409.223 | 302.090 | 0.823x [0.822,0.823] | 0.805x [0.804,0.805] | 0.665x [0.665,0.666] | 0.738x [0.738,0.738] | legacy (all), resident (all) |

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

506 rows have at least one >5% hash cost increase against the matching native arm, across all timing/memory/storage counters. Each appears in the complete CSV and typed summary, including cases that improve at other sizes. Read/write/lifecycle rows and main RSS rows are shown above; secondary memory counters are in the CSV. No universal no-regression claim follows from this campaign.

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
