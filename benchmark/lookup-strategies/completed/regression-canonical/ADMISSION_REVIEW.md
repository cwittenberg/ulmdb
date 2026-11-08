# Canonical regression admission review

The 30 throughput centers pass the observed 5% adverse screen, with changes from -4.839% to +4.765%; the strict all-metric gate remains **not clear**.

Nine fresh paired rounds per filesystem produced 36 validated processes and retained all 92 ULMDB target metrics plus native LMDB controls. There are 51 adverse target point estimates and five target centers worse by more than 5%, with no adjusted regression interval in this canonical campaign. Neither an inconclusive interval nor absence of a detected regression establishes 5% noninferiority or universal nonregression.

## Target centers worse by more than 5%

| Exact metric path | Baseline | Candidate | Change | Adjusted candidate/baseline interval | Status |
|---|---:|---:|---:|---|---|
| `full/disk/ulmdb/durable_batch1/pooled_sampled_p99` (ns) | 217,012.589 | 323,123.799 | +48.896% | 0.52813–4.19781 | inconclusive_interval |
| `full/disk/ulmdb/durable_batch64/sampled_p99` (ns) | 322,239.844 | 345,376.681 | +7.180% | 0.82554–1.39152 | inconclusive_interval |
| `full/disk/ulmdb/reopen/complete_cold_check_ns` (ns) | 100,549,242.839 | 105,990,764.615 | +5.412% | 0.83297–1.33398 | inconclusive_interval |
| `full/disk/ulmdb/reopen/validation_ns` (ns) | 6,318,239.190 | 6,955,272.835 | +10.082% | 0.74080–1.63582 | inconclusive_interval |
| `full/ram/ulmdb/reopen/experimental_reenable_ns` (ns) | 2,130.393 | 2,294.854 | +7.720% | 0.76801–1.51087 | inconclusive_interval |

`reopen/complete_cold_check_ns` includes snapshot capture, environment close/open, experimental re-enablement and old/new reader validation. `reopen/validation_ns` is its validation phase; `reopen/experimental_reenable_ns` is the explicit experimental configuration phase. These fields describe a warm-cache reopen journey: OS caches were not evicted and true cold-storage behavior was not measured.

## Earlier prefix tails and canonical control

| Filesystem | Case | Path-separated confirmation p99 change | Canonical p99 change | Canonical adjusted interval |
|---|---|---:|---:|---|
| disk | ipv4_lpm_inside_gap_misses | +8.538% | +4.955% | 0.73697–1.49471 |
| disk | ipv4_lpm_misses | +10.387% | +0.758% | 0.95986–1.05766 |
| ram | ipv4_lpm_inside_gap_misses | +8.866% | +0.127% | 0.97381–1.02950 |
| ram | ipv4_lpm_misses | +8.298% | -0.244% | 0.96269–1.03370 |

The earlier 8–10% IPv4 miss-tail pattern was not reproduced under the same canonical source/target paths. Static review found identical core/prefix fingerprints and matching normalized instructions with matched function order/alignment; `.text` bytes still differ, so timing equivalence is not assumed and prior adverse observations remain retained.

## Scope and admission

The complete default-engine matrix measures ordinary reads/writes with the optional accelerator unused. It does not prove unchanged costs while a live side index retains a snapshot, rebuilds, competes for memory/cache, or serves concurrent application work. Resident anonymous mapping RSS is not established by pathname-attributed mapping observations. Native LMDB controls are retained without normalization, and no bad case or lifecycle phase was discarded. The correct admission conclusion is an operational explicit capability with unresolved broad nonregression qualification, not an automatic/default performance promotion.

## All 92 ULMDB target metric statuses

Change is candidate versus baseline; throughput decreases are adverse and time/space increases are adverse. The interval-bound column is a descriptive sufficient-bound check under the comparison model, not a universal noninferiority claim.

| Exact metric path | Change | Adverse point estimate | Worse than 5% center | Adjusted ratio interval | Excludes >5% adverse change | Status |
|---|---:|---|---|---|---|---|
| `full/disk/ulmdb/durable_batch1/combined_throughput` | +0.742% | False | False | 0.88496–1.1468 | False | inconclusive_interval |
| `full/disk/ulmdb/durable_batch1/pooled_sampled_p99` | +48.896% | True | True | 0.52813–4.1978 | False | inconclusive_interval |
| `full/disk/ulmdb/durable_batch1/sampled_p99` | +1.672% | True | False | 0.72583–1.4242 | False | inconclusive_interval |
| `full/disk/ulmdb/durable_batch1/throughput` | +1.753% | False | False | 0.8947–1.1572 | False | inconclusive_interval |
| `full/disk/ulmdb/durable_batch64/combined_throughput` | -3.343% | True | False | 0.79419–1.1763 | False | inconclusive_interval |
| `full/disk/ulmdb/durable_batch64/pooled_sampled_p99` | +2.682% | True | False | 0.71141–1.4821 | False | inconclusive_interval |
| `full/disk/ulmdb/durable_batch64/sampled_p99` | +7.180% | True | True | 0.82554–1.3915 | False | inconclusive_interval |
| `full/disk/ulmdb/durable_batch64/throughput` | -3.552% | True | False | 0.7851–1.1849 | False | inconclusive_interval |
| `full/disk/ulmdb/footprint/after/allocated_file_bytes` | +0.001% | True | False | 0.99996–1 | True | inconclusive_interval |
| `full/disk/ulmdb/footprint/after/apparent_file_bytes` | +0.000% | False | False | 1–1 | True | identical_observed_changes |
| `full/disk/ulmdb/footprint/after/files` | +0.000% | False | False | 1–1 | True | identical_observed_changes |
| `full/disk/ulmdb/footprint/before/allocated_file_bytes` | +0.001% | True | False | 0.99996–1 | True | inconclusive_interval |
| `full/disk/ulmdb/footprint/before/apparent_file_bytes` | +0.000% | False | False | 1–1 | True | identical_observed_changes |
| `full/disk/ulmdb/footprint/before/files` | +0.000% | False | False | 1–1 | True | identical_observed_changes |
| `full/disk/ulmdb/ipv4_lpm_hits/sampled_p99` | -1.318% | False | False | 0.89154–1.0923 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv4_lpm_hits/throughput` | +0.820% | False | False | 0.96553–1.0527 | True | inconclusive_interval |
| `full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99` | +4.955% | True | False | 0.73697–1.4947 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput` | -4.590% | True | False | 0.67616–1.3463 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv4_lpm_misses/sampled_p99` | +0.758% | True | False | 0.95986–1.0577 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv4_lpm_misses/throughput` | +0.196% | False | False | 0.96588–1.0394 | True | inconclusive_interval |
| `full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99` | -0.976% | False | False | 0.93819–1.0452 | True | inconclusive_interval |
| `full/disk/ulmdb/ipv4_lpm_mixed/throughput` | +0.401% | False | False | 0.9762–1.0326 | True | inconclusive_interval |
| `full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99` | +1.796% | True | False | 0.8841–1.1721 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput` | -0.303% | True | False | 0.94895–1.0474 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv6_lpm_hits/sampled_p99` | +4.383% | True | False | 0.74212–1.4682 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv6_lpm_hits/throughput` | -4.839% | True | False | 0.69288–1.307 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99` | -1.705% | False | False | 0.85397–1.1314 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput` | +2.105% | False | False | 0.95891–1.0872 | True | inconclusive_interval |
| `full/disk/ulmdb/ipv6_lpm_misses/sampled_p99` | -1.425% | False | False | 0.78044–1.2451 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv6_lpm_misses/throughput` | -0.290% | True | False | 0.80097–1.2412 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99` | +2.406% | True | False | 0.74294–1.4116 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv6_lpm_mixed/throughput` | -4.068% | True | False | 0.69083–1.3321 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99` | -6.680% | False | False | 0.71248–1.2223 | False | inconclusive_interval |
| `full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput` | +4.765% | False | False | 0.82676–1.3276 | False | inconclusive_interval |
| `full/disk/ulmdb/ordered_kv_hits/sampled_p99` | +3.705% | True | False | 0.90737–1.1853 | False | inconclusive_interval |
| `full/disk/ulmdb/ordered_kv_hits/throughput` | -2.670% | True | False | 0.90225–1.05 | False | inconclusive_interval |
| `full/disk/ulmdb/ordered_kv_misses/sampled_p99` | +1.034% | True | False | 0.96917–1.0533 | False | inconclusive_interval |
| `full/disk/ulmdb/ordered_kv_misses/throughput` | -1.097% | True | False | 0.9727–1.0056 | True | inconclusive_interval |
| `full/disk/ulmdb/ordered_kv_mixed/sampled_p99` | +2.330% | True | False | 0.99027–1.0574 | False | inconclusive_interval |
| `full/disk/ulmdb/ordered_kv_mixed/throughput` | -1.043% | True | False | 0.97523–1.0041 | True | inconclusive_interval |
| `full/disk/ulmdb/reopen/complete_cold_check_ns` | +5.412% | True | True | 0.83297–1.334 | False | inconclusive_interval |
| `full/disk/ulmdb/reopen/environment_drop_ns` | +4.790% | True | False | 0.81844–1.3417 | False | inconclusive_interval |
| `full/disk/ulmdb/reopen/experimental_reenable_ns` | -17.438% | False | False | 0.26977–2.5268 | False | inconclusive_interval |
| `full/disk/ulmdb/reopen/open_ns` | +4.930% | True | False | 0.84633–1.3009 | False | inconclusive_interval |
| `full/disk/ulmdb/reopen/validation_ns` | +10.082% | True | True | 0.7408–1.6358 | False | inconclusive_interval |
| `full/disk/ulmdb/setup/create` | -1.844% | False | False | 0.91878–1.0486 | True | inconclusive_interval |
| `full/ram/ulmdb/durable_batch1/combined_throughput` | -0.329% | True | False | 0.96971–1.0245 | True | inconclusive_interval |
| `full/ram/ulmdb/durable_batch1/pooled_sampled_p99` | -2.559% | False | False | 0.75371–1.2597 | False | inconclusive_interval |
| `full/ram/ulmdb/durable_batch1/sampled_p99` | -0.653% | False | False | 0.93959–1.0504 | False | inconclusive_interval |
| `full/ram/ulmdb/durable_batch1/throughput` | -0.742% | True | False | 0.94546–1.042 | False | inconclusive_interval |
| `full/ram/ulmdb/durable_batch64/combined_throughput` | +0.196% | False | False | 0.9813–1.023 | True | inconclusive_interval |
| `full/ram/ulmdb/durable_batch64/pooled_sampled_p99` | +0.293% | True | False | 0.96744–1.0397 | True | inconclusive_interval |
| `full/ram/ulmdb/durable_batch64/sampled_p99` | -0.841% | False | False | 0.9681–1.0156 | True | inconclusive_interval |
| `full/ram/ulmdb/durable_batch64/throughput` | +0.199% | False | False | 0.97374–1.0311 | True | inconclusive_interval |
| `full/ram/ulmdb/footprint/after/allocated_file_bytes` | +0.000% | False | False | 1–1 | True | identical_observed_changes |
| `full/ram/ulmdb/footprint/after/apparent_file_bytes` | +0.000% | False | False | 1–1 | True | identical_observed_changes |
| `full/ram/ulmdb/footprint/after/files` | +0.000% | False | False | 1–1 | True | identical_observed_changes |
| `full/ram/ulmdb/footprint/before/allocated_file_bytes` | +0.000% | False | False | 1–1 | True | identical_observed_changes |
| `full/ram/ulmdb/footprint/before/apparent_file_bytes` | +0.000% | False | False | 1–1 | True | identical_observed_changes |
| `full/ram/ulmdb/footprint/before/files` | +0.000% | False | False | 1–1 | True | identical_observed_changes |
| `full/ram/ulmdb/ipv4_lpm_hits/sampled_p99` | +4.015% | True | False | 0.85394–1.267 | False | inconclusive_interval |
| `full/ram/ulmdb/ipv4_lpm_hits/throughput` | -2.502% | True | False | 0.84577–1.1239 | False | inconclusive_interval |
| `full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99` | +0.127% | True | False | 0.97381–1.0295 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput` | -0.545% | True | False | 0.9624–1.0278 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv4_lpm_misses/sampled_p99` | -0.244% | False | False | 0.96269–1.0337 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv4_lpm_misses/throughput` | +0.072% | False | False | 0.96355–1.0393 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99` | +0.267% | True | False | 0.95064–1.0575 | False | inconclusive_interval |
| `full/ram/ulmdb/ipv4_lpm_mixed/throughput` | -0.736% | True | False | 0.96438–1.0217 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99` | +0.000% | False | False | 0.98006–1.0203 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput` | -0.733% | True | False | 0.94769–1.0398 | False | inconclusive_interval |
| `full/ram/ulmdb/ipv6_lpm_hits/sampled_p99` | -0.627% | False | False | 0.95976–1.0289 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv6_lpm_hits/throughput` | +0.635% | False | False | 0.99023–1.0227 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99` | +0.971% | True | False | 0.93547–1.0899 | False | inconclusive_interval |
| `full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput` | -0.530% | True | False | 0.95769–1.0331 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv6_lpm_misses/sampled_p99` | +0.247% | True | False | 0.93277–1.0774 | False | inconclusive_interval |
| `full/ram/ulmdb/ipv6_lpm_misses/throughput` | +0.551% | False | False | 0.96995–1.0424 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99` | -0.488% | False | False | 0.96611–1.025 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv6_lpm_mixed/throughput` | +0.213% | False | False | 0.99633–1.008 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99` | -0.905% | False | False | 0.96281–1.0199 | True | inconclusive_interval |
| `full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput` | +0.164% | False | False | 0.94919–1.057 | False | inconclusive_interval |
| `full/ram/ulmdb/ordered_kv_hits/sampled_p99` | +1.627% | True | False | 0.96711–1.0679 | False | inconclusive_interval |
| `full/ram/ulmdb/ordered_kv_hits/throughput` | -0.649% | True | False | 0.96752–1.0202 | True | inconclusive_interval |
| `full/ram/ulmdb/ordered_kv_misses/sampled_p99` | +0.647% | True | False | 0.97151–1.0427 | True | inconclusive_interval |
| `full/ram/ulmdb/ordered_kv_misses/throughput` | -0.790% | True | False | 0.97426–1.0103 | True | inconclusive_interval |
| `full/ram/ulmdb/ordered_kv_mixed/sampled_p99` | +2.227% | True | False | 0.97179–1.0754 | False | inconclusive_interval |
| `full/ram/ulmdb/ordered_kv_mixed/throughput` | -0.748% | True | False | 0.96744–1.0182 | True | inconclusive_interval |
| `full/ram/ulmdb/reopen/complete_cold_check_ns` | +0.292% | True | False | 0.98168–1.0246 | True | inconclusive_interval |
| `full/ram/ulmdb/reopen/environment_drop_ns` | -0.068% | False | False | 0.76163–1.3112 | False | inconclusive_interval |
| `full/ram/ulmdb/reopen/experimental_reenable_ns` | +7.720% | True | True | 0.76801–1.5109 | False | inconclusive_interval |
| `full/ram/ulmdb/reopen/open_ns` | +0.378% | True | False | 0.98259–1.0254 | True | inconclusive_interval |
| `full/ram/ulmdb/reopen/validation_ns` | -0.008% | False | False | 0.96274–1.0385 | True | inconclusive_interval |
| `full/ram/ulmdb/setup/create` | +0.500% | True | False | 0.93976–1.0748 | False | inconclusive_interval |
