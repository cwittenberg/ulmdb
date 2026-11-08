# Snapshot-log benchmark — latest22, 2026-11-08

Latest22 has higher displayed median throughput than LMDB in all 13 primary rows. **The 10× and strict no-regression targets remain unmet.** New-key RAM insertion wins only 3/5 matched mean comparisons; RAM reads remain slower than Retained11, and disk held-hit latency is higher than Prior19. Cold open, ranges, memory and submission costs remain explicit below.

This is the real opt-in `ulmdb::snapshot_log` versioned backend, using complete public transaction calls. Public `ulmdb::Environment::create/open` still selects **Legacy**; it was not remeasured or promoted in this run. Prefix types, duplicates, custom comparators, backup/replication and independent-process readers are unavailable in the opt-in API. These results do not establish complete LMDB feature parity or universal performance.

The 140 fresh engine processes comprise 125 canonical runs (five matched quintets across five workloads: Retained11, Previous06, Prior19, Latest22 and LMDB) and 15 separate entropy runs (five matched Retained11/Latest22/LMDB triplets). All declared source/input/dispatch guards and complete applicable value/order/snapshot/abort/reopen oracles passed. No process was retried, replaced or excluded. No synchronization flag was weakened. Development numbers are frozen implementation labels, not release versions.

## All13 primary throughput rows

Values are medians of five process rates. “Difference” is the ratio of displayed marginal median rates minus1, using full precision before rounding. Matched gain is a separate median of five per-process throughput ratios; paired wins are not inferred from the displayed medians. RAM batch64 is normalized to 64 logical puts per transaction; the other cells already report one row/query/one-row transaction.

|Cell|Latest22 op/s|LMDB op/s|Difference|Matched gain|Mean wins/5|
|---|---:|---:|---:|---:|---:|
|RAM 8/8 write, batch1|562,645|139,507|+303.308%|+303.308%|5|
|RAM 8/8 write, batch64|2,390,771|761,530|+213.943%|+213.939%|5|
|RAM 8/8 held hit|11,672,560|4,130,660|+182.583%|+182.583%|5|
|RAM 8/8 held miss|18,223,450|4,183,085|+335.646%|+337.281%|5|
|RAM 8/8 begin/get/end|8,825,075|2,303,870|+283.054%|+281.412%|5|
|RAM 8/8 ordered scan|82,614,113|62,070,422|+33.097%|+33.338%|5|
|RAM 16/64 new-key insert|866,452|850,213|+1.910%|+1.908%|3|
|Disk 16/64 single write|39,776|15,773|+152.177%|+150.708%|5|
|Disk 16/64 random batch1000|521,448|177,492|+193.787%|+187.758%|5|
|Disk 16/64 held hit|6,108,015|2,782,737|+119.497%|+119.117%|5|
|Disk 16/64 held miss|9,703,226|2,995,386|+223.939%|+223.939%|5|
|Disk 16/64 begin/get/end|5,059,189|2,188,654|+131.155%|+132.078%|5|
|Disk 16/64 ordered scan|74,575,386|69,191,655|+7.781%|+9.347%|5|

All five signs/ranges and all five-arm means are in [results.json](results.json). The headline RAM hit is 11.67 million/s versus 4.13million/s LMDB: 2.83× LMDB, not 11.67 million/s for every workload.

## Retained-engine losses

All percentages in this section are **matched latency** changes: positive means slower. Latest22 RAM held hit, held miss and begin/get/end regress versus Retained11 in all five pairs. Disk held hit regresses versus Prior19 in all five. Prior21 was not a paired arm; comparisons with that older campaign are unpaired.

|Cell|Latest/Retained11 latency|Losses/5|Latest/Prior19 latency|Losses/5|
|---|---:|---:|---:|---:|
|ram/ram8/held_hit|+3.701%|5|-5.189%|0|
|ram/ram8/held_miss|+3.194%|5|+3.229%|5|
|ram/ram8/begin_get_end|+3.079%|5|-3.609%|0|
|disk/reads/get_hit|-29.211%|0|+10.398%|5|
|ram/insert16/insert|-31.733%|0|-1.340%|2|

[ADVERSE.md](ADVERSE.md) retains all primary, tail, lifecycle and resource losses against 11/06/19. A lower median does not erase a losing process.

## Original write-transaction tails

Each percentile below is calculated within its original process using floor((N−1)×p/100), then the table shows the median of five process endpoints. A pooled percentile is never substituted. RAM write1/write64 have 128 sampled transactions/process (the separate aggregate half has no individual samples), RAM new-key insertion has 32, disk single 1000 and disk random 100. Batch64, new-key and random tails cover 64-,256- and 1000-row transactions, respectively—not individual rows. No per-query read-tail vectors were captured; the 100k-row read seed is one transaction and has no meaningful p95/p99.

|Write cell|Latest p95 µs/TX|LMDB p95|Latest p99|LMDB p99|Latest median process max|LMDB median process max|
|---|---:|---:|---:|---:|---:|---:|
|ram/ram8/write_batch1|1.472|6.914|6.282|7.334|11.612|11.222|
|ram/ram8/write_batch64|30.518|90.311|31.450|92.105|35.147|96.653|
|ram/insert16/insert|327.152|412.414|335.347|417.824|338.865|421.632|
|disk/single/insert|28.163|64.070|42.540|322.024|289.182|509.886|
|disk/random/insert|2,313.799|7,358.220|2,425.600|7,644.897|2,524.655|7,736.740|

RAM write1 median process maximum is 11.612µs versus 11.222µs LMDB, despite its lower mean and p99. All individual maxima, worst observed maxima and per-process percentile endpoints are published in [PER_PROCESS_TAILS.md](PER_PROCESS_TAILS.md); every recorded transaction vector is in results.json.

## Memory and disk

These are marginal process medians, including fixtures, observers and engine state. PeakRSS is whole-process kernel high water, not an admitted engine-memory bound. Inventories include directory allocation; apparent sparse length is not allocated space. Previous06 disk lacked the extra linked Legacy adapter present in11/19/22, so its image/layout/RSS comparison retains that confound.

|Case|Latest peakRSS KiB|LMDB peakRSS KiB|Latest allocated B|LMDB allocated B|Latest apparent B|LMDB apparent B|
|---|---:|---:|---:|---:|---:|---:|
|ram/ram8|26,984|24,940|528,384|614,400|8,392,764|618,576|
|ram/insert16|18,432|18,432|692,224|2,396,160|8,392,764|2,400,336|
|disk/single|7,168|4,096|73,728|143,360|67,117,056|147,456|
|disk/random|49,104|30,720|8,318,976|20,611,072|67,117,056|20,615,168|
|disk/reads|79,448|25,184|8,310,784|9,392,128|67,117,056|9,396,224|

Source/accounting components and all 125 process inventories are retained in results.json. Current pool metadata costs exceed11/06 by 8200B per RAM fixture and 16392B per disk fixture, while the ordered-index component shrinks. These component charges are not summed into an asserted whole-process bound.

## Cold open, reopen and range costs

Cold attachment does not mean an OS-cache flush: cache dropping was disabled. Native verification/statistics observers differ between engines, so total-process/verification timings are not pure engine lifecycle scores. All 58 canonical phase/diagnostic means, including close/create, are in results.json.

|Operation|Latest22|LMDB|Unit|
|---|---:|---:|---|
|disk/single/cold_open|2.650892|0.134322|ms/attachment|
|disk/random/cold_open|164.167648|0.143980|ms/attachment|
|disk/reads/cold_open|70.161254|0.213089|ms/attachment|
|ram/ram8/lifecycle_reopen_ns|11.827611|0.166317|ms/attachment|
|ram/insert16/lifecycle_reopen_ns|10.520566|1.097717|ms/attachment|
|disk/reads/range_32_fresh_cursor|1,068.059667|832.060333|ns/32-row query|
|disk/reads/seed_sorted|1.020431|0.504119|µs/row|

## Submitted bytes and synchronous work

The journal is unchanged from11/19. Post-workload/pre-reopen counters include setup/catalog plus business work. On persistent storage, `append_bytes` counts bytes submitted through synchronous write vectors, including certificates, repeated zero gaps and reservation zeros. It is neither device traffic nor allocated disk. RAM mapped counters describe bytes modified instead. Reported `sync_calls` counts journal fences/submissions. The packet design’s single-write submission amplification remains a real cost.

|Persistent workload|Latest submitted B|Previous06 submitted B|Ratio|Latest sync calls|Previous06 sync calls|
|---|---:|---:|---:|---:|---:|
|single|2,206,838|134,024|16.46599×|1004|1004|
|random|8,309,704|8,309,704|1.00000×|104|104|
|reads|8,300,200|8,300,200|1.00000×|5|5|

Retained11/Prior19/Latest22 single counters all show2,206,838B and1004 sync calls, versus134,024B/1004 for06:16.46599× submitted bytes. Random/read-seed counters are identical across those four implementations. LMDB submission counts were not instrumented; no equal-fence or device-byte claim is made. Kernel output-block accounting remains separate in results.json.

The measured RAM new-key pending charge is 8192B in all five processes: before-header 524680B, after 532872B, workspace 2048B. Prior19 retains 262144B pending. This verifies the bounded sparse path/charge, without assigning a per-allocation latency or moving setup cost outside clocks.

## Separate high-entropy 64-byte values

The15 entropy processes use the same 1000 sorted keys but a separately verified SplitMix stream generates all 64 value bytes inside each transaction clock. Retained11/Latest22/LMDB are matched here; older06/19 entropy scores are not pooled into this cohort or the canonical table.

|Arm|Mean µs/TX|p95 µs/TX|p99 µs/TX|Median process max µs/TX|
|---|---:|---:|---:|---:|
|Retained11|27.404|29.485|81.082|382.908|
|Candidate22|26.584|28.664|63.479|307.847|
|Contemporaneous LMDB|65.414|65.683|342.883|718.678|

## Reproducibility, limits and published evidence

- Engine children were pinned to CPU16. Persistent fixtures used ext4 on `/mnt/data3`; RAM used `/dev/shm` with ordinary synchronous APIs. RAM tmpfs results do not establish physical-device or reboot durability. No power-cut qualification ran.
- RAM control is native LMDB 1.0.2; disk/entropy control is freshly built historical LMDB 0.9.36 with flags 0 and an unnamed 8GiB map. They are separate controls. ULMDB durability is Full; no `NOSYNC`/async acknowledgement was introduced.
- Workloads/clock boundaries remained unchanged: RAM 8192 existing 8/8 rows with 256 writes per batch size; RAM 8192 new 16/64 rows in 32×256 transactions; disk 1000 single writes,100000 rows in 100×1000 random batches, and100000 sorted read-seed rows plus 300000 hit/miss queries. Begin/put/commit, value generation, held-view/cursor boundaries and terminal maintenance retain their original scopes.
- RAM quotas: 64MiB index split 32/32, two 4MiB journal banks. Disk: 192MiB split 96/96, two 32MiB banks. Quota/pending/index/pool counters are actual production outputs, not an 8GiB physical allocation claim.
- 80 focused backend checks and the unchanged 21 source proofs passed before timing; both actual ordinary 22 ELFs were checked for the requested outlined-get calls, Outcome-only results and preserved guards. These intent checks are not speed proofs.
- All 789 compiled inputs and 761 shared production inputs, including `lookup/pending.rs`, matched the current workspace at the recorded source-proof time. Exact component, ELF, context, archive and native-library hashes are in [results.json](results.json). Source identity is not inferred from a clean Git commit.

`results.json` publishes all 13 primary rate/ratio/sign rows, all 58 canonical metric aggregates and per-repetition means, all 49,725 recorded transaction samples in 165 vectors, process resources, tails, lifecycle and I/O/accounting/source identities. It does **not** contain every native JSONL field, launcher file, fixture database, binary or complete source archive. Complete raw local provenance is retained outside this portable package at:

```text
/mnt/data3/ipdon/ulmdb-dev-artifacts/measurements/outline-22-full
```

That path is local-only, not a GitHub download link. Public portable evidence is [results.json](results.json), [ADVERSE.md](ADVERSE.md) and [PER_PROCESS_TAILS.md](PER_PROCESS_TAILS.md). No binary or database is included.

## Earlier evidence retained

The previous public report/data are preserved byte-for-byte in [prior-11/REPORT.md](prior-11/REPORT.md) and [prior-11/results.json](prior-11/results.json), with hashes in [prior-11/ARCHIVE.json](prior-11/ARCHIVE.json). Their 130-process campaign includes a separate unpaired Legacy cohort; it is historical and is not labeled latest22. SQLite was not measured in this campaign; the older separate SQLite comparison is not changed or merged here.
