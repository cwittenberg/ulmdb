# Benchmark comparison

92 ULMDB metrics; 204 total including controls.

Adverse ULMDB point estimates: 55; adverse intervals: 6; consistent observed increases/decreases: 0; insufficient repeats: 0; unavailable: 0.

descriptive regression screen; no equivalence, universal no-regression, or causal proof; all adverse estimates retained; no outlier exclusion or local-LMDB normalization.

Change is candidate versus baseline. Throughput increases are favorable; cost increases are adverse. Intervals describe process variation under the stated model, not population latency percentiles.

|Metric|Role|Baseline|Candidate|Change|Interval|Status / flags|
|---|---|---:|---:|---:|---|---|
|full/disk/harness/setup/after_setup_ns (ns)|diagnostic|2,141.032|1,878.470|-12.26%|[0.11007, 6.9938] ratio|inconclusive_interval|
|full/disk/harness/setup/dataset_ns (ns)|diagnostic|248,314,618.683|239,217,998.154|-3.66%|[0.71752, 1.2935] ratio|inconclusive_interval|
|full/disk/harness/setup/fixture_load_both_ns (ns)|diagnostic|15,552,827.648|14,298,904.394|-8.06%|[0.6314, 1.3387] ratio|inconclusive_interval|
|full/disk/lmdb/durable_batch1/combined_throughput (records/s)|control|11,416.685|11,231.216|-1.62%|[0.75575, 1.2805] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch1/pooled_sampled_p99 (ns)|control|304,116.110|307,268.781|+1.04%|[0.4985, 2.0478] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch1/sampled_p99 (ns)|control|188,785.096|199,481.696|+5.67%|[0.61033, 1.8294] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch1/throughput (records/s)|control|10,810.441|10,752.141|-0.54%|[0.72824, 1.3584] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch64/combined_throughput (records/s)|control|183,127.660|182,314.434|-0.44%|[0.91712, 1.0807] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch64/pooled_sampled_p99 (ns)|control|526,644.831|532,454.090|+1.10%|[0.9221, 1.1085] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch64/sampled_p99 (ns)|control|459,800.382|463,319.245|+0.77%|[0.9024, 1.1252] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch64/throughput (records/s)|control|178,473.704|177,408.853|-0.60%|[0.91079, 1.0849] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/footprint/after/allocated_file_bytes (bytes)|control|729,088.000|729,088.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/lmdb/footprint/after/apparent_file_bytes (bytes)|control|733,184.000|733,184.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/lmdb/footprint/after/files (files)|control|2.000|2.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/lmdb/footprint/after/mapped_pss_kib (KiB)|diagnostic|712.000|712.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/lmdb/footprint/after/mapped_rss_kib (KiB)|diagnostic|712.000|712.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/lmdb/footprint/after/mapped_virtual_kib (KiB)|diagnostic|524,296.000|524,296.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/lmdb/footprint/before/allocated_file_bytes (bytes)|control|352,256.000|352,256.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/lmdb/footprint/before/apparent_file_bytes (bytes)|control|356,352.000|356,352.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/lmdb/footprint/before/files (files)|control|2.000|2.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/lmdb/footprint/before/mapped_pss_kib (KiB)|diagnostic|12.000|12.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/lmdb/footprint/before/mapped_rss_kib (KiB)|diagnostic|12.000|12.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/lmdb/footprint/before/mapped_virtual_kib (KiB)|diagnostic|524,296.000|524,296.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/lmdb/ipv4_lpm_hits/sampled_p99 (ns)|control|781.857|784.316|+0.31%|[0.90911, 1.1069] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv4_lpm_hits/throughput (queries/s)|control|2,148,602.924|2,143,599.663|-0.23%|[0.97072, 1.0254] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99 (ns)|control|613.698|613.431|-0.04%|[0.88702, 1.1264] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/throughput (queries/s)|control|2,034,212.446|2,088,372.496|+2.66%|[0.95166, 1.1075] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_misses/sampled_p99 (ns)|control|605.289|607.222|+0.32%|[0.87009, 1.1567] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv4_lpm_misses/throughput (queries/s)|control|3,931,278.594|4,049,046.951|+3.00%|[0.94982, 1.1169] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_mixed/sampled_p99 (ns)|control|760.906|762.137|+0.16%|[0.96722, 1.0372] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv4_lpm_mixed/throughput (queries/s)|control|2,689,419.031|2,718,601.098|+1.09%|[0.95333, 1.0718] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99 (ns)|control|46.896|46.820|-0.16%|[0.81668, 1.2205] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/throughput (queries/s)|control|93,222,307.547|93,597,106.446|+0.40%|[0.78883, 1.2779] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_hits/sampled_p99 (ns)|control|1,253.668|1,243.536|-0.81%|[0.94571, 1.0404] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_hits/throughput (queries/s)|control|1,238,117.959|1,239,304.799|+0.10%|[0.95889, 1.0449] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99 (ns)|control|1,096.691|1,091.652|-0.46%|[0.94234, 1.0515] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/throughput (queries/s)|control|1,137,409.349|1,131,306.786|-0.54%|[0.93552, 1.0575] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_misses/sampled_p99 (ns)|control|1,086.062|1,061.820|-2.23%|[0.87789, 1.0888] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_misses/throughput (queries/s)|control|2,217,467.154|2,239,353.734|+0.99%|[0.90383, 1.1283] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_mixed/sampled_p99 (ns)|control|1,256.475|1,231.105|-2.02%|[0.911, 1.0538] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_mixed/throughput (queries/s)|control|1,554,271.771|1,562,424.064|+0.52%|[0.95163, 1.0619] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99 (ns)|control|46.774|43.701|-6.57%|[0.78157, 1.1169] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/throughput (queries/s)|control|93,749,084.717|93,820,386.345|+0.08%|[0.84628, 1.1834] ratio|inconclusive_interval|
|full/disk/lmdb/ordered_kv_hits/sampled_p99 (ns)|control|333.593|337.790|+1.26%|[0.96457, 1.063] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ordered_kv_hits/throughput (queries/s)|control|4,121,960.970|4,191,849.884|+1.70%|[1.0087, 1.0253] ratio|improvement_interval|
|full/disk/lmdb/ordered_kv_misses/sampled_p99 (ns)|control|339.640|340.752|+0.33%|[0.9736, 1.0338] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ordered_kv_misses/throughput (queries/s)|control|4,001,284.956|4,029,584.700|+0.71%|[0.99042, 1.024] ratio|inconclusive_interval|
|full/disk/lmdb/ordered_kv_mixed/sampled_p99 (ns)|control|334.565|343.008|+2.52%|[0.97689, 1.076] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ordered_kv_mixed/throughput (queries/s)|control|4,032,170.991|4,089,660.772|+1.43%|[1.0023, 1.0263] ratio|improvement_interval|
|full/disk/lmdb/setup/create (ns)|control|200,394.555|190,875.004|-4.75%|[0.81909, 1.1076] ratio|inconclusive_interval|
|full/disk/ulmdb/durable_batch1/combined_throughput (records/s)|target|13,492.975|13,289.138|-1.51%|[0.89115, 1.0885] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99 (ns)|target|239,898.584|325,872.258|+35.84%|[0.52508, 3.5141] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch1/sampled_p99 (ns)|target|96,387.681|99,883.004|+3.63%|[0.81528, 1.3171] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch1/throughput (records/s)|target|12,761.670|12,624.819|-1.07%|[0.8868, 1.1036] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch64/combined_throughput (records/s)|target|340,318.863|342,952.017|+0.77%|[0.91218, 1.1133] ratio|inconclusive_interval|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99 (ns)|target|730,223.818|699,180.023|-4.25%|[0.59761, 1.5341] ratio|inconclusive_interval|
|full/disk/ulmdb/durable_batch64/sampled_p99 (ns)|target|337,251.630|322,223.706|-4.46%|[0.80789, 1.1299] ratio|inconclusive_interval|
|full/disk/ulmdb/durable_batch64/throughput (records/s)|target|340,473.600|342,634.747|+0.63%|[0.89927, 1.1262] ratio|inconclusive_interval|
|full/disk/ulmdb/footprint/after/allocated_file_bytes (bytes)|target|67,108,864.000|67,108,864.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/ulmdb/footprint/after/apparent_file_bytes (bytes)|target|67,108,864.000|67,108,864.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/ulmdb/footprint/after/files (files)|target|2.000|2.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/ulmdb/footprint/after/mapped_pss_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/disk/ulmdb/footprint/after/mapped_rss_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/disk/ulmdb/footprint/after/mapped_virtual_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/disk/ulmdb/footprint/before/allocated_file_bytes (bytes)|target|67,108,864.000|67,108,864.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/ulmdb/footprint/before/apparent_file_bytes (bytes)|target|67,108,864.000|67,108,864.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/ulmdb/footprint/before/files (files)|target|2.000|2.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/ulmdb/footprint/before/mapped_pss_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/disk/ulmdb/footprint/before/mapped_rss_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/disk/ulmdb/footprint/before/mapped_virtual_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99 (ns)|target|457.528|471.879|+3.14%|[0.91426, 1.1635] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv4_lpm_hits/throughput (queries/s)|target|3,020,331.757|2,988,297.326|-1.06%|[0.9282, 1.0546] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99 (ns)|target|122.435|132.888|+8.54%|[1.0525, 1.1193] ratio|regression_interval; adverse_point_estimate, regression_interval|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput (queries/s)|target|10,941,033.333|10,833,910.899|-0.98%|[0.97122, 1.0096] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99 (ns)|target|122.291|134.993|+10.39%|[1.0473, 1.1634] ratio|regression_interval; adverse_point_estimate, regression_interval|
|full/disk/ulmdb/ipv4_lpm_misses/throughput (queries/s)|target|20,486,877.770|20,282,956.103|-1.00%|[0.94115, 1.0415] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99 (ns)|target|445.668|462.963|+3.88%|[1.0102, 1.0682] ratio|regression_interval; adverse_point_estimate, regression_interval|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput (queries/s)|target|5,154,490.601|5,193,109.267|+0.75%|[0.99364, 1.0215] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99 (ns)|target|41.483|41.119|-0.88%|[0.96394, 1.0193] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput (queries/s)|target|99,399,754.222|100,125,146.268|+0.73%|[0.96326, 1.0534] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99 (ns)|target|532.363|541.710|+1.76%|[0.99496, 1.0407] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv6_lpm_hits/throughput (queries/s)|target|2,503,433.059|2,509,022.273|+0.22%|[0.98757, 1.0171] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99 (ns)|target|142.873|142.244|-0.44%|[0.97147, 1.0203] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput (queries/s)|target|9,144,786.232|9,216,478.248|+0.78%|[0.97388, 1.043] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99 (ns)|target|146.126|142.797|-2.28%|[0.87201, 1.0951] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_misses/throughput (queries/s)|target|16,802,625.859|17,174,363.555|+2.21%|[0.94631, 1.104] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99 (ns)|target|524.030|529.641|+1.07%|[0.94504, 1.0809] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput (queries/s)|target|4,327,555.493|4,369,224.800|+0.96%|[0.96675, 1.0544] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99 (ns)|target|41.420|41.420|+0.00%|[0.96576, 1.0355] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput (queries/s)|target|96,987,288.864|95,856,237.219|-1.17%|[0.91123, 1.072] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99 (ns)|target|202.564|204.620|+1.02%|[0.97637, 1.0451] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ordered_kv_hits/throughput (queries/s)|target|7,511,187.433|7,502,710.410|-0.11%|[0.98224, 1.0158] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99 (ns)|target|200.902|203.832|+1.46%|[0.97303, 1.0579] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ordered_kv_misses/throughput (queries/s)|target|7,494,176.340|7,446,753.499|-0.63%|[0.96941, 1.0185] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99 (ns)|target|205.109|207.903|+1.36%|[0.95631, 1.0744] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ordered_kv_mixed/throughput (queries/s)|target|7,215,068.518|7,177,429.573|-0.52%|[0.97552, 1.0144] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/reopen/complete_cold_check_ns (ns)|target|101,050,615.894|100,860,818.398|-0.19%|[0.91957, 1.0834] ratio|inconclusive_interval|
|full/disk/ulmdb/reopen/environment_drop_ns (ns)|target|91,329.841|92,091.669|+0.83%|[0.75751, 1.3422] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/reopen/experimental_reenable_ns (ns)|target|2,299.507|2,187.708|-4.86%|[0.67413, 1.3427] ratio|inconclusive_interval|
|full/disk/ulmdb/reopen/open_ns (ns)|target|90,406,344.703|90,161,608.900|-0.27%|[0.91333, 1.089] ratio|inconclusive_interval|
|full/disk/ulmdb/reopen/validation_ns (ns)|target|6,373,164.595|6,367,926.867|-0.08%|[0.97499, 1.024] ratio|inconclusive_interval|
|full/disk/ulmdb/setup/create (ns)|target|105,746,788.486|106,904,881.214|+1.10%|[0.86885, 1.1763] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/harness/setup/after_setup_ns (ns)|diagnostic|1,752.832|2,078.622|+18.59%|[0.1116, 12.601] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/harness/setup/dataset_ns (ns)|diagnostic|240,027,109.002|239,611,847.986|-0.17%|[0.95756, 1.0407] ratio|inconclusive_interval|
|full/ram/harness/setup/fixture_load_both_ns (ns)|diagnostic|13,637,288.646|13,232,734.726|-2.97%|[0.80156, 1.1746] ratio|inconclusive_interval|
|full/ram/lmdb/durable_batch1/combined_throughput (records/s)|control|135,674.984|132,851.161|-2.08%|[0.86255, 1.1116] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch1/pooled_sampled_p99 (ns)|control|9,767.565|9,531.644|-2.42%|[0.6996, 1.3612] ratio|inconclusive_interval|
|full/ram/lmdb/durable_batch1/sampled_p99 (ns)|control|8,400.845|8,601.974|+2.39%|[0.86055, 1.2184] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch1/throughput (records/s)|control|130,797.154|128,441.826|-1.80%|[0.85179, 1.1321] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch64/combined_throughput (records/s)|control|751,767.181|734,485.256|-2.30%|[0.91075, 1.0481] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch64/pooled_sampled_p99 (ns)|control|99,442.149|104,836.405|+5.42%|[0.84946, 1.3084] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch64/sampled_p99 (ns)|control|91,932.842|94,257.910|+2.53%|[0.94551, 1.1118] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch64/throughput (records/s)|control|743,719.435|725,456.676|-2.46%|[0.90575, 1.0505] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/footprint/after/allocated_file_bytes (bytes)|control|729,088.000|729,088.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/lmdb/footprint/after/apparent_file_bytes (bytes)|control|733,184.000|733,184.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/lmdb/footprint/after/files (files)|control|2.000|2.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/lmdb/footprint/after/mapped_pss_kib (KiB)|diagnostic|712.000|712.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/lmdb/footprint/after/mapped_rss_kib (KiB)|diagnostic|712.000|712.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/lmdb/footprint/after/mapped_virtual_kib (KiB)|diagnostic|524,296.000|524,296.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/lmdb/footprint/before/allocated_file_bytes (bytes)|control|352,256.000|352,256.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/lmdb/footprint/before/apparent_file_bytes (bytes)|control|356,352.000|356,352.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/lmdb/footprint/before/files (files)|control|2.000|2.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/lmdb/footprint/before/mapped_pss_kib (KiB)|diagnostic|12.000|12.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/lmdb/footprint/before/mapped_rss_kib (KiB)|diagnostic|12.000|12.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/lmdb/footprint/before/mapped_virtual_kib (KiB)|diagnostic|524,296.000|524,296.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/lmdb/ipv4_lpm_hits/sampled_p99 (ns)|control|778.241|789.826|+1.49%|[0.94457, 1.0904] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv4_lpm_hits/throughput (queries/s)|control|2,131,154.617|2,131,060.493|-0.00%|[0.9824, 1.0178] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99 (ns)|control|622.565|626.190|+0.58%|[0.86717, 1.1666] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/throughput (queries/s)|control|1,977,647.127|1,986,838.069|+0.46%|[0.96065, 1.0507] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_misses/sampled_p99 (ns)|control|621.063|610.063|-1.77%|[0.85671, 1.1263] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_misses/throughput (queries/s)|control|3,820,117.001|3,879,245.392|+1.55%|[0.92726, 1.1121] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_mixed/sampled_p99 (ns)|control|767.244|768.856|+0.21%|[0.94511, 1.0625] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv4_lpm_mixed/throughput (queries/s)|control|2,636,287.360|2,656,391.267|+0.76%|[0.9558, 1.0623] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99 (ns)|control|46.184|47.008|+1.78%|[0.8219, 1.2605] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/throughput (queries/s)|control|86,759,443.760|92,906,369.763|+7.09%|[0.74017, 1.5493] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_hits/sampled_p99 (ns)|control|1,271.986|1,263.892|-0.64%|[0.90662, 1.089] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_hits/throughput (queries/s)|control|1,238,872.186|1,230,759.034|-0.65%|[0.93332, 1.0575] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99 (ns)|control|1,147.904|1,153.425|+0.48%|[0.75487, 1.3375] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/throughput (queries/s)|control|1,083,566.704|1,080,684.060|-0.27%|[0.70118, 1.4186] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_misses/sampled_p99 (ns)|control|1,088.888|1,145.100|+5.16%|[0.74066, 1.4931] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_misses/throughput (queries/s)|control|2,208,511.775|2,114,342.693|-4.26%|[0.68048, 1.3469] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_mixed/sampled_p99 (ns)|control|1,242.194|1,236.504|-0.46%|[0.931, 1.0643] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_mixed/throughput (queries/s)|control|1,563,031.710|1,561,163.040|-0.12%|[0.96444, 1.0344] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99 (ns)|control|46.303|46.585|+0.61%|[0.68268, 1.4828] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/throughput (queries/s)|control|85,650,319.614|89,770,535.309|+4.81%|[0.66696, 1.6471] ratio|inconclusive_interval|
|full/ram/lmdb/ordered_kv_hits/sampled_p99 (ns)|control|334.933|339.524|+1.37%|[0.97738, 1.0514] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ordered_kv_hits/throughput (queries/s)|control|4,113,317.886|4,159,189.061|+1.12%|[0.9812, 1.042] ratio|inconclusive_interval|
|full/ram/lmdb/ordered_kv_misses/sampled_p99 (ns)|control|343.824|340.951|-0.84%|[0.93936, 1.0468] ratio|inconclusive_interval|
|full/ram/lmdb/ordered_kv_misses/throughput (queries/s)|control|3,985,441.675|4,019,771.518|+0.86%|[0.97772, 1.0405] ratio|inconclusive_interval|
|full/ram/lmdb/ordered_kv_mixed/sampled_p99 (ns)|control|337.654|342.950|+1.57%|[0.98802, 1.0441] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ordered_kv_mixed/throughput (queries/s)|control|4,028,813.365|4,073,531.871|+1.11%|[0.98525, 1.0376] ratio|inconclusive_interval|
|full/ram/lmdb/setup/create (ns)|control|126,036.997|123,153.592|-2.29%|[0.85408, 1.1179] ratio|inconclusive_interval|
|full/ram/ulmdb/durable_batch1/combined_throughput (records/s)|target|40,548.690|39,694.284|-2.11%|[0.84972, 1.1278] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99 (ns)|target|26,702.018|27,979.083|+4.78%|[0.7183, 1.5285] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch1/sampled_p99 (ns)|target|22,339.862|22,896.510|+2.49%|[0.86007, 1.2214] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch1/throughput (records/s)|target|34,537.332|33,879.596|-1.90%|[0.84208, 1.1427] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch64/combined_throughput (records/s)|target|529,183.004|523,126.208|-1.14%|[0.90812, 1.0761] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99 (ns)|target|533,730.899|546,345.188|+2.36%|[0.95044, 1.1025] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch64/sampled_p99 (ns)|target|198,983.004|202,008.002|+1.52%|[0.91576, 1.1254] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch64/throughput (records/s)|target|534,812.049|528,534.168|-1.17%|[0.89984, 1.0854] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/footprint/after/allocated_file_bytes (bytes)|target|67,108,864.000|67,108,864.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/ulmdb/footprint/after/apparent_file_bytes (bytes)|target|67,108,864.000|67,108,864.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/ulmdb/footprint/after/files (files)|target|2.000|2.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/ulmdb/footprint/after/mapped_pss_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/ram/ulmdb/footprint/after/mapped_rss_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/ram/ulmdb/footprint/after/mapped_virtual_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/ram/ulmdb/footprint/before/allocated_file_bytes (bytes)|target|67,108,864.000|67,108,864.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/ulmdb/footprint/before/apparent_file_bytes (bytes)|target|67,108,864.000|67,108,864.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/ulmdb/footprint/before/files (files)|target|2.000|2.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/ram/ulmdb/footprint/before/mapped_pss_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/ram/ulmdb/footprint/before/mapped_rss_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/ram/ulmdb/footprint/before/mapped_virtual_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99 (ns)|target|454.939|476.888|+4.82%|[0.96153, 1.1428] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_hits/throughput (queries/s)|target|3,016,328.439|2,971,511.357|-1.49%|[0.90959, 1.067] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99 (ns)|target|122.054|132.876|+8.87%|[1.0516, 1.1271] ratio|regression_interval; adverse_point_estimate, regression_interval|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput (queries/s)|target|10,947,942.930|10,800,880.100|-1.34%|[0.96697, 1.0066] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99 (ns)|target|122.820|133.011|+8.30%|[1.0314, 1.1371] ratio|regression_interval; adverse_point_estimate, regression_interval|
|full/ram/ulmdb/ipv4_lpm_misses/throughput (queries/s)|target|20,566,340.340|20,404,799.585|-0.79%|[0.97339, 1.0113] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99 (ns)|target|447.625|463.609|+3.57%|[0.99801, 1.0748] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput (queries/s)|target|5,143,750.971|5,190,395.010|+0.91%|[0.98307, 1.0358] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99 (ns)|target|41.426|41.352|-0.18%|[0.96359, 1.0341] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput (queries/s)|target|99,799,377.377|99,893,244.608|+0.09%|[0.93222, 1.0747] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99 (ns)|target|540.136|553.434|+2.46%|[0.97476, 1.077] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_hits/throughput (queries/s)|target|2,512,354.986|2,493,953.363|-0.73%|[0.94947, 1.0378] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99 (ns)|target|151.598|151.080|-0.34%|[0.67909, 1.4625] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput (queries/s)|target|8,654,287.414|8,792,588.554|+1.60%|[0.68521, 1.5064] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99 (ns)|target|145.456|152.431|+4.80%|[0.69791, 1.5736] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_misses/throughput (queries/s)|target|16,839,249.340|16,325,401.987|-3.05%|[0.68611, 1.3699] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99 (ns)|target|517.856|526.545|+1.68%|[0.98313, 1.0516] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput (queries/s)|target|4,358,108.860|4,375,289.386|+0.39%|[0.99279, 1.0152] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99 (ns)|target|42.874|43.934|+2.47%|[0.6436, 1.6316] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput (queries/s)|target|93,576,822.983|93,269,230.381|-0.33%|[0.7042, 1.4107] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99 (ns)|target|203.263|205.241|+0.97%|[0.97773, 1.0428] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_hits/throughput (queries/s)|target|7,523,619.818|7,470,209.351|-0.71%|[0.97899, 1.007] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99 (ns)|target|203.235|203.334|+0.05%|[0.95801, 1.0448] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_misses/throughput (queries/s)|target|7,452,206.646|7,424,078.824|-0.38%|[0.97645, 1.0164] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99 (ns)|target|204.978|208.384|+1.66%|[1.0001, 1.0334] ratio|regression_interval; adverse_point_estimate, regression_interval|
|full/ram/ulmdb/ordered_kv_mixed/throughput (queries/s)|target|7,191,535.425|7,193,590.814|+0.03%|[0.98366, 1.0172] ratio|inconclusive_interval|
|full/ram/ulmdb/reopen/complete_cold_check_ns (ns)|target|58,914,430.563|59,168,117.644|+0.43%|[0.98091, 1.0283] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/reopen/environment_drop_ns (ns)|target|89,555.170|97,986.444|+9.41%|[0.79798, 1.5002] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/reopen/experimental_reenable_ns (ns)|target|2,289.921|2,379.737|+3.92%|[0.79034, 1.3665] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/reopen/open_ns (ns)|target|48,293,389.026|48,431,032.322|+0.29%|[0.97419, 1.0324] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/reopen/validation_ns (ns)|target|6,381,430.227|6,384,620.308|+0.05%|[0.95848, 1.0444] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/setup/create (ns)|target|47,699,985.333|48,118,470.632|+0.88%|[0.91213, 1.1157] ratio|inconclusive_interval; adverse_point_estimate|
