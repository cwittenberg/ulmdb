# Benchmark comparison

92 ULMDB metrics; 204 total including controls.

Adverse ULMDB point estimates: 51; adverse intervals: 0; consistent observed increases/decreases: 0; insufficient repeats: 0; unavailable: 0.

descriptive regression screen; no equivalence, universal no-regression, or causal proof; all adverse estimates retained; no outlier exclusion or local-LMDB normalization.

Change is candidate versus baseline. Throughput increases are favorable; cost increases are adverse. Intervals describe process variation under the stated model, not population latency percentiles.

|Metric|Role|Baseline|Candidate|Change|Interval|Status / flags|
|---|---|---:|---:|---:|---|---|
|full/disk/harness/setup/after_setup_ns (ns)|diagnostic|2,199.728|2,290.053|+4.11%|[0.13484, 8.0379] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/harness/setup/dataset_ns (ns)|diagnostic|249,557,676.128|241,279,832.564|-3.32%|[0.73958, 1.2639] ratio|inconclusive_interval|
|full/disk/harness/setup/fixture_load_both_ns (ns)|diagnostic|14,358,307.368|14,326,042.372|-0.22%|[0.97636, 1.0196] ratio|inconclusive_interval|
|full/disk/lmdb/durable_batch1/combined_throughput (records/s)|control|11,099.681|10,936.305|-1.47%|[0.86525, 1.122] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch1/pooled_sampled_p99 (ns)|control|330,185.456|328,324.088|-0.56%|[0.95601, 1.0343] ratio|inconclusive_interval|
|full/disk/lmdb/durable_batch1/sampled_p99 (ns)|control|203,991.969|203,244.606|-0.37%|[0.7772, 1.2773] ratio|inconclusive_interval|
|full/disk/lmdb/durable_batch1/throughput (records/s)|control|10,544.425|10,352.677|-1.82%|[0.83756, 1.1509] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch64/combined_throughput (records/s)|control|181,757.251|176,957.162|-2.64%|[0.86632, 1.0941] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch64/pooled_sampled_p99 (ns)|control|535,535.501|540,636.860|+0.95%|[0.88105, 1.1567] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch64/sampled_p99 (ns)|control|468,059.485|484,935.469|+3.61%|[0.93693, 1.1457] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch64/throughput (records/s)|control|177,541.873|172,181.285|-3.02%|[0.8461, 1.1116] ratio|inconclusive_interval; adverse_point_estimate|
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
|full/disk/lmdb/ipv4_lpm_hits/sampled_p99 (ns)|control|781.127|776.735|-0.56%|[0.94465, 1.0467] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_hits/throughput (queries/s)|control|2,142,252.032|2,167,341.963|+1.17%|[0.95807, 1.0684] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99 (ns)|control|636.050|654.201|+2.85%|[0.72251, 1.4642] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/throughput (queries/s)|control|1,959,121.020|1,935,220.273|-1.22%|[0.74735, 1.3056] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv4_lpm_misses/sampled_p99 (ns)|control|617.082|618.316|+0.20%|[0.88383, 1.136] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv4_lpm_misses/throughput (queries/s)|control|3,834,922.591|3,956,475.873|+3.17%|[0.9407, 1.1315] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_mixed/sampled_p99 (ns)|control|774.015|763.010|-1.42%|[0.95767, 1.0147] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_mixed/throughput (queries/s)|control|2,665,344.891|2,716,336.184|+1.91%|[0.96452, 1.0768] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99 (ns)|control|46.638|45.255|-2.97%|[0.68882, 1.3669] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/throughput (queries/s)|control|91,427,810.676|88,985,691.688|-2.67%|[0.68931, 1.3743] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_hits/sampled_p99 (ns)|control|1,268.994|1,302.038|+2.60%|[0.78823, 1.3356] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_hits/throughput (queries/s)|control|1,232,826.511|1,187,507.024|-3.68%|[0.73704, 1.2589] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99 (ns)|control|1,147.179|1,097.271|-4.35%|[0.86307, 1.06] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/throughput (queries/s)|control|1,098,249.571|1,124,779.872|+2.42%|[0.95322, 1.1004] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_misses/sampled_p99 (ns)|control|1,121.345|1,088.289|-2.95%|[0.79214, 1.1891] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_misses/throughput (queries/s)|control|2,182,329.069|2,192,101.868|+0.45%|[0.8553, 1.1797] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_mixed/sampled_p99 (ns)|control|1,280.925|1,294.782|+1.08%|[0.78016, 1.3097] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_mixed/throughput (queries/s)|control|1,540,215.863|1,493,772.041|-3.02%|[0.73602, 1.278] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99 (ns)|control|46.661|43.301|-7.20%|[0.72088, 1.1946] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/throughput (queries/s)|control|89,627,488.133|90,366,914.494|+0.82%|[0.70697, 1.4379] ratio|inconclusive_interval|
|full/disk/lmdb/ordered_kv_hits/sampled_p99 (ns)|control|336.653|338.829|+0.65%|[0.91146, 1.1114] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ordered_kv_hits/throughput (queries/s)|control|4,180,984.338|4,141,171.368|-0.95%|[0.89993, 1.0901] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ordered_kv_misses/sampled_p99 (ns)|control|341.097|340.276|-0.24%|[0.96171, 1.0348] ratio|inconclusive_interval|
|full/disk/lmdb/ordered_kv_misses/throughput (queries/s)|control|4,016,203.721|4,039,318.866|+0.58%|[0.98897, 1.0228] ratio|inconclusive_interval|
|full/disk/lmdb/ordered_kv_mixed/sampled_p99 (ns)|control|334.831|338.947|+1.23%|[0.9833, 1.0421] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ordered_kv_mixed/throughput (queries/s)|control|4,100,663.516|4,122,809.397|+0.54%|[0.99055, 1.0205] ratio|inconclusive_interval|
|full/disk/lmdb/setup/create (ns)|control|195,984.586|199,564.094|+1.83%|[0.89165, 1.1629] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch1/combined_throughput (records/s)|target|13,214.587|13,312.628|+0.74%|[0.88496, 1.1468] ratio|inconclusive_interval|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99 (ns)|target|217,012.589|323,123.799|+48.90%|[0.52813, 4.1978] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch1/sampled_p99 (ns)|target|95,846.354|97,449.273|+1.67%|[0.72583, 1.4242] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch1/throughput (records/s)|target|12,397.177|12,614.443|+1.75%|[0.8947, 1.1572] ratio|inconclusive_interval|
|full/disk/ulmdb/durable_batch64/combined_throughput (records/s)|target|343,127.252|331,654.957|-3.34%|[0.79419, 1.1763] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99 (ns)|target|701,375.165|720,183.168|+2.68%|[0.71141, 1.4821] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch64/sampled_p99 (ns)|target|322,239.844|345,376.681|+7.18%|[0.82554, 1.3915] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch64/throughput (records/s)|target|342,822.311|330,646.215|-3.55%|[0.7851, 1.1849] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/footprint/after/allocated_file_bytes (bytes)|target|67,110,229.306|67,110,684.414|+0.00%|[0.99996, 1] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/footprint/after/apparent_file_bytes (bytes)|target|67,108,864.000|67,108,864.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/ulmdb/footprint/after/files (files)|target|2.000|2.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/ulmdb/footprint/after/mapped_pss_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/disk/ulmdb/footprint/after/mapped_rss_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/disk/ulmdb/footprint/after/mapped_virtual_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/disk/ulmdb/footprint/before/allocated_file_bytes (bytes)|target|67,110,229.306|67,110,684.414|+0.00%|[0.99996, 1] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/footprint/before/apparent_file_bytes (bytes)|target|67,108,864.000|67,108,864.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/ulmdb/footprint/before/files (files)|target|2.000|2.000|+0.00%|[1, 1] ratio|identical_observed_changes|
|full/disk/ulmdb/footprint/before/mapped_pss_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/disk/ulmdb/footprint/before/mapped_rss_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/disk/ulmdb/footprint/before/mapped_virtual_kib (KiB)|diagnostic|Unavailable|Unavailable|n/a|n/a|unavailable; unavailable|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99 (ns)|target|476.324|470.045|-1.32%|[0.89154, 1.0923] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_hits/throughput (queries/s)|target|2,920,809.860|2,944,755.122|+0.82%|[0.96553, 1.0527] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99 (ns)|target|133.257|139.860|+4.95%|[0.73697, 1.4947] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput (queries/s)|target|10,809,143.355|10,312,988.630|-4.59%|[0.67616, 1.3463] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99 (ns)|target|131.767|132.765|+0.76%|[0.95986, 1.0577] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv4_lpm_misses/throughput (queries/s)|target|20,304,392.693|20,344,182.247|+0.20%|[0.96588, 1.0394] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99 (ns)|target|468.143|463.573|-0.98%|[0.93819, 1.0452] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput (queries/s)|target|5,101,381.924|5,121,862.890|+0.40%|[0.9762, 1.0326] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99 (ns)|target|41.885|42.637|+1.80%|[0.8841, 1.1721] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput (queries/s)|target|99,746,624.949|99,444,215.273|-0.30%|[0.94895, 1.0474] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99 (ns)|target|546.754|570.720|+4.38%|[0.74212, 1.4682] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv6_lpm_hits/throughput (queries/s)|target|2,437,958.877|2,319,984.391|-4.84%|[0.69288, 1.307] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99 (ns)|target|147.587|145.070|-1.71%|[0.85397, 1.1314] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput (queries/s)|target|8,985,043.264|9,174,152.980|+2.10%|[0.95891, 1.0872] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99 (ns)|target|148.315|146.201|-1.43%|[0.78044, 1.2451] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_misses/throughput (queries/s)|target|16,648,329.746|16,600,007.868|-0.29%|[0.80097, 1.2412] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99 (ns)|target|539.671|552.657|+2.41%|[0.74294, 1.4116] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput (queries/s)|target|4,217,774.143|4,046,175.502|-4.07%|[0.69083, 1.3321] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99 (ns)|target|44.301|41.341|-6.68%|[0.71248, 1.2223] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput (queries/s)|target|92,133,097.678|96,523,178.415|+4.76%|[0.82676, 1.3276] ratio|inconclusive_interval|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99 (ns)|target|200.763|208.202|+3.71%|[0.90737, 1.1853] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ordered_kv_hits/throughput (queries/s)|target|7,554,075.837|7,352,416.277|-2.67%|[0.90225, 1.05] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99 (ns)|target|202.051|204.140|+1.03%|[0.96917, 1.0533] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ordered_kv_misses/throughput (queries/s)|target|7,511,534.185|7,429,129.270|-1.10%|[0.9727, 1.0056] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99 (ns)|target|203.129|207.861|+2.33%|[0.99027, 1.0574] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ordered_kv_mixed/throughput (queries/s)|target|7,256,620.905|7,180,932.612|-1.04%|[0.97523, 1.0041] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/reopen/complete_cold_check_ns (ns)|target|100,549,242.839|105,990,764.615|+5.41%|[0.83297, 1.334] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/reopen/environment_drop_ns (ns)|target|96,205.830|100,814.298|+4.79%|[0.81844, 1.3417] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/reopen/experimental_reenable_ns (ns)|target|2,628.358|2,170.033|-17.44%|[0.26977, 2.5268] ratio|inconclusive_interval|
|full/disk/ulmdb/reopen/open_ns (ns)|target|89,935,206.390|94,368,934.932|+4.93%|[0.84633, 1.3009] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/reopen/validation_ns (ns)|target|6,318,239.190|6,955,272.835|+10.08%|[0.7408, 1.6358] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/setup/create (ns)|target|108,063,655.472|106,071,446.773|-1.84%|[0.91878, 1.0486] ratio|inconclusive_interval|
|full/ram/harness/setup/after_setup_ns (ns)|diagnostic|1,091.946|1,686.894|+54.49%|[0.24595, 9.7035] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/harness/setup/dataset_ns (ns)|diagnostic|237,365,745.165|239,606,201.047|+0.94%|[0.97994, 1.0398] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/harness/setup/fixture_load_both_ns (ns)|diagnostic|13,205,818.638|13,127,381.607|-0.59%|[0.97842, 1.01] ratio|inconclusive_interval|
|full/ram/lmdb/durable_batch1/combined_throughput (records/s)|control|136,223.358|135,536.029|-0.50%|[0.97925, 1.0109] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch1/pooled_sampled_p99 (ns)|control|9,888.295|9,814.090|-0.75%|[0.50634, 1.9454] ratio|inconclusive_interval|
|full/ram/lmdb/durable_batch1/sampled_p99 (ns)|control|8,396.841|8,377.894|-0.23%|[0.88567, 1.124] ratio|inconclusive_interval|
|full/ram/lmdb/durable_batch1/throughput (records/s)|control|131,777.692|130,928.015|-0.64%|[0.95509, 1.0336] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch64/combined_throughput (records/s)|control|750,146.293|747,711.613|-0.32%|[0.97757, 1.0163] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch64/pooled_sampled_p99 (ns)|control|98,805.895|98,878.496|+0.07%|[0.92606, 1.0814] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch64/sampled_p99 (ns)|control|92,041.728|92,443.879|+0.44%|[0.9808, 1.0285] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch64/throughput (records/s)|control|742,304.633|738,554.249|-0.51%|[0.96221, 1.0288] ratio|inconclusive_interval; adverse_point_estimate|
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
|full/ram/lmdb/ipv4_lpm_hits/sampled_p99 (ns)|control|785.912|790.867|+0.63%|[0.85595, 1.1831] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv4_lpm_hits/throughput (queries/s)|control|2,129,104.558|2,114,130.241|-0.70%|[0.84161, 1.1716] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99 (ns)|control|620.065|605.693|-2.32%|[0.86944, 1.0975] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/throughput (queries/s)|control|2,021,878.612|2,048,988.639|+1.34%|[0.91174, 1.1264] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_misses/sampled_p99 (ns)|control|615.203|596.779|-2.99%|[0.82329, 1.143] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_misses/throughput (queries/s)|control|3,918,632.920|3,988,832.357|+1.79%|[0.90182, 1.149] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_mixed/sampled_p99 (ns)|control|771.933|762.109|-1.27%|[0.91883, 1.0608] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_mixed/throughput (queries/s)|control|2,666,743.433|2,691,365.029|+0.92%|[0.93378, 1.0908] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99 (ns)|control|44.923|47.989|+6.83%|[0.87495, 1.3043] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/throughput (queries/s)|control|90,146,182.197|95,976,766.219|+6.47%|[0.85981, 1.3184] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_hits/sampled_p99 (ns)|control|1,247.791|1,232.183|-1.25%|[0.93869, 1.0388] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_hits/throughput (queries/s)|control|1,236,846.439|1,242,632.700|+0.47%|[0.95425, 1.0578] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99 (ns)|control|1,088.299|1,071.896|-1.51%|[0.88441, 1.0969] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/throughput (queries/s)|control|1,128,071.857|1,140,829.839|+1.13%|[0.96829, 1.0562] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_misses/sampled_p99 (ns)|control|1,077.180|1,059.484|-1.64%|[0.88307, 1.0955] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_misses/throughput (queries/s)|control|2,219,632.222|2,257,780.297|+1.72%|[0.94917, 1.0901] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_mixed/sampled_p99 (ns)|control|1,227.898|1,217.779|-0.82%|[0.94513, 1.0407] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_mixed/throughput (queries/s)|control|1,558,799.189|1,567,256.651|+0.54%|[0.95264, 1.0611] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99 (ns)|control|43.467|44.430|+2.22%|[0.88833, 1.1761] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/throughput (queries/s)|control|89,013,942.835|95,190,831.402|+6.94%|[0.91628, 1.2481] ratio|inconclusive_interval|
|full/ram/lmdb/ordered_kv_hits/sampled_p99 (ns)|control|334.366|336.198|+0.55%|[0.96084, 1.0522] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ordered_kv_hits/throughput (queries/s)|control|4,201,999.735|4,213,707.840|+0.28%|[0.98314, 1.0228] ratio|inconclusive_interval|
|full/ram/lmdb/ordered_kv_misses/sampled_p99 (ns)|control|344.056|339.633|-1.29%|[0.95553, 1.0198] ratio|inconclusive_interval|
|full/ram/lmdb/ordered_kv_misses/throughput (queries/s)|control|4,006,257.571|4,023,363.843|+0.43%|[0.98002, 1.0291] ratio|inconclusive_interval|
|full/ram/lmdb/ordered_kv_mixed/sampled_p99 (ns)|control|340.885|337.380|-1.03%|[0.93326, 1.0496] ratio|inconclusive_interval|
|full/ram/lmdb/ordered_kv_mixed/throughput (queries/s)|control|4,104,463.328|4,110,359.784|+0.14%|[0.98466, 1.0185] ratio|inconclusive_interval|
|full/ram/lmdb/setup/create (ns)|control|122,792.326|125,090.375|+1.87%|[0.80122, 1.2952] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch1/combined_throughput (records/s)|target|40,653.468|40,519.755|-0.33%|[0.96971, 1.0245] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99 (ns)|target|27,131.070|26,436.815|-2.56%|[0.75371, 1.2597] ratio|inconclusive_interval|
|full/ram/ulmdb/durable_batch1/sampled_p99 (ns)|target|22,379.949|22,233.825|-0.65%|[0.93959, 1.0504] ratio|inconclusive_interval|
|full/ram/ulmdb/durable_batch1/throughput (records/s)|target|34,756.110|34,498.212|-0.74%|[0.94546, 1.042] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch64/combined_throughput (records/s)|target|531,899.033|532,940.186|+0.20%|[0.9813, 1.023] ratio|inconclusive_interval|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99 (ns)|target|537,563.848|539,136.613|+0.29%|[0.96744, 1.0397] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch64/sampled_p99 (ns)|target|198,413.602|196,744.489|-0.84%|[0.9681, 1.0156] ratio|inconclusive_interval|
|full/ram/ulmdb/durable_batch64/throughput (records/s)|target|537,258.534|538,329.836|+0.20%|[0.97374, 1.0311] ratio|inconclusive_interval|
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
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99 (ns)|target|469.474|488.322|+4.01%|[0.85394, 1.267] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_hits/throughput (queries/s)|target|2,935,345.907|2,861,908.612|-2.50%|[0.84577, 1.1239] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99 (ns)|target|131.881|132.048|+0.13%|[0.97381, 1.0295] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput (queries/s)|target|10,929,018.250|10,869,444.496|-0.55%|[0.9624, 1.0278] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99 (ns)|target|133.235|132.910|-0.24%|[0.96269, 1.0337] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv4_lpm_misses/throughput (queries/s)|target|20,242,781.573|20,257,416.054|+0.07%|[0.96355, 1.0393] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99 (ns)|target|465.833|467.076|+0.27%|[0.95064, 1.0575] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput (queries/s)|target|5,119,261.231|5,081,595.383|-0.74%|[0.96438, 1.0217] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99 (ns)|target|41.244|41.244|+0.00%|[0.98006, 1.0203] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput (queries/s)|target|100,516,891.936|99,780,495.506|-0.73%|[0.94769, 1.0398] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99 (ns)|target|542.784|539.382|-0.63%|[0.95976, 1.0289] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_hits/throughput (queries/s)|target|2,432,110.880|2,447,548.218|+0.63%|[0.99023, 1.0227] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99 (ns)|target|142.519|143.903|+0.97%|[0.93547, 1.0899] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput (queries/s)|target|9,215,113.289|9,166,285.520|-0.53%|[0.95769, 1.0331] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99 (ns)|target|142.780|143.133|+0.25%|[0.93277, 1.0774] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_misses/throughput (queries/s)|target|17,056,724.753|17,150,737.992|+0.55%|[0.96995, 1.0424] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99 (ns)|target|526.056|523.487|-0.49%|[0.96611, 1.025] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput (queries/s)|target|4,255,950.360|4,265,007.536|+0.21%|[0.99633, 1.008] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99 (ns)|target|41.615|41.238|-0.91%|[0.96281, 1.0199] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput (queries/s)|target|96,433,547.205|96,591,740.801|+0.16%|[0.94919, 1.057] ratio|inconclusive_interval|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99 (ns)|target|200.920|204.190|+1.63%|[0.96711, 1.0679] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_hits/throughput (queries/s)|target|7,510,402.347|7,461,644.942|-0.65%|[0.96752, 1.0202] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99 (ns)|target|201.984|203.290|+0.65%|[0.97151, 1.0427] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_misses/throughput (queries/s)|target|7,505,411.643|7,446,119.166|-0.79%|[0.97426, 1.0103] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99 (ns)|target|204.852|209.414|+2.23%|[0.97179, 1.0754] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_mixed/throughput (queries/s)|target|7,235,053.200|7,180,930.009|-0.75%|[0.96744, 1.0182] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/reopen/complete_cold_check_ns (ns)|target|59,108,009.240|59,280,601.306|+0.29%|[0.98168, 1.0246] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/reopen/environment_drop_ns (ns)|target|93,387.182|93,323.482|-0.07%|[0.76163, 1.3112] ratio|inconclusive_interval|
|full/ram/ulmdb/reopen/experimental_reenable_ns (ns)|target|2,130.393|2,294.854|+7.72%|[0.76801, 1.5109] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/reopen/open_ns (ns)|target|48,472,248.775|48,655,334.504|+0.38%|[0.98259, 1.0254] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/reopen/validation_ns (ns)|target|6,376,706.033|6,376,215.342|-0.01%|[0.96274, 1.0385] ratio|inconclusive_interval|
|full/ram/ulmdb/setup/create (ns)|target|48,024,020.366|48,264,185.872|+0.50%|[0.93976, 1.0748] ratio|inconclusive_interval; adverse_point_estimate|
