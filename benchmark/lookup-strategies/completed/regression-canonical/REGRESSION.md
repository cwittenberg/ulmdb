# Existing-engine regression campaign

All 15 existing read/write cases on disk and RAM: **30 cases**, 9 fresh processes per arm/filesystem, 16 alternating ULMDB/LMDB trials within each process. Baseline/candidate processes run as adjacent AB/BA pairs.

This measures ordinary engine behavior with the optional lookup accelerator unused. It does not measure an application keeping a side index alive during writes, rebuilding it, or using it for other operations. The separate direct comparison qualifies accelerated reads.

All engines use normal durability flags. RAM uses real tmpfs/ramfs and does not provide reboot durability. Measurements are warm synthetic single-core results on a shared host. Native LMDB controls are retained without normalization or outlier removal.

Centers are process-level geometric means. Sampled p99 columns are geometric means of each trial's timer-inclusive sampled p99; they are not pooled population percentiles. Intervals use paired process log ratios with Bonferroni adjustment across all captured metrics and 9 process pairs. Overlapping intervals do not establish equivalence, and no universal no-regression guarantee follows.

ULMDB target metrics with adverse point estimates: 51. Target metrics worse by more than 5% at their observed centers: **5**. Full uncertainty, unavailable values and every raw observation are retained in comparison.json.

## Complete throughput table

| Filesystem | Case | Baseline ULMDB/s | Candidate ULMDB/s | Change | Baseline LMDB/s | Candidate LMDB/s | LMDB drift | Candidate vs local LMDB | Paired ratio interval |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|
| disk | durable_batch1 | 12,397 | 12,614 | +1.75% | 10,544 | 10,353 | -1.82% | +21.85% | 0.895–1.157 |
| disk | durable_batch64 | 342,822 | 330,646 | -3.55% | 177,542 | 172,181 | -3.02% | +92.03% | 0.785–1.185 |
| disk | ipv4_lpm_hits | 2,920,810 | 2,944,755 | +0.82% | 2,142,252 | 2,167,342 | +1.17% | +35.87% | 0.966–1.053 |
| disk | ipv4_lpm_inside_gap_misses | 10,809,143 | 10,312,989 | -4.59% | 1,959,121 | 1,935,220 | -1.22% | +432.91% | 0.676–1.346 |
| disk | ipv4_lpm_misses | 20,304,393 | 20,344,182 | +0.20% | 3,834,923 | 3,956,476 | +3.17% | +414.20% | 0.966–1.039 |
| disk | ipv4_lpm_mixed | 5,101,382 | 5,121,863 | +0.40% | 2,665,345 | 2,716,336 | +1.91% | +88.56% | 0.976–1.033 |
| disk | ipv4_lpm_outside_cover_misses | 99,746,625 | 99,444,215 | -0.30% | 91,427,811 | 88,985,692 | -2.67% | +11.75% | 0.949–1.047 |
| disk | ipv6_lpm_hits | 2,437,959 | 2,319,984 | -4.84% | 1,232,827 | 1,187,507 | -3.68% | +95.37% | 0.693–1.307 |
| disk | ipv6_lpm_inside_gap_misses | 8,985,043 | 9,174,153 | +2.10% | 1,098,250 | 1,124,780 | +2.42% | +715.64% | 0.959–1.087 |
| disk | ipv6_lpm_misses | 16,648,330 | 16,600,008 | -0.29% | 2,182,329 | 2,192,102 | +0.45% | +657.26% | 0.801–1.241 |
| disk | ipv6_lpm_mixed | 4,217,774 | 4,046,176 | -4.07% | 1,540,216 | 1,493,772 | -3.02% | +170.87% | 0.691–1.332 |
| disk | ipv6_lpm_outside_cover_misses | 92,133,098 | 96,523,178 | +4.76% | 89,627,488 | 90,366,914 | +0.82% | +6.81% | 0.827–1.328 |
| disk | ordered_kv_hits | 7,554,076 | 7,352,416 | -2.67% | 4,180,984 | 4,141,171 | -0.95% | +77.54% | 0.902–1.050 |
| disk | ordered_kv_misses | 7,511,534 | 7,429,129 | -1.10% | 4,016,204 | 4,039,319 | +0.58% | +83.92% | 0.973–1.006 |
| disk | ordered_kv_mixed | 7,256,621 | 7,180,933 | -1.04% | 4,100,664 | 4,122,809 | +0.54% | +74.18% | 0.975–1.004 |
| ram | durable_batch1 | 34,756 | 34,498 | -0.74% | 131,778 | 130,928 | -0.64% | -73.65% | 0.945–1.042 |
| ram | durable_batch64 | 537,259 | 538,330 | +0.20% | 742,305 | 738,554 | -0.51% | -27.11% | 0.974–1.031 |
| ram | ipv4_lpm_hits | 2,935,346 | 2,861,909 | -2.50% | 2,129,105 | 2,114,130 | -0.70% | +35.37% | 0.846–1.124 |
| ram | ipv4_lpm_inside_gap_misses | 10,929,018 | 10,869,444 | -0.55% | 2,021,879 | 2,048,989 | +1.34% | +430.48% | 0.962–1.028 |
| ram | ipv4_lpm_misses | 20,242,782 | 20,257,416 | +0.07% | 3,918,633 | 3,988,832 | +1.79% | +407.85% | 0.964–1.039 |
| ram | ipv4_lpm_mixed | 5,119,261 | 5,081,595 | -0.74% | 2,666,743 | 2,691,365 | +0.92% | +88.81% | 0.964–1.022 |
| ram | ipv4_lpm_outside_cover_misses | 100,516,892 | 99,780,496 | -0.73% | 90,146,182 | 95,976,766 | +6.47% | +3.96% | 0.948–1.040 |
| ram | ipv6_lpm_hits | 2,432,111 | 2,447,548 | +0.63% | 1,236,846 | 1,242,633 | +0.47% | +96.96% | 0.990–1.023 |
| ram | ipv6_lpm_inside_gap_misses | 9,215,113 | 9,166,286 | -0.53% | 1,128,072 | 1,140,830 | +1.13% | +703.48% | 0.958–1.033 |
| ram | ipv6_lpm_misses | 17,056,725 | 17,150,738 | +0.55% | 2,219,632 | 2,257,780 | +1.72% | +659.63% | 0.970–1.042 |
| ram | ipv6_lpm_mixed | 4,255,950 | 4,265,008 | +0.21% | 1,558,799 | 1,567,257 | +0.54% | +172.13% | 0.996–1.008 |
| ram | ipv6_lpm_outside_cover_misses | 96,433,547 | 96,591,741 | +0.16% | 89,013,943 | 95,190,831 | +6.94% | +1.47% | 0.949–1.057 |
| ram | ordered_kv_hits | 7,510,402 | 7,461,645 | -0.65% | 4,202,000 | 4,213,708 | +0.28% | +77.08% | 0.968–1.020 |
| ram | ordered_kv_misses | 7,505,412 | 7,446,119 | -0.79% | 4,006,258 | 4,023,364 | +0.43% | +85.07% | 0.974–1.010 |
| ram | ordered_kv_mixed | 7,235,053 | 7,180,930 | -0.75% | 4,104,463 | 4,110,360 | +0.14% | +74.70% | 0.967–1.018 |

## Complete sampled latency table

| Filesystem | Case | Baseline ULMDB p99 ns | Candidate ULMDB p99 ns | Change | Baseline LMDB p99 ns | Candidate LMDB p99 ns | Paired ratio interval |
|---|---|---:|---:|---:|---:|---:|---|
| disk | durable_batch1 | 95,846.4 | 97,449.3 | +1.67% | 203,992.0 | 203,244.6 | 0.726–1.424 |
| disk | durable_batch64 | 322,239.8 | 345,376.7 | +7.18% | 468,059.5 | 484,935.5 | 0.826–1.392 |
| disk | ipv4_lpm_hits | 476.3 | 470.0 | -1.32% | 781.1 | 776.7 | 0.892–1.092 |
| disk | ipv4_lpm_inside_gap_misses | 133.3 | 139.9 | +4.95% | 636.1 | 654.2 | 0.737–1.495 |
| disk | ipv4_lpm_misses | 131.8 | 132.8 | +0.76% | 617.1 | 618.3 | 0.960–1.058 |
| disk | ipv4_lpm_mixed | 468.1 | 463.6 | -0.98% | 774.0 | 763.0 | 0.938–1.045 |
| disk | ipv4_lpm_outside_cover_misses | 41.9 | 42.6 | +1.80% | 46.6 | 45.3 | 0.884–1.172 |
| disk | ipv6_lpm_hits | 546.8 | 570.7 | +4.38% | 1,269.0 | 1,302.0 | 0.742–1.468 |
| disk | ipv6_lpm_inside_gap_misses | 147.6 | 145.1 | -1.71% | 1,147.2 | 1,097.3 | 0.854–1.131 |
| disk | ipv6_lpm_misses | 148.3 | 146.2 | -1.43% | 1,121.3 | 1,088.3 | 0.780–1.245 |
| disk | ipv6_lpm_mixed | 539.7 | 552.7 | +2.41% | 1,280.9 | 1,294.8 | 0.743–1.412 |
| disk | ipv6_lpm_outside_cover_misses | 44.3 | 41.3 | -6.68% | 46.7 | 43.3 | 0.712–1.222 |
| disk | ordered_kv_hits | 200.8 | 208.2 | +3.71% | 336.7 | 338.8 | 0.907–1.185 |
| disk | ordered_kv_misses | 202.1 | 204.1 | +1.03% | 341.1 | 340.3 | 0.969–1.053 |
| disk | ordered_kv_mixed | 203.1 | 207.9 | +2.33% | 334.8 | 338.9 | 0.990–1.057 |
| ram | durable_batch1 | 22,379.9 | 22,233.8 | -0.65% | 8,396.8 | 8,377.9 | 0.940–1.050 |
| ram | durable_batch64 | 198,413.6 | 196,744.5 | -0.84% | 92,041.7 | 92,443.9 | 0.968–1.016 |
| ram | ipv4_lpm_hits | 469.5 | 488.3 | +4.01% | 785.9 | 790.9 | 0.854–1.267 |
| ram | ipv4_lpm_inside_gap_misses | 131.9 | 132.0 | +0.13% | 620.1 | 605.7 | 0.974–1.029 |
| ram | ipv4_lpm_misses | 133.2 | 132.9 | -0.24% | 615.2 | 596.8 | 0.963–1.034 |
| ram | ipv4_lpm_mixed | 465.8 | 467.1 | +0.27% | 771.9 | 762.1 | 0.951–1.058 |
| ram | ipv4_lpm_outside_cover_misses | 41.2 | 41.2 | +0.00% | 44.9 | 48.0 | 0.980–1.020 |
| ram | ipv6_lpm_hits | 542.8 | 539.4 | -0.63% | 1,247.8 | 1,232.2 | 0.960–1.029 |
| ram | ipv6_lpm_inside_gap_misses | 142.5 | 143.9 | +0.97% | 1,088.3 | 1,071.9 | 0.935–1.090 |
| ram | ipv6_lpm_misses | 142.8 | 143.1 | +0.25% | 1,077.2 | 1,059.5 | 0.933–1.077 |
| ram | ipv6_lpm_mixed | 526.1 | 523.5 | -0.49% | 1,227.9 | 1,217.8 | 0.966–1.025 |
| ram | ipv6_lpm_outside_cover_misses | 41.6 | 41.2 | -0.91% | 43.5 | 44.4 | 0.963–1.020 |
| ram | ordered_kv_hits | 200.9 | 204.2 | +1.63% | 334.4 | 336.2 | 0.967–1.068 |
| ram | ordered_kv_misses | 202.0 | 203.3 | +0.65% | 344.1 | 339.6 | 0.972–1.043 |
| ram | ordered_kv_mixed | 204.9 | 209.4 | +2.23% | 340.9 | 337.4 | 0.972–1.075 |

## Every other measured metric

Includes combined durable commit phases, setup, ULMDB reopen, file allocation and mapping observations. LMDB cold reopen is not measured by the existing suite; unavailable Resident pathname-attributed RSS remains unavailable.

| Metric | Role | Baseline | Candidate | Change | Paired ratio interval | Status |
|---|---|---:|---:|---:|---|---|
| full/disk/harness/setup/after_setup_ns (ns) | diagnostic | 2,199.728 | 2,290.053 | +4.11% | 0.1348–8.038 | inconclusive_interval |
| full/disk/harness/setup/dataset_ns (ns) | diagnostic | 249,557,676.128 | 241,279,832.564 | -3.32% | 0.7396–1.264 | inconclusive_interval |
| full/disk/harness/setup/fixture_load_both_ns (ns) | diagnostic | 14,358,307.368 | 14,326,042.372 | -0.22% | 0.9764–1.02 | inconclusive_interval |
| full/disk/lmdb/durable_batch1/combined_throughput (records/s) | control | 11,099.681 | 10,936.305 | -1.47% | 0.8653–1.122 | inconclusive_interval |
| full/disk/lmdb/durable_batch1/pooled_sampled_p99 (ns) | control | 330,185.456 | 328,324.088 | -0.56% | 0.956–1.034 | inconclusive_interval |
| full/disk/lmdb/durable_batch64/combined_throughput (records/s) | control | 181,757.251 | 176,957.162 | -2.64% | 0.8663–1.094 | inconclusive_interval |
| full/disk/lmdb/durable_batch64/pooled_sampled_p99 (ns) | control | 535,535.501 | 540,636.860 | +0.95% | 0.881–1.157 | inconclusive_interval |
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
| full/disk/lmdb/setup/create (ns) | control | 195,984.586 | 199,564.094 | +1.83% | 0.8917–1.163 | inconclusive_interval |
| full/disk/ulmdb/durable_batch1/combined_throughput (records/s) | target | 13,214.587 | 13,312.628 | +0.74% | 0.885–1.147 | inconclusive_interval |
| full/disk/ulmdb/durable_batch1/pooled_sampled_p99 (ns) | target | 217,012.589 | 323,123.799 | +48.90% | 0.5281–4.198 | inconclusive_interval |
| full/disk/ulmdb/durable_batch64/combined_throughput (records/s) | target | 343,127.252 | 331,654.957 | -3.34% | 0.7942–1.176 | inconclusive_interval |
| full/disk/ulmdb/durable_batch64/pooled_sampled_p99 (ns) | target | 701,375.165 | 720,183.168 | +2.68% | 0.7114–1.482 | inconclusive_interval |
| full/disk/ulmdb/footprint/after/allocated_file_bytes (bytes) | target | 67,110,229.306 | 67,110,684.414 | +0.00% | 1–1 | inconclusive_interval |
| full/disk/ulmdb/footprint/after/apparent_file_bytes (bytes) | target | 67,108,864.000 | 67,108,864.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/ulmdb/footprint/after/files (files) | target | 2.000 | 2.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/ulmdb/footprint/after/mapped_pss_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/disk/ulmdb/footprint/after/mapped_rss_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/disk/ulmdb/footprint/after/mapped_virtual_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/disk/ulmdb/footprint/before/allocated_file_bytes (bytes) | target | 67,110,229.306 | 67,110,684.414 | +0.00% | 1–1 | inconclusive_interval |
| full/disk/ulmdb/footprint/before/apparent_file_bytes (bytes) | target | 67,108,864.000 | 67,108,864.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/ulmdb/footprint/before/files (files) | target | 2.000 | 2.000 | +0.00% | 1–1 | identical_observed_changes |
| full/disk/ulmdb/footprint/before/mapped_pss_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/disk/ulmdb/footprint/before/mapped_rss_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/disk/ulmdb/footprint/before/mapped_virtual_kib (KiB) | diagnostic | Unavailable | Unavailable | N/A | Unavailable | unavailable |
| full/disk/ulmdb/reopen/complete_cold_check_ns (ns) | target | 100,549,242.839 | 105,990,764.615 | +5.41% | 0.833–1.334 | inconclusive_interval |
| full/disk/ulmdb/reopen/environment_drop_ns (ns) | target | 96,205.830 | 100,814.298 | +4.79% | 0.8184–1.342 | inconclusive_interval |
| full/disk/ulmdb/reopen/experimental_reenable_ns (ns) | target | 2,628.358 | 2,170.033 | -17.44% | 0.2698–2.527 | inconclusive_interval |
| full/disk/ulmdb/reopen/open_ns (ns) | target | 89,935,206.390 | 94,368,934.932 | +4.93% | 0.8463–1.301 | inconclusive_interval |
| full/disk/ulmdb/reopen/validation_ns (ns) | target | 6,318,239.190 | 6,955,272.835 | +10.08% | 0.7408–1.636 | inconclusive_interval |
| full/disk/ulmdb/setup/create (ns) | target | 108,063,655.472 | 106,071,446.773 | -1.84% | 0.9188–1.049 | inconclusive_interval |
| full/ram/harness/setup/after_setup_ns (ns) | diagnostic | 1,091.946 | 1,686.894 | +54.49% | 0.2459–9.704 | inconclusive_interval |
| full/ram/harness/setup/dataset_ns (ns) | diagnostic | 237,365,745.165 | 239,606,201.047 | +0.94% | 0.9799–1.04 | inconclusive_interval |
| full/ram/harness/setup/fixture_load_both_ns (ns) | diagnostic | 13,205,818.638 | 13,127,381.607 | -0.59% | 0.9784–1.01 | inconclusive_interval |
| full/ram/lmdb/durable_batch1/combined_throughput (records/s) | control | 136,223.358 | 135,536.029 | -0.50% | 0.9792–1.011 | inconclusive_interval |
| full/ram/lmdb/durable_batch1/pooled_sampled_p99 (ns) | control | 9,888.295 | 9,814.090 | -0.75% | 0.5063–1.945 | inconclusive_interval |
| full/ram/lmdb/durable_batch64/combined_throughput (records/s) | control | 750,146.293 | 747,711.613 | -0.32% | 0.9776–1.016 | inconclusive_interval |
| full/ram/lmdb/durable_batch64/pooled_sampled_p99 (ns) | control | 98,805.895 | 98,878.496 | +0.07% | 0.9261–1.081 | inconclusive_interval |
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
| full/ram/lmdb/setup/create (ns) | control | 122,792.326 | 125,090.375 | +1.87% | 0.8012–1.295 | inconclusive_interval |
| full/ram/ulmdb/durable_batch1/combined_throughput (records/s) | target | 40,653.468 | 40,519.755 | -0.33% | 0.9697–1.024 | inconclusive_interval |
| full/ram/ulmdb/durable_batch1/pooled_sampled_p99 (ns) | target | 27,131.070 | 26,436.815 | -2.56% | 0.7537–1.26 | inconclusive_interval |
| full/ram/ulmdb/durable_batch64/combined_throughput (records/s) | target | 531,899.033 | 532,940.186 | +0.20% | 0.9813–1.023 | inconclusive_interval |
| full/ram/ulmdb/durable_batch64/pooled_sampled_p99 (ns) | target | 537,563.848 | 539,136.613 | +0.29% | 0.9674–1.04 | inconclusive_interval |
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
| full/ram/ulmdb/reopen/complete_cold_check_ns (ns) | target | 59,108,009.240 | 59,280,601.306 | +0.29% | 0.9817–1.025 | inconclusive_interval |
| full/ram/ulmdb/reopen/environment_drop_ns (ns) | target | 93,387.182 | 93,323.482 | -0.07% | 0.7616–1.311 | inconclusive_interval |
| full/ram/ulmdb/reopen/experimental_reenable_ns (ns) | target | 2,130.393 | 2,294.854 | +7.72% | 0.768–1.511 | inconclusive_interval |
| full/ram/ulmdb/reopen/open_ns (ns) | target | 48,472,248.775 | 48,655,334.504 | +0.38% | 0.9826–1.025 | inconclusive_interval |
| full/ram/ulmdb/reopen/validation_ns (ns) | target | 6,376,706.033 | 6,376,215.342 | -0.01% | 0.9627–1.039 | inconclusive_interval |
| full/ram/ulmdb/setup/create (ns) | target | 48,024,020.366 | 48,264,185.872 | +0.50% | 0.9398–1.075 | inconclusive_interval |

## Target metrics worse by more than 5%

- `full/disk/ulmdb/durable_batch1/pooled_sampled_p99`: +48.90% (inconclusive_interval).
- `full/disk/ulmdb/durable_batch64/sampled_p99`: +7.18% (inconclusive_interval).
- `full/disk/ulmdb/reopen/complete_cold_check_ns`: +5.41% (inconclusive_interval).
- `full/disk/ulmdb/reopen/validation_ns`: +10.08% (inconclusive_interval).
- `full/ram/ulmdb/reopen/experimental_reenable_ns`: +7.72% (inconclusive_interval).

Source manifests, build logs, executable SHA-256 values, exact schedule, process reports, complete latency samples and their digests are retained beside this report. No existing public README, performance table or benchmark/results file is changed.
