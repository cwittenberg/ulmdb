# Snapshot-log benchmark — 8 November 2026

The target of 10× LMDB performance and the strict no-regression target were not met.
All 130 declared native processes completed successfully:100 canonical processes (five matched quartets for RAM8/8 replacement+reads, RAM16/64 insertion and original disk single/random/reads),15 separately labeled entropy processes (five matched triplets), and 15 public-default Legacy processes (five per original disk case, separately scheduled and unpaired). No failed/replacement/excluded engine sample. All complete key/value/order/reopen oracles, source/input/dispatch guards and post-run frozen hashes passed.

The latest column measures the real opt-in `ulmdb::snapshot_log` versioned backend, with actual production sources and complete public transaction calls. Public `ulmdb::Environment::create/open` still selects Legacy; its separate fresh cohort is shown explicitly. These are different APIs/formats and this report does not promote the opt-in backend or claim full LMDB parity.

Development labels identify frozen implementations, not release versions: Accepted32
is the earlier opt-in baseline, Previous06 is the retained compact-journal backend,
and Latest11 is the packed-journal backend. All three were measured again in this
campaign; only the separate Legacy column uses the public default.

## Original disk table

All displayed values are marginal medians of five process aggregate-time/operation measurements. Every matched ratio is the median of the five per-process ratios, NOT the ratio of displayed marginal medians. This also applies to paired tail ratios: equal rounded marginal percentile medians can coexist with a non-unit matched ratio. Public-default values are unpaired marginal medians; no paired Legacy/control ratio is claimed.

|Cell|Unit|Public default Legacy (unpaired)|Accepted32 opt-in|Previous06 opt-in|Latest11 opt-in|LMDB0.9.36|Latest/06 matched latency|Latest/LMDB matched latency|
|---|---|---:|---:|---:|---:|---:|---:|---:|
|Durable single|µs/txn|4,476.040|36.289|30.190|25.767|63.821|0.8610x|0.4047x|
|Random batch1000|µs/row|16.386|2.835|2.857|2.790|5.492|0.9661x|0.5087x|
|Held hit|ns/query|504.806|572.900|231.174|230.342|356.039|0.9991x|0.6519x|
|Held miss|ns/query|470.035|311.962|118.257|116.987|331.261|0.9957x|0.3570x|
|Begin/get/end|ns/query|571.238|625.810|271.833|273.256|459.656|1.0030x|0.5954x|
|Ordered scan|ns/row|20.057|27.296|17.639|17.674|14.956|0.9946x|1.1873x|

## RAM workloads

Both engines use normal synchronous calls on tmpfs. These numbers do not establish physical-device or reboot durability. RAM control is its original native LMDB 1.0.2; disk control is native LMDB 0.9.36 flags0. No public-default RAM cohort was added. `ram8` means key8/value8; `insert16` means key16/value64.

|Case/cell|Unit|Accepted32 opt-in|Previous06 opt-in|Latest11 opt-in|LMDB1.0.2|Latest/06 matched latency|Latest/LMDB matched latency|
|---|---|---:|---:|---:|---:|---:|---:|
|ram8/write_batch1|µs/txn|2.025|1.760|1.797|7.237|1.0268x|0.2480x|
|ram8/write_batch64|µs/txn64rows|35.036|26.995|27.406|82.733|1.0251x|0.3316x|
|ram8/held_hit|ns/query|229.467|82.213|82.100|241.038|0.9986x|0.3424x|
|ram8/held_miss|ns/query|195.564|53.499|53.358|241.455|0.9871x|0.2210x|
|ram8/begin_get_end|ns/query|259.592|110.335|110.502|415.481|0.9965x|0.2656x|
|ram8/ordered_scan|ns/row|24.957|15.590|15.748|16.179|1.0074x|0.9707x|
|insert16/insert|µs/row|1.682|1.706|1.698|1.179|0.9938x|1.4401x|

## Original transaction tails

Each displayed percentile is the median of the five per-process floor((N−1)×p/100) quantiles. Pooled quantiles are supplementary only in the retained local summary and are not substituted here. Random disk tails cover a full1000-put transaction, not one row. RAM16/64 has 32 transactions/process; its ranked tails are descriptive of that small vector. The 100k-row disk read seed is one transaction and has no meaningful p95/p99.

|Case/cell|Arm|Samples/process|p50 µs/TX|p95 µs/TX|p99 µs/TX|Median process max µs/TX|Worst observed max µs/TX|
|---|---|---:|---:|---:|---:|---:|---:|
|disk/random/insert|Accepted32 opt-in|100|2,929.845|3,427.578|3,498.020|3,595.373|3,711.310|
|disk/random/insert|Previous06 opt-in|100|2,980.360|3,519.591|3,733.602|3,756.906|5,445.854|
|disk/random/insert|Latest11 opt-in|100|2,856.968|3,403.643|3,492.651|3,620.790|5,944.038|
|disk/random/insert|LMDB|100|5,992.970|7,319.838|7,370.683|7,627.906|7,762.098|
|ram/ram8/write_batch1|Accepted32 opt-in|128|1.493|1.593|6.332|6.752|11.402|
|ram/ram8/write_batch1|Previous06 opt-in|128|1.363|1.433|6.502|7.404|13.506|
|ram/ram8/write_batch1|Latest11 opt-in|128|1.402|1.462|6.502|7.314|27.092|
|ram/ram8/write_batch1|LMDB|128|6.653|7.053|7.274|13.165|14.117|
|ram/ram8/write_batch64|Accepted32 opt-in|128|34.405|39.215|40.898|43.543|67.959|
|ram/ram8/write_batch64|Previous06 opt-in|128|25.118|29.315|31.270|36.229|36.871|
|ram/ram8/write_batch64|Latest11 opt-in|128|25.528|30.327|31.690|35.798|37.661|
|ram/ram8/write_batch64|LMDB|128|81.745|89.160|93.949|100.010|107.214|
|ram/insert16/insert|Accepted32 opt-in|32|441.971|531.671|535.578|543.884|752.832|
|ram/insert16/insert|Previous06 opt-in|32|456.558|516.062|537.271|547.571|548.222|
|ram/insert16/insert|Latest11 opt-in|32|450.006|511.022|529.347|537.202|543.493|
|ram/insert16/insert|LMDB|32|309.088|416.221|420.038|425.900|431.270|
|disk/single/insert|Accepted32 opt-in|1000|24.105|140.683|222.277|295.654|640.761|
|disk/single/insert|Previous06 opt-in|1000|23.234|33.453|216.676|393.017|411.722|
|disk/single/insert|Latest11 opt-in|1000|23.695|28.834|40.205|388.077|405.431|
|disk/single/insert|LMDB|1000|54.883|66.494|320.391|571.211|737.142|
|disk/single/insert|Public default Legacy (unpaired)|1000|3,992.427|7,597.489|21,363.346|24,173.106|24,624.682|
|disk/random/insert|Public default Legacy (unpaired)|100|13,006.443|35,577.644|37,728.278|43,068.302|44,372.879|

## High-entropy 64-byte values (separate workload)

Same1000 sorted keys and complete transaction clock, with all 64 value bytes generated by the independently verified SplitMix stream inside the clock in both engines. Compression dependence is observable rather than inferred.

|Arm|Median µs/txn|Median process p95 µs|Median process p99 µs|Median process max µs|
|---|---:|---:|---:|---:|
|Previous06 opt-in|32.623|46.537|229.710|375.054|
|Latest11 opt-in|27.341|29.234|84.949|386.725|
|LMDB|63.161|65.593|280.607|590.828|

## Memory and allocated disk

Medians include input/query arrays, observers and engine state. Final inventories include directory allocation; apparent sparse-file length is not physical space. Child peak RSS is a kernel highwater gauge, not the configured quota. Per-process aggregates and resource totals are published in [results.json](results.json); the complete phase endpoints remain in retained local evidence.

|Suite/case|Arm|Median child peak RSS KiB|Median final allocated B|Median final apparent B|
|---|---|---:|---:|---:|
|ram/ram8|Accepted32 opt-in|39,264|888,832|8,388,772|
|ram/ram8|Previous06 opt-in|28,008|528,384|8,392,764|
|ram/ram8|Latest11 opt-in|28,004|528,384|8,392,764|
|ram/ram8|LMDB|24,928|614,400|618,576|
|ram/insert16|Accepted32 opt-in|23,904|802,816|8,388,772|
|ram/insert16|Previous06 opt-in|18,432|692,224|8,392,764|
|ram/insert16|Latest11 opt-in|18,432|692,224|8,392,764|
|ram/insert16|LMDB|18,432|2,396,160|2,400,336|
|disk/single|Accepted32 opt-in|28,292|266,240|67,113,024|
|disk/single|Previous06 opt-in|6,144|135,168|67,117,056|
|disk/single|Latest11 opt-in|7,168|73,728|67,117,056|
|disk/single|LMDB|4,096|143,360|147,456|
|disk/single|Public default Legacy (unpaired)|9,216|9,158,656|7,109,888|
|disk/random|Accepted32 opt-in|59,392|9,625,600|67,113,024|
|disk/random|Previous06 opt-in|58,048|8,318,976|67,117,056|
|disk/random|Latest11 opt-in|58,912|8,318,976|67,117,056|
|disk/random|LMDB|30,720|20,611,072|20,615,168|
|disk/random|Public default Legacy (unpaired)|109,892|55,173,120|54,841,920|
|disk/reads|Accepted32 opt-in|113,264|9,609,216|67,113,024|
|disk/reads|Previous06 opt-in|86,832|8,310,784|67,117,056|
|disk/reads|Latest11 opt-in|89,136|8,310,784|67,117,056|
|disk/reads|LMDB|25,160|9,392,128|9,396,224|
|disk/reads|Public default Legacy (unpaired)|26,624|9,367,552|9,336,128|

## Full disk lifecycle and diagnostic phases

Units below are milliseconds per stated operation. Seed/insert/verify/scan rows divide by rows; get rows divide by queries; range divides by3000 fresh cursor/32-row attempts. These are all actual timed phases, including extra observations; `process_total` is not engine-only work. Backend verify/stats and terminal sync/GC perform different native work between implementations.

|Case/phase|Public default Legacy (unpaired)|Accepted32 opt-in|Previous06 opt-in|Latest11 opt-in|LMDB|
|---|---:|---:|---:|---:|---:|
|reads/cold_create|10.063774|17.343788|1.822367|2.062307|0.234821|
|reads/seed_sorted|0.001193|0.001044|0.001171|0.001148|0.000504|
|reads/verify_after_insert|0.000029|0.000035|0.000025|0.000025|0.000036|
|reads/close_before_reopen|0.663364|8.132513|4.858812|4.831211|0.646753|
|reads/cold_open|7.107640|80.017990|76.459306|76.839609|0.214472|
|reads/first_read_after_open|0.002184|0.004679|0.002425|0.002514|0.018174|
|reads/warmup|0.000506|0.000575|0.000239|0.000238|0.000365|
|reads/get_hit|0.000505|0.000573|0.000231|0.000230|0.000356|
|reads/get_miss|0.000470|0.000312|0.000118|0.000117|0.000331|
|reads/get_new_txn|0.000571|0.000626|0.000272|0.000273|0.000460|
|reads/cursor_scan|0.000020|0.000027|0.000018|0.000018|0.000015|
|reads/range_32_fresh_cursor|0.001527|0.567707|0.001465|0.001454|0.000844|
|reads/terminal_settle|0.862788|0.022613|0.053229|0.061355|0.006913|
|reads/verify_final_values|0.000030|0.000033|0.000024|0.000024|0.000023|
|reads/backend_verify|13.626757|0.002164|0.002685|0.002685|0.001342|
|reads/final_close|0.715462|4.323579|3.822730|2.178736|0.485981|
|reads/process_total|775.726048|2,548.774616|556.784583|555.321730|517.050153|
|random/cold_create|10.031183|17.194247|2.120748|1.949236|0.222798|
|random/insert|0.016386|0.002835|0.002857|0.002790|0.005492|
|random/verify_after_insert|0.000038|0.000144|0.000101|0.000099|0.000050|
|random/close_before_reopen|2.538661|9.562674|35.474030|34.850851|1.062382|
|random/cold_open|28.975463|231.850435|249.496559|247.624158|0.151584|
|random/first_read_after_open|0.002825|0.002745|0.002234|0.002625|0.012944|
|random/terminal_settle|1.252970|0.034825|0.080210|0.087935|0.003787|
|random/verify_final_values|0.000052|0.000143|0.000101|0.000101|0.000056|
|random/backend_verify|17.171875|0.002064|0.002896|0.003185|0.001753|
|random/final_close|2.545634|8.517153|3.904051|3.728843|0.944662|
|random/process_total|1,795.132707|629.771016|641.760602|635.852130|596.559840|
|single/cold_create|9.815027|17.895042|2.001594|1.977569|0.224040|
|single/insert|4.476040|0.036289|0.030190|0.025767|0.063821|
|single/verify_after_insert|0.000031|0.000034|0.000024|0.000024|0.000027|
|single/close_before_reopen|0.843322|3.492019|0.831208|0.808446|0.064140|
|single/cold_open|6.340672|17.618182|3.274681|3.257690|0.136586|
|single/first_read_after_open|0.001733|0.002154|0.000861|0.001012|0.007635|
|single/terminal_settle|3.620691|0.021119|0.043852|0.048371|0.002455|
|single/verify_final_values|0.000030|0.000034|0.000023|0.000023|0.000023|
|single/backend_verify|0.139411|0.001032|0.000872|0.000902|0.000981|
|single/final_close|0.806903|1.081759|0.275466|0.249468|0.039935|
|single/process_total|4,577.262170|102.289669|57.993103|53.978053|86.087914|

## RAM lifecycle

Milliseconds per fresh process, medians of five. No public-default RAM cohort.

|Case/phase|Accepted32 opt-in|Previous06 opt-in|Latest11 opt-in|LMDB|Latest/06 matched latency|
|---|---:|---:|---:|---:|---:|
|ram8/lifecycle_close_ns|0.617|0.259|0.262|0.065|1.0128x|
|ram8/lifecycle_create_ns|7.623|0.748|0.751|0.240|1.0116x|
|ram8/lifecycle_reopen_ns|12.390|12.369|12.448|0.170|0.9984x|
|insert16/lifecycle_close_ns|0.892|4.696|4.702|0.155|1.0016x|
|insert16/lifecycle_create_ns|7.691|0.742|0.750|0.236|1.0186x|
|insert16/lifecycle_reopen_ns|14.308|14.075|14.087|1.099|0.9990x|

## Terminal maintenance charged to the caller

Primary original clock boundaries remain unchanged; these additional rows add the complete separately measured terminal collection/forced-sync phase.

|Suite/case/cell|Accepted32 ns/op|Previous06 ns/op|Latest11 ns/op|LMDB ns/op|
|---|---:|---:|---:|---:|
|disk/reads/seed_sorted_plus_terminal|1,044.568|1,171.766|1,148.540|504.093|
|disk/random/insert_plus_terminal|2,835.230|2,857.576|2,790.467|5,491.993|
|ram/ram8/write_batch1_plus_terminal|2,025.570|1,760.840|1,798.375|7,236.957|
|ram/ram8/write_batch64_plus_terminal|35,039.668|26,998.883|27,410.168|82,733.359|
|ram/insert16/insert_plus_terminal|1,682.388|1,705.933|1,697.719|1,179.004|
|disk/single/insert_plus_terminal|36,310.589|30,232.900|25,815.657|63,823.499|

## Adverse evidence and measurement limits

The all-metrics/no-regression objective is not achieved. Relative to previous06, matched RAM single/batch64 mean latency rose 2.68%/2.51%; single worsened 5/5 pairs, batch64 worsened 3/5. Their p99 rose 3.40%/1.28%, and batch64 p95 rose 3.76%. Canonical single p50 rose 1.94% (all five pairs), while entropy p50 rose 1.71% (all five). Random-batch p99 worsened 3/5 pairs: +0.25%, +12.79% and +15.18%; the median matched p99 ratio is1.00245. Latest11 still loses LMDB on RAM key16/value64 insertion (1.4401x matched latency), original disk scan (1.1873x), fresh32-row ranges (1.454 versus 0.844 µs/attempt), large sorted read-fixture seed (1.148 versus 0.504 µs/row), and cold open. Across the 640 observed RAM single-write samples per arm, the worst latest11 sample is27.092 µs versus 13.506 µs for 06; this outlier is retained alongside the per-process tail medians. Entropy p99 is above previous06 in two of five pairs (+2.73%, +3.57%), despite lower median tails and lower p99 than LMDB in all five pairs. All those observations remain retained.

Other adverse observations versus 06 are visible in full lifecycle tables: read-fixture creation +15.89%, first read after open +3.24% (read fixture), +15.95% (random) and +28.30% (single); separately timed terminal settlement +15.73%/+10.08%/+11.22% for reads/random/single. RAM creation medians of matched ratios rose 1.16% (ram8) and 1.86% (insert16); RAM8 close rose 1.28%. Several tiny backend-stat phases also rose. Disk child peak RSS marginal medians rose 6144→7168 KiB (single),58048→58912 KiB (random),86832→89136 KiB (reads); these include observer/image effects and are not exclusive engine allocation counts. The material single-case allocated-inventory gain is73728 versus 135168 B (latest11 versus 06), including directory inventory.

Journal bytes submitted to the kernel rose from 134,024 to 2,206,838 for the
canonical single-write fixture (16.47×), and from 176,104 to 2,269,341 for the
entropy fixture (12.89×). Synchronization counts stayed unchanged. Submitted
bytes, allocated filesystem blocks and physical device writes are distinct
measurements.

The new primary disk executable also contains the public-default Legacy adapter; previous06 primary disk executable did not. Generic operation bodies are byte-identical, but extra linked code can change image/layout/RSS. Its actual impact is unproven; no result was discarded or rerun to resolve this after timing began. RAM always had its existing public-backend dispatch; no new Legacy disk adapter was linked into the RAM executable.

A brief controller-only scheduling hold occurred after 41 native processes had already exited successfully. Only Python controller PID3801540 received SIGSTOP/CONT; no timed child was signaled, interrupted, replaced or rerun, and all subsequent processes continued in the original declared order with the same frozen binaries. The scheduling records remain in the retained local evidence.

Cold open means a fresh process attachment without dropping the OS page cache. Held reads bind a KV view before their original clocks; begin/get/end includes that binding. Timed canonical reads check value length/endpoints, with complete64 B key/value/order/reopen oracles outside those primary read clocks. Actual backend stats expose format 3 mapped RAM, format 4 persistent pages for small data, original compact fallback for large batches, reservations, submitted bytes and all live/quota/GC state. Prefix types, duplicates, custom comparators, backup/replication and independent-process readers are explicitly UNAVAILABLE in the opt-in backend. Device-power-loss behavior remains unqualified; no production-parity, all-metrics completion or default promotion is claimed.

## Measured configuration

The disk fixtures ran on a Linux NVMe/ext4 volume, pinned to logical CPU16.
RAM fixtures used tmpfs. The OS page cache was not dropped. Release builds used
O3, ThinLTO and one codegen unit; optional profiling clocks were disabled.
Disk and entropy use LMDB 0.9.36 with flags0 and an 8 GiB maximum map; RAM retains
the original LMDB 1.0.2 control. Configured virtual limits are not allocated space.

Build the optional backend with `cargo build --release --locked -p ulmdb --features snapshot-log`.
The measured API is `ulmdb::snapshot_log::Environment`, using:

```rust
ulmdb::snapshot_log::Options {
    index_bytes: 192 << 20,
    journal_capacity: 32 << 20,
    transaction_bytes: 16 << 20,
    max_operations: 131072,
    max_databases: 4096,
    batch_threshold: 16,
    versioned_values: true,
    exact_lookup: true,
}
```

New persistent environments on 4 KiB hosts used journal format 4; RAM used format 3.
The ordinary `ulmdb::Environment::create/open` API uses Legacy and was measured
without changing its backend, maintenance, residency or durability settings.

Disk single inserts 1000 new key16/value64 rows in 1000 transactions. Random
insertion uses 100000 unique rows in 100 transactions of1000 puts. Reads use
100000 sorted rows, close/reopen,300000 warmup/hit/miss queries, begin/get/end
for at least100 ms, one full scan, and 3000 fresh32-row range cursors. RAM8/8
uses 8192 keys with single/batch64 replacements; RAM16/64 inserts 8192 new rows
in 256-row transactions. Entropy values use an independent eight-word SplitMix
stream generated inside each transaction clock. Terminal maintenance/sync is
measured separately and also charged in the additional rows above.

## Published evidence and source identity

[results.json](results.json) contains every canonical latency-summary cell,
per-process aggregate times and tail ranks, the entropy and unpaired Legacy
summaries, allocated/apparent inventory, peak process RSS, journal counters and
measured production-source hashes. Transaction vectors, raw databases, binaries,
full source archives, test logs and the complete independent audit remain in
retained local evidence; they are not included in this repository snapshot.
The existing `benchmark/run.sh` Resident/default suites are different workloads
and do not reproduce this130-process campaign.

The measured integrated source passed 58 storage tests and 54 backend tests before
timing. All 130 declared engine processes exited successfully and passed complete
value/order/reopen checks; no engine process was retried, replaced or excluded.
An independent read-only audit verified 813 frozen input files, both candidate
source archives, process identities and 48285 transaction-sample ranks. These are
separate correctness and measurement checks, not device-power-loss qualification.

The committed production source is `2d2acf54643496aafbf4f69c9b0bf7b8d7ad069a`. This commit was created after
measurement; 762 production files/manifests were checked against
the frozen measured hashes when preparing this publication.

Candidate RAM ELF SHA256: `d210e7598beabb2c255c054d38f9cea8f2a4ecbcb739dbd5e24b91a92d256c1e`.

Candidate disk ELF SHA256: `6d05f14517417a3c70f4f6904ad0e2d46a37b56c34a88ca2d86ba9aab4f27d87`.

Disk LMDB DSO SHA256: `733c05a9090c015293afd024cdec3b90bf966083e2cd49a5ce03caafb506e5ab`.

Retained full-report SHA256: `a53e9f6c0bedff586321ce3169af946669e5d86f88bdb9996faaef890efb5375`.
