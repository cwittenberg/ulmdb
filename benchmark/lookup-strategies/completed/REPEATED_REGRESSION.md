# Primary and confirmation observations

Both complete 30-case campaigns are retained separately. Changes compare candidate with baseline within each campaign. Delta is confirmation change minus primary change in percentage points. All sampled latency changes and native LMDB control drift remain visible. Neither campaign establishes universal nonregression.

| Filesystem | Case | Primary ULMDB throughput change | Confirmation throughput change | Delta pp | Primary sampled p99 change | Confirmation sampled p99 change | Primary LMDB drift | Confirmation LMDB drift |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| disk | durable_batch1 | +1.82% | -1.07% | -2.89 | -3.42% | +3.63% | +1.43% | -0.54% |
| disk | durable_batch64 | -5.75% | +0.63% | +6.39 | +1.70% | -4.46% | -3.03% | -0.60% |
| disk | ipv4_lpm_hits | +19.41% | -1.06% | -20.47 | -12.87% | +3.14% | +11.43% | -0.23% |
| disk | ipv4_lpm_inside_gap_misses | +18.70% | -0.98% | -19.68 | -11.05% | +8.54% | +13.57% | +2.66% |
| disk | ipv4_lpm_misses | +20.02% | -1.00% | -21.02 | -12.55% | +10.39% | +13.54% | +3.00% |
| disk | ipv4_lpm_mixed | +19.41% | +0.75% | -18.66 | -9.65% | +3.88% | +11.11% | +1.09% |
| disk | ipv4_lpm_outside_cover_misses | +5.79% | +0.73% | -5.06 | -7.40% | -0.88% | +8.39% | +0.40% |
| disk | ipv6_lpm_hits | +0.42% | +0.22% | -0.19 | -0.38% | +1.76% | -1.26% | +0.10% |
| disk | ipv6_lpm_inside_gap_misses | +1.29% | +0.78% | -0.50 | +0.42% | -0.44% | -2.18% | -0.54% |
| disk | ipv6_lpm_misses | +0.02% | +2.21% | +2.19 | +0.40% | -2.28% | -2.35% | +0.99% |
| disk | ipv6_lpm_mixed | +0.15% | +0.96% | +0.82 | -0.59% | +1.07% | -1.34% | +0.52% |
| disk | ipv6_lpm_outside_cover_misses | -0.19% | -1.17% | -0.97 | +2.91% | +0.00% | -3.07% | +0.08% |
| disk | ordered_kv_hits | +6.62% | -0.11% | -6.73 | -6.16% | +1.02% | +8.66% | +1.70% |
| disk | ordered_kv_misses | +10.31% | -0.63% | -10.94 | -9.54% | +1.46% | +10.97% | +0.71% |
| disk | ordered_kv_mixed | +0.00% | -0.52% | -0.52 | -1.38% | +1.36% | +1.01% | +1.43% |
| ram | durable_batch1 | -2.58% | -1.90% | +0.68 | +0.78% | +2.49% | +0.03% | -1.80% |
| ram | durable_batch64 | -0.99% | -1.17% | -0.18 | +1.11% | +1.52% | +0.24% | -2.46% |
| ram | ipv4_lpm_hits | +0.11% | -1.49% | -1.59 | +3.58% | +4.82% | -0.29% | -0.00% |
| ram | ipv4_lpm_inside_gap_misses | -0.38% | -1.34% | -0.96 | +6.91% | +8.87% | +0.82% | +0.46% |
| ram | ipv4_lpm_misses | -1.15% | -0.79% | +0.36 | +9.91% | +8.30% | +2.64% | +1.55% |
| ram | ipv4_lpm_mixed | -0.40% | +0.91% | +1.31 | +7.83% | +3.57% | -2.15% | +0.76% |
| ram | ipv4_lpm_outside_cover_misses | +0.96% | +0.09% | -0.86 | +0.46% | -0.18% | -7.47% | +7.09% |
| ram | ipv6_lpm_hits | +0.25% | -0.73% | -0.98 | +1.00% | +2.46% | +1.32% | -0.65% |
| ram | ipv6_lpm_inside_gap_misses | -0.12% | +1.60% | +1.71 | +1.21% | -0.34% | -0.03% | -0.27% |
| ram | ipv6_lpm_misses | +0.07% | -3.05% | -3.12 | +0.07% | +4.80% | +0.40% | -4.26% |
| ram | ipv6_lpm_mixed | -2.07% | +0.39% | +2.46 | +3.30% | +1.68% | -0.99% | -0.12% |
| ram | ipv6_lpm_outside_cover_misses | -1.63% | -0.33% | +1.30 | +1.79% | +2.47% | -6.54% | +4.81% |
| ram | ordered_kv_hits | -5.65% | -0.71% | +4.94 | +8.43% | +0.97% | -3.17% | +1.12% |
| ram | ordered_kv_misses | -1.74% | -0.38% | +1.36 | +2.86% | +0.05% | +0.44% | +0.86% |
| ram | ordered_kv_mixed | +1.09% | +0.03% | -1.06 | +3.37% | +1.66% | +3.34% | +1.11% |
