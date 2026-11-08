# Existing-engine regression campaign

All 15 existing read/write cases on disk and RAM: **30 cases**, 3 fresh processes per arm/filesystem, 16 alternating ULMDB/LMDB trials within each process. Baseline/candidate processes run as adjacent AB/BA pairs.

This measures ordinary engine behavior with the optional lookup accelerator unused. It does not measure an application keeping a side index alive during writes, rebuilding it, or using it for other operations. The separate direct comparison qualifies accelerated reads.

All engines use normal durability flags. RAM uses real tmpfs/ramfs and does not provide reboot durability. Measurements are warm synthetic single-core results on a shared host. Native LMDB controls are retained without normalization or outlier removal.

Centers are process-level geometric means. Sampled p99 columns are geometric means of each trial's timer-inclusive sampled p99; they are not pooled population percentiles. Intervals use paired process log ratios with Bonferroni adjustment across all captured metrics. Three process pairs have limited power; overlapping intervals do not establish equivalence, and no universal no-regression guarantee follows.

ULMDB target metrics with adverse point estimates: 39. Target metrics worse by more than 5% at their observed centers: **6**. Full uncertainty, unavailable values and every raw observation are retained in comparison.json.

## Complete throughput table

| Filesystem | Case | Baseline ULMDB/s | Candidate ULMDB/s | Change | Baseline LMDB/s | Candidate LMDB/s | LMDB drift | Candidate vs local LMDB | Paired ratio interval |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|
| disk | durable_batch1 | 12,558 | 12,787 | +1.82% | 10,408 | 10,556 | +1.43% | +21.13% | 0.560–1.850 |
| disk | durable_batch64 | 345,944 | 326,037 | -5.75% | 178,485 | 173,079 | -3.03% | +88.37% | 0.154–5.779 |
| disk | ipv4_lpm_hits | 2,527,559 | 3,018,232 | +19.41% | 1,883,526 | 2,098,831 | +11.43% | +43.81% | 0.000–57331.280 |
| disk | ipv4_lpm_inside_gap_misses | 9,123,340 | 10,829,033 | +18.70% | 1,749,385 | 1,986,810 | +13.57% | +445.05% | 0.000–33058.000 |
| disk | ipv4_lpm_misses | 16,999,746 | 20,403,711 | +20.02% | 3,382,798 | 3,840,976 | +13.54% | +431.21% | 0.000–205314.881 |
| disk | ipv4_lpm_mixed | 4,347,067 | 5,191,014 | +19.41% | 2,351,245 | 2,612,401 | +11.11% | +98.71% | 0.000–59421.044 |
| disk | ipv4_lpm_outside_cover_misses | 94,047,938 | 99,488,833 | +5.79% | 86,667,669 | 93,938,030 | +8.39% | +5.91% | 0.007–169.745 |
| disk | ipv6_lpm_hits | 2,502,486 | 2,512,935 | +0.42% | 1,239,352 | 1,223,739 | -1.26% | +105.35% | 0.885–1.140 |
| disk | ipv6_lpm_inside_gap_misses | 9,082,665 | 9,199,397 | +1.29% | 1,136,303 | 1,111,527 | -2.18% | +727.64% | 0.592–1.733 |
| disk | ipv6_lpm_misses | 17,117,633 | 17,121,099 | +0.02% | 2,249,409 | 2,196,553 | -2.35% | +679.45% | 0.691–1.448 |
| disk | ipv6_lpm_mixed | 4,296,232 | 4,302,563 | +0.15% | 1,550,010 | 1,529,283 | -1.34% | +181.35% | 0.243–4.134 |
| disk | ipv6_lpm_outside_cover_misses | 96,566,329 | 96,378,480 | -0.19% | 93,956,808 | 91,074,993 | -3.07% | +5.82% | 0.681–1.462 |
| disk | ordered_kv_hits | 6,961,199 | 7,421,886 | +6.62% | 3,792,636 | 4,121,035 | +8.66% | +80.10% | 0.012–98.570 |
| disk | ordered_kv_misses | 6,703,026 | 7,394,237 | +10.31% | 3,599,292 | 3,994,116 | +10.97% | +85.13% | 0.002–794.615 |
| disk | ordered_kv_mixed | 7,150,989 | 7,151,035 | +0.00% | 4,020,789 | 4,061,428 | +1.01% | +76.07% | 0.531–1.883 |
| ram | durable_batch1 | 34,747 | 33,850 | -2.58% | 131,493 | 131,530 | +0.03% | -74.26% | 0.407–2.334 |
| ram | durable_batch64 | 541,444 | 536,059 | -0.99% | 745,726 | 747,529 | +0.24% | -28.29% | 0.812–1.207 |
| ram | ipv4_lpm_hits | 3,022,315 | 3,025,594 | +0.11% | 2,152,082 | 2,145,790 | -0.29% | +41.00% | 0.784–1.278 |
| ram | ipv4_lpm_inside_gap_misses | 10,956,313 | 10,914,324 | -0.38% | 1,968,646 | 1,984,721 | +0.82% | +449.92% | 0.933–1.063 |
| ram | ipv4_lpm_misses | 20,576,368 | 20,340,149 | -1.15% | 3,764,286 | 3,863,809 | +2.64% | +426.43% | 0.758–1.289 |
| ram | ipv4_lpm_mixed | 5,161,933 | 5,141,230 | -0.40% | 2,672,447 | 2,615,090 | -2.15% | +96.60% | 0.269–3.689 |
| ram | ipv4_lpm_outside_cover_misses | 99,228,926 | 100,177,232 | +0.96% | 87,920,065 | 81,352,739 | -7.47% | +23.14% | 0.241–4.224 |
| ram | ipv6_lpm_hits | 2,511,563 | 2,517,746 | +0.25% | 1,225,671 | 1,241,859 | +1.32% | +102.74% | 0.905–1.111 |
| ram | ipv6_lpm_inside_gap_misses | 9,158,484 | 9,147,898 | -0.12% | 1,129,742 | 1,129,357 | -0.03% | +710.01% | 0.521–1.915 |
| ram | ipv6_lpm_misses | 17,144,918 | 17,156,336 | +0.07% | 2,215,480 | 2,224,429 | +0.40% | +671.27% | 0.891–1.123 |
| ram | ipv6_lpm_mixed | 4,371,610 | 4,281,314 | -2.07% | 1,540,300 | 1,525,041 | -0.99% | +180.73% | 0.281–3.413 |
| ram | ipv6_lpm_outside_cover_misses | 97,565,542 | 95,973,838 | -1.63% | 88,133,634 | 82,370,046 | -6.54% | +16.52% | 0.345–2.806 |
| ram | ordered_kv_hits | 7,543,783 | 7,117,483 | -5.65% | 4,116,071 | 3,985,750 | -3.17% | +78.57% | 0.122–7.272 |
| ram | ordered_kv_misses | 7,470,205 | 7,340,505 | -1.74% | 3,977,473 | 3,994,960 | +0.44% | +83.74% | 0.596–1.621 |
| ram | ordered_kv_mixed | 7,051,462 | 7,128,061 | +1.09% | 3,902,861 | 4,033,239 | +3.34% | +76.73% | 0.183–5.591 |

## Complete sampled latency table

| Filesystem | Case | Baseline ULMDB p99 ns | Candidate ULMDB p99 ns | Change | Baseline LMDB p99 ns | Candidate LMDB p99 ns | Paired ratio interval |
|---|---|---:|---:|---:|---:|---:|---|
| disk | durable_batch1 | 98,064.6 | 94,713.6 | -3.42% | 195,767.4 | 189,044.5 | 0.065–14.419 |
| disk | durable_batch64 | 314,487.0 | 319,846.2 | +1.70% | 461,370.8 | 489,057.9 | 0.014–73.188 |
| disk | ipv4_lpm_hits | 536.0 | 467.0 | -12.87% | 872.0 | 785.7 | 0.000–20946.051 |
| disk | ipv4_lpm_inside_gap_misses | 149.7 | 133.2 | -11.05% | 708.8 | 628.3 | 0.000–20962.198 |
| disk | ipv4_lpm_misses | 152.3 | 133.2 | -12.55% | 726.8 | 620.8 | 0.000–194255.488 |
| disk | ipv4_lpm_mixed | 521.3 | 471.0 | -9.65% | 866.4 | 783.0 | 0.000–15096.331 |
| disk | ipv4_lpm_outside_cover_misses | 44.5 | 41.2 | -7.40% | 47.1 | 47.2 | 0.010–85.030 |
| disk | ipv6_lpm_hits | 543.5 | 541.5 | -0.38% | 1,273.0 | 1,290.3 | 0.593–1.673 |
| disk | ipv6_lpm_inside_gap_misses | 143.1 | 143.7 | +0.42% | 1,102.3 | 1,138.4 | 0.242–4.169 |
| disk | ipv6_lpm_misses | 142.7 | 143.3 | +0.40% | 1,060.2 | 1,113.9 | 0.511–1.971 |
| disk | ipv6_lpm_mixed | 534.7 | 531.6 | -0.59% | 1,263.3 | 1,282.3 | 0.136–7.266 |
| disk | ipv6_lpm_outside_cover_misses | 41.2 | 42.4 | +2.91% | 46.6 | 44.4 | 0.378–2.798 |
| disk | ordered_kv_hits | 221.9 | 208.2 | -6.16% | 367.1 | 341.4 | 0.021–41.440 |
| disk | ordered_kv_misses | 226.3 | 204.7 | -9.54% | 378.8 | 345.7 | 0.006–130.963 |
| disk | ordered_kv_mixed | 213.3 | 210.3 | -1.38% | 351.2 | 341.6 | 0.188–5.177 |
| ram | durable_batch1 | 21,819.6 | 21,990.0 | +0.78% | 8,442.0 | 8,433.2 | 0.797–1.274 |
| ram | durable_batch64 | 195,209.6 | 197,384.6 | +1.11% | 91,454.9 | 91,613.1 | 0.608–1.683 |
| ram | ipv4_lpm_hits | 450.0 | 466.1 | +3.58% | 772.9 | 772.0 | 0.776–1.383 |
| ram | ipv4_lpm_inside_gap_misses | 123.1 | 131.6 | +6.91% | 630.0 | 628.2 | 0.956–1.196 |
| ram | ipv4_lpm_misses | 122.1 | 134.2 | +9.91% | 623.4 | 611.3 | 0.781–1.547 |
| ram | ipv4_lpm_mixed | 439.9 | 474.3 | +7.83% | 758.5 | 777.8 | 0.187–6.203 |
| ram | ipv4_lpm_outside_cover_misses | 41.0 | 41.2 | +0.46% | 44.0 | 41.2 | 0.751–1.343 |
| ram | ipv6_lpm_hits | 530.5 | 535.8 | +1.00% | 1,261.6 | 1,273.6 | 0.774–1.317 |
| ram | ipv6_lpm_inside_gap_misses | 141.0 | 142.7 | +1.21% | 1,085.8 | 1,129.6 | 1.012–1.012 |
| ram | ipv6_lpm_misses | 142.0 | 142.1 | +0.07% | 1,075.6 | 1,093.4 | 0.960–1.043 |
| ram | ipv6_lpm_mixed | 514.9 | 531.9 | +3.30% | 1,250.5 | 1,257.3 | 0.321–3.322 |
| ram | ipv6_lpm_outside_cover_misses | 41.2 | 41.9 | +1.79% | 43.8 | 41.2 | 0.484–2.141 |
| ram | ordered_kv_hits | 202.1 | 219.1 | +8.43% | 336.6 | 364.3 | 0.094–12.491 |
| ram | ordered_kv_misses | 201.9 | 207.7 | +2.86% | 340.2 | 347.4 | 0.285–3.717 |
| ram | ordered_kv_mixed | 213.6 | 220.8 | +3.37% | 355.7 | 355.3 | 0.015–70.701 |

## Every other measured metric

Includes combined durable commit phases, setup, ULMDB reopen, file allocation and mapping observations. LMDB cold reopen is not measured by the existing suite; unavailable Resident pathname-attributed RSS remains unavailable.

| Metric | Role | Baseline | Candidate | Change | Paired ratio interval | Status |
|---|---|---:|---:|---:|---|---|
| full/disk/harness/setup/after_setup_ns (ns) | diagnostic | 1,133.592 | 1,219.336 | +7.56% | 0.002056–562.9 | inconclusive_interval |
| full/disk/harness/setup/dataset_ns (ns) | diagnostic | 242,824,327.000 | 248,529,090.939 | +2.35% | 0.0384–27.28 | inconclusive_interval |
| full/disk/harness/setup/fixture_load_both_ns (ns) | diagnostic | 14,558,216.206 | 14,515,887.073 | -0.29% | 0.3337–2.979 | inconclusive_interval |
| full/disk/lmdb/durable_batch1/combined_throughput (records/s) | control | 11,033.289 | 11,280.506 | +2.24% | 0.3301–3.166 | inconclusive_interval |
| full/disk/lmdb/durable_batch1/pooled_sampled_p99 (ns) | control | 327,938.950 | 328,493.716 | +0.17% | 0.4842–2.072 | inconclusive_interval |
| full/disk/lmdb/durable_batch64/combined_throughput (records/s) | control | 183,860.884 | 177,542.936 | -3.44% | 0.149–6.259 | inconclusive_interval |
| full/disk/lmdb/durable_batch64/pooled_sampled_p99 (ns) | control | 533,157.918 | 537,959.690 | +0.90% | 0.09595–10.61 | inconclusive_interval |
| full/disk/lmdb/footprint/after/allocated_file_bytes (bytes) | control | 729,088.000 | 729,088.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/lmdb/footprint/after/apparent_file_bytes (bytes) | control | 733,184.000 | 733,184.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/lmdb/footprint/after/files (files) | control | 2.000 | 2.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/lmdb/footprint/after/mapped_pss_kib (KiB) | diagnostic | 712.000 | 712.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/lmdb/footprint/after/mapped_rss_kib (KiB) | diagnostic | 712.000 | 712.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/lmdb/footprint/after/mapped_virtual_kib (KiB) | diagnostic | 524,296.000 | 524,296.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/lmdb/footprint/before/allocated_file_bytes (bytes) | control | 352,256.000 | 352,256.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/lmdb/footprint/before/apparent_file_bytes (bytes) | control | 356,352.000 | 356,352.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/lmdb/footprint/before/files (files) | control | 2.000 | 2.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/lmdb/footprint/before/mapped_pss_kib (KiB) | diagnostic | 12.000 | 12.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/lmdb/footprint/before/mapped_rss_kib (KiB) | diagnostic | 12.000 | 12.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/lmdb/footprint/before/mapped_virtual_kib (KiB) | diagnostic | 524,296.000 | 524,296.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/lmdb/setup/create (ns) | control | 200,471.604 | 231,820.256 | +15.64% | 0.0005477–2442 | inconclusive_interval |
| full/disk/ulmdb/durable_batch1/combined_throughput (records/s) | target | 13,323.681 | 13,515.386 | +1.44% | 0.7721–1.333 | inconclusive_interval |
| full/disk/ulmdb/durable_batch1/pooled_sampled_p99 (ns) | target | 320,746.271 | 246,450.423 | -23.16% | 2.565e-08–2.301e+07 | inconclusive_interval |
| full/disk/ulmdb/durable_batch64/combined_throughput (records/s) | target | 345,382.976 | 330,914.872 | -4.19% | 0.1276–7.192 | inconclusive_interval |
| full/disk/ulmdb/durable_batch64/pooled_sampled_p99 (ns) | target | 671,077.378 | 698,058.296 | +4.02% | 0.04045–26.75 | inconclusive_interval |
| full/disk/ulmdb/footprint/after/allocated_file_bytes (bytes) | target | 67,108,864.000 | 67,108,864.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/ulmdb/footprint/after/apparent_file_bytes (bytes) | target | 67,108,864.000 | 67,108,864.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/ulmdb/footprint/after/files (files) | target | 2.000 | 2.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/ulmdb/footprint/after/mapped_pss_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/disk/ulmdb/footprint/after/mapped_rss_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/disk/ulmdb/footprint/after/mapped_virtual_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/disk/ulmdb/footprint/before/allocated_file_bytes (bytes) | target | 67,108,864.000 | 67,108,864.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/ulmdb/footprint/before/apparent_file_bytes (bytes) | target | 67,108,864.000 | 67,108,864.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/ulmdb/footprint/before/files (files) | target | 2.000 | 2.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/ulmdb/footprint/before/mapped_pss_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/disk/ulmdb/footprint/before/mapped_rss_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/disk/ulmdb/footprint/before/mapped_virtual_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/disk/ulmdb/reopen/complete_cold_check_ns (ns) | target | 100,836,102.361 | 98,083,236.596 | -2.73% | 0.4067–2.326 | inconclusive_interval |
| full/disk/ulmdb/reopen/environment_drop_ns (ns) | target | 88,525.318 | 87,458.637 | -1.20% | 0.1259–7.754 | inconclusive_interval |
| full/disk/ulmdb/reopen/experimental_reenable_ns (ns) | target | 2,185.205 | 2,231.238 | +2.11% | 0.04546–22.93 | inconclusive_interval |
| full/disk/ulmdb/reopen/open_ns (ns) | target | 90,257,814.228 | 87,537,114.018 | -3.01% | 0.3663–2.568 | inconclusive_interval |
| full/disk/ulmdb/reopen/validation_ns (ns) | target | 6,383,741.958 | 6,302,930.946 | -1.27% | 0.9037–1.079 | inconclusive_interval |
| full/disk/ulmdb/setup/create (ns) | target | 102,734,761.585 | 101,686,104.104 | -1.02% | 0.9062–1.081 | inconclusive_interval |
| full/ram/harness/setup/after_setup_ns (ns) | diagnostic | 1,097.469 | 2,130.394 | +94.12% | 1.904e-18–1.979e+18 | inconclusive_interval |
| full/ram/harness/setup/dataset_ns (ns) | diagnostic | 239,067,214.295 | 237,564,728.002 | -0.63% | 0.3591–2.75 | inconclusive_interval |
| full/ram/harness/setup/fixture_load_both_ns (ns) | diagnostic | 13,122,408.446 | 13,541,785.944 | +3.20% | 0.1695–6.283 | inconclusive_interval |
| full/ram/lmdb/durable_batch1/combined_throughput (records/s) | control | 135,889.197 | 136,212.236 | +0.24% | 0.4574–2.196 | inconclusive_interval |
| full/ram/lmdb/durable_batch1/pooled_sampled_p99 (ns) | control | 9,871.925 | 10,251.086 | +3.84% | 4.427e-06–2.436e+05 | inconclusive_interval |
| full/ram/lmdb/durable_batch64/combined_throughput (records/s) | control | 753,083.945 | 755,120.032 | +0.27% | 0.4685–2.146 | inconclusive_interval |
| full/ram/lmdb/durable_batch64/pooled_sampled_p99 (ns) | control | 95,200.622 | 100,087.062 | +5.13% | 0.1051–10.51 | inconclusive_interval |
| full/ram/lmdb/footprint/after/allocated_file_bytes (bytes) | control | 729,088.000 | 729,088.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/lmdb/footprint/after/apparent_file_bytes (bytes) | control | 733,184.000 | 733,184.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/lmdb/footprint/after/files (files) | control | 2.000 | 2.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/lmdb/footprint/after/mapped_pss_kib (KiB) | diagnostic | 712.000 | 712.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/lmdb/footprint/after/mapped_rss_kib (KiB) | diagnostic | 712.000 | 712.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/lmdb/footprint/after/mapped_virtual_kib (KiB) | diagnostic | 524,296.000 | 524,296.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/lmdb/footprint/before/allocated_file_bytes (bytes) | control | 352,256.000 | 352,256.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/lmdb/footprint/before/apparent_file_bytes (bytes) | control | 356,352.000 | 356,352.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/lmdb/footprint/before/files (files) | control | 2.000 | 2.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/lmdb/footprint/before/mapped_pss_kib (KiB) | diagnostic | 12.000 | 12.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/lmdb/footprint/before/mapped_rss_kib (KiB) | diagnostic | 12.000 | 12.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/lmdb/footprint/before/mapped_virtual_kib (KiB) | diagnostic | 524,296.000 | 524,296.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/lmdb/setup/create (ns) | control | 120,518.150 | 126,275.966 | +4.78% | 0.1954–5.617 | inconclusive_interval |
| full/ram/ulmdb/durable_batch1/combined_throughput (records/s) | target | 40,803.767 | 40,152.200 | -1.60% | 0.5751–1.684 | inconclusive_interval |
| full/ram/ulmdb/durable_batch1/pooled_sampled_p99 (ns) | target | 25,810.768 | 26,748.550 | +3.63% | 0.2592–4.144 | inconclusive_interval |
| full/ram/ulmdb/durable_batch64/combined_throughput (records/s) | target | 535,408.682 | 530,581.134 | -0.90% | 0.8433–1.164 | inconclusive_interval |
| full/ram/ulmdb/durable_batch64/pooled_sampled_p99 (ns) | target | 528,481.510 | 538,226.083 | +1.84% | 0.3037–3.415 | inconclusive_interval |
| full/ram/ulmdb/footprint/after/allocated_file_bytes (bytes) | target | 67,108,864.000 | 67,108,864.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/ulmdb/footprint/after/apparent_file_bytes (bytes) | target | 67,108,864.000 | 67,108,864.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/ulmdb/footprint/after/files (files) | target | 2.000 | 2.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/ulmdb/footprint/after/mapped_pss_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/ram/ulmdb/footprint/after/mapped_rss_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/ram/ulmdb/footprint/after/mapped_virtual_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/ram/ulmdb/footprint/before/allocated_file_bytes (bytes) | target | 67,108,864.000 | 67,108,864.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/ulmdb/footprint/before/apparent_file_bytes (bytes) | target | 67,108,864.000 | 67,108,864.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/ulmdb/footprint/before/files (files) | target | 2.000 | 2.000 | +0.00% | 1–1 | identical_observed_changes |
| full/ram/ulmdb/footprint/before/mapped_pss_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/ram/ulmdb/footprint/before/mapped_rss_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/ram/ulmdb/footprint/before/mapped_virtual_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/ram/ulmdb/reopen/complete_cold_check_ns (ns) | target | 58,677,169.792 | 58,463,883.675 | -0.36% | 0.6828–1.454 | inconclusive_interval |
| full/ram/ulmdb/reopen/environment_drop_ns (ns) | target | 88,501.638 | 88,294.336 | -0.23% | 0.5183–1.92 | inconclusive_interval |
| full/ram/ulmdb/reopen/experimental_reenable_ns (ns) | target | 2,529.728 | 2,355.468 | -6.89% | 0.0002412–3595 | inconclusive_interval |
| full/ram/ulmdb/reopen/open_ns (ns) | target | 48,117,615.998 | 47,770,809.152 | -0.72% | 0.6389–1.543 | inconclusive_interval |
| full/ram/ulmdb/reopen/validation_ns (ns) | target | 6,357,842.428 | 6,384,098.504 | +0.41% | 0.4959–2.033 | inconclusive_interval |
| full/ram/ulmdb/setup/create (ns) | target | 46,981,279.085 | 46,296,042.839 | -1.46% | 0.3336–2.911 | inconclusive_interval |

## Target metrics worse by more than 5%

- `full/disk/ulmdb/durable_batch64/throughput`: -5.75% (inconclusive_interval).
- `full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99`: +6.91% (inconclusive_interval).
- `full/ram/ulmdb/ipv4_lpm_misses/sampled_p99`: +9.91% (inconclusive_interval).
- `full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99`: +7.83% (inconclusive_interval).
- `full/ram/ulmdb/ordered_kv_hits/sampled_p99`: +8.43% (inconclusive_interval).
- `full/ram/ulmdb/ordered_kv_hits/throughput`: -5.65% (inconclusive_interval).

Source manifests, build logs, executable SHA-256 values, exact schedule, process reports, complete latency samples and their digests are retained beside this report. No existing public README, performance table or benchmark/results file is changed.
