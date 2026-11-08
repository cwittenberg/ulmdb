# Benchmark comparison

92 ULMDB metrics; 204 total including controls.

Adverse ULMDB point estimates: 39; adverse intervals: 0; consistent observed increases/decreases: 1; insufficient repeats: 0; unavailable: 0.

descriptive regression screen; no equivalence, universal no-regression, or causal proof; all adverse estimates retained; no outlier exclusion or local-LMDB normalization.

Change is candidate versus baseline. Throughput increases are favorable; cost increases are adverse. Intervals describe process variation under the stated model, not population latency percentiles.

|Metric|Role|Baseline|Candidate|Change|Interval|Status / flags|
|---|---|---:|---:|---:|---|---|
|full/disk/harness/setup/after_setup_ns (ns)|diagnostic|1,133.592|1,219.336|+7.56%|[0.0020556, 562.85] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/harness/setup/dataset_ns (ns)|diagnostic|242,824,327.000|248,529,090.939|+2.35%|[0.038405, 27.276] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/harness/setup/fixture_load_both_ns (ns)|diagnostic|14,558,216.206|14,515,887.073|-0.29%|[0.33369, 2.9794] ratio|inconclusive_interval|
|full/disk/lmdb/durable_batch1/combined_throughput (records/s)|control|11,033.289|11,280.506|+2.24%|[0.33014, 3.1663] ratio|inconclusive_interval|
|full/disk/lmdb/durable_batch1/pooled_sampled_p99 (ns)|control|327,938.950|328,493.716|+0.17%|[0.48415, 2.0725] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch1/sampled_p99 (ns)|control|195,767.398|189,044.512|-3.43%|[0.0033944, 274.71] ratio|inconclusive_interval|
|full/disk/lmdb/durable_batch1/throughput (records/s)|control|10,407.812|10,556.434|+1.43%|[0.088698, 11.599] ratio|inconclusive_interval|
|full/disk/lmdb/durable_batch64/combined_throughput (records/s)|control|183,860.884|177,542.936|-3.44%|[0.14898, 6.2588] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch64/pooled_sampled_p99 (ns)|control|533,157.918|537,959.690|+0.90%|[0.095954, 10.61] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch64/sampled_p99 (ns)|control|461,370.778|489,057.938|+6.00%|[0.069868, 16.082] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/durable_batch64/throughput (records/s)|control|178,485.446|173,079.242|-3.03%|[0.11001, 8.5477] ratio|inconclusive_interval; adverse_point_estimate|
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
|full/disk/lmdb/ipv4_lpm_hits/sampled_p99 (ns)|control|872.001|785.677|-9.90%|[0.0002459, 3301.4] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_hits/throughput (queries/s)|control|1,883,526.237|2,098,830.614|+11.43%|[0.00015542, 7989.2] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99 (ns)|control|708.834|628.287|-11.36%|[0.00010154, 7737] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/throughput (queries/s)|control|1,749,385.279|1,986,810.349|+13.57%|[4.7859e-05, 26951] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_misses/sampled_p99 (ns)|control|726.752|620.755|-14.58%|[1.2187e-05, 59864] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_misses/throughput (queries/s)|control|3,382,798.274|3,840,976.188|+13.54%|[1.3064e-05, 98686] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_mixed/sampled_p99 (ns)|control|866.421|783.036|-9.62%|[0.00063498, 1286.3] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_mixed/throughput (queries/s)|control|2,351,245.338|2,612,400.804|+11.11%|[5.451e-05, 22647] ratio|inconclusive_interval|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99 (ns)|control|47.090|47.156|+0.14%|[0.44938, 2.2315] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/throughput (queries/s)|control|86,667,668.928|93,938,030.490|+8.39%|[0.0062544, 187.84] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_hits/sampled_p99 (ns)|control|1,272.952|1,290.311|+1.36%|[0.37239, 2.7591] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_hits/throughput (queries/s)|control|1,239,351.951|1,223,738.744|-1.26%|[0.33643, 2.8979] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99 (ns)|control|1,102.317|1,138.384|+3.27%|[0.16675, 6.396] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/throughput (queries/s)|control|1,136,302.769|1,111,527.349|-2.18%|[0.13982, 6.8438] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_misses/sampled_p99 (ns)|control|1,060.200|1,113.874|+5.06%|[0.2054, 5.374] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_misses/throughput (queries/s)|control|2,249,408.752|2,196,552.819|-2.35%|[0.17294, 5.5139] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_mixed/sampled_p99 (ns)|control|1,263.253|1,282.342|+1.51%|[0.2069, 4.9804] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_mixed/throughput (queries/s)|control|1,550,009.994|1,529,283.070|-1.34%|[0.39624, 2.4567] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99 (ns)|control|46.606|44.424|-4.68%|[0.11826, 7.6824] ratio|inconclusive_interval|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/throughput (queries/s)|control|93,956,808.179|91,074,992.505|-3.07%|[0.14675, 6.4027] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/lmdb/ordered_kv_hits/sampled_p99 (ns)|control|367.096|341.398|-7.00%|[0.020451, 42.291] ratio|inconclusive_interval|
|full/disk/lmdb/ordered_kv_hits/throughput (queries/s)|control|3,792,636.463|4,121,035.370|+8.66%|[0.0082976, 142.29] ratio|inconclusive_interval|
|full/disk/lmdb/ordered_kv_misses/sampled_p99 (ns)|control|378.760|345.669|-8.74%|[0.006741, 123.56] ratio|inconclusive_interval|
|full/disk/lmdb/ordered_kv_misses/throughput (queries/s)|control|3,599,291.757|3,994,115.672|+10.97%|[0.0027281, 451.39] ratio|inconclusive_interval|
|full/disk/lmdb/ordered_kv_mixed/sampled_p99 (ns)|control|351.230|341.604|-2.74%|[0.33303, 2.8404] ratio|inconclusive_interval|
|full/disk/lmdb/ordered_kv_mixed/throughput (queries/s)|control|4,020,789.089|4,061,428.380|+1.01%|[0.49074, 2.0791] ratio|inconclusive_interval|
|full/disk/lmdb/setup/create (ns)|control|200,471.604|231,820.256|+15.64%|[0.00054766, 2441.7] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch1/combined_throughput (records/s)|target|13,323.681|13,515.386|+1.44%|[0.77207, 1.3328] ratio|inconclusive_interval|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99 (ns)|target|320,746.271|246,450.423|-23.16%|[2.5652e-08, 2.3015e+07] ratio|inconclusive_interval|
|full/disk/ulmdb/durable_batch1/sampled_p99 (ns)|target|98,064.602|94,713.619|-3.42%|[0.064694, 14.419] ratio|inconclusive_interval|
|full/disk/ulmdb/durable_batch1/throughput (records/s)|target|12,558.218|12,786.840|+1.82%|[0.56027, 1.8504] ratio|inconclusive_interval|
|full/disk/ulmdb/durable_batch64/combined_throughput (records/s)|target|345,382.976|330,914.872|-4.19%|[0.12763, 7.1922] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99 (ns)|target|671,077.378|698,058.296|+4.02%|[0.040449, 26.751] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch64/sampled_p99 (ns)|target|314,487.048|319,846.156|+1.70%|[0.014133, 73.188] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/durable_batch64/throughput (records/s)|target|345,943.822|326,036.754|-5.75%|[0.15371, 5.7785] ratio|inconclusive_interval; adverse_point_estimate|
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
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99 (ns)|target|535.960|466.963|-12.87%|[3.6241e-05, 20946] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_hits/throughput (queries/s)|target|2,527,559.384|3,018,231.989|+19.41%|[2.4872e-05, 57331] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99 (ns)|target|149.736|133.192|-11.05%|[3.7746e-05, 20962] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput (queries/s)|target|9,123,339.551|10,829,033.286|+18.70%|[4.2618e-05, 33058] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99 (ns)|target|152.288|133.174|-12.55%|[3.9367e-06, 1.9426e+05] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_misses/throughput (queries/s)|target|16,999,745.968|20,403,710.912|+20.02%|[7.0164e-06, 2.0531e+05] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99 (ns)|target|521.328|471.023|-9.65%|[5.4074e-05, 15096] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput (queries/s)|target|4,347,067.339|5,191,014.393|+19.41%|[2.3998e-05, 59421] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99 (ns)|target|44.460|41.170|-7.40%|[0.010084, 85.03] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput (queries/s)|target|94,047,938.061|99,488,832.976|+5.79%|[0.0065925, 169.75] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99 (ns)|target|543.530|541.453|-0.38%|[0.59303, 1.6734] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_hits/throughput (queries/s)|target|2,502,486.193|2,512,934.992|+0.42%|[0.88484, 1.1396] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99 (ns)|target|143.079|143.675|+0.42%|[0.24188, 4.1687] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput (queries/s)|target|9,082,664.652|9,199,397.491|+1.29%|[0.59181, 1.7334] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99 (ns)|target|142.693|143.267|+0.40%|[0.51134, 1.9714] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv6_lpm_misses/throughput (queries/s)|target|17,117,632.561|17,121,099.024|+0.02%|[0.69105, 1.4477] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99 (ns)|target|534.740|531.582|-0.59%|[0.13601, 7.2658] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput (queries/s)|target|4,296,231.873|4,302,562.629|+0.15%|[0.2426, 4.1341] ratio|inconclusive_interval|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99 (ns)|target|41.187|42.385|+2.91%|[0.37847, 2.7982] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput (queries/s)|target|96,566,328.903|96,378,479.547|-0.19%|[0.68127, 1.4621] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99 (ns)|target|221.877|208.213|-6.16%|[0.021251, 41.44] ratio|inconclusive_interval|
|full/disk/ulmdb/ordered_kv_hits/throughput (queries/s)|target|6,961,199.460|7,421,885.595|+6.62%|[0.011532, 98.57] ratio|inconclusive_interval|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99 (ns)|target|226.315|204.731|-9.54%|[0.0062487, 130.96] ratio|inconclusive_interval|
|full/disk/ulmdb/ordered_kv_misses/throughput (queries/s)|target|6,703,025.637|7,394,237.164|+10.31%|[0.0015314, 794.61] ratio|inconclusive_interval|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99 (ns)|target|213.252|210.313|-1.38%|[0.18786, 5.1775] ratio|inconclusive_interval|
|full/disk/ulmdb/ordered_kv_mixed/throughput (queries/s)|target|7,150,988.891|7,151,035.226|+0.00%|[0.53096, 1.8834] ratio|inconclusive_interval|
|full/disk/ulmdb/reopen/complete_cold_check_ns (ns)|target|100,836,102.361|98,083,236.596|-2.73%|[0.40672, 2.3263] ratio|inconclusive_interval|
|full/disk/ulmdb/reopen/environment_drop_ns (ns)|target|88,525.318|87,458.637|-1.20%|[0.12588, 7.7539] ratio|inconclusive_interval|
|full/disk/ulmdb/reopen/experimental_reenable_ns (ns)|target|2,185.205|2,231.238|+2.11%|[0.045464, 22.932] ratio|inconclusive_interval; adverse_point_estimate|
|full/disk/ulmdb/reopen/open_ns (ns)|target|90,257,814.228|87,537,114.018|-3.01%|[0.36633, 2.5677] ratio|inconclusive_interval|
|full/disk/ulmdb/reopen/validation_ns (ns)|target|6,383,741.958|6,302,930.946|-1.27%|[0.90373, 1.0787] ratio|inconclusive_interval|
|full/disk/ulmdb/setup/create (ns)|target|102,734,761.585|101,686,104.104|-1.02%|[0.90618, 1.0811] ratio|inconclusive_interval|
|full/ram/harness/setup/after_setup_ns (ns)|diagnostic|1,097.469|2,130.394|+94.12%|[1.9042e-18, 1.9789e+18] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/harness/setup/dataset_ns (ns)|diagnostic|239,067,214.295|237,564,728.002|-0.63%|[0.35911, 2.7497] ratio|inconclusive_interval|
|full/ram/harness/setup/fixture_load_both_ns (ns)|diagnostic|13,122,408.446|13,541,785.944|+3.20%|[0.1695, 6.2829] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch1/combined_throughput (records/s)|control|135,889.197|136,212.236|+0.24%|[0.45745, 2.1965] ratio|inconclusive_interval|
|full/ram/lmdb/durable_batch1/pooled_sampled_p99 (ns)|control|9,871.925|10,251.086|+3.84%|[4.4267e-06, 2.4359e+05] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch1/sampled_p99 (ns)|control|8,442.044|8,433.159|-0.11%|[0.11986, 8.3256] ratio|inconclusive_interval|
|full/ram/lmdb/durable_batch1/throughput (records/s)|control|131,492.856|131,529.981|+0.03%|[0.38042, 2.6302] ratio|inconclusive_interval|
|full/ram/lmdb/durable_batch64/combined_throughput (records/s)|control|753,083.945|755,120.032|+0.27%|[0.46849, 2.1461] ratio|inconclusive_interval|
|full/ram/lmdb/durable_batch64/pooled_sampled_p99 (ns)|control|95,200.622|100,087.062|+5.13%|[0.10515, 10.512] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch64/sampled_p99 (ns)|control|91,454.878|91,613.073|+0.17%|[0.33079, 3.0336] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/durable_batch64/throughput (records/s)|control|745,726.326|747,528.501|+0.24%|[0.49057, 2.0483] ratio|inconclusive_interval|
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
|full/ram/lmdb/ipv4_lpm_hits/sampled_p99 (ns)|control|772.871|771.976|-0.12%|[0.56806, 1.7563] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_hits/throughput (queries/s)|control|2,152,082.373|2,145,789.966|-0.29%|[0.79528, 1.2501] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99 (ns)|control|630.043|628.164|-0.30%|[0.22739, 4.3716] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/throughput (queries/s)|control|1,968,645.744|1,984,721.165|+0.82%|[0.47199, 2.1534] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_misses/sampled_p99 (ns)|control|623.355|611.256|-1.94%|[0.42763, 2.2486] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_misses/throughput (queries/s)|control|3,764,285.615|3,863,809.017|+2.64%|[0.31744, 3.319] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_mixed/sampled_p99 (ns)|control|758.465|777.805|+2.55%|[0.31299, 3.36] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv4_lpm_mixed/throughput (queries/s)|control|2,672,446.752|2,615,090.050|-2.15%|[0.50042, 1.9135] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99 (ns)|control|43.985|41.187|-6.36%|[0.01316, 66.627] ratio|inconclusive_interval|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/throughput (queries/s)|control|87,920,065.225|81,352,738.546|-7.47%|[0.0092487, 92.573] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_hits/sampled_p99 (ns)|control|1,261.598|1,273.579|+0.95%|[0.86762, 1.1746] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_hits/throughput (queries/s)|control|1,225,670.578|1,241,859.230|+1.32%|[0.84445, 1.2157] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99 (ns)|control|1,085.753|1,129.648|+4.04%|[0.64532, 1.6774] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/throughput (queries/s)|control|1,129,742.024|1,129,357.133|-0.03%|[0.8116, 1.2313] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_misses/sampled_p99 (ns)|control|1,075.603|1,093.403|+1.65%|[0.68832, 1.5013] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_misses/throughput (queries/s)|control|2,215,480.066|2,224,428.830|+0.40%|[0.69965, 1.4409] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_mixed/sampled_p99 (ns)|control|1,250.530|1,257.302|+0.54%|[0.69448, 1.4556] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_mixed/throughput (queries/s)|control|1,540,300.100|1,525,040.662|-0.99%|[0.35255, 2.7806] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99 (ns)|control|43.804|41.187|-5.97%|[0.011813, 74.842] ratio|inconclusive_interval|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/throughput (queries/s)|control|88,133,633.950|82,370,045.887|-6.54%|[0.047521, 18.381] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ordered_kv_hits/sampled_p99 (ns)|control|336.557|364.307|+8.25%|[0.12918, 9.0705] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ordered_kv_hits/throughput (queries/s)|control|4,116,071.077|3,985,750.453|-3.17%|[0.18001, 5.2089] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ordered_kv_misses/sampled_p99 (ns)|control|340.163|347.378|+2.12%|[0.2034, 5.1271] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/lmdb/ordered_kv_misses/throughput (queries/s)|control|3,977,473.025|3,994,959.684|+0.44%|[0.3844, 2.6244] ratio|inconclusive_interval|
|full/ram/lmdb/ordered_kv_mixed/sampled_p99 (ns)|control|355.653|355.267|-0.11%|[0.0061096, 163.32] ratio|inconclusive_interval|
|full/ram/lmdb/ordered_kv_mixed/throughput (queries/s)|control|3,902,861.019|4,033,238.945|+3.34%|[0.088384, 12.083] ratio|inconclusive_interval|
|full/ram/lmdb/setup/create (ns)|control|120,518.150|126,275.966|+4.78%|[0.19545, 5.6171] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch1/combined_throughput (records/s)|target|40,803.767|40,152.200|-1.60%|[0.57509, 1.6838] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99 (ns)|target|25,810.768|26,748.550|+3.63%|[0.25917, 4.1439] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch1/sampled_p99 (ns)|target|21,819.572|21,990.048|+0.78%|[0.79724, 1.274] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch1/throughput (records/s)|target|34,747.092|33,849.631|-2.58%|[0.40655, 2.3343] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch64/combined_throughput (records/s)|target|535,408.682|530,581.134|-0.90%|[0.84334, 1.1645] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99 (ns)|target|528,481.510|538,226.083|+1.84%|[0.30371, 3.4152] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch64/sampled_p99 (ns)|target|195,209.607|197,384.583|+1.11%|[0.60756, 1.6828] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/durable_batch64/throughput (records/s)|target|541,443.602|536,059.491|-0.99%|[0.8121, 1.207] ratio|inconclusive_interval; adverse_point_estimate|
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
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99 (ns)|target|450.049|466.138|+3.58%|[0.77568, 1.383] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_hits/throughput (queries/s)|target|3,022,314.634|3,025,593.735|+0.11%|[0.78442, 1.2776] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99 (ns)|target|123.060|131.564|+6.91%|[0.95564, 1.196] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput (queries/s)|target|10,956,312.655|10,914,324.299|-0.38%|[0.93334, 1.0632] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99 (ns)|target|122.088|134.188|+9.91%|[0.78073, 1.5473] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_misses/throughput (queries/s)|target|20,576,367.517|20,340,148.646|-1.15%|[0.75811, 1.289] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99 (ns)|target|439.864|474.297|+7.83%|[0.18743, 6.2035] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput (queries/s)|target|5,161,932.587|5,141,230.188|-0.40%|[0.26888, 3.6894] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99 (ns)|target|41.000|41.187|+0.46%|[0.75139, 1.343] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput (queries/s)|target|99,228,926.371|100,177,231.735|+0.96%|[0.24131, 4.2236] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99 (ns)|target|530.465|535.760|+1.00%|[0.7744, 1.3172] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_hits/throughput (queries/s)|target|2,511,562.720|2,517,745.995|+0.25%|[0.90473, 1.1108] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99 (ns)|target|141.000|142.710|+1.21%|[1.0121, 1.0121] ratio|identical_observed_changes; adverse_point_estimate, consistent_observed_regression|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput (queries/s)|target|9,158,483.512|9,147,898.144|-0.12%|[0.52088, 1.9154] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99 (ns)|target|142.045|142.138|+0.07%|[0.95988, 1.0432] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_misses/throughput (queries/s)|target|17,144,918.110|17,156,336.343|+0.07%|[0.89129, 1.1235] ratio|inconclusive_interval|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99 (ns)|target|514.858|531.859|+3.30%|[0.32119, 3.3224] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput (queries/s)|target|4,371,609.972|4,281,313.918|-2.07%|[0.28101, 3.4131] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99 (ns)|target|41.170|41.908|+1.79%|[0.48401, 2.1409] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput (queries/s)|target|97,565,542.471|95,973,837.794|-1.63%|[0.34487, 2.8058] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99 (ns)|target|202.053|219.085|+8.43%|[0.09412, 12.491] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_hits/throughput (queries/s)|target|7,543,782.977|7,117,482.537|-5.65%|[0.1224, 7.2724] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99 (ns)|target|201.883|207.651|+2.86%|[0.28462, 3.7171] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_misses/throughput (queries/s)|target|7,470,205.260|7,340,505.073|-1.74%|[0.59578, 1.6207] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99 (ns)|target|213.584|220.775|+3.37%|[0.015113, 70.701] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/ordered_kv_mixed/throughput (queries/s)|target|7,051,462.416|7,128,060.589|+1.09%|[0.18276, 5.5913] ratio|inconclusive_interval|
|full/ram/ulmdb/reopen/complete_cold_check_ns (ns)|target|58,677,169.792|58,463,883.675|-0.36%|[0.68276, 1.454] ratio|inconclusive_interval|
|full/ram/ulmdb/reopen/environment_drop_ns (ns)|target|88,501.638|88,294.336|-0.23%|[0.51827, 1.9205] ratio|inconclusive_interval|
|full/ram/ulmdb/reopen/experimental_reenable_ns (ns)|target|2,529.728|2,355.468|-6.89%|[0.00024118, 3594.7] ratio|inconclusive_interval|
|full/ram/ulmdb/reopen/open_ns (ns)|target|48,117,615.998|47,770,809.152|-0.72%|[0.63889, 1.5427] ratio|inconclusive_interval|
|full/ram/ulmdb/reopen/validation_ns (ns)|target|6,357,842.428|6,384,098.504|+0.41%|[0.49594, 2.0331] ratio|inconclusive_interval; adverse_point_estimate|
|full/ram/ulmdb/setup/create (ns)|target|46,981,279.085|46,296,042.839|-1.46%|[0.33363, 2.9106] ratio|inconclusive_interval|
