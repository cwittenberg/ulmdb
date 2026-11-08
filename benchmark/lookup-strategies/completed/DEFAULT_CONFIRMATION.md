# Existing-engine regression campaign

All 15 existing read/write cases on disk and RAM: **30 cases**, 9 fresh processes per arm/filesystem, 16 alternating ULMDB/LMDB trials within each process. Baseline/candidate processes run as adjacent AB/BA pairs.

This measures ordinary engine behavior with the optional lookup accelerator unused. It does not measure an application keeping a side index alive during writes, rebuilding it, or using it for other operations. The separate direct comparison qualifies accelerated reads.

All engines use normal durability flags. RAM uses real tmpfs/ramfs and does not provide reboot durability. Measurements are warm synthetic single-core results on a shared host. Native LMDB controls are retained without normalization or outlier removal.

Centers are process-level geometric means. Sampled p99 columns are geometric means of each trial's timer-inclusive sampled p99; they are not pooled population percentiles. Intervals use paired process log ratios with Bonferroni adjustment across all captured metrics and 9 process pairs. Overlapping intervals do not establish equivalence, and no universal no-regression guarantee follows.

ULMDB target metrics with adverse point estimates: 55. Target metrics worse by more than 5% at their observed centers: **6**. Full uncertainty, unavailable values and every raw observation are retained in comparison.json.

## Complete throughput table

| Filesystem | Case | Baseline ULMDB/s | Candidate ULMDB/s | Change | Baseline LMDB/s | Candidate LMDB/s | LMDB drift | Candidate vs local LMDB | Paired ratio interval |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|
| disk | durable_batch1 | 12,762 | 12,625 | -1.07% | 10,810 | 10,752 | -0.54% | +17.42% | 0.887–1.104 |
| disk | durable_batch64 | 340,474 | 342,635 | +0.63% | 178,474 | 177,409 | -0.60% | +93.13% | 0.899–1.126 |
| disk | ipv4_lpm_hits | 3,020,332 | 2,988,297 | -1.06% | 2,148,603 | 2,143,600 | -0.23% | +39.41% | 0.928–1.055 |
| disk | ipv4_lpm_inside_gap_misses | 10,941,033 | 10,833,911 | -0.98% | 2,034,212 | 2,088,372 | +2.66% | +418.77% | 0.971–1.010 |
| disk | ipv4_lpm_misses | 20,486,878 | 20,282,956 | -1.00% | 3,931,279 | 4,049,047 | +3.00% | +400.93% | 0.941–1.041 |
| disk | ipv4_lpm_mixed | 5,154,491 | 5,193,109 | +0.75% | 2,689,419 | 2,718,601 | +1.09% | +91.02% | 0.994–1.022 |
| disk | ipv4_lpm_outside_cover_misses | 99,399,754 | 100,125,146 | +0.73% | 93,222,308 | 93,597,106 | +0.40% | +6.97% | 0.963–1.053 |
| disk | ipv6_lpm_hits | 2,503,433 | 2,509,022 | +0.22% | 1,238,118 | 1,239,305 | +0.10% | +102.45% | 0.988–1.017 |
| disk | ipv6_lpm_inside_gap_misses | 9,144,786 | 9,216,478 | +0.78% | 1,137,409 | 1,131,307 | -0.54% | +714.68% | 0.974–1.043 |
| disk | ipv6_lpm_misses | 16,802,626 | 17,174,364 | +2.21% | 2,217,467 | 2,239,354 | +0.99% | +666.93% | 0.946–1.104 |
| disk | ipv6_lpm_mixed | 4,327,555 | 4,369,225 | +0.96% | 1,554,272 | 1,562,424 | +0.52% | +179.64% | 0.967–1.054 |
| disk | ipv6_lpm_outside_cover_misses | 96,987,289 | 95,856,237 | -1.17% | 93,749,085 | 93,820,386 | +0.08% | +2.17% | 0.911–1.072 |
| disk | ordered_kv_hits | 7,511,187 | 7,502,710 | -0.11% | 4,121,961 | 4,191,850 | +1.70% | +78.98% | 0.982–1.016 |
| disk | ordered_kv_misses | 7,494,176 | 7,446,753 | -0.63% | 4,001,285 | 4,029,585 | +0.71% | +84.80% | 0.969–1.019 |
| disk | ordered_kv_mixed | 7,215,069 | 7,177,430 | -0.52% | 4,032,171 | 4,089,661 | +1.43% | +75.50% | 0.976–1.014 |
| ram | durable_batch1 | 34,537 | 33,880 | -1.90% | 130,797 | 128,442 | -1.80% | -73.62% | 0.842–1.143 |
| ram | durable_batch64 | 534,812 | 528,534 | -1.17% | 743,719 | 725,457 | -2.46% | -27.14% | 0.900–1.085 |
| ram | ipv4_lpm_hits | 3,016,328 | 2,971,511 | -1.49% | 2,131,155 | 2,131,060 | -0.00% | +39.44% | 0.910–1.067 |
| ram | ipv4_lpm_inside_gap_misses | 10,947,943 | 10,800,880 | -1.34% | 1,977,647 | 1,986,838 | +0.46% | +443.62% | 0.967–1.007 |
| ram | ipv4_lpm_misses | 20,566,340 | 20,404,800 | -0.79% | 3,820,117 | 3,879,245 | +1.55% | +426.00% | 0.973–1.011 |
| ram | ipv4_lpm_mixed | 5,143,751 | 5,190,395 | +0.91% | 2,636,287 | 2,656,391 | +0.76% | +95.39% | 0.983–1.036 |
| ram | ipv4_lpm_outside_cover_misses | 99,799,377 | 99,893,245 | +0.09% | 86,759,444 | 92,906,370 | +7.09% | +7.52% | 0.932–1.075 |
| ram | ipv6_lpm_hits | 2,512,355 | 2,493,953 | -0.73% | 1,238,872 | 1,230,759 | -0.65% | +102.64% | 0.949–1.038 |
| ram | ipv6_lpm_inside_gap_misses | 8,654,287 | 8,792,589 | +1.60% | 1,083,567 | 1,080,684 | -0.27% | +713.61% | 0.685–1.506 |
| ram | ipv6_lpm_misses | 16,839,249 | 16,325,402 | -3.05% | 2,208,512 | 2,114,343 | -4.26% | +672.13% | 0.686–1.370 |
| ram | ipv6_lpm_mixed | 4,358,109 | 4,375,289 | +0.39% | 1,563,032 | 1,561,163 | -0.12% | +180.26% | 0.993–1.015 |
| ram | ipv6_lpm_outside_cover_misses | 93,576,823 | 93,269,230 | -0.33% | 85,650,320 | 89,770,535 | +4.81% | +3.90% | 0.704–1.411 |
| ram | ordered_kv_hits | 7,523,620 | 7,470,209 | -0.71% | 4,113,318 | 4,159,189 | +1.12% | +79.61% | 0.979–1.007 |
| ram | ordered_kv_misses | 7,452,207 | 7,424,079 | -0.38% | 3,985,442 | 4,019,772 | +0.86% | +84.69% | 0.976–1.016 |
| ram | ordered_kv_mixed | 7,191,535 | 7,193,591 | +0.03% | 4,028,813 | 4,073,532 | +1.11% | +76.59% | 0.984–1.017 |

## Complete sampled latency table

| Filesystem | Case | Baseline ULMDB p99 ns | Candidate ULMDB p99 ns | Change | Baseline LMDB p99 ns | Candidate LMDB p99 ns | Paired ratio interval |
|---|---|---:|---:|---:|---:|---:|---|
| disk | durable_batch1 | 96,387.7 | 99,883.0 | +3.63% | 188,785.1 | 199,481.7 | 0.815–1.317 |
| disk | durable_batch64 | 337,251.6 | 322,223.7 | -4.46% | 459,800.4 | 463,319.2 | 0.808–1.130 |
| disk | ipv4_lpm_hits | 457.5 | 471.9 | +3.14% | 781.9 | 784.3 | 0.914–1.163 |
| disk | ipv4_lpm_inside_gap_misses | 122.4 | 132.9 | +8.54% | 613.7 | 613.4 | 1.052–1.119 |
| disk | ipv4_lpm_misses | 122.3 | 135.0 | +10.39% | 605.3 | 607.2 | 1.047–1.163 |
| disk | ipv4_lpm_mixed | 445.7 | 463.0 | +3.88% | 760.9 | 762.1 | 1.010–1.068 |
| disk | ipv4_lpm_outside_cover_misses | 41.5 | 41.1 | -0.88% | 46.9 | 46.8 | 0.964–1.019 |
| disk | ipv6_lpm_hits | 532.4 | 541.7 | +1.76% | 1,253.7 | 1,243.5 | 0.995–1.041 |
| disk | ipv6_lpm_inside_gap_misses | 142.9 | 142.2 | -0.44% | 1,096.7 | 1,091.7 | 0.971–1.020 |
| disk | ipv6_lpm_misses | 146.1 | 142.8 | -2.28% | 1,086.1 | 1,061.8 | 0.872–1.095 |
| disk | ipv6_lpm_mixed | 524.0 | 529.6 | +1.07% | 1,256.5 | 1,231.1 | 0.945–1.081 |
| disk | ipv6_lpm_outside_cover_misses | 41.4 | 41.4 | +0.00% | 46.8 | 43.7 | 0.966–1.035 |
| disk | ordered_kv_hits | 202.6 | 204.6 | +1.02% | 333.6 | 337.8 | 0.976–1.045 |
| disk | ordered_kv_misses | 200.9 | 203.8 | +1.46% | 339.6 | 340.8 | 0.973–1.058 |
| disk | ordered_kv_mixed | 205.1 | 207.9 | +1.36% | 334.6 | 343.0 | 0.956–1.074 |
| ram | durable_batch1 | 22,339.9 | 22,896.5 | +2.49% | 8,400.8 | 8,602.0 | 0.860–1.221 |
| ram | durable_batch64 | 198,983.0 | 202,008.0 | +1.52% | 91,932.8 | 94,257.9 | 0.916–1.125 |
| ram | ipv4_lpm_hits | 454.9 | 476.9 | +4.82% | 778.2 | 789.8 | 0.962–1.143 |
| ram | ipv4_lpm_inside_gap_misses | 122.1 | 132.9 | +8.87% | 622.6 | 626.2 | 1.052–1.127 |
| ram | ipv4_lpm_misses | 122.8 | 133.0 | +8.30% | 621.1 | 610.1 | 1.031–1.137 |
| ram | ipv4_lpm_mixed | 447.6 | 463.6 | +3.57% | 767.2 | 768.9 | 0.998–1.075 |
| ram | ipv4_lpm_outside_cover_misses | 41.4 | 41.4 | -0.18% | 46.2 | 47.0 | 0.964–1.034 |
| ram | ipv6_lpm_hits | 540.1 | 553.4 | +2.46% | 1,272.0 | 1,263.9 | 0.975–1.077 |
| ram | ipv6_lpm_inside_gap_misses | 151.6 | 151.1 | -0.34% | 1,147.9 | 1,153.4 | 0.679–1.463 |
| ram | ipv6_lpm_misses | 145.5 | 152.4 | +4.80% | 1,088.9 | 1,145.1 | 0.698–1.574 |
| ram | ipv6_lpm_mixed | 517.9 | 526.5 | +1.68% | 1,242.2 | 1,236.5 | 0.983–1.052 |
| ram | ipv6_lpm_outside_cover_misses | 42.9 | 43.9 | +2.47% | 46.3 | 46.6 | 0.644–1.632 |
| ram | ordered_kv_hits | 203.3 | 205.2 | +0.97% | 334.9 | 339.5 | 0.978–1.043 |
| ram | ordered_kv_misses | 203.2 | 203.3 | +0.05% | 343.8 | 341.0 | 0.958–1.045 |
| ram | ordered_kv_mixed | 205.0 | 208.4 | +1.66% | 337.7 | 342.9 | 1.000–1.033 |

## Every other measured metric

Includes combined durable commit phases, setup, ULMDB reopen, file allocation and mapping observations. LMDB cold reopen is not measured by the existing suite; unavailable Resident pathname-attributed RSS remains unavailable.

| Metric | Role | Baseline | Candidate | Change | Paired ratio interval | Status |
|---|---|---:|---:|---:|---|---|
| full/disk/harness/setup/after_setup_ns (ns) | diagnostic | 2,141.032 | 1,878.470 | -12.26% | 0.1101–6.994 | inconclusive_interval |
| full/disk/harness/setup/dataset_ns (ns) | diagnostic | 248,314,618.683 | 239,217,998.154 | -3.66% | 0.7175–1.293 | inconclusive_interval |
| full/disk/harness/setup/fixture_load_both_ns (ns) | diagnostic | 15,552,827.648 | 14,298,904.394 | -8.06% | 0.6314–1.339 | inconclusive_interval |
| full/disk/lmdb/durable_batch1/combined_throughput (records/s) | control | 11,416.685 | 11,231.216 | -1.62% | 0.7558–1.281 | inconclusive_interval |
| full/disk/lmdb/durable_batch1/pooled_sampled_p99 (ns) | control | 304,116.110 | 307,268.781 | +1.04% | 0.4985–2.048 | inconclusive_interval |
| full/disk/lmdb/durable_batch64/combined_throughput (records/s) | control | 183,127.660 | 182,314.434 | -0.44% | 0.9171–1.081 | inconclusive_interval |
| full/disk/lmdb/durable_batch64/pooled_sampled_p99 (ns) | control | 526,644.831 | 532,454.090 | +1.10% | 0.9221–1.109 | inconclusive_interval |
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
| full/disk/lmdb/setup/create (ns) | control | 200,394.555 | 190,875.004 | -4.75% | 0.8191–1.108 | inconclusive_interval |
| full/disk/ulmdb/durable_batch1/combined_throughput (records/s) | target | 13,492.975 | 13,289.138 | -1.51% | 0.8911–1.089 | inconclusive_interval |
| full/disk/ulmdb/durable_batch1/pooled_sampled_p99 (ns) | target | 239,898.584 | 325,872.258 | +35.84% | 0.5251–3.514 | inconclusive_interval |
| full/disk/ulmdb/durable_batch64/combined_throughput (records/s) | target | 340,318.863 | 342,952.017 | +0.77% | 0.9122–1.113 | inconclusive_interval |
| full/disk/ulmdb/durable_batch64/pooled_sampled_p99 (ns) | target | 730,223.818 | 699,180.023 | -4.25% | 0.5976–1.534 | inconclusive_interval |
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
| full/disk/ulmdb/reopen/complete_cold_check_ns (ns) | target | 101,050,615.894 | 100,860,818.398 | -0.19% | 0.9196–1.083 | inconclusive_interval |
| full/disk/ulmdb/reopen/environment_drop_ns (ns) | target | 91,329.841 | 92,091.669 | +0.83% | 0.7575–1.342 | inconclusive_interval |
| full/disk/ulmdb/reopen/experimental_reenable_ns (ns) | target | 2,299.507 | 2,187.708 | -4.86% | 0.6741–1.343 | inconclusive_interval |
| full/disk/ulmdb/reopen/open_ns (ns) | target | 90,406,344.703 | 90,161,608.900 | -0.27% | 0.9133–1.089 | inconclusive_interval |
| full/disk/ulmdb/reopen/validation_ns (ns) | target | 6,373,164.595 | 6,367,926.867 | -0.08% | 0.975–1.024 | inconclusive_interval |
| full/disk/ulmdb/setup/create (ns) | target | 105,746,788.486 | 106,904,881.214 | +1.10% | 0.8689–1.176 | inconclusive_interval |
| full/ram/harness/setup/after_setup_ns (ns) | diagnostic | 1,752.832 | 2,078.622 | +18.59% | 0.1116–12.6 | inconclusive_interval |
| full/ram/harness/setup/dataset_ns (ns) | diagnostic | 240,027,109.002 | 239,611,847.986 | -0.17% | 0.9576–1.041 | inconclusive_interval |
| full/ram/harness/setup/fixture_load_both_ns (ns) | diagnostic | 13,637,288.646 | 13,232,734.726 | -2.97% | 0.8016–1.175 | inconclusive_interval |
| full/ram/lmdb/durable_batch1/combined_throughput (records/s) | control | 135,674.984 | 132,851.161 | -2.08% | 0.8625–1.112 | inconclusive_interval |
| full/ram/lmdb/durable_batch1/pooled_sampled_p99 (ns) | control | 9,767.565 | 9,531.644 | -2.42% | 0.6996–1.361 | inconclusive_interval |
| full/ram/lmdb/durable_batch64/combined_throughput (records/s) | control | 751,767.181 | 734,485.256 | -2.30% | 0.9108–1.048 | inconclusive_interval |
| full/ram/lmdb/durable_batch64/pooled_sampled_p99 (ns) | control | 99,442.149 | 104,836.405 | +5.42% | 0.8495–1.308 | inconclusive_interval |
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
| full/ram/lmdb/setup/create (ns) | control | 126,036.997 | 123,153.592 | -2.29% | 0.8541–1.118 | inconclusive_interval |
| full/ram/ulmdb/durable_batch1/combined_throughput (records/s) | target | 40,548.690 | 39,694.284 | -2.11% | 0.8497–1.128 | inconclusive_interval |
| full/ram/ulmdb/durable_batch1/pooled_sampled_p99 (ns) | target | 26,702.018 | 27,979.083 | +4.78% | 0.7183–1.529 | inconclusive_interval |
| full/ram/ulmdb/durable_batch64/combined_throughput (records/s) | target | 529,183.004 | 523,126.208 | -1.14% | 0.9081–1.076 | inconclusive_interval |
| full/ram/ulmdb/durable_batch64/pooled_sampled_p99 (ns) | target | 533,730.899 | 546,345.188 | +2.36% | 0.9504–1.102 | inconclusive_interval |
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
| full/ram/ulmdb/reopen/complete_cold_check_ns (ns) | target | 58,914,430.563 | 59,168,117.644 | +0.43% | 0.9809–1.028 | inconclusive_interval |
| full/ram/ulmdb/reopen/environment_drop_ns (ns) | target | 89,555.170 | 97,986.444 | +9.41% | 0.798–1.5 | inconclusive_interval |
| full/ram/ulmdb/reopen/experimental_reenable_ns (ns) | target | 2,289.921 | 2,379.737 | +3.92% | 0.7903–1.366 | inconclusive_interval |
| full/ram/ulmdb/reopen/open_ns (ns) | target | 48,293,389.026 | 48,431,032.322 | +0.29% | 0.9742–1.032 | inconclusive_interval |
| full/ram/ulmdb/reopen/validation_ns (ns) | target | 6,381,430.227 | 6,384,620.308 | +0.05% | 0.9585–1.044 | inconclusive_interval |
| full/ram/ulmdb/setup/create (ns) | target | 47,699,985.333 | 48,118,470.632 | +0.88% | 0.9121–1.116 | inconclusive_interval |

## Target metrics worse by more than 5%

- `full/disk/ulmdb/durable_batch1/pooled_sampled_p99`: +35.84% (inconclusive_interval).
- `full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99`: +8.54% (regression_interval).
- `full/disk/ulmdb/ipv4_lpm_misses/sampled_p99`: +10.39% (regression_interval).
- `full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99`: +8.87% (regression_interval).
- `full/ram/ulmdb/ipv4_lpm_misses/sampled_p99`: +8.30% (regression_interval).
- `full/ram/ulmdb/reopen/environment_drop_ns`: +9.41% (inconclusive_interval).

Source manifests, build logs, executable SHA-256 values, exact schedule, process reports, complete latency samples and their digests are retained beside this report. No existing public README, performance table or benchmark/results file is changed.
