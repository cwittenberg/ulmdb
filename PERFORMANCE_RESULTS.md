# Measured optimization campaign

**Published accepted baseline: iteration6. Latest THREE-COHORT EXPERIMENT: prefix cold-entry candidate HOLD, closed after117 original workloads; no promotion or further automatic runs.** The best retained private candidate remains admitted541 Collect mark deduplication after its separate117 observations. Current-main integration V6 remains HOLD/unqualified. The published scorecard/charts and all previous evidence remain unchanged.

## Latest THREE-COHORT EXPERIMENT: cold-entry candidate HOLD — closed, no promotion

**Root's final decision is HOLD this exact frozen551-source artifact: no promotion and no further automatic runs.** [Final root retention decision](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/root-retention-decision-v1.json). All three unchanged39 cohorts are complete:117 original workloads/oracles pass,109 boundary observations are quiet and eight are exposed. Nothing is excluded, replaced or normalized by Native. The best retained private candidate remains the separately ADMITTED541 Collect mark-dedup branch (+5.334% RAM single aggregate/+3.393% combined after its own117 observations); published iteration6, README, accepted charts and scorecard remain unchanged. Current-main integration V6 remains HOLD/unqualified. These private results do not claim a fix of current main.

Both the full AND million-key suites use the **experimental Resident backend**, not ULMDB's default native file backend. A and B are fresh same-version controls using the SAME admitted541 executables/reference; B→C is primary, while A→C and B→A remain visible. Combined centers are computed from the original twelve full-suite pairs per filesystem or nine million pairs, not by averaging reported percentages. The complete [combined103 direct Native targets/status/gaps](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-three-cohorts-v1/report/NATIVE_SCORECARD.md) and [combined474 metrics, pairs and flags](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-three-cohorts-v1/report/ALL_COMPARISONS.md) retain every target and unavailable cell.

| Throughput | Admitted541 B | Experimental C | Combined C/B | Contemporary LMDB | C/LMDB |
|---|---:|---:|---:|---:|---:|
|RAM one-record aggregate|65,955.768|78,362.167|+18.810%|134,046.871|-41.541%|
|RAM one-record combined|77,951.144|85,425.342|+9.588%|138,272.769|-38.220%|
|RAM batch64 aggregate|984,068.353|975,023.392|-0.919%|747,336.403|+30.466%|
|RAM batch64 combined|992,707.159|983,758.098|-0.901%|754,807.557|+30.332%|
|Disk one-record aggregate|15,095.131|13,574.762|-10.072%|10,243.224|+32.524%|
|Disk one-record combined|15,627.423|14,523.839|-7.062%|10,948.171|+32.660%|
|Disk batch64 aggregate|490,598.715|486,067.477|-0.924%|174,793.668|+178.081%|
|Disk batch64 combined|501,268.879|494,232.050|-1.404%|180,311.794|+174.099%|
|Million load|1,383,292.037|1,369,230.275|-1.017%|1,573,447.645|-12.979%|
|Million read|2,788,514.156|2,731,660.234|-2.039%|1,696,024.731|+61.063%|
|Million update|146,420.881|145,778.453|-0.439%|128,414.581|+13.522%|

RAM single-write aggregate/combined gains are+18.810%/+9.588%, favorable in all12 pairs against BOTH A and B (C/A+19.267%/+9.922%). This is a reproduced cold-aggregate benefit, not warmed-write parity. RAM batch64 combined is adverse11/12 B pairs and all three centers are adverse versus A. Disk1 is unstable: the cohort2 aggregate/combined losses remain−35.680%/−28.244%, including a quiet candidate observation; the combined result is−10.072%/−7.062%. A/A timing spread and contemporary Native movement remain context, not a causal explanation or correction factor.

### Every cohort and protected read vector remains separate

| C/B throughput change | Cohort1 | Cohort2 | Cohort3 | All3 original observations |
|---|---:|---:|---:|---:|
|RAM one-record aggregate|+18.598%|+18.221%|+19.616%|+18.810%|
|RAM one-record combined|+9.022%|+9.921%|+9.825%|+9.588%|
|RAM batch64 aggregate|-1.046%|-0.714%|-0.997%|-0.919%|
|RAM batch64 combined|-0.932%|-0.819%|-0.954%|-0.901%|
|Disk one-record aggregate|+10.992%|-35.680%|+1.871%|-10.072%|
|Disk one-record combined|+13.916%|-28.244%|-1.794%|-7.062%|
|Disk batch64 aggregate|-1.534%|+1.790%|-2.967%|-0.924%|
|Disk batch64 combined|-1.418%|-0.145%|-2.633%|-1.404%|
|Million load|-0.869%|-0.640%|-1.539%|-1.017%|
|Million read|-2.141%|-0.999%|-2.967%|-2.039%|
|Million update|-0.555%|-0.265%|-0.496%|-0.439%|
|RAM IPv6 ordinary misses|-9.526%|-0.132%|-1.456%|-3.796%|
|RAM IPv6 inside-gap misses|-8.372%|-0.153%|-0.900%|-3.214%|

RAM IPv6 ordinary/inside-gap misses are adverse10/12 B pairs. The large cohort1 losses shrink in cohorts2/3 but remain in the pooled−3.796%/−3.214% centers. The quiet cohort1 round2 Native slowdown does not justify normalizing target reads or dropping the observation. Other pooled ordered-KV hit/miss/mixed rates are RAM+0.220/+0.045/+0.796% and disk+0.920/+1.601/+1.605%. Million reads lose2.039%, adverse7/9 B pairs and all three centers against A as well. No protected case is omitted: [all13 reads on BOTH filesystems, every cohort/pair and A/A context](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-three-cohorts-v1/ALL_READ_WRITE_VECTORS_AND_AA.md) and [independent26-case all-cohort review](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/retention-review-all3-v1/all26-reads-all-cohorts.json) retain rates, sampled p99 and direct Native observations.

Each cohort's original103/474 tables are preserved separately: [Cohort1 Native103](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-v1/report/NATIVE_SCORECARD.md) / [Cohort1 metrics474](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-v1/report/ALL_COMPARISONS.md); [Cohort2 Native103](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-v2/report/NATIVE_SCORECARD.md) / [Cohort2 metrics474](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-v2/report/ALL_COMPARISONS.md); [Cohort3 Native103](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-v3/report/NATIVE_SCORECARD.md) / [Cohort3 metrics474](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-v3/report/ALL_COMPARISONS.md). The [full original observations and explicit cohort/round association](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-three-cohorts-v1/aggregation-provenance.json) preserves all1,422 individual comparison cells/309 Native targets alongside combined474/103.

### True GLOBAL1536 tails do not establish a clean tradeoff

These are nearest-rank quantiles of all1,536 original sampled transactions per arm/cell. All per-cohort512, process-pooled and trial-geometric summaries remain separate.

| Resident cell | A global p99 ns | B global p99 ns | C global p99 ns | Raw C/B p99 | Raw C/A p99 | B→C max ns |
|---|---:|---:|---:|---:|---:|---|
|RAM one record|15,440|12,043|14,809|+22.968%|−4.087%|17,724→52,450|
|RAM batch64|250,556|249,866|256,468|+2.642%|+2.360%|267,298→273,570|
|Disk one record|313,947|320,069|312,113|−2.486%|−0.584%|1,300,924→588,730|
|Disk batch64|595,452|432,042|392,917|−9.056%|−34.014%|631,381→642,532|

Positive tail change is worse. RAM1 C/B cohort-global p99 changes are+51.067/−20.114/+33.957%; all four process p99s worsen in cohorts1/3, while only one worsens in cohort2. Its pooled maximum is52,450ns versus B17,724/A23,775ns. The A/A spread and improved C/A pooled p99 prevent a simple causal claim of a23% candidate-added latency, and also prevent calling the latency tradeoff reproducibly clean. Cohort3 has no exposed Resident process. RAM64 cohort3 global p99 rises7.580% versus B/8.329% versus A, with all four process p99s worse against BOTH controls. Disk64≥500µs counts rise10→12 despite its improved pooled p99; disk1 counts fall6→3. No summary replaces another. [ALL cohort512 AND combined1536 tails, BOTH engines](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-three-cohorts-v1/ALL_COHORT_AND_GLOBAL_TAILS.md), [complete distributions/thresholds JSON](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-three-cohorts-v1/global-write-tails-all3.json) and [independent96-quantile-cell reconstruction](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/retention-review-all3-v1/independent-all96-global-write-tails.json) retain all adverse tails and maxima.

### Upfront, sampled and broader named costs are conserved; no net-wall claim

The original setup seeds BOTH engines then calls after_setup; no benchmark pre-enabling, audit priming, warmup or workload/timer change is introduced. Arithmetic mean RAM sampled one-record cost increases exactly20.015833µs per process's128 sampled commits (20,015.833333ns), versus mean aggregate saving307.334µs. This is the sampled phase, not a measured pure-commit CPU share. The matched per-process geometric sampled-cost center is+1.483%; it is distinct from the arithmetic mean difference. Mean front cost is+8.688µs and RAM64 combined cost+150.046µs. Below, units are µs except the explicitly labelled whole-child seconds. Combined rows overlap aggregate+sampled components and must not be added again.

| RAM named-timer C−B arithmetic mean | Cohort1 | Cohort2 | Cohort3 | All3 |
|---|---:|---:|---:|---:|
|Fixture load BOTH engines|+28.514000|-9.999250|+15.271500|+11.262083|
|Adjacent after_setup|-3.298750|-1.369750|-3.053000|-2.573833|
|Disjoint front sum|+25.215250|-11.369000|+12.218500|+8.688250|
|Resident creation|+324.584750|-266.371000|-140.241500|-27.342583|
|Resident one-record aggregate|-307.410250|-296.968000|-317.624000|-307.334083|
|Resident one-record sampled|+33.596500|+1.720750|+24.730250|+20.015833|
|Resident one-record combined|-273.813750|-295.247250|-292.893750|-287.318250|
|Resident batch64 aggregate|+88.130250|+59.686250|+83.643250|+77.153250|
|Resident batch64 sampled|+67.541250|+75.963750|+75.173250|+72.892750|
|Resident batch64 combined|+155.671500|+135.650000|+158.816500|+150.046000|
|Resident BOTH write cases|-118.142250|-159.597250|-134.077250|-137.272250|
|BOTH engines, BOTH write cases|-325.136000|-466.740250|-189.847500|-327.241250|
|Whole child ALL workload (seconds)|+0.004861|-0.001076|-0.006826|-0.001014|

The [complete every-cohort/every-process timer ledger, BOTH filesystems](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-three-cohorts-v1/UPFRONT_AND_WRITE_COSTS_ALL3.md) includes dataset/create/front, each engine's original aggregate/sampled/combined phases and whole-child time; [same-population machine-readable costs](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-three-cohorts-v1/same-population-cost-ledger-all3.json) preserves exact denominators. Per-process scope sums are computed before centers: RAM front+ULM1 combined improves2.252% versus B; front+both Resident write cases improves0.445%; adding creation improves0.209% (only0.116% versus A). These are small partial named-scope observations, not a new acceptance gate, whole workload wall result, or a sum of unrelated geometric means. They exclude intervening validation/dispatch/read/cold-check work; whole-child time contains all such work and cannot be assigned to writes. [Independent scope-sum proof](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/retention-review-all3-v1/front-and-business-cost-scopes.json) retains the adverse disk scopes as well.

Million load/read/update remain−1.017/−2.039/−0.439% versus B; every cohort's center is adverse. Setup cost+0.390% is mixed by cohort; drop+open−1.482% is not a recovery fix. Native gaps remain large: setup409.510ms versus1.643ms (~249×), drop+open6.818s versus3.804ms (~1,792×), allocation256MiB versus~33.04MiB (~7.75×). Full-suite Native cold-reopen values remain unavailable. No storage/reopen parity or native-backend improvement is claimed.

### Closed evidence and next boundary

All117 children return zero and pass original value/borrow/cold/source/native/resource oracles. Exposures are exactly C1[004,033,036], C2[009,019,029,032], C3[038], all retained: [all117 availability records](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/retention-review-all3-v1/availability117.json). Quiet boundaries are not continuous-isolation proof. The [primary closed all3 report](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-three-cohorts-v1/REPORT.md) / [immutable primary closure receipt](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-three-cohorts-v1/final-receipt.json), [independent full raw/source/math audit](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/independent-three-cohort-review-v1/REPORT.md) / [audit receipt](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/independent-three-cohort-review-v1/receipt.json), and [closed independent HOLD decision](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/retention-review-all3-v1/REPORT.md) / [retention receipt](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/retention-review-all3-v1/receipt.json) verify all72 raw files/2,160 populations,72 cohort512+24 combined1536 cells,551/483 source maps, actual Native contexts and every original table. [Root preservation after all3](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/root-preservation-after-three-cohorts-v1.json) confirms C1's250 and C2's163 preserved files byte-for-byte.

The6,479-read conservation/layout/abort/admission proof and838-test qualification remain in the historical first-cohort section below; they establish mechanism/correctness, not performance admission or nanosecond attribution. Best admitted541 and all earlier results remain unchanged. The next proposed [StructuralKV ownership control](benchmark/work/optimization-20261105/next-native-kv-page-ownership/PLAN.md) is SOURCE-ONLY/not implemented: explicit ownership admission, typed final-link tracking, Prefix/Tiny fallback and current-Copy deferral require independent design review and recovery/checkpoint cost gates. It is not authorization for a new run or a predicted win. This exact C551 artifact stays HOLD; no promotion or further automatic measurement follows.

## Previous first-cohort HOLD checkpoint (historical, all evidence retained)

The frozen551-source private candidate has completed **one** unchanged39 campaign: all39 original oracles pass, with36 quiet and3 exposed observations retained. Root's decision is **HOLD, no admission or main integration**. Exactly two additional unchanged39 cohorts (v2/v3, three total) are authorized; their results do not exist in this first-cohort summary. No rebuild, gate waiver, quiet-only subset or pass-until-good repetition is authorized. The previously ADMITTED541 mark-dedup candidate remains the best retained private candidate after its separate117 observations; published iteration6 and its scorecard/charts remain unchanged.

**Both the full suite AND the million-key suite use the experimental Resident backend, not ULMDB's default native file backend.** A and B are separate fresh observations of the SAME admitted541 version and authentic reference. B→C is primary; A is a same-version reproducibility control, not an older engine. Contemporary LMDB remains directly reported, never used to normalize ULMDB results. Geometric process centers follow; higher throughput is better. Million load/update are records/s, reads queries/s.

| Throughput metric | Admitted541 B | Experimental C | C/B | Contemporary LMDB | C/LMDB |
|---|---:|---:|---:|---:|---:|
|RAM one-record aggregate|65,316.540|77,464.349|+18.598%|132,543.600|-41.556%|
|RAM one-record combined|77,387.843|84,369.453|+9.022%|137,182.976|-38.499%|
|RAM batch64 aggregate|981,014.512|970,750.024|-1.046%|744,880.007|+30.323%|
|RAM batch64 combined|988,804.624|979,590.184|-0.932%|752,336.314|+30.206%|
|Disk one-record aggregate|14,162.025|15,718.761|+10.992%|10,088.185|+55.814%|
|Disk one-record combined|14,236.760|16,217.899|+13.916%|10,748.893|+50.880%|
|Disk batch64 aggregate|490,152.510|482,635.780|-1.534%|172,910.611|+179.124%|
|Disk batch64 combined|497,188.781|490,140.547|-1.418%|177,678.438|+175.858%|
|Million load|1,387,328.313|1,375,275.300|-0.869%|1,578,877.552|-12.895%|
|Million read|2,815,721.807|2,755,451.205|-2.141%|1,689,216.819|+63.120%|
|Million update|147,126.998|146,310.650|-0.555%|128,802.550|+13.593%|

All four RAM one-record aggregate and combined pairs improve against BOTH controls. The separately sampled one-record phase costs33.597µs more over128 commits (+2.492% arithmetic mean), while the aggregate phase saves307.410µs. This does not establish warmed-write parity or a total wall-time win. RAM batch64 combined is adverse4/4 against both controls; disk batch64 is adverse3/4 versus B. Disk A/B spread is large, so the larger C/A disk gains are retained as observations rather than substituted for the primary comparison. Full detail: [all103 direct Native targets, status and remaining gaps](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-v1/report/NATIVE_SCORECARD.md) and [all474 comparison cells, original pairs, intervals and flags](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-v1/report/ALL_COMPARISONS.md).

### Protected reads and true sampled tails remain blockers

RAM IPv6 inside-gap/ordinary-miss rates are−8.372%/−9.526% versus B, adverse4/4; they are also−7.387%/−9.202% versus A. Quiet C round2 contributes−27.969%/−28.350%, while corresponding LMDB rates also fall−25.131%/−22.063%. This association does not identify a cause, justify normalization, or erase the other adverse ULMDB pairs. RAM IPv6 outside-cover is−2.406%; disk outside-cover is−4.640%. RAM ordered-KV hit/miss/mixed centers are+0.815/−0.840/+1.202%; disk centers+1.769/+3.032/+2.001%. Every13-case read vector on both filesystems remains in [all read/core pairs and Native context](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/independent-result-review-v1/READS_AND_CORE_PAIRS.md) and [the independent26-case review](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/retention-review-v1/all26-read-cases.json).

| Resident true GLOBAL512 sampled-write tails | A p99 ns | B p99 ns | C p99 ns | Raw C/B p99 | B max ns | C max ns |
|---|---:|---:|---:|---:|---:|---:|
|RAM one record|16,892|11,812|17,844|+51.067%|12,264|37,902|
|RAM batch64|249,003|250,156|251,037|+0.352%|267,298|271,156|
|Disk one record|365,615|536,881|311,733|−41.936%|1,300,924|572,449|
|Disk batch64|2,845,982|441,980|380,914|−13.816%|631,381|617,455|

Positive tail change is worse. RAM one-record C/A global p99 is+5.636%, versus+51.067% against B; maximum is+209.05% versus B and+100.58% versus A. All four B→C process p99s worsen (11,922→17,844;11,772→12,214;11,692→19,457;12,043→14,809ns), so the concern is not restricted to exposed C round0. Mean-trial p99+7.203%, geometric process-pooled p99+33.516% and GLOBAL512 p99 are distinct summaries. Disk batch64≥500µs counts rise3→5 despite improved global p99. [ALL_RAW_TAILS: every arm/backend/process/percentile](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-v1/ALL_RAW_TAILS.md) and [all original512-sample populations and thresholds](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/analysis-v1/global-write-tails.json) preserve every point.

### Constructor work is charged in setup; named costs are not a total wall timer

The unchanged full setup seeds BOTH engines and then calls after_setup; no cache pre-enabling, audit priming, query warmup or benchmark change was introduced. The same-capture fixture-load-both mean increases28.514µs in RAM; adjacent after_setup falls3.299µs, leaving their sum25.215µs more expensive. Fixture loading includes BOTH engines, so it cannot be called isolated Resident constructor cost. The following are arithmetic means of four matched processes, distinct from geometric throughput centers. Each write-case combined value contains128 aggregate +128 sampled commits; it overlaps its component rows and must not be added again.

| Named timer | Unit | RAM B | RAM C | RAM C−B | Disk C−B |
|---|---|---:|---:|---:|---:|
|Fixture load, BOTH engines|µs|9,068.106500|9,096.620500|+28.514000|-9.695750|
|Explicit cache after_setup|µs|4.546000|1.247250|-3.298750|-1.533000|
|Load-both + after_setup|µs|9,072.652500|9,097.867750|+25.215250|-11.228750|
|Resident creation|µs|45,996.516750|46,321.101500|+324.584750|+4,252.675000|
|Resident one-record aggregate|µs|1,959.920500|1,652.510250|-307.410250|-997.847250|
|Resident one-record sampled|µs|1,348.225500|1,381.822000|+33.596500|-1,637.782000|
|Resident one-record combined|µs|3,308.146000|3,034.332250|-273.813750|-2,635.629250|
|Resident batch64 aggregate|µs|8,350.715000|8,438.845250|+88.130250|+266.418500|
|Resident batch64 sampled|µs|8,218.982500|8,286.523750|+67.541250|+211.764000|
|Resident batch64 combined|µs|16,569.697500|16,725.369000|+155.671500|+478.182500|
|Resident BOTH write cases|µs|19,877.843500|19,759.701250|-118.142250|-2,157.446750|
|BOTH engines, BOTH write cases|µs|43,729.130000|43,403.994000|-325.136000|-3,558.746000|
|Whole child, ALL workload|s|1.146117|1.150978|+0.004861|+0.000871|

These are named timer scopes. Whole-child wall includes setup, reads, validation, cold checks and dispatch; the front/write subsets exclude those intervening costs and do not establish a complete write-block wall timer or CPU share. Summing named scopes PER PROCESS before taking centers gives RAM load-both+after_setup+Resident one-record improvement2.008%; including both write cases leaves0.320% improvement; adding Resident creation makes that named sum0.305% MORE expensive. No sum of geometric centers or synthetic net wall-time claim replaces the original timers. [Exact same-population cost ledger](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/independent-result-review-v1/UPFRONT_AND_WRITE_COSTS.md) and [the independently calculated disjoint scope sums](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/retention-review-v1/front-disjoint-sums.json) retain both filesystems and every process.

Million B/C targets are quiet; A rounds1/2 are exposed and remain. B→C load/read/update are−0.869/−2.141/−0.555%, with load/update adverse3/3. Setup is−2.242%, close+open−1.394%;256MiB/two-file storage is unchanged. Reopen remains6.815s versus3.799ms LMDB (about1,794×), setup399.806ms versus1.597ms (about250×), and allocation7.747× LMDB. Full-suite Native cold-reopen comparisons are unavailable, not invented. These gaps remain unresolved.

### Evidence, conservation and decision boundary

The unclocked canonical fixture independently decodes3,111 prefix nodes/1,683 values and retains exactly6,479 original logical reads of cold debt. Physical reads plus6,479 equal the exact original auditor: cache-only14,946=8,467+6,479; all-feature6,554=75+6,479 (the original single-pass KV feature changes the other reads). Live/retired/value/error checks and ordinary admission/validation budgets remain. This proves removed traversal work, not its nanosecond share. Before business C retains2 cache entries/2,368 accounted bytes versus B0/648; the1,720-byte difference is owned cache metadata, not RSS. [Canonical work proof and assertions](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/candidate/crates/ulmdb/src/resident/collect_cache/operation_fixture_tests.rs), [actual58/60 focused source qualification](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/functional-v3-receipt.json), [final source/layout audit](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/independent-final-source-review-v1.json) and [838-test portable qualification](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/build-qualification-v1-receipt.json) bind the frozen source. Existing Entry size/offset and serving-layout conservation do not explain historical read losses.

The complete [independent first39 correctness/provenance/raw/math audit](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/independent-result-review-v1/REPORT.md) and [immutable audit receipt](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/independent-result-review-v1/receipt.json) verify all39 zero-return children,313 pins,551/483 source contexts, all474 comparison cells,103 Native rows (93 numeric/10 unavailable),24 raw files/720 populations and every GLOBAL512 sample. The [closed quantitative HOLD review](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/retention-review-v1/REPORT.md) retains all adverse points. Exposures004 C RAM round0,033 A million round1 and036 A million round2 remain in all centers, tails and paired arrays; [all39 availability contexts](benchmark/work/optimization-20261105/next-prefix-cold-entry/retained6-v1/retention-review-v1/availability39.json) records them. Boundary quiet does not prove continuous isolation. Two unchanged confirmations are the next permitted measurement; stable protected read/tail losses cannot be waived as marginal merely because cold aggregate means improve. No zero-regression, warmed-parity, net-wall or main-integration claim is made.

## Retained iteration 6 directly versus LMDB: current accepted baseline

Positive advantage means ULMDB is better: more throughput or less time/space. A within-2% observed difference is labelled roughly on par, not statistically equivalent. Every row below comes from the same complete iteration-6 primary campaign; separate prototype improvements are not combined into an unmeasured latest version.

# Latest iteration6 candidate versus contemporary LMDB

All39 predeclared processes passed, with source unchanged. Iteration6 is retained after three fixed confirmation blocks, with explicit marginal read tradeoffs and an unresolved disk-tail caveat. Positive advantage means ULMDB is better: more throughput or less time/space. Roughly on par means within2% of the observed centre, not statistical equivalence.

## full: throughput

| Filesystem / metric | ULMDB | LMDB | Unit | ULMDB advantage | Verdict | Improvement still needed for parity |
|---|---:|---:|---|---:|---|---:|
| disk / durable_batch1/throughput | 15,826.385 | 10,740.944 | records/s | +47.35% | outperforming | none at observed centre |
| disk / durable_batch64/throughput | 484,009.522 | 178,231.869 | records/s | +171.56% | outperforming | none at observed centre |
| disk / ipv4_lpm_hits/throughput | 3,188,947.779 | 2,153,763.707 | queries/s | +48.06% | outperforming | none at observed centre |
| disk / ipv4_lpm_inside_gap_misses/throughput | 11,606,004.587 | 2,015,409.314 | queries/s | +475.86% | outperforming | none at observed centre |
| disk / ipv4_lpm_misses/throughput | 21,655,867.562 | 3,879,158.505 | queries/s | +458.26% | outperforming | none at observed centre |
| disk / ipv4_lpm_mixed/throughput | 5,513,007.982 | 2,680,987.971 | queries/s | +105.63% | outperforming | none at observed centre |
| disk / ipv4_lpm_outside_cover_misses/throughput | 100,917,093.269 | 92,286,003.266 | queries/s | +9.35% | outperforming | none at observed centre |
| disk / ipv6_lpm_hits/throughput | 2,468,934.339 | 1,241,271.241 | queries/s | +98.90% | outperforming | none at observed centre |
| disk / ipv6_lpm_inside_gap_misses/throughput | 9,332,632.210 | 1,120,666.198 | queries/s | +732.78% | outperforming | none at observed centre |
| disk / ipv6_lpm_misses/throughput | 17,149,137.566 | 2,211,360.420 | queries/s | +675.50% | outperforming | none at observed centre |
| disk / ipv6_lpm_mixed/throughput | 4,284,398.130 | 1,556,785.448 | queries/s | +175.21% | outperforming | none at observed centre |
| disk / ipv6_lpm_outside_cover_misses/throughput | 97,491,465.295 | 91,485,018.086 | queries/s | +6.57% | outperforming | none at observed centre |
| disk / ordered_kv_hits/throughput | 7,652,712.667 | 4,142,882.498 | queries/s | +84.72% | outperforming | none at observed centre |
| disk / ordered_kv_misses/throughput | 7,708,110.456 | 3,978,779.771 | queries/s | +93.73% | outperforming | none at observed centre |
| disk / ordered_kv_mixed/throughput | 7,330,137.148 | 4,076,748.280 | queries/s | +79.80% | outperforming | none at observed centre |
| ram / durable_batch1/throughput | 62,048.090 | 135,053.416 | records/s | -54.06% | underperforming | 117.659% throughput increase |
| ram / durable_batch64/throughput | 967,351.612 | 739,503.850 | records/s | +30.81% | outperforming | none at observed centre |
| ram / ipv4_lpm_hits/throughput | 3,171,227.450 | 2,100,199.871 | queries/s | +51.00% | outperforming | none at observed centre |
| ram / ipv4_lpm_inside_gap_misses/throughput | 11,551,516.412 | 1,990,090.622 | queries/s | +480.45% | outperforming | none at observed centre |
| ram / ipv4_lpm_misses/throughput | 21,543,521.708 | 3,906,180.676 | queries/s | +451.52% | outperforming | none at observed centre |
| ram / ipv4_lpm_mixed/throughput | 5,492,890.164 | 2,630,134.097 | queries/s | +108.84% | outperforming | none at observed centre |
| ram / ipv4_lpm_outside_cover_misses/throughput | 99,939,993.878 | 94,153,863.056 | queries/s | +6.15% | outperforming | none at observed centre |
| ram / ipv6_lpm_hits/throughput | 2,460,443.319 | 1,233,707.371 | queries/s | +99.43% | outperforming | none at observed centre |
| ram / ipv6_lpm_inside_gap_misses/throughput | 9,300,425.916 | 1,121,263.920 | queries/s | +729.46% | outperforming | none at observed centre |
| ram / ipv6_lpm_misses/throughput | 17,141,013.957 | 2,217,213.033 | queries/s | +673.09% | outperforming | none at observed centre |
| ram / ipv6_lpm_mixed/throughput | 4,271,910.446 | 1,550,202.278 | queries/s | +175.57% | outperforming | none at observed centre |
| ram / ipv6_lpm_outside_cover_misses/throughput | 97,385,014.041 | 94,383,548.932 | queries/s | +3.18% | outperforming | none at observed centre |
| ram / ordered_kv_hits/throughput | 7,469,251.583 | 4,134,826.571 | queries/s | +80.64% | outperforming | none at observed centre |
| ram / ordered_kv_misses/throughput | 7,495,281.183 | 3,977,679.996 | queries/s | +88.43% | outperforming | none at observed centre |
| ram / ordered_kv_mixed/throughput | 7,120,200.893 | 4,052,864.937 | queries/s | +75.68% | outperforming | none at observed centre |

## full: combined_throughput

| Filesystem / metric | ULMDB | LMDB | Unit | ULMDB advantage | Verdict | Improvement still needed for parity |
|---|---:|---:|---|---:|---|---:|
| disk / durable_batch1/combined_throughput | 16,293.845 | 11,178.819 | records/s | +45.76% | outperforming | none at observed centre |
| disk / durable_batch64/combined_throughput | 502,966.995 | 182,622.853 | records/s | +175.41% | outperforming | none at observed centre |
| ram / durable_batch1/combined_throughput | 74,780.392 | 139,287.999 | records/s | -46.31% | underperforming | 86.263% throughput increase |
| ram / durable_batch64/combined_throughput | 978,473.669 | 746,387.366 | records/s | +31.09% | outperforming | none at observed centre |

## millions: throughput

| Filesystem / metric | ULMDB | LMDB | Unit | ULMDB advantage | Verdict | Improvement still needed for parity |
|---|---:|---:|---|---:|---|---:|
| disk / load/throughput | 1,396,939.833 | 1,539,753.386 | operations/s | -9.28% | underperforming | 10.223% throughput increase |
| disk / read/throughput | 2,776,988.785 | 1,660,287.797 | operations/s | +67.26% | outperforming | none at observed centre |
| disk / update/throughput | 145,257.236 | 128,142.683 | operations/s | +13.36% | outperforming | none at observed centre |

## full: sampled_p99

| Filesystem / metric | ULMDB | LMDB | Unit | ULMDB advantage | Verdict | Improvement still needed for parity |
|---|---:|---:|---|---:|---|---:|
| disk / durable_batch1/sampled_p99 | 86,677.988 | 207,656.766 | ns | +58.26% | outperforming | none at observed centre |
| disk / durable_batch64/sampled_p99 | 199,542.954 | 471,235.156 | ns | +57.66% | outperforming | none at observed centre |
| disk / ipv4_lpm_hits/sampled_p99 | 434.071 | 791.319 | ns | +45.15% | outperforming | none at observed centre |
| disk / ipv4_lpm_inside_gap_misses/sampled_p99 | 124.007 | 650.264 | ns | +80.93% | outperforming | none at observed centre |
| disk / ipv4_lpm_misses/sampled_p99 | 123.869 | 635.545 | ns | +80.51% | outperforming | none at observed centre |
| disk / ipv4_lpm_mixed/sampled_p99 | 423.922 | 780.364 | ns | +45.68% | outperforming | none at observed centre |
| disk / ipv4_lpm_outside_cover_misses/sampled_p99 | 41.654 | 45.277 | ns | +8.00% | outperforming | none at observed centre |
| disk / ipv6_lpm_hits/sampled_p99 | 543.671 | 1,258.202 | ns | +56.79% | outperforming | none at observed centre |
| disk / ipv6_lpm_inside_gap_misses/sampled_p99 | 150.328 | 1,136.898 | ns | +86.78% | outperforming | none at observed centre |
| disk / ipv6_lpm_misses/sampled_p99 | 141.990 | 1,071.714 | ns | +86.75% | outperforming | none at observed centre |
| disk / ipv6_lpm_mixed/sampled_p99 | 523.666 | 1,240.211 | ns | +57.78% | outperforming | none at observed centre |
| disk / ipv6_lpm_outside_cover_misses/sampled_p99 | 41.512 | 43.353 | ns | +4.25% | outperforming | none at observed centre |
| disk / ordered_kv_hits/sampled_p99 | 200.477 | 341.532 | ns | +41.30% | outperforming | none at observed centre |
| disk / ordered_kv_misses/sampled_p99 | 198.574 | 344.280 | ns | +42.32% | outperforming | none at observed centre |
| disk / ordered_kv_mixed/sampled_p99 | 204.291 | 343.428 | ns | +40.51% | outperforming | none at observed centre |
| ram / durable_batch1/sampled_p99 | 12,155.147 | 7,936.141 | ns | -53.16% | underperforming | 34.710% cost reduction |
| ram / durable_batch64/sampled_p99 | 104,316.401 | 94,415.200 | ns | -10.49% | underperforming | 9.492% cost reduction |
| ram / ipv4_lpm_hits/sampled_p99 | 452.485 | 797.922 | ns | +43.29% | outperforming | none at observed centre |
| ram / ipv4_lpm_inside_gap_misses/sampled_p99 | 126.155 | 694.249 | ns | +81.83% | outperforming | none at observed centre |
| ram / ipv4_lpm_misses/sampled_p99 | 127.578 | 663.565 | ns | +80.77% | outperforming | none at observed centre |
| ram / ipv4_lpm_mixed/sampled_p99 | 432.510 | 784.729 | ns | +44.88% | outperforming | none at observed centre |
| ram / ipv4_lpm_outside_cover_misses/sampled_p99 | 41.525 | 47.742 | ns | +13.02% | outperforming | none at observed centre |
| ram / ipv6_lpm_hits/sampled_p99 | 560.496 | 1,285.545 | ns | +56.40% | outperforming | none at observed centre |
| ram / ipv6_lpm_inside_gap_misses/sampled_p99 | 152.655 | 1,189.435 | ns | +87.17% | outperforming | none at observed centre |
| ram / ipv6_lpm_misses/sampled_p99 | 148.791 | 1,124.245 | ns | +86.77% | outperforming | none at observed centre |
| ram / ipv6_lpm_mixed/sampled_p99 | 541.908 | 1,258.581 | ns | +56.94% | outperforming | none at observed centre |
| ram / ipv6_lpm_outside_cover_misses/sampled_p99 | 41.525 | 44.733 | ns | +7.17% | outperforming | none at observed centre |
| ram / ordered_kv_hits/sampled_p99 | 211.427 | 353.723 | ns | +40.23% | outperforming | none at observed centre |
| ram / ordered_kv_misses/sampled_p99 | 211.587 | 358.180 | ns | +40.93% | outperforming | none at observed centre |
| ram / ordered_kv_mixed/sampled_p99 | 211.767 | 350.388 | ns | +39.56% | outperforming | none at observed centre |

## full: pooled_sampled_p99

| Filesystem / metric | ULMDB | LMDB | Unit | ULMDB advantage | Verdict | Improvement still needed for parity |
|---|---:|---:|---|---:|---|---:|
| disk / durable_batch1/pooled_sampled_p99 | 311,199.990 | 326,784.331 | ns | +4.77% | outperforming | none at observed centre |
| disk / durable_batch64/pooled_sampled_p99 | 381,838.574 | 522,675.973 | ns | +26.95% | outperforming | none at observed centre |
| ram / durable_batch1/pooled_sampled_p99 | 14,594.410 | 9,030.263 | ns | -61.62% | underperforming | 38.125% cost reduction |
| ram / durable_batch64/pooled_sampled_p99 | 263,297.827 | 105,896.695 | ns | -148.64% | underperforming | 59.781% cost reduction |

## millions: costs

| Filesystem / metric | ULMDB | LMDB | Unit | ULMDB advantage | Verdict | Improvement still needed for parity |
|---|---:|---:|---|---:|---|---:|
| disk / close_reopen_ns | 7,088,011,774.592 | 3,891,514.475 | ns | -182040.19% | underperforming | 99.945% cost reduction |
| disk / setup_ns | 408,682,667.378 | 1,225,252.490 | ns | -33254.98% | underperforming | 99.700% cost reduction |
| disk / storage_after_close_reopen/allocated_bytes | 268,436,821.326 | 34,648,064.000 | bytes | -674.75% | underperforming | 87.093% cost reduction |
| disk / storage_after_close_reopen/apparent_bytes | 268,435,456.000 | 34,652,160.000 | bytes | -674.66% | underperforming | 87.091% cost reduction |
| disk / storage_after_close_reopen/files | 2.000 | 2.000 | files | -0.00% | roughly on par (within 2%) | none at observed centre |
| disk / storage_before_close/allocated_bytes | 268,436,821.326 | 34,648,064.000 | bytes | -674.75% | underperforming | 87.093% cost reduction |
| disk / storage_before_close/apparent_bytes | 268,435,456.000 | 34,652,160.000 | bytes | -674.66% | underperforming | 87.091% cost reduction |
| disk / storage_before_close/files | 2.000 | 2.000 | files | -0.00% | roughly on par (within 2%) | none at observed centre |



## Best retained private candidate: Collect mark deduplication admitted after three cohorts

The published iteration-6 baseline above remains unchanged. Root admitted this isolated archived-source candidate as an explicit marginal tradeoff, with all 117 observations and every adverse result retained. This is not a benchmark or qualification of current main. The complete decision and tables follow.

### Three unchanged Collect mark-deduplication cohorts

**Root decision: ADMIT the frozen private candidate as an explicit marginal tradeoff after all three unchanged cohorts. Current-main integration remains separate and unqualified.** All 117 children passed original oracles. The RAM single-write gain repeats across all three cohorts: +5.334% aggregate and +3.393% combined overall,11/12 favorable pairs. This is an incremental cold-work benefit, not RAM parity or an equivalence/no-regression claim. All adverse read/load/tail/cost observations remain.

Final source 541/snapshot 473 and d57118dd/e944dc73 ELFs never changed between cohorts; qualification7f6d10cc records 814 workspace tests/105 summaries plus focused/fault gates. No rebuild or observer was introduced. All initial and retained6 references remain immutable. Current main has unrelated external work and is not represented by this private archived-source measurement. Root's actual completed v2 inventory (163files,277,843,159B) was captured before v3 and verified byte-for-byte afterward.

#### Core rates and lifecycle costs

Geometric centers. Positive signed improvement is better: higher throughput or lower time/space. Direct Native values retain the exposure qualifications below and never normalize target changes.

| Metric | R6 | Candidate | Signed vsR6 | Native LMDB | Signed vsNative | Native status |
|---|---:|---:|---:|---:|---:|---|
|ram batch1 throughput|62,437.688|65,768.298|+5.334%|134,081.814|-50.949%|slower / less throughput|
|ram batch1 combined_throughput|75,343.957|77,900.042|+3.393%|138,228.266|-43.644%|slower / less throughput|
|ram batch64 throughput|979,040.505|983,270.616|+0.432%|740,573.532|+32.772%|faster / more throughput|
|ram batch64 combined_throughput|989,332.702|991,595.496|+0.229%|748,412.636|+32.493%|faster / more throughput|
|disk batch1 throughput|15,299.531|15,742.656|+2.896%|10,809.704|+45.634%|faster / more throughput|
|disk batch1 combined_throughput|16,017.369|16,395.127|+2.358%|11,465.552|+42.995%|faster / more throughput|
|disk batch64 throughput|473,580.107|492,087.385|+3.908%|177,304.210|+177.538%|faster / more throughput|
|disk batch64 combined_throughput|486,974.082|500,563.802|+2.791%|182,508.903|+174.268%|faster / more throughput|
|Load records/s|1,394,530.650|1,375,326.482|-1.377%|1,587,783.698|-13.381%|slower / less throughput|
|Read queries/s|2,760,740.520|2,756,790.741|-0.143%|1,670,578.020|+65.020%|faster / more throughput|
|Update records/s|144,286.666|144,905.762|+0.429%|128,178.483|+13.050%|faster / more throughput|
|Setup ms|404.678|415.201|-2.600%|1.526|-27114.477%|higher cost|
|Drop+open ms|7,115.558|7,010.834|+1.472%|3.759|-186407.473%|higher cost|
|Allocated MiB|256.000|256.001|-0.000%|33.044|-674.721%|higher cost|
|Apparent MiB|256.000|256.000|-0.000%|33.047|-674.657%|higher cost|

RAM singles remain 50.949% below Native aggregate throughput and 43.644% below Native combined throughput. Load remains 13.381% below Native; read/update are observed 65.020%/13.050% above Native. Setup remains 272.145× Native, drop+open 1,865.075× and allocation 7.747×. Those architecture gaps are not solved. Million centers include exposed points; these are observed comparisons, not qualified causal claims.

#### Cohort repetition and phase scope

| Metric | Cohort1 | Cohort2 | Cohort3 | All3 | Favorable/adverse pairs |
|---|---:|---:|---:|---:|---|
|full/ram/ulmdb/durable_batch1/throughput|+5.569%|+3.965%|+6.484%|+5.334%|11/1|
|full/ram/ulmdb/durable_batch1/combined_throughput|+3.634%|+2.732%|+3.815%|+3.393%|11/1|
|full/ram/ulmdb/durable_batch64/throughput|+0.302%|+0.679%|+0.315%|+0.432%|9/3|
|full/ram/ulmdb/durable_batch64/combined_throughput|+0.142%|+0.348%|+0.196%|+0.229%|8/4|
|millions/disk/ulmdb/load/throughput|-2.023%|-1.203%|-0.902%|-1.377%|1/8|
|millions/disk/ulmdb/read/throughput|-1.287%|-0.171%|+1.043%|-0.143%|4/5|
|millions/disk/ulmdb/update/throughput|+0.935%|+0.527%|-0.171%|+0.429%|7/2|
|millions/disk/ulmdb/setup_ns|-0.624%|+2.394%|+6.143%|+2.600%|3/6|
|millions/disk/ulmdb/close_reopen_ns|-1.002%|-1.059%|-2.349%|-1.472%|9/0|

RAM single sampled-phase throughput is only+.584% (7/12 favorable), versus aggregate+5.334%; batch64 sampled+.022% is mixed. The gain is concentrated in the aggregate phase, consistent with the targeted cold Collect mechanism. No specific Collect nanoseconds are inferred from this uninstrumented workload or from prior failed-gate observers. Original untimed checks retain 14,946 logical reads while eliminating 6,390 duplicate tree-insert attempts; that count is not a speedup estimate. [Every phase and pair](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-three-cohorts-v1/WRITE_PHASES.md).

#### Protected reads and remaining marginal costs

All 13 RAM read centers lie between −.767% and+2.350%; KV hit/miss/mixed are−.214/−.033/−.022%, with mixed pair signs. Disk KV centers are−1.887/−1.581/−1.602%, adverse10/12,9/12,9/12. Disk IPv4 mixed−.702% is adverse11/12. Disk IPv6 outside-cover−2.171% is mixed7/12 adverse (cohorts−3.037/+.049/−3.489); its worst individual pair is−13.432%. No outlier is dropped. The next-largest disk read loss is IPv6 inside-gap−.894%; coherent positive IPv6 hit/mixed rates also remain. [All 26 read vectors and Native context](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-three-cohorts-v1/ALL_READS.md).

Load−1.377% is adverse8/9, including all three points of the fully quiet second cohort (−1.203%). This is a real adverse observation requiring acknowledgement, not equivalence or a dismissed hypothesis. Five of nine overall target-million pairs have exposed endpoints, so all pooled lifecycle/read/load contrasts remain descriptive. Setup cost+2.600% and apparent/allocated capacity essentially unchanged are retained. Observed drop+open−1.472% in all 9 pairs is not credited as an architectural recovery fix.

#### All raw tails

True global 1,536 p50/p95/p99/max and each cohort's512 values are preserved in [raw JSON](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-three-cohorts-v1/global-write-tails-all3.json). Trial-geometric, process-pooled and global p99 are distinct; no summary replaces another. Positive raw tail change is worse.

| FS/batch | R6 global 1,536 p99 ns | Candidate p99 ns | Raw change | R6 max ns | Candidate max ns | Each cohort p99 raw change |
|---|---:|---:|---:|---:|---:|---|
|disk/1|313346|311131|-0.707%|399981|574953|-0.326%, -1.092%, -0.870%|
|disk/64|402104|377187|-6.197%|711002|799350|-14.118%, -0.747%, -3.884%|
|ram/1|14497|13545|-6.567%|22833|34395|-9.999%, -15.262%, -3.486%|
|ram/64|253181|256117|+1.160%|283519|291655|+3.068%, -2.528%, -1.369%|

All four pooled maxima are worse, despite p99 improvements in three of four cells. Disk ≥500µs counts rise 0→2 for singles and 5→9 for batch64; these rare events and all three cohorts' adverse disk maxima remain. The RAM64 global p99 rise+1.160% is also retained. These observations are part of the marginal-tradeoff recommendation, not discarded as noise. [Every p50/p95/p99/max/process vector/sample offset](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-three-cohorts-v1/ALL_RAW_TAILS.md).

#### Availability, integrity and decision boundary

All 117 observations stay: 104quiet/13exposed. Cohort quiet counts 31/39/34. V1 exposures are 001acceptedD0,009acceptedD1,020initialRAM3,026candidateM0,027LMDBM0,028SQLiteM0,032candidateM1,037acceptedM2. V2 has none. V3 exposures are 017candidateRAM2,029LMDBM1,031acceptedM1,033initialM1,037acceptedM2. All exposures retain original commands, return codes, process contexts and raw data. Pre/post boundaries do not prove continuous isolation. No Native normalization occurs.

The independent audit verifies all 72 raw files/2,160 populations,72 cohort-global 512 and24 combined-global 1,536 cells,1,422 separate-cohort comparison rows plus474 combined rows,309 separate Native cells plus103 combined, all 117 original validators, private source/native/SQLite/launch identities,91 original/repeat tool pins plus preserved history, and root's actual v2-before/after-v3 preservation. Empty adjusted-regression/consistent-regression flags do not establish equivalence. Every first-cohort outlier, favorable or adverse, remains.

The measured tradeoff is a repeatable modest RAM benefit with approximately 1–2% protected disk-read/load costs, a 2.600% setup estimate and rare tail costs. There is no persistent large protected loss like the earlier rejected candidates. Under the user's explicit three-repeat marginal-tradeoff policy, admission of this isolated candidate is defensible. It does not meet the RAM-parity goal, certify zero regressions, or authorize overwriting concurrent current-main work. Root admitted the frozen private candidate; ownership-safe current-main integration and qualification remain separate. Accepted assets have not been edited by this analysis.

#### Complete tables

- [All 474 combined signed metrics, pairs and flags](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-three-cohorts-v1/ALL_METRICS.md), [103 combined direct Native targets/status/gaps](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-three-cohorts-v1/report/NATIVE_SCORECARD.md), [original full comparisons](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-three-cohorts-v1/report/ALL_COMPARISONS.md), [all 26 reads](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-three-cohorts-v1/ALL_READS.md).
- [Cohort1 report](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-v1/report/REPORT.md), [all 474 signed cells](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-v1/ALL_METRICS.md), [103 Native cells](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-v1/report/NATIVE_SCORECARD.md).
- [Cohort2 all 474 signed cells](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-v2/ALL_METRICS.md), [103 Native cells](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-v2/report/NATIVE_SCORECARD.md), [all 26 reads](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-v2/ALL_READS.md).
- [Cohort3 all 474 signed cells](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-v3/ALL_METRICS.md), [103 Native cells](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-v3/report/NATIVE_SCORECARD.md), [all 26 reads](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-v3/ALL_READS.md).
- [Every unchanged original report and pair association](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-three-cohorts-v1/aggregation-provenance.json), [root v2 preservation before v3](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/root-preserve-v2-before-v3.json), [verification after v3](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/root-preserve-v2-after-v3.json), [independent all3 audit](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/independent-three-cohort-review-v1/REPORT.md).



[Final immutable analysis receipt](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-three-cohorts-v1/final-receipt.json) and [explicit private admission decision](benchmark/work/optimization-20261105/next-collect-mark-dedup/retained6-v1/analysis-three-cohorts-v1/retention-decision-v1.json).

## Sparse commit diagnosis closed: both studies jointly UNQUALIFIED

No third repetition is authorized. All 28 original processes pass their oracles, source/native/raw bindings and five-interval arithmetic checks. This observer does not establish qualified production costs or a new optimization. P is the admitted archived mark-dedup branch (541 files/d571); O/C use one private observer (542 files/310e), separate from current main.

| Study | Numerical cost-center gates | Availability | Complete attribution |
|---|---|---|---|
| Sparse v1 | 24/24 pass; maximum absolute center +4.230% | Exposed OFF smoke and clock-on round1; 12/14 quiet | UNQUALIFIED |
| Unchanged confirmation v2 | 23/24 pass; Native single aggregate C/P +5.411025% fails the unchanged 5% limit | Exposed OFF round3; 13/14 quiet | UNQUALIFIED |

All individual pairs, including those above 5%, and every exposure remain. No threshold increase, selected-scope rescue, Native normalization or overhead subtraction is used. Quiet boundaries are not continuous isolation.

The following are descriptive instrumented arithmetic microseconds per combined single commit, with 1,024 commits/backend/study. Five adjacent Resident intervals equal the same outer commit envelope; do not add that envelope again.

| Scope | Sparse v1 µs | Confirmation v2 µs |
|---|---:|---:|
|Catalog preparation through freeze|1.465710|1.485821|
|Post-freeze preparation|0.068109|0.069006|
|Live-recipe build|0.952908|0.979197|
|Borrowed Journal call|6.250241|6.234621|
|Post-Journal activation and Write destruction|0.686314|0.692540|
|Resident whole commit|9.423283|9.461186|
|Matched Native whole commit|6.371349|6.361697|

The first interval is not isolated Arena.freeze; the last is not isolated ArcSwap. Journal includes its original preparation/callbacks/fences and local cleanup. Native includes its whole consuming commit, not pure metadata publication. Internal work within these coarse spans stays unsplit.

| Study | Single phase | n/backend | Resident total µs | Native total µs | Resident commit µs | Native commit µs | Charged Collect µs | Resident outside remainder µs | Native outside remainder µs |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
|Sparse v1|aggregate|512|15.465883|7.723906|9.626879|6.492078|4.236885|1.602119|1.231828|
|Sparse v1|latency|512|10.699309|7.307893|9.219687|6.250619|0.000000|1.479621|1.057273|
|Sparse v1|combined|1024|13.082596|7.515899|9.423283|6.371349|2.118442|1.540870|1.144551|
|Confirmation v2|aggregate|512|15.459723|7.756459|9.623945|6.518980|4.238666|1.597111|1.237479|
|Confirmation v2|latency|512|10.762320|7.240869|9.298426|6.204414|0.000000|1.463895|1.036455|
|Confirmation v2|combined|1024|13.111021|7.498664|9.461186|6.361697|2.119333|1.530503|1.136967|

Each phase uses the SAME eight-commit groups: aggregate wall or the sum of its eight raw latency samples. External remainder covers begin/puts/recording and other work outside the measured commit/Collect scopes; it stays unassigned. No CPU fraction, allocation cause or pristine timing is inferred. The four first Collects average 542.321/542.549µs respectively, with all actual calls individually observed (k=1). First-use work remains charged. The format would retain only sum/count for k>1, without inventing individual first/later durations.

- Sparse v1: [report](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v1-presentation-v2/REPORT.md), [all354 metrics](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v1-presentation-v2/METRIC_TABLES.md), [all13 reads and Native controls](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v1-presentation-v2/READ_TABLES.md), [all process/trial tails](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v1-presentation-v2/TAIL_TABLES.md), [true global512 tails](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v1-presentation-v2/GLOBAL_WRITE_TAILS.md), [all paired values](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v1-presentation-v2/METRIC_PAIRS.csv), [all512 phase ledgers and Collect accounting](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v1-presentation-v2/TRANSACTION_ACCOUNTING.md).
- Confirmation v2: [report](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v2/REPORT.md), [all354 metrics](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v2/METRIC_TABLES.md), [all13 reads and Native controls](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v2/READ_TABLES.md), [all process/trial tails](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v2/TAIL_TABLES.md), [true global512 tails](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v2/GLOBAL_WRITE_TAILS.md), [all paired values](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v2/METRIC_PAIRS.csv), [all512 phase ledgers and Collect accounting](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/analysis-v2/TRANSACTION_ACCOUNTING.md).

[Explicit closure/no third repeat](benchmark/work/optimization-20261105/next-sparse-commit-diagnosis/retained6-v1/sparse-closure-v2.json). Original v1 controls/analysis/audits remain byte-exact; presentation was completed in a new directory after an offline formatter error. No data or engine observation was replaced.

**Current-source integration V6 remains HOLD/unqualified** after a baseline-matched native compaction error-case failure. Its three identical private compatibility/test repairs and every failed context are preserved; no live runtime patch or four-reference/A/A/C campaign was executed. Next work is source-only constructor-proof and directory-metadata reuse assessment. Identical Rust read bodies alone do not prove unchanged linked layout; all direct/fused patch capabilities must remain. Earlier rejected constructor variants are not revived by these unqualified profiles.

## Native wall and CPU diagnosis: completed; no qualified optimization

All 33 scheduled original-workload runs passed their oracles, but both wall observers and the hardware sampler failed their complete perturbation gates. No observer overhead is subtracted, no scope is selected to manufacture a pass, and these archived retained6 inputs do not represent current main.

| Experiment | Complete perturbation result |
|---|---|
| Wall v1, 14 processes | FAIL: Native single O/P +5.071% aggregate /+5.235% combined; one pristine exposure retained |
| Wall v2, 14 processes | FAIL: Resident single combined C/P +5.095934% against the unchanged 5% bound; all boundaries quiet |
| Hardware, 5 processes | FAIL: seven of eight cost centers; Resident singles +8.346% aggregate /+8.518% combined |

Descriptive v2 combined single-write wall accounting is Resident13.829745µs versus Native7.541372µs per commit. The observed6.288373µs gap conserves 3.290087µs whole-commit difference +2.622685µs charged first-Collect amortization +0.375602µs external-remainder difference. The Resident commit contains6.493789µs in adjacent Journal intervals and3.200107µs unassigned outside Journal. This is arithmetic of observed scopes, not pristine production shares or a cause assigned to allocation/Space/Snapshot.

Actual hardware samples confirm recipe comparison, encoding, freeze and SHA-NI execution; sparse/variable Native stack coverage prevents a cross-engine CPU ratio. All unknowns remain unassigned. The user later granted private host perf access and root verified it; no global perf policy changed and earlier captures are not rebound by that permission.

[Integrated diagnosis, all wall/CPU tables and independent audits](benchmark/work/optimization-20261105/native-diagnosis-20261106/REPORT.md). All earlier rejected recipe, current-image and transport experiments remain rejected.

## Latest measured candidate: dirty-window recipe REJECTED after complete39

The 541-file/473-entry candidate passes all 39 original oracles and 817 workspace tests/105 summaries. It improves batch64 and observed million updates but gives no coherent main RAM-single gain. Protected RAM KV hits/misses/mixed are slower in all four quiet full-suite pairs; true global sampled tails also remain adverse. Root rejected retention. Retained6 and its accepted assets remain unchanged.

### Retained6-only dirty-window recipe: REJECTED for retention

**Root decision: retain accepted iteration6; do not apply this candidate.** The complete 39-process campaign establishes no coherent main RAM-single gain: aggregate−1.984% and all 256-commit combined+0.560%, with two favorable/two adverse pairs each. RAM KV hits−3.591%, misses−1.936% and mixed−1.998% are adverse in all four quiet full-suite pairs. Batch64 gains and the observed million-update gain do not justify those protected costs and remaining sampled-tail observations. Root assessed the whole result, including the batch/update benefits and protected costs; statistical equivalence is not established.

#### Completeness and exact context

All 39 fresh processes passed original value/semantic/owner/borrow/cold/row denominator/workload/source/ELF/native/SQLite/raw guards. Final qualification records 817 workspace tests/105 summaries, exit0, plus focused9/default64/all-feature95/direct78/fused68/indexed89 gates. Final 541-file inventory and 473-entry snapshot match both release after-maps. Qualified full 7321f7e5… and million a1f44af1… remain exact. All 57 final pins,12 prior capture inventories and 343 historical bindings pass; failed/scoped profile attempts remain unchanged. Root main preservation receipt fd8e6e… confirms77 accepted Resident files; the candidate was not applied to main and unrelated user work is excluded from that claim.

Six byte-identical 0555 launch copies use equal-length A/B/C executable paths. Every fixture uses a unique twelve-character name inside the original filesystem parent. Original process tags/schedule/order/cases/timers/oracles are unchanged. Actual argv/paths, preserved driver copy and fixture mapping are recorded per child. Strict SQLite checks cover actual launch copies and preserved copies. Both actual native TOML/archive/embedded strings match; only the predeclared parsed archive hash differs from retained6.

All 24 full-suite return boundaries observed quiet. Total 35 quiet/four exposed: 024 initial ULMDB million/r0,034 shared SQLite/r2,035 candidate ULMDB million/r2 and036 initial ULMDB million/r2. Every exposure is retained. Consequently million candidate/reference and candidate/native centers include an exposed candidate; initial/reference comparisons also include exposed initial points. These million gains/costs are descriptive observations, not newly qualified performance claims. Retained6 million and native LMDB controls have quiet endpoints. No retry, exclusion, native normalization or continuous host-isolation claim occurs. Exit1 is a completed performance screen, not an engine failure.

Offline primary verification recalculates both 237-row comparisons and audits all 24 raw files/172,898,304 bytes,11,544 spans,21,612,288 samples and1,536 write spans, complete coverage/hashes, read/timer p50/p95/p99, write p99, global pools/maxima and128+128=256 commit conservation. Every original resource schema remains; there is no seed/proof namespace, report projection or altered timer.

#### Core rates and lifecycle costs

Raw C/LMDB percent is candidate/native−1 for every unit. Higher throughput is better; lower time/space is better. Within 2% is a descriptive roughly on-par label, not equivalence. All million rows below include the exposed candidate observation.

| Metric | Retained6 | Candidate | C vs6 | LMDB | Raw C vsLMDB | Native status |
|---|---:|---:|---:|---:|---:|---|
|ram batch1 throughput|61,696.751|60,472.655|-1.984%|130,073.073|-53.509%|slower / less throughput|
|ram batch1 combined_throughput|74,859.074|75,278.356|+0.560%|135,828.334|-44.578%|slower / less throughput|
|ram batch64 throughput|982,358.526|1,021,035.258|+3.937%|742,260.279|+37.558%|faster / more throughput|
|ram batch64 combined_throughput|991,542.012|1,035,367.126|+4.420%|749,609.239|+38.121%|faster / more throughput|
|disk batch1 throughput|15,229.373|15,301.149|+0.471%|10,465.947|+46.199%|faster / more throughput|
|disk batch1 combined_throughput|15,971.709|15,592.701|-2.373%|11,202.083|+39.195%|faster / more throughput|
|disk batch64 throughput|469,196.611|500,859.024|+6.748%|176,967.205|+183.024%|faster / more throughput|
|disk batch64 combined_throughput|490,682.608|511,005.662|+4.142%|182,165.388|+180.517%|faster / more throughput|
|Million load records/s|1,382,041.992|1,391,271.900|+0.668%|1,606,562.270|-13.401%|slower / less throughput|
|Million read queries/s|2,692,369.614|2,767,525.573|+2.791%|1,668,000.730|+65.919%|faster / more throughput|
|Million update records/s|143,615.743|152,243.508|+6.008%|128,502.165|+18.475%|faster / more throughput|
|Setup ms|411.177|411.534|+0.087%|1.325|+30952.297%|higher cost|
|Drop+open ms|7,137.641|6,894.618|-3.405%|3.661|+188214.323%|higher cost|
|Allocated MiB|256.001|256.000|-0.001%|33.044|+674.718%|higher cost|
|Apparent MiB|256.000|256.000|+0.000%|33.047|+674.657%|higher cost|
|Files|2.000|2.000|+0.000%|2.000|+0.000%|roughly on par (observed within2%; not equivalence)|

The candidate remains 53.509% below native RAM-single aggregate (115.094% more throughput needed), and 44.578% below on all combined commits. Observed loading remains 13.401% below native. Setup is 310.523×, drop+open 1883.143× and allocation 7.747× native. The persistent/recovery architecture and fixed256MiB capacity are unchanged; observed reopen−3.405% is not an architectural reopen fix.

#### All 13 protected reads per filesystem and every paired rate sign

| FS/case | Rate C vs6 | All 4 rate changes | Trial p99 C vs6 | C vsLMDB throughput | Native status |
|---|---:|---|---:|---:|---|
|disk/ipv4_lpm_hits|-1.146%|-0.948%, -0.928%, -0.795%, -1.907%|+2.576%|+53.485%|faster / more throughput|
|disk/ipv4_lpm_inside_gap_misses|-0.756%|-0.512%, -0.730%, -0.148%, -1.629%|-1.471%|+514.250%|faster / more throughput|
|disk/ipv4_lpm_misses|-1.707%|-1.093%, -1.125%, -1.680%, -2.918%|-3.964%|+468.402%|faster / more throughput|
|disk/ipv4_lpm_mixed|-0.794%|-0.096%, -0.408%, -0.857%, -1.808%|+1.679%|+115.074%|faster / more throughput|
|disk/ipv4_lpm_outside_cover_misses|-1.001%|-3.418%, -0.377%, -0.119%, -0.048%|+0.966%|+4.385%|faster / more throughput|
|disk/ipv6_lpm_hits|+1.362%|+2.207%, +0.866%, +2.419%, -0.023%|+2.400%|+103.431%|faster / more throughput|
|disk/ipv6_lpm_inside_gap_misses|+0.164%|-0.770%, -0.155%, -0.460%, +2.065%|+1.888%|+737.998%|faster / more throughput|
|disk/ipv6_lpm_misses|+0.874%|+0.986%, +0.472%, +0.477%, +1.563%|+4.566%|+688.503%|faster / more throughput|
|disk/ipv6_lpm_mixed|+1.462%|+2.214%, +1.515%, +1.717%, +0.409%|+2.235%|+183.009%|faster / more throughput|
|disk/ipv6_lpm_outside_cover_misses|-0.431%|-0.172%, +3.501%, +0.059%, -4.932%|+0.716%|+2.141%|faster / more throughput|
|disk/ordered_kv_hits|-1.045%|-0.243%, -4.285%, -1.467%, +1.917%|+4.134%|+81.301%|faster / more throughput|
|disk/ordered_kv_misses|-1.147%|-0.891%, -5.569%, +0.248%, +1.779%|+3.055%|+89.073%|faster / more throughput|
|disk/ordered_kv_mixed|-1.225%|-0.801%, -5.055%, -1.537%, +2.644%|+3.432%|+76.889%|faster / more throughput|
|ram/ipv4_lpm_hits|-1.184%|-0.600%, -0.221%, -2.986%, -0.904%|+1.040%|+53.727%|faster / more throughput|
|ram/ipv4_lpm_inside_gap_misses|-1.722%|-0.078%, -0.641%, -5.177%, -0.907%|-1.144%|+502.674%|faster / more throughput|
|ram/ipv4_lpm_misses|-1.283%|+0.178%, -1.683%, -2.895%, -0.706%|-3.254%|+475.094%|faster / more throughput|
|ram/ipv4_lpm_mixed|-0.898%|-0.866%, +0.627%, -2.809%, -0.514%|+1.986%|+114.277%|faster / more throughput|
|ram/ipv4_lpm_outside_cover_misses|-2.460%|+0.048%, -4.169%, -5.652%, +0.066%|-0.340%|+13.898%|faster / more throughput|
|ram/ipv6_lpm_hits|+1.606%|+2.340%, +2.024%, -0.328%, +2.415%|+1.614%|+103.192%|faster / more throughput|
|ram/ipv6_lpm_inside_gap_misses|-0.055%|-0.940%, +1.262%, -2.902%, +2.445%|+1.344%|+737.201%|faster / more throughput|
|ram/ipv6_lpm_misses|+0.532%|+0.759%, +0.667%, -0.300%, +1.006%|+0.165%|+685.382%|faster / more throughput|
|ram/ipv6_lpm_mixed|+1.831%|+2.800%, +3.004%, -0.196%, +1.749%|+1.513%|+183.358%|faster / more throughput|
|ram/ipv6_lpm_outside_cover_misses|-0.292%|-0.099%, +0.044%, -5.574%, +4.731%|+1.884%|+10.826%|faster / more throughput|
|ram/ordered_kv_hits|-3.591%|-3.846%, -0.908%, -5.006%, -4.553%|+5.153%|+81.951%|faster / more throughput|
|ram/ordered_kv_misses|-1.936%|-0.487%, -0.821%, -4.814%, -1.561%|+4.948%|+90.412%|faster / more throughput|
|ram/ordered_kv_mixed|-1.998%|-1.891%, -0.691%, -4.968%, -0.374%|+2.861%|+76.988%|faster / more throughput|

RAM KV hits/misses/mixed lose 3.591/1.936/1.998%, each adverse in all four quiet process pairs. Disk KV hits/misses/mixed lose 1.045/1.147/1.225% with mixed signs. RAM IPv4 outside-cover misses−2.460% also remains (mixed signs). No adjusted regression interval for these cells establishes equivalence; no flag is interpreted as proof of no cost. All other read improvements and losses remain in the table and103-target scorecard.

#### All write means, trial tails and process-pooled tails

| FS/batch/metric | C vs6 | All 4 changes |
|---|---:|---|
|ram/1/throughput|-1.984%|+2.804%, +0.942%, -6.847%, -4.520%|
|ram/1/combined_throughput|+0.560%|+4.297%, +2.965%, -3.186%, -1.644%|
|ram/1/sampled_p99|-4.619%|-8.884%, -7.239%, +2.527%, -4.488%|
|ram/1/pooled_sampled_p99|-13.798%|-31.079%, -10.449%, +6.714%, -16.164%|
|ram/64/throughput|+3.937%|+5.962%, +6.745%, +0.140%, +3.033%|
|ram/64/combined_throughput|+4.420%|+6.477%, +6.487%, +1.267%, +3.542%|
|ram/64/sampled_p99|-3.722%|-5.648%, -5.175%, -1.760%, -2.243%|
|ram/64/pooled_sampled_p99|+1.032%|-2.323%, -8.379%, +8.026%, +7.777%|
|disk/1/throughput|+0.471%|+4.887%, +0.137%, -4.451%, +1.537%|
|disk/1/combined_throughput|-2.373%|-6.075%, -1.862%, -2.301%, +0.873%|
|disk/1/sampled_p99|+10.498%|+40.623%, +10.274%, -3.117%, -0.772%|
|disk/1/pooled_sampled_p99|+0.327%|+0.433%, +2.141%, -1.229%, -0.006%|
|disk/64/throughput|+6.748%|+2.444%, +17.882%, +4.802%, +2.598%|
|disk/64/combined_throughput|+4.142%|+3.978%, +9.932%, +1.560%, +1.323%|
|disk/64/sampled_p99|+0.192%|-14.066%, +6.143%, +15.354%, -4.228%|
|disk/64/pooled_sampled_p99|-0.333%|-39.795%, +34.405%, +23.499%, -1.261%|

Trial-p99 geometric mean, process-pooled-p99 geometric mean and true global 512 p99 are different statistics. None is a production percentile. Every sampled value, maximum and source offset remains.

| FS/batch | Global 512 p99 R→C ns | Global change | Maximum R→C ns | Candidate native global p99 ns | Process p99 R | Process p99 C |
|---|---:|---:|---:|---:|---|---|
|disk/1|311242→313677|+0.782%|324056→580013|325499|[309919, 309859, 320249, 313696]|[311262, 316492, 316312, 313677]|
|disk/64|376346→404168|+7.393%|627303→631450|524878|[618085, 374232, 372809, 370425]|[372118, 502987, 460415, 365755]|
|ram/1|12724→13425|+5.509%|18455→19227|8666|[16471, 12173, 11632, 17162]|[11352, 10901, 12413, 14388]|
|ram/64|248252→266597|+7.390%|275504→288969|110370|[247069, 270114, 246789, 248252]|[241329, 247480, 266597, 267558]|

RAM-single trial-p99−4.619% and process-pooled−13.798% coexist with global p99+5.509%; RAM64 trial−3.722% coexists with global+7.390%. Disk64 global rises 7.393%, while process-pooled geometric change is−0.333%. Disk-single trial p99+10.498% and process-pooled+0.327% coexist with global+0.782%. Its maximum rises 324.056→580.013µs (+78.985%). Every full return boundary is quiet, so these sampled observations cannot be dismissed by the recorded million exposures. Different summaries are retained together, with no favorable statistic substituted for another.

#### Million pairs and limited charged lifecycle sum

| Metric | C vs6 | All 3 changes |
|---|---:|---|
|load/throughput|+0.668%|+1.729%, +2.008%, -1.691%|
|read/throughput|+2.791%|+3.411%, +4.217%, +0.779%|
|update/throughput|+6.008%|+5.775%, +5.833%, +6.415%|
|setup_ns|+0.087%|+3.798%, -6.121%, +2.890%|
|close_reopen_ns|-3.405%|-3.609%, -3.717%, -2.887%|
|storage_after_close_reopen/allocated_bytes|-0.001%|+0.000%, -0.002%, +0.000%|

Limited lifecycle task centers: retained6 15.236003s, candidate 14.593385s, native 8.409615s. C cost vs6 -4.218%; direct native +73.532%. These include one exposed candidate and do not justify protected read costs.

Observed update+6.008% is favorable in all three pairs; read+2.791% is also favorable in all three. Loading+0.668% is mixed. All remain visible despite the exposed candidate/r2. Setup+0.087% is mixed, not equivalent. Reopen−3.405% is favorable in all three, but unchanged full-history recovery and allocation are not relabelled architectural improvements.

#### Mechanism, decision and next source assessment

The focused128-commit/384-page fixture proves that trusted private dirty windows reduce comparison operand coverage from 1,572,864 to 9,216 bytes per operand, without fallback and with widest window 48. That is a source-operation count, not physical bandwidth, elapsed time or a native speedup. Main RAM-single and load goals did not gain coherently. The batch/update observations are insufficient to offset repeated protected read costs and sampled-tail evidence; root rejects retention. No confirmation campaign is planned for this rejected candidate.

Retained6 stays accepted. The next work is source assessment of an optional recovery-local slot-position sidecar on clean retained6. It must remain a validated locator hint with original fallback/errors, allocation/budget admission, full history/retirement audits and serving read representation unchanged. No current-image format, source implementation or measured recovery gain is claimed by this result.

[All 103 native targets/status/gaps](benchmark/work/optimization-20261105/next-dirty-window/retained6-v1/analysis-v1/report/NATIVE_SCORECARD.md), [all 474 unfiltered comparison cells](benchmark/work/optimization-20261105/next-dirty-window/retained6-v1/analysis-v1/report/ALL_COMPARISONS.md), [every raw/source/native/history check](benchmark/work/optimization-20261105/next-dirty-window/retained6-v1/analysis-v1/review-receipt.json), [all raw write values, top offsets and source trials](benchmark/work/optimization-20261105/next-dirty-window/retained6-v1/analysis-v1/raw-review.json), [global sampled pools/maxima](benchmark/work/optimization-20261105/next-dirty-window/retained6-v1/analysis-v1/global-write-tails.json), [actual availability identities](benchmark/work/optimization-20261105/next-dirty-window/retained6-v1/analysis-v1/availability-review.json), [original namespace/resource confirmation](benchmark/work/optimization-20261105/next-dirty-window/retained6-v1/analysis-v1/namespace-review.json).


[Independent actual raw/source/native/math audit](benchmark/work/optimization-20261105/next-dirty-window/retained6-v1/independent-campaign-result-review-v1/REPORT.md) agrees with all 103 direct-native targets and both 237-row comparisons. All failed-profile history, exposures and rejected candidates remain.

**Next source assessment:** an optional recovery-local slot-position sidecar on a [clean retained6 copy](benchmark/work/optimization-20261105/next-replay-slot-index/retained6-v1/retained6-copy-v1.json). It is a validated locator hint with original scan fallback, generation/queue/back-reference checks and scratch/OOM admission preserving otherwise-admitted recovery. Full history replay, retirement audits, normal Pool/State/Free/Version/View and serving read bodies stay. There is no current-image format or predicted gain. Root review precedes builds; qualification and the unchanged full39 precede any retention decision.


## Completed RAM-write instruction diagnosis: qualified update-edge scope only

The actual saved R6 and new seed-ON captures qualify only exact engine-update inclusive call edges. Full-phase isolation failed and remains failed. Both modes retain 16 outside-leak parts and 31 raw summary discrepancies unassigned; every raw dump stays. The complete report, all guest-event groups, phase signs, raw quantiles and provenance follow. These are no native timing or CPU-share claims.

### R6 versus seed-ON: actual engine-update call-edge work

**Scope PASS: ENGINE_UPDATE_EDGE_ONLY. Full-phase isolation remains FAILED. All wall timings are UNQUALIFIED.**

One saved actual v3 R6 observation and one new ON observation passed every original workload/value/old-reader/cold/resource/raw/source/native/mode check. No R6 engine rerun occurred. Each has128 ordered phase-trigger dumps and eight completed updates per dump, plus one retained outside termination dump. Both observed quiet endpoints; boundary checks do not prove continuous host isolation.

The only numerical engine-work numerator is the inclusive cost of the exact direct phase→update call edges, with the original backend, caller PC, trial, batch and aggregate/sampled order. Each updater begins a write, applies the supplied records and commits; automatic Collect is inside that update. Trace generation, validation and outer sample clocks do not call those updaters. The [scope proof](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/amendment-v5/UPDATE_EDGE_PROOF_V5.md) and actual eight-edge gate bound that interval.

**Both captures retain16 whole-dump outside-leak parts and31 summary/exclusive discrepancies, unassigned.** Raw summaries, exact totals/self costs, outside helper costs and terminal costs remain available. Nothing is subtracted or normalized into engine work. The original phase-only failures remain failures.

#### Confirmed processing difference and next priority

Both modes execute11 Collect methods in the same update phases. The first is part62 (batch1, aggregate). Its source-bounded Collect edge falls from6,414,378 to2,608,653 guest instructions:3,805,725 fewer (−59.33%). The actual ON observation consumes two genesis prefix closures once, skipping6479 logical prefix reads with zero actual prefix reads for that consuming Collect. Later ten Collect calls remain about2.55–2.59 million instructions each. These historical constructor counters are not lifetime/reopen I/O totals.

All256 aggregate+sampled single-write updates fall from27,075,231 to23,279,251 guest instructions (−14.02%); batch64 falls only0.033%. Separate whole phases containing no Collect retain about81,015 R6 versus81,054 ON instructions/commit for aggregate singles. The one cold traversal is confirmed avoidable processing; it does not remove the steady write work.

The next bounded priority is the existing commit path, especially source-bounded Journal/publication children. Independent direct updater edges put aggregate-single ULMDB commit near75.9k guest instructions/commit, versus about8.23k for native commit; ULMDB put is about4.49k versus native16.96k. This is evidence to inspect commit rather than rewriting the successful mutation/read path. The separate [qualified five-boundary Journal control](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/analysis-v1/REPORT.md) measured publication5,079.746ns inside its four recorded intervals; that experiment does not split publication syscalls/hashing or turn current instruction counts into native time.

**Mandatory crypto/runtime limitation:** Valgrind changes CPUID/guest ISA exposure, may select software SHA instead of host-accelerated SHA, and replaces/selects libc routines differently. Its guest instruction counts cannot establish native SHA CPU shares, native crypto cost or a speed projection. Kernel I/O and durability fence time are omitted. Branch-miss events are a software predictor model, not PMU measurements. No optimization or retention claim follows from a large guest SHA count.

#### All charged update work: aggregate, sampled and combined

Every aggregate/sampled row contains128 commits; combined contains256. The direct local Native columns are raw references, never normalization. Positive guest-Ir percentages mean more instructions, not slower native execution.

| Mode | Batch | Kind | Commits | ULMDB Ir/commit | Local Native Ir/commit | U vs Native guest Ir | U vs R6 guest Ir |
|---|---:|---|---:|---:|---:|---:|---:|
| R6 | 1 | aggregate | 128 | 131,155.188 | 25,769.562 | +408.954% | +0.000% |
| R6 | 1 | sampled | 128 | 80,370.055 | 25,781.117 | +211.740% | +0.000% |
| R6 | 1 | combined | 256 | 105,762.621 | 25,775.340 | +310.325% | +0.000% |
| R6 | 64 | aggregate | 128 | 815,900.375 | 335,130.195 | +143.458% | +0.000% |
| R6 | 64 | sampled | 128 | 713,407.359 | 336,228.961 | +112.179% | +0.000% |
| R6 | 64 | combined | 256 | 764,653.867 | 335,679.578 | +127.793% | +0.000% |
| ON | 1 | aggregate | 128 | 101,460.891 | 25,768.305 | +293.743% | -22.641% |
| ON | 1 | sampled | 128 | 80,408.258 | 25,780.367 | +211.897% | +0.048% |
| ON | 1 | combined | 256 | 90,934.574 | 25,774.336 | +252.811% | -14.020% |
| ON | 64 | aggregate | 128 | 815,563.797 | 335,199.578 | +143.307% | -0.041% |
| ON | 64 | sampled | 128 | 713,237.312 | 336,303.016 | +112.082% | -0.024% |
| ON | 64 | combined | 256 | 764,400.555 | 335,751.297 | +127.669% | -0.033% |

#### Separate phases with zero Collect calls

This is a conditional subset of entire eight-update phases, with the corresponding native trial/kind selected. It is displayed alongside all-work totals, never subtracted from or substituted for them. Seven ordinary updates sharing a phase with one Collect remain in the all-work total and are absent from this subset.

| Mode | Batch | Kind | Selected phases | Commits | ULMDB Ir/commit | Matching Native Ir/commit |
|---|---:|---|---:|---:|---:|---:|
| R6 | 1 | aggregate | 15 | 120 | 81,015.017 | 25,771.367 |
| R6 | 1 | sampled | 16 | 128 | 80,370.055 | 25,781.117 |
| R6 | 1 | combined | 31 | 248 | 80,682.133 | 25,776.399 |
| R6 | 64 | aggregate | 12 | 96 | 734,264.969 | 334,074.396 |
| R6 | 64 | sampled | 10 | 80 | 593,276.488 | 335,506.562 |
| R6 | 64 | combined | 22 | 176 | 670,179.295 | 334,725.381 |
| ON | 1 | aggregate | 15 | 120 | 81,053.508 | 25,770.133 |
| ON | 1 | sampled | 16 | 128 | 80,408.258 | 25,780.367 |
| ON | 1 | combined | 31 | 248 | 80,720.476 | 25,775.415 |
| ON | 64 | aggregate | 12 | 96 | 733,926.250 | 334,122.927 |
| ON | 64 | sampled | 10 | 80 | 592,989.488 | 335,593.463 |
| ON | 64 | combined | 22 | 176 | 669,864.085 | 334,791.352 |

#### Every Collect call retained

Counts below are the exact method call edges associated with update phases, not whole-part self costs. First/later labels follow actual part order.

| Mode | Part | Batch | Kind | Calls | First cold | Collect inclusive Ir | All8 update inclusive Ir |
|---|---:|---:|---|---:|---|---:|---:|
| ON | 62 | 1 | aggregate | 1 | True | 2,608,653 | 3,260,573 |
| ON | 72 | 64 | sampled | 1 | False | 2,584,746 | 7,281,137 |
| ON | 78 | 64 | aggregate | 1 | False | 2,579,936 | 8,458,174 |
| ON | 83 | 64 | sampled | 1 | False | 2,558,065 | 7,257,246 |
| ON | 89 | 64 | aggregate | 1 | False | 2,586,632 | 8,584,852 |
| ON | 96 | 64 | sampled | 1 | False | 2,545,356 | 7,326,287 |
| ON | 102 | 64 | aggregate | 1 | False | 2,554,942 | 8,396,095 |
| ON | 107 | 64 | sampled | 1 | False | 2,586,988 | 7,322,240 |
| ON | 113 | 64 | aggregate | 1 | False | 2,576,122 | 8,496,125 |
| ON | 120 | 64 | sampled | 1 | False | 2,573,923 | 7,383,722 |
| ON | 128 | 64 | sampled | 1 | False | 2,578,926 | 7,284,585 |
| R6 | 62 | 1 | aggregate | 1 | True | 6,414,378 | 7,066,062 |
| R6 | 72 | 64 | sampled | 1 | False | 2,576,139 | 7,272,310 |
| R6 | 78 | 64 | aggregate | 1 | False | 2,579,898 | 8,465,859 |
| R6 | 83 | 64 | sampled | 1 | False | 2,558,013 | 7,259,202 |
| R6 | 89 | 64 | aggregate | 1 | False | 2,586,576 | 8,584,681 |
| R6 | 96 | 64 | sampled | 1 | False | 2,545,292 | 7,326,224 |
| R6 | 102 | 64 | aggregate | 1 | False | 2,555,156 | 8,399,873 |
| R6 | 107 | 64 | sampled | 1 | False | 2,586,889 | 7,327,542 |
| R6 | 113 | 64 | aggregate | 1 | False | 2,576,128 | 8,495,398 |
| R6 | 120 | 64 | sampled | 1 | False | 2,573,721 | 7,384,412 |
| R6 | 128 | 64 | sampled | 1 | False | 2,578,928 | 7,284,333 |

#### All paired phase signs, including adverse counts

These are deterministic-work observations within one run per ELF, not independent timing replicates or confidence intervals. Lower/higher refers only to ON guest Ir relative to R6.

| Backend | Batch | Kind | Lower | Equal | Higher |
|---|---:|---|---:|---:|---:|
| ulmdb | 1 | aggregate | 1 | 0 | 15 |
| ulmdb | 1 | sampled | 0 | 0 | 16 |
| ulmdb | 64 | aggregate | 8 | 0 | 8 |
| ulmdb | 64 | sampled | 6 | 0 | 10 |
| lmdb | 1 | aggregate | 10 | 1 | 5 |
| lmdb | 1 | sampled | 10 | 1 | 5 |
| lmdb | 64 | aggregate | 3 | 0 | 13 |
| lmdb | 64 | sampled | 3 | 0 | 13 |

#### Audit and full artifacts

The offline audit verified all258 Callgrind files,256 selected update phases,962 raw latency populations (14,408,192 bytes) with exact coverage and p50/p95/p99/max, all global128-sample write quantiles and256-commit conservation. All23+14+37 protocol pins,269 historical bindings, nine historical capture inventories, both failed capture139-file inventories and both retained six-file fixtures match. The frozen source remains548 inventory files/480 captured source entries, with469 archival R6 source entries. Actual ELF, embedded native metadata, mode counters and all original oracles match; the only parsed native context difference is the already declared archive hash.

The original R6 child wall67.7939569989685s is retained with its scope. The never-registered original outer duration remains null. Those tool elapsed values and every benchmark ns/p99 value are unqualified performance data under Valgrind. Raw timing tails are fully retained in CSV, not used as performance evidence.

- [Audit JSON](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/audit.json)
- [All24 update groups, every guest event](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/ALL_UPDATE_GROUPS.csv)
- [All12 local-Native/R6 instruction comparisons](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/ALL_12_GUEST_NATIVE_AND_R6_COMPARISONS.csv)
- [All256 phase update counts](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/ALL_256_UPDATE_PHASES.csv)
- [All128 R6/ON paired phase changes](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/ALL_128_R6_ON_PHASE_COMPARISONS.csv)
- [All22 Collect calls](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/ALL_22_COLLECT_EDGES.csv)
- [All24 zero-Collect conditional groups](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/ALL_ZERO_COLLECT_PHASE_SUBSETS.csv)
- [Matching Native zero-Collect subsets](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/MATCHED_ZERO_COLLECT_PHASE_SUBSETS.csv)
- [All962 raw timing populations, unqualified](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/ALL_962_RAW_POPULATIONS_UNQUALIFIED_TIMINGS.csv)
- [True global raw sampled write quantiles, unqualified](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/GLOBAL_WRITE_RAW_QUANTILES_UNQUALIFIED_TIMINGS.csv)
- [Availability and original/reused context](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/availability.json)
- [All outside/discrepancy parts, unassigned](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/outside_scope_UNASSIGNED.json)
- [Cold oracles and unqualified clocks](benchmark/work/optimization-20261105/next-ram-write-callgrind/retained6-on-v1/analysis-v5/complete-v2/cold_oracles_UNQUALIFIED_TIMINGS.json)

Retained6 remains accepted. The seed-ON production candidate remains rejected; this scoped diagnostic neither repairs historical read regressions nor qualifies retention, reopen, setup, allocation, CPU shares or wall performance. No engine, runtime, benchmark workload, accepted scorecard, chart or README was changed by this analysis.

A first offline report helper incorrectly expected the terminal descriptions list to contain only its Trigger; the ordinary cache/Timerange descriptions caused an assertion. That source/failure is preserved in complete-v1; complete-v2 selects the Trigger field exactly as the frozen checker does. No captured file, event, observation or engine was changed or rerun.


**Subsequent measured implementation: retained6-only dirty-window recipe, rejected.** Source inspection confirms that `RecipeBuilder::committed` compares three committed 4096-byte pages per canonical transaction, while checked private patch operations already know their change bounds. The candidate records those bounds for the trusted private path and keeps full comparison for unsupported/raw operations. Public before-image identity remains strict; original `supports_patch_many` and `try_patch_many` dispatch and all read/audit/durability paths must remain. Its complete standard39 above is REJECTED for retention: source comparison savings did not produce a coherent main RAM-single gain, while protected reads and sampled tails worsened. It combines none of the rejected seed, seek, scanner, O_DSYNC or current-image variants.

[Recipe construction source](benchmark/work/optimization-20261105/next-dirty-window/retained6-v1/candidate/crates/ulmdb-storage/src/resident/wal_codec_v2/borrowed.rs), [private Resident store](benchmark/work/optimization-20261105/next-dirty-window/retained6-v1/candidate/crates/ulmdb-storage/src/resident/store.rs) were separately reviewed, qualified and sealed before portable timing. The unchanged standard39 completed all 24 full plus 15 million processes (original initial, retained6, candidate and contemporaneous LMDB/SQLite). All 13 reads, both write batches, all 256-commit charged totals, raw tails, setup/reopen/allocation costs, native/source/SQLite identities and resource exposures remain protected. All observations remain separate from accepted6.


## Completed read diagnostics: limited placement effect; old large gap not reproduced

The fixed32 placement control observes a small same-dispatch disk association (P0 versus P16 KV +1.81–2.30%), with the exposed final P16 run kept. RAM changes are small/mixed; there is no large RAM restoration or PMU/cache cause claim. The subsequent unchanged8 R6/OFF check measures only −0.695%/−0.743%/−0.950% KV hit/miss/mixed differences, with mixed signs and one exposed R6 reference kept. This does not prove equivalence or erase either adverse historical39. It also does not establish a current 10% read cost for stall profiling to explain. No runtime is promoted.

### Word-leaf placement32: small disk sensitivity; no large RAM read recovery

**Diagnostic conclusion:** P0 versus P16, with the same five-byte dispatch structure, has disk KV hits +2.298%, misses+2.022% and mixed+1.812%, favorable in all four paired observations. The final disk P16 process 031 is exposed to an observed compiler job and remains; all three other paired directions are also favorable. Those full centers retain the exposure and are descriptive, not a qualified hardware-cause or general speedup claim. RAM same-dispatch changes are smaller and mixed. None of the three placements produces a coherent roughly 10% RAM restoration. No equivalence, PMU/cache cause or production retention follows.

#### Integrity and causal scope

All 32 original full-suite semantic/value/point/miss/prefix/cursor/owner/retained-borrow/fresh-reopen oracles pass. Source 548/snapshot 480, 42 current pins and 201 historical bindings/seven previous capture inventories remain exact. Actual static/ABI/CFI qualification binds 2,380,680 normal comparisons, 626,832 malformed-geometry comparisons, 8 forced unwind paths, 225 instruction-PC/CFA checks and all five RIP-relative targets; 15 author and 24 independent negative mutations reject. These qualify the diagnostic bytes, not production acceptance. All 32 reports retain explicit null seed observations before and after.

Thirty-one immediate return boundaries are quiet. Only 031 disk/P16/block3 observes external cc work. Every comparison using that disk P16 observation retains the context; no point is retried, excluded or divided by native. All RAM observations and disk REF/P0/P32 observations have quiet boundaries. Boundaries are not continuous isolation or proof of actual overlap timing.

REF is the original seed-OFF diagnostic. Each placement adds the same entry JMP, retains all 75 instructions and absolute branch/data/cold targets with five operand fixups, and replaces only the declared unused FDE program. Other code/headers/symbols/index/native bytes remain unchanged as qualified. Comparisons with REF include adding the dispatch jump; only comparisons between P0/P16/P32 hold that structure constant. Normal ASLR remains. The copied source/build-id fields are unchanged; distinct full-file SHA identifies every patched diagnostic. Source/function names at clone PCs retain the old Builder label, so explicit PC maps are authoritative.

Independent offline verification reproduces all five 204-cell comparisons (1020 complete cells) and all 32 raw SHA values: 230,531,072 bytes, 15,392 spans, 28,816,384 samples and 2,048 write spans. Every timer/read p50/p95/p99, write p99, aggregate+sampled 256-commit sum, resource schema, denominator, complete byte extent, global sampled pool and maximum passes. Stored reports remain exact; only the required null namespace and raw locator are projected in temporary audit copies. Comparison capture documents are analysis extractions using original workload metadata, not a fresh host survey; actual process order, times and qualification come from the preserved captures-v1 manifest, reports and availability records.

#### Decisive KV centers, every paired sign and native context

|Comparison|FS/case|Reference queries/s|Placement queries/s|Change|All 4 changes|Native change|
|---|---|---:|---:|---:|---|---:|
|REF→P0|disk/ordered_kv_hits|7,394,832.228|7,521,772.489|+1.717%|+0.885%, -0.600%, +2.564%, +4.079%|-0.213%|
|REF→P0|disk/ordered_kv_misses|7,447,386.012|7,529,361.178|+1.101%|+1.614%, -2.030%, +2.268%, +2.620%|-0.344%|
|REF→P0|disk/ordered_kv_mixed|7,103,026.305|7,185,012.496|+1.154%|+1.421%, -1.819%, +2.783%, +2.297%|-0.198%|
|REF→P0|ram/ordered_kv_hits|7,430,805.063|7,441,484.997|+0.144%|-0.355%, +2.099%, +0.262%, -1.398%|+0.312%|
|REF→P0|ram/ordered_kv_misses|7,394,033.791|7,441,042.665|+0.636%|+0.421%, +4.453%, -0.133%, -2.086%|-0.177%|
|REF→P0|ram/ordered_kv_mixed|7,073,630.058|7,124,146.619|+0.714%|+0.191%, +3.771%, +0.161%, -1.200%|+0.437%|
|REF→P16|disk/ordered_kv_hits|7,394,832.228|7,352,787.672|-0.569%|-0.922%, -2.086%, +0.163%, +0.594%|+0.101%|
|REF→P16|disk/ordered_kv_misses|7,447,386.012|7,380,109.560|-0.903%|-0.245%, -2.962%, +0.357%, -0.731%|-0.157%|
|REF→P16|disk/ordered_kv_mixed|7,103,026.305|7,057,135.703|-0.646%|+0.212%, -2.682%, +0.234%, -0.319%|-0.047%|
|REF→P16|ram/ordered_kv_hits|7,430,805.063|7, 421,393.082|-0.127%|+2.264%, -0.440%, -1.110%, -1.181%|+0.170%|
|REF→P16|ram/ordered_kv_misses|7,394,033.791|7,478,483.851|+1.142%|+2.971%, +1.424%, -0.020%, +0.222%|+0.023%|
|REF→P16|ram/ordered_kv_mixed|7,073,630.058|7,128,542.440|+0.776%|+2.361%, +1.372%, -0.272%, -0.330%|+0.401%|
|REF→P32|disk/ordered_kv_hits|7,394,832.228|7,464,867.198|+0.947%|+0.463%, +1.531%, +0.053%, +1.752%|-0.270%|
|REF→P32|disk/ordered_kv_misses|7,447,386.012|7,498,577.733|+0.687%|+0.510%, +0.420%, +1.050%, +0.771%|-0.263%|
|REF→P32|disk/ordered_kv_mixed|7,103,026.305|7,158,273.242|+0.778%|+1.104%, +0.274%, +0.769%, +0.965%|-0.245%|
|REF→P32|ram/ordered_kv_hits|7,430,805.063|7,480,967.992|+0.675%|+4.303%, +0.568%, +0.326%, -2.384%|-0.125%|
|REF→P32|ram/ordered_kv_misses|7,394,033.791|7,494,869.355|+1.364%|+3.519%, +2.113%, +0.668%, -0.794%|-0.075%|
|REF→P32|ram/ordered_kv_mixed|7,073,630.058|7,151,881.400|+1.106%|+3.374%, +1.728%, +0.365%, -0.991%|+0.482%|
|P16→P0|disk/ordered_kv_hits|7,352,787.672|7,521,772.489|+2.298%|+1.824%, +1.518%, +2.397%, +3.465%|-0.314%|
|P16→P0|disk/ordered_kv_misses|7,380,109.560|7,529,361.178|+2.022%|+1.864%, +0.961%, +1.904%, +3.376%|-0.187%|
|P16→P0|disk/ordered_kv_mixed|7,057,135.703|7,185,012.496|+1.812%|+1.206%, +0.886%, +2.543%, +2.625%|-0.151%|
|P16→P0|ram/ordered_kv_hits|7, 421,393.082|7,441,484.997|+0.271%|-2.561%, +2.550%, +1.388%, -0.220%|+0.141%|
|P16→P0|ram/ordered_kv_misses|7,478,483.851|7,441,042.665|-0.501%|-2.477%, +2.987%, -0.113%, -2.303%|-0.200%|
|P16→P0|ram/ordered_kv_mixed|7,128,542.440|7,124,146.619|-0.062%|-2.119%, +2.366%, +0.434%, -0.873%|+0.036%|
|P16→P32|disk/ordered_kv_hits|7,352,787.672|7,464,867.198|+1.524%|+1.399%, +3.694%, -0.110%, +1.151%|-0.371%|
|P16→P32|disk/ordered_kv_misses|7,380,109.560|7,498,577.733|+1.605%|+0.757%, +3.486%, +0.690%, +1.513%|-0.107%|
|P16→P32|disk/ordered_kv_mixed|7,057,135.703|7,158,273.242|+1.433%|+0.890%, +3.038%, +0.534%, +1.289%|-0.198%|
|P16→P32|ram/ordered_kv_hits|7, 421,393.082|7,480,967.992|+0.803%|+1.994%, +1.013%, +1.452%, -1.218%|-0.295%|
|P16→P32|ram/ordered_kv_misses|7,478,483.851|7,494,869.355|+0.219%|+0.532%, +0.680%, +0.688%, -1.013%|-0.098%|
|P16→P32|ram/ordered_kv_mixed|7,128,542.440|7,151,881.400|+0.327%|+0.990%, +0.351%, +0.639%, -0.663%|+0.080%|

Same-dispatch disk P32 versus P16 improves misses 1.605% and mixed 1.433% in all four pairs; hits 1.524% includes one −0.110% point. Same-dispatch RAM P0 versus P16 is hits+0.271%/misses−0.501%/mixed−0.062%; P32 is +0.803%/+0.219%/+0.327%, with mixed directions. No subset center or native normalization replaces any result. Empty adjusted-interval flags do not establish equal performance.

#### Every read case and sampled p99: all comparisons

##### REF→P0

REF has no added JMP; this comparison combines dispatch/placement. All selected arm boundaries are quiet.

|FS/case|Rate change|All 4 rate changes|Trial p99 change|All 4 p99 changes|Native rate change|
|---|---:|---|---:|---|---:|
|disk/ipv4_lpm_hits|+0.610%|+1.049%, -0.527%, +1.551%, +0.381%|-0.057%|+0.119%, +2.207%, -2.561%, +0.064%|+0.336%|
|disk/ipv4_lpm_inside_gap_misses|+0.573%|-0.866%, +1.647%, +1.772%, -0.236%|-0.211%|-2.273%, -0.386%, +1.352%, +0.498%|-1.522%|
|disk/ipv4_lpm_misses|+0.651%|-0.263%, -0.450%, +3.019%, +0.336%|+0.120%|-0.396%, +0.654%, -0.447%, +0.677%|-1.397%|
|disk/ipv4_lpm_mixed|+0.596%|+0.807%, -0.626%, +1.532%, +0.684%|+0.093%|+2.054%, +0.700%, -1.517%, -0.826%|-0.977%|
|disk/ipv4_lpm_outside_cover_misses|-2.333%|-0.083%, -4.163%, -5.423%, +0.469%|-0.310%|+0.000%, +0.000%, -1.233%, +0.000%|-3.762%|
|disk/ipv6_lpm_hits|-0.166%|+0.169%, -0.459%, -0.412%, +0.040%|+1.348%|+0.090%, +2.850%, +0.946%, +1.524%|-1.274%|
|disk/ipv6_lpm_inside_gap_misses|+0.501%|-0.530%, +0.714%, +1.284%, +0.543%|+0.152%|+2.852%, +0.569%, -1.198%, -1.555%|-0.172%|
|disk/ipv6_lpm_misses|+0.415%|+1.115%, -1.702%, +2.820%, -0.515%|+0.473%|+3.334%, -0.178%, +1.213%, -2.390%|+0.198%|
|disk/ipv6_lpm_mixed|+0.113%|-0.085%, -0.098%, -0.032%, +0.667%|-0.568%|-0.128%, -0.116%, -0.479%, -1.540%|-0.611%|
|disk/ipv6_lpm_outside_cover_misses|-3.662%|-14.360%, -0.253%, -0.012%, +0.845%|-0.031%|-1.355%, +0.000%, +0.000%, +1.248%|-0.541%|
|disk/ordered_kv_hits|+1.717%|+0.885%, -0.600%, +2.564%, +4.079%|-0.238%|+0.031%, +1.494%, -0.444%, -2.004%|-0.213%|
|disk/ordered_kv_misses|+1.101%|+1.614%, -2.030%, +2.268%, +2.620%|-0.206%|+1.062%, +0.557%, -0.677%, -1.744%|-0.344%|
|disk/ordered_kv_mixed|+1.154%|+1.421%, -1.819%, +2.783%, +2.297%|-1.262%|+1.245%, +0.536%, -3.045%, -3.689%|-0.198%|
|ram/ipv4_lpm_hits|-0.350%|-0.121%, +0.332%, -0.867%, -0.738%|+1.078%|-2.565%, -1.693%, +7.916%, +0.982%|+0.820%|
|ram/ipv4_lpm_inside_gap_misses|-0.364%|-0.484%, -0.545%, +0.411%, -0.833%|+1.331%|-0.884%, +0.000%, +3.366%, +2.910%|-1.196%|
|ram/ipv4_lpm_misses|+0.251%|+3.075%, -0.517%, -1.333%, -0.167%|+0.419%|-0.547%, -1.850%, +4.118%, +0.052%|-2.451%|
|ram/ipv4_lpm_mixed|-0.188%|+0.485%, +0.351%, -0.700%, -0.879%|+0.625%|-1.345%, -1.151%, +4.137%, +0.952%|-0.042%|
|ram/ipv4_lpm_outside_cover_misses|+1.338%|+4.257%, +0.257%, -0.372%, +1.270%|-0.310%|+0.000%, -1.233%, -1.355%, +1.373%|+0.986%|
|ram/ipv6_lpm_hits|-0.315%|+0.843%, -0.591%, -0.578%, -0.927%|+1.682%|-0.807%, +0.476%, +5.525%, +1.643%|+0.186%|
|ram/ipv6_lpm_inside_gap_misses|-0.310%|+0.646%, -0.068%, -1.428%, -0.379%|-0.397%|-3.148%, +0.044%, +0.358%, +1.213%|-0.242%|
|ram/ipv6_lpm_misses|-0.392%|+0.592%, -0.837%, +0.442%, -1.747%|+1.089%|-2.083%, +1.176%, +2.266%, +3.072%|-0.312%|
|ram/ipv6_lpm_mixed|-0.531%|+0.981%, -1.109%, -0.979%, -1.003%|+2.138%|-0.114%, -0.356%, +6.430%, +2.738%|+0.033%|
|ram/ipv6_lpm_outside_cover_misses|+2.611%|+0.885%, +9.810%, +0.034%, +0.037%|-0.649%|+0.000%, -1.233%, +0.000%, -1.355%|+2.007%|
|ram/ordered_kv_hits|+0.144%|-0.355%, +2.099%, +0.262%, -1.398%|+1.560%|+7.041%, -0.578%, +0.274%, -0.304%|+0.312%|
|ram/ordered_kv_misses|+0.636%|+0.421%, +4.453%, -0.133%, -2.086%|+0.125%|+4.633%, -4.292%, +1.519%, -1.143%|-0.177%|
|ram/ordered_kv_mixed|+0.714%|+0.191%, +3.771%, +0.161%, -1.200%|+0.436%|+6.067%, -2.703%, -0.313%, -1.089%|+0.437%|
##### REF→P16

REF has no added JMP; this comparison combines dispatch/placement. Disk P16 block3 exposure stays.

|FS/case|Rate change|All 4 rate changes|Trial p99 change|All 4 p99 changes|Native rate change|
|---|---:|---|---:|---|---:|
|disk/ipv4_lpm_hits|-0.087%|+0.001%, -1.381%, +1.267%, -0.219%|+0.978%|+1.334%, +1.728%, -0.351%, +1.215%|+0.985%|
|disk/ipv4_lpm_inside_gap_misses|+0.098%|-0.796%, -1.060%, +1.761%, +0.512%|-0.361%|-1.334%, -0.386%, +1.247%, -0.952%|-2.291%|
|disk/ipv4_lpm_misses|+0.627%|+0.312%, -0.611%, +3.920%, -1.040%|+0.048%|-1.014%, +1.013%, -0.107%, +0.312%|-0.959%|
|disk/ipv4_lpm_mixed|+0.015%|+0.556%, -1.008%, +1.037%, -0.513%|+0.255%|+0.011%, +0.659%, -0.341%, +0.694%|-0.109%|
|disk/ipv4_lpm_outside_cover_misses|-0.021%|-0.056%, -0.007%, +0.097%, -0.119%|+1.543%|+0.000%, +2.766%, -2.571%, +6.184%|+17.093%|
|disk/ipv6_lpm_hits|-0.220%|-0.122%, -0.659%, -0.198%, +0.101%|+1.075%|+2.270%, +2.669%, -0.597%, -0.004%|-1.030%|
|disk/ipv6_lpm_inside_gap_misses|-0.873%|-0.431%, -3.652%, -0.216%, +0.863%|+0.698%|+1.619%, +0.988%, -0.222%, +0.416%|+0.147%|
|disk/ipv6_lpm_misses|-0.247%|+0.492%, -1.350%, +1.007%, -1.115%|-0.615%|+0.946%, -0.355%, +0.682%, -3.662%|+0.424%|
|disk/ipv6_lpm_mixed|-0.326%|-0.226%, -0.542%, -0.656%, +0.123%|+0.521%|+1.826%, -0.358%, +0.987%, -0.354%|-0.348%|
|disk/ipv6_lpm_outside_cover_misses|+0.255%|-0.102%, -0.398%, +0.001%, +1.531%|+0.000%|-1.355%, -1.233%, +1.373%, +1.248%|+13.942%|
|disk/ordered_kv_hits|-0.569%|-0.922%, -2.086%, +0.163%, +0.594%|+1.427%|-0.045%, +4.900%, -0.304%, +1.243%|+0.101%|
|disk/ordered_kv_misses|-0.903%|-0.245%, -2.962%, +0.357%, -0.731%|+0.389%|-1.377%, +3.779%, -0.311%, -0.456%|-0.157%|
|disk/ordered_kv_mixed|-0.646%|+0.212%, -2.682%, +0.234%, -0.319%|+0.411%|+0.856%, +3.066%, -0.576%, -1.642%|-0.047%|
|ram/ipv4_lpm_hits|+0.097%|+0.883%, -0.476%, +0.063%, -0.075%|-0.413%|-3.384%, +1.668%, +0.443%, -0.309%|+1.153%|
|ram/ipv4_lpm_inside_gap_misses|-1.201%|-0.511%, -1.718%, -1.522%, -1.048%|-0.232%|-1.375%, +1.878%, -1.395%, +0.000%|+0.313%|
|ram/ipv4_lpm_misses|+0.387%|+1.581%, -0.077%, +0.785%, -0.728%|+1.496%|-0.547%, +7.830%, -1.054%, +0.007%|-0.269%|
|ram/ipv4_lpm_mixed|-0.162%|+1.061%, -1.648%, -0.139%, +0.096%|-0.378%|-2.462%, +0.549%, +0.168%, +0.265%|+1.136%|
|ram/ipv4_lpm_outside_cover_misses|-3.121%|+4.221%, -16.526%, -0.020%, +1.274%|-0.340%|+1.248%, -1.233%, -1.355%, +0.000%|+2.505%|
|ram/ipv6_lpm_hits|+0.027%|+1.080%, -0.038%, -0.306%, -0.619%|+0.463%|-1.989%, +0.700%, +2.248%, +0.940%|+0.112%|
|ram/ipv6_lpm_inside_gap_misses|+0.230%|+0.988%, -1.572%, +1.686%, -0.151%|+0.136%|-1.580%, +0.898%, +0.037%, +1.213%|-0.312%|
|ram/ipv6_lpm_misses|-0.396%|-0.451%, -1.236%, +0.107%, +0.003%|-0.983%|-2.659%, +1.086%, -2.311%, +0.000%|-0.258%|
|ram/ipv6_lpm_mixed|-0.248%|+1.391%, -1.309%, -0.271%, -0.782%|+0.700%|-1.675%, +1.432%, +1.578%, +1.502%|+0.283%|
|ram/ipv6_lpm_outside_cover_misses|+3.521%|+0.997%, +13.829%, +0.050%, -0.152%|-0.618%|+0.000%, +0.124%, -1.233%, -1.355%|+0.199%|
|ram/ordered_kv_hits|-0.127%|+2.264%, -0.440%, -1.110%, -1.181%|+0.466%|+0.409%, +1.460%, +0.305%, -0.304%|+0.170%|
|ram/ordered_kv_misses|+1.142%|+2.971%, +1.424%, -0.020%, +0.222%|-1.035%|-1.000%, -2.182%, +0.077%, -1.020%|+0.023%|
|ram/ordered_kv_mixed|+0.776%|+2.361%, +1.372%, -0.272%, -0.330%|-0.561%|-1.947%, +0.549%, +0.510%, -1.330%|+0.401%|
##### REF→P32

REF has no added JMP; this comparison combines dispatch/placement. All selected arm boundaries are quiet.

|FS/case|Rate change|All 4 rate changes|Trial p99 change|All 4 p99 changes|Native rate change|
|---|---:|---|---:|---|---:|
|disk/ipv4_lpm_hits|+0.459%|+0.512%, +0.608%, +0.725%, -0.009%|+0.801%|+1.232%, +0.118%, +1.372%, +0.486%|+0.718%|
|disk/ipv4_lpm_inside_gap_misses|-0.027%|-1.737%, -0.075%, +1.752%, -0.017%|+0.021%|-2.323%, -1.334%, +3.848%, +0.000%|-2.710%|
|disk/ipv4_lpm_misses|+0.804%|-0.122%, -0.090%, +3.053%, +0.408%|+0.779%|-1.342%, +1.013%, +2.812%, +0.677%|-2.429%|
|disk/ipv4_lpm_mixed|+0.520%|+0.385%, +0.398%, +1.047%, +0.250%|+0.418%|-0.198%, +0.195%, +1.173%, +0.507%|-0.674%|
|disk/ipv4_lpm_outside_cover_misses|+0.123%|-0.032%, -0.038%, +0.127%, +0.436%|+0.000%|+0.000%, +0.000%, -1.355%, +1.373%|+17.841%|
|disk/ipv6_lpm_hits|-0.165%|-0.374%, -0.285%, +0.057%, -0.059%|+1.668%|+0.838%, +2.631%, +1.575%, +1.636%|-0.930%|
|disk/ipv6_lpm_inside_gap_misses|+0.124%|-0.614%, +0.870%, +0.253%, -0.006%|+0.148%|+0.401%, +0.000%, -0.089%, +0.283%|-0.055%|
|disk/ipv6_lpm_misses|+0.027%|+0.540%, -0.312%, +0.203%, -0.320%|-0.896%|-0.220%, -0.986%, +2.195%, -4.459%|+0.482%|
|disk/ipv6_lpm_mixed|+0.098%|+0.194%, -0.290%, +0.084%, +0.407%|+0.469%|+0.121%, +1.817%, +0.916%, -0.959%|-0.178%|
|disk/ipv6_lpm_outside_cover_misses|+0.239%|+0.083%, +0.018%, -0.048%, +0.904%|-0.340%|-1.355%, -1.233%, -1.355%, +2.639%|+14.004%|
|disk/ordered_kv_hits|+0.947%|+0.463%, +1.531%, +0.053%, +1.752%|+1.787%|+3.679%, +0.324%, +2.210%, +0.968%|-0.270%|
|disk/ordered_kv_misses|+0.687%|+0.510%, +0.420%, +1.050%, +0.771%|+1.302%|+4.098%, -0.555%, +1.919%, -0.184%|-0.263%|
|disk/ordered_kv_mixed|+0.778%|+1.104%, +0.274%, +0.769%, +0.965%|-0.106%|+3.723%, -0.576%, -1.021%, -2.447%|-0.245%|
|ram/ipv4_lpm_hits|-0.217%|+0.944%, -1.096%, -0.205%, -0.500%|-0.583%|-2.730%, -0.019%, -0.957%, +1.419%|-0.134%|
|ram/ipv4_lpm_inside_gap_misses|+0.041%|+0.696%, -0.497%, +0.770%, -0.796%|+0.965%|-1.375%, +5.486%, -1.446%, +1.352%|+0.284%|
|ram/ipv4_lpm_misses|+1.206%|+3.252%, -0.146%, +1.413%, +0.340%|-0.625%|-0.547%, -0.585%, +0.260%, -1.617%|-0.122%|
|ram/ipv4_lpm_mixed|+0.065%|+1.550%, -0.085%, -0.597%, -0.594%|+0.057%|-1.504%, +0.782%, -0.073%, +1.042%|+0.228%|
|ram/ipv4_lpm_outside_cover_misses|+0.531%|+4.281%, +0.175%, +0.068%, -2.288%|+0.031%|+1.373%, +0.124%, -1.355%, +0.000%|-3.886%|
|ram/ipv6_lpm_hits|+0.162%|+1.436%, +0.234%, -0.199%, -0.807%|-0.531%|-3.235%, +0.346%, +0.355%, +0.461%|+1.207%|
|ram/ipv6_lpm_inside_gap_misses|+0.952%|-0.268%, +1.606%, +1.957%, +0.530%|+0.116%|-2.759%, +2.304%, -0.008%, +0.996%|+0.425%|
|ram/ipv6_lpm_misses|-0.942%|-1.797%, -0.401%, -0.587%, -0.977%|-0.915%|-2.659%, -0.151%, -2.311%, +1.520%|+0.530%|
|ram/ipv6_lpm_mixed|+0.220%|+1.447%, -0.448%, +0.405%, -0.510%|-0.167%|-1.551%, -0.481%, -0.599%, +1.997%|+1.058%|
|ram/ipv6_lpm_outside_cover_misses|+3.547%|+0.744%, +14.001%, +0.081%, +0.016%|+0.031%|+0.000%, +0.000%, +0.124%, +0.000%|-3.859%|
|ram/ordered_kv_hits|+0.675%|+4.303%, +0.568%, +0.326%, -2.384%|-0.010%|+0.758%, -1.176%, -0.273%, +0.665%|-0.125%|
|ram/ordered_kv_misses|+1.364%|+3.519%, +2.113%, +0.668%, -0.794%|-1.198%|-1.199%, -2.662%, +0.014%, -0.927%|-0.075%|
|ram/ordered_kv_mixed|+1.106%|+3.374%, +1.728%, +0.365%, -0.991%|-1.524%|-2.274%, -2.701%, +0.539%, -1.629%|+0.482%|
##### P16→P0

Same JMP structure; disk P16 block3 exposure stays.

|FS/case|Rate change|All 4 rate changes|Trial p99 change|All 4 p99 changes|Native rate change|
|---|---:|---|---:|---|---:|
|disk/ipv4_lpm_hits|+0.698%|+1.047%, +0.866%, +0.281%, +0.601%|-1.025%|-1.199%, +0.470%, -2.217%, -1.137%|-0.642%|
|disk/ipv4_lpm_inside_gap_misses|+0.474%|-0.070%, +2.735%, +0.011%, -0.744%|+0.150%|-0.952%, +0.000%, +0.104%, +1.463%|+0.787%|
|disk/ipv4_lpm_misses|+0.024%|-0.574%, +0.162%, -0.867%, +1.390%|+0.072%|+0.624%, -0.355%, -0.340%, +0.364%|-0.442%|
|disk/ipv4_lpm_mixed|+0.581%|+0.249%, +0.386%, +0.490%, +1.203%|-0.161%|+2.043%, +0.041%, -1.180%, -1.509%|-0.869%|
|disk/ipv4_lpm_outside_cover_misses|-2.312%|-0.027%, -4.156%, -5.515%, +0.589%|-1.824%|+0.000%, -2.691%, +1.373%, -5.824%|-17.811%|
|disk/ipv6_lpm_hits|+0.054%|+0.291%, +0.201%, -0.214%, -0.061%|+0.270%|-2.132%, +0.177%, +1.553%, +1.528%|-0.247%|
|disk/ipv6_lpm_inside_gap_misses|+1.386%|-0.099%, +4.532%, +1.503%, -0.317%|-0.543%|+1.213%, -0.415%, -0.978%, -1.964%|-0.318%|
|disk/ipv6_lpm_misses|+0.663%|+0.620%, -0.358%, +1.794%, +0.607%|+1.094%|+2.366%, +0.178%, +0.527%, +1.321%|-0.224%|
|disk/ipv6_lpm_mixed|+0.440%|+0.142%, +0.447%, +0.628%, +0.543%|-1.083%|-1.920%, +0.243%, -1.452%, -1.190%|-0.264%|
|disk/ipv6_lpm_outside_cover_misses|-3.908%|-14.273%, +0.145%, -0.013%, -0.676%|-0.031%|+0.000%, +1.248%, -1.355%, +0.000%|-12.710%|
|disk/ordered_kv_hits|+2.298%|+1.824%, +1.518%, +2.397%, +3.465%|-1.642%|+0.077%, -3.246%, -0.140%, -3.207%|-0.314%|
|disk/ordered_kv_misses|+2.022%|+1.864%, +0.961%, +1.904%, +3.376%|-0.593%|+2.473%, -3.104%, -0.367%, -1.294%|-0.187%|
|disk/ordered_kv_mixed|+1.812%|+1.206%, +0.886%, +2.543%, +2.625%|-1.666%|+0.386%, -2.454%, -2.484%, -2.081%|-0.151%|
|ram/ipv4_lpm_hits|-0.447%|-0.994%, +0.812%, -0.929%, -0.664%|+1.497%|+0.848%, -3.306%, +7.440%, +1.295%|-0.329%|
|ram/ipv4_lpm_inside_gap_misses|+0.847%|+0.027%, +1.193%, +1.964%, +0.217%|+1.567%|+0.498%, -1.843%, +4.828%, +2.910%|-1.505%|
|ram/ipv4_lpm_misses|-0.135%|+1.471%, -0.441%, -2.101%, +0.566%|-1.061%|+0.000%, -8.978%, +5.228%, +0.044%|-2.188%|
|ram/ipv4_lpm_mixed|-0.025%|-0.571%, +2.033%, -0.561%, -0.974%|+1.006%|+1.146%, -1.691%, +3.963%, +0.686%|-1.165%|
|ram/ipv4_lpm_outside_cover_misses|+4.602%|+0.034%, +20.107%, -0.353%, -0.004%|+0.031%|-1.233%, +0.000%, +0.000%, +1.373%|-1.482%|
|ram/ipv6_lpm_hits|-0.342%|-0.234%, -0.553%, -0.273%, -0.310%|+1.214%|+1.206%, -0.222%, +3.205%, +0.697%|+0.074%|
|ram/ipv6_lpm_inside_gap_misses|-0.539%|-0.339%, +1.527%, -3.063%, -0.228%|-0.532%|-1.593%, -0.846%, +0.321%, +0.000%|+0.071%|
|ram/ipv6_lpm_misses|+0.004%|+1.047%, +0.405%, +0.335%, -1.750%|+2.093%|+0.592%, +0.089%, +4.685%, +3.072%|-0.054%|
|ram/ipv6_lpm_mixed|-0.284%|-0.404%, +0.203%, -0.710%, -0.223%|+1.429%|+1.587%, -1.763%, +4.777%, +1.219%|-0.249%|
|ram/ipv6_lpm_outside_cover_misses|-0.879%|-0.111%, -3.531%, -0.016%, +0.189%|-0.031%|+0.000%, -1.355%, +1.248%, +0.000%|+1.805%|
|ram/ordered_kv_hits|+0.271%|-2.561%, +2.550%, +1.388%, -0.220%|+1.090%|+6.605%, -2.009%, -0.031%, +0.000%|+0.141%|
|ram/ordered_kv_misses|-0.501%|-2.477%, +2.987%, -0.113%, -2.303%|+1.172%|+5.690%, -2.157%, +1.441%, -0.125%|-0.200%|
|ram/ordered_kv_mixed|-0.062%|-2.119%, +2.366%, +0.434%, -0.873%|+1.003%|+8.173%, -3.235%, -0.818%, +0.244%|+0.036%|
##### P16→P32

Same JMP structure; disk P16 block3 exposure stays.

|FS/case|Rate change|All 4 rate changes|Trial p99 change|All 4 p99 changes|Native rate change|
|---|---:|---|---:|---|---:|
|disk/ipv4_lpm_hits|+0.546%|+0.511%, +2.017%, -0.535%, +0.210%|-0.176%|-0.101%, -1.583%, +1.729%, -0.720%|-0.265%|
|disk/ipv4_lpm_inside_gap_misses|-0.124%|-0.948%, +0.995%, -0.009%, -0.526%|+0.383%|-1.003%, -0.952%, +2.570%, +0.961%|-0.429%|
|disk/ipv4_lpm_misses|+0.176%|-0.433%, +0.525%, -0.835%, +1.462%|+0.731%|-0.331%, +0.000%, +2.922%, +0.364%|-1.485%|
|disk/ipv4_lpm_mixed|+0.505%|-0.170%, +1.421%, +0.010%, +0.767%|+0.163%|-0.209%, -0.461%, +1.519%, -0.185%|-0.566%|
|disk/ipv4_lpm_outside_cover_misses|+0.145%|+0.024%, -0.031%, +0.030%, +0.556%|-1.519%|+0.000%, -2.691%, +1.248%, -4.530%|+0.639%|
|disk/ipv6_lpm_hits|+0.055%|-0.252%, +0.376%, +0.256%, -0.159%|+0.587%|-1.401%, -0.036%, +2.185%, +1.640%|+0.101%|
|disk/ipv6_lpm_inside_gap_misses|+1.006%|-0.184%, +4.693%, +0.470%, -0.862%|-0.546%|-1.198%, -0.978%, +0.134%, -0.133%|-0.202%|
|disk/ipv6_lpm_misses|+0.274%|+0.048%, +1.051%, -0.797%, +0.804%|-0.283%|-1.154%, -0.633%, +1.503%, -0.827%|+0.058%|
|disk/ipv6_lpm_mixed|+0.425%|+0.421%, +0.253%, +0.745%, +0.283%|-0.052%|-1.675%, +2.183%, -0.071%, -0.608%|+0.170%|
|disk/ipv6_lpm_outside_cover_misses|-0.017%|+0.185%, +0.417%, -0.049%, -0.618%|-0.340%|+0.000%, +0.000%, -2.691%, +1.373%|+0.055%|
|disk/ordered_kv_hits|+1.524%|+1.399%, +3.694%, -0.110%, +1.151%|+0.355%|+3.726%, -4.362%, +2.522%, -0.272%|-0.371%|
|disk/ordered_kv_misses|+1.605%|+0.757%, +3.486%, +0.690%, +1.513%|+0.910%|+5.552%, -4.176%, +2.237%, +0.273%|-0.107%|
|disk/ordered_kv_mixed|+1.433%|+0.890%, +3.038%, +0.534%, +1.289%|-0.515%|+2.843%, -3.533%, -0.448%, -0.818%|-0.198%|
|ram/ipv4_lpm_hits|-0.314%|+0.061%, -0.623%, -0.268%, -0.426%|-0.171%|+0.678%, -1.660%, -1.394%, +1.733%|-1.273%|
|ram/ipv4_lpm_inside_gap_misses|+1.257%|+1.214%, +1.242%, +2.327%, +0.255%|+1.200%|+0.000%, +3.542%, -0.052%, +1.352%|-0.029%|
|ram/ipv4_lpm_misses|+0.817%|+1.645%, -0.069%, +0.623%, +1.076%|-2.089%|+0.000%, -7.804%, +1.328%, -1.624%|+0.147%|
|ram/ipv4_lpm_mixed|+0.228%|+0.484%, +1.590%, -0.459%, -0.689%|+0.436%|+0.982%, +0.231%, -0.241%, +0.775%|-0.897%|
|ram/ipv4_lpm_outside_cover_misses|+3.770%|+0.057%, +20.008%, +0.087%, -3.517%|+0.373%|+0.124%, +1.373%, +0.000%, +0.000%|-6.235%|
|ram/ipv6_lpm_hits|+0.135%|+0.352%, +0.272%, +0.107%, -0.190%|-0.989%|-1.271%, -0.352%, -1.851%, -0.475%|+1.094%|
|ram/ipv6_lpm_inside_gap_misses|+0.721%|-1.244%, +3.229%, +0.266%, +0.682%|-0.020%|-1.198%, +1.393%, -0.044%, -0.214%|+0.740%|
|ram/ipv6_lpm_misses|-0.549%|-1.352%, +0.845%, -0.693%, -0.980%|+0.069%|+0.000%, -1.223%, +0.000%, +1.520%|+0.791%|
|ram/ipv6_lpm_mixed|+0.469%|+0.055%, +0.872%, +0.678%, +0.274%|-0.861%|+0.126%, -1.886%, -2.143%, +0.488%|+0.772%|
|ram/ipv6_lpm_outside_cover_misses|+0.025%|-0.251%, +0.151%, +0.030%, +0.169%|+0.653%|+0.000%, -0.124%, +1.373%, +1.373%|-4.050%|
|ram/ordered_kv_hits|+0.803%|+1.994%, +1.013%, +1.452%, -1.218%|-0.473%|+0.347%, -2.598%, -0.577%, +0.972%|-0.295%|
|ram/ordered_kv_misses|+0.219%|+0.532%, +0.680%, +0.688%, -1.013%|-0.165%|-0.201%, -0.491%, -0.062%, +0.094%|-0.098%|
|ram/ordered_kv_mixed|+0.327%|+0.990%, +0.351%, +0.639%, -0.663%|-0.969%|-0.334%, -3.232%, +0.030%, -0.303%|+0.080%|

#### Every write mean and tail: no winner selection

|Comparison|FS/batch/metric|Reference center|Placement center|Change|All 4 changes|Native change|
|---|---|---:|---:|---:|---|---:|
|REF→P0|ram/1/throughput|61,528.761|61,892.401|+0.591%|+6.032%, +1.073%, -4.898%, +0.456%|+1.090%|
|REF→P0|ram/1/combined_throughput|74,770.026|75,020.476|+0.335%|+4.124%, +0.057%, -3.780%, +1.099%|+0.349%|
|REF→P0|ram/1/sampled_p99|11,440.184|11,678.955|+2.087%|-0.642%, +6.029%, +10.053%, -6.319%|+1.314%|
|REF→P0|ram/1/pooled_sampled_p99|12,753.052|14,193.443|+11.294%|+0.009%, +46.549%, +46.162%, -28.379%|+6.258%|
|REF→P0|ram/64/throughput|975,259.935|976,307.340|+0.107%|+0.775%, -0.140%, -1.382%, +1.197%|-0.657%|
|REF→P0|ram/64/combined_throughput|984,413.420|983,907.776|-0.051%|+1.272%, -0.363%, -2.157%, +1.081%|-0.592%|
|REF→P0|ram/64/sampled_p99|103,800.189|103,293.559|-0.488%|-6.510%, +3.189%, +4.216%, -2.462%|+0.561%|
|REF→P0|ram/64/pooled_sampled_p99|251,140.870|250,436.442|-0.280%|-4.951%, -3.007%, +5.908%, +1.275%|+0.034%|
|REF→P0|disk/1/throughput|15,773.594|15,632.436|-0.895%|-1.234%, -3.921%, +1.134%, +0.520%|+0.512%|
|REF→P0|disk/1/combined_throughput|16,275.269|16,283.170|+0.049%|-1.261%, -3.449%, +5.014%, +0.081%|-0.437%|
|REF→P0|disk/1/sampled_p99|85,359.810|81,653.827|-4.342%|-2.638%, +7.502%, -20.880%, +1.111%|+2.920%|
|REF→P0|disk/1/pooled_sampled_p99|312,911.450|239,533.138|-23.450%|+1.069%, +0.595%, -67.220%, +3.032%|+0.264%|
|REF→P0|disk/64/throughput|487,628.952|492,341.077|+0.966%|+0.602%, -1.184%, +5.023%, -0.463%|-0.042%|
|REF→P0|disk/64/combined_throughput|498, 425.789|498,470.097|+0.009%|-1.558%, -3.106%, +2.314%, +2.504%|-0.323%|
|REF→P0|disk/64/sampled_p99|218,878.821|224,517.092|+2.576%|+12.102%, +29.904%, -6.815%, -18.417%|+3.205%|
|REF→P0|disk/64/pooled_sampled_p99|414,361.390|420,969.609|+1.595%|+62.094%, +2.215%, -2.036%, -34.365%|-1.339%|
|REF→P16|ram/1/throughput|61,528.761|61,646.873|+0.192%|+3.309%, -4.209%, +0.727%, +1.093%|+0.487%|
|REF→P16|ram/1/combined_throughput|74,770.026|74,788.090|+0.024%|+2.541%, -3.476%, +0.424%, +0.705%|+0.007%|
|REF→P16|ram/1/sampled_p99|11,440.184|11,726.844|+2.506%|-2.025%, +11.361%, +1.679%, -0.480%|+1.434%|
|REF→P16|ram/1/pooled_sampled_p99|12,753.052|13,698.772|+7.416%|-2.555%, +47.844%, +1.009%, -8.515%|+5.268%|
|REF→P16|ram/64/throughput|975,259.935|976,506.966|+0.128%|-0.207%, +0.292%, +0.558%, -0.130%|-0.769%|
|REF→P16|ram/64/combined_throughput|984,413.420|984,213.912|-0.020%|+0.379%, -0.094%, +0.237%, -0.600%|-0.780%|
|REF→P16|ram/64/sampled_p99|103,800.189|102,934.597|-0.834%|-4.718%, +3.370%, -0.527%, -1.294%|+0.804%|
|REF→P16|ram/64/pooled_sampled_p99|251,140.870|245,910.924|-2.082%|-4.917%, -2.024%, -1.384%, +0.062%|-1.928%|
|REF→P16|disk/1/throughput|15,773.594|15,496.050|-1.760%|-2.282%, -5.201%, +3.168%, -2.537%|-3.391%|
|REF→P16|disk/1/combined_throughput|16,275.269|16,302.348|+0.166%|-0.351%, -2.289%, +4.724%, -1.276%|-3.798%|
|REF→P16|disk/1/sampled_p99|85,359.810|79,890.508|-6.407%|-3.569%, -2.214%, -16.520%, -2.525%|+13.830%|
|REF→P16|disk/1/pooled_sampled_p99|312,911.450|233,832.914|-25.272%|-2.355%, -0.212%, -68.021%, +0.081%|+1.694%|
|REF→P16|disk/64/throughput|487,628.952|489,287.270|+0.340%|+0.886%, -1.276%, +5.700%, -3.712%|-0.502%|
|REF→P16|disk/64/combined_throughput|498, 425.789|497,331.373|-0.220%|-1.945%, -1.035%, +2.958%, -0.786%|-0.500%|
|REF→P16|disk/64/sampled_p99|218,878.821|229,825.715|+5.001%|+25.056%, +11.700%, -7.428%, -5.997%|+2.993%|
|REF→P16|disk/64/pooled_sampled_p99|414,361.390|404,068.716|-2.484%|+1.743%, +38.951%, -2.770%, -34.213%|-1.561%|
|REF→P32|ram/1/throughput|61,528.761|62,026.240|+0.809%|+3.088%, -0.361%, -1.123%, +1.685%|+0.577%|
|REF→P32|ram/1/combined_throughput|74,770.026|74,675.860|-0.126%|+2.100%, -0.571%, -2.778%, +0.812%|-0.060%|
|REF→P32|ram/1/sampled_p99|11,440.184|12,119.382|+5.937%|-0.527%, +0.589%, +20.913%, +4.103%|+3.780%|
|REF→P32|ram/1/pooled_sampled_p99|12,753.052|16,670.878|+30.721%|+0.937%, -0.352%, +173.278%, +6.231%|+6.447%|
|REF→P32|ram/64/throughput|975,259.935|972,876.932|-0.244%|+0.151%, -1.628%, -1.238%, +1.773%|-0.899%|
|REF→P32|ram/64/combined_throughput|984,413.420|981,726.807|-0.273%|+0.162%, -1.508%, -1.110%, +1.389%|-0.944%|
|REF→P32|ram/64/sampled_p99|103,800.189|102,519.816|-1.233%|-6.045%, +3.443%, +0.552%, -2.628%|+1.019%|
|REF→P32|ram/64/pooled_sampled_p99|251,140.870|246,333.486|-1.914%|-3.701%, -2.111%, -0.278%, -1.537%|-1.175%|
|REF→P32|disk/1/throughput|15,773.594|15,090.676|-4.329%|-8.698%, -2.658%, -3.135%, -2.688%|-1.108%|
|REF→P32|disk/1/combined_throughput|16,275.269|16,094.947|-1.108%|-2.059%, -0.231%, -3.921%, +1.873%|+0.247%|
|REF→P32|disk/1/sampled_p99|85,359.810|79,079.829|-7.357%|-14.598%, -7.224%, +12.149%, -17.101%|-4.594%|
|REF→P32|disk/1/pooled_sampled_p99|312,911.450|199,856.997|-36.130%|-68.002%, +2.317%, +4.901%, -51.545%|-15.044%|
|REF→P32|disk/64/throughput|487,628.952|477,263.378|-2.126%|+1.269%, -3.200%, -0.986%, -5.457%|+0.921%|
|REF→P32|disk/64/combined_throughput|498, 425.789|486,090.282|-2.475%|-0.594%, -3.381%, -1.742%, -4.144%|+0.297%|
|REF→P32|disk/64/sampled_p99|218,878.821|240,654.649|+9.949%|+10.262%, +12.564%, -4.767%, +23.638%|+3.458%|
|REF→P32|disk/64/pooled_sampled_p99|414,361.390|428,543.467|+3.423%|+60.637%, +5.053%, -0.993%, -31.523%|-1.159%|
|P16→P0|ram/1/throughput|61,646.873|61,892.401|+0.398%|+2.636%, +5.514%, -5.585%, -0.630%|+0.600%|
|P16→P0|ram/1/combined_throughput|74,788.090|75,020.476|+0.311%|+1.543%, +3.660%, -4.186%, +0.391%|+0.342%|
|P16→P0|ram/1/sampled_p99|11,726.844|11,678.955|-0.408%|+1.412%, -4.787%, +8.235%, -5.867%|-0.119%|
|P16→P0|ram/1/pooled_sampled_p99|13,698.772|14,193.443|+3.611%|+2.631%, -0.876%, +44.702%, -21.712%|+0.941%|
|P16→P0|ram/64/throughput|976,506.966|976,307.340|-0.020%|+0.983%, -0.431%, -1.929%, +1.329%|+0.113%|
|P16→P0|ram/64/combined_throughput|984,213.912|983,907.776|-0.031%|+0.890%, -0.269%, -2.389%, +1.691%|+0.189%|
|P16→P0|ram/64/sampled_p99|102,934.597|103,293.559|+0.349%|-1.881%, -0.175%, +4.768%, -1.184%|-0.241%|
|P16→P0|ram/64/pooled_sampled_p99|245,910.924|250,436.442|+1.840%|-0.036%, -1.004%, +7.395%, +1.213%|+2.000%|
|P16→P0|disk/1/throughput|15,496.050|15,632.436|+0.880%|+1.072%, +1.350%, -1.971%, +3.136%|+4.040%|
|P16→P0|disk/1/combined_throughput|16,302.348|16,283.170|-0.118%|-0.914%, -1.188%, +0.277%, +1.375%|+3.495%|
|P16→P0|disk/1/sampled_p99|79,890.508|81,653.827|+2.207%|+0.966%, +9.936%, -5.222%, +3.731%|-9.584%|
|P16→P0|disk/1/pooled_sampled_p99|233,832.914|239,533.138|+2.438%|+3.507%, +0.809%, +2.505%, +2.949%|-1.406%|
|P16→P0|disk/64/throughput|489,287.270|492,341.077|+0.624%|-0.281%, +0.094%, -0.640%, +3.374%|+0.462%|
|P16→P0|disk/64/combined_throughput|497,331.373|498,470.097|+0.229%|+0.395%, -2.092%, -0.626%, +3.316%|+0.178%|
|P16→P0|disk/64/sampled_p99|229,825.715|224,517.092|-2.310%|-10.359%, +16.298%, +0.662%, -13.213%|+0.206%|
|P16→P0|disk/64/pooled_sampled_p99|404,068.716|420,969.609|+4.183%|+59.317%, -26.438%, +0.755%, -0.230%|+0.225%|
|P16→P32|ram/1/throughput|61,646.873|62,026.240|+0.615%|-0.214%, +4.017%, -1.837%, +0.585%|+0.090%|
|P16→P32|ram/1/combined_throughput|74,788.090|74,675.860|-0.150%|-0.430%, +3.009%, -3.188%, +0.106%|-0.067%|
|P16→P32|ram/1/sampled_p99|11,726.844|12,119.382|+3.347%|+1.529%, -9.673%, +18.916%, +4.605%|+2.312%|
|P16→P32|ram/1/pooled_sampled_p99|13,698.772|16,670.878|+21.696%|+3.583%, -32.599%, +170.549%, +16.119%|+1.120%|
|P16→P32|ram/64/throughput|976,506.966|972,876.932|-0.372%|+0.358%, -1.915%, -1.786%, +1.906%|-0.131%|
|P16→P32|ram/64/combined_throughput|984,213.912|981,726.807|-0.253%|-0.216%, -1.415%, -1.343%, +2.002%|-0.166%|
|P16→P32|ram/64/sampled_p99|102,934.597|102,519.816|-0.403%|-1.393%, +0.070%, +1.085%, -1.352%|+0.214%|
|P16→P32|ram/64/pooled_sampled_p99|245,910.924|246,333.486|+0.172%|+1.279%, -0.089%, +1.122%, -1.597%|+0.768%|
|P16→P32|disk/1/throughput|15,496.050|15,090.676|-2.616%|-6.566%, +2.683%, -6.109%, -0.155%|+2.362%|
|P16→P32|disk/1/combined_throughput|16,302.348|16,094.947|-1.272%|-1.715%, +2.106%, -8.255%, +3.190%|+4.205%|
|P16→P32|disk/1/sampled_p99|79,890.508|79,079.829|-1.015%|-11.437%, -5.124%, +34.343%, -14.953%|-16.185%|
|P16→P32|disk/1/pooled_sampled_p99|233,832.914|199,856.997|-14.530%|-67.230%, +2.535%, +228.035%, -51.584%|-16.459%|
|P16→P32|disk/64/throughput|489,287.270|477,263.378|-2.457%|+0.379%, -1.949%, -6.325%, -1.812%|+1.429%|
|P16→P32|disk/64/combined_throughput|497,331.373|486,090.282|-2.260%|+1.378%, -2.370%, -4.565%, -3.384%|+0.801%|
|P16→P32|disk/64/sampled_p99|229,825.715|240,654.649|+4.712%|-11.830%, +0.774%, +2.874%, +31.525%|+0.452%|
|P16→P32|disk/64/pooled_sampled_p99|404,068.716|428,543.467|+6.057%|+57.886%, -24.396%, +1.828%, +4.089%|+0.408%|

P32 versus REF disk-single aggregate−4.330% and disk64 combined−2.475% are adverse in all four quiet pairs, alongside its small favorable disk KV means. RAM write changes are mostly small and mixed. A favorable read alignment is not selected as a candidate while these costs are ignored. Both aggregate and all 256-commit combined rates remain.

#### True global 512 pools and maxima: every arm and engine

|FS/batch/engine|REF p99/max ns|P0 p99/max ns|P16 p99/max ns|P32 p99/max ns|
|---|---:|---:|---:|---:|
|disk/1/ulmdb|314428/324237|314959/570925|310229/409699|310721/408577|
|disk/1/lmdb|326250/334737|326911/332813|331761/337632|324086/328565|
|disk/64/ulmdb|382707/601714|375584/609730|433945/619398|389441/635459|
|disk/64/lmdb|533695/594510|529827/538414|528815/552431|531070/591335|
|ram/1/ulmdb|12023/16742|12434/25629|15961/18505|12814/36789|
|ram/1/lmdb|8516/12354|8857/13555|8536/15389|8807/29126|
|ram/64/ulmdb|252530/270875|248252/269382|245637/281645|248392/270625|
|ram/64/lmdb|103958/111192|97917/115449|105130/113886|97255/147951|

RAM-single global p99 is REF 12.023µs/P0 12.434/P16 15.961/P32 12.814; maxima 16.742/25.629/18.505/36.789µs. Disk64 globals 382.707/375.584/433.945/389.441µs and maxima 601.714/609.730/619.398/635.459µs stay. Geometric trial p99, geometric process-pooled p99 and global 512 p99 are distinct; these samples do not establish production percentiles or tail equivalence. Native variation is included rather than normalized.

#### Boundary on interpretation

The disk association is evidence that placement can affect this routine under these complete workloads, with an exposed P16 observation explicitly retained. It does not show a universal alignment winner or the hardware mechanism, and the small/mixed RAM effects do not account for the earlier large read gap. No contemporaneous retained6 or million arm is present. The original seeded-cache and capacity candidates remain rejected; accepted6 and published charts stay unchanged. Any next controlled measurement must be separately declared, with source/semantic/CFI qualification before timing; no production patch is authorized here.

[All 204 metrics, all four centers and process values](benchmark/work/optimization-20261105/next-word-leaf-placement/seed-off-v1/controls-v1/analysis-v1/ALL_ARM_VALUES.md), [all five 1020-cell comparisons](benchmark/work/optimization-20261105/next-word-leaf-placement/seed-off-v1/controls-v1/analysis-v1/ALL_COMPARISONS.md), [every raw sample/context/counter binding](benchmark/work/optimization-20261105/next-word-leaf-placement/seed-off-v1/controls-v1/analysis-v1/raw-review.json), [global pools and process maxima](benchmark/work/optimization-20261105/next-word-leaf-placement/seed-off-v1/controls-v1/analysis-v1/global-write-tails.json), [independent raw/source audit](benchmark/work/optimization-20261105/next-word-leaf-placement/seed-off-v1/controls-v1/analysis-v1/audit-receipt.json), [predeclared plan](benchmark/work/optimization-20261105/next-word-leaf-placement/seed-off-v1/controls-v1/PLAN_V1.md).


### Current unchanged retained6/OFF gap: large legacy loss not reproduced

**Qualification result:** OFF versus retained6 KV hits−0.695%, misses−0.743% and mixed−0.950%, with mixed paired signs. The earlier coherent roughly 10% loss does not reproduce in this fixed run and must not be assumed to be a current cost for a stall profile to explain. This is not proof of equal performance, not deletion of earlier adverse campaigns and not permission to retain the rejected seeded-cache candidate. Accepted6 stays current.

#### Exact scope and provenance

All 8 unchanged full RAM processes pass every original 13-read/two-write semantic/value/cursor/prefix/owner/retained-borrow/fresh-reopen check. Seven immediate return boundaries are quiet; 000 retained6/block0 observes external activity and remains. No point is retried, excluded or normalized against native. Aggregate baseline comparisons include that exposed reference; all three other KV pair directions are preserved separately without a replacement subset mean. Boundaries do not prove continuous isolation.

Retained6 is exact 1aedee19… with its separate 469-file archived source snapshot, native metadata and Rust build fields. OFF is exact 6d5b9e54… with separate 480-file snapshot and frozen 548-file tree; only its existing seed namespace is projected from deep copies, with explicit null before and after. R6 namespace is absent. No code is rebuilt, byte patched, profiled or given a query flag in this run. Native flags remain 0 and each report retains its own actual embedded native/archive identity. Current 26 pins plus 243 historical bindings/eight previous inventories pass. R6 is not rebound to the newer OFF source tree.

Original CLI defaults are 8192 KV keys,1024 specific prefix attempts,4000 generated queries,16 trials,8 commits per write phase and seed73. KV mixed/hit/miss groups are 4000/1990/2010, giving 128000 aggregate KV queries across those groups, plus original separate latency phases. Four paired blocks balance process order and equal-length artifact/fixture paths. All creation/resources/native values and both all 256-commit write budgets remain charged. No first-use cost is hidden.

Independent offline audit verifies all 8 raw files/57,632,768 bytes,3848 spans,7,204,096 samples and 512 write spans. Every SHA, complete extent coverage, timer/read p50/p95/p99, write p99, original denominator, combined sum, resource schema, global 512 pool and maximum passes. The original comparison helper is adapted only from disk+RAM inventory to the explicitly declared RAM-only inventory; all 102 metric formulas/statistics stay unchanged, with no fake disk observations. All paired observations and unavailable values remain in the complete table.

#### Every read rate, sampled p99 and direct native context

Direct OFF/native percentages are raw ratio minus1. Higher throughput is better; lower latency/time/space is better. Native is context, not a normalizing divisor.

|Case|R6 queries/s|OFF queries/s|OFF vsR6|All 4 paired changes|Trial p99 change|All 4 p99 changes|R6 native queries/s|OFF native queries/s|OFF vsnative|
|---|---:|---:|---:|---|---:|---|---:|---:|---:|
|ipv4_lpm_hits|3,181,915.735|3,199,762.795|+0.561%|-0.398%, +1.938%, +0.836%, -0.115%|-2.904%|-0.242%, -9.312%, -1.720%, -0.036%|2,106,494.013|2,138,524.659|+49.625%|
|ipv4_lpm_inside_gap_misses|11,420,155.279|11,577,725.657|+1.380%|-0.612%, +6.062%, -0.158%, +0.368%|-1.245%|-4.223%, +0.052%, -0.952%, +0.208%|1,956,506.618|1,972,270.671|+487.025%|
|ipv4_lpm_misses|21,514,010.479|21,535,539.625|+0.100%|+0.477%, +2.620%, -0.975%, -1.668%|-2.117%|-3.387%, -4.321%, +0.000%, -0.694%|3,866,354.004|3,895,914.042|+452.772%|
|ipv4_lpm_mixed|5,480,332.788|5,510,049.723|+0.542%|-0.562%, +2.009%, +1.007%, -0.265%|-2.591%|-0.855%, -6.971%, -1.935%, -0.459%|2,642,833.052|2,675,057.041|+105.979%|
|ipv4_lpm_outside_cover_misses|98,428,856.747|99,711,181.812|+1.303%|+3.047%, +5.735%, -3.345%, +0.002%|-1.263%|-3.653%, -4.010%, +1.373%, +1.373%|94,240,198.417|95,764,059.125|+4.122%|
|ipv6_lpm_hits|2,474,665.428|2,498,689.417|+0.971%|+1.346%, +1.541%, +0.514%, +0.487%|-1.256%|-3.562%, -0.479%, +0.714%, -1.646%|1,251,809.252|1,211,113.980|+106.313%|
|ipv6_lpm_inside_gap_misses|9,313,813.362|9,356,871.340|+0.462%|+2.248%, +0.073%, -0.425%, -0.024%|-1.002%|+0.000%, -1.356%, +0.863%, -3.461%|1,139,188.462|1,100,917.198|+749.916%|
|ipv6_lpm_misses|17,196,446.249|17,297,091.139|+0.585%|+0.340%, +1.915%, -0.102%, +0.201%|-1.475%|-2.347%, -0.532%, +0.000%, -2.989%|2,236,597.693|2,187,902.576|+690.579%|
|ipv6_lpm_mixed|4,303,480.676|4,331,022.323|+0.640%|+1.358%, +0.972%, +0.325%, -0.089%|-1.532%|-4.396%, -0.351%, -0.611%, -0.715%|1,570,156.930|1,521,607.415|+184.635%|
|ipv6_lpm_outside_cover_misses|97,103,943.140|97,326,381.984|+0.229%|+0.731%, -0.170%, -0.181%, +0.540%|+0.311%|-1.355%, +0.000%, +2.512%, +0.124%|93,953,720.141|95,111,209.374|+2.329%|
|ordered_kv_hits|7,494,088.476|7,442,013.018|-0.695%|-2.377%, +0.391%, +0.706%, -1.466%|-0.180%|+3.253%, -4.171%, -0.307%, +0.650%|4,146,069.474|4,187,417.217|+77.723%|
|ordered_kv_misses|7,486,724.687|7,431,112.145|-0.743%|-2.607%, +2.362%, +0.187%, -2.822%|-0.441%|+1.889%, -5.805%, +0.813%, +1.543%|3,987,948.614|4,049,019.167|+83.529%|
|ordered_kv_mixed|7,168,138.819|7,100,063.884|-0.950%|-2.455%, +0.432%, -0.351%, -1.401%|-0.586%|+4.304%, -8.634%, +0.000%, +2.495%|4,091,269.371|4,081,465.073|+73.959%|

KV paired changes: hits[−2.377,+0.391,+0.706,−1.466], misses[−2.607,+2.362,+0.187,−2.822], mixed[−2.455,+0.432,−0.351,−1.401]%. Native hit/miss/mixed context changes +0.997%/+1.531%/−0.240%; none is used as a normalization divisor. Native ratios above are direct OFF/native centers within the OFF processes; all OFF/native return boundaries are quiet. Empty adjusted-regression flags are uncertainty, not equivalence.

#### Every write rate and sampled tail

|Batch/metric|R6 center|OFF center|OFF vsR6|All 4 changes|R6 native center|OFF native center|OFF vsnative|
|---|---:|---:|---:|---|---:|---:|---:|
|1/throughput|62,956.068|63,315.932|+0.572%|+2.627%, -0.563%, +0.786%, -0.530%|135,383.938|134,279.340|-52.848%|
|1/combined_throughput|75,807.801|75,905.233|+0.129%|+1.169%, -0.412%, +0.070%, -0.305%|139,152.703|138,050.432|-45.016%|
|1/sampled_p99|11,685.923|11,781.987|+0.822%|+2.565%, +2.701%, +1.345%, -3.206%|7,989.155|8,068.992|+46.016%|
|1/pooled_sampled_p99|14,211.688|15,397.502|+8.344%|+49.912%, +40.099%, -29.006%, -7.589%|10,066.417|9,207.181|+67.234%|
|64/throughput|981,018.153|976,804.680|-0.429%|+0.204%, -0.082%, -1.429%, -0.403%|749,989.981|744,820.049|+31.146%|
|64/combined_throughput|990,503.738|985,923.299|-0.462%|-0.289%, -0.042%, -1.559%, +0.050%|756,879.867|752,022.551|+31.103%|
|64/sampled_p99|101,893.266|103,325.611|+1.406%|+3.998%, -0.553%, +1.052%, +1.178%|92,115.812|92,655.431|+11.516%|
|64/pooled_sampled_p99|251,068.523|245,714.785|-2.132%|-7.680%, -1.856%, +0.212%, +1.036%|99,173.059|99,251.600|+147.568%|

OFF single-write aggregate+0.572%/combined+0.129% and batch64 −0.429%/−0.462% are small/mixed observed changes. These are no new optimization result; OFF disables the constructor benefit and is used only to qualify the current read comparison.

#### True global sampled tails and all maxima

|Batch/engine|Global 512 p99 R6→OFF ns|Maximum R6→OFF ns|Process p99 R6|Process p99 OFF|
|---|---:|---:|---|---|
|1/ulmdb|16762→16732|20680→18115|[11943, 11943, 17062, 16762]|[17904, 16732, 12113, 15490]|
|1/lmdb|8797→8867|13215→14337|[8226, 8046, 13035, 11902]|[8106, 8396, 13044, 8095]|
|64/ulmdb|250256→246519|265936→258252|[262630, 249394, 245997, 246609]|[242461, 244765, 246519, 249163]|
|64/lmdb|98327→99419|115149→117253|[96143, 100041, 93448, 107624]|[98157, 99419, 100732, 98717]|

Single-write global p99 is 16.762→16.732µs and maximum 20.680→18.115µs; batch64 global 250.256→246.519µs/maximum 265.936→258.252µs. Trial geometric, process-pooled geometric and global 512 p99 are separate statistics; no production-percentile or no-regression claim follows. Native tails and all creation/resources/cold costs remain in the 102-cell table.

#### Gate for subsequent profiling

The old large read gap is not a reproduced present target. A future instruction-count or sampling experiment can characterize these current ELFs, but cannot call its result the explanation of the historical 10% cost without reproducing that cost. Original aggregate KV windows are short (only 128000 total aggregate queries); full-process samples mix setup, validation, latency and writes. Exact machine-code entry/exit windows, normalized ELF/function-relative RIP, honest unresolved/kernel and callchain limits, sample denominator/budget and a separately measured perturbation gate are required before a bounded profiling plan. No sampler implementation or new production candidate is authorized by these observations. Earlier rejected campaigns and every adverse estimate remain unchanged.

[All 102 full RAM metrics/native/resources](benchmark/work/optimization-20261105/next-read-gap-reproduction/retained6-off-v1/analysis-v1/ALL_METRICS.md), [every paired observation/interval](benchmark/work/optimization-20261105/next-read-gap-reproduction/retained6-off-v1/analysis-v1/R6-vs-OFF.json), [raw samples/contexts/namespace bindings](benchmark/work/optimization-20261105/next-read-gap-reproduction/retained6-off-v1/analysis-v1/raw-review.json), [raw/source/history audit receipt](benchmark/work/optimization-20261105/next-read-gap-reproduction/retained6-off-v1/analysis-v1/audit-receipt.json), [explicit RAM-only comparator adaptation](benchmark/work/optimization-20261105/next-read-gap-reproduction/retained6-off-v1/analysis-v1/comparison-adaptation-v1.json), [original fixed8 plan](benchmark/work/optimization-20261105/next-read-gap-reproduction/retained6-off-v1/PLAN_V1.md).


[Source-only read-profile gate and required measurement boundaries](benchmark/work/optimization-20261105/next-read-gap-reproduction/retained6-off-v1/PROFILE_GATE.md). No read sampling capture is justified by the unreproduced large gap. The completed RAM-write study below qualifies only exact updater call edges; its failed full-phase isolation and all raw discrepancies remain.


## Earlier measured candidate: RAM capacity SEEK_END REJECTED

The complete unchanged standard39 improves RAM-single throughput 2.59% aggregate/3.55% combined, but loses 2.62–3.15% on protected RAM KV reads in all four pairs. The miss interval excludes parity; loading is effectively flat at the observed center. Root rejected retention. This mechanism remains a study, with no runtime promotion or inferred source cause for the read cost.

### Capacity SEEK_END candidate: REJECTED for retention

**Decision: keep accepted iteration6.** RAM-single throughput improves 2.593% aggregate and 3.546% over all 256 charged commits, favorable in all four pairs. Protected RAM KV reads lose 2.618–3.154% in all four pairs for each hit/miss/mixed case; misses have an adjusted regression interval. The modest write gain and unchanged loading do not justify that coherent protected read cost. Root rejects this candidate for retention. The narrow syscall mechanism remains a study; source cause of read differences is unconfirmed, and no main runtime or accepted benchmark is promoted.

#### Completeness and context

All 39 fresh processes complete every original semantic/value/owner/borrow/cold check, source539/snapshot471, ELF/native/SQLite and workload/denominator guard. Full bbedaa97… and millions 2a7b1c7c… remain qualified, with 813 workspace tests/106 summaries and 38 actual fault-worker cases recorded separately from benchmark measurements. Actual full/million native TOML and archive bytes are pinned; only parsed archive_sha256 differs from retained6, with all returned metadata otherwise strict. Current SQLite DSO c912… matches the accepted runtime. Current 40 pins and historical40/21/9/48/9 (127) pins plus five prior campaign inventories pass. No prior campaign is rewritten.

Thirty-seven immediate return boundaries observed quiet. Exposed036 initial/million/r2 and 037 accepted/million/r2 remain. Every full-suite and every candidate/native boundary is quiet. Thus all full comparisons and direct candidate/native million ratios have observed-quiet contexts; million initial/reference aggregate ratios include exposed points and are descriptive/unqualified as new gain/loss evidence. No point is excluded, retried or normalized by native. Boundaries do not prove continuous isolation. Exit1 is a completed performance screen, not an engine failure.

Independent offline verification recomputes both 237-row comparisons and audits all 24 raw files/172,898,304 bytes,11,544 spans,21,612,288 samples and1,536 write spans. All hashes, complete byte coverage, every timer/read p50/p95/p99, write p99, true global pools/maxima,128+128=256 commit budgets and original resource schemas pass. Candidate reports retain the original schema with no seeded namespace; stored reports and timers are untouched. A Python audit import-path typo was corrected before verification, with its preparation failure preserved separately; it was no engine failure or observation retry.

#### Core rates and lifecycle costs

Raw C/native percent is candidate/native−1 for all units: higher throughput is better, lower time/space is better. The 103-row scorecard gives every signed native advantage, status and remaining gap. Within 2% is descriptive roughly on par, not equivalence.

|Metric|Retained6|Candidate|C vs6|LMDB|Raw C vsLMDB|Native status|
|---|---:|---:|---:|---:|---:|---|
|ram batch1 throughput|62,711.869|64,338.091|+2.593%|133,478.030|-51.799%|slower / less throughput|
|ram batch1 combined_throughput|75,253.032|77,921.621|+3.546%|137,846.504|-43.472%|slower / less throughput|
|ram batch64 throughput|977,740.653|987,780.903|+1.027%|748,688.815|+31.935%|faster / more throughput|
|ram batch64 combined_throughput|989,086.535|995,195.378|+0.618%|757,315.916|+31.411%|faster / more throughput|
|disk batch1 throughput|15,468.712|15,263.409|-1.327%|10,621.712|+43.700%|faster / more throughput|
|disk batch1 combined_throughput|16,014.845|16,254.787|+1.498%|11,223.966|+44.822%|faster / more throughput|
|disk batch64 throughput|490,681.939|483,182.199|-1.528%|177,311.158|+172.505%|faster / more throughput|
|disk batch64 combined_throughput|495,900.150|497,716.611|+0.366%|182,248.238|+173.098%|faster / more throughput|
|Million load records/s|1,402,587.746|1,401,710.298|-0.063%|1,576,541.990|-11.090%|slower / less throughput|
|Million read queries/s|2,798,029.793|2,763,771.959|-1.224%|1,687,748.561|+63.755%|faster / more throughput|
|Million update records/s|145,431.622|145,502.981|+0.049%|128,894.470|+12.885%|faster / more throughput|
|Setup ms|404.307|401.032|-0.810%|1.354|+29518.897%|higher cost|
|Drop+open ms|7,048.220|6,829.784|-3.099%|3.763|+181396.217%|higher cost|
|Allocated MiB|256.001|256.000|-0.001%|33.043|+674.749%|higher cost|
|Apparent MiB|256.000|256.000|+0.000%|33.047|+674.657%|higher cost|
|Files|2.000|2.000|+0.000%|2.000|+0.000%|roughly on par (observed within2%; not equivalence)|

The rejected candidate remains 51.799% below native on RAM-single aggregate (107.463% more throughput needed) and 43.472% below on combined commits (76.904% more needed). Loading remains 11.090% below native (12.473% more needed). Setup 401.032ms versus 1.354ms is 296.189×; drop+open 6829.784ms versus 3.763ms is 1814.962×. Allocation 256MiB versus 33.043MiB is 7.747×. The fixed WAL capacity is unchanged; tiny block-allocation differences in the retained reference are preserved, with no architecture/footprint gain claimed.

#### Every protected full read and paired direction

|FS/case|Rate C vs6|All 4 rate changes|Trial p99 C vs6|
|---|---:|---|---:|
|disk/ipv4_lpm_hits|+0.182%|-0.006%, -0.585%, +0.683%, +0.642%|-1.082%|
|disk/ipv4_lpm_inside_gap_misses|+0.670%|+0.784%, -0.706%, +1.038%, +1.580%|-6.826%|
|disk/ipv4_lpm_misses|+0.836%|+0.205%, +1.994%, +0.229%, +0.925%|-7.448%|
|disk/ipv4_lpm_mixed|+0.484%|+0.605%, +0.449%, +0.447%, +0.434%|-1.056%|
|disk/ipv4_lpm_outside_cover_misses|-0.109%|-0.471%, +0.051%, -0.020%, +0.005%|-0.987%|
|disk/ipv6_lpm_hits|+1.986%|+2.588%, +1.099%, +2.001%, +2.260%|-0.329%|
|disk/ipv6_lpm_inside_gap_misses|-0.671%|-0.989%, -1.478%, -0.349%, +0.140%|-0.461%|
|disk/ipv6_lpm_misses|-0.417%|-0.121%, -0.001%, -0.972%, -0.572%|-1.803%|
|disk/ipv6_lpm_mixed|+1.615%|+1.047%, +1.413%, +2.122%, +1.880%|-0.061%|
|disk/ipv6_lpm_outside_cover_misses|+1.744%|+2.965%, +2.722%, +0.165%, +1.150%|+0.342%|
|disk/ordered_kv_hits|-1.087%|-2.553%, -2.477%, +3.356%, -2.545%|+0.738%|
|disk/ordered_kv_misses|-1.084%|-1.840%, -3.533%, +3.953%, -2.745%|-0.041%|
|disk/ordered_kv_mixed|-1.085%|-2.802%, -2.575%, +2.835%, -1.693%|+2.261%|
|ram/ipv4_lpm_hits|-0.675%|-0.431%, +0.297%, -2.221%, -0.329%|+0.636%|
|ram/ipv4_lpm_inside_gap_misses|+0.598%|+0.598%, +0.806%, +1.496%, -0.495%|-5.878%|
|ram/ipv4_lpm_misses|+0.293%|+0.003%, +1.308%, -0.919%, +0.793%|-4.663%|
|ram/ipv4_lpm_mixed|-0.988%|-0.226%, -0.371%, -2.950%, -0.378%|+1.345%|
|ram/ipv4_lpm_outside_cover_misses|+0.142%|+0.082%, -0.011%, +0.652%, -0.153%|-0.340%|
|ram/ipv6_lpm_hits|+2.175%|+2.240%, +2.499%, +1.926%, +2.038%|+0.650%|
|ram/ipv6_lpm_inside_gap_misses|-0.977%|+1.229%, -2.649%, -1.328%, -1.120%|-0.478%|
|ram/ipv6_lpm_misses|-0.546%|-0.612%, -1.399%, -0.404%, +0.239%|+0.595%|
|ram/ipv6_lpm_mixed|+1.300%|+1.368%, +1.548%, +0.635%, +1.652%|+1.165%|
|ram/ipv6_lpm_outside_cover_misses|-1.871%|+0.126%, -0.089%, -3.561%, -3.887%|+0.692%|
|ram/ordered_kv_hits|-3.154%|-2.401%, -1.868%, -5.601%, -2.704%|+4.397%|
|ram/ordered_kv_misses|-2.899%|-2.806%, -2.990%, -2.982%, -2.818%|+3.400%|
|ram/ordered_kv_mixed|-2.618%|-3.017%, -3.170%, -2.123%, -2.158%|+4.045%|

RAM KV hits lose 3.154%, misses 2.899%, mixed 2.618%, every pair adverse and every full boundary quiet. Misses adjusted candidate/reference ratio interval [0.960081,0.982066] excludes parity. Disk KV means are roughly−1.085% with three adverse/one favorable pair; every direction remains. RAM IPv4 mixed−0.988% is adverse all four, while IPv6 hits+2.175%/mixed+1.300% improve all four. The candidate is judged on the entire inventory, not only these winners or costs.

#### All write means, trial and process-pooled tails

|FS/batch/metric|C vs6|All 4 changes|
|---|---:|---|
|ram/1/throughput|+2.593%|+3.007%, +1.165%, +2.957%, +3.258%|
|ram/1/combined_throughput|+3.546%|+4.040%, +2.468%, +4.255%, +3.432%|
|ram/1/sampled_p99|-8.536%|-10.574%, -9.280%, -12.040%, -1.927%|
|ram/1/pooled_sampled_p99|-31.763%|-36.933%, -33.533%, -39.619%, -14.342%|
|ram/64/throughput|+1.027%|+0.858%, -0.570%, +2.814%, +1.034%|
|ram/64/combined_throughput|+0.618%|+0.673%, -0.760%, +1.681%, +0.891%|
|ram/64/sampled_p99|+0.772%|+1.064%, +4.620%, -1.509%, -0.974%|
|ram/64/pooled_sampled_p99|-1.995%|-5.632%, +2.189%, +2.130%, -6.326%|
|disk/1/throughput|-1.327%|-7.732%, +3.027%, +5.262%, -5.264%|
|disk/1/combined_throughput|+1.498%|-4.993%, +6.176%, +5.814%, -0.571%|
|disk/1/sampled_p99|-8.621%|+6.449%, -14.354%, -12.171%, -12.924%|
|disk/1/pooled_sampled_p99|-1.837%|-0.526%, -3.393%, -2.589%, -0.809%|
|disk/64/throughput|-1.528%|+0.121%, -1.180%, +4.845%, -9.358%|
|disk/64/combined_throughput|+0.366%|+0.291%, +0.186%, +4.395%, -3.261%|
|disk/64/sampled_p99|-0.585%|-1.624%, +5.555%, -10.624%, +5.250%|
|disk/64/pooled_sampled_p99|+28.244%|+65.213%, +0.889%, -0.357%, +62.861%|

Trial geometric, process-pooled geometric and global 512 p99 are different statistics. All sampled values and maxima remain; none is a production percentile.

|FS/batch|Global 512 p99 R→C ns|Maximum R→C ns|Global candidate LMDB p99 ns|Process p99 R|Process p99 C|
|---|---:|---:|---:|---|---|
|disk/1|311663→309498|572218→316211|329477|[310560, 320369, 318064, 310701]|[308927, 309498, 309829, 308186]|
|disk/64|371065→444545|644386→608467|525800|[361197, 370735, 374062, 371065]|[596744, 374031, 372728, 604319]|
|ram/1|18224→12002|33754→32041|8567|[18555, 18224, 18385, 17884]|[11702, 12113, 11101, 15319]|
|ram/64|259925→252180|275213→270184|97395|[260596, 247200, 246920, 259925]|[245918, 252610, 252180, 243483]|

RAM-single global p99 improves 18.224→12.002µs (−34.142%), maximum 33.754→32.041µs. Disk64 global p99 worsens 371.065→444.545µs (+19.802%), with candidate process p99s 596.744/374.031/372.728/604.319µs versus 361.197/370.735/374.062/371.065µs. Its maximum improves 644.386→608.467µs; both observations stand. Disk-single global p99 improves 0.695% and maximum 572.218→316.211µs. These mixed tails do not establish no-regression or equivalence.

#### Million pairs and charged lifecycle sum

|Metric|C vs6|All 3 changes|
|---|---:|---|
|load/throughput|-0.063%|+0.296%, -0.323%, -0.159%|
|read/throughput|-1.224%|+1.194%, -2.282%, -2.541%|
|update/throughput|+0.049%|+0.719%, -1.389%, +0.832%|
|setup_ns|-0.810%|-1.876%, +0.088%, -0.632%|
|close_reopen_ns|-3.099%|-3.313%, -2.768%, -3.216%|
|storage_after_close_reopen/allocated_bytes|-0.001%|+0.000%, -0.002%, +0.000%|

Limited lifecycle task centers: retained6 15.041623s, candidate 14.817063s, native 8.397788s. C cost vs6 -1.493% (one exposed reference), direct native +76.440% (observedquiet). This sum does not justify protected read regressions.

Loading−0.063% and updates+0.049% are effectively flat observed centers with mixed pairs, not equivalence. Read−1.224% is mixed. Reopen−3.099% repeats all three, but the persistent query/recovery architecture is unchanged; no architectural reopen gain is inferred. The root decision is REJECT, with no additional confirmation to search for a favorable result. The mechanism and native fault evidence remain available as a study. The next authorized work is a separate same-ELF constructor causal diagnostic; it has no accepted speedup or production code claim.

[All 103 native targets/status/gaps](benchmark/work/optimization-20261105/next-capacity-seek-study/retained6-v1/analysis-v1/report/NATIVE_SCORECARD.md), [all 474 unfiltered comparison cells](benchmark/work/optimization-20261105/next-capacity-seek-study/retained6-v1/analysis-v1/report/ALL_COMPARISONS.md), [every raw/source/native/history check](benchmark/work/optimization-20261105/next-capacity-seek-study/retained6-v1/analysis-v1/review-receipt.json), [global sampled pools/maxima](benchmark/work/optimization-20261105/next-capacity-seek-study/retained6-v1/analysis-v1/global-write-tails.json), [actual availability identities](benchmark/work/optimization-20261105/next-capacity-seek-study/retained6-v1/analysis-v1/availability-review.json), [original namespace/resource confirmation](benchmark/work/optimization-20261105/next-capacity-seek-study/retained6-v1/analysis-v1/namespace-review.json).



## Completed causal control: constructor execution, same linked addresses

A one-byte ON/OFF diagnostic shows no coherent KV-rate restoration when Builder is disabled. OFF loses 22.68% RAM-single aggregate throughput and 14.29% combined throughput, all four pairs adverse. All16 original semantic/raw checks pass and all16 return boundaries are quiet. This diagnoses executing the seed path and its resulting runtime state; it does not isolate malloc, prove code placement as the read cause, or qualify the rejected candidate for acceptance.

### Same-ELF seed ON/OFF: no coherent KV restoration; write benefit repeats

**Diagnostic conclusion:** disabling Builder does not produce a coherent KV-throughput restoration. OFF versus ON is RAM hits +0.047%/misses−0.399%/mixed+0.040% and disk hits+0.847%/misses+0.498%/mixed+0.005%, with mixed signs. This does not establish equivalence. It does not explain the earlier roughly 10% protected read loss as an effect solely of executing Builder in this frozen binary.

The same change drops RAM-single throughput 22.676% aggregate and 14.292% over all 256 charged commits, adverse for OFF in all four pairs. Thus the executed seed path and its resulting runtime state benefit RAM-single work in this binary. This does not isolate malloc or a single Collect clock. There is no contemporary retained6 arm, so no claim that the earlier R6 read gap is resolved, that code placement is the unique cause, or that the rejected candidate is acceptable. Retained6 stays current.

#### Scope and integrity

All 16 original full-suite processes pass every value/read/write/cursor/prefix/borrow/owner/fresh-reopen oracle. Every immediate return boundary is quiet; no point is excluded or retried. Four paired blocks/filesystem use balanced ON-first/OFF-first order and fixture slots. Native context is retained without normalization. Quiet boundaries are not continuous host isolation.

ON is the original source-built ac080432… full ELF. OFF is a separate 6d5b9e54… copy with exactly byte 0xdc1fb 84→83: JE becomes JAE after unsigned byte compare against zero, forcing the existing Builder fallback. All other ELF bytes and linked addresses are identical; normal ASLR still applies. The OFF copy retains the original GNU build-id/embedded source metadata and is always identified by its distinct full-file SHA and diagnostic patch receipt. It is no rebuilt production artifact. Source 548/snapshot 480,34 current pins and 127 historical bindings/five prior campaign inventories remain exact. Actual native archive and TOML are unchanged and still strict. No SQLite workload is measured here.

All 8 ON reports have the actual pending→consumed states, 13584B→0 charged seed metadata, 2 families/6479 logical reads skipped/0 actual prefix reads/89 membership checks for one consuming Collect. All 8 OFF reports have the namespace explicitly null before and after. Only this namespace and the raw locator in a temporary audit copy are projected for original validators; stored reports stay exact. The counters are not lifetime/recovery I/O or physical memory peak measurements.

Independent offline verification reproduces all 204 full comparison cells (92 targets,82 controls,30 diagnostic metrics) and all 16 raw file hashes: 115,265,536 bytes,7,696 spans,14,408,192 samples and1,024 write spans. Every timer/read p50/p95/p99, write p99, original denominator,256-commit combined sum, complete byte coverage, resource schema, true global pool and maximum passes. All 16 manifest-recorded raw hashes were rechecked after completion. This audit executes no engine, build or profile.

#### Every full read: OFF versus ON

|FS/case|ON queries/s|OFF queries/s|OFF change|All 4 changes|ULMDB trial p99 change|LMDB rate change|
|---|---:|---:|---:|---|---:|---:|
|disk/ipv4_lpm_hits|3,199,009.852|3,195,109.383|-0.122%|-0.811%, -1.202%, +0.824%, +0.717%|+0.855%|-0.599%|
|disk/ipv4_lpm_inside_gap_misses|11,540,365.940|11,513,148.989|-0.236%|+1.895%, -0.522%, -2.825%, +0.570%|+0.729%|-4.181%|
|disk/ipv4_lpm_misses|21,501,134.374|21,532,154.694|+0.144%|+0.004%, +1.254%, -0.802%, +0.132%|+1.461%|-3.659%|
|disk/ipv4_lpm_mixed|5,517,566.861|5,519,112.671|+0.028%|+0.305%, -1.427%, +0.372%, +0.878%|-0.198%|-1.019%|
|disk/ipv4_lpm_outside_cover_misses|99,194,164.018|100,726,322.880|+1.545%|-0.032%, +3.657%, +2.960%, -0.345%|+0.653%|+0.105%|
|disk/ipv6_lpm_hits|2,486,538.407|2,486,743.807|+0.008%|+0.664%, +0.164%, +0.592%, -1.374%|+1.728%|-0.781%|
|disk/ipv6_lpm_inside_gap_misses|9,287,786.429|9,279,387.089|-0.090%|+0.891%, -1.342%, +0.050%, +0.052%|+2.935%|-0.115%|
|disk/ipv6_lpm_misses|17,174,234.364|17,159,341.183|-0.087%|+1.401%, +0.399%, -0.518%, -1.605%|+3.225%|-0.610%|
|disk/ipv6_lpm_mixed|4,320,696.208|4,305,489.610|-0.352%|+0.345%, +0.180%, +0.280%, -2.190%|+1.737%|-0.368%|
|disk/ipv6_lpm_outside_cover_misses|97,349,065.488|95,558,531.810|-1.839%|-1.049%, +0.219%, -0.025%, -6.355%|+2.402%|-1.102%|
|disk/ordered_kv_hits|7,405,497.125|7,468,236.458|+0.847%|+0.986%, -1.684%, +2.550%, +1.587%|+2.171%|+0.326%|
|disk/ordered_kv_misses|7,421,855.634|7,458,841.244|+0.498%|+0.886%, -2.533%, +2.194%, +1.513%|+1.743%|+0.338%|
|disk/ordered_kv_mixed|7,108,563.215|7,108,950.429|+0.005%|+0.628%, -2.710%, +1.462%, +0.695%|+2.928%|-0.442%|
|ram/ipv4_lpm_hits|3,196,363.596|3,189,614.646|-0.211%|-0.188%, +0.096%, -0.293%, -0.458%|+1.985%|-2.285%|
|ram/ipv4_lpm_inside_gap_misses|11,575,113.476|11,478,792.677|-0.832%|-0.572%, -2.893%, +1.445%, -1.260%|+1.092%|+0.515%|
|ram/ipv4_lpm_misses|21,581,638.913|21,521,870.055|-0.277%|-0.996%, +2.032%, +0.729%, -2.805%|+1.597%|-0.389%|
|ram/ipv4_lpm_mixed|5,504,758.232|5,482,835.225|-0.398%|+0.624%, -0.785%, -1.047%, -0.377%|+0.949%|-1.244%|
|ram/ipv4_lpm_outside_cover_misses|99,164,943.372|100,832,775.649|+1.682%|+0.062%, +4.299%, +2.440%, -0.011%|+0.591%|+0.434%|
|ram/ipv6_lpm_hits|2,496,978.825|2,496,069.002|-0.036%|+0.421%, +0.396%, -0.205%, -0.753%|+2.522%|-0.391%|
|ram/ipv6_lpm_inside_gap_misses|9,277,734.974|9,365,203.274|+0.943%|-1.911%, +3.930%, +1.901%, -0.055%|+3.246%|+0.250%|
|ram/ipv6_lpm_misses|17,259,128.664|17,164,788.449|-0.547%|+0.435%, +0.366%, -1.022%, -1.946%|+2.273%|+0.194%|
|ram/ipv6_lpm_mixed|4,330,958.851|4,325,214.266|-0.133%|+0.455%, +0.690%, -0.609%, -1.056%|+1.791%|+0.630%|
|ram/ipv6_lpm_outside_cover_misses|95,703,044.333|97,333,288.306|+1.703%|-0.238%, +3.913%, +3.945%, -0.711%|+0.966%|-0.084%|
|ram/ordered_kv_hits|7,402,946.175|7,406,454.915|+0.047%|+0.916%, +0.350%, -0.909%, -0.158%|+0.446%|-0.412%|
|ram/ordered_kv_misses|7,408,402.111|7,378,819.528|-0.399%|+0.222%, -1.028%, -1.100%, +0.318%|+1.478%|+0.372%|
|ram/ordered_kv_mixed|7,062,587.528|7,065,407.779|+0.040%|+1.424%, +1.103%, -2.178%, -0.148%|-0.237%|-0.449%|

Every read rate/p99/control mean, process value, interval and flag remains in the 204-cell table. Native changes are context, never a divisor used to remove candidate variation. Close mixed KV centers do not prove equal performance or eliminate the earlier unfavorable campaigns.

#### Every write rate and tail

|FS/batch/metric|ON center|OFF center|OFF change|All 4 changes|Native change|
|---|---:|---:|---:|---|---:|
|ram/1/throughput|79,141.941|61,195.328|-22.676%|-20.828%, -20.986%, -27.718%, -20.944%|-3.018%|
|ram/1/combined_throughput|86,182.351|73,864.949|-14.292%|-13.986%, -11.941%, -18.102%, -13.011%|-2.663%|
|ram/1/sampled_p99|11,842.262|12,145.688|+2.562%|+7.923%, -2.111%, +4.709%, +0.027%|+4.898%|
|ram/1/pooled_sampled_p99|15,595.233|16,175.221|+3.719%|+35.334%, -16.510%, +2.591%, -0.166%|-5.693%|
|ram/64/throughput|985,409.793|974,753.374|-1.081%|-0.789%, -0.086%, -2.545%, -0.890%|-1.431%|
|ram/64/combined_throughput|992,575.824|984,322.439|-0.832%|-0.657%, +0.107%, -1.929%, -0.837%|-1.397%|
|ram/64/sampled_p99|101,820.247|102,462.715|+0.631%|-1.151%, -2.448%, +3.975%, +2.280%|+2.828%|
|ram/64/pooled_sampled_p99|251,445.089|246,537.994|-1.952%|-1.021%, -0.968%, -5.774%, +0.063%|+4.737%|
|disk/1/throughput|16,203.976|15,563.857|-3.950%|-4.893%, -6.699%, +3.900%, -7.687%|+5.738%|
|disk/1/combined_throughput|16,580.653|16,367.176|-1.288%|+2.653%, -1.655%, +3.351%, -8.998%|+4.561%|
|disk/1/sampled_p99|82,297.937|79,328.452|-3.608%|-29.743%, -6.602%, -6.427%, +40.601%|-6.531%|
|disk/1/pooled_sampled_p99|235,976.296|236,217.262|+0.102%|-1.097%, -2.229%, -67.295%, +217.495%|+1.390%|
|disk/64/throughput|490,188.954|486,611.168|-0.730%|-1.607%, +0.166%, -1.309%, -0.158%|+1.291%|
|disk/64/combined_throughput|497,204.224|499,020.030|+0.365%|+2.935%, -0.961%, -0.131%, -0.337%|+2.172%|
|disk/64/sampled_p99|221,045.736|211,456.427|-4.338%|-21.344%, +16.060%, -3.114%, -5.314%|-5.048%|
|disk/64/pooled_sampled_p99|371,284.597|375,880.898|+1.238%|+4.628%, -1.736%, -0.931%, +3.133%|+0.314%|

RAM-single ON 79,141.941 versus OFF 61,195.328 records/s, combined ON 86,182.351 versus OFF 73,864.949. Native RAM-single OFF-context rate changes −3.018% aggregate/−2.663% combined; these remain without adjustment. RAM64 OFF−1.081% aggregate/−0.832% combined and disk-single mixed −3.950%/−1.288% are also retained.

#### Global sampled tails and maxima, separately from geometric process tails

|FS/batch/engine|Global 512 p99 ON→OFF ns|Maximum ON→OFF ns|Process p99 ON|Process p99 OFF|
|---|---:|---:|---|---|
|disk/1/ulmdb|313096→311182|571156→330608|[316091, 316391, 309899, 100050]|[312624, 309338, 101353, 317654]|
|disk/1/lmdb|327032→331591|334245→347441|[327162, 322323, 325439, 325098]|[329667, 324407, 332462, 331591]|
|disk/64/ulmdb|372248→374773|598990→591846|[365886, 381395, 372248, 365826]|[382818, 374773, 368781, 377287]|
|disk/64/lmdb|525038→528315|554264→555977|[527624, 518236, 523025, 525038]|[529006, 528315, 513597, 529707]|
|ram/1/ulmdb|16452→14438|19768→80302|[17864, 17293, 15860, 12073]|[24176, 14438, 16271, 12053]|
|ram/1/lmdb|8396→9478|14728→29897|[13195, 13356, 8075, 8035]|[8596, 8576, 14107, 8697]|
|ram/64/ulmdb|257690→247601|266176→265495|[245367, 247451, 260987, 252260]|[242861, 245056, 245917, 252420]|
|ram/64/lmdb|99910→106202|107714→114348|[100692, 102785, 93909, 98086]|[110239, 107014, 102815, 94580]|

ULMDB RAM-single global p99 improves 16.452→14.438µs while the maximum worsens 19.768→80.302µs. Disk-single global 313.096→311.182µs and maximum 571.156→330.608µs improve; disk64 global 372.248→374.773µs worsens while its maximum 598.990→591.846µs improves. Both favorable and adverse observations stand. No production-percentile or tail-equivalence claim.

#### Interpretation boundary and next evidence

The control holds emitted read/harness/native code and linked addresses fixed while changing execution of the seed path and its later observer/allocation/metadata/cache state. It does not change all possible common runtime layouts back to retained6. Lack of KV recovery therefore does not uniquely prove instruction placement or disprove every allocation/layout effect. A future placement-only experiment needs its own static semantics/relocation/unwind proof and fixed controlled measurements before any cause is asserted. The substantial write result is diagnostic evidence, not permission to accept the read-regressing candidate or change production code.

[All204 metrics and controls](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/same-elf-seed-control-plan-v1/controls-v1/analysis-v1/ALL_METRICS.md), [all values/paired intervals](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/same-elf-seed-control-plan-v1/controls-v1/analysis-v1/ON-vs-OFF.json), [raw samples/contexts/counter bindings](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/same-elf-seed-control-plan-v1/controls-v1/analysis-v1/raw-review.json), [global sampled pools](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/same-elf-seed-control-plan-v1/controls-v1/analysis-v1/global-write-tails.json), [independent audit receipt](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/same-elf-seed-control-plan-v1/controls-v1/analysis-v1/audit-receipt.json), [exact predeclared plan](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/same-elf-seed-control-plan-v1/controls-v1/PLAN_V1.md), [static one-byte proof](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/same-elf-seed-control-plan-v1/static-proof.json).



## Earlier measured candidate: lazy seeded cache REJECTED

Two complete unchanged 39-process campaigns repeat a material RAM-single benefit, but the fresh campaign loses 9.51–10.13% on RAM KV reads and 11.62–13.26% on disk KV reads, adverse in all four pairs for each hit/miss/mixed case. Root rejected this candidate for retention. The protected read cost is substantial; the source cause remains unconfirmed. Accepted iteration6, its scorecard and all published chart data remain current. No prototype gains are combined into an unmeasured version.

The full first and fresh populations follow, including aggregate and all 256-commit combined budgets, every read case, all write tails, direct signed LMDB percentages and exposure qualification. All 474 comparison cells and 103 native targets per population are linked without winner filtering.

### Lazy seeded cache: rejected after complete unchanged confirmation

**Decision: REJECT for retention; keep accepted iteration6.** The first campaign improves RAM-single aggregate throughput 24.752% and all 256-commit combined throughput 14.010%; the fresh confirmation repeats +24.239%/+13.459%, favorable in every one of the eight paired observations. The fresh campaign also loses 9.514–10.133% in RAM KV reads and 11.623–13.259% in disk KV reads, all four pairs adverse for each hit/miss/mixed case. That substantial protected read loss blocks retention despite the real RAM-write benefit. Source cause is unconfirmed. All earlier data and accepted6 assets remain unchanged.

#### Completeness and qualification

Both standard39 campaigns complete all 39 fresh engine processes, original semantic/cold/oracle checks, source/ELF/native/SQLite/observation guards and every 237×2 comparison. Each has 103 target/native rows, including unavailable native ratios where the native metric was not measured. Source548/snapshot480, full ac080432… and million e35226fb… remain identical. Current48 pins, historical40/21/9, confirmation9 and all four prior campaign inventories pass. No engine, test, build, retry, exclusion or native normalization was performed in this review.

**First campaign:** 35 observed-quiet and four exposed boundaries:001 accepted/full/disk/r0, 002 candidate/full/disk/r0,027 shared LMDB/million/r0 and029 shared LMDB/million/r1. Every RAM and ULMDB-million target/reference boundary is quiet. Direct million/native aggregates include two exposed native controls and remain descriptive.

**Fresh confirmation:** 33 observed-quiet and six exposed boundaries:001 accepted/full/disk/r0, 024 initial/million/r0,026 candidate/million/r0,028 SQLite/million/r0,033 initial/million/r1, 038 LMDB/million/r2. All RAM full boundaries are quiet. Disk comparisons include an exposed r0 reference; the other three disk KV pairs remain adverse by roughly9–15%. Fresh candidate/reference and direct-native million aggregates include exposures and are descriptive/unqualified as independent gain/loss or parity claims. No observation is removed. Quiet return boundaries do not prove continuous isolation.

Each raw audit verifies 24 files/172,898,304 bytes,11,544 spans,21,612,288 samples and1,536 write spans: exact hashes/full extent coverage, read p50/p95/p99, write p99, maxima, true global 512-sample pools and aggregate+sampled combined budgets. Empty adjusted regression-interval and consistent-regression flags do not establish equivalence or erase coherent observed costs.

#### Whole-campaign effects and every paired sign

|Metric|First C vs6|All first pairs|Fresh C vs6|All fresh pairs|All-pair descriptive change|
|---|---:|---|---:|---|---:|
|ram batch1 throughput|+24.752%|+24.522%, +21.816%, +25.521%, +27.212%|+24.239%|+27.189%, +23.643%, +21.057%, +25.146%|+24.495%|
|ram batch1 combined_throughput|+14.010%|+13.775%, +12.667%, +14.215%, +15.401%|+13.459%|+14.926%, +14.389%, +11.560%, +12.990%|+13.734%|
|ram batch1 sampled_p99|-5.426%|-1.627%, -9.444%, -3.171%, -7.253%|-2.559%|-2.341%, -10.555%, +0.562%, +2.627%|-4.003%|
|ram batch1 pooled_sampled_p99|-29.069%|-23.221%, -36.414%, -25.414%, -30.485%|+0.908%|-2.865%, +3.844%, +1.179%, +1.591%|-15.398%|
|ram batch64 throughput|-0.417%|-1.569%, -0.168%, +0.415%, -0.336%|-0.634%|-1.873%, +0.667%, -0.675%, -0.639%|-0.526%|
|ram batch64 combined_throughput|-0.321%|-0.692%, +0.002%, +0.112%, -0.704%|-0.640%|-1.213%, +0.323%, -0.925%, -0.736%|-0.481%|
|ram batch64 sampled_p99|-0.382%|+0.782%, -3.135%, -2.078%, +3.019%|-0.656%|-3.247%, +0.104%, -0.971%, +1.552%|-0.519%|
|ram batch64 pooled_sampled_p99|+0.831%|+1.138%, +2.576%, -0.027%, -0.335%|-1.816%|-6.649%, +1.298%, +0.700%, -2.406%|-0.501%|
|disk batch1 throughput|+7.720%|+8.650%, +5.904%, +5.479%, +10.938%|-5.418%|+7.153%, -32.115%, +4.090%, +5.692%|+0.937%|
|disk batch1 combined_throughput|+2.450%|+1.036%, +1.497%, +2.056%, +5.265%|-10.184%|+0.756%, -34.057%, -2.664%, +0.626%|-4.074%|
|disk batch1 sampled_p99|+7.456%|+18.964%, +4.273%, +6.096%, +1.306%|+18.229%|+16.241%, +21.541%, +22.765%, +12.651%|+12.714%|
|disk batch1 pooled_sampled_p99|+1.476%|+3.565%, +0.495%, +2.008%, -0.124%|+62.142%|+0.246%, +245.238%, +1.882%, +96.021%|+28.272%|
|disk batch64 throughput|+0.971%|-0.991%, -2.829%, -0.963%, +9.089%|-2.968%|-1.566%, -6.060%, -3.177%, -0.991%|-1.018%|
|disk batch64 combined_throughput|+18.161%|+90.817%, -1.502%, -0.403%, +4.138%|-1.149%|-0.194%, -3.976%, +0.331%, -0.700%|+8.076%|
|disk batch64 sampled_p99|+1.079%|-21.642%, +15.702%, +6.616%, +7.995%|-4.990%|-7.747%, +3.022%, -19.363%, +6.325%|-2.002%|
|disk batch64 pooled_sampled_p99|-46.146%|-95.319%, -0.191%, +15.944%, +55.291%|-11.458%|-37.353%, -0.535%, -3.516%, +2.228%|-30.947%|
|load/throughput|-0.835%|-2.860%, +3.111%, -2.643%|-9.225%|-8.404%, -8.742%, -10.516%|-5.123%|
|read/throughput|+3.342%|-3.426%, +20.679%, -5.302%|-5.434%|-7.954%, -6.157%, -2.095%|-1.143%|
|update/throughput|-1.540%|-2.765%, +0.076%, -1.909%|-2.812%|-7.191%, -0.766%, -0.324%|-2.178%|
|setup_ns|-0.048%|+4.199%, +0.261%, -4.416%|+5.328%|+6.017%, +7.944%, +2.107%|+2.605%|
|close_reopen_ns|-3.698%|-4.531%, -2.962%, -3.594%|-3.821%|-3.786%, -4.252%, -3.423%|-3.759%|
|storage_after_close_reopen/allocated_bytes|-0.001%|+0.000%, -0.002%, +0.000%|+0.000%|+0.000%, +0.000%, +0.000%|-0.000%|

The descriptive last column is the GM of all eight full or six million paired ratios, including exposures. It is not a new inference interval or replacement for either campaign. Disk-single fresh aggregate−5.418%/combined−10.184% includes one quiet r1 loss of −32.115%/−34.057%; the other aggregate pairs are favorable. Disk64 aggregate−2.968% is adverse in all four fresh pairs. Million load−9.225%, read−5.434% and setup+5.328% are adverse in all three fresh pairs, with exposed candidate/r0 retained.

#### Every full read case

|FS/case|First rate change|All first pairs|Fresh rate change|All fresh pairs|Fresh trial p99 change|
|---|---:|---|---:|---|---:|
|disk/ipv4_lpm_hits|+0.380%|+0.299%, +1.331%, +0.053%, -0.157%|-0.266%|-0.171%, +0.050%, -1.353%, +0.418%|+2.942%|
|disk/ipv4_lpm_inside_gap_misses|+0.009%|+0.153%, +1.487%, -1.356%, -0.228%|-0.257%|-0.158%, -0.413%, -0.245%, -0.213%|+1.158%|
|disk/ipv4_lpm_misses|-0.026%|-1.151%, +0.648%, +1.646%, -1.217%|-0.965%|-0.830%, -1.076%, -0.805%, -1.148%|+2.786%|
|disk/ipv4_lpm_mixed|+0.346%|+0.665%, +0.953%, +0.251%, -0.480%|-1.104%|+0.073%, -0.642%, -3.086%, -0.733%|+2.040%|
|disk/ipv4_lpm_outside_cover_misses|+0.002%|-0.018%, -0.087%, -0.021%, +0.133%|+0.198%|-2.685%, +3.705%, -0.119%, -0.006%|+0.249%|
|disk/ipv6_lpm_hits|+1.210%|+0.878%, +1.834%, +1.666%, +0.466%|+0.809%|+1.363%, +1.346%, -0.315%, +0.852%|+2.083%|
|disk/ipv6_lpm_inside_gap_misses|+0.289%|-0.600%, +2.894%, -0.139%, -0.953%|-0.286%|+1.310%, -0.702%, -1.432%, -0.302%|+1.583%|
|disk/ipv6_lpm_misses|+0.206%|+0.254%, +1.434%, -1.047%, +0.196%|-0.848%|-0.122%, -0.771%, -2.527%, +0.050%|+0.550%|
|disk/ipv6_lpm_mixed|+0.642%|-0.394%, +1.497%, +0.767%, +0.706%|-1.850%|+0.456%, -7.115%, -0.677%, +0.137%|+5.159%|
|disk/ipv6_lpm_outside_cover_misses|-0.147%|+0.005%, +0.008%, -0.032%, -0.570%|+0.645%|-0.109%, +3.774%, -0.942%, -0.078%|+1.342%|
|disk/ordered_kv_hits|-2.225%|+0.190%, -4.972%, -1.130%, -2.910%|-13.259%|-12.474%, -14.703%, -14.896%, -10.899%|+10.731%|
|disk/ordered_kv_misses|-2.941%|-0.918%, -5.598%, -2.105%, -3.082%|-12.788%|-11.241%, -13.573%, -15.018%, -11.260%|+11.265%|
|disk/ordered_kv_mixed|-2.416%|+0.053%, -5.338%, -1.419%, -2.877%|-11.623%|-10.344%, -12.499%, -14.444%, -9.110%|+11.743%|
|ram/ipv4_lpm_hits|+0.462%|+1.561%, -0.399%, +0.589%, +0.106%|+0.002%|-0.076%, -0.076%, +0.263%, -0.104%|-0.295%|
|ram/ipv4_lpm_inside_gap_misses|+0.016%|+0.251%, +0.552%, -0.328%, -0.409%|-0.592%|-0.358%, -0.668%, +0.006%, -1.342%|+0.492%|
|ram/ipv4_lpm_misses|-0.433%|+0.548%, -0.435%, -1.616%, -0.219%|-0.806%|-1.090%, -0.793%, -0.796%, -0.545%|-0.132%|
|ram/ipv4_lpm_mixed|-0.264%|+0.119%, -0.937%, +0.046%, -0.282%|-0.235%|+0.120%, -0.077%, -0.387%, -0.592%|-0.532%|
|ram/ipv4_lpm_outside_cover_misses|-1.441%|+1.288%, -0.123%, -9.453%, +3.010%|-3.384%|+0.018%, -13.278%, -0.013%, +0.473%|+0.342%|
|ram/ipv6_lpm_hits|+0.974%|+3.001%, -0.420%, +0.668%, +0.677%|+0.816%|+0.613%, +1.280%, +0.786%, +0.587%|-0.526%|
|ram/ipv6_lpm_inside_gap_misses|-0.209%|+0.913%, -1.913%, +0.359%, -0.173%|-0.204%|-0.657%, -0.483%, -0.262%, +0.590%|-1.845%|
|ram/ipv6_lpm_misses|+0.307%|+2.012%, -0.745%, +0.639%, -0.654%|+0.358%|+0.849%, +0.233%, -0.201%, +0.555%|-0.907%|
|ram/ipv6_lpm_mixed|+1.115%|+2.082%, +0.690%, +0.977%, +0.716%|+0.826%|-0.006%, +1.695%, +1.213%, +0.412%|-0.572%|
|ram/ipv6_lpm_outside_cover_misses|-2.025%|+0.104%, -3.816%, -4.108%, -0.202%|-0.038%|-0.399%, +0.328%, +0.182%, -0.262%|-1.018%|
|ram/ordered_kv_hits|-1.173%|+0.385%, -4.319%, +1.147%, -1.811%|-9.762%|-6.548%, -9.029%, -13.318%, -10.023%|+9.471%|
|ram/ordered_kv_misses|-1.678%|+1.834%, -5.126%, -0.783%, -2.507%|-10.133%|-6.941%, -8.847%, -14.097%, -10.489%|+9.313%|
|ram/ordered_kv_mixed|-1.577%|+1.559%, -5.305%, -0.438%, -1.996%|-9.514%|-5.738%, -7.983%, -13.488%, -10.660%|+9.475%|

KV fresh losses repeat in all four pairs for hits, misses and mixed queries on both filesystems. RAM is fully observed-quiet; disk r0 has an exposed accepted reference but all other disk pairs also lose substantially. First-campaign KV losses were smaller (disk−2.225…−2.941%; RAM−1.173…−1.678%) and remain visible; a descriptive two-campaign mean cannot turn the new coherent roughly10% loss into a marginal cost. The prefix read cases are mostly much closer to retained6, but no code-layout, compiler or host cause is inferred.

#### True global sampled tails and maxima

|Population|FS/batch|Global 512 p99 R→C ns|Maximum R→C ns|Process p99 R|Process p99 C|
|---|---|---:|---:|---|---|
|original|disk/1|310190→314358|316431→327502|[309969, 311562, 146148, 314748]|[321020, 313105, 149083, 314358]|
|original|disk/64|2586017→371387|8352932→608637|[7843063, 367549, 383189, 383759]|[367098, 366847, 444284, 595944]|
|original|ram/1|16631→12083|33493→20910|[16442, 18515, 16200, 16631]|[12624, 11773, 12083, 11561]|
|original|ram/64|253251→257590|268510→264814|[247450, 253251, 257660, 245126]|[250266, 259774, 257590, 244304]|
|confirmation|disk/1|310591→325729|322223→1959305|[309939, 310591, 313496, 158652]|[310701, 1072279, 319397, 310991]|
|confirmation|disk/64|386945→374152|642181→600512|[597236, 383469, 386945, 365134]|[374152, 381416, 373340, 373270]|
|confirmation|ram/1|16191→15629|32211→18726|[16090, 17974, 11962, 15780]|[15629, 18665, 12103, 16031]|
|confirmation|ram/64|259804→249585|265585→270835|[261758, 246388, 247521, 259804]|[244355, 249585, 249254, 253552]|

Fresh disk-single global p99 increases 4.874% (310.591→325.729µs), while the maximum rises 508.059% (322.223→1959.305µs). Candidate r1 process p99 is 1072.279µs, versus retained6 r1 310.591µs; that quiet-context spike remains, along with all other samples. Fresh RAM-single global p99 improves 3.471% (16.191→15.629µs), maximum 32.211→18.726µs. First RAM-single global 16.631→12.083µs also stays. First disk64 apparent pooled improvement includes the exposed reference/r0 maximum 8.352932ms and cannot qualify a general tail win. Trial-geometric, process-pooled-geometric and global pools are distinct; no production tail-equivalence claim is made.

#### Direct candidate versus contemporary LMDB, each population separate

Raw percent is C/native−1; higher throughput is better and lower time/space is better. All signed statuses and remaining gaps are in the unfiltered 103-row native scorecards. Within2% means descriptively roughly on par, never equivalence. Exposed million candidate/reference/native points qualify the appropriate ratios as described above.

|Metric|First C|First LMDB|Raw first C vsLMDB|Fresh C|Fresh LMDB|Raw fresh C vsLMDB|Direction|
|---|---:|---:|---:|---:|---:|---:|---|
|ram batch1 throughput|78,415.802|133,577.987|-41.296%|77,393.067|132,726.315|-41.690%|higher better|
|ram batch1 combined_throughput|86,040.426|137,577.263|-37.460%|85,045.949|137,519.630|-38.157%|higher better|
|ram batch64 throughput|974,480.121|739,189.292|+31.831%|975,339.480|746,930.532|+30.580%|higher better|
|ram batch64 combined_throughput|984,261.785|746,305.719|+31.885%|983,499.760|754,542.103|+30.344%|higher better|
|disk batch1 throughput|16,769.458|10,798.144|+55.299%|14,196.984|7,650.742|+85.564%|higher better|
|disk batch1 combined_throughput|16,711.843|11,350.470|+47.235%|14,387.648|8,713.637|+65.116%|higher better|
|disk batch64 throughput|488,498.990|179,905.486|+171.531%|464,835.641|178,107.251|+160.986%|higher better|
|disk batch64 combined_throughput|492,284.915|183,928.259|+167.651%|480,447.370|184,256.505|+160.749%|higher better|
|Million load records/s|1,362,819.399|1,612,055.515|-15.461%|1,266,373.059|1,520,784.715|-16.729%|higher better|
|Million read queries/s|2,689,976.936|1,675,800.473|+60.519%|2,667,427.091|1,644,331.414|+62.220%|higher better|
|Million update records/s|142,613.971|128,535.907|+10.953%|140,951.874|127,931.246|+10.178%|higher better|
|Setup ms|404.936|1.226|+32926.493%|434.247|0.944|+45899.076%|lower better|
|Drop+open ms|6,822.015|3.818|+178602.845%|6,821.874|3.945|+172803.408%|lower better|
|Allocated MiB|256.000|33.044|+674.718%|256.000|33.046|+674.688%|lower better|

Fresh rejected RAM-single aggregate remains 41.690% below LMDB (71.496% throughput increase needed), combined 38.157% below (61.700% increase needed). Fresh load is 16.729% below native (20.090% increase needed), with exposed context retained. Setup 434.247ms versus native 0.944ms is 459.991×; drop+open 6821.874ms versus 3.945ms is 1729.03×. Physical allocation remains 256MiB versus native 33.046MiB. These rejected-candidate measurements do not replace accepted6’s published scorecard. Recovery source is unchanged, so the repeat observed reopen reduction of 3.698%/3.821% is not an architectural reopen fix.

#### Limited lifecycle sum and mechanism observations

|Population|Retained6 task seconds|Candidate task seconds|LMDB task seconds|C cost vs6|C cost vsLMDB|
|---|---:|---:|---:|---:|---:|
|original|15.121|14.973|8.405|-0.978%|+78.140%|
|confirmation|15.117|15.143|8.480|+0.169%|+78.577%|

Each process sum includes setup+load+update+drop/open before taking the GM; reads, oracles and generation are excluded. Exposed contexts remain, and no aggregate budget offset justifies an individual protected read/write regression.

In both campaigns all eight full candidates start with pending=true, 2 families, 13584B charged seed/pending metadata and zero use; after the consuming successful Collect they have pending=false/consumed status, charge 0, skipped 2 families/6479 logical reads, actual prefix reads 0 and 89 membership checks. All three million KV-only observations are null. These are historical counters for that one consuming Collect, not lifetime/cold-reopen prefix I/O or cumulative warm-cache traversals. Original Capture/cache scratch is separate; two observations do not establish peak physical memory. Original report accounting, digests and cadence guards pass, but deleted fixtures do not prove whole-campaign decoded WAL-byte equality. The mechanism works as instrumented; its overall measured tradeoff fails retention. No runtime is promoted.

[Fresh all 103 targets/status/gaps](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/analysis-v3/report/NATIVE_SCORECARD.md), [fresh all 474 cells](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/analysis-v3/report/ALL_COMPARISONS.md), [all 237 paired signs, both campaigns](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/analysis-v3/report/ALL_PAIRED_SIGNS.md), [every metric object and original interval](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/analysis-v3/population-comparison.json), [fresh raw/source receipt](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/analysis-v3/review-receipt.json), [fresh exposure identities](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/analysis-v3/availability-review.json), [first complete review](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/analysis-v2/report/REVIEW.md), [first103 targets](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/analysis-v2/report/NATIVE_SCORECARD.md), [first474 cells](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/analysis-v2/report/ALL_COMPARISONS.md).


[Independent peer raw/provenance audit](benchmark/work/optimization-20261105/next-prefix-seeded-cache/retained6-v1/independent-two-campaign-audit-v1/REPORT.md) checks all 48 raw files and both complete populations without rerunning engines.


## Earlier measured candidate: per-write prefix certificate REJECTED

The complete amended 39 confirms RAM-single aggregate throughput +19.00% and all 256-commit combined throughput +8.59% versus retained6, both favorable in all four pairs. The fresh unchanged39 repeats +20.34% aggregate/+10.05% combined, all four favorable. Candidate is REJECTED for retention/unpromoted: fresh IPv4 hits fall 6.32% on disk and 5.84% on RAM in all four pairs, both with adjusted regression intervals. All disk full boundaries are observed quiet. The substantial protected read cost blocks promotion despite the real RAM-single gain. Both populations remain visible.

Coherent adverse estimates are load -2.51% in all three pairs, RAM KV hits -2.09%/mixed -2.30% in all four, and RAM64 aggregate -1.03% in all four. RAM-single trial p99 rises 6.36% in all four; true global 512 p99 rises 12.765→16.852us (+32.02%). Disk-single maximum rises 323.655→569.824us (+76.06%). Empty adjusted-regression flags do not establish equivalence or erase these observations.

**Original population:**

| Metric | Retained6 | Rejected candidate | Local LMDB | C vs6 | Raw C vsLMDB | Direction |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| RAM1 aggregate records/s | 61,888 | 73,646 | 130,780 | +19.00% | -43.69% | higher better |
| RAM1 COMBINED records/s | 74,963 | 81,400 | 134,513 | +8.59% | -39.49% | higher better |
| Million load records/s | 1,404,869 | 1,369,609 | 1,603,196 | -2.51% | -14.57% | higher better |
| Million reads queries/s | 2,780,072 | 2,743,596 | 1,661,299 | -1.31% | +65.15% | higher better |
| Million updates records/s | 139,458 | 146,302 | 128,921 | +4.91% | +13.48% | higher better |
| Setup ms | 402.764 | 419.005 | 1.182 | +4.03% | +35352.05% | lower better |
| Drop+open ms | 7,103.414 | 6,809.327 | 3.637 | -4.14% | +187107.58% | lower better |
| Allocated MiB | 256.000 | 256.000 | 33.044 | +0.00% | +674.72% | lower better |

All 39 semantic/source/native/raw guards pass: three original observations were adopted after a tools-only success-label guard repair, with 36 additional children and original timing scopes preserved. Thirty-eight return boundaries were observed quiet; only initial million/r0 step 024 is exposed and retained. The observed reopen -4.14% is not an architectural improvement: recovery source remains unchanged. No prior result is excluded, normalized or replaced. Retained6 and its accepted assets stay current.

[Complete interpretation/all reads and tails](benchmark/work/optimization-20261105/next-prefix-constructor-control/candidate-cold-prefix-v2/analysis-v2/report/REVIEW.md), [all 103 signed native/reference targets](benchmark/work/optimization-20261105/next-prefix-constructor-control/candidate-cold-prefix-v2/analysis-v2/report/NATIVE_SCORECARD.md), [all 474 comparison cells](benchmark/work/optimization-20261105/next-prefix-constructor-control/candidate-cold-prefix-v2/analysis-v2/report/ALL_COMPARISONS.md), [independent audit receipt](benchmark/work/optimization-20261105/next-prefix-constructor-control/candidate-cold-prefix-v2/analysis-v2/final-review-receipt.json).

**Fresh confirmation:** 39 new children; 33 quiet/six exposed boundaries. Steps 8/25/37 are accepted references, 27 native LMDB, 33 initial and 35 candidate. RAM reference comparisons include one exposed accepted process; fresh million aggregate/reference/native ratios include exposures and remain unqualified as new gain/loss claims. Every point stays; no native normalization. All disk full boundaries and full candidate/native boundaries are observed quiet. Boundary checks are not continuous isolation.

Fresh IPv4 mixed loses 5.24% disk/4.88% RAM in all pairs. Original KV costs reverse to favorable fresh centers, and original reopen -4.14% becomes fresh +0.75%; recovery source is unchanged. Load is -2.51% originally/-2.89% fresh, with the latter exposed context retained. RAM64 aggregate loses 1.03%/1.56%, all eight pairs adverse. Earlier RAM-global p99 +32.02% remains; fresh is +7.67%. Earlier disk1 maximum +76.06% remains even though the fresh maximum improves; fresh disk64 maximum reaches 785.033us.

| Fresh metric | Retained6 | Rejected candidate | Local LMDB | C vs6 | Raw C vsLMDB | Direction |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| RAM1 aggregate records/s | 62,921 | 75,718 | 130,303 | +20.34% | -41.89% | higher better |
| RAM1 COMBINED records/s | 75,792 | 83,405 | 134,718 | +10.05% | -38.09% | higher better |
| Million load records/s — exposed context | 1,403,397 | 1,362,911 | 1,503,689 | -2.88% | -9.36% | higher better |
| Million reads queries/s — exposed context | 2,769,272 | 2,782,873 | 1,663,000 | +0.49% | +67.34% | higher better |
| Million updates records/s — exposed context | 143,604 | 140,198 | 116,286 | -2.37% | +20.56% | higher better |
| Setup ms — exposed context | 410.334 | 422.357 | 1.462 | +2.93% | +28780.90% | lower better |
| Drop+open ms — exposed context | 7,108.098 | 7,161.354 | 4.673 | +0.75% | +153148.02% | lower better |
| Allocated MiB | 256.001 | 256.001 | 33.044 | +0.00% | +674.72% | lower better |

[Complete confirmation/side-by-side/all read cases](benchmark/work/optimization-20261105/next-prefix-constructor-control/candidate-cold-prefix-v2/analysis-v3/report/CONFIRMATION_REVIEW.md), [every paired sign for all 237 metrics](benchmark/work/optimization-20261105/next-prefix-constructor-control/candidate-cold-prefix-v2/analysis-v3/report/ALL_PAIRED_SIGNS.md), [fresh 103 signed native/reference targets](benchmark/work/optimization-20261105/next-prefix-constructor-control/candidate-cold-prefix-v2/analysis-v3/report/NATIVE_SCORECARD.md), [fresh 474 comparison cells](benchmark/work/optimization-20261105/next-prefix-constructor-control/candidate-cold-prefix-v2/analysis-v3/report/ALL_COMPARISONS.md), [every exposure](benchmark/work/optimization-20261105/next-prefix-constructor-control/candidate-cold-prefix-v2/analysis-v3/availability-review.json), [fresh audit receipt](benchmark/work/optimization-20261105/next-prefix-constructor-control/candidate-cold-prefix-v2/analysis-v3/final-review-receipt.json). All original source/native/raw/counter guards pass in both populations. Root rejected this exact per-write variant for retention; the read cause is unconfirmed. The later lazy seeded-cache variant is measured separately above and also rejected after confirmation. Retained6 and accepted charts remain current.


**Earlier measured RW3 full-image foundation: REJECT promotion / HOLD.** Its complete 39 passes correctness and source guards and substantially improves setup/reopen/allocation, but measured checkpoint work causes considerable write/load losses. All initial/retained6/native comparisons, all sampled tails, checkpoint costs and two exposed observations are preserved below. No new runtime is adopted; the accepted iteration6 scorecard remains byte-identical.

**Checkpoint profile complete:** RAM perturbation centers pass; million compiled-artifact gates fail. All 26 observations, two exposed native controls, adverse read pairs and outer residuals remain. These are wall intervals, not CPU or production shares.

## Latest measured architecture: RW3 full-image foundation HELD; no promotion

**The full 39 campaign is complete.** Setup improves 90.27%, reopen 95.73% and million physical allocation 88.58% versus contemporary retained6. That footprint is 11.50% below LMDB. But loading loses 47.80%, updates 21.76%, RAM-single combined throughput 70.38% and RAM64 combined 51.30%. These considerable losses reject promotion. Aggregate-only RAM singles (-0.77%) hide maintenance charged to the original sampled commits; both aggregate and 256-commit combined metrics lead below. Retained6 stays accepted.

All 39 semantic/source/ELF/native/SQLite/raw checks pass; qualification passed 832 tests across 108 groups and source 551/snapshot 483 remained unchanged. Thirty-seven boundaries were observed quiet. Exposed process 013 is initial/disk/r2; 023 is candidate/disk/r3. Keep both. All RAM and million target/native boundaries were observed quiet; disk centers include one exposed candidate/native point and the initial comparison also includes its exposed reference point.

|Metric|Retained6|Held RW3|Local LMDB|C change vs6|Native advantage|Status|Required increase/cost reduction|
|---|---:|---:|---:|---:|---:|---|---:|
|ram batch1 aggregate records/s|63,058|62,575|135,196|-0.77%|-53.72%|slower / worse|116.06%|
|ram batch1 COMBINED records/s|76,009|22,516|139,244|-70.38%|-83.83%|slower / worse|518.42%|
|ram batch64 aggregate records/s|990,701|496,966|754,443|-49.84%|-34.13%|slower / worse|51.81%|
|ram batch64 COMBINED records/s|998,853|486,481|757,805|-51.30%|-35.80%|slower / worse|55.77%|
|disk batch1 aggregate records/s|15,402|8,777|11,051|-43.01%|-20.58%|slower / worse|25.91%|
|disk batch1 COMBINED records/s|16,283|6,021|11,366|-63.02%|-47.03%|slower / worse|88.78%|
|disk batch64 aggregate records/s|494,496|139,969|179,377|-71.69%|-21.97%|slower / worse|28.15%|
|disk batch64 COMBINED records/s|503,708|144,642|182,566|-71.28%|-20.77%|slower / worse|26.22%|
|Million load records/s|1,399,603|730,614|1,561,772|-47.80%|-53.22%|slower / worse|113.76%|
|Million reads queries/s|2,803,538|2,787,038|1,709,820|-0.59%|+63.00%|faster / better|0.00%|
|Million updates records/s|145,553|113,884|128,507|-21.76%|-11.38%|slower / worse|12.84%|
|Setup ms|401.854|39.118|1.068|-90.27%|-3564.24%|slower / worse|97.27%|
|Drop+open ms|7,083.864|302.330|3.828|-95.73%|-7798.74%|slower / worse|98.73%|
|Allocated after reopen MiB|256.000|29.242|33.043|-88.58%|+11.50%|faster / better|0.00%|
|Apparent after reopen MiB|256.000|282.702|33.047|+10.43%|-755.46%|slower / worse|88.31%|

Positive native advantage means better throughput or lower cost; within2% is descriptive, not equivalence. Reopen still takes 78.99 times native and setup 36.64 times. Apparent length grows 10.43% to 282.702MiB and files double 2→4, even though physical allocation falls to 29.242MiB versus native 33.043MiB. RAM-single true global p99 grows 12.584us→1.931252ms; disk64 maximum reaches 14.465081ms. Every adverse cell stays.

The setup+load+update+reopen task budget sums each process first, then takes geometric means: retained6 15.070755s, candidate 10.493659s, native 8.426882s. Candidate cost is 30.37% lower than retained6 but 24.53% higher than native. This excludes reads/oracles/generation and cannot justify considerable losses in individual writes/loading.

**Actual maintenance:** each full process pays 4 checkpoints and 9583712 encoded image bytes, with 23.911–24.167ms RAM checkpoint time. Each million process pays 18 checkpoints and 413228512 encoded bytes; their times are 1.682272/1.568224/1.498580s. All work remains in normal operation timers. Initial image/recovery clearing are in setup/open but excluded from periodic counters. Logical discard spans and encoded image bytes are not physical-device traffic.

**The fixed26 checkpoint profile is now complete below.** Its RAM center gates pass; disk-million compiled-artifact gates fail and both exposed native controls remain. Persistent changed-page COW plus bounded metadata is the next architecture direction, with no measured gain or final design claim yet. No repeat selection, checkpoint on close, hidden maintenance or prototype promotion occurs.

[Complete decision/all13 reads/tails](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/DECISION.md), [both 237-row comparisons](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/FULL_TABLES.md), [all 103 direct-native targets](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/NATIVE_SCORECARD.md), [every checkpoint observation](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/CHECKPOINTS.md). The preceding architectural diagnosis remains source-bound in [the nine-process lifecycle audit](benchmark/work/optimization-20261105/next-reopen-allocation/current6-cost-audit-v1/COST_REPORT.md): its 2335 frames/335 full audits and 3.85GB logical archive materializations are explicitly old-J2 evidence, not the new bounded-tail work count.

Frozen held source: [checkpoint policy/stats](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/candidate/crates/ulmdb/src/resident/checkpoint.rs), [RW3 authority](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/candidate/crates/ulmdb-storage/src/resident/journal_v3/mod.rs). The full-image prototype is a measured foundation; it has not fixed all goals or replaced retained6.


**Iteration 7 is HELD and its four owned paths were rolled back.** Its private load-probe gain failed to reproduce in the complete 39-process main campaign. Historical held-7 measurements and the complete warmup diagnosis appear below; they do not replace the published iteration-6 scorecard or certify unrelated concurrent user changes.

**The Page-only candidate is also HELD after all 25 private processes:** load is 2.42% slower than the contemporary retained-6 arm and 10.46% below LMDB; setup and reopen costs also rise. The small update gain does not offset the failed load target. The sustained diagnostic still trails native by about 35% with every cost charged. These separate controls do not replace the retained-6 scorecard below.

**Earlier ownership control: REJECT the existing generic tracker.** Its same-ELF tracking-only RAM screen loses 76.01% single-write aggregate throughput and 74.75% batch64 throughput, adverse in all four pairs while retaining the same WAL history and full Collect cadence. The completed coarse profile also fails its 5% perturbation gate; it identifies the instrumented ordinary commit envelope as the next question, without establishing production phase shares. Both complete 14-process controls, every read/tail/resource value and operation counts appear below. No certificate issuer or runtime change is adopted.

**Publication diagnosis:** the compact counter-free profile still fails the 5% perturbation gate (native combined singles +7.66%). A narrower five-clock Journal control passes the declared primary-center gate (maximum 2.67%). Its measured single-write publication interval averages 5.080us versus 0.868us encoding. The subsequent O_DSYNC candidate is held after full39. This is wall-clock diagnosis, not a kernel fraction or demonstrated engine gain. All tables and limits appear below; retained 6 remains current.

**Pure hierarchical recipe scanner: HELD after the complete39 campaign.** Load +0.780% and RAM singles -0.378% versus retained6 do not justify RAM KV-read losses of 2.443-2.935%, all four pairs adverse, updates -1.313% or adverse disk64 sampled tails. All 39 correctness/provenance checks pass; 38 boundaries were observed quiet and one SQLite control remains explicitly exposed/unqualified. No observation is excluded or rerun; no root runtime change or candidate promotion occurs. Complete237-row and103-native tables appear below, separately from the unchanged retained6 scorecard.

**Decision: retain iteration 6 as the improved working baseline; the performance goal remains active.** All 807 workspace tests across 105 targets, all 39 primary processes and all 48 fixed confirmation processes pass. Every checked read, local validation and both durability fences remain. The user authorized assessing marginal read reductions against material improvements after three repeats. Repeated IPv6 hit/mixed throughput costs around 2.2-2.5% remain explicit tradeoffs, alongside 20.23% faster loading, 12.3-19.0% faster KV reads and improved RAM single writes versus iteration 3. This is not a universal no-regression or LMDB-parity claim.

**Remaining gaps versus contemporary LMDB:** loading is 9.28% behind and needs 10.22% more ULMDB throughput; RAM single-record writes are 54.06% behind and need 117.66% more throughput. Post-update reopening is about 1,821 times longer, setup 333.5 times longer and allocated files 7.75 times larger. RAM sampled write latencies also remain behind.

**Disk-tail caveat remains open:** the 12 confirmation pairs show disk single-write trial p99 +6.55% and process-pooled p99 +10.33%. One block contributes trial +23.30% and pooled +95.05%; the other two blocks differ in sign. Across all 1,536 sampled transactions per arm, >=250us observations rise 24 to 32, while >=500us remains two per arm and maxima are 577.157us versus 572.047us. These observations are retained; they do not establish tail equivalence or production percentiles. Full raw confirmation and native controls are in iteration-6-full-confirmation.

## Current work

Current integration V6 is held and unqualified after the same native compaction error-case failure on baseline and candidate. All private compatibility/test amendments and failed qualification contexts remain; no live runtime patch, actual reference preparation or A/A/C39 was run. Both sparse wall diagnoses are now closed as jointly unqualified: availability invalidates v1; numerical Native-single overhead and availability invalidate its one unchanged confirmation. No third repeat or overhead subtraction is authorized.

Next is source-only assessment of the old measured constructor proof against a variant preserving serving reader representation/dispatch, plus directory-metadata cloning constraints. Actual emitted-code and layout evidence is required before claiming a layout guarantee. The same-ELF ON/OFF control isolates the whole executing seed path and resulting runtime state; it does not isolate malloc, an individual Collect time or a unique placement cause. Its lack of coherent KV restoration and the later non-reproduced large R6/OFF gap do not erase the rejected candidates or prove equivalence. No new source capture, build, engine or predicted speedup is implied by these audits.

The current private three-cohort result does not solve remaining RAM single-write, load, setup, reopen or allocation goals. The recovery-local slot-position experiment and its three cohorts remain separate, unpromoted evidence. Earlier dirty-window, seed, capacity, current-image and transport results retain their rejection decisions. The 33-run native diagnosis preserves all failed perturbation gates and unknown CPU/wall remainders. No further implementation is selected from an unqualified profile or an unassigned whole-dump cost.

The held iteration-7 private load candidate removes two eager 296-byte Change-result tail copies before the no-split check. Generated code and matched dynamic counts confirm exactly 2,957,563 copies and 875,438,648 logical argument bytes removed, with unchanged memset/memmove and 4096-byte operations. Nine ordinary disk probe processes pass. Median paired throughput improves 3.75%; aggregate candidate throughput 1,453,002/s is still 8.64% below the accompanying native LMDB 1,590,466/s. The complete isolated 39-process main campaign then measured load -0.31% and million reads -2.88% versus retained 6, with a repeatable disk-KV-hit cost of -1.62%; the small write gains did not earn retention. The four owned paths were rolled back, with every unrelated user edit preserved. Exact mechanism counts alone did not predict main performance.

Nine same-ELF order controls confirm that the forward single-write case pays the first cold Collect and that reversing the order shifts it into batch64. Reverse single writes still trail native by 31.35% aggregate / 26.49% combined, and total charged time across both cases changes only +0.17%. The completed sustained-single control remains about 34% behind native in its final four positions and 35% behind over all charged commits. The quiet census confirms exact operation counts but fails its 5% recording-overhead gate. No first-use cost is dropped or hidden by priming.

Separate private Capture-proof collection and cold-only replay-index candidates have passed their focused controls. Capture improves RAM single-write aggregate throughput 5.38%; replay index reduces close/reopen 21.50% but leaves setup +2.47% and update throughput -0.27% adverse estimates. Both require a complete retained baseline gate before integration. Compact endpoint-audit kernels improve dense pages but show fallback-size regressions; all three revisions remain private/held.

## Historical matched profiling

Earlier matched profiling completed 18 diagnostic processes, all oracles passing. Values below are instrumented nanoseconds per application record, not accepted throughput:

|Workload / phase|ULMDB ns/record|LMDB ns/record|Interpretation|
|---|---:|---:|---|
|RAM singles / begin including collection|1,169|78|Transaction setup and maintenance cost|
|RAM singles / mutation|799|821|Mutation is comparable|
|RAM singles / commit|9,565|5,985|Publication is a remaining target|
|Sorted load / mutation|1,066|304|127.8 million complete-order comparisons identified|
|Sorted load / commit|143|337|ULMDB commit is cheaper in this diagnostic|
|Shuffled update / begin including collection|7,300|1|333 collections, about 1 billion comparisons|
|Shuffled update / mutation|1,376|1,587|ULMDB mutation is cheaper in this diagnostic|
|Shuffled update / commit|1,139|6,272|ULMDB redo preserves an I/O advantage|

The update syscall trace wrote 106,352,624 bytes for ULMDB versus 3,936,212,160 for LMDB. Preserve that benefit while reducing collection CPU. Source and actual traces also confirm LMDB combines metadata writing and its fence using an O_DSYNC descriptor. Its separately qualified O_DSYNC transport candidate completed39 processes and is held for insufficient write benefit and adverse reads/batches. Full matched evidence: [MATCHED_PROFILE_REPORT.md](benchmark/work/optimization-20261105/lmdb-analysis/matched-20261105/MATCHED_PROFILE_REPORT.md).

## Fresh post-return-copy profile: held private source, diagnostic only

This profile describes the private explicit-return arm later held as iteration 7; it is not a profile of the currently retained-6 source. The complete checked 1M-key workload and all native/source/ELF/phase guards pass. Selected Resident samples total 635, native 392, with zero dropped. The exact compact-validation PC union has 177/635 samples (27.87%); its overlapping tail subset has 53 (8.35%), cell-layout subset 42 (6.61%) and key-compare subset 33 (5.20%). Resolved copy/move has 18/635 (2.83%) and memcmp/bcmp 41/635 (6.46%). Resolved targets are verified from actual GLOB_DAT pointers, bytes and disassembly; nearest stripped-libc NSS names are not used as attribution.

These are interrupted user-PC proportions, not wall-time fractions, caller stacks or expected speedups. Inline subsets overlap and cannot be added. Sampling is finite and ITIMER_PROF may coalesce. Shared/unknown PCs remain in each denominator. All exact classes and native observations follow:

### Complete post-copy-removal profile

Exactly two frozen normal-durability1M/batch1000 loads pass every source/ELF/oracle/phase/native/stats/module/IFUNC guard, with no retries or exclusions. No accepted throughput claim. Only measurement_begin≤sample<measurement_end is selected; all selected samples, including shared/unknown PCs, remain in the denominator.

Actual memcpy/memmove,memset,memcmp/bcmp target GLOB_DAT pointers were read from each target before measurement. They resolve to libc offsets0x188cf0,0x189710,0x188620 respectively, with the same digest/disassembly as the retained region proof. Stripped-libc nearest NSS symbol names are not treated as NSS activity.

## 01-explicit

Selected 635 load samples, 1876 complete-process samples, zero dropped. Requested interval1000us; ITIMER_PROF delivery may coalesce.

| Validation/inline category (overlaps possible) | Samples | Selected share |
|---|---:|---:|
| validate_compact_PC_union | 177 | 27.87% |
| validate_compact_tail_source130_union | 53 | 8.35% |
| validate_compact_layout_cell_inline_union | 42 | 6.61% |
| validate_compact_key_compare_inline_union | 33 | 5.20% |

| Flat interrupted user-PC class | Samples | Selected share |
|---|---:|---:|
| <core::result::Result<T,E> as core::ops::try_trait::Try>::branch | 52 | 8.19% |
| core::num::<impl usize>::checked_sub | 48 | 7.56% |
| <core::slice::iter::Iter<T> as core::iter::traits::iterator::Iterator>::fold | 46 | 7.24% |
| libc unresolved PC (no nearest-symbol claim) | 42 | 6.61% |
| resolved memcmp/bcmp implementation region | 41 | 6.46% |
| core::num::<impl u64>::swap_bytes | 36 | 5.67% |
| ulmdb_kv::page::Page::validate_compact | 27 | 4.25% |
| ulmdb_kv::compact::search_word_rows | 26 | 4.09% |
| ulmdb_kv::page::Page::decode | 20 | 3.15% |
| ulmdb_storage::resident::private_workspace::indexed::Buffers::position | 20 | 3.15% |
| resolved copy/move implementation region | 18 | 2.83% |
| ulmdb_kv::write::change | 18 | 2.83% |
| ulmdb_storage::resident::pool::check | 18 | 2.83% |
| <ulmdb_storage::resident::store::Arena as ulmdb_core::storage::ReadStore>::read | 15 | 2.36% |
| core::cmp::Ordering::is_ge | 15 | 2.36% |
| ulmdb_kv::write::change_leaf | 14 | 2.20% |
| core::result::Result<&T,E>::copied | 8 | 1.26% |
| ulmdb_storage::resident::store::patch_many::<impl ulmdb_storage::resident::store::Arena>::patch_many_page | 6 | 0.94% |
| <ulmdb_storage::resident::store::Arena as ulmdb_core::storage::WriteStore>::try_patch_many | 6 | 0.94% |
| core::option::Option<T>::is_some_and | 6 | 0.94% |
| core::num::<impl usize>::checked_add | 6 | 0.94% |
| <u8 as core::ops::bit::BitOr>::bitor | 6 | 0.94% |
| ulmdb_storage::resident::pool::check::{{closure}} | 5 | 0.79% |
| ulmdb_kv::page::Page::compact | 5 | 0.79% |
| ulmdb_kv::write::plan | 5 | 0.79% |
| ulmdb_kv::compact::search_word_records | 4 | 0.63% |
| ulmdb_core::endian::decode_reference | 4 | 0.63% |
| ulmdb_kv::write::put | 4 | 0.63% |
| ulmdb_kv::compact::Canonical::new | 4 | 0.63% |
| core::core_arch::x86::sse2::_mm_add_epi32 | 4 | 0.63% |
| core::ptr::write | 4 | 0.63% |
| ulmdb_core::compact_page_geometry::valid_geometry | 4 | 0.63% |
| ulmdb_storage::resident::pool::Permit::write | 4 | 0.63% |
| core::ptr::drop_in_place<ulmdb_kv::write::Change> | 3 | 0.47% |
| ulmdb_kv::compact::Layout::capacity | 3 | 0.47% |

## 02-native

Selected 392 load samples, 1416 complete-process samples, zero dropped. Requested interval1000us; ITIMER_PROF delivery may coalesce.

| Validation/inline category (overlaps possible) | Samples | Selected share |
|---|---:|---:|

| Flat interrupted user-PC class | Samples | Selected share |
|---|---:|---:|
| libc unresolved PC (no nearest-symbol claim) | 87 | 22.19% |
| resolved memcmp/bcmp implementation region | 83 | 21.17% |
| mdb_node_search | 46 | 11.73% |
| mdb_cmp_memn | 26 | 6.63% |
| mdb_node_add | 19 | 4.85% |
| _mdb_cursor_put | 17 | 4.34% |
| mdb_page_search_root | 16 | 4.08% |
| mdb_page_get | 15 | 3.83% |
| mdb_page_spill | 14 | 3.57% |
| mdb_cursor_set | 10 | 2.55% |
| mdb_cursor_init | 9 | 2.30% |
| mdb_put | 6 | 1.53% |
| mdb_mid2l_search | 6 | 1.53% |
| resolved copy/move implementation region | 6 | 1.53% |
| mdb_page_split | 6 | 1.53% |
| mdb_page_search | 5 | 1.28% |
| mdb_page_touch | 5 | 1.28% |
| mdb_cursor_push | 3 | 0.77% |
| memcpy | 3 | 0.77% |
| resolved memset implementation region | 2 | 0.51% |
| mdb_dbis_update | 2 | 0.51% |
| <core::result::Result<T,E> as core::ops::try_trait::Try>::branch | 1 | 0.26% |
| ulmdb_lmdb_baseline::transaction::WriteTransaction::put | 1 | 0.26% |
| ulmdb_lmdb_baseline::check | 1 | 0.26% |
| ulmdb_profile_probe::native::run | 1 | 0.26% |
| mdb_cursor_touch | 1 | 0.26% |
| ulmdb_lmdb_baseline::transaction::val | 1 | 0.26% |

These are interrupted user-PC proportions, not wall-time shares, kernel stacks, runtime caller stacks, expected speedups or nonregression evidence. Inline-inclusive categories can overlap and must not be added. A single635-sample Resident load has finite sampling uncertainty; raw PCs/full inline DWARF and all other categories remain in attribution.json and original reports. Current material validation cost can motivate a separately approved controlled strategy; this profile does not authorize validator code or removal of any check.


## Historical iteration 7: HELD; private gain failed the main campaign

All 39 processes passed, using the original sealed release ELFs and an isolated exact 472-file source tree. Native flags, current SQLite loader/version/source/path/hash, all source/ELF/tool guards and every oracle passed. The independent review recomputed both 237-row comparisons and checked all 24 raw files, 172,898,304 bytes and 21,612,288 samples, including complete span coverage, every trial p99 and combined write totals.

**Do not retain iteration 7.** The separate private probe gained 3.75%, but main loading changed -0.31% versus retained 6, with all three pairs adverse; million reads changed -2.88%. RAM singles gained only 0.78% aggregate / 0.48% combined. Disk singles gained 3.63% aggregate / 1.37% combined, while disk KV hits lost 1.62% in all four pairs (familywise-adjusted ratio interval 0.972125-0.995550). Disk KV misses/mixed lost 1.44%/1.10%. This main result provides no large gain to justify those costs.

The ownership-scoped rollback restored only `crates/ulmdb-kv/src/write.rs` to retained 6 and removed its three newly added return-test files. The actual [post-rollback verification](benchmark/work/optimization-20261105/next-change-return/candidate7-explicit-v1/retained6-to-explicit-package-v1/post-rollback-verification.json) confirms all four preimages; concurrent coordination/C API and other user work were preserved. The installer inventory alone is not an execution receipt. Retained 6 remains the optimization baseline; this does not assert that the whole concurrently edited workspace is identical to its historical source.

Every target cell follows. Baseline is the contemporaneous retained-6 arm; candidate is historical held 7. Positive native advantage means more throughput or less cost. Within 2% is a descriptive roughly-on-par label, not equivalence. All adverse values remain; unavailable native lifecycle observations are explicitly unmeasured.

### Throughput, including all 13 reads per filesystem and charged write windows

|Metric|Unit|Retained 6|Held 7|7 vs 6|Local LMDB|7 native advantage|Status|
|---|---|---:|---:|---:|---:|---:|---|
|full/disk/ulmdb/durable_batch1/combined_throughput|records/s|16,399|16,624|+1.37%|11,683|+42.29%|faster|
|full/disk/ulmdb/durable_batch1/throughput|records/s|15,713|16,284|+3.63%|11,073|+47.06%|faster|
|full/disk/ulmdb/durable_batch64/combined_throughput|records/s|503,003|509,853|+1.36%|181,498|+180.91%|faster|
|full/disk/ulmdb/durable_batch64/throughput|records/s|494,172|494,705|+0.11%|177,354|+178.94%|faster|
|full/disk/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,210,615|3,203,758|-0.21%|2,137,349|+49.89%|faster|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,626,961|11,628,319|+0.01%|2,009,018|+478.81%|faster|
|full/disk/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,712,360|21,559,925|-0.70%|3,836,159|+462.02%|faster|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,541,807|5,506,085|-0.64%|2,663,620|+106.71%|faster|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|100,918,359|100,953,379|+0.03%|92,756,444|+8.84%|faster|
|full/disk/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,472,963|2,518,741|+1.85%|1,256,267|+100.49%|faster|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,377,790|9,355,794|-0.23%|1,141,495|+719.61%|faster|
|full/disk/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,141,587|17,324,521|+1.07%|2,255,528|+668.09%|faster|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,301,928|4,382,388|+1.87%|1,578,269|+177.67%|faster|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|96,381,269|96,746,858|+0.38%|90,994,299|+6.32%|faster|
|full/disk/ulmdb/ordered_kv_hits/throughput|queries/s|7,615,610|7,491,989|-1.62%|4,192,215|+78.71%|faster|
|full/disk/ulmdb/ordered_kv_misses/throughput|queries/s|7,638,029|7,528,038|-1.44%|4,042,580|+86.22%|faster|
|full/disk/ulmdb/ordered_kv_mixed/throughput|queries/s|7,250,000|7,170,447|-1.10%|4,092,944|+75.19%|faster|
|full/ram/ulmdb/durable_batch1/combined_throughput|records/s|76,644|77,012|+0.48%|140,347|-45.13%|slower|
|full/ram/ulmdb/durable_batch1/throughput|records/s|63,670|64,169|+0.78%|136,791|-53.09%|slower|
|full/ram/ulmdb/durable_batch64/combined_throughput|records/s|996,933|1,007,778|+1.09%|756,529|+33.21%|faster|
|full/ram/ulmdb/durable_batch64/throughput|records/s|989,336|1,000,193|+1.10%|750,791|+33.22%|faster|
|full/ram/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,213,751|3,212,144|-0.05%|2,133,667|+50.55%|faster|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,664,238|11,618,229|-0.39%|2,020,941|+474.89%|faster|
|full/ram/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,650,990|21,582,359|-0.32%|3,878,823|+456.42%|faster|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,540,459|5,531,768|-0.16%|2,673,832|+106.89%|faster|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|100,392,637|99,691,158|-0.70%|92,737,431|+7.50%|faster|
|full/ram/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,472,747|2,532,850|+2.43%|1,269,334|+99.54%|faster|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,409,426|9,363,903|-0.48%|1,150,860|+713.64%|faster|
|full/ram/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,275,706|17,358,236|+0.48%|2,268,234|+665.28%|faster|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,298,229|4,400,518|+2.38%|1,594,566|+175.97%|faster|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|97,342,051|97,318,044|-0.02%|91,539,714|+6.31%|faster|
|full/ram/ulmdb/ordered_kv_hits/throughput|queries/s|7,538,940|7,484,695|-0.72%|4,206,790|+77.92%|faster|
|full/ram/ulmdb/ordered_kv_misses/throughput|queries/s|7,582,978|7,546,886|-0.48%|4,039,619|+86.82%|faster|
|full/ram/ulmdb/ordered_kv_mixed/throughput|queries/s|7,216,776|7,174,555|-0.59%|4,115,336|+74.34%|faster|
|millions/disk/ulmdb/load/throughput|operations/s|1,403,718|1,399,332|-0.31%|1,551,855|-9.83%|slower|
|millions/disk/ulmdb/read/throughput|operations/s|2,901,946|2,818,323|-2.88%|1,702,565|+65.53%|faster|
|millions/disk/ulmdb/update/throughput|operations/s|146,442|146,945|+0.34%|129,251|+13.69%|faster|

### All trial-sampled and process-pooled p99 costs

|Metric|Unit|Retained 6|Held 7|7 vs 6|Local LMDB|7 native advantage|Status|
|---|---|---:|---:|---:|---:|---:|---|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99|ns|309,315|311,254|+0.63%|326,322|+4.62%|faster / lower cost|
|full/disk/ulmdb/durable_batch1/sampled_p99|ns|80,772|85,828|+6.26%|180,541|+52.46%|faster / lower cost|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99|ns|467,368|371,433|-20.53%|518,484|+28.36%|faster / lower cost|
|full/disk/ulmdb/durable_batch64/sampled_p99|ns|212,611|206,924|-2.67%|475,109|+56.45%|faster / lower cost|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99|ns|429|427|-0.50%|775|+44.84%|faster / lower cost|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|121|122|+0.53%|620|+80.33%|faster / lower cost|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99|ns|121|122|+0.34%|615|+80.19%|faster / lower cost|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|418|421|+0.76%|769|+45.27%|faster / lower cost|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|42|41|-0.65%|45|+9.14%|faster / lower cost|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99|ns|533|528|-1.04%|1,232|+57.17%|faster / lower cost|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|142|142|+0.05%|1,079|+86.82%|faster / lower cost|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99|ns|144|143|-0.45%|1,049|+86.37%|faster / lower cost|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|517|513|-0.77%|1,218|+57.89%|faster / lower cost|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|42|41|-1.32%|44|+5.72%|faster / lower cost|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99|ns|199|202|+1.30%|335|+39.86%|faster / lower cost|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99|ns|198|201|+1.31%|343|+41.43%|faster / lower cost|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99|ns|204|205|+0.51%|340|+39.82%|faster / lower cost|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99|ns|12,604|13,953|+10.70%|9,005|-54.94%|slower / higher cost|
|full/ram/ulmdb/durable_batch1/sampled_p99|ns|11,374|11,510|+1.19%|7,847|-46.67%|slower / higher cost|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99|ns|248,818|245,667|-1.27%|101,433|-142.20%|slower / higher cost|
|full/ram/ulmdb/durable_batch64/sampled_p99|ns|101,208|101,222|+0.01%|92,123|-9.88%|slower / higher cost|
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99|ns|431|428|-0.60%|781|+45.17%|faster / lower cost|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|121|122|+0.42%|584|+79.12%|faster / lower cost|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99|ns|121|123|+1.36%|597|+79.48%|faster / lower cost|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|419|420|+0.04%|766|+45.22%|faster / lower cost|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41|41|+0.31%|45|+8.91%|faster / lower cost|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99|ns|527|529|+0.36%|1,202|+55.97%|faster / lower cost|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|142|142|-0.05%|1,074|+86.75%|faster / lower cost|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99|ns|142|143|+0.19%|1,052|+86.43%|faster / lower cost|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|518|511|-1.37%|1,199|+57.37%|faster / lower cost|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41|41|-0.03%|43|+4.87%|faster / lower cost|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99|ns|202|201|-0.15%|340|+40.80%|faster / lower cost|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99|ns|200|199|-0.45%|345|+42.28%|faster / lower cost|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99|ns|203|205|+0.97%|342|+40.02%|faster / lower cost|

### All remaining lifecycle, setup and file-resource costs

|Metric|Unit|Retained 6|Held 7|7 vs 6|Local LMDB|7 native advantage|Status|
|---|---|---:|---:|---:|---:|---:|---|
|full/disk/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864|67,108,864|+0.00%|729,088|-9104.49%|slower / higher cost|
|full/disk/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864|67,108,864|+0.00%|733,184|-9053.07%|slower / higher cost|
|full/disk/ulmdb/footprint/after/files|files|2|2|+0.00%|2|-0.00%|roughly on par|
|full/disk/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864|67,108,864|+0.00%|352,256|-18951.16%|slower / higher cost|
|full/disk/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864|67,108,864|+0.00%|356,352|-18732.18%|slower / higher cost|
|full/disk/ulmdb/footprint/before/files|files|2|2|+0.00%|2|-0.00%|roughly on par|
|full/disk/ulmdb/reopen/complete_cold_check_ns|ns|94,530,773|93,945,349|-0.62%|unavailable|not measured|not measured|
|full/disk/ulmdb/reopen/environment_drop_ns|ns|95,647|95,058|-0.62%|unavailable|not measured|not measured|
|full/disk/ulmdb/reopen/experimental_reenable_ns|ns|2,189|2,353|+7.49%|unavailable|not measured|not measured|
|full/disk/ulmdb/reopen/open_ns|ns|84,087,304|83,393,691|-0.82%|unavailable|not measured|not measured|
|full/disk/ulmdb/reopen/validation_ns|ns|6,096,198|6,256,675|+2.63%|unavailable|not measured|not measured|
|full/disk/ulmdb/setup/create|ns|104,053,016|103,764,229|-0.28%|203,142|-50979.54%|slower / higher cost|
|full/ram/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864|67,108,864|+0.00%|729,088|-9104.49%|slower / higher cost|
|full/ram/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864|67,108,864|+0.00%|733,184|-9053.07%|slower / higher cost|
|full/ram/ulmdb/footprint/after/files|files|2|2|+0.00%|2|-0.00%|roughly on par|
|full/ram/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864|67,108,864|+0.00%|352,256|-18951.16%|slower / higher cost|
|full/ram/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864|67,108,864|+0.00%|356,352|-18732.18%|slower / higher cost|
|full/ram/ulmdb/footprint/before/files|files|2|2|+0.00%|2|-0.00%|roughly on par|
|full/ram/ulmdb/reopen/complete_cold_check_ns|ns|53,298,277|52,731,561|-1.06%|unavailable|not measured|not measured|
|full/ram/ulmdb/reopen/environment_drop_ns|ns|89,329|85,649|-4.12%|unavailable|not measured|not measured|
|full/ram/ulmdb/reopen/experimental_reenable_ns|ns|1,999|2,091|+4.58%|unavailable|not measured|not measured|
|full/ram/ulmdb/reopen/open_ns|ns|42,870,830|42,404,400|-1.09%|unavailable|not measured|not measured|
|full/ram/ulmdb/reopen/validation_ns|ns|6,105,207|6,074,419|-0.50%|unavailable|not measured|not measured|
|full/ram/ulmdb/setup/create|ns|48,515,429|46,975,641|-3.17%|124,018|-37777.95%|slower / higher cost|
|millions/disk/ulmdb/close_reopen_ns|ns|7,023,975,214|6,787,951,552|-3.36%|3,853,812|-176036.03%|slower / higher cost|
|millions/disk/ulmdb/setup_ns|ns|397,330,237|402,155,765|+1.21%|1,207,653|-33200.60%|slower / higher cost|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|bytes|268,435,456|268,435,456|+0.00%|34,648,064|-674.75%|slower / higher cost|
|millions/disk/ulmdb/storage_after_close_reopen/apparent_bytes|bytes|268,435,456|268,435,456|+0.00%|34,652,160|-674.66%|slower / higher cost|
|millions/disk/ulmdb/storage_after_close_reopen/files|files|2|2|+0.00%|2|-0.00%|roughly on par|
|millions/disk/ulmdb/storage_before_close/allocated_bytes|bytes|268,435,456|268,435,456|+0.00%|34,648,064|-674.75%|slower / higher cost|
|millions/disk/ulmdb/storage_before_close/apparent_bytes|bytes|268,435,456|268,435,456|+0.00%|34,652,160|-674.66%|slower / higher cost|
|millions/disk/ulmdb/storage_before_close/files|files|2|2|+0.00%|2|-0.00%|roughly on par|

Disk single-write trial p99 increased 6.26% and process-pooled p99 0.63%; RAM single-write pooled p99 increased 10.70% with mixed process signs. Candidate disk-single maximum was 315.260us versus retained-control 577.688us; fewer largest spikes do not erase the trial-p99 increase. Candidate RAM-single pooled values were 11.952-16.231us versus 11.492-15.340us. Every process/maximum/offset and native control is retained in the [independent decision review](benchmark/work/optimization-20261105/iteration-7-independent-review/DECISION.md). All original and uncertainty-bearing cells remain in the [complete comparison](benchmark/work/optimization-20261105/iteration-7-three-arm-isolated/accepted-vs-candidate.md).

## Warmup and write-history diagnosis: all costs remain charged

These nine uninstrumented full-process controls diagnose order/history in a private held-7 source mirror; they do not replace the retained-6 qualification. P is pristine, D is forward [1,64], R is reverse [64,1]; D/R use one identical ELF. Reversing order moves the first cold Collect and changes allocation, cache and native history as well. It is not evidence of pure CPU-cache warmup.

### Completed nine-process write-order control

All nine fixed RAM processes passed full15 semantic, raw/slice/source/ELF/native/trace/cold and boundary guards. No retries, exclusions or added warmup. D/R use the same diagnostic ELF; P is unchanged pristine ce70. No engine or validator source changed.

**The forward single-write aggregate is inflated by the first cold Collect.** P and D reach it at batch1 trial15 aggregate transaction5 in every run. Reversing order moves that cold Collect to batch64 trial1 sampled transaction1; batch1 later pays a warm Collect at trial10 aggregate transaction5. All9 runs still pay11 Collects total. Root/runtime were not altered.

R improves RAM1 by moving maintenance/history cost: D16.793µs/transaction aggregate becomes R11.747µs. However R remains31.35% slower than native in aggregate throughput, and26.49% slower with all256 commits combined. Total charged ULMDB time across BOTH cases is D19.836ms versus R19.870ms (+0.17%); the total cost is largely redistributed. This is an order/history effect, not proof of CPU-cache warmup alone.

## Absolute costs and direct native comparison

Rates aggregate all three processes. Aggregate means cover128 commits per case; separately sampled means cover the other128 existing timer-inclusive commits. Combined includes all256. Native throughput percentages compare the same arm/batch population.

| Arm | Batch | ULM aggregate µs/tx | ULM sampled µs/tx | ULM combined µs/tx | Native aggregate µs/tx | Native combined µs/tx | ULM aggregate records/s | ULM combined records/s | Aggregate vs native | Combined vs native |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|---|
| P | 1 | 16.873 | 10.551 | 13.712 | 7.777 | 7.424 | 59,267 | 72,929 | 53.91% slower | 45.85% slower |
| P | 64 | 66.381 | 64.918 | 65.649 | 87.519 | 86.379 | 964,136 | 974,876 | 31.84% faster | 31.58% faster |
| D | 1 | 16.793 | 10.484 | 13.639 | 7.570 | 7.285 | 59,547 | 73,320 | 54.92% slower | 46.59% slower |
| D | 64 | 64.382 | 63.308 | 63.845 | 86.977 | 85.966 | 994,061 | 1,002,423 | 35.09% faster | 34.65% faster |
| R | 1 | 11.747 | 9.900 | 10.824 | 8.065 | 7.957 | 85,125 | 92,390 | 31.35% slower | 26.49% slower |
| R | 64 | 65.651 | 67.932 | 66.792 | 86.981 | 85.909 | 974,846 | 958,199 | 32.49% faster | 28.62% faster |

P/D quantifies diagnostic/compiler/observation perturbation; R/D compares order in exactly one ELF. All paired outcomes remain:

| Comparison | Batch | Aggregate geometric rate change | Aggregate all3 pairs | Combined geometric rate change | Combined all3 pairs |
|---|---:|---:|---|---:|---|
| D/P | 1 | +0.45% | +3.00%, +0.14%, -1.74% | +0.53% | +1.59%, +0.17%, -0.15% |
| D/P | 64 | +3.04% | +7.72%, +0.64%, +0.91% | +2.76% | +7.10%, +0.67%, +0.66% |
| R/D | 1 | +42.95% | +41.82%, +45.99%, +41.08% | +26.01% | +25.48%, +28.01%, +24.55% |
| R/D | 64 | -1.93% | -1.69%, -1.61%, -2.50% | -4.41% | -4.08%, -3.99%, -5.16% |

## Total charged cost across both cases

Each row includes the identical512 business commits/16640 records; timing includes every Collect paid in those commits and both measurement phases. Validation/dispatch gaps remain excluded exactly as in the existing benchmark. Lower time is faster.

| Round | Arm | ULM charged ms | Native charged ms | ULM time vs native | Total Collect frames |
|---|---|---:|---:|---:|---:|
| 1 | P | 21.185 | 24.045 | -11.89% | 11 |
| 1 | D | 19.960 | 23.799 | -16.13% | 11 |
| 1 | R | 19.953 | 23.735 | -15.93% | 11 |
| 2 | P | 19.999 | 24.223 | -17.44% | 11 |
| 2 | D | 19.884 | 23.714 | -16.15% | 11 |
| 2 | R | 19.792 | 24.063 | -17.75% | 11 |
| 3 | P | 19.765 | 23.773 | -16.86% | 11 |
| 3 | D | 19.663 | 24.104 | -18.42% | 11 |
| 3 | R | 19.864 | 24.292 | -18.23% | 11 |

## Actual maintenance and cache observations

D/R Collect counts from ΔphysicalLSN−Δsequence agree with an independent offline scan of every selected WAL frame, verifying each domain digest and physical/business sequence transition. The same scan supplies P counts without new instrumentation or opening an engine. Cache hits/misses below describe the LAST completed Collect snapshot; they are NOT cumulative counters. A decline in misses is not a negative event count.

| Round | Arm | Batch | Case position | LSN before→after | Sequence before→after | Actual Collects | Last-cache hits/misses before→after |
|---|---|---:|---:|---|---|---:|---|
| 1 | D | 1 | 0 | 1→258 | 1→257 | 1 | 0/0→0/2 |
| 1 | D | 64 | 1 | 258→524 | 257→513 | 10 | 0/2→2/0 |
| 1 | R | 64 | 0 | 1→267 | 1→257 | 10 | 0/0→2/0 |
| 1 | R | 1 | 1 | 267→524 | 257→513 | 1 | 2/0→2/0 |
| 2 | D | 1 | 0 | 1→258 | 1→257 | 1 | 0/0→0/2 |
| 2 | D | 64 | 1 | 258→524 | 257→513 | 10 | 0/2→2/0 |
| 2 | R | 64 | 0 | 1→267 | 1→257 | 10 | 0/0→2/0 |
| 2 | R | 1 | 1 | 267→524 | 257→513 | 1 | 2/0→2/0 |
| 3 | R | 64 | 0 | 1→267 | 1→257 | 10 | 0/0→2/0 |
| 3 | R | 1 | 1 | 267→524 | 257→513 | 1 | 2/0→2/0 |
| 3 | D | 1 | 0 | 1→258 | 1→257 | 1 | 0/0→0/2 |
| 3 | D | 64 | 1 | 258→524 | 257→513 | 10 | 0/2→2/0 |

WAL-derived chronology is identical in all3 runs per arm. P and D share the same positions. Trial/transaction numbers are zero-based; a Collect preserves the displayed sequence and executes in the next business transaction begin. Every raw frame/position is retained in metrics.json.

| Arm | Batch | Trial | Phase | Transaction | Prewrite business sequence |
|---|---:|---:|---|---:|---:|
| P | 1 | 15 | aggregate | 5 | 246 |
| P | 64 | 1 | sampled | 0 | 281 |
| P | 64 | 3 | aggregate | 1 | 306 |
| P | 64 | 4 | sampled | 2 | 331 |
| P | 64 | 6 | aggregate | 3 | 356 |
| P | 64 | 7 | sampled | 3 | 380 |
| P | 64 | 9 | aggregate | 4 | 405 |
| P | 64 | 10 | sampled | 5 | 430 |
| P | 64 | 12 | aggregate | 6 | 455 |
| P | 64 | 13 | sampled | 7 | 480 |
| P | 64 | 15 | sampled | 0 | 505 |
| D | 1 | 15 | aggregate | 5 | 246 |
| D | 64 | 1 | sampled | 0 | 281 |
| D | 64 | 3 | aggregate | 1 | 306 |
| D | 64 | 4 | sampled | 2 | 331 |
| D | 64 | 6 | aggregate | 3 | 356 |
| D | 64 | 7 | sampled | 3 | 380 |
| D | 64 | 9 | aggregate | 4 | 405 |
| D | 64 | 10 | sampled | 5 | 430 |
| D | 64 | 12 | aggregate | 6 | 455 |
| D | 64 | 13 | sampled | 7 | 480 |
| D | 64 | 15 | sampled | 0 | 505 |
| R | 64 | 1 | sampled | 1 | 26 |
| R | 64 | 3 | aggregate | 2 | 51 |
| R | 64 | 4 | sampled | 3 | 76 |
| R | 64 | 6 | aggregate | 4 | 101 |
| R | 64 | 7 | sampled | 4 | 125 |
| R | 64 | 9 | aggregate | 5 | 150 |
| R | 64 | 10 | sampled | 6 | 175 |
| R | 64 | 12 | aggregate | 7 | 200 |
| R | 64 | 13 | sampled | 7 | 224 |
| R | 64 | 15 | sampled | 0 | 249 |
| R | 1 | 10 | aggregate | 5 | 422 |

## Single-write spike chronology, all16 trials

Each cell is aggregate mean µs/transaction; all events and costs stay in the absolute totals above. No final trial or Collect is removed. Full sampled8-transaction arrays and per-trial means/p99 remain in metrics.json.

| Arm | Round | Trials0–15 aggregate µs/tx |
|---|---:|---|
| P | 1 | 15.516, 10.844, 10.518, 11.524, 10.883, 11.171, 10.536, 10.835, 10.719, 10.951, 10.430, 11.422, 10.754, 10.811, 10.560, 108.037 |
| P | 2 | 15.487, 10.846, 10.370, 10.742, 11.030, 10.644, 10.485, 10.814, 11.141, 10.695, 10.395, 11.271, 11.504, 10.739, 10.441, 106.626 |
| P | 3 | 15.208, 10.866, 10.506, 10.741, 10.690, 10.644, 10.361, 10.874, 10.764, 10.833, 10.527, 10.912, 10.762, 10.744, 10.488, 96.231 |
| D | 1 | 15.140, 11.471, 10.533, 10.844, 10.794, 10.760, 10.390, 10.931, 10.749, 10.799, 10.426, 10.893, 10.789, 10.680, 10.559, 101.720 |
| D | 2 | 16.341, 10.819, 10.533, 13.076, 10.679, 10.806, 10.452, 10.765, 10.761, 10.768, 10.406, 10.757, 10.677, 10.655, 11.943, 103.399 |
| D | 3 | 15.309, 11.368, 10.382, 10.797, 10.691, 10.765, 11.027, 10.903, 10.720, 10.652, 10.665, 11.161, 10.832, 10.773, 11.156, 98.565 |
| R | 1 | 10.475, 10.144, 10.032, 10.401, 10.164, 10.980, 9.909, 10.308, 10.093, 10.477, 33.506, 11.633, 9.963, 10.266, 10.133, 10.116 |
| R | 2 | 10.259, 10.073, 10.245, 10.212, 10.777, 10.045, 9.817, 10.218, 10.078, 10.104, 34.523, 10.126, 9.933, 10.321, 10.100, 10.059 |
| R | 3 | 10.316, 10.124, 10.020, 10.311, 10.748, 10.118, 9.896, 10.968, 10.186, 10.201, 34.434, 10.196, 10.102, 10.357, 10.239, 10.168 |



### All 13 read controls, native comparisons and sampled tails

Read loops execute before write-order selection. Differences here are process/codegen context observations, not causal effects of a later order. Rates pool equal-sized process populations; p99 columns are descriptive means of each process trial-p99 summary. Every raw span and individual tail remains in the archived reports.

|Read case|Arm|ULMDB/s|LMDB/s|ULMDB native advantage|Status|ULMDB sampled p99 ns|LMDB sampled p99 ns|
|---|---|---:|---:|---:|---|---:|---:|
|ordered_kv_mixed|P|7,171,835|4,071,029|+76.17%|faster|205|344|
|ordered_kv_mixed|D|6,943,251|4,096,248|+69.50%|faster|212|339|
|ordered_kv_mixed|R|6,931,792|4,110,755|+68.63%|faster|214|339|
|ordered_kv_hits|P|7,418,043|4,120,524|+80.03%|faster|202|340|
|ordered_kv_hits|D|7,265,153|4,157,235|+74.76%|faster|209|336|
|ordered_kv_hits|R|7,258,935|4,154,900|+74.71%|faster|212|340|
|ordered_kv_misses|P|7,517,180|3,965,483|+89.57%|faster|200|345|
|ordered_kv_misses|D|7,286,140|4,023,978|+81.07%|faster|208|344|
|ordered_kv_misses|R|7,256,714|3,993,906|+81.69%|faster|213|346|
|ipv4_lpm_mixed|P|5,479,955|2,572,707|+113.00%|faster|426|780|
|ipv4_lpm_mixed|D|5,471,796|2,672,531|+104.74%|faster|432|759|
|ipv4_lpm_mixed|R|5,449,870|2,596,927|+109.86%|faster|436|793|
|ipv4_lpm_hits|P|3,179,637|2,075,832|+53.17%|faster|434|801|
|ipv4_lpm_hits|D|3,145,128|2,161,346|+45.52%|faster|443|778|
|ipv4_lpm_hits|R|3,129,842|2,094,438|+49.44%|faster|450|812|
|ipv4_lpm_misses|P|21,479,244|3,710,485|+478.88%|faster|114|654|
|ipv4_lpm_misses|D|20,964,562|3,812,992|+449.82%|faster|132|599|
|ipv4_lpm_misses|R|20,824,656|3,749,139|+455.45%|faster|133|660|
|ipv4_lpm_outside_cover_misses|P|99,930,049|100,974,615|-1.03%|roughly on par|41|50|
|ipv4_lpm_outside_cover_misses|D|100,936,395|92,679,838|+8.91%|faster|41|47|
|ipv4_lpm_outside_cover_misses|R|100,451,824|100,456,449|-0.00%|roughly on par|41|50|
|ipv4_lpm_inside_gap_misses|P|11,578,058|1,897,846|+510.06%|faster|115|644|
|ipv4_lpm_inside_gap_misses|D|11,120,023|1,948,302|+470.75%|faster|131|606|
|ipv4_lpm_inside_gap_misses|R|11,019,415|1,914,217|+475.66%|faster|133|679|
|ipv6_lpm_mixed|P|4,386,386|1,531,318|+186.45%|faster|518|1,264|
|ipv6_lpm_mixed|D|4,293,377|1,542,414|+178.35%|faster|522|1,254|
|ipv6_lpm_mixed|R|4,302,859|1,535,223|+180.28%|faster|520|1,260|
|ipv6_lpm_hits|P|2,518,069|1,217,725|+106.78%|faster|534|1,272|
|ipv6_lpm_hits|D|2,474,075|1,236,055|+100.16%|faster|531|1,262|
|ipv6_lpm_hits|R|2,465,244|1,220,106|+102.05%|faster|535|1,275|
|ipv6_lpm_misses|P|17,382,377|2,197,802|+690.90%|faster|141|1,111|
|ipv6_lpm_misses|D|17,041,215|2,215,557|+669.16%|faster|142|1,080|
|ipv6_lpm_misses|R|17,059,433|2,228,415|+665.54%|faster|142|1,080|
|ipv6_lpm_outside_cover_misses|P|96,496,765|97,837,187|-1.37%|roughly on par|41|46|
|ipv6_lpm_outside_cover_misses|D|96,432,015|92,604,025|+4.13%|faster|41|47|
|ipv6_lpm_outside_cover_misses|R|96,863,245|97,815,654|-0.97%|roughly on par|41|50|
|ipv6_lpm_inside_gap_misses|P|9,341,217|1,111,586|+740.35%|faster|141|1,127|
|ipv6_lpm_inside_gap_misses|D|9,174,622|1,114,023|+723.56%|faster|145|1,107|
|ipv6_lpm_inside_gap_misses|R|9,210,985|1,123,490|+719.85%|faster|142|1,120|

### Load chronology: the gap persists after startup

#### Existing clocks, all windows

All three explicit/native pairs and all ten fixed100-batch epochs are retained below; no workload or timer was added. First100 is exactly epoch0 (transactions0–99). The summary uses medians of the three per-run window means; each direct-native change remains listed per pair.

| Window | Explicit begin ns/record | Explicit mutation ns/record | Explicit commit ns/record | Explicit total ns/record | Native mutation ns/record | Native total ns/record | Total-time E/native, all3 pairs |
|---|---:|---:|---:|---:|---:|---:|---|
| first1 | 0.421 | 472.338 | 105.270 | 578.701 | 218.836 | 562.690 | +7.12%, +2.85%, -0.48% |
| first10 | 0.250 | 482.252 | 122.037 | 604.694 | 233.447 | 606.553 | +4.55%, -8.02%, -0.31% |
| first100 | 0.231 | 516.649 | 151.251 | 668.279 | 280.618 | 673.006 | +1.17%, -0.70%, -8.43% |
| middle100 (400–499) | 0.220 | 537.881 | 143.696 | 681.923 | 295.790 | 611.755 | +11.47%, +12.78%, +8.11% |
| last100 (900–999) | 0.216 | 542.383 | 138.259 | 680.428 | 298.703 | 615.815 | +10.49%, +13.86%, +5.38% |

The retained disk-load gap is not confined to startup: explicit takes longer in all three middle/late comparisons, while first100 total time is close or faster in two pairs. Mutation cost is already higher from the first batch and remains higher late; cheaper explicit commits offset part of it. This does not prove a specific warmup mechanism, and growing depth/occupancy/splits plus periodic maintenance confound the timeline. The separate RAM order experiment tests write-history sensitivity directly.

Original full tables and exact per-row statistics follow.

#### All retained load batches and epochs

All six retained explicit/native loads and all ordered1000 rows per process pass original value/miss/cursor/snapshot/reopen and configuration/normal-durability checks, with raw report digests verified. No engine was rerun. All ten100-batch epochs and first1/10 views are retained. Mean ns/record below uses existing clocks; each transaction has1000 records.

| Round | Engine | Window | Begin ns/record | Mutation ns/record | Commit ns/record | Total ns/record |
|---|---|---|---:|---:|---:|---:|
| 1 | explicit | first1 | 0.401 | 472.338 | 107.565 | 580.484 |
| 1 | explicit | first10 | 0.223 | 480.667 | 147.611 | 628.644 |
| 1 | explicit | epoch0 | 0.218 | 516.922 | 130.464 | 647.744 |
| 1 | explicit | epoch1 | 0.227 | 538.107 | 147.223 | 685.695 |
| 1 | explicit | epoch2 | 0.214 | 538.847 | 138.411 | 677.611 |
| 1 | explicit | epoch3 | 0.214 | 536.866 | 129.106 | 666.326 |
| 1 | explicit | epoch4 | 0.207 | 537.881 | 143.696 | 681.923 |
| 1 | explicit | epoch5 | 0.225 | 536.739 | 139.418 | 676.521 |
| 1 | explicit | epoch6 | 0.211 | 538.770 | 134.961 | 674.081 |
| 1 | explicit | epoch7 | 102.743 | 539.652 | 143.180 | 785.715 |
| 1 | explicit | epoch8 | 0.208 | 542.862 | 139.263 | 682.472 |
| 1 | explicit | epoch9 | 0.216 | 541.815 | 138.259 | 680.428 |
| 1 | lmdb | first1 | 0.421 | 218.836 | 322.473 | 541.900 |
| 1 | lmdb | first10 | 0.277 | 232.158 | 368.713 | 601.304 |
| 1 | lmdb | epoch0 | 0.283 | 275.903 | 363.890 | 640.234 |
| 1 | lmdb | epoch1 | 0.250 | 308.599 | 371.047 | 680.059 |
| 1 | lmdb | epoch2 | 0.222 | 295.559 | 357.309 | 653.239 |
| 1 | lmdb | epoch3 | 0.223 | 298.825 | 322.975 | 622.174 |
| 1 | lmdb | epoch4 | 0.223 | 290.880 | 320.510 | 611.755 |
| 1 | lmdb | epoch5 | 0.221 | 294.396 | 304.451 | 599.215 |
| 1 | lmdb | epoch6 | 0.224 | 298.811 | 322.826 | 622.006 |
| 1 | lmdb | epoch7 | 0.217 | 298.259 | 327.763 | 626.386 |
| 1 | lmdb | epoch8 | 0.221 | 297.608 | 325.998 | 623.971 |
| 1 | lmdb | epoch9 | 0.225 | 297.525 | 317.920 | 615.815 |
| 2 | explicit | first1 | 0.421 | 472.869 | 105.270 | 578.701 |
| 2 | explicit | first10 | 0.283 | 482.420 | 120.289 | 603.135 |
| 2 | explicit | epoch0 | 0.240 | 516.649 | 151.251 | 668.279 |
| 2 | explicit | epoch1 | 0.245 | 537.526 | 136.402 | 674.312 |
| 2 | explicit | epoch2 | 0.245 | 539.577 | 131.366 | 671.325 |
| 2 | explicit | epoch3 | 0.238 | 538.113 | 148.645 | 687.218 |
| 2 | explicit | epoch4 | 0.245 | 541.939 | 137.345 | 679.666 |
| 2 | explicit | epoch5 | 0.244 | 541.667 | 136.160 | 678.208 |
| 2 | explicit | epoch6 | 0.242 | 541.284 | 148.286 | 689.949 |
| 2 | explicit | epoch7 | 105.837 | 543.737 | 140.545 | 790.255 |
| 2 | explicit | epoch8 | 0.249 | 545.966 | 132.556 | 678.909 |
| 2 | explicit | epoch9 | 0.259 | 546.374 | 151.299 | 698.071 |
| 2 | lmdb | first1 | 0.701 | 217.563 | 344.185 | 562.690 |
| 2 | lmdb | first10 | 0.339 | 233.447 | 421.760 | 655.717 |
| 2 | lmdb | epoch0 | 0.273 | 280.618 | 391.953 | 673.006 |
| 2 | lmdb | epoch1 | 0.229 | 291.639 | 334.495 | 626.512 |
| 2 | lmdb | epoch2 | 0.241 | 295.009 | 322.407 | 617.808 |
| 2 | lmdb | epoch3 | 0.224 | 295.495 | 320.644 | 616.512 |
| 2 | lmdb | epoch4 | 0.229 | 295.790 | 306.480 | 602.649 |
| 2 | lmdb | epoch5 | 0.228 | 298.142 | 325.198 | 623.719 |
| 2 | lmdb | epoch6 | 0.224 | 302.742 | 321.466 | 624.583 |
| 2 | lmdb | epoch7 | 0.219 | 303.338 | 314.926 | 618.635 |
| 2 | lmdb | epoch8 | 0.227 | 300.569 | 318.489 | 619.436 |
| 2 | lmdb | epoch9 | 0.223 | 298.703 | 314.006 | 613.082 |
| 3 | lmdb | first1 | 0.771 | 234.205 | 342.622 | 577.729 |
| 3 | lmdb | first10 | 0.401 | 238.559 | 367.431 | 606.553 |
| 3 | lmdb | epoch0 | 0.333 | 313.687 | 416.223 | 730.428 |
| 3 | lmdb | epoch1 | 0.261 | 297.933 | 323.515 | 621.864 |
| 3 | lmdb | epoch2 | 0.251 | 291.400 | 324.288 | 616.087 |
| 3 | lmdb | epoch3 | 0.256 | 294.830 | 340.799 | 636.029 |
| 3 | lmdb | epoch4 | 0.252 | 302.580 | 329.687 | 632.671 |
| 3 | lmdb | epoch5 | 0.257 | 295.810 | 316.180 | 612.395 |
| 3 | lmdb | epoch6 | 0.255 | 299.084 | 314.393 | 613.876 |
| 3 | lmdb | epoch7 | 0.251 | 299.130 | 313.182 | 612.708 |
| 3 | lmdb | epoch8 | 0.258 | 299.146 | 312.238 | 611.789 |
| 3 | lmdb | epoch9 | 0.251 | 299.443 | 339.801 | 639.641 |
| 3 | explicit | first1 | 0.441 | 469.633 | 104.619 | 574.943 |
| 3 | explicit | first10 | 0.250 | 482.252 | 122.037 | 604.694 |
| 3 | explicit | epoch0 | 0.231 | 514.509 | 153.965 | 668.844 |
| 3 | explicit | epoch1 | 0.227 | 535.330 | 149.578 | 685.273 |
| 3 | explicit | epoch2 | 0.220 | 536.468 | 152.281 | 689.107 |
| 3 | explicit | epoch3 | 0.220 | 536.403 | 153.931 | 690.692 |
| 3 | explicit | epoch4 | 0.220 | 537.114 | 146.528 | 684.000 |
| 3 | explicit | epoch5 | 0.227 | 534.146 | 142.891 | 677.402 |
| 3 | explicit | epoch6 | 0.226 | 535.566 | 128.470 | 664.401 |
| 3 | explicit | epoch7 | 101.237 | 541.309 | 126.010 | 768.697 |
| 3 | explicit | epoch8 | 0.218 | 541.353 | 123.675 | 665.383 |
| 3 | explicit | epoch9 | 0.212 | 542.383 | 131.304 | 674.036 |

| Round | Window | Explicit/native total time | Explicit/native mutation time | Explicit/native commit time |
|---|---|---:|---:|---:|
| 1 | first1 | +7.12% | +115.84% | -66.64% |
| 1 | first10 | +4.55% | +107.04% | -59.97% |
| 1 | epoch0 | +1.17% | +87.36% | -64.15% |
| 1 | epoch1 | +0.83% | +74.37% | -60.32% |
| 1 | epoch2 | +3.73% | +82.31% | -61.26% |
| 1 | epoch3 | +7.10% | +79.66% | -60.03% |
| 1 | epoch4 | +11.47% | +84.92% | -55.17% |
| 1 | epoch5 | +12.90% | +82.32% | -54.21% |
| 1 | epoch6 | +8.37% | +80.30% | -58.19% |
| 1 | epoch7 | +25.44% | +80.93% | -56.32% |
| 1 | epoch8 | +9.38% | +82.41% | -57.28% |
| 1 | epoch9 | +10.49% | +82.11% | -56.51% |
| 2 | first1 | +2.85% | +117.35% | -69.41% |
| 2 | first10 | -8.02% | +106.65% | -71.48% |
| 2 | epoch0 | -0.70% | +84.11% | -61.41% |
| 2 | epoch1 | +7.63% | +84.31% | -59.22% |
| 2 | epoch2 | +8.66% | +82.90% | -59.25% |
| 2 | epoch3 | +11.47% | +82.11% | -53.64% |
| 2 | epoch4 | +12.78% | +83.22% | -55.19% |
| 2 | epoch5 | +8.74% | +81.68% | -58.13% |
| 2 | epoch6 | +10.47% | +78.79% | -53.87% |
| 2 | epoch7 | +27.74% | +79.25% | -55.37% |
| 2 | epoch8 | +9.60% | +81.64% | -58.38% |
| 2 | epoch9 | +13.86% | +82.92% | -51.82% |
| 3 | first1 | -0.48% | +100.52% | -69.47% |
| 3 | first10 | -0.31% | +102.15% | -66.79% |
| 3 | epoch0 | -8.43% | +64.02% | -63.01% |
| 3 | epoch1 | +10.20% | +79.68% | -53.76% |
| 3 | epoch2 | +11.85% | +84.10% | -53.04% |
| 3 | epoch3 | +8.59% | +81.94% | -54.83% |
| 3 | epoch4 | +8.11% | +77.51% | -55.56% |
| 3 | epoch5 | +10.62% | +80.57% | -54.81% |
| 3 | epoch6 | +8.23% | +79.07% | -59.14% |
| 3 | epoch7 | +25.46% | +80.96% | -59.76% |
| 3 | epoch8 | +8.76% | +80.97% | -60.39% |
| 3 | epoch9 | +5.38% | +81.13% | -61.36% |

Positive direct percentages mean longer time. Growing page occupancy/depth/splits and maintenance confound pure warmup attribution. These diagnostic phase totals do not establish RAM single-transaction behavior or accepted throughput. All individual means/medians/min/max, exact windows and six raw hashes remain in metrics.json/CSV. No parameter or engine-source change was made.



The sustained eight-process control and quiet 15-process census are now complete and appear below. Their held-source diagnostic results preserve every first-use and cached maintenance cost; neither replaces the retained-6 scorecard.

## Page-only retained-6 candidate: HOLD, all 25 processes complete

The dense local-validation kernel gain did not reproduce in the real load workload. All three ordinary load pairs are slower; the complete result is held with no root apply. Retained 6 remains current. Full tables preserve all 13 reads per filesystem, aggregate/combined writes, tails, cold/setup/resource observations and direct native comparisons. SQLite was explicitly omitted from this private screen, so it is not full-39 qualification.

### Complete Page-only hold decision and tables

Root decision: HOLD, no production apply. Retained6 remains the current optimization baseline. All16 full plus9 ordinary processes passed correctness, source/ELF/native, exact settings/accounting and raw guards; passing those guards does not qualify the performance. The intended load gain failed in all3 actual ordinary pairs. No further benchmark or retry is used to replace that result.

Positive throughput difference means Page C is faster; for time/space the table reports its cost ratio. Roughly-on-par means within2% of the observed center, not equivalence. Every target/control/diagnostic observation is retained in the complete226-cell comparison, including all103 target cells. The analysis-only parser changes ONLY the declared missing SQLite group; the original frozen comparator and all engine inputs remain untouched. Three million observations retain normal insufficient-repeat flags. This is not promoted as a full39 gate.

|Metric|Retained6 R|Page C|C/R change|Contemporary LMDB|C vs native|Observed status|Distance from native|
|---|---:|---:|---:|---:|---|---|---|
|RAM single aggregate /s|63,052.907|63,199.788|+0.233%|136,159.234|-53.584%|slower|115.443% more throughput|
|RAM single combined /s|75,824.771|75,905.045|+0.106%|139,564.695|-45.613%|slower|83.867% more throughput|
|RAM64 aggregate /s|990,120.311|977,847.661|-1.240%|742,801.554|+31.643%|faster|0.000% more throughput|
|Million load /s|1,405,482.242|1,371,487.898|-2.419%|1,531,695.294|-10.459%|slower|11.681% more throughput|
|Million read /s|2,804,334.094|2,805,210.486|+0.031%|1,701,214.159|+64.895%|faster|0.000% more throughput|
|Million update /s|146,380.875|149,247.368|+1.958%|129,751.834|+15.025%|faster|0.000% more throughput|
|Million setup ms|403.128|417.536|+3.574%|1.156|361.139x cost|higher cost|99.723% less cost|
|Million close/reopen ms|6,978.907|7,092.213|+1.624%|3.828|1852.697x cost|higher cost|99.946% less cost|
|Allocated after reopen MiB|256.003|256.000|-0.001%|33.046|7.747x cost|higher cost|87.092% less cost|

The modest update gain does not offset a failed load target, increased setup/reopen costs, near-flat RAM singles, and all4 adverse RAM64 pairs. Full read tradeoffs remain visible: RAM KV hits−1.821%, misses−0.987%, mixed−1.016%; IPv6 hit/mixed improvements coexist with all4 miss/inside-gap reductions1.27–1.75% on both filesystems. Disk64 combined−1.570% and trial p99+6.233% remain. Candidate disk-single398.959us and RAM-single34.857us maxima are retained. One full candidate allocation is4096 bytes larger; two ordinary retained6 allocations (rounds1 and3) are4096 bytes larger. File observations are not normalized or assigned an unconfirmed cause.

The dense kernel improvement did not carry through to the real ordinary workload. This result does not establish the source-level cause of the macro loss. Preserve every validation/error/budget/durability guarantee; obtain a new confirmed attribution before proposing the next architecture change. No root runtime changes were applied.

# Page-only ordinary nine: all observations and native controls

Exactly9 original histories passed, three each retained6 R, Page-only C, native L. All source/native/settings/accounting/value/miss/cursor/reopen/workload/denominator guards remain; SQLite omission is explicit. No process, metric or adverse point is dropped. Three million pairs do not establish equivalence. Native L observations are shared contemporaneous controls, not independent A/A repeats.

|Round|Position|Arm|Load/s|Read/s|Update/s|Setup ms|Close/reopen ms|Before allocated bytes|After allocated bytes|
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|
|1|1|R|1,403,859|2,818,595|145,654|407.203|6983.510|268439552|268439552|
|1|2|C|1,375,455|2,866,131|149,383|421.397|7021.053|268435456|268435456|
|1|3|L|1,481,181|1,697,012|129,753|0.973|3.804|34652160|34652160|
|2|1|C|1,364,479|2,805,310|149,067|414.465|7044.836|268435456|268435456|
|2|2|L|1,550,219|1,708,280|129,753|0.932|3.942|34648064|34648064|
|2|3|R|1,410,171|2,773,841|146,574|404.186|6982.631|268435456|268435456|
|3|1|L|1,565,005|1,698,373|129,749|1.705|3.741|34652160|34652160|
|3|2|R|1,402,429|2,820,817|146,918|398.050|6970.588|268439552|268439552|
|3|3|C|1,374,557|2,745,487|149,293|416.777|7212.273|268435456|268435456|

Geometric process centers and all three paired estimates:

|Metric|Unit|Retained6 R|Page C|C/R change|All3 paired changes|Native L|C/native rate difference or cost ratio|
|---|---|---:|---:|---:|---|---:|---|
|close_reopen_ns|ns|6,978,907,220.878|7,092,213,011.644|+1.624%|+0.538%, +0.891%, +3.467%|3,828,048.120|1852.697x cost|
|load/throughput|operations/s|1,405,482.242|1,371,487.898|-2.419%|-2.023%, -3.240%, -1.987%|1,531,695.294|-10.459%|
|read/throughput|operations/s|2,804,334.094|2,805,210.486|+0.031%|+1.686%, +1.134%, -2.670%|1,701,214.159|+64.895%|
|setup_ns|ns|403,128,104.761|417,536,431.989|+3.574%|+3.486%, +2.543%, +4.705%|1,156,166.937|361.139x cost|
|storage_after_close_reopen/allocated_bytes|bytes|268,438,186.660|268,435,456.000|-0.001%|-0.002%, +0.000%, -0.002%|34,650,794.613|7.747x cost|
|storage_after_close_reopen/apparent_bytes|bytes|268,435,456.000|268,435,456.000|+0.000%|+0.000%, +0.000%, +0.000%|34,652,160.000|7.747x cost|
|storage_after_close_reopen/files|files|2.000|2.000|+0.000%|+0.000%, +0.000%, +0.000%|2.000|1.000x cost|
|storage_before_close/allocated_bytes|bytes|268,438,186.660|268,435,456.000|-0.001%|-0.002%, +0.000%, -0.002%|34,650,794.613|7.747x cost|
|storage_before_close/apparent_bytes|bytes|268,435,456.000|268,435,456.000|+0.000%|+0.000%, +0.000%, +0.000%|34,652,160.000|7.747x cost|
|storage_before_close/files|files|2.000|2.000|+0.000%|+0.000%, +0.000%, +0.000%|2.000|1.000x cost|
|update/throughput|operations/s|146,380.875|149,247.368|+1.958%|+2.560%, +1.701%, +1.617%|129,751.834|+15.025%|

# Page-only retained6 full16 diagnostic: every cell retained

All16 processes and source/ELF/native/oracle bindings passed. All outputs below are private screen evidence; no retention or full39 qualification is claimed. Native controls remain absolute and are never subtracted. Familywise intervals preserve all negative estimates; overlap is not equivalence.

|Filesystem / case|R ULMDB/s|C ULMDB/s|C/R change|All4 paired changes|C native/s|C vs native|Native C/R change|
|---|---:|---:|---:|---|---:|---:|---:|
|disk / durable_batch1/combined_throughput|16,051|16,383|+2.07%|+6.11%, -4.67%, +4.44%, +2.74%|11,390|+43.84%|+0.16%|
|disk / durable_batch1/throughput|15,041|15,481|+2.93%|+5.55%, -8.83%, +7.85%, +8.15%|10,860|+42.55%|+1.42%|
|disk / durable_batch64/combined_throughput|499,964|492,115|-1.57%|+4.26%, -6.61%, -3.03%, -0.57%|184,063|+167.36%|+0.37%|
|disk / durable_batch64/throughput|487,526|487,444|-0.02%|+7.22%, -2.49%, -5.88%, +1.55%|177,838|+174.09%|-0.06%|
|disk / ipv4_lpm_hits/throughput|3,190,271|3,185,897|-0.14%|-1.40%, -0.47%, +1.82%, -0.47%|2,155,042|+47.83%|+2.77%|
|disk / ipv4_lpm_inside_gap_misses/throughput|11,610,808|11,420,539|-1.64%|-2.90%, -2.78%, +0.17%, -1.00%|1,953,221|+484.70%|+0.56%|
|disk / ipv4_lpm_misses/throughput|21,645,209|21,523,977|-0.56%|-0.91%, -0.26%, +0.54%, -1.60%|3,854,369|+458.43%|+4.75%|
|disk / ipv4_lpm_mixed/throughput|5,526,629|5,503,225|-0.42%|-0.18%, -0.88%, +0.29%, -0.92%|2,647,136|+107.89%|+1.49%|
|disk / ipv4_lpm_outside_cover_misses/throughput|100,247,571|100,949,398|+0.70%|-0.01%, +2.88%, -0.01%, -0.03%|92,736,292|+8.86%|+27.09%|
|disk / ipv6_lpm_hits/throughput|2,476,938|2,507,961|+1.25%|+0.71%, +1.32%, +2.09%, +0.89%|1,250,971|+100.48%|+0.15%|
|disk / ipv6_lpm_inside_gap_misses/throughput|9,359,845|9,222,280|-1.47%|-1.70%, -0.77%, -1.65%, -1.76%|1,137,792|+710.54%|+1.02%|
|disk / ipv6_lpm_misses/throughput|17,236,847|16,934,930|-1.75%|-2.28%, -0.67%, -1.10%, -2.95%|2,243,326|+654.90%|+1.08%|
|disk / ipv6_lpm_mixed/throughput|4,299,275|4,363,174|+1.49%|+1.34%, +1.60%, +1.80%, +1.20%|1,571,580|+177.63%|+0.24%|
|disk / ipv6_lpm_outside_cover_misses/throughput|96,301,969|96,730,263|+0.44%|-0.42%, -2.56%, +4.62%, +0.27%|90,843,569|+6.48%|+19.54%|
|disk / ordered_kv_hits/throughput|7,609,076|7,616,217|+0.09%|+3.52%, -0.40%, -1.49%, -1.17%|4,205,838|+81.09%|+0.60%|
|disk / ordered_kv_misses/throughput|7,660,924|7,658,162|-0.04%|+2.96%, -1.08%, -1.17%, -0.80%|4,015,767|+90.70%|-0.35%|
|disk / ordered_kv_mixed/throughput|7,289,887|7,284,136|-0.08%|+2.97%, +0.07%, -1.65%, -1.64%|4,106,539|+77.38%|-0.29%|
|ram / durable_batch1/combined_throughput|75,825|75,905|+0.11%|+0.20%, +1.19%, -0.72%, -0.24%|139,565|-45.61%|-0.41%|
|ram / durable_batch1/throughput|63,053|63,200|+0.23%|+0.32%, +1.29%, -0.85%, +0.18%|136,159|-53.58%|-0.45%|
|ram / durable_batch64/combined_throughput|998,977|985,354|-1.36%|-1.82%, -1.03%, -1.03%, -1.57%|751,064|+31.19%|-2.17%|
|ram / durable_batch64/throughput|990,120|977,848|-1.24%|-1.65%, -1.05%, -0.70%, -1.55%|742,802|+31.64%|-2.32%|
|ram / ipv4_lpm_hits/throughput|3,208,639|3,191,415|-0.54%|-0.65%, -0.16%, -0.18%, -1.16%|2,117,318|+50.73%|-0.05%|
|ram / ipv4_lpm_inside_gap_misses/throughput|11,491,203|11,510,345|+0.17%|-0.61%, +1.09%, +0.28%, -0.08%|1,898,034|+506.44%|-3.56%|
|ram / ipv4_lpm_misses/throughput|21,531,589|21,519,617|-0.06%|-1.75%, +0.34%, +1.98%, -0.75%|3,722,643|+478.07%|-4.56%|
|ram / ipv4_lpm_mixed/throughput|5,523,172|5,494,973|-0.51%|+0.29%, -0.56%, -0.16%, -1.60%|2,563,448|+114.36%|-3.32%|
|ram / ipv4_lpm_outside_cover_misses/throughput|100,565,868|100,553,359|-0.01%|+1.50%, +0.01%, -1.55%, +0.02%|91,528,188|+9.86%|+7.57%|
|ram / ipv6_lpm_hits/throughput|2,466,174|2,519,438|+2.16%|+3.54%, +1.71%, +1.88%, +1.52%|1,230,581|+104.74%|-1.71%|
|ram / ipv6_lpm_inside_gap_misses/throughput|9,395,510|9,263,122|-1.41%|-1.78%, -0.19%, -1.49%, -2.16%|1,122,247|+725.41%|-0.95%|
|ram / ipv6_lpm_misses/throughput|17,304,458|17,084,978|-1.27%|-0.99%, -1.17%, -1.92%, -0.99%|2,220,969|+669.26%|-0.58%|
|ram / ipv6_lpm_mixed/throughput|4,292,510|4,373,019|+1.88%|+1.88%, +1.76%, +1.49%, +2.37%|1,547,668|+182.56%|-1.44%|
|ram / ipv6_lpm_outside_cover_misses/throughput|95,977,364|96,754,310|+0.81%|-0.44%, -0.17%, +3.66%, +0.24%|90,694,614|+6.68%|+2.31%|
|ram / ordered_kv_hits/throughput|7,585,400|7,447,267|-1.82%|+2.25%, -3.75%, -2.61%, -3.07%|4,209,449|+76.92%|+0.72%|
|ram / ordered_kv_misses/throughput|7,622,268|7,547,038|-0.99%|+1.84%, -3.30%, -1.21%, -1.22%|4,043,985|+86.62%|+0.26%|
|ram / ordered_kv_mixed/throughput|7,248,121|7,174,462|-1.02%|+1.60%, -3.39%, -1.84%, -0.37%|4,125,954|+73.89%|+0.04%|

All target sampled-tail, cold, setup and file costs:

|Metric|Unit|R|C|C/R change|All4 paired changes|
|---|---|---:|---:|---:|---|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99|ns|234,684.138|193,247.463|-17.66%|-67.94%, -0.92%, -48.58%, +181.50%|
|full/disk/ulmdb/durable_batch1/sampled_p99|ns|79,043.222|78,574.353|-0.59%|-14.47%, -2.54%, +1.59%, +15.30%|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99|ns|416,833.896|378,953.444|-9.09%|-37.96%, +4.09%, -4.58%, +10.86%|
|full/disk/ulmdb/durable_batch64/sampled_p99|ns|218,082.864|231,677.159|+6.23%|-0.78%, +23.86%, -0.65%, +4.32%|
|full/disk/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|67,109,887.977|+0.00%|+0.00%, +0.00%, +0.01%, +0.00%|
|full/disk/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|+0.00%, +0.00%, +0.00%, +0.00%|
|full/disk/ulmdb/footprint/after/files|files|2.000|2.000|+0.00%|+0.00%, +0.00%, +0.00%, +0.00%|
|full/disk/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|67,109,887.977|+0.00%|+0.00%, +0.00%, +0.01%, +0.00%|
|full/disk/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|+0.00%, +0.00%, +0.00%, +0.00%|
|full/disk/ulmdb/footprint/before/files|files|2.000|2.000|+0.00%|+0.00%, +0.00%, +0.00%, +0.00%|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99|ns|430.401|433.277|+0.67%|+2.23%, +1.44%, -3.35%, +2.48%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|121.964|117.298|-3.83%|-7.37%, -3.21%, -3.13%, -1.50%|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99|ns|122.169|115.842|-5.18%|-7.50%, -8.26%, -1.37%, -3.40%|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|419.964|422.693|+0.65%|+2.03%, -0.25%, -0.41%, +1.26%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.281|41.396|+0.28%|+2.64%, -0.12%, -1.35%, +0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99|ns|537.465|528.567|-1.66%|-0.78%, -1.18%, -6.10%, +1.59%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|142.505|141.000|-1.06%|-0.77%, -3.04%, -0.39%, +0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99|ns|142.418|142.134|-0.20%|-1.57%, +0.00%, -1.20%, +2.01%|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|521.792|512.140|-1.85%|-0.64%, -0.72%, -5.68%, -0.24%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.127|41.127|+0.00%|-1.23%, +0.00%, +1.25%, +0.00%|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99|ns|198.963|197.860|-0.55%|-3.77%, +0.60%, +0.41%, +0.61%|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99|ns|196.481|197.587|+0.56%|-3.15%, +3.59%, +1.06%, +0.87%|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99|ns|202.499|200.102|-1.18%|-4.18%, +0.86%, -2.22%, +0.90%|
|full/disk/ulmdb/reopen/complete_cold_check_ns|ns|92,819,024.002|95,117,817.268|+2.48%|+0.87%, +8.54%, +1.19%, -0.46%|
|full/disk/ulmdb/reopen/environment_drop_ns|ns|87,464.385|87,852.268|+0.44%|+3.21%, -3.90%, +0.85%, +1.75%|
|full/disk/ulmdb/reopen/experimental_reenable_ns|ns|1,893.732|2,079.392|+9.80%|+32.72%, +13.96%, +0.47%, -4.34%|
|full/disk/ulmdb/reopen/open_ns|ns|82,472,835.408|84,670,962.137|+2.67%|+0.83%, +9.44%, +1.31%, -0.62%|
|full/disk/ulmdb/reopen/validation_ns|ns|6,096,236.812|6,230,497.508|+2.20%|+2.77%, +2.64%, +1.77%, +1.63%|
|full/disk/ulmdb/setup/create|ns|102,969,150.991|102,765,106.635|-0.20%|+0.09%, +1.56%, -1.45%, -0.97%|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99|ns|15,388.692|14,338.208|-6.83%|+23.93%, -31.88%, -7.95%, -3.02%|
|full/ram/ulmdb/durable_batch1/sampled_p99|ns|11,684.691|11,718.985|+0.29%|+2.23%, -5.01%, -0.43%, +4.65%|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99|ns|247,102.083|245,581.457|-0.62%|-1.51%, -2.61%, +0.57%, +1.13%|
|full/ram/ulmdb/durable_batch64/sampled_p99|ns|100,752.074|101,782.106|+1.02%|+2.92%, -0.76%, +0.23%, +1.73%|
|full/ram/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|+0.00%, +0.00%, +0.00%, +0.00%|
|full/ram/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|+0.00%, +0.00%, +0.00%, +0.00%|
|full/ram/ulmdb/footprint/after/files|files|2.000|2.000|+0.00%|+0.00%, +0.00%, +0.00%, +0.00%|
|full/ram/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|+0.00%, +0.00%, +0.00%, +0.00%|
|full/ram/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|+0.00%, +0.00%, +0.00%, +0.00%|
|full/ram/ulmdb/footprint/before/files|files|2.000|2.000|+0.00%|+0.00%, +0.00%, +0.00%, +0.00%|
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99|ns|427.031|432.844|+1.36%|+0.83%, +2.58%, +0.82%, +1.22%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|121.284|113.919|-6.07%|-6.72%, -6.43%, -6.45%, -4.68%|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99|ns|120.534|113.488|-5.85%|-4.66%, -5.04%, -7.82%, -5.84%|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|419.566|423.613|+0.96%|-0.98%, +2.00%, +2.18%, +0.70%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.268|41.835|+1.37%|+1.37%, +2.77%, +2.64%, -1.23%|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99|ns|531.037|524.105|-1.31%|-5.88%, +0.25%, +0.67%, -0.11%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|142.532|141.151|-0.97%|-3.07%, +0.00%, -0.39%, -0.39%|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99|ns|141.966|141.426|-0.38%|-1.51%, +0.00%, +0.00%, +0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|519.881|508.586|-2.17%|-3.57%, -0.74%, -0.60%, -3.74%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.281|41.127|-0.37%|+0.00%, +0.00%, -1.35%, -0.12%|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99|ns|198.930|204.154|+2.63%|-0.91%, +5.02%, +0.25%, +6.33%|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99|ns|198.949|202.952|+2.01%|+0.38%, +3.58%, +0.34%, +3.80%|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99|ns|201.354|207.085|+2.85%|+1.09%, +1.02%, +3.14%, +6.23%|
|full/ram/ulmdb/reopen/complete_cold_check_ns|ns|52,434,943.699|52,531,603.962|+0.18%|+1.54%, +0.73%, -1.36%, -0.14%|
|full/ram/ulmdb/reopen/environment_drop_ns|ns|86,509.174|89,734.930|+3.73%|-4.45%, +1.48%, +11.31%, +7.27%|
|full/ram/ulmdb/reopen/experimental_reenable_ns|ns|2,135.086|2,042.885|-4.32%|+4.10%, -16.87%, -15.56%, +14.70%|
|full/ram/ulmdb/reopen/open_ns|ns|42,145,038.531|41,970,125.998|-0.42%|+1.02%, +0.18%, -2.14%, -0.69%|
|full/ram/ulmdb/reopen/validation_ns|ns|6,023,763.534|6,215,432.016|+3.18%|+5.15%, +2.72%, +1.91%, +2.97%|
|full/ram/ulmdb/setup/create|ns|46,269,005.879|46,279,765.786|+0.02%|+0.77%, -0.13%, -1.66%, +1.13%|

All process-level raw write tails, microseconds:

|FS / batch / engine / arm|Pair|Position|Trial p99|Pooled p99|Maximum|
|---|---:|---:|---:|---:|---:|
|disk/1/ulmdb/R|1|1|86.823|312.985|315.119|
|disk/1/lmdb/R|1|1|187.737|329.897|332.732|
|disk/64/ulmdb/R|1|1|258.496|591.775|596.555|
|disk/64/lmdb/R|1|1|432.090|526.481|550.406|
|disk/1/ulmdb/C|1|2|74.262|100.341|312.094|
|disk/1/lmdb/C|1|2|188.165|329.997|332.913|
|disk/64/ulmdb/C|1|2|256.488|367.128|368.742|
|disk/64/lmdb/C|1|2|496.826|535.438|551.208|
|ram/1/ulmdb/R|1|1|11.426|12.434|12.895|
|ram/1/lmdb/R|1|1|7.900|8.486|12.473|
|ram/64/ulmdb/R|1|1|100.296|246.348|287.126|
|ram/64/lmdb/R|1|1|90.951|100.381|105.009|
|ram/1/ulmdb/C|1|2|11.681|15.410|15.650|
|ram/1/lmdb/C|1|2|8.449|8.676|33.093|
|ram/64/ulmdb/C|1|2|103.229|242.631|255.305|
|ram/64/lmdb/C|1|2|93.973|101.253|104.499|
|disk/1/ulmdb/C|2|1|81.995|310.080|398.959|
|disk/1/lmdb/C|2|1|194.866|330.448|337.843|
|disk/64/ulmdb/C|2|1|233.720|374.683|563.311|
|disk/64/lmdb/C|2|1|438.891|522.713|533.584|
|disk/1/ulmdb/R|2|2|84.129|312.955|319.548|
|disk/1/lmdb/R|2|2|169.240|320.329|330.048|
|disk/64/ulmdb/R|2|2|188.695|359.945|369.804|
|disk/64/lmdb/R|2|2|438.306|518.295|535.257|
|ram/1/ulmdb/C|2|1|11.197|11.733|11.973|
|ram/1/lmdb/C|2|1|8.262|12.263|12.293|
|ram/64/ulmdb/C|2|1|100.734|239.345|252.950|
|ram/64/lmdb/C|2|1|90.301|96.854|98.537|
|ram/1/ulmdb/R|2|2|11.788|17.223|17.273|
|ram/1/lmdb/R|2|2|8.071|9.077|12.574|
|ram/64/ulmdb/R|2|2|101.505|245.758|264.382|
|ram/64/lmdb/R|2|2|91.176|98.628|101.513|
|disk/1/ulmdb/C|3|1|77.716|144.414|310.601|
|disk/1/lmdb/C|3|1|188.071|324.036|339.936|
|disk/64/ulmdb/C|3|1|200.745|364.263|372.298|
|disk/64/lmdb/C|3|1|462.077|519.207|521.652|
|disk/1/ulmdb/R|3|2|76.497|280.874|317.294|
|disk/1/lmdb/R|3|2|225.772|330.559|511.242|
|disk/64/ulmdb/R|3|2|202.067|381.756|633.294|
|disk/64/lmdb/R|3|2|462.130|530.329|535.017|
|ram/1/ulmdb/C|3|1|11.822|15.189|17.062|
|ram/1/lmdb/C|3|1|7.753|8.276|11.723|
|ram/64/ulmdb/C|3|1|100.678|245.687|263.201|
|ram/64/lmdb/C|3|1|91.720|95.341|96.353|
|ram/1/ulmdb/R|3|2|11.873|16.501|17.874|
|ram/1/lmdb/R|3|2|7.527|7.945|8.426|
|ram/64/ulmdb/R|3|2|100.448|244.284|263.752|
|ram/64/lmdb/R|3|2|88.911|92.876|93.327|
|disk/1/ulmdb/R|4|1|69.860|110.260|312.594|
|disk/1/lmdb/R|4|1|170.930|320.219|321.652|
|disk/64/ulmdb/R|4|1|229.496|371.256|618.646|
|disk/64/lmdb/R|4|1|469.788|512.795|513.367|
|disk/1/ulmdb/C|4|2|80.548|310.380|318.806|
|disk/1/lmdb/C|4|2|193.463|324.317|324.477|
|disk/64/ulmdb/C|4|2|239.401|411.573|438.003|
|disk/64/lmdb/C|4|2|452.804|532.863|696.013|
|ram/1/ulmdb/R|4|1|11.657|15.870|16.511|
|ram/1/lmdb/R|4|1|7.870|8.646|12.053|
|ram/64/ulmdb/R|4|1|100.764|252.089|271.586|
|ram/64/lmdb/R|4|1|91.022|94.740|107.163|
|ram/1/ulmdb/C|4|2|12.198|15.390|34.857|
|ram/1/lmdb/C|4|2|8.009|8.616|11.642|
|ram/64/ulmdb/C|4|2|102.512|254.935|260.645|
|ram/64/lmdb/C|4|2|89.338|92.055|92.446|

All204 metric/control/diagnostic cells, paired observations, unavailable values and uncertainty are retained in comparison.json/comparison.md. Raw write offsets/maxima are retained in raw-write-review.json; original reports and fixtures are unchanged. Four neighboring process pairs are the experimental units, not16 per-process trials. Native archive difference was predeclared and exact current native metadata remained strict.



All 226 diagnostic cells and 103 targets: [combined comparison](benchmark/work/optimization-20261105/next-dense-validation/retained6-page-v1/full-screen-v1-independent-review/combined25-comparison.md); [source/analysis receipt](benchmark/work/optimization-20261105/next-dense-validation/retained6-page-v1/full-screen-v1-independent-review/combined25-verification.json).

## Sustained single-write control: all 16 positions and every read control

All eight fixed processes pass. The original successful first D observation was retained byte-for-byte and followed by exactly seven processes; no engine rerun or exclusion occurred. The historical native archive difference was bound before resume, while each current full native identity stayed strict. This is the private held-7 source context, not a combined retained-6 version.

### Complete sustained result and all 16 positions

All8 fixed processes pass; the original successful firstD observation was reused byte-for-byte, followed by exactly7 new processes. No engine process was rerun or excluded. The zero-engine preflight stop is separately preserved. Historical native metadata differs only in the predeclared archive hash; current full metadata remains strict. This private source contains held7 return control and cannot be spliced into the retained6 scorecard.

All original13 reads, full per-trial value checks, normal sync and cold/old-reader/pointer oracles remain. S repeats the original128 permuted keys, not a new random-key workload. All256 commits/case and4096 S commits/process remain charged, including first and later maintenance. Combined means exclude original outside-phase validation/dispatch gaps; they are not whole-process wall time.

Position0 has geometric combined75,189/s versus native137,973/s (-45.50%). Final4 positions cost10.697–10.775us/commit versus native7.018–7.083us: all4 process rate gaps remain34.01–34.39%. Final4 within-process cost spreads are0.28–1.92%; these are descriptive, not equivalence or a universal stable floor. First8/last8 views and every position remain below.

All4096 S commits cost44.251–44.611ms versus native28.805–29.081ms (34.81–35.07% slower throughput). Each case has256 business increments and one additional physical-LSN increment. First last-Collect cache snapshot is0hits/2misses, later2/0; these are NOT cumulative counters or all engine caches.

Final4 windows each contain512 sampled transactions: ULMDB pooled p9912.454–14.708us versus native8.015–8.747us. ULMDB maxima are200.231/205.050/212.825/202.816us, all retained even when pooled p99 misses sparse events. Initial windows contain128 samples; these are different-size descriptive quantiles. No cold/Collect transaction is removed.

## All13 read controls, all four pairs

Reads precede write inventory selection. D/S read changes are process/context controls, not causal effects of later writes. No native subtraction. Rates are geometric centers across4 processes; JSON retains all52 absolute read pairs and tails.

|Read case|D ULMDB/s|S ULMDB/s|S/D change|ULMDB paired changes1–4|D native/s|S native/s|Native paired changes1–4|S vs native|
|---|---:|---:|---:|---|---:|---:|---|---:|
|ordered_kv_mixed|7,173,836|7,145,707|-0.39%|-1.08%, +0.10%, +0.32%, -0.90%|4,102,716|4,118,377|+0.05%, +0.95%, +0.88%, -0.34%|+73.51%|
|ordered_kv_hits|7,488,615|7,392,885|-1.28%|-2.32%, -1.51%, -0.23%, -1.05%|4,180,166|4,189,194|+0.37%, +0.16%, +0.68%, -0.34%|+76.48%|
|ordered_kv_misses|7,525,165|7,518,609|-0.09%|+0.73%, -0.07%, -0.75%, -0.25%|4,029,865|4,025,052|+0.13%, +0.25%, +0.17%, -1.02%|+86.80%|
|ipv4_lpm_mixed|5,467,456|5,502,635|+0.64%|+1.71%, +0.64%, -0.03%, +0.26%|2,680,649|2,699,147|+3.83%, -5.13%, +3.53%, +0.79%|+103.87%|
|ipv4_lpm_hits|3,170,350|3,183,593|+0.42%|+0.96%, +0.47%, +0.32%, -0.08%|2,129,804|2,143,497|+2.18%, -1.23%, +0.26%, +1.39%|+48.52%|
|ipv4_lpm_misses|21,375,383|21,502,297|+0.59%|+0.36%, +0.95%, +0.69%, +0.38%|3,989,651|3,986,062|+4.64%, -10.96%, +5.64%, +1.23%|+439.44%|
|ipv4_lpm_outside_cover_misses|100,790,991|100,546,861|-0.24%|-0.89%, -0.08%, +0.00%, -0.01%|95,570,881|95,437,679|-19.29%, -0.00%, -0.01%, +23.22%|+5.35%|
|ipv4_lpm_inside_gap_misses|11,494,199|11,561,799|+0.59%|+3.24%, -0.73%, +0.47%, -0.57%|2,065,485|2,047,959|+4.87%, -10.27%, +5.18%, -2.35%|+464.55%|
|ipv6_lpm_mixed|4,344,730|4,316,536|-0.65%|-0.74%, -0.63%, -0.53%, -0.70%|1,568,797|1,563,836|+0.47%, -2.83%, +2.24%, -1.07%|+176.02%|
|ipv6_lpm_hits|2,507,190|2,497,679|-0.38%|+0.15%, -1.30%, -0.07%, -0.29%|1,250,963|1,246,304|+0.51%, -2.96%, +3.03%, -1.96%|+100.41%|
|ipv6_lpm_misses|16,999,079|17,034,560|+0.21%|-0.23%, +0.36%, +0.93%, -0.22%|2,245,642|2,253,665|+1.21%, -1.27%, +1.27%, +0.23%|+655.86%|
|ipv6_lpm_outside_cover_misses|95,251,563|96,984,684|+1.82%|+3.42%, +0.11%, +0.68%, +3.12%|94,706,622|94,467,717|-18.49%, +3.96%, -0.08%, +16.91%|+2.66%|
|ipv6_lpm_inside_gap_misses|9,212,291|9,255,635|+0.47%|+2.17%, +0.34%, +0.60%, -1.21%|1,132,900|1,138,388|+1.85%, -1.88%, +1.36%, +0.65%|+713.05%|

## Interpretation

The sustained control removes batch64 and confirms first-maintenance cost. Later fully charged cases still trail native by about34%, with material maintenance outliers. Allocation/history and both engines caches evolve; original value-validation loops can warm data. This does not prove pure CPU-cache startup, justify a free production warmup, or establish a no-Collect floor. Keep canonical first-use costs and full steady charged costs. The exact-comparison census and current-source profiling must identify remaining work before a production architecture change.

All16 sustained positions retained; combined costs include every aggregate and sampled commit.

| Case position | ULMDB aggregate/s | LMDB aggregate/s | Aggregate vs native | ULMDB combined/s | LMDB combined/s | Combined vs native | ULMDB pooled p99 ns | LMDB pooled p99 ns | Tail cost vs native |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
|0|62110.3|134451.8|-53.80%|75188.5|137973.1|-45.50%|14503.8|10655.7|+36.11%|
|1|100199.8|141145.5|-29.01%|94020.6|141837.5|-33.71%|12793.8|10493.0|+21.93%|
|2|98352.1|141370.4|-30.43%|93377.3|142685.4|-34.56%|15458.8|8024.5|+92.65%|
|3|86734.9|141226.8|-38.58%|93729.7|142269.9|-34.12%|13488.1|8508.7|+58.52%|
|4|99491.4|140148.6|-29.01%|93950.0|141967.0|-33.82%|14285.7|8164.0|+74.98%|
|5|100553.6|140055.9|-28.20%|93790.9|141666.7|-33.79%|14748.2|8986.0|+64.12%|
|6|85730.5|140901.5|-39.16%|93183.3|142284.6|-34.51%|12929.7|8269.0|+56.36%|
|7|99643.2|141208.3|-29.44%|94019.0|141731.8|-33.66%|14103.3|9078.6|+55.35%|
|8|99587.9|141146.1|-29.44%|93703.4|142532.0|-34.26%|14369.0|7893.4|+82.04%|
|9|86482.1|140762.1|-38.56%|93776.9|141837.0|-33.88%|12641.6|9071.9|+39.35%|
|10|87233.1|140832.1|-38.06%|94043.5|141697.2|-33.63%|12976.0|11373.6|+14.09%|
|11|99729.4|140702.3|-29.12%|93783.5|141984.3|-33.95%|13600.6|9102.9|+49.41%|
|12|86449.9|139167.5|-37.88%|93275.6|141305.9|-33.99%|13500.6|9106.5|+48.25%|
|13|86106.8|140830.9|-38.86%|93517.1|141464.3|-33.89%|11136.3|10201.6|+9.16%|
|14|99468.5|140892.0|-29.40%|93345.9|142367.2|-34.43%|15148.3|7907.9|+91.56%|
|15|85285.9|141325.5|-39.65%|92804.7|142197.4|-34.74%|14028.0|9580.2|+46.43%|

First versus predeclared final4 positions, every process retained. First window has128 sampled transactions/engine; final4 has512. These are descriptive quantiles with different sample counts.

|Pair|Window|ULMDB combined ns/commit|LMDB combined ns/commit|Rate vs native|ULMDB pooled p99 ns|LMDB pooled p99 ns|Tail cost vs native|
|---|---|---:|---:|---:|---:|---:|---:|
|1|first position0|13266.4|7258.0|-45.29%|15860|14317|+10.78%|
|1|final positions12-15|10697.4|7018.2|-34.39%|14678|8285|+77.16%|
|2|first position0|13311.4|7228.5|-45.70%|13787|8255|+67.01%|
|2|final positions12-15|10774.7|7080.6|-34.29%|12454|8747|+42.38%|
|3|first position0|13055.0|7155.7|-45.19%|11903|11632|+2.33%|
|3|final positions12-15|10697.4|7021.0|-34.37%|14237|8015|+77.63%|
|4|first position0|13571.8|7350.4|-45.84%|17002|9378|+81.30%|
|4|final positions12-15|10733.4|7083.4|-34.01%|14708|8667|+69.70%|

All charged process totals; D and S have different declared write inventories, so their whole-process rates are not interchangeable.

|Pair|Mode|Commits per engine|ULMDB charged ms|LMDB charged ms|Rate vs native|
|---|---|---:|---:|---:|---:|
|1|D|512|19.556|24.109|+23.28%|
|1|S|4096|44.373|28.813|-35.07%|
|2|S|4096|44.456|28.903|-34.98%|
|2|D|512|19.523|23.872|+22.28%|
|3|D|512|19.612|23.480|+19.72%|
|3|S|4096|44.251|28.805|-34.91%|
|4|S|4096|44.611|29.081|-34.81%|
|4|D|512|19.706|23.495|+19.23%|

All72 individual ordered case rows, all8 charged process totals, paired first-position controls,
final4 individual positions and stability spreads remain in analysis-v1.json.
These are descriptive diagnostics; no stable-floor or equivalence claim follows automatically.



All 72 individual ordered write-case rows and 52 read pairs remain in [the full analysis](benchmark/work/optimization-20261105/next-sustained-single/explicit-v1/analysis-v1.json) and [read controls](benchmark/work/optimization-20261105/next-sustained-single/explicit-v1/read-controls-v1.json). [Verification](benchmark/work/optimization-20261105/next-sustained-single/explicit-v1/analysis-verification-v1.json) binds the reused first report/raw hashes and every completed process.

## Quiet census: operation counts verified, recording overhead gate failed

All 15 quiet processes pass the original guards. The four quiet ON captures match each other and both earlier co-loaded captures exactly. Recording adds 10.80% RAM-single aggregate cost and 6.15% combined cost; all four paired changes are adverse and exceed the declared 5% overhead gate. These timings cannot supply CPU fractions, production rates or a speedup. Whole operation counts remain valid. Native OFF/pristine -10.21% variation is retained without subtraction or normalization.

### All quiet control tables

All3smokes+12fixed full processes passed original full15 semantic/native/source/ELF/config/raw/tail/cold/accounting guards. The4on captures exactly equal each other AND both earlier co-loaded captures. First context/read/live values remain [0,246,246]/6,554/123; all11contexts, allocation/page/span hits, bins and zero/error/budget/unavailable/overflow values agree. Counts are established; recording is too intrusive for CPU attribution.

The5% perturbation gate FAILS: Resident RAM1 aggregate cost on/off +10.80% (all4paired increases +8.74,+13.97,+10.24,+10.25%), complete256-commit combined cost +6.15%. Native off/pristine aggregate cost is itself −10.21%; that variation is retained without normalization or dismissal. No accepted speedup/nonregression or CPU-share claim follows. This is sealed held source7 diagnostic context, not a new retained6 headline.

All97 evidence files across15fixtures (111,219,260bytes) are SHA256-verified outsideRAM in preserved-fixture-evidence; no fixtures were deleted. Raw pooling across multiple files is deferred during Page timing. Tail means below are explicitly means of process summaries, not one newly pooled percentile.

## Aggregate rates and recording perturbation

Rates are reads/s or write records/s. Cost changes are positive when slower. All paired signs/tails remain in analysis.json.

|Case|Engine|Pristine rate|Off rate|On rate|Off/P cost|On/off cost|
|---|---|---:|---:|---:|---:|---:|
|ordered_kv_mixed|ulmdb|7,048,045|7,075,202|7,073,427|-0.38%|+0.03%|
|ordered_kv_mixed|lmdb|4,101,736|4,095,546|4,075,040|+0.15%|+0.50%|
|ordered_kv_hits|ulmdb|7,337,914|7,352,688|7,362,236|-0.20%|-0.13%|
|ordered_kv_hits|lmdb|4,179,169|4,170,925|4,169,274|+0.20%|+0.04%|
|ordered_kv_misses|ulmdb|7,400,780|7,358,745|7,413,023|+0.57%|-0.73%|
|ordered_kv_misses|lmdb|4,014,041|3,992,555|4,005,734|+0.54%|-0.33%|
|ipv4_lpm_mixed|ulmdb|5,478,692|5,487,943|5,494,167|-0.17%|-0.11%|
|ipv4_lpm_mixed|lmdb|2,660,188|2,536,916|2,583,362|+4.86%|-1.80%|
|ipv4_lpm_hits|ulmdb|3,185,777|3,165,447|3,183,002|+0.64%|-0.55%|
|ipv4_lpm_hits|lmdb|2,123,603|2,071,792|2,072,885|+2.50%|-0.05%|
|ipv4_lpm_misses|ulmdb|21,486,255|21,579,421|21,485,623|-0.43%|+0.44%|
|ipv4_lpm_misses|lmdb|3,882,685|3,714,406|3,834,815|+4.53%|-3.14%|
|ipv4_lpm_outside_cover_misses|ulmdb|100,713,809|100,932,681|99,788,573|-0.22%|+1.15%|
|ipv4_lpm_outside_cover_misses|lmdb|93,802,993|95,523,386|96,217,162|-1.80%|-0.72%|
|ipv4_lpm_inside_gap_misses|ulmdb|11,620,817|11,578,004|11,542,562|+0.37%|+0.31%|
|ipv4_lpm_inside_gap_misses|lmdb|2,023,477|1,889,869|1,973,395|+7.07%|-4.23%|
|ipv6_lpm_mixed|ulmdb|4,376,512|4,370,490|4,353,230|+0.14%|+0.40%|
|ipv6_lpm_mixed|lmdb|1,567,223|1,540,701|1,535,036|+1.72%|+0.37%|
|ipv6_lpm_hits|ulmdb|2,502,337|2,518,107|2,509,345|-0.63%|+0.35%|
|ipv6_lpm_hits|lmdb|1,246,169|1,233,264|1,223,336|+1.05%|+0.81%|
|ipv6_lpm_misses|ulmdb|17,307,322|16,964,576|16,832,829|+2.02%|+0.78%|
|ipv6_lpm_misses|lmdb|2,238,222|2,222,149|2,196,890|+0.72%|+1.15%|
|ipv6_lpm_outside_cover_misses|ulmdb|97,384,466|96,176,387|96,599,253|+1.26%|-0.44%|
|ipv6_lpm_outside_cover_misses|lmdb|95,089,518|86,112,595|96,395,704|+10.42%|-10.67%|
|ipv6_lpm_inside_gap_misses|ulmdb|9,376,839|9,242,444|9,183,116|+1.45%|+0.65%|
|ipv6_lpm_inside_gap_misses|lmdb|1,130,599|1,120,541|1,110,321|+0.90%|+0.92%|
|write_batch1|ulmdb|61,927|63,714|57,502|-2.80%|+10.80%|
|write_batch1|lmdb|122,382|136,300|134,252|-10.21%|+1.53%|
|write_batch64|ulmdb|990,865|1,002,662|1,005,630|-1.18%|-0.30%|
|write_batch64|lmdb|733,345|757,230|751,672|-3.15%|+0.74%|

## Tails retained

|Case|Engine|Mean trial p99 P/off/on ns|Mean per-process pooled p99 P/off/on ns|Worst paired on/off trial-p99 cost|
|---|---|---|---|---:|
|ordered_kv_mixed|ulmdb|211/214/214|213/216/213|+0.74%|
|ordered_kv_mixed|lmdb|346/340/342|350/343/343|+1.57%|
|ordered_kv_hits|ulmdb|205/209/208|206/211/213|+6.17%|
|ordered_kv_hits|lmdb|346/343/339|351/351/341|+2.35%|
|ordered_kv_misses|ulmdb|205/211/207|213/218/208|+0.12%|
|ordered_kv_misses|lmdb|351/350/343|358/358/343|-1.06%|
|ipv4_lpm_mixed|ulmdb|428/429/423|428/431/423|+0.37%|
|ipv4_lpm_mixed|lmdb|780/777/776|779/779/776|+4.48%|
|ipv4_lpm_hits|ulmdb|435/435/436|436/434/434|+1.74%|
|ipv4_lpm_hits|lmdb|807/792/794|812/797/794|+3.67%|
|ipv4_lpm_misses|ulmdb|124/123/122|126/126/121|+0.87%|
|ipv4_lpm_misses|lmdb|595/626/646|596/636/648|+7.90%|
|ipv4_lpm_outside_cover_misses|ulmdb|42/42/41|43/48/43|+0.00%|
|ipv4_lpm_outside_cover_misses|lmdb|48/48/50|48/48/50|+21.95%|
|ipv4_lpm_inside_gap_misses|ulmdb|122/122/122|126/131/128|+0.56%|
|ipv4_lpm_inside_gap_misses|lmdb|612/624/654|618/641/654|+6.94%|
|ipv6_lpm_mixed|ulmdb|526/518/520|531/518/521|+1.33%|
|ipv6_lpm_mixed|lmdb|1,245/1,260/1,244|1,250/1,255/1,245|+1.65%|
|ipv6_lpm_hits|ulmdb|534/532/532|544/536/534|+0.59%|
|ipv6_lpm_hits|lmdb|1,266/1,254/1,262|1,295/1,253/1,260|+2.77%|
|ipv6_lpm_misses|ulmdb|143/142/145|148/141/150|+5.19%|
|ipv6_lpm_misses|lmdb|1,101/1,088/1,109|1,122/1,087/1,115|+4.01%|
|ipv6_lpm_outside_cover_misses|ulmdb|41/41/42|41/41/41|+2.90%|
|ipv6_lpm_outside_cover_misses|lmdb|44/44/46|50/48/50|+11.83%|
|ipv6_lpm_inside_gap_misses|ulmdb|142/145/149|148/150/150|+6.31%|
|ipv6_lpm_inside_gap_misses|lmdb|1,097/1,107/1,125|1,134/1,104/1,147|+5.50%|
|write_batch1|ulmdb|11,902/12,360/11,857|15,134/16,797/15,514|+5.97%|
|write_batch1|lmdb|8,696/7,942/8,231|11,382/8,461/10,938|+9.61%|
|write_batch64|ulmdb|131,687/131,202/129,585|255,168/254,083/257,454|+0.06%|
|write_batch64|lmdb|96,085/91,789/91,443|104,218/97,132/96,436|+3.07%|

## Complete business cost

|Batch|Engine|Combined ns/commit P/off/on|Off/P cost|On/off cost|Combined p50/p95/p99 by mode|
|---|---|---|---:|---:|---|
|1|ulmdb|13,339/13,091/13,896|-1.86%|+6.15%|pristine:10,340/11,780/15,134; off:10,247/11,161/16,797; on:10,240/11,126/15,514|
|1|lmdb|7,665/7,162/7,246|-6.56%|+1.17%|pristine:6,901/8,158/11,382; off:6,840/7,630/8,461; on:6,846/7,842/10,938|
|64|ulmdb|64,100/63,369/63,154|-1.14%|-0.34%|pristine:53,604/80,072/255,168; off:52,966/80,971/254,083; on:52,973/61,878/257,454|
|64|lmdb|86,282/83,817/84,227|-2.86%|+0.49%|pristine:84,168/95,401/104,218; off:82,392/91,947/97,132; on:82,922/92,360/96,436|

## Cold and resources

|Cold component|Pristine ms|Off ms|On ms|On/off cost|
|---|---:|---:|---:|---:|
|environment_drop_ns|0.086|0.092|0.095|+2.58%|
|open_ns|41.882|42.940|43.036|+0.22%|
|experimental_reenable_ns|0.002|0.002|0.002|+0.19%|
|validation_ns|6.106|6.132|6.190|+0.95%|
|complete_cold_check_ns|52.299|53.397|53.508|+0.21%|

|Mode|Whole-process RSS before mean/range KiB|After mean/range KiB|
|---|---|---|
|pristine|13301 / 13256–13352|14701 / 14664–14736|
|off|13364 / 13348–13372|14744 / 14728–14752|
|on|13364 / 13316–13412|14768 / 14740–14800|

All numeric backend policy/accounting values match across12full runs after excluding opaque commit IDs: sequence1→513, WAL selected end725,136→2,205,392bytes,7,722reused pages; pool mapping admission8,388,608bytes, metadata bound13,320,224bytes, private charge283,312bytes, frame workspace528,528bytes and recipe workspace9,216bytes. Last Collect cache stats are2hits/0misses/2entries/2,368bytes, not cumulative totals.

Resident file allocation remains67,108,864bytes; anonymous Resident mapped RSS/PSS are unavailable, never zero. Native apparent/allocated file bytes and pathname mapping accounting, every creation cost and per-process RSS are retained in analysis.json. Whole-process RSS includes both engines, fixtures and recorder; metadata admission bounds are not measured RSS. Every adverse read/write/tail/cold point stays in the tables or paired machine-readable records.

For allocation policy interpretation see ../counts-only-v1-1/COUNTS_ONLY_REPORT.md: same canonical allocation hits26.32% first-cold, same VersionKey88.60%, prior-span0.64%; all6on captures share those exact counts. The present controls provide no reason to bypass bounds or implement a binding cache without its own source proof and performance qualification.


### Exact count proof, confirmed again by all four quiet ON captures



|Family|Checked reads|Same VersionKey|Same canonical allocation|Prior returned span contains|Allocation hit % of reads|
|---|---:|---:|---:|---:|---:|
|catalog|7|6|6|6|85.71%|
|kv|68|34|34|34|50.00%|
|ipv4|2,879|2,570|780|1|27.09%|
|ipv6|3,600|3,197|905|1|25.14%|

Total:6,554 checked reads,5,807 same-page/version matches (88.60%),1,725 same-allocation matches (26.32%) and42 prior-span containment matches (0.64%). Prefix alone:6,479 reads,5,767 same-key matches (89.01%),1,685 allocation matches (26.01%) and2 prior-span matches (0.03%). These are percentages of operations, not CPU time.

Context is [0 prior Collects,sequence246,LSN246], live set123, cache [0 hits,2 misses,2 admissions]. The old source4 observations6,554/123/6,479 and first context all match exactly. Every request succeeds; zero-length/error/budget/unavailable-range/overflow counts are all zero. This confirms only this fixed fixture’s path; zero/generation/foreign/bounds safety still requires the already-written negative tests for any implementation.

All Tiny allocation runs are length1 or2: IPv4 has1,319 singleton runs and780 two-read runs; IPv6 has1,790 singleton runs and905 two-read runs. Source ReadStore::payload reads a16-byte header then following value within one Tiny allocation. The length bins and two-read runs are consistent with that mechanism; the census did not record semantic callsites, so it does not prove every pair is a payload header/value pair.

## Later cached Collects

|Ordinal|Prior Collects / sequence / LSN|Checked reads|Live keys|Cache hit/miss/admit|Same key / allocation / span|
|---|---|---:|---:|---|---|
|1|1 / 281 / 282|75|123|2 / 0 / 2|40 / 40 / 40|
|2|2 / 306 / 308|75|123|2 / 0 / 2|40 / 40 / 40|
|3|3 / 331 / 334|75|123|2 / 0 / 2|40 / 40 / 40|
|4|4 / 356 / 360|75|123|2 / 0 / 2|40 / 40 / 40|
|5|5 / 380 / 385|75|123|2 / 0 / 2|40 / 40 / 40|
|6|6 / 405 / 411|75|123|2 / 0 / 2|40 / 40 / 40|
|7|7 / 430 / 437|75|123|2 / 0 / 2|40 / 40 / 40|
|8|8 / 455 / 463|75|123|2 / 0 / 2|40 / 40 / 40|
|9|9 / 480 / 489|75|123|2 / 0 / 2|40 / 40 / 40|
|10|10 / 505 / 515|75|123|2 / 0 / 2|40 / 40 / 40|

Each later Collect performs7 catalog+68 KV Trace reads, with6+34 hits under each policy; prefixes use the validated cached closure and perform no traced prefix reads. Those cache membership/empty-endpoint checks occur outside Trace and are not included in these hit counts. Across all11 Collects:7,304 checked reads,6,207 same-key matches (84.98%),2,125 allocation matches (29.09%) and442 span matches (6.05%).



First-Collect operation percentages are not CPU shares. A span-only binding hits 0.64% of first-event reads, allocation binding 26.32%, and same-key matching 88.60%; miss processing can add work to existing O(1) bounds paths. No bitmap or binding-cache implementation follows from these counts. The next controlled design is documented in the ownership study below. [All 97 preserved files, 111,219,260 bytes](benchmark/work/optimization-20261105/next-collect-read-binding/private-census-v1/controls-v1-1/preservation-receipt.json) are verified outside RAM; no fixture was deleted.

## Next architecture and controls: ownership tracking, measured before adoption

[LMDB writes and a Resident ownership plan](LMDB_WRITE_ARCHITECTURE.md) compares the benchmark pin with the inspected repository default. Sixteen relevant function bodies match; complete-file equivalence is not claimed. Normal LMDB has no equivalent typed prefix-audit cache. Its OS-backed mappings and dirty/free/loose-page structures support a different ownership model.

The tracking-only and fresh retained-6 coarse controls are now complete. The existing generic certificate fails the tracking-cost gate: RAM-single aggregate throughput falls 76.01%, batch64 74.75%, in all four pairs. Normal writes retire zero objects and the original full Collect cadence and WAL history remain. No certificate issuer is implemented. The coarse instrumentation also fails its 5% perturbation gate, so it supports investigating the ordinary commit envelope without establishing production phase shares. Complete tables and limits follow.

## Latest completed controls: generic ownership tracking REJECTED; coarse attribution limited

**Retained 6 remains current.** Each private control completed two smoke and 12 full RAM processes with every original oracle and source/ELF/native/WAL guard passing. The tracking result rejects this existing generic certificate as a hot-write implementation: single-write aggregate throughput falls 76.01% and batch64 74.75%, all four pairs adverse. Single combined throughput falls 78.50%. This does not refute LMDB's principle of recording ownership changes at mutation; it establishes that this general graph-maintenance implementation is too costly. There is no new ledger issuer or production runtime change.

The coarse P/O/C result separately fails its 5% perturbation gate: native OFF/pristine single cost rises 8.71% aggregate / 9.81% combined and cumulative ON/pristine 11.60% / 12.33%. ULMDB cumulative combined cost rises 6.24%. No overhead is subtracted. The instrumented post-cold subset has only 40 singles per backend across four processes: ULMDB begin/mutation/commit 0.151/0.791/8.926us versus native 0.118/0.918/6.056us. These observed phase costs support a smaller commit-envelope control, not production CPU fractions or an estimated speedup. First/later Collect means 629.117/189.700us remain inside begin and business; they must not be added again or removed from rates.

### Complete coarse report, all 354 metric cells and all 120 phase strata

### Retained6 coarse phases: commit dominates the instrumented ordinary path; perturbation gate fails

This diagnostic establishes operation counts and instrumented phase costs. **It does not qualify uninstrumented phase shares or a production speedup.** The fixed2 smoke+12 full RAM journey passed every original oracle, source/ELF/native/raw/counter guard. All12 timed post-child observations were quiet; boundary checks do not prove continuous host isolation. No engine observation was retried or excluded. The first OFF smoke was reused byte-for-byte after an external-compiler preflight stop; the remaining13 scheduled processes ran once under the reviewed wait barrier.

P is frozen retained6; O is the new diagnostic ELF with stage clocks off; C uses that SAME ELF with coarse clocks on. Rows capture every original512 commits/backend/process (256 at each batch), with Collect nested in its original begin. OFF records0 stage clocks; C records8 per ordinary transaction and2 more for Collect. All8192 O/C rows and all354 metric cells are retained in [analysis.json](benchmark/work/optimization-20261105/next-retained6-stages/retained6-coarse-v1/analysis-v1/analysis.json); [metric tables](benchmark/work/optimization-20261105/next-retained6-stages/retained6-coarse-v1/analysis-v1/METRIC_TABLES.md), [all paired cells](benchmark/work/optimization-20261105/next-retained6-stages/retained6-coarse-v1/analysis-v1/METRIC_PAIRS.csv) and [all120 phase strata](benchmark/work/optimization-20261105/next-retained6-stages/retained6-coarse-v1/analysis-v1/PHASE_TABLES.md) preserve adverse points and unavailable fields.

#### Perturbation gate

Positive changes below are more elapsed cost. Values are geometric centers over4 processes/arm. The5% gate applies to P→O, O→C and cumulative P→C; no overhead is subtracted. On/off is relatively small, but the off/control overhead and cumulative difference fail for singles, especially Native. The exact cause is not established.

|Workload|P µs/commit|O µs/commit|C µs/commit|O/P cost|C/O cost|C/P cost|Over5%?|
|---|---:|---:|---:|---:|---:|---:|---|
|batch1 lmdb aggregate_cost|7.433|8.081|8.296|+8.71%|+2.66%|+11.60%|yes|
|batch1 lmdb combined_cost|7.202|7.908|8.090|+9.81%|+2.29%|+12.33%|yes|
|batch1 ulmdb aggregate_cost|16.127|16.746|16.865|+3.84%|+0.71%|+4.57%|no|
|batch1 ulmdb combined_cost|13.360|14.113|14.194|+5.64%|+0.57%|+6.24%|yes|
|batch64 lmdb aggregate_cost|85.185|86.427|88.074|+1.46%|+1.90%|+3.39%|no|
|batch64 lmdb combined_cost|84.270|85.478|86.989|+1.43%|+1.77%|+3.23%|no|
|batch64 ulmdb aggregate_cost|65.342|66.678|67.034|+2.04%|+0.53%|+2.59%|no|
|batch64 ulmdb combined_cost|64.693|66.107|66.408|+2.19%|+0.45%|+2.65%|no|

C/O alone is not sufficient to waive a failed P/O or P/C gate. These durations remain valid descriptions of this instrumented journey; percentages cannot be promoted to production CPU attribution or corrected by subtracting observed overhead.

#### Complete original business costs and direct Native comparison

Rates are records/s. Combined includes all128 aggregate+128 separately sampled commits, including maintenance; it is not total process wall time or setup time. No cold transaction is removed.

|Batch|Population|Arm|ULMDB records/s|Native records/s|ULMDB rate vs Native|
|---|---|---|---:|---:|---:|
|1|aggregate|P|62,009|134,527|-53.91%|
|1|aggregate|O|59,714|123,746|-51.74%|
|1|aggregate|C|59,296|120,543|-50.81%|
|1|combined|P|74,853|138,854|-46.09%|
|1|combined|O|70,855|126,448|-43.97%|
|1|combined|C|70,453|123,613|-43.00%|
|64|aggregate|P|979,462|751,307|+30.37%|
|64|aggregate|O|959,841|740,508|+29.62%|
|64|aggregate|C|954,746|726,665|+31.39%|
|64|combined|P|989,295|759,468|+30.26%|
|64|combined|O|968,122|748,728|+29.30%|
|64|combined|C|963,740|735,728|+30.99%|

Pristine6's combined singles still have46.09% less throughput than contemporary Native in this full, first-use-inclusive workload. The descriptive post-cold phase subset below is smaller and instrumented; it cannot replace that rate or the separately recorded S16 sustained control.

#### Instrumented phases, including ordinary transactions

The following are arithmetic means across all four C processes, both aggregate/sampled phases and both backend orders. All underlying phase/order/process strata, means, medians, p50/p95/p99/maxima remain in the JSON and PHASE_TABLES. Native category names align to the corresponding Resident transaction context; Native has no instrumented Collect operation.

|Resident context|Batch|Backend|Rows|Business µs|Begin µs|Mutation µs|Commit µs|Collect inside begin µs|
|---|---:|---|---:|---:|---:|---:|---:|---:|
|ordinary_no_collect_pre_first|1|ulmdb|980|11.069|0.153|1.363|9.399|unavailable|
|ordinary_no_collect_pre_first|1|lmdb|980|7.404|0.119|0.983|6.150|unavailable|
|first_cold_collect|1|ulmdb|4|640.989|629.409|0.979|10.415|629.116|
|first_cold_collect|1|lmdb|4|7.088|0.098|0.914|5.928|unavailable|
|ordinary_no_collect_post_first|1|ulmdb|40|10.021|0.151|0.791|8.926|unavailable|
|ordinary_no_collect_post_first|1|lmdb|40|7.237|0.118|0.918|6.056|unavailable|
|ordinary_no_collect_post_first|64|ulmdb|984|58.165|0.166|25.767|32.066|unavailable|
|ordinary_no_collect_post_first|64|lmdb|984|86.169|0.150|35.460|50.408|unavailable|
|later_cached_collect|64|ulmdb|40|248.400|189.955|25.958|32.289|189.700|
|later_cached_collect|64|lmdb|40|86.027|0.155|35.585|50.136|unavailable|

There are only10 post-first-Collect ordinary singles per process (40 total/backend), because the first event occurs late in batch1. The980 earlier ordinary rows are not post-cold warm. Instrumented post-cold ULMDB singles spend8.926µs in commit out of10.021µs business; aligned Native commit is6.056µs out of7.237µs. This supports investigating the **commit envelope** next; it does not establish those exact production costs or a production percentage split after the failed perturbation gate.

Collect is nested inside begin and business. Do not add it again. First cold Collect averages629.117µs; later cached Collect averages189.700µs. Subtracting Collect from its containing begin is merely a displayed instrumented nesting residual, not subtraction from the original benchmark or a maintenance-free rate. Business-minus-begin/mutation/commit residuals retain wrapper/clock bookkeeping rather than pretending all work has been attributed.

#### Actual event counts and maintenance contexts

All43 counter slots match exactly OFF versus ON for every logical row in each of the four pairs. Each process records512 business publications plus11 Collect publications. The first occurs at batch1/trial15/aggregate/ordinal5 (zero-based), before business sequence246/LSN246. It reads6554 graph extents, retires735 logical versions, and records0 cache hits/2 misses. Ten later Collects each read75 extents, record2 hits/0 misses and89 membership checks; all original contexts and varying retired counts remain in analysis.json. Cache fields are per-audit snapshots, not cumulative cache activity or CPU shares.

|Per-process event|Business|Collect|Total|
|---|---:|---:|---:|
|Publications|512|11|523|
|Witness block0 read_exact completed|512|11|523|
|Witness block1 read_exact completed|512|11|523|
|Both witness requested bytes|262,144|5,632|267,776|
|Fresh capacity query completed|512|11|523|
|WAL write_cut completed|512|11|523|
|WAL successful requested bytes|1,345,152|135,104|1,480,256|
|Data sync completed|512|11|523|
|Witness write_cut/sync completed, each|512|11|523|
|Witness successful requested bytes|131,072|2,816|133,888|
|Journal/facade activation, each|512|11|523|
|Candidate activation|512|0|512|
|Logical versions retired|0|8,246|8,246|
|Completed-audit graph reads|unavailable|7,304|7,304 audited reads|
|Reset attempts|0|0|0|

These are API invocation/completion and successful requested-byte counts, **not** kernel syscall counts, partial-I/O accounting or time shares. A completed witness read is counted before equality validation; it is not independently proof that a guard accepted. The actual capacity method is unwired/null; no method was inferred from an initialized zero. Native I/O counters are unavailable because its C implementation was unchanged. A failed partial audit's read cost would be null, and retired_versions increases only after successful retirement publication.

#### Tail and read regressions remain visible

All read, write, creation, storage, whole-process RSS and cold metrics are included in METRIC_TABLES. In particular, single-write Native mean-trial p99 increases from7.945µs(P) to10.510µs(C); pooled p99 rises9.388→12.701µs. ULMDB singles mean-trial p99 rises11.944→14.003µs and pooled p9915.994→16.224µs. These adverse diagnostic changes reinforce the failed perturbation gate. Per-process sample maxima are retained; the displayed metric tables' maximum centers are geometric summaries of process maxima, not a universal worst case or a pooled maximum.

#### Supported next question

The one supported next profiling target is **Resident's ordinary commit envelope**: determine how much of the instrumented commit work belongs to preparation versus guard/data/witness publication, retaining activation/owner-release costs. The coarse result supports that location, not a claim that a particular syscall, reference count or layout is the cause. Any finer attribution needs its own minimal controlled measurement and perturbation gate. No further instrumentation, engine change, profile or benchmark was started by this analysis. Tracking-only ownership costs are a separate control and cannot be credited with fixing the warm commit gap before their actual result.

The sources, original and corrected private builds, proof projections, Native metadata/archive bindings and every counter correction remain in the parent package. See [final inputs](benchmark/work/optimization-20261105/next-retained6-stages/retained6-coarse-v1/FINAL_INPUTS.json), [corrected qualification](benchmark/work/optimization-20261105/next-retained6-stages/retained6-coarse-v1/build-qualification-v2-receipt.json), [capture contract](benchmark/work/optimization-20261105/next-retained6-stages/retained6-coarse-v1/CAPTURE_CONTRACT.md), [fixed summary](benchmark/work/optimization-20261105/next-retained6-stages/retained6-coarse-v1/controls-v1/summary.json), [original seal](benchmark/work/optimization-20261105/next-retained6-stages/retained6-coarse-v1/screen-seal-v1.json), [resume seal](benchmark/work/optimization-20261105/next-retained6-stages/retained6-coarse-v1/resume-seal-v3.json) and [analysis receipt](benchmark/work/optimization-20261105/next-retained6-stages/retained6-coarse-v1/analysis-v1/receipt.json). This is diagnostic evidence; retained6 remains unchanged.


### All coarse metric cells

P/O/C columns are geometric centers only when every paired observation is positive. Raw process values, zeros and nulls remain in the CSV/JSON. Changes are value ratios; higher is worse for elapsed costs and better for rates. Resource/count ratios are descriptive, not an optimization verdict.

|Metric|Unit|P|O|C|O/P|C/O|C/P|
|---|---|---:|---:|---:|---:|---:|---:|
|after.lmdb.allocated_file_bytes|reported scalar|729,088.000|729,088.000|729,088.000|+0.00%|+0.00%|+0.00%|
|after.lmdb.apparent_file_bytes|reported scalar|733,184.000|733,184.000|733,184.000|+0.00%|+0.00%|+0.00%|
|after.lmdb.files|reported scalar|2.000|2.000|2.000|+0.00%|+0.00%|+0.00%|
|after.lmdb.mapped_pss_kib|reported scalar|712.000|712.000|712.000|+0.00%|+0.00%|+0.00%|
|after.lmdb.mapped_rss_kib|reported scalar|712.000|712.000|712.000|+0.00%|+0.00%|+0.00%|
|after.lmdb.mapped_virtual_kib|reported scalar|524,296.000|524,296.000|524,296.000|+0.00%|+0.00%|+0.00%|
|after.ulmdb.allocated_file_bytes|reported scalar|67,108,864.000|67,108,864.000|67,108,864.000|+0.00%|+0.00%|+0.00%|
|after.ulmdb.apparent_file_bytes|reported scalar|67,108,864.000|67,108,864.000|67,108,864.000|+0.00%|+0.00%|+0.00%|
|after.ulmdb.files|reported scalar|2.000|2.000|2.000|+0.00%|+0.00%|+0.00%|
|after.ulmdb.mapped_pss_kib|unavailable|[None, None, None, None]|[None, None, None, None]|[None, None, None, None]|unavailable|unavailable|unavailable|
|after.ulmdb.mapped_rss_kib|unavailable|[None, None, None, None]|[None, None, None, None]|[None, None, None, None]|unavailable|unavailable|unavailable|
|after.ulmdb.mapped_virtual_kib|unavailable|[None, None, None, None]|[None, None, None, None]|[None, None, None, None]|unavailable|unavailable|unavailable|
|after.whole_process_vm_rss|KiB whole process, both engines|14,717.998|15,632.981|15,642.995|+6.22%|+0.06%|+6.28%|
|backend_after.auto_collect_below|reported scalar|256.000|256.000|256.000|+0.00%|+0.00%|+0.00%|
|backend_after.collect_cache.entries|reported scalar|2.000|2.000|2.000|+0.00%|+0.00%|+0.00%|
|backend_after.collect_cache.hits|reported scalar|2.000|2.000|2.000|+0.00%|+0.00%|+0.00%|
|backend_after.collect_cache.misses|reported scalar|[0, 0, 0, 0]|[0, 0, 0, 0]|[0, 0, 0, 0]|unavailable|unavailable|unavailable|
|backend_after.collect_cache.retained_bytes|reported scalar|2,368.000|2,368.000|2,368.000|+0.00%|+0.00%|+0.00%|
|backend_after.pool_kv_pages|reported scalar|1,024.000|1,024.000|1,024.000|+0.00%|+0.00%|+0.00%|
|backend_after.pool_metadata_bound_bytes|reported scalar|13,320,224.000|13,320,224.000|13,320,224.000|+0.00%|+0.00%|+0.00%|
|backend_after.pool_reused_pages|reported scalar|7,722.000|7,722.000|7,722.000|+0.00%|+0.00%|+0.00%|
|backend_after.pool_rounded_mapping_bytes|reported scalar|8,388,608.000|8,388,608.000|8,388,608.000|+0.00%|+0.00%|+0.00%|
|backend_after.pool_tiny_pages|reported scalar|1,024.000|1,024.000|1,024.000|+0.00%|+0.00%|+0.00%|
|backend_after.private_admitted_capacity_hints.charged|reported scalar|283,312.000|283,312.000|283,312.000|+0.00%|+0.00%|+0.00%|
|backend_after.private_admitted_capacity_hints.frozen|reported scalar|65,536.000|65,536.000|65,536.000|+0.00%|+0.00%|+0.00%|
|backend_after.private_admitted_capacity_hints.index|reported scalar|4,096.000|4,096.000|4,096.000|+0.00%|+0.00%|+0.00%|
|backend_after.private_admitted_capacity_hints.packed|reported scalar|212,992.000|212,992.000|212,992.000|+0.00%|+0.00%|+0.00%|
|backend_after.private_current_global_budget_bytes|reported scalar|53,788,640.000|53,788,640.000|53,788,640.000|+0.00%|+0.00%|+0.00%|
|backend_after.private_current_global_reserved_bytes|reported scalar|283,312.000|283,312.000|283,312.000|+0.00%|+0.00%|+0.00%|
|backend_after.retained_frame_workspace_bytes|reported scalar|528,528.000|528,528.000|528,528.000|+0.00%|+0.00%|+0.00%|
|backend_after.retained_recipe_workspace_bytes|reported scalar|9,216.000|9,216.000|9,216.000|+0.00%|+0.00%|+0.00%|
|backend_after.sequence|reported scalar|513.000|513.000|513.000|+0.00%|+0.00%|+0.00%|
|backend_after.wal_capacity|reported scalar|67,108,864.000|67,108,864.000|67,108,864.000|+0.00%|+0.00%|+0.00%|
|backend_after.wal_selected_end|reported scalar|2,205,392.000|2,205,392.000|2,205,392.000|+0.00%|+0.00%|+0.00%|
|backend_before.auto_collect_below|reported scalar|256.000|256.000|256.000|+0.00%|+0.00%|+0.00%|
|backend_before.collect_cache.entries|reported scalar|[0, 0, 0, 0]|[0, 0, 0, 0]|[0, 0, 0, 0]|unavailable|unavailable|unavailable|
|backend_before.collect_cache.hits|reported scalar|[0, 0, 0, 0]|[0, 0, 0, 0]|[0, 0, 0, 0]|unavailable|unavailable|unavailable|
|backend_before.collect_cache.misses|reported scalar|[0, 0, 0, 0]|[0, 0, 0, 0]|[0, 0, 0, 0]|unavailable|unavailable|unavailable|
|backend_before.collect_cache.retained_bytes|reported scalar|648.000|648.000|648.000|+0.00%|+0.00%|+0.00%|
|backend_before.pool_kv_pages|reported scalar|1,024.000|1,024.000|1,024.000|+0.00%|+0.00%|+0.00%|
|backend_before.pool_metadata_bound_bytes|reported scalar|13,320,224.000|13,320,224.000|13,320,224.000|+0.00%|+0.00%|+0.00%|
|backend_before.pool_reused_pages|reported scalar|[0, 0, 0, 0]|[0, 0, 0, 0]|[0, 0, 0, 0]|unavailable|unavailable|unavailable|
|backend_before.pool_rounded_mapping_bytes|reported scalar|8,388,608.000|8,388,608.000|8,388,608.000|+0.00%|+0.00%|+0.00%|
|backend_before.pool_tiny_pages|reported scalar|1,024.000|1,024.000|1,024.000|+0.00%|+0.00%|+0.00%|
|backend_before.private_admitted_capacity_hints.charged|reported scalar|283,312.000|283,312.000|283,312.000|+0.00%|+0.00%|+0.00%|
|backend_before.private_admitted_capacity_hints.frozen|reported scalar|65,536.000|65,536.000|65,536.000|+0.00%|+0.00%|+0.00%|
|backend_before.private_admitted_capacity_hints.index|reported scalar|4,096.000|4,096.000|4,096.000|+0.00%|+0.00%|+0.00%|
|backend_before.private_admitted_capacity_hints.packed|reported scalar|212,992.000|212,992.000|212,992.000|+0.00%|+0.00%|+0.00%|
|backend_before.private_current_global_budget_bytes|reported scalar|53,788,640.000|53,788,640.000|53,788,640.000|+0.00%|+0.00%|+0.00%|
|backend_before.private_current_global_reserved_bytes|reported scalar|283,312.000|283,312.000|283,312.000|+0.00%|+0.00%|+0.00%|
|backend_before.retained_frame_workspace_bytes|reported scalar|528,528.000|528,528.000|528,528.000|+0.00%|+0.00%|+0.00%|
|backend_before.retained_recipe_workspace_bytes|reported scalar|7,168.000|7,168.000|7,168.000|+0.00%|+0.00%|+0.00%|
|backend_before.sequence|reported scalar|1.000|1.000|1.000|+0.00%|+0.00%|+0.00%|
|backend_before.wal_capacity|reported scalar|67,108,864.000|67,108,864.000|67,108,864.000|+0.00%|+0.00%|+0.00%|
|backend_before.wal_selected_end|reported scalar|725,136.000|725,136.000|725,136.000|+0.00%|+0.00%|+0.00%|
|before.lmdb.allocated_file_bytes|reported scalar|352,256.000|352,256.000|352,256.000|+0.00%|+0.00%|+0.00%|
|before.lmdb.apparent_file_bytes|reported scalar|356,352.000|356,352.000|356,352.000|+0.00%|+0.00%|+0.00%|
|before.lmdb.files|reported scalar|2.000|2.000|2.000|+0.00%|+0.00%|+0.00%|
|before.lmdb.mapped_pss_kib|reported scalar|12.000|12.000|12.000|+0.00%|+0.00%|+0.00%|
|before.lmdb.mapped_rss_kib|reported scalar|12.000|12.000|12.000|+0.00%|+0.00%|+0.00%|
|before.lmdb.mapped_virtual_kib|reported scalar|524,296.000|524,296.000|524,296.000|+0.00%|+0.00%|+0.00%|
|before.ulmdb.allocated_file_bytes|reported scalar|67,108,864.000|67,108,864.000|67,108,864.000|+0.00%|+0.00%|+0.00%|
|before.ulmdb.apparent_file_bytes|reported scalar|67,108,864.000|67,108,864.000|67,108,864.000|+0.00%|+0.00%|+0.00%|
|before.ulmdb.files|reported scalar|2.000|2.000|2.000|+0.00%|+0.00%|+0.00%|
|before.ulmdb.mapped_pss_kib|unavailable|[None, None, None, None]|[None, None, None, None]|[None, None, None, None]|unavailable|unavailable|unavailable|
|before.ulmdb.mapped_rss_kib|unavailable|[None, None, None, None]|[None, None, None, None]|[None, None, None, None]|unavailable|unavailable|unavailable|
|before.ulmdb.mapped_virtual_kib|unavailable|[None, None, None, None]|[None, None, None, None]|[None, None, None, None]|unavailable|unavailable|unavailable|
|before.whole_process_vm_rss|KiB whole process, both engines|13,333.957|13,380.978|13,390.994|+0.35%|+0.07%|+0.43%|
|cold.complete_cold_check_ns|reported scalar|52,930,189.307|52,262,696.107|53,952,591.235|-1.26%|+3.23%|+1.93%|
|cold.environment_drop_ns|reported scalar|90,898.086|90,359.699|94,315.958|-0.59%|+4.38%|+3.76%|
|cold.experimental_reenable_ns|reported scalar|2,173.898|2,007.588|2,167.612|-7.65%|+7.97%|-0.29%|
|cold.ip_queries_per_reader|reported scalar|8,000.000|8,000.000|8,000.000|+0.00%|+0.00%|+0.00%|
|cold.kv_rows|reported scalar|8,192.000|8,192.000|8,192.000|+0.00%|+0.00%|+0.00%|
|cold.open_ns|reported scalar|42,419,180.529|41,617,654.879|43,240,063.259|-1.89%|+3.90%|+1.94%|
|cold.validation_ns|reported scalar|6,160,763.403|6,238,566.994|6,292,502.402|+1.26%|+0.86%|+2.14%|
|creation.after_setup_ns|reported scalar|1,061.411|1,640.455|2,880.777|+54.55%|+75.61%|+171.41%|
|creation.dataset_ns|reported scalar|260,739,185.936|266,138,359.123|266,027,047.434|+2.07%|-0.04%|+2.03%|
|creation.fixture_load_both_ns|reported scalar|9,036,492.283|8,997,519.718|8,887,452.209|-0.43%|-1.22%|-1.65%|
|creation.lmdb_create_ns|reported scalar|121,479.323|169,540.342|133,030.871|+39.56%|-21.53%|+9.51%|
|creation.ulmdb_create_ns|reported scalar|47,107,970.143|48,358,991.022|47,472,619.255|+2.66%|-1.83%|+0.77%|
|read.ipv4_lpm_hits.lmdb.mean_cost|ns/operation|467.440|477.313|473.198|+2.11%|-0.86%|+1.23%|
|read.ipv4_lpm_hits.lmdb.mean_trial_p50|ns|555.070|551.030|549.160|-0.73%|-0.34%|-1.06%|
|read.ipv4_lpm_hits.lmdb.mean_trial_p95|ns|729.807|726.104|719.014|-0.51%|-0.98%|-1.48%|
|read.ipv4_lpm_hits.lmdb.mean_trial_p99|ns|786.895|783.972|779.114|-0.37%|-0.62%|-0.99%|
|read.ipv4_lpm_hits.lmdb.pooled_p50|ns|555.978|551.000|550.955|-0.90%|-0.01%|-0.90%|
|read.ipv4_lpm_hits.lmdb.pooled_p95|ns|728.985|728.487|718.919|-0.07%|-1.31%|-1.38%|
|read.ipv4_lpm_hits.lmdb.pooled_p99|ns|788.424|781.968|781.397|-0.82%|-0.07%|-0.89%|
|read.ipv4_lpm_hits.lmdb.rate|operations/s|2,139,311.410|2,095,062.924|2,113,279.963|-2.07%|+0.87%|-1.22%|
|read.ipv4_lpm_hits.lmdb.sample_max|ns|14,606.970|15,729.721|23,643.178|+7.69%|+50.31%|+61.86%|
|read.ipv4_lpm_hits.ulmdb.mean_cost|ns/operation|312.361|319.772|319.824|+2.37%|+0.02%|+2.39%|
|read.ipv4_lpm_hits.ulmdb.mean_trial_p50|ns|336.110|344.225|344.138|+2.41%|-0.03%|+2.39%|
|read.ipv4_lpm_hits.ulmdb.mean_trial_p95|ns|397.324|403.091|407.028|+1.45%|+0.98%|+2.44%|
|read.ipv4_lpm_hits.ulmdb.mean_trial_p99|ns|429.301|435.159|442.450|+1.36%|+1.68%|+3.06%|
|read.ipv4_lpm_hits.ulmdb.pooled_p50|ns|335.716|345.471|345.471|+2.91%|+0.00%|+2.91%|
|read.ipv4_lpm_hits.ulmdb.pooled_p95|ns|398.476|403.231|410.500|+1.19%|+1.80%|+3.02%|
|read.ipv4_lpm_hits.ulmdb.pooled_p99|ns|428.478|435.724|441.000|+1.69%|+1.21%|+2.92%|
|read.ipv4_lpm_hits.ulmdb.rate|operations/s|3,201,422.395|3,127,232.187|3,126,721.370|-2.32%|-0.02%|-2.33%|
|read.ipv4_lpm_hits.ulmdb.sample_max|ns|11,036.114|9,144.139|6,183.827|-17.14%|-32.37%|-43.97%|
|read.ipv4_lpm_inside_gap_misses.lmdb.mean_cost|ns/operation|496.725|514.075|496.099|+3.49%|-3.50%|-0.13%|
|read.ipv4_lpm_inside_gap_misses.lmdb.mean_trial_p50|ns|534.724|531.951|519.175|-0.52%|-2.40%|-2.91%|
|read.ipv4_lpm_inside_gap_misses.lmdb.mean_trial_p95|ns|602.455|596.809|583.082|-0.94%|-2.30%|-3.22%|
|read.ipv4_lpm_inside_gap_misses.lmdb.mean_trial_p99|ns|640.828|632.217|615.477|-1.34%|-2.65%|-3.96%|
|read.ipv4_lpm_inside_gap_misses.lmdb.pooled_p50|ns|535.931|533.204|518.384|-0.51%|-2.78%|-3.27%|
|read.ipv4_lpm_inside_gap_misses.lmdb.pooled_p95|ns|600.623|595.755|585.763|-0.81%|-1.68%|-2.47%|
|read.ipv4_lpm_inside_gap_misses.lmdb.pooled_p99|ns|659.969|643.005|631.866|-2.57%|-1.73%|-4.26%|
|read.ipv4_lpm_inside_gap_misses.lmdb.rate|operations/s|2,013,185.613|1,945,241.673|2,015,726.138|-3.37%|+3.62%|+0.13%|
|read.ipv4_lpm_inside_gap_misses.lmdb.sample_max|ns|6,701.357|12,015.691|6,225.687|+79.30%|-48.19%|-7.10%|
|read.ipv4_lpm_inside_gap_misses.ulmdb.mean_cost|ns/operation|85.880|85.793|86.166|-0.10%|+0.44%|+0.33%|
|read.ipv4_lpm_inside_gap_misses.ulmdb.mean_trial_p50|ns|110.047|110.481|110.171|+0.39%|-0.28%|+0.11%|
|read.ipv4_lpm_inside_gap_misses.ulmdb.mean_trial_p95|ns|121.256|121.488|121.521|+0.19%|+0.03%|+0.22%|
|read.ipv4_lpm_inside_gap_misses.ulmdb.mean_trial_p99|ns|122.492|121.890|122.489|-0.49%|+0.49%|-0.00%|
|read.ipv4_lpm_inside_gap_misses.ulmdb.pooled_p50|ns|110.000|110.000|110.000|+0.00%|+0.00%|+0.00%|
|read.ipv4_lpm_inside_gap_misses.ulmdb.pooled_p95|ns|122.680|125.493|121.000|+2.29%|-3.58%|-1.37%|
|read.ipv4_lpm_inside_gap_misses.ulmdb.pooled_p99|ns|127.992|125.717|127.765|-1.78%|+1.63%|-0.18%|
|read.ipv4_lpm_inside_gap_misses.ulmdb.rate|operations/s|11,644,208.251|11,656,027.726|11,605,470.907|+0.10%|-0.43%|-0.33%|
|read.ipv4_lpm_inside_gap_misses.ulmdb.sample_max|ns|5,918.958|2,684.674|7,421.524|-54.64%|+176.44%|+25.39%|
|read.ipv4_lpm_misses.lmdb.mean_cost|ns/operation|254.699|262.607|250.061|+3.10%|-4.78%|-1.82%|
|read.ipv4_lpm_misses.lmdb.mean_trial_p50|ns|68.767|69.951|70.680|+1.72%|+1.04%|+2.78%|
|read.ipv4_lpm_misses.lmdb.mean_trial_p95|ns|594.608|586.886|562.395|-1.30%|-4.17%|-5.42%|
|read.ipv4_lpm_misses.lmdb.mean_trial_p99|ns|638.194|626.612|603.568|-1.81%|-3.68%|-5.43%|
|read.ipv4_lpm_misses.lmdb.pooled_p50|ns|258.323|261.065|267.531|+1.06%|+2.48%|+3.56%|
|read.ipv4_lpm_misses.lmdb.pooled_p95|ns|593.231|587.969|563.439|-0.89%|-4.17%|-5.02%|
|read.ipv4_lpm_misses.lmdb.pooled_p99|ns|650.724|625.428|601.200|-3.89%|-3.87%|-7.61%|
|read.ipv4_lpm_misses.lmdb.rate|operations/s|3,926,200.088|3,807,976.467|3,999,024.208|-3.01%|+5.02%|+1.85%|
|read.ipv4_lpm_misses.lmdb.sample_max|ns|11,411.162|8,325.199|6,797.175|-27.04%|-18.35%|-40.43%|
|read.ipv4_lpm_misses.ulmdb.mean_cost|ns/operation|46.096|46.722|46.760|+1.36%|+0.08%|+1.44%|
|read.ipv4_lpm_misses.ulmdb.mean_trial_p50|ns|51.485|47.171|47.782|-8.38%|+1.30%|-7.19%|
|read.ipv4_lpm_misses.ulmdb.mean_trial_p95|ns|116.007|119.467|119.936|+2.98%|+0.39%|+3.39%|
|read.ipv4_lpm_misses.ulmdb.mean_trial_p99|ns|122.105|121.451|121.748|-0.54%|+0.24%|-0.29%|
|read.ipv4_lpm_misses.ulmdb.pooled_p50|ns|91.469|76.450|100.000|-16.42%|+30.80%|+9.33%|
|read.ipv4_lpm_misses.ulmdb.pooled_p95|ns|120.000|120.000|120.000|+0.00%|+0.00%|+0.00%|
|read.ipv4_lpm_misses.ulmdb.pooled_p99|ns|130.154|121.000|125.493|-7.03%|+3.71%|-3.58%|
|read.ipv4_lpm_misses.ulmdb.rate|operations/s|21,693,735.332|21,403,369.206|21,385,714.969|-1.34%|-0.08%|-1.42%|
|read.ipv4_lpm_misses.ulmdb.sample_max|ns|5,551.451|8,017.362|4,926.540|+44.42%|-38.55%|-11.26%|
|read.ipv4_lpm_mixed.lmdb.mean_cost|ns/operation|374.215|383.704|375.044|+2.54%|-2.26%|+0.22%|
|read.ipv4_lpm_mixed.lmdb.mean_trial_p50|ns|523.014|515.134|508.246|-1.51%|-1.34%|-2.82%|
|read.ipv4_lpm_mixed.lmdb.mean_trial_p95|ns|709.730|707.147|698.752|-0.36%|-1.19%|-1.55%|
|read.ipv4_lpm_mixed.lmdb.mean_trial_p99|ns|773.069|774.922|761.721|+0.24%|-1.70%|-1.47%|
|read.ipv4_lpm_mixed.lmdb.pooled_p50|ns|523.482|513.433|508.481|-1.92%|-0.96%|-2.87%|
|read.ipv4_lpm_mixed.lmdb.pooled_p95|ns|708.736|706.486|698.663|-0.32%|-1.11%|-1.42%|
|read.ipv4_lpm_mixed.lmdb.pooled_p99|ns|774.208|774.240|761.368|+0.00%|-1.66%|-1.66%|
|read.ipv4_lpm_mixed.lmdb.rate|operations/s|2,672,259.221|2,606,174.189|2,666,350.765|-2.47%|+2.31%|-0.22%|
|read.ipv4_lpm_mixed.lmdb.sample_max|ns|22,802.122|40,776.817|10,488.106|+78.83%|-74.28%|-54.00%|
|read.ipv4_lpm_mixed.ulmdb.mean_cost|ns/operation|180.868|185.117|184.835|+2.35%|-0.15%|+2.19%|
|read.ipv4_lpm_mixed.ulmdb.mean_trial_p50|ns|184.137|186.856|184.281|+1.48%|-1.38%|+0.08%|
|read.ipv4_lpm_mixed.ulmdb.mean_trial_p95|ns|386.903|394.943|394.603|+2.08%|-0.09%|+1.99%|
|read.ipv4_lpm_mixed.ulmdb.mean_trial_p99|ns|420.665|430.264|430.794|+2.28%|+0.12%|+2.41%|
|read.ipv4_lpm_mixed.ulmdb.pooled_p50|ns|262.465|267.465|267.721|+1.90%|+0.10%|+2.00%|
|read.ipv4_lpm_mixed.ulmdb.pooled_p95|ns|385.968|395.721|397.731|+2.53%|+0.51%|+3.05%|
|read.ipv4_lpm_mixed.ulmdb.pooled_p99|ns|423.226|435.218|430.750|+2.83%|-1.03%|+1.78%|
|read.ipv4_lpm_mixed.ulmdb.rate|operations/s|5,528,895.791|5,401,994.405|5,410,226.760|-2.30%|+0.15%|-2.15%|
|read.ipv4_lpm_mixed.ulmdb.sample_max|ns|14,755.747|7,756.781|31,703.530|-47.43%|+308.72%|+114.86%|
|read.ipv4_lpm_outside_cover_misses.lmdb.mean_cost|ns/operation|12.019|11.391|11.257|-5.23%|-1.18%|-6.34%|
|read.ipv4_lpm_outside_cover_misses.lmdb.mean_trial_p50|ns|40.311|40.000|40.000|-0.77%|+0.00%|-0.77%|
|read.ipv4_lpm_outside_cover_misses.lmdb.mean_trial_p95|ns|41.450|41.421|41.140|-0.07%|-0.68%|-0.75%|
|read.ipv4_lpm_outside_cover_misses.lmdb.mean_trial_p99|ns|41.739|43.544|45.448|+4.33%|+4.37%|+8.89%|
|read.ipv4_lpm_outside_cover_misses.lmdb.pooled_p50|ns|40.000|40.000|40.000|+0.00%|+0.00%|+0.00%|
|read.ipv4_lpm_outside_cover_misses.lmdb.pooled_p95|ns|43.085|41.000|41.000|-4.84%|+0.00%|-4.84%|
|read.ipv4_lpm_outside_cover_misses.lmdb.pooled_p99|ns|45.277|43.085|47.580|-4.84%|+10.43%|+5.09%|
|read.ipv4_lpm_outside_cover_misses.lmdb.rate|operations/s|83,198,541.047|87,786,803.773|88,831,998.386|+5.51%|+1.19%|+6.77%|
|read.ipv4_lpm_outside_cover_misses.lmdb.sample_max|ns|538.492|156.124|147.086|-71.01%|-5.79%|-72.69%|
|read.ipv4_lpm_outside_cover_misses.ulmdb.mean_cost|ns/operation|9.977|9.927|9.911|-0.51%|-0.16%|-0.66%|
|read.ipv4_lpm_outside_cover_misses.ulmdb.mean_trial_p50|ns|40.155|40.000|40.000|-0.39%|+0.00%|-0.39%|
|read.ipv4_lpm_outside_cover_misses.ulmdb.mean_trial_p95|ns|41.249|41.280|41.265|+0.08%|-0.04%|+0.04%|
|read.ipv4_lpm_outside_cover_misses.ulmdb.mean_trial_p99|ns|41.588|41.280|41.437|-0.74%|+0.38%|-0.36%|
|read.ipv4_lpm_outside_cover_misses.ulmdb.pooled_p50|ns|40.000|40.000|40.000|+0.00%|+0.00%|+0.00%|
|read.ipv4_lpm_outside_cover_misses.ulmdb.pooled_p95|ns|43.085|41.000|41.000|-4.84%|+0.00%|-4.84%|
|read.ipv4_lpm_outside_cover_misses.ulmdb.pooled_p99|ns|45.277|41.000|43.085|-9.45%|+5.09%|-4.84%|
|read.ipv4_lpm_outside_cover_misses.ulmdb.rate|operations/s|100,226,674.272|100,737,853.493|100,895,958.713|+0.51%|+0.16%|+0.67%|
|read.ipv4_lpm_outside_cover_misses.ulmdb.sample_max|ns|169.812|1,336.715|1,861.126|+687.17%|+39.23%|+995.99%|
|read.ipv6_lpm_hits.lmdb.mean_cost|ns/operation|804.734|802.276|792.562|-0.31%|-1.21%|-1.51%|
|read.ipv6_lpm_hits.lmdb.mean_trial_p50|ns|977.688|975.744|958.352|-0.20%|-1.78%|-1.98%|
|read.ipv6_lpm_hits.lmdb.mean_trial_p95|ns|1,183.234|1,175.273|1,163.441|-0.67%|-1.01%|-1.67%|
|read.ipv6_lpm_hits.lmdb.mean_trial_p99|ns|1,258.724|1,243.071|1,231.035|-1.24%|-0.97%|-2.20%|
|read.ipv6_lpm_hits.lmdb.pooled_p50|ns|976.961|974.465|959.215|-0.26%|-1.56%|-1.82%|
|read.ipv6_lpm_hits.lmdb.pooled_p95|ns|1,179.783|1,174.696|1,159.722|-0.43%|-1.27%|-1.70%|
|read.ipv6_lpm_hits.lmdb.pooled_p99|ns|1,259.784|1,237.408|1,234.604|-1.78%|-0.23%|-2.00%|
|read.ipv6_lpm_hits.lmdb.rate|operations/s|1,242,646.231|1,246,454.176|1,261,731.205|+0.31%|+1.23%|+1.54%|
|read.ipv6_lpm_hits.lmdb.sample_max|ns|20,808.162|18,770.909|30,240.886|-9.79%|+61.11%|+45.33%|
|read.ipv6_lpm_hits.ulmdb.mean_cost|ns/operation|405.218|397.403|399.707|-1.93%|+0.58%|-1.36%|
|read.ipv6_lpm_hits.ulmdb.mean_trial_p50|ns|430.765|426.199|427.196|-1.06%|+0.23%|-0.83%|
|read.ipv6_lpm_hits.ulmdb.mean_trial_p95|ns|498.396|494.273|494.681|-0.83%|+0.08%|-0.75%|
|read.ipv6_lpm_hits.ulmdb.mean_trial_p99|ns|531.271|525.634|524.432|-1.06%|-0.23%|-1.29%|
|read.ipv6_lpm_hits.ulmdb.pooled_p50|ns|430.750|425.723|427.732|-1.17%|+0.47%|-0.70%|
|read.ipv6_lpm_hits.ulmdb.pooled_p95|ns|498.481|493.481|498.232|-1.00%|+0.96%|-0.05%|
|read.ipv6_lpm_hits.ulmdb.pooled_p99|ns|535.883|528.435|528.482|-1.39%|+0.01%|-1.38%|
|read.ipv6_lpm_hits.ulmdb.rate|operations/s|2,467,810.156|2,516,337.150|2,501,829.782|+1.97%|-0.58%|+1.38%|
|read.ipv6_lpm_hits.ulmdb.sample_max|ns|13,951.818|6,299.093|14,970.993|-54.85%|+137.67%|+7.30%|
|read.ipv6_lpm_inside_gap_misses.lmdb.mean_cost|ns/operation|889.001|881.062|867.752|-0.89%|-1.51%|-2.39%|
|read.ipv6_lpm_inside_gap_misses.lmdb.mean_trial_p50|ns|943.739|931.790|917.133|-1.27%|-1.57%|-2.82%|
|read.ipv6_lpm_inside_gap_misses.lmdb.mean_trial_p95|ns|1,056.476|1,047.200|1,030.214|-0.88%|-1.62%|-2.49%|
|read.ipv6_lpm_inside_gap_misses.lmdb.mean_trial_p99|ns|1,107.068|1,104.347|1,078.439|-0.25%|-2.35%|-2.59%|
|read.ipv6_lpm_inside_gap_misses.lmdb.pooled_p50|ns|944.136|931.663|916.683|-1.32%|-1.61%|-2.91%|
|read.ipv6_lpm_inside_gap_misses.lmdb.pooled_p95|ns|1,056.941|1,046.585|1,026.867|-0.98%|-1.88%|-2.85%|
|read.ipv6_lpm_inside_gap_misses.lmdb.pooled_p99|ns|1,102.179|1,121.709|1,088.720|+1.77%|-2.94%|-1.22%|
|read.ipv6_lpm_inside_gap_misses.lmdb.rate|operations/s|1,124,858.679|1,134,993.478|1,152,402.780|+0.90%|+1.53%|+2.45%|
|read.ipv6_lpm_inside_gap_misses.lmdb.sample_max|ns|11,643.072|12,200.111|13,047.149|+4.78%|+6.94%|+12.06%|
|read.ipv6_lpm_inside_gap_misses.ulmdb.mean_cost|ns/operation|106.135|108.006|108.491|+1.76%|+0.45%|+2.22%|
|read.ipv6_lpm_inside_gap_misses.ulmdb.mean_trial_p50|ns|140.000|130.638|130.203|-6.69%|-0.33%|-7.00%|
|read.ipv6_lpm_inside_gap_misses.ulmdb.mean_trial_p95|ns|140.984|142.576|140.997|+1.13%|-1.11%|+0.01%|
|read.ipv6_lpm_inside_gap_misses.ulmdb.mean_trial_p99|ns|141.296|143.749|141.621|+1.74%|-1.48%|+0.23%|
|read.ipv6_lpm_inside_gap_misses.ulmdb.pooled_p50|ns|140.000|130.249|130.000|-6.96%|-0.19%|-7.14%|
|read.ipv6_lpm_inside_gap_misses.ulmdb.pooled_p95|ns|141.000|145.754|140.749|+3.37%|-3.43%|-0.18%|
|read.ipv6_lpm_inside_gap_misses.ulmdb.pooled_p99|ns|141.000|152.717|143.436|+8.31%|-6.08%|+1.73%|
|read.ipv6_lpm_inside_gap_misses.ulmdb.rate|operations/s|9,421,952.974|9,258,727.376|9,217,336.374|-1.73%|-0.45%|-2.17%|
|read.ipv6_lpm_inside_gap_misses.ulmdb.sample_max|ns|7,585.578|5,653.961|7,160.459|-25.46%|+26.65%|-5.60%|
|read.ipv6_lpm_misses.lmdb.mean_cost|ns/operation|449.696|448.521|439.639|-0.26%|-1.98%|-2.24%|
|read.ipv6_lpm_misses.lmdb.mean_trial_p50|ns|69.945|47.971|74.076|-31.42%|+54.42%|+5.91%|
|read.ipv6_lpm_misses.lmdb.mean_trial_p95|ns|1,036.778|1,021.488|1,003.198|-1.47%|-1.79%|-3.24%|
|read.ipv6_lpm_misses.lmdb.mean_trial_p99|ns|1,089.762|1,080.107|1,051.250|-0.89%|-2.67%|-3.53%|
|read.ipv6_lpm_misses.lmdb.pooled_p50|ns|224.648|61.997|346.201|-72.40%|+458.41%|+54.11%|
|read.ipv6_lpm_misses.lmdb.pooled_p95|ns|1,034.419|1,014.392|1,004.163|-1.94%|-1.01%|-2.92%|
|read.ipv6_lpm_misses.lmdb.pooled_p99|ns|1,089.400|1,093.365|1,051.881|+0.36%|-3.79%|-3.44%|
|read.ipv6_lpm_misses.lmdb.rate|operations/s|2,223,725.468|2,229,551.071|2,274,592.362|+0.26%|+2.02%|+2.29%|
|read.ipv6_lpm_misses.lmdb.sample_max|ns|9,511.365|31,121.417|18,107.848|+227.20%|-41.82%|+90.38%|
|read.ipv6_lpm_misses.ulmdb.mean_cost|ns/operation|57.994|59.109|58.640|+1.92%|-0.79%|+1.11%|
|read.ipv6_lpm_misses.ulmdb.mean_trial_p50|ns|46.684|48.404|47.166|+3.68%|-2.56%|+1.03%|
|read.ipv6_lpm_misses.ulmdb.mean_trial_p95|ns|141.000|141.584|140.109|+0.41%|-1.04%|-0.63%|
|read.ipv6_lpm_misses.ulmdb.mean_trial_p99|ns|141.466|143.443|141.451|+1.40%|-1.39%|-0.01%|
|read.ipv6_lpm_misses.ulmdb.pooled_p50|ns|80.709|78.223|78.223|-3.08%|+0.00%|-3.08%|
|read.ipv6_lpm_misses.ulmdb.pooled_p95|ns|141.000|140.249|140.000|-0.53%|-0.18%|-0.71%|
|read.ipv6_lpm_misses.ulmdb.pooled_p99|ns|141.000|152.494|141.000|+8.15%|-7.54%|+0.00%|
|read.ipv6_lpm_misses.ulmdb.rate|operations/s|17,243,028.825|16,917,913.944|17,053,163.533|-1.89%|+0.80%|-1.10%|
|read.ipv6_lpm_misses.ulmdb.sample_max|ns|5,255.369|7,767.763|4,542.454|+47.81%|-41.52%|-13.57%|
|read.ipv6_lpm_mixed.lmdb.mean_cost|ns/operation|642.332|636.521|629.879|-0.90%|-1.04%|-1.94%|
|read.ipv6_lpm_mixed.lmdb.mean_trial_p50|ns|874.975|870.125|859.586|-0.55%|-1.21%|-1.76%|
|read.ipv6_lpm_mixed.lmdb.mean_trial_p95|ns|1,172.826|1,161.202|1,145.461|-0.99%|-1.36%|-2.33%|
|read.ipv6_lpm_mixed.lmdb.mean_trial_p99|ns|1,254.265|1,223.826|1,222.090|-2.43%|-0.14%|-2.57%|
|read.ipv6_lpm_mixed.lmdb.pooled_p50|ns|876.454|868.988|861.250|-0.85%|-0.89%|-1.73%|
|read.ipv6_lpm_mixed.lmdb.pooled_p95|ns|1,170.018|1,162.188|1,146.967|-0.67%|-1.31%|-1.97%|
|read.ipv6_lpm_mixed.lmdb.pooled_p99|ns|1,261.474|1,222.102|1,217.202|-3.12%|-0.40%|-3.51%|
|read.ipv6_lpm_mixed.lmdb.rate|operations/s|1,556,828.086|1,571,040.803|1,587,607.153|+0.91%|+1.05%|+1.98%|
|read.ipv6_lpm_mixed.lmdb.sample_max|ns|25,992.316|17,044.870|25,106.338|-34.42%|+47.30%|-3.41%|
|read.ipv6_lpm_mixed.ulmdb.mean_cost|ns/operation|232.613|230.124|229.992|-1.07%|-0.06%|-1.13%|
|read.ipv6_lpm_mixed.ulmdb.mean_trial_p50|ns|218.747|210.405|219.031|-3.81%|+4.10%|+0.13%|
|read.ipv6_lpm_mixed.ulmdb.mean_trial_p95|ns|481.759|475.038|476.261|-1.40%|+0.26%|-1.14%|
|read.ipv6_lpm_mixed.ulmdb.mean_trial_p99|ns|521.390|516.759|518.453|-0.89%|+0.33%|-0.56%|
|read.ipv6_lpm_mixed.ulmdb.pooled_p50|ns|290.786|307.469|290.786|+5.74%|-5.43%|+0.00%|
|read.ipv6_lpm_mixed.ulmdb.pooled_p95|ns|483.481|473.480|475.974|-2.07%|+0.53%|-1.55%|
|read.ipv6_lpm_mixed.ulmdb.pooled_p99|ns|521.098|515.976|518.434|-0.98%|+0.48%|-0.51%|
|read.ipv6_lpm_mixed.ulmdb.rate|operations/s|4,298,979.331|4,345,474.116|4,347,976.146|+1.08%|+0.06%|+1.14%|
|read.ipv6_lpm_mixed.ulmdb.sample_max|ns|13,450.639|19,188.815|14,219.747|+42.66%|-25.90%|+5.72%|
|read.ipv6_lpm_outside_cover_misses.lmdb.mean_cost|ns/operation|11.827|11.413|11.023|-3.49%|-3.42%|-6.80%|
|read.ipv6_lpm_outside_cover_misses.lmdb.mean_trial_p50|ns|40.000|40.000|40.000|+0.00%|+0.00%|+0.00%|
|read.ipv6_lpm_outside_cover_misses.lmdb.mean_trial_p95|ns|41.140|41.828|41.668|+1.67%|-0.38%|+1.28%|
|read.ipv6_lpm_outside_cover_misses.lmdb.mean_trial_p99|ns|41.155|43.289|45.925|+5.18%|+6.09%|+11.59%|
|read.ipv6_lpm_outside_cover_misses.lmdb.pooled_p50|ns|40.000|40.000|40.000|+0.00%|+0.00%|+0.00%|
|read.ipv6_lpm_outside_cover_misses.lmdb.pooled_p95|ns|41.000|43.085|41.000|+5.09%|-4.84%|+0.00%|
|read.ipv6_lpm_outside_cover_misses.lmdb.pooled_p99|ns|43.085|43.085|47.580|+0.00%|+10.43%|+10.43%|
|read.ipv6_lpm_outside_cover_misses.lmdb.rate|operations/s|84,554,536.746|87,616,617.916|90,722,089.820|+3.62%|+3.54%|+7.29%|
|read.ipv6_lpm_outside_cover_misses.lmdb.sample_max|ns|1,912.554|463.783|69.056|-75.75%|-85.11%|-96.39%|
|read.ipv6_lpm_outside_cover_misses.ulmdb.mean_cost|ns/operation|10.551|10.489|10.247|-0.59%|-2.31%|-2.89%|
|read.ipv6_lpm_outside_cover_misses.ulmdb.mean_trial_p50|ns|40.000|40.155|40.000|+0.39%|-0.39%|+0.00%|
|read.ipv6_lpm_outside_cover_misses.ulmdb.mean_trial_p95|ns|40.952|41.108|41.234|+0.38%|+0.31%|+0.69%|
|read.ipv6_lpm_outside_cover_misses.ulmdb.mean_trial_p99|ns|41.155|41.296|41.296|+0.34%|+0.00%|+0.34%|
|read.ipv6_lpm_outside_cover_misses.ulmdb.pooled_p50|ns|40.000|40.000|40.000|+0.00%|+0.00%|+0.00%|
|read.ipv6_lpm_outside_cover_misses.ulmdb.pooled_p95|ns|41.000|43.085|41.000|+5.09%|-4.84%|+0.00%|
|read.ipv6_lpm_outside_cover_misses.ulmdb.pooled_p99|ns|41.000|43.085|41.000|+5.09%|-4.84%|+0.00%|
|read.ipv6_lpm_outside_cover_misses.ulmdb.rate|operations/s|94,774,495.786|95,338,535.181|97,594,007.874|+0.60%|+2.37%|+2.97%|
|read.ipv6_lpm_outside_cover_misses.ulmdb.sample_max|ns|152.640|923.764|1,388.564|+505.19%|+50.32%|+809.70%|
|read.ordered_kv_hits.lmdb.mean_cost|ns/operation|240.485|238.883|238.387|-0.67%|-0.21%|-0.87%|
|read.ordered_kv_hits.lmdb.mean_trial_p50|ns|271.000|268.339|269.875|-0.98%|+0.57%|-0.42%|
|read.ordered_kv_hits.lmdb.mean_trial_p95|ns|320.109|312.976|313.011|-2.23%|+0.01%|-2.22%|
|read.ordered_kv_hits.lmdb.mean_trial_p99|ns|338.199|333.766|333.523|-1.31%|-0.07%|-1.38%|
|read.ordered_kv_hits.lmdb.pooled_p50|ns|271.000|270.000|270.000|-0.37%|+0.00%|-0.37%|
|read.ordered_kv_hits.lmdb.pooled_p95|ns|320.000|313.226|313.226|-2.12%|+0.00%|-2.12%|
|read.ordered_kv_hits.lmdb.pooled_p99|ns|340.250|335.716|335.470|-1.33%|-0.07%|-1.40%|
|read.ordered_kv_hits.lmdb.rate|operations/s|4,158,266.076|4,186,151.656|4,194,864.552|+0.67%|+0.21%|+0.88%|
|read.ordered_kv_hits.lmdb.sample_max|ns|11,770.031|12,463.523|13,581.990|+5.89%|+8.97%|+15.39%|
|read.ordered_kv_hits.ulmdb.mean_cost|ns/operation|130.259|139.015|139.153|+6.72%|+0.10%|+6.83%|
|read.ordered_kv_hits.ulmdb.mean_trial_p50|ns|155.338|157.701|160.000|+1.52%|+1.46%|+3.00%|
|read.ordered_kv_hits.ulmdb.mean_trial_p95|ns|184.693|187.786|190.234|+1.67%|+1.30%|+3.00%|
|read.ordered_kv_hits.ulmdb.mean_trial_p99|ns|197.312|201.855|202.368|+2.30%|+0.25%|+2.56%|
|read.ordered_kv_hits.ulmdb.pooled_p50|ns|155.435|157.701|160.000|+1.46%|+1.46%|+2.94%|
|read.ordered_kv_hits.ulmdb.pooled_p95|ns|184.932|187.709|190.000|+1.50%|+1.22%|+2.74%|
|read.ordered_kv_hits.ulmdb.pooled_p99|ns|198.094|200.750|203.213|+1.34%|+1.23%|+2.58%|
|read.ordered_kv_hits.ulmdb.rate|operations/s|7,677,030.677|7,193,492.064|7,186,356.014|-6.30%|-0.10%|-6.39%|
|read.ordered_kv_hits.ulmdb.sample_max|ns|18,292.914|7,876.298|10,463.786|-56.94%|+32.85%|-42.80%|
|read.ordered_kv_misses.lmdb.mean_cost|ns/operation|249.274|248.037|249.259|-0.50%|+0.49%|-0.01%|
|read.ordered_kv_misses.lmdb.mean_trial_p50|ns|280.140|273.106|276.672|-2.51%|+1.31%|-1.24%|
|read.ordered_kv_misses.lmdb.mean_trial_p95|ns|322.449|320.968|324.135|-0.46%|+0.99%|+0.52%|
|read.ordered_kv_misses.lmdb.mean_trial_p99|ns|341.714|339.410|344.732|-0.67%|+1.57%|+0.88%|
|read.ordered_kv_misses.lmdb.pooled_p50|ns|280.250|271.000|277.722|-3.30%|+2.48%|-0.90%|
|read.ordered_kv_misses.lmdb.pooled_p95|ns|321.000|320.750|325.648|-0.08%|+1.53%|+1.45%|
|read.ordered_kv_misses.lmdb.pooled_p99|ns|345.456|340.500|350.331|-1.43%|+2.89%|+1.41%|
|read.ordered_kv_misses.lmdb.rate|operations/s|4,011,657.091|4,031,652.721|4,011,888.928|+0.50%|-0.49%|+0.01%|
|read.ordered_kv_misses.lmdb.sample_max|ns|8,866.493|13,079.176|6,608.358|+47.51%|-49.47%|-25.47%|
|read.ordered_kv_misses.ulmdb.mean_cost|ns/operation|130.484|133.474|135.806|+2.29%|+1.75%|+4.08%|
|read.ordered_kv_misses.ulmdb.mean_trial_p50|ns|154.968|157.668|160.482|+1.74%|+1.78%|+3.56%|
|read.ordered_kv_misses.ulmdb.mean_trial_p95|ns|183.627|187.878|191.621|+2.32%|+1.99%|+4.35%|
|read.ordered_kv_misses.ulmdb.mean_trial_p99|ns|195.731|200.322|203.474|+2.35%|+1.57%|+3.96%|
|read.ordered_kv_misses.ulmdb.pooled_p50|ns|154.919|157.701|160.000|+1.80%|+1.46%|+3.28%|
|read.ordered_kv_misses.ulmdb.pooled_p95|ns|184.932|187.709|192.452|+1.50%|+2.53%|+4.07%|
|read.ordered_kv_misses.ulmdb.pooled_p99|ns|195.936|200.499|205.824|+2.33%|+2.66%|+5.05%|
|read.ordered_kv_misses.ulmdb.rate|operations/s|7,663,747.646|7,492,116.347|7,363,445.079|-2.24%|-1.72%|-3.92%|
|read.ordered_kv_misses.ulmdb.sample_max|ns|10,031.856|12,253.571|14,307.159|+22.15%|+16.76%|+42.62%|
|read.ordered_kv_mixed.lmdb.mean_cost|ns/operation|243.701|243.141|243.669|-0.23%|+0.22%|-0.01%|
|read.ordered_kv_mixed.lmdb.mean_trial_p50|ns|271.141|271.015|271.078|-0.05%|+0.02%|-0.02%|
|read.ordered_kv_mixed.lmdb.mean_trial_p95|ns|320.469|320.034|320.733|-0.14%|+0.22%|+0.08%|
|read.ordered_kv_mixed.lmdb.mean_trial_p99|ns|338.618|338.443|339.809|-0.05%|+0.40%|+0.35%|
|read.ordered_kv_mixed.lmdb.pooled_p50|ns|271.000|270.750|271.000|-0.09%|+0.09%|+0.00%|
|read.ordered_kv_mixed.lmdb.pooled_p95|ns|320.750|320.500|320.250|-0.08%|-0.08%|-0.16%|
|read.ordered_kv_mixed.lmdb.pooled_p99|ns|340.500|340.684|342.976|+0.05%|+0.67%|+0.73%|
|read.ordered_kv_mixed.lmdb.rate|operations/s|4,103,388.920|4,112,841.429|4,103,935.037|+0.23%|-0.22%|+0.01%|
|read.ordered_kv_mixed.lmdb.sample_max|ns|7,105.512|21,299.020|16,529.460|+199.75%|-22.39%|+132.63%|
|read.ordered_kv_mixed.ulmdb.mean_cost|ns/operation|135.997|143.536|144.627|+5.54%|+0.76%|+6.35%|
|read.ordered_kv_mixed.ulmdb.mean_trial_p50|ns|160.281|160.890|160.891|+0.38%|+0.00%|+0.38%|
|read.ordered_kv_mixed.ulmdb.mean_trial_p95|ns|186.882|191.621|191.125|+2.54%|-0.26%|+2.27%|
|read.ordered_kv_mixed.ulmdb.mean_trial_p99|ns|202.790|204.535|204.963|+0.86%|+0.21%|+1.07%|
|read.ordered_kv_mixed.ulmdb.pooled_p50|ns|160.249|160.749|161.000|+0.31%|+0.16%|+0.47%|
|read.ordered_kv_mixed.ulmdb.pooled_p95|ns|185.689|190.750|191.000|+2.73%|+0.13%|+2.86%|
|read.ordered_kv_mixed.ulmdb.pooled_p99|ns|202.707|205.451|207.713|+1.35%|+1.10%|+2.47%|
|read.ordered_kv_mixed.ulmdb.rate|operations/s|7,353,093.056|6,966,886.782|6,914,355.182|-5.25%|-0.75%|-5.97%|
|read.ordered_kv_mixed.ulmdb.sample_max|ns|8,450.271|8,050.201|9,576.888|-4.73%|+18.96%|+13.33%|
|write.batch1.lmdb.aggregate_cost|ns/commit|7,433.439|8,081.066|8,295.765|+8.71%|+2.66%|+11.60%|
|write.batch1.lmdb.aggregate_rate|records/s|134,527.236|123,746.049|120,543.437|-8.01%|-2.59%|-10.39%|
|write.batch1.lmdb.combined_cost|ns/commit|7,201.813|7,908.365|8,089.773|+9.81%|+2.29%|+12.33%|
|write.batch1.lmdb.combined_rate|records/s|138,853.922|126,448.384|123,612.862|-8.93%|-2.24%|-10.98%|
|write.batch1.lmdb.mean_trial_p99|ns|7,945.212|10,527.948|10,510.298|+32.51%|-0.17%|+32.28%|
|write.batch1.lmdb.pooled_p50|ns|6,782.996|6,977.970|7,190.989|+2.87%|+3.05%|+6.01%|
|write.batch1.lmdb.pooled_p95|ns|7,905.019|10,214.433|10,163.524|+29.21%|-0.50%|+28.57%|
|write.batch1.lmdb.pooled_p99|ns|9,388.422|13,267.268|12,700.727|+41.32%|-4.27%|+35.28%|
|write.batch1.lmdb.sample_max|ns|10,312.016|15,072.737|13,556.155|+46.17%|-10.06%|+31.46%|
|write.batch1.lmdb.sampled_mean_cost|ns/commit|6,969.193|7,735.221|7,883.489|+10.99%|+1.92%|+13.12%|
|write.batch1.ulmdb.aggregate_cost|ns/commit|16,126.745|16,746.414|16,864.501|+3.84%|+0.71%|+4.57%|
|write.batch1.ulmdb.aggregate_rate|records/s|62,008.793|59,714.278|59,296.151|-3.70%|-0.70%|-4.37%|
|write.batch1.ulmdb.combined_cost|ns/commit|13,359.519|14,113.289|14,193.810|+5.64%|+0.57%|+6.24%|
|write.batch1.ulmdb.combined_rate|records/s|74,852.996|70,855.204|70,453.247|-5.34%|-0.57%|-5.88%|
|write.batch1.ulmdb.mean_trial_p99|ns|11,944.138|14,768.968|14,003.264|+23.65%|-5.18%|+17.24%|
|write.batch1.ulmdb.pooled_p50|ns|10,402.355|10,855.043|11,018.397|+4.35%|+1.50%|+5.92%|
|write.batch1.ulmdb.pooled_p95|ns|11,869.613|13,908.211|13,835.990|+17.17%|-0.52%|+16.57%|
|write.batch1.ulmdb.pooled_p99|ns|15,994.185|17,175.898|16,224.027|+7.39%|-5.54%|+1.44%|
|write.batch1.ulmdb.sample_max|ns|17,120.768|24,728.774|17,694.008|+44.44%|-28.45%|+3.35%|
|write.batch1.ulmdb.sampled_mean_cost|ns/commit|10,589.204|11,479.528|11,523.080|+8.41%|+0.38%|+8.82%|
|write.batch64.lmdb.aggregate_cost|ns/commit|85,184.888|86,427.186|88,073.603|+1.46%|+1.90%|+3.39%|
|write.batch64.lmdb.aggregate_rate|records/s|751,306.965|740,507.736|726,664.948|-1.44%|-1.87%|-3.28%|
|write.batch64.lmdb.combined_cost|ns/commit|84,269.520|85,478.321|86,988.719|+1.43%|+1.77%|+3.23%|
|write.batch64.lmdb.combined_rate|records/s|759,467.953|748,727.852|735,727.580|-1.41%|-1.74%|-3.13%|
|write.batch64.lmdb.mean_trial_p99|ns|90,946.860|91,801.801|93,547.770|+0.94%|+1.90%|+2.86%|
|write.batch64.lmdb.pooled_p50|ns|82,859.310|84,202.078|85,593.450|+1.62%|+1.65%|+3.30%|
|write.batch64.lmdb.pooled_p95|ns|91,326.658|92,732.559|94,646.260|+1.54%|+2.06%|+3.63%|
|write.batch64.lmdb.pooled_p99|ns|97,139.433|97,465.402|97,696.686|+0.34%|+0.24%|+0.57%|
|write.batch64.lmdb.sample_max|ns|103,283.347|102,433.480|101,098.133|-0.82%|-1.30%|-2.12%|
|write.batch64.lmdb.sampled_mean_cost|ns/commit|83,353.526|84,529.273|85,898.367|+1.41%|+1.62%|+3.05%|
|write.batch64.ulmdb.aggregate_cost|ns/commit|65,341.987|66,677.725|67,033.546|+2.04%|+0.53%|+2.59%|
|write.batch64.ulmdb.aggregate_rate|records/s|979,462.109|959,840.788|954,745.851|-2.00%|-0.53%|-2.52%|
|write.batch64.ulmdb.combined_cost|ns/commit|64,692.515|66,107.403|66,407.984|+2.19%|+0.45%|+2.65%|
|write.batch64.ulmdb.combined_rate|records/s|989,295.289|968,121.527|963,739.546|-2.14%|-0.45%|-2.58%|
|write.batch64.ulmdb.mean_trial_p99|ns|130,656.816|131,405.684|131,494.178|+0.57%|+0.07%|+0.64%|
|write.batch64.ulmdb.pooled_p50|ns|54,452.057|56,035.577|55,994.128|+2.91%|-0.07%|+2.83%|
|write.batch64.ulmdb.pooled_p95|ns|66,419.701|71,593.634|84,733.928|+7.79%|+18.35%|+27.57%|
|write.batch64.ulmdb.pooled_p99|ns|254,879.377|251,075.480|253,844.468|-1.49%|+1.10%|-0.41%|
|write.batch64.ulmdb.sample_max|ns|268,428.679|260,605.215|262,715.595|-2.91%|+0.81%|-2.13%|
|write.batch64.ulmdb.sampled_mean_cost|ns/commit|64,042.460|65,537.008|65,781.252|+2.33%|+0.37%|+2.72%|


### All120 instrumented phase strata

Means are µs; counts include every row in the named process/backend/batch/phase/order/Resident-maintenance context. Native contexts align to Resident and do not imply Native Collect. Complete distribution statistics, including all maxima, remain in analysis.json.

|Round|Backend|Batch|Phase|First backend|Resident context|Rows|Business mean|Begin mean|Mutation mean|Commit mean|Collect mean|Business p99|Business max|
|---:|---|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
|1|lmdb|1|aggregate|lmdb|first_cold_collect|1|7.083|0.100|0.922|5.911|unavailable|7.083|7.083|
|1|lmdb|1|aggregate|lmdb|ordinary_no_collect_post_first|2|6.989|0.105|0.857|5.881|unavailable|7.064|7.064|
|1|lmdb|1|aggregate|lmdb|ordinary_no_collect_pre_first|61|7.189|0.111|0.902|6.030|unavailable|8.226|8.226|
|1|lmdb|1|aggregate|ulmdb|ordinary_no_collect_pre_first|64|7.855|0.121|1.146|6.442|unavailable|20.649|20.649|
|1|lmdb|1|latency|lmdb|ordinary_no_collect_post_first|8|7.275|0.121|0.943|6.070|unavailable|8.617|8.617|
|1|lmdb|1|latency|lmdb|ordinary_no_collect_pre_first|56|7.104|0.111|0.883|5.967|unavailable|7.925|7.925|
|1|lmdb|1|latency|ulmdb|ordinary_no_collect_pre_first|64|7.235|0.110|0.883|6.095|unavailable|11.983|11.983|
|1|lmdb|64|aggregate|lmdb|later_cached_collect|2|88.253|0.150|34.530|53.427|unavailable|92.105|92.105|
|1|lmdb|64|aggregate|lmdb|ordinary_no_collect_post_first|62|83.598|0.141|34.336|48.970|unavailable|95.241|95.241|
|1|lmdb|64|aggregate|ulmdb|later_cached_collect|2|81.535|0.145|32.331|48.908|unavailable|84.060|84.060|
|1|lmdb|64|aggregate|ulmdb|ordinary_no_collect_post_first|62|86.943|0.153|35.332|51.307|unavailable|127.833|127.833|
|1|lmdb|64|latency|lmdb|later_cached_collect|4|84.599|0.148|35.700|48.590|unavailable|87.938|87.938|
|1|lmdb|64|latency|lmdb|ordinary_no_collect_post_first|60|83.743|0.150|34.666|48.773|unavailable|96.443|96.443|
|1|lmdb|64|latency|ulmdb|later_cached_collect|2|85.658|0.125|34.791|50.590|unavailable|89.080|89.080|
|1|lmdb|64|latency|ulmdb|ordinary_no_collect_post_first|62|84.126|0.153|34.870|48.948|unavailable|96.053|96.053|
|1|ulmdb|1|aggregate|lmdb|first_cold_collect|1|633.414|621.832|0.961|10.450|621.572|633.414|633.414|
|1|ulmdb|1|aggregate|lmdb|ordinary_no_collect_post_first|2|10.069|0.155|0.857|8.912|unavailable|10.210|10.210|
|1|ulmdb|1|aggregate|lmdb|ordinary_no_collect_pre_first|61|11.192|0.153|1.387|9.498|unavailable|13.997|13.997|
|1|ulmdb|1|aggregate|ulmdb|ordinary_no_collect_pre_first|64|11.301|0.154|1.433|9.555|unavailable|24.626|24.626|
|1|ulmdb|1|latency|lmdb|ordinary_no_collect_post_first|8|10.015|0.150|0.795|8.917|unavailable|11.291|11.291|
|1|ulmdb|1|latency|lmdb|ordinary_no_collect_pre_first|56|10.922|0.146|1.428|9.196|unavailable|16.612|16.612|
|1|ulmdb|1|latency|ulmdb|ordinary_no_collect_pre_first|64|10.891|0.147|1.358|9.231|unavailable|13.034|13.034|
|1|ulmdb|64|aggregate|lmdb|later_cached_collect|2|248.918|187.452|26.004|35.273|187.211|251.488|251.488|
|1|ulmdb|64|aggregate|lmdb|ordinary_no_collect_post_first|62|60.337|0.170|25.676|34.323|unavailable|69.031|69.031|
|1|ulmdb|64|aggregate|ulmdb|later_cached_collect|2|246.609|186.595|25.944|33.880|186.359|248.973|248.973|
|1|ulmdb|64|aggregate|ulmdb|ordinary_no_collect_post_first|62|60.241|0.159|25.920|33.993|unavailable|83.298|83.298|
|1|ulmdb|64|latency|lmdb|later_cached_collect|4|248.440|191.747|25.970|30.526|191.470|258.922|258.922|
|1|ulmdb|64|latency|lmdb|ordinary_no_collect_post_first|60|55.606|0.166|25.392|29.881|unavailable|60.425|60.425|
|1|ulmdb|64|latency|ulmdb|later_cached_collect|2|241.263|184.726|25.980|30.348|184.476|245.056|245.056|
|1|ulmdb|64|latency|ulmdb|ordinary_no_collect_post_first|62|55.875|0.170|25.551|29.987|unavailable|63.030|63.030|
|2|lmdb|1|aggregate|lmdb|first_cold_collect|1|7.063|0.090|0.902|5.921|unavailable|7.063|7.063|
|2|lmdb|1|aggregate|lmdb|ordinary_no_collect_post_first|2|6.958|0.101|0.822|5.896|unavailable|6.984|6.984|
|2|lmdb|1|aggregate|lmdb|ordinary_no_collect_pre_first|61|7.198|0.111|0.903|6.039|unavailable|8.275|8.275|
|2|lmdb|1|aggregate|ulmdb|ordinary_no_collect_pre_first|64|7.910|0.125|1.166|6.473|unavailable|19.507|19.507|
|2|lmdb|1|latency|lmdb|ordinary_no_collect_post_first|8|7.290|0.118|0.929|6.098|unavailable|8.566|8.566|
|2|lmdb|1|latency|lmdb|ordinary_no_collect_pre_first|56|7.116|0.106|0.881|5.982|unavailable|8.075|8.075|
|2|lmdb|1|latency|ulmdb|ordinary_no_collect_pre_first|64|7.229|0.109|0.986|5.989|unavailable|12.835|12.835|
|2|lmdb|64|aggregate|lmdb|later_cached_collect|2|89.245|0.170|35.207|53.722|unavailable|93.117|93.117|
|2|lmdb|64|aggregate|lmdb|ordinary_no_collect_post_first|62|84.341|0.136|34.876|49.182|unavailable|102.085|102.085|
|2|lmdb|64|aggregate|ulmdb|later_cached_collect|2|80.433|0.140|33.038|47.105|unavailable|80.593|80.593|
|2|lmdb|64|aggregate|ulmdb|ordinary_no_collect_post_first|62|93.700|0.147|36.548|56.856|unavailable|466.397|466.397|
|2|lmdb|64|latency|lmdb|later_cached_collect|4|84.759|0.178|35.946|48.484|unavailable|87.747|87.747|
|2|lmdb|64|latency|lmdb|ordinary_no_collect_post_first|60|84.249|0.138|34.551|49.412|unavailable|99.479|99.479|
|2|lmdb|64|latency|ulmdb|later_cached_collect|2|88.903|0.130|37.206|51.413|unavailable|89.590|89.590|
|2|lmdb|64|latency|ulmdb|ordinary_no_collect_post_first|62|84.456|0.138|35.041|49.130|unavailable|97.525|97.525|
|2|ulmdb|1|aggregate|lmdb|first_cold_collect|1|643.253|631.741|1.012|10.300|631.411|643.253|643.253|
|2|ulmdb|1|aggregate|lmdb|ordinary_no_collect_post_first|2|10.005|0.150|0.857|8.847|unavailable|10.150|10.150|
|2|ulmdb|1|aggregate|lmdb|ordinary_no_collect_pre_first|61|11.229|0.157|1.399|9.523|unavailable|14.046|14.046|
|2|ulmdb|1|aggregate|ulmdb|ordinary_no_collect_pre_first|64|11.380|0.160|1.401|9.664|unavailable|28.604|28.604|
|2|ulmdb|1|latency|lmdb|ordinary_no_collect_post_first|8|9.982|0.148|0.761|8.918|unavailable|11.241|11.241|
|2|ulmdb|1|latency|lmdb|ordinary_no_collect_pre_first|56|10.893|0.148|1.330|9.265|unavailable|12.143|12.143|
|2|ulmdb|1|latency|ulmdb|ordinary_no_collect_pre_first|64|10.872|0.148|1.312|9.263|unavailable|16.822|16.822|
|2|ulmdb|64|aggregate|lmdb|later_cached_collect|2|255.456|193.839|26.309|35.097|193.588|265.274|265.274|
|2|ulmdb|64|aggregate|lmdb|ordinary_no_collect_post_first|62|61.628|0.172|25.987|35.294|unavailable|92.285|92.285|
|2|ulmdb|64|aggregate|ulmdb|later_cached_collect|2|249.410|189.680|25.814|33.704|189.451|255.576|255.576|
|2|ulmdb|64|aggregate|ulmdb|ordinary_no_collect_post_first|62|59.925|0.157|25.548|34.048|unavailable|70.755|70.755|
|2|ulmdb|64|latency|lmdb|later_cached_collect|4|258.058|201.326|25.954|30.580|201.027|271.106|271.106|
|2|ulmdb|64|latency|lmdb|ordinary_no_collect_post_first|60|55.568|0.163|25.481|29.753|unavailable|61.828|61.828|
|2|ulmdb|64|latency|ulmdb|later_cached_collect|2|243.863|187.016|26.160|30.463|186.785|248.843|248.843|
|2|ulmdb|64|latency|ulmdb|ordinary_no_collect_post_first|62|55.877|0.165|25.643|29.891|unavailable|63.871|63.871|
|3|lmdb|1|aggregate|lmdb|first_cold_collect|1|7.123|0.090|0.932|5.951|unavailable|7.123|7.123|
|3|lmdb|1|aggregate|lmdb|ordinary_no_collect_post_first|2|7.023|0.096|0.847|5.936|unavailable|7.103|7.103|
|3|lmdb|1|aggregate|lmdb|ordinary_no_collect_pre_first|61|7.321|0.107|0.918|6.150|unavailable|11.342|11.342|
|3|lmdb|1|aggregate|ulmdb|ordinary_no_collect_pre_first|64|8.073|0.134|1.289|6.505|unavailable|26.380|26.380|
|3|lmdb|1|latency|lmdb|ordinary_no_collect_post_first|8|7.335|0.123|0.957|6.106|unavailable|8.567|8.567|
|3|lmdb|1|latency|lmdb|ordinary_no_collect_pre_first|56|7.147|0.112|0.880|6.010|unavailable|7.955|7.955|
|3|lmdb|1|latency|ulmdb|ordinary_no_collect_pre_first|64|7.204|0.109|0.908|6.041|unavailable|8.816|8.816|
|3|lmdb|64|aggregate|lmdb|later_cached_collect|2|91.624|0.131|36.389|54.955|unavailable|95.702|95.702|
|3|lmdb|64|aggregate|lmdb|ordinary_no_collect_post_first|62|85.593|0.149|35.337|49.957|unavailable|99.419|99.419|
|3|lmdb|64|aggregate|ulmdb|later_cached_collect|2|82.031|0.171|33.709|47.990|unavailable|82.197|82.197|
|3|lmdb|64|aggregate|ulmdb|ordinary_no_collect_post_first|62|89.921|0.165|36.920|52.681|unavailable|138.063|138.063|
|3|lmdb|64|latency|lmdb|later_cached_collect|4|88.724|0.188|38.879|49.504|unavailable|89.290|89.290|
|3|lmdb|64|latency|lmdb|ordinary_no_collect_post_first|60|86.679|0.151|36.466|49.908|unavailable|108.286|108.286|
|3|lmdb|64|latency|ulmdb|later_cached_collect|2|87.427|0.120|35.448|51.699|unavailable|91.324|91.324|
|3|lmdb|64|latency|ulmdb|ordinary_no_collect_post_first|62|86.789|0.157|36.260|50.221|unavailable|96.713|96.713|
|3|ulmdb|1|aggregate|lmdb|first_cold_collect|1|648.824|637.092|1.042|10.520|636.781|648.824|648.824|
|3|ulmdb|1|aggregate|lmdb|ordinary_no_collect_post_first|2|9.979|0.166|0.801|8.857|unavailable|10.169|10.169|
|3|ulmdb|1|aggregate|lmdb|ordinary_no_collect_pre_first|61|11.254|0.161|1.362|9.578|unavailable|14.077|14.077|
|3|ulmdb|1|aggregate|ulmdb|ordinary_no_collect_pre_first|64|11.299|0.165|1.412|9.565|unavailable|32.010|32.010|
|3|ulmdb|1|latency|lmdb|ordinary_no_collect_post_first|8|10.085|0.154|0.787|8.989|unavailable|11.482|11.482|
|3|ulmdb|1|latency|lmdb|ordinary_no_collect_pre_first|56|11.029|0.153|1.328|9.398|unavailable|17.202|17.202|
|3|ulmdb|1|latency|ulmdb|ordinary_no_collect_pre_first|64|10.830|0.152|1.307|9.217|unavailable|12.644|12.644|
|3|ulmdb|64|aggregate|lmdb|later_cached_collect|2|252.741|190.332|26.841|35.392|190.107|253.913|253.913|
|3|ulmdb|64|aggregate|lmdb|ordinary_no_collect_post_first|62|61.187|0.171|26.200|34.655|unavailable|85.342|85.342|
|3|ulmdb|64|aggregate|ulmdb|later_cached_collect|2|246.319|186.514|25.964|33.639|186.285|248.393|248.393|
|3|ulmdb|64|aggregate|ulmdb|ordinary_no_collect_post_first|62|59.670|0.161|25.641|33.708|unavailable|69.752|69.752|
|3|ulmdb|64|latency|lmdb|later_cached_collect|4|249.347|190.793|26.105|32.249|190.520|261.958|261.958|
|3|ulmdb|64|latency|lmdb|ordinary_no_collect_post_first|60|55.796|0.167|25.511|29.954|unavailable|63.792|63.792|
|3|ulmdb|64|latency|ulmdb|later_cached_collect|2|240.998|184.141|26.040|30.628|183.894|242.140|242.140|
|3|ulmdb|64|latency|ulmdb|ordinary_no_collect_post_first|62|55.984|0.173|25.781|29.871|unavailable|66.206|66.206|
|4|lmdb|1|aggregate|lmdb|first_cold_collect|1|7.083|0.110|0.902|5.931|unavailable|7.083|7.083|
|4|lmdb|1|aggregate|lmdb|ordinary_no_collect_post_first|2|6.933|0.110|0.797|5.881|unavailable|6.963|6.963|
|4|lmdb|1|aggregate|lmdb|ordinary_no_collect_pre_first|61|7.414|0.201|0.923|6.144|unavailable|12.894|12.894|
|4|lmdb|1|aggregate|ulmdb|ordinary_no_collect_pre_first|64|8.020|0.122|1.232|6.442|unavailable|22.844|22.844|
|4|lmdb|1|latency|lmdb|ordinary_no_collect_post_first|8|7.310|0.127|0.929|6.109|unavailable|8.697|8.697|
|4|lmdb|1|latency|lmdb|ordinary_no_collect_pre_first|56|7.136|0.108|0.883|5.999|unavailable|8.015|8.015|
|4|lmdb|1|latency|ulmdb|ordinary_no_collect_pre_first|64|7.148|0.107|0.889|6.005|unavailable|8.135|8.135|
|4|lmdb|64|aggregate|lmdb|later_cached_collect|2|89.721|0.155|35.242|54.178|unavailable|93.769|93.769|
|4|lmdb|64|aggregate|lmdb|ordinary_no_collect_post_first|62|84.831|0.149|35.032|49.503|unavailable|97.446|97.446|
|4|lmdb|64|aggregate|ulmdb|later_cached_collect|2|81.841|0.146|33.879|47.671|unavailable|81.896|81.896|
|4|lmdb|64|aggregate|ulmdb|ordinary_no_collect_post_first|62|89.129|0.160|36.237|52.586|unavailable|132.141|132.141|
|4|lmdb|64|latency|lmdb|later_cached_collect|4|85.675|0.180|36.875|48.475|unavailable|91.273|91.273|
|4|lmdb|64|latency|lmdb|ordinary_no_collect_post_first|60|85.059|0.152|35.054|49.707|unavailable|95.191|95.191|
|4|lmdb|64|latency|ulmdb|later_cached_collect|2|86.355|0.131|35.127|50.947|unavailable|89.761|89.761|
|4|lmdb|64|latency|ulmdb|ordinary_no_collect_post_first|62|85.381|0.159|35.806|49.270|unavailable|96.804|96.804|
|4|ulmdb|1|aggregate|lmdb|first_cold_collect|1|638.464|626.972|0.901|10.390|626.702|638.464|638.464|
|4|ulmdb|1|aggregate|lmdb|ordinary_no_collect_post_first|2|9.919|0.161|0.781|8.822|unavailable|10.069|10.069|
|4|ulmdb|1|aggregate|lmdb|ordinary_no_collect_pre_first|61|11.132|0.153|1.329|9.496|unavailable|14.247|14.247|
|4|ulmdb|1|aggregate|ulmdb|ordinary_no_collect_pre_first|64|11.192|0.158|1.374|9.500|unavailable|27.322|27.322|
|4|ulmdb|1|latency|lmdb|ordinary_no_collect_post_first|8|10.032|0.147|0.788|8.947|unavailable|11.422|11.422|
|4|ulmdb|1|latency|lmdb|ordinary_no_collect_pre_first|56|10.896|0.149|1.374|9.222|unavailable|18.465|18.465|
|4|ulmdb|1|latency|ulmdb|ordinary_no_collect_pre_first|64|10.754|0.148|1.282|9.173|unavailable|12.924|12.924|
|4|ulmdb|64|aggregate|lmdb|later_cached_collect|2|250.281|188.559|25.994|35.532|188.323|250.877|250.877|
|4|ulmdb|64|aggregate|lmdb|ordinary_no_collect_post_first|62|59.804|0.172|25.524|33.949|unavailable|66.296|66.296|
|4|ulmdb|64|aggregate|ulmdb|later_cached_collect|2|245.727|186.219|25.448|33.874|185.978|248.512|248.512|
|4|ulmdb|64|aggregate|ulmdb|ordinary_no_collect_post_first|62|59.861|0.160|25.540|34.002|unavailable|66.887|66.887|
|4|ulmdb|64|latency|lmdb|later_cached_collect|4|246.381|190.257|25.476|30.453|189.978|258.462|258.462|
|4|ulmdb|64|latency|lmdb|ordinary_no_collect_post_first|60|55.280|0.164|25.138|29.820|unavailable|64.373|64.373|
|4|ulmdb|64|latency|ulmdb|later_cached_collect|2|241.956|185.779|25.654|30.328|185.548|246.499|246.499|
|4|ulmdb|64|latency|ulmdb|ordinary_no_collect_post_first|62|57.670|0.168|27.693|29.650|unavailable|181.756|181.756|


### Complete tracking-only report: all reads, writes, tails, setup and resources

P is frozen retained 6, B is the tracking ELF with tracking disabled, and T is the SAME ELF with tracking enabled from empty genesis. Its table uses arithmetic means of four process values, including mean process rates; the coarse tables above use geometric centers. Do not splice or normalize the two campaigns. All signs and native changes remain visible. Observed quiet boundaries do not prove continuous host isolation.

### Ownership-maintenance cost control

All12 fixed full observations are included. P/B/T columns are arithmetic means of four process values, including mean per-process rates; ns cost rows are used for paired inference. Positive cost percentages mean slower/higher; rate percentages use their own direction. P→B is observed compiled/control difference; its cause is not assumed. B→T is the matched same-ELF tracking-cost comparison. Native changes are retained without normalization. No CPU fraction or production acceptance claim.

Timing qualification is only observed pre/post quiet; it is not continuous host isolation. Unqualified tags: none observed. No observations are excluded.

#### All13 reads and contemporaneous native

| Metric | P | B | T | P→B % | signs | B→T % | signs |
|---|---:|---:|---:|---:|:---:|---:|:---:|
| read.ipv4_lpm_hits.lmdb.mean_ns_per_operation | 467.96 | 468.98 | 463.41 | 0.22 | +++- | -1.19 | ---- |
| read.ipv4_lpm_hits.lmdb.mean_trial_p99_ns | 788.98 | 774.56 | 784.98 | -1.83 | ---- | 1.35 | +-++ |
| read.ipv4_lpm_hits.lmdb.operations_per_second | 2,137,275.06 | 2,132,293.45 | 2,158,135.60 | -0.23 | ---+ | 1.21 | ++++ |
| read.ipv4_lpm_hits.lmdb.pooled_sampled_p99_ns | 789.25 | 774.00 | 788.75 | -1.93 | ---- | 1.91 | +-++ |
| read.ipv4_lpm_hits.lmdb.sampled_max_ns | 15,664.50 | 42,718.75 | 20,251.50 | 172.71 | +-++ | -52.59 | +--+ |
| read.ipv4_lpm_hits.ulmdb.mean_ns_per_operation | 311.65 | 311.20 | 311.56 | -0.15 | --+- | 0.12 | ++-+ |
| read.ipv4_lpm_hits.ulmdb.mean_trial_p99_ns | 425.25 | 424.97 | 427.27 | -0.07 | --++ | 0.54 | ++-+ |
| read.ipv4_lpm_hits.ulmdb.operations_per_second | 3,208,722.77 | 3,213,419.61 | 3,209,810.15 | 0.15 | ++-+ | -0.11 | --+- |
| read.ipv4_lpm_hits.ulmdb.pooled_sampled_p99_ns | 425.75 | 426.00 | 430.75 | 0.06 | +-+0 | 1.12 | 0+-+ |
| read.ipv4_lpm_hits.ulmdb.sampled_max_ns | 16,411.00 | 15,331.75 | 11,053.75 | -6.58 | +-+- | -27.90 | ---+ |
| read.ipv4_lpm_inside_gap_misses.lmdb.mean_ns_per_operation | 486.43 | 498.29 | 498.87 | 2.44 | ++++ | 0.12 | -++- |
| read.ipv4_lpm_inside_gap_misses.lmdb.mean_trial_p99_ns | 606.22 | 613.98 | 622.03 | 1.28 | -+++ | 1.31 | +++- |
| read.ipv4_lpm_inside_gap_misses.lmdb.operations_per_second | 2,057,360.66 | 2,007,854.78 | 2,005,136.96 | -2.41 | ---- | -0.14 | +--+ |
| read.ipv4_lpm_inside_gap_misses.lmdb.pooled_sampled_p99_ns | 603.75 | 609.00 | 636.25 | 0.87 | -+++ | 4.47 | ++++ |
| read.ipv4_lpm_inside_gap_misses.lmdb.sampled_max_ns | 7,181.00 | 5,420.00 | 6,948.25 | -24.52 | ---- | 28.20 | ++-+ |
| read.ipv4_lpm_inside_gap_misses.ulmdb.mean_ns_per_operation | 85.80 | 85.27 | 87.94 | -0.62 | ---- | 3.14 | ++++ |
| read.ipv4_lpm_inside_gap_misses.ulmdb.mean_trial_p99_ns | 120.98 | 112.67 | 117.50 | -6.87 | ---- | 4.29 | ++++ |
| read.ipv4_lpm_inside_gap_misses.ulmdb.operations_per_second | 11,654,962.88 | 11,728,051.50 | 11,380,852.45 | 0.63 | ++++ | -2.96 | ---- |
| read.ipv4_lpm_inside_gap_misses.ulmdb.pooled_sampled_p99_ns | 121.00 | 122.75 | 137.75 | 1.45 | +--+ | 12.22 | ++++ |
| read.ipv4_lpm_inside_gap_misses.ulmdb.sampled_max_ns | 3,787.25 | 3,359.00 | 20,333.75 | -11.31 | ---+ | 505.35 | +--- |
| read.ipv4_lpm_misses.lmdb.mean_ns_per_operation | 248.58 | 256.65 | 254.59 | 3.25 | ++++ | -0.80 | -++- |
| read.ipv4_lpm_misses.lmdb.mean_trial_p99_ns | 593.75 | 609.25 | 608.53 | 2.61 | ++++ | -0.12 | -++- |
| read.ipv4_lpm_misses.lmdb.operations_per_second | 4,026,034.77 | 3,899,292.30 | 3,929,077.65 | -3.15 | ---- | 0.76 | +--+ |
| read.ipv4_lpm_misses.lmdb.pooled_sampled_p99_ns | 591.25 | 608.75 | 613.50 | 2.96 | 0+++ | 0.78 | +++- |
| read.ipv4_lpm_misses.lmdb.sampled_max_ns | 10,094.25 | 13,666.00 | 10,657.25 | 35.38 | +-++ | -22.02 | ++-- |
| read.ipv4_lpm_misses.ulmdb.mean_ns_per_operation | 46.13 | 45.79 | 46.42 | -0.74 | ---- | 1.39 | +--+ |
| read.ipv4_lpm_misses.ulmdb.mean_trial_p99_ns | 120.81 | 113.56 | 114.59 | -6.00 | ---- | 0.91 | +-+- |
| read.ipv4_lpm_misses.ulmdb.operations_per_second | 21,679,412.04 | 21,840,922.12 | 21,546,696.93 | 0.74 | ++++ | -1.35 | -++- |
| read.ipv4_lpm_misses.ulmdb.pooled_sampled_p99_ns | 123.50 | 115.50 | 118.25 | -6.48 | ---- | 2.38 | +-+- |
| read.ipv4_lpm_misses.ulmdb.sampled_max_ns | 3,501.75 | 4,989.75 | 5,761.00 | 42.49 | -+++ | 15.46 | ++-+ |
| read.ipv4_lpm_mixed.lmdb.mean_ns_per_operation | 371.64 | 378.42 | 370.72 | 1.82 | ++++ | -2.03 | ---- |
| read.ipv4_lpm_mixed.lmdb.mean_trial_p99_ns | 771.31 | 762.22 | 755.20 | -1.18 | +-+- | -0.92 | +-+- |
| read.ipv4_lpm_mixed.lmdb.operations_per_second | 2,691,307.73 | 2,643,060.68 | 2,698,319.39 | -1.79 | ---- | 2.09 | ++++ |
| read.ipv4_lpm_mixed.lmdb.pooled_sampled_p99_ns | 771.75 | 761.75 | 754.25 | -1.30 | +-+- | -0.98 | 0-0- |
| read.ipv4_lpm_mixed.lmdb.sampled_max_ns | 16,601.75 | 21,650.75 | 9,781.25 | 30.41 | ++++ | -54.82 | +--- |
| read.ipv4_lpm_mixed.ulmdb.mean_ns_per_operation | 180.33 | 180.55 | 180.49 | 0.12 | +-++ | -0.03 | ++-- |
| read.ipv4_lpm_mixed.ulmdb.mean_trial_p99_ns | 418.59 | 416.22 | 413.20 | -0.57 | +--- | -0.72 | -0-- |
| read.ipv4_lpm_mixed.ulmdb.operations_per_second | 5,545,430.05 | 5,538,842.58 | 5,540,519.59 | -0.12 | -+-- | 0.03 | --++ |
| read.ipv4_lpm_mixed.ulmdb.pooled_sampled_p99_ns | 420.75 | 415.75 | 413.25 | -1.19 | ---0 | -0.60 | 000- |
| read.ipv4_lpm_mixed.ulmdb.sampled_max_ns | 15,577.25 | 15,063.25 | 19,937.75 | -3.30 | -+-+ | 32.36 | +++- |
| read.ipv4_lpm_outside_cover_misses.lmdb.mean_ns_per_operation | 11.68 | 11.27 | 11.19 | -3.46 | -+-- | -0.71 | --++ |
| read.ipv4_lpm_outside_cover_misses.lmdb.mean_trial_p99_ns | 43.55 | 45.64 | 45.95 | 4.81 | +-+- | 0.68 | ++-+ |
| read.ipv4_lpm_outside_cover_misses.lmdb.operations_per_second | 86,376,816.17 | 90,197,769.12 | 90,406,875.58 | 4.42 | +-++ | 0.23 | ++-- |
| read.ipv4_lpm_outside_cover_misses.lmdb.pooled_sampled_p99_ns | 45.50 | 45.50 | 47.75 | 0.00 | +-00 | 4.95 | 0+-+ |
| read.ipv4_lpm_outside_cover_misses.lmdb.sampled_max_ns | 1,009.75 | 1,889.00 | 2,783.25 | 87.08 | --+- | 47.34 | --++ |
| read.ipv4_lpm_outside_cover_misses.ulmdb.mean_ns_per_operation | 9.97 | 9.90 | 9.97 | -0.69 | +--+ | 0.63 | ++-+ |
| read.ipv4_lpm_outside_cover_misses.ulmdb.mean_trial_p99_ns | 41.28 | 41.16 | 41.91 | -0.30 | 00-+ | 1.82 | +00+ |
| read.ipv4_lpm_outside_cover_misses.ulmdb.operations_per_second | 100,299,483.10 | 100,977,909.22 | 100,351,786.69 | 0.68 | -++- | -0.62 | --+- |
| read.ipv4_lpm_outside_cover_misses.ulmdb.pooled_sampled_p99_ns | 41.00 | 43.25 | 45.50 | 5.49 | 000+ | 5.20 | +000 |
| read.ipv4_lpm_outside_cover_misses.ulmdb.sampled_max_ns | 1,052.25 | 1,382.75 | 5,959.25 | 31.41 | 0+-+ | 330.97 | --+- |
| read.ipv6_lpm_hits.lmdb.mean_ns_per_operation | 797.58 | 836.69 | 836.11 | 4.90 | ++++ | -0.07 | -+-- |
| read.ipv6_lpm_hits.lmdb.mean_trial_p99_ns | 1,228.12 | 1,286.06 | 1,291.41 | 4.72 | ++++ | 0.42 | -+++ |
| read.ipv6_lpm_hits.lmdb.operations_per_second | 1,253,939.64 | 1,195,278.24 | 1,196,035.93 | -4.68 | ---- | 0.06 | +-++ |
| read.ipv6_lpm_hits.lmdb.pooled_sampled_p99_ns | 1,229.50 | 1,297.50 | 1,292.25 | 5.53 | ++++ | -0.40 | -+++ |
| read.ipv6_lpm_hits.lmdb.sampled_max_ns | 24,626.75 | 28,041.00 | 25,205.50 | 13.86 | --+- | -10.11 | ++-+ |
| read.ipv6_lpm_hits.ulmdb.mean_ns_per_operation | 404.44 | 398.13 | 396.48 | -1.56 | ---- | -0.41 | ---+ |
| read.ipv6_lpm_hits.ulmdb.mean_trial_p99_ns | 526.64 | 525.70 | 523.97 | -0.18 | +--- | -0.33 | -+-+ |
| read.ipv6_lpm_hits.ulmdb.operations_per_second | 2,472,571.75 | 2,511,754.50 | 2,522,205.00 | 1.58 | ++++ | 0.42 | +++- |
| read.ipv6_lpm_hits.ulmdb.pooled_sampled_p99_ns | 526.00 | 531.00 | 526.00 | 0.95 | +-00 | -0.94 | -+0+ |
| read.ipv6_lpm_hits.ulmdb.sampled_max_ns | 11,253.75 | 13,981.50 | 18,159.25 | 24.24 | +++- | 29.88 | -++- |
| read.ipv6_lpm_inside_gap_misses.lmdb.mean_ns_per_operation | 877.58 | 922.49 | 924.08 | 5.12 | ++++ | 0.17 | +++- |
| read.ipv6_lpm_inside_gap_misses.lmdb.mean_trial_p99_ns | 1,098.02 | 1,117.94 | 1,127.30 | 1.81 | +++- | 0.84 | -++- |
| read.ipv6_lpm_inside_gap_misses.lmdb.operations_per_second | 1,139,791.29 | 1,084,043.82 | 1,082,254.79 | -4.89 | ---- | -0.17 | ---+ |
| read.ipv6_lpm_inside_gap_misses.lmdb.pooled_sampled_p99_ns | 1,094.50 | 1,112.00 | 1,117.25 | 1.60 | 0++- | 0.47 | +++- |
| read.ipv6_lpm_inside_gap_misses.lmdb.sampled_max_ns | 18,392.75 | 50,150.00 | 11,003.75 | 172.66 | ++-+ | -78.06 | -++- |
| read.ipv6_lpm_inside_gap_misses.ulmdb.mean_ns_per_operation | 106.04 | 109.49 | 108.74 | 3.25 | ++++ | -0.68 | ---- |
| read.ipv6_lpm_inside_gap_misses.ulmdb.mean_trial_p99_ns | 141.14 | 142.25 | 142.03 | 0.79 | ++-0 | -0.15 | --++ |
| read.ipv6_lpm_inside_gap_misses.ulmdb.operations_per_second | 9,430,612.34 | 9,133,695.37 | 9,196,219.47 | -3.15 | ---- | 0.68 | ++++ |
| read.ipv6_lpm_inside_gap_misses.ulmdb.pooled_sampled_p99_ns | 141.00 | 141.00 | 143.50 | 0.00 | 0000 | 1.77 | 00+0 |
| read.ipv6_lpm_inside_gap_misses.ulmdb.sampled_max_ns | 2,998.25 | 4,916.50 | 3,879.75 | 63.98 | ++++ | -21.09 | ---- |
| read.ipv6_lpm_misses.lmdb.mean_ns_per_operation | 444.43 | 467.26 | 468.62 | 5.14 | ++++ | 0.29 | +-+- |
| read.ipv6_lpm_misses.lmdb.mean_trial_p99_ns | 1,069.38 | 1,102.97 | 1,099.91 | 3.14 | ++++ | -0.28 | +-+- |
| read.ipv6_lpm_misses.lmdb.operations_per_second | 2,250,769.80 | 2,140,196.53 | 2,134,137.94 | -4.91 | ---- | -0.28 | -+-+ |
| read.ipv6_lpm_misses.lmdb.pooled_sampled_p99_ns | 1,067.00 | 1,099.50 | 1,099.75 | 3.05 | 0+++ | 0.02 | +-+- |
| read.ipv6_lpm_misses.lmdb.sampled_max_ns | 11,710.00 | 23,640.00 | 10,131.75 | 101.88 | --++ | -57.14 | 0+-- |
| read.ipv6_lpm_misses.ulmdb.mean_ns_per_operation | 58.00 | 58.19 | 57.88 | 0.32 | ++-- | -0.53 | ---+ |
| read.ipv6_lpm_misses.ulmdb.mean_trial_p99_ns | 143.48 | 143.25 | 142.33 | -0.16 | --+- | -0.64 | +--- |
| read.ipv6_lpm_misses.ulmdb.operations_per_second | 17,242,043.99 | 17,186,823.20 | 17,277,923.22 | -0.32 | --++ | 0.53 | +++- |
| read.ipv6_lpm_misses.ulmdb.pooled_sampled_p99_ns | 141.00 | 141.00 | 141.00 | 0.00 | 0000 | 0.00 | 0000 |
| read.ipv6_lpm_misses.ulmdb.sampled_max_ns | 14,615.25 | 12,944.50 | 9,112.50 | -11.43 | +--+ | -29.60 | ---- |
| read.ipv6_lpm_mixed.lmdb.mean_ns_per_operation | 635.48 | 666.97 | 667.79 | 4.96 | ++++ | 0.12 | ++-+ |
| read.ipv6_lpm_mixed.lmdb.mean_trial_p99_ns | 1,226.47 | 1,273.22 | 1,285.61 | 3.81 | ++++ | 0.97 | ++++ |
| read.ipv6_lpm_mixed.lmdb.operations_per_second | 1,573,761.34 | 1,499,384.01 | 1,497,496.10 | -4.73 | ---- | -0.13 | --+- |
| read.ipv6_lpm_mixed.lmdb.pooled_sampled_p99_ns | 1,222.25 | 1,270.00 | 1,282.50 | 3.91 | ++++ | 0.98 | ++++ |
| read.ipv6_lpm_mixed.lmdb.sampled_max_ns | 32,854.50 | 29,606.25 | 31,955.75 | -9.89 | +--+ | 7.94 | -++- |
| read.ipv6_lpm_mixed.ulmdb.mean_ns_per_operation | 233.08 | 228.54 | 228.54 | -1.95 | ---- | 0.00 | ++-- |
| read.ipv6_lpm_mixed.ulmdb.mean_trial_p99_ns | 518.97 | 511.58 | 512.86 | -1.42 | 0--- | 0.25 | ++-+ |
| read.ipv6_lpm_mixed.ulmdb.operations_per_second | 4,290,404.56 | 4,375,757.85 | 4,375,704.00 | 1.99 | ++++ | -0.00 | --++ |
| read.ipv6_lpm_mixed.ulmdb.pooled_sampled_p99_ns | 518.50 | 511.00 | 511.00 | -1.45 | 0--0 | 0.00 | 0000 |
| read.ipv6_lpm_mixed.ulmdb.sampled_max_ns | 11,043.50 | 12,050.25 | 11,281.50 | 9.12 | -+-- | -6.38 | +-+- |
| read.ipv6_lpm_outside_cover_misses.lmdb.mean_ns_per_operation | 11.40 | 10.97 | 10.98 | -3.81 | -+-- | 0.10 | --++ |
| read.ipv6_lpm_outside_cover_misses.lmdb.mean_trial_p99_ns | 42.72 | 43.55 | 42.97 | 1.94 | +-+0 | -1.33 | -+-0 |
| read.ipv6_lpm_outside_cover_misses.lmdb.operations_per_second | 88,115,488.35 | 91,719,177.75 | 91,661,837.42 | 4.09 | +-++ | -0.06 | ++-- |
| read.ipv6_lpm_outside_cover_misses.lmdb.pooled_sampled_p99_ns | 45.50 | 45.50 | 45.50 | 0.00 | 0-+0 | 0.00 | 0+-0 |
| read.ipv6_lpm_outside_cover_misses.lmdb.sampled_max_ns | 1,220.25 | 2,610.00 | 3,531.75 | 113.89 | +0++ | 35.32 | +-+- |
| read.ipv6_lpm_outside_cover_misses.ulmdb.mean_ns_per_operation | 10.26 | 10.36 | 10.26 | 0.98 | ++-+ | -0.98 | ---- |
| read.ipv6_lpm_outside_cover_misses.ulmdb.mean_trial_p99_ns | 41.14 | 41.16 | 41.14 | 0.04 | 0+00 | -0.04 | +-00 |
| read.ipv6_lpm_outside_cover_misses.ulmdb.operations_per_second | 97,490,269.75 | 96,562,683.93 | 97,498,780.54 | -0.95 | --+- | 0.97 | ++++ |
| read.ipv6_lpm_outside_cover_misses.ulmdb.pooled_sampled_p99_ns | 41.00 | 43.25 | 41.00 | 5.49 | 0+00 | -5.20 | 0-00 |
| read.ipv6_lpm_outside_cover_misses.ulmdb.sampled_max_ns | 3,070.75 | 1,217.50 | 1,483.25 | -60.35 | ---0 | 21.83 | ++-+ |
| read.ordered_kv_hits.lmdb.mean_ns_per_operation | 238.88 | 244.77 | 245.25 | 2.47 | ++++ | 0.20 | ++-+ |
| read.ordered_kv_hits.lmdb.mean_trial_p99_ns | 338.16 | 334.08 | 337.67 | -1.21 | +--- | 1.08 | ++-+ |
| read.ordered_kv_hits.lmdb.operations_per_second | 4,186,237.43 | 4,085,569.18 | 4,077,549.25 | -2.40 | ---- | -0.20 | --+- |
| read.ordered_kv_hits.lmdb.pooled_sampled_p99_ns | 340.25 | 333.25 | 342.75 | -2.06 | --0- | 2.85 | ++-+ |
| read.ordered_kv_hits.lmdb.sampled_max_ns | 37,032.75 | 22,347.50 | 8,726.75 | -39.65 | +--- | -60.95 | ---- |
| read.ordered_kv_hits.ulmdb.mean_ns_per_operation | 128.31 | 133.36 | 132.98 | 3.94 | +-++ | -0.28 | ++-- |
| read.ordered_kv_hits.ulmdb.mean_trial_p99_ns | 195.09 | 199.77 | 201.86 | 2.39 | +-++ | 1.05 | ++-- |
| read.ordered_kv_hits.ulmdb.operations_per_second | 7,795,934.96 | 7,502,521.13 | 7,526,984.85 | -3.76 | -+-- | 0.33 | --++ |
| read.ordered_kv_hits.ulmdb.pooled_sampled_p99_ns | 193.50 | 198.25 | 207.50 | 2.45 | +-++ | 4.67 | ++-0 |
| read.ordered_kv_hits.ulmdb.sampled_max_ns | 5,157.50 | 21,744.00 | 14,209.50 | 321.60 | ++++ | -34.65 | -+-- |
| read.ordered_kv_misses.lmdb.mean_ns_per_operation | 247.78 | 247.72 | 248.24 | -0.03 | --+- | 0.21 | ++-+ |
| read.ordered_kv_misses.lmdb.mean_trial_p99_ns | 340.62 | 341.48 | 339.53 | 0.25 | --++ | -0.57 | -+-- |
| read.ordered_kv_misses.lmdb.operations_per_second | 4,035,900.67 | 4,036,917.10 | 4,028,489.12 | 0.03 | ++-+ | -0.21 | --+- |
| read.ordered_kv_misses.lmdb.pooled_sampled_p99_ns | 343.00 | 345.50 | 345.25 | 0.73 | --++ | -0.07 | 0++- |
| read.ordered_kv_misses.lmdb.sampled_max_ns | 20,787.00 | 6,655.25 | 7,101.00 | -67.98 | --++ | 6.70 | +--+ |
| read.ordered_kv_misses.ulmdb.mean_ns_per_operation | 128.07 | 132.13 | 131.46 | 3.18 | +-++ | -0.51 | ++-- |
| read.ordered_kv_misses.ulmdb.mean_trial_p99_ns | 195.56 | 200.58 | 200.53 | 2.56 | +-++ | -0.02 | ++-- |
| read.ordered_kv_misses.ulmdb.operations_per_second | 7,812,192.78 | 7,572,009.95 | 7,611,980.03 | -3.07 | -+-- | 0.53 | --++ |
| read.ordered_kv_misses.ulmdb.pooled_sampled_p99_ns | 198.25 | 203.00 | 203.00 | 2.40 | 0-++ | 0.00 | ++-- |
| read.ordered_kv_misses.ulmdb.sampled_max_ns | 6,447.25 | 5,788.50 | 7,248.75 | -10.22 | --++ | 25.23 | ++-+ |
| read.ordered_kv_mixed.lmdb.mean_ns_per_operation | 242.38 | 249.93 | 248.88 | 3.11 | ++++ | -0.42 | +--+ |
| read.ordered_kv_mixed.lmdb.mean_trial_p99_ns | 336.84 | 340.20 | 337.64 | 1.00 | --++ | -0.75 | +--+ |
| read.ordered_kv_mixed.lmdb.operations_per_second | 4,125,703.16 | 4,001,334.06 | 4,017,990.74 | -3.01 | ---- | 0.42 | -++- |
| read.ordered_kv_mixed.lmdb.pooled_sampled_p99_ns | 340.00 | 345.75 | 340.75 | 1.69 | 0+++ | -1.45 | 0--+ |
| read.ordered_kv_mixed.lmdb.sampled_max_ns | 27,291.75 | 23,662.75 | 20,794.75 | -13.30 | ---+ | -12.12 | ++-- |
| read.ordered_kv_mixed.ulmdb.mean_ns_per_operation | 134.35 | 139.12 | 137.95 | 3.55 | +-++ | -0.84 | ++-- |
| read.ordered_kv_mixed.ulmdb.mean_trial_p99_ns | 199.86 | 207.67 | 204.95 | 3.91 | +-++ | -1.31 | ++-- |
| read.ordered_kv_mixed.ulmdb.operations_per_second | 7,445,536.27 | 7,190,254.18 | 7,252,188.72 | -3.43 | -+-- | 0.86 | --++ |
| read.ordered_kv_mixed.ulmdb.pooled_sampled_p99_ns | 200.25 | 208.50 | 207.75 | 4.12 | +0++ | -0.36 | ++-- |
| read.ordered_kv_mixed.ulmdb.sampled_max_ns | 15,106.25 | 10,971.00 | 7,621.75 | -27.37 | --++ | -30.53 | +--- |

#### Both writes: aggregate, sampled, combined and tails

| Metric | P | B | T | P→B % | signs | B→T % | signs |
|---|---:|---:|---:|---:|:---:|---:|:---:|
| write.batch1.lmdb.aggregate_ns_per_commit | 7,345.65 | 7,360.67 | 7,503.04 | 0.20 | -+++ | 1.93 | ++++ |
| write.batch1.lmdb.aggregate_records_per_second | 136,141.14 | 135,864.73 | 133,287.13 | -0.20 | +--- | -1.90 | ---- |
| write.batch1.lmdb.combined_ns_per_commit | 7,150.07 | 7,170.54 | 7,347.13 | 0.29 | -+++ | 2.46 | ++++ |
| write.batch1.lmdb.combined_records_per_second | 139,861.31 | 139,462.07 | 136,109.49 | -0.29 | +--- | -2.40 | ---- |
| write.batch1.lmdb.mean_trial_sampled_p99_ns | 7,867.92 | 7,928.67 | 9,091.23 | 0.77 | +--+ | 14.66 | ++++ |
| write.batch1.lmdb.pooled_sampled_p99_ns | 9,290.50 | 8,506.25 | 9,485.75 | -8.44 | +-++ | 11.52 | ++++ |
| write.batch1.lmdb.sampled_max_ns | 10,334.50 | 11,734.50 | 9,623.25 | 13.55 | ++-+ | -17.99 | --+- |
| write.batch1.lmdb.sampled_ns_per_commit | 6,954.50 | 6,980.40 | 7,191.23 | 0.37 | +--+ | 3.02 | ++++ |
| write.batch1.lmdb.sampled_records_per_second | 143,797.90 | 143,259.02 | 139,059.56 | -0.37 | -++- | -2.93 | ---- |
| write.batch1.ulmdb.aggregate_ns_per_commit | 15,785.06 | 15,749.06 | 65,652.86 | -0.23 | ---+ | 316.87 | ++++ |
| write.batch1.ulmdb.aggregate_records_per_second | 63,354.17 | 63,496.57 | 15,231.70 | 0.22 | +++- | -76.01 | ---- |
| write.batch1.ulmdb.combined_ns_per_commit | 13,103.10 | 13,099.78 | 60,941.07 | -0.03 | +-++ | 365.21 | ++++ |
| write.batch1.ulmdb.combined_records_per_second | 76,319.50 | 76,337.30 | 16,409.42 | 0.02 | -+-- | -78.50 | ---- |
| write.batch1.ulmdb.mean_trial_sampled_p99_ns | 11,441.78 | 11,644.70 | 61,656.97 | 1.77 | +-+- | 429.49 | ++++ |
| write.batch1.ulmdb.pooled_sampled_p99_ns | 12,912.00 | 13,811.25 | 65,199.00 | 6.96 | +-+- | 372.07 | ++++ |
| write.batch1.ulmdb.sampled_max_ns | 13,906.25 | 15,459.25 | 69,747.75 | 11.17 | +-+- | 351.17 | ++++ |
| write.batch1.ulmdb.sampled_ns_per_commit | 10,421.13 | 10,450.49 | 56,229.28 | 0.28 | +-+- | 438.05 | ++++ |
| write.batch1.ulmdb.sampled_records_per_second | 95,962.19 | 95,689.86 | 17,784.56 | -0.28 | -+-+ | -81.41 | ---- |
| write.batch64.lmdb.aggregate_ns_per_commit | 84,945.12 | 85,513.37 | 84,757.38 | 0.67 | --++ | -0.88 | ++-- |
| write.batch64.lmdb.aggregate_records_per_second | 753,550.84 | 748,857.09 | 755,122.55 | -0.62 | ++-- | 0.84 | --++ |
| write.batch64.lmdb.combined_ns_per_commit | 84,116.60 | 84,731.63 | 83,784.58 | 0.73 | --++ | -1.12 | ++-- |
| write.batch64.lmdb.combined_records_per_second | 760,959.49 | 755,707.69 | 763,866.83 | -0.69 | ++-- | 1.08 | --++ |
| write.batch64.lmdb.mean_trial_sampled_p99_ns | 89,927.70 | 91,965.50 | 91,987.91 | 2.27 | --++ | 0.02 | ++-- |
| write.batch64.lmdb.pooled_sampled_p99_ns | 95,360.75 | 100,098.00 | 97,701.00 | 4.97 | -+-+ | -2.39 | +++- |
| write.batch64.lmdb.sampled_max_ns | 97,029.50 | 105,686.00 | 100,007.50 | 8.92 | ++-+ | -5.37 | -++- |
| write.batch64.lmdb.sampled_ns_per_commit | 83,288.07 | 83,949.89 | 82,811.79 | 0.79 | --++ | -1.36 | +--- |
| write.batch64.lmdb.sampled_records_per_second | 768,516.10 | 762,692.47 | 772,887.18 | -0.76 | ++-- | 1.34 | -+++ |
| write.batch64.ulmdb.aggregate_ns_per_commit | 64,461.29 | 65,081.22 | 257,721.55 | 0.96 | ++++ | 296.00 | ++++ |
| write.batch64.ulmdb.aggregate_records_per_second | 992,872.29 | 983,401.00 | 248,330.69 | -0.95 | ---- | -74.75 | ---- |
| write.batch64.ulmdb.combined_ns_per_commit | 63,996.02 | 64,482.19 | 259,699.06 | 0.76 | ++++ | 302.75 | ++++ |
| write.batch64.ulmdb.combined_records_per_second | 1,000,077.80 | 992,531.34 | 246,439.72 | -0.75 | ---- | -75.17 | ---- |
| write.batch64.ulmdb.mean_trial_sampled_p99_ns | 130,053.77 | 129,608.06 | 440,618.97 | -0.34 | -+-- | 239.96 | ++++ |
| write.batch64.ulmdb.pooled_sampled_p99_ns | 253,288.75 | 246,962.00 | 763,907.75 | -2.50 | -+-- | 209.32 | ++++ |
| write.batch64.ulmdb.sampled_max_ns | 272,255.25 | 267,137.50 | 783,755.25 | -1.88 | -+++ | 193.39 | ++++ |
| write.batch64.ulmdb.sampled_ns_per_commit | 63,530.75 | 63,883.15 | 261,676.58 | 0.55 | ++++ | 309.62 | ++++ |
| write.batch64.ulmdb.sampled_records_per_second | 1,007,393.14 | 1,001,837.16 | 244,577.41 | -0.55 | ---- | -75.59 | ---- |

Maximum/p99 write values describe the separate sampled commits only. Aggregate single-commit maxima/p99 are unavailable because that phase has no per-commit samples. Mean process p99 and true global pooled p99 are distinct; exact global values are retained in analysis.json.

#### Creation and cold costs

| Metric | P | B | T | P→B % | signs | B→T % | signs |
|---|---:|---:|---:|---:|:---:|---:|:---:|
| cold.complete_cold_check_ns | 52,861,746.75 | 52,652,511.25 | 53,222,162.00 | -0.40 | +-+- | 1.08 | ++++ |
| cold.environment_drop_ns | 87,083.00 | 96,538.50 | 1,389,324.75 | 10.86 | -+++ | 1,339.14 | ++++ |
| cold.experimental_reenable_ns | 1,971.25 | 2,379.50 | 2,514.75 | 20.71 | ++++ | 5.68 | -+-+ |
| cold.open_ns | 42,466,997.50 | 42,231,820.25 | 41,592,303.75 | -0.55 | +-+- | -1.51 | ---- |
| cold.validation_ns | 6,077,767.00 | 6,100,701.25 | 6,172,104.50 | 0.38 | +-+- | 1.17 | ++-+ |
| creation.after_setup_ns | 4,451.25 | 4,636.25 | 976.75 | 4.16 | --++ | -78.93 | ---- |
| creation.dataset_ns | 266,113,160.50 | 267,075,247.00 | 266,880,592.25 | 0.36 | +-++ | -0.07 | -++- |
| creation.fixture_load_both_ns | 9,001,710.00 | 9,064,657.25 | 16,332,189.50 | 0.70 | --++ | 80.17 | ++++ |
| creation.lmdb_create_ns | 127,783.00 | 132,086.25 | 120,719.50 | 3.37 | +-++ | -8.61 | -+-- |
| creation.sum_reported_setup_ns | 321,973,126.00 | 324,154,032.25 | 331,606,749.50 | 0.68 | +-++ | 2.30 | ++++ |
| creation.ulmdb_create_ns | 46,726,021.25 | 47,877,405.50 | 48,272,271.50 | 2.46 | ++++ | 0.82 | --+- |

ULMDB create includes the from-empty bootstrap. fixture_load_both_ns is combined ULMDB+LMDB fixture loading, not an isolated ULMDB cost. All setup fields stay charged and reported; sum_reported_setup_ns is the sum of reported setup categories, not a whole-process total. Reopen does not enable tracking.

#### Resources and ordinary accounting

| Metric | P | B | T | P→B % | signs | B→T % | signs |
|---|---:|---:|---:|---:|:---:|---:|:---:|
| accounting.backend_after.auto_collect_below | 256.00 | 256.00 | 256.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.collect_cache.entries | 2.00 | 2.00 | 2.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.collect_cache.hits | 2.00 | 2.00 | 2.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.collect_cache.misses | 0.00 | 0.00 | 0.00 | unavailable | 0000 | unavailable | 0000 |
| accounting.backend_after.collect_cache.retained_bytes | 2,368.00 | 2,368.00 | 2,368.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.pool_kv_pages | 1,024.00 | 1,024.00 | 1,024.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.pool_metadata_bound_bytes | 13,320,224.00 | 13,320,224.00 | 13,320,224.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.pool_reused_pages | 7,722.00 | 7,722.00 | 7,722.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.pool_rounded_mapping_bytes | 8,388,608.00 | 8,388,608.00 | 8,388,608.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.pool_tiny_pages | 1,024.00 | 1,024.00 | 1,024.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.private_admitted_capacity_hints.charged | 283,312.00 | 283,312.00 | 283,312.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.private_admitted_capacity_hints.frozen | 65,536.00 | 65,536.00 | 65,536.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.private_admitted_capacity_hints.index | 4,096.00 | 4,096.00 | 4,096.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.private_admitted_capacity_hints.packed | 212,992.00 | 212,992.00 | 212,992.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.private_current_global_budget_bytes | 53,788,640.00 | 53,788,640.00 | 53,788,640.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.private_current_global_reserved_bytes | 283,312.00 | 283,312.00 | 283,312.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.retained_frame_workspace_bytes | 528,528.00 | 528,528.00 | 528,528.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.retained_recipe_workspace_bytes | 9,216.00 | 9,216.00 | 9,216.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.sequence | 513.00 | 513.00 | 513.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.wal_capacity | 67,108,864.00 | 67,108,864.00 | 67,108,864.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.wal_selected_end | 2,205,392.00 | 2,205,392.00 | 2,205,392.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.auto_collect_below | 256.00 | 256.00 | 256.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.collect_cache.entries | 0.00 | 0.00 | 0.00 | unavailable | 0000 | unavailable | 0000 |
| accounting.backend_before.collect_cache.hits | 0.00 | 0.00 | 0.00 | unavailable | 0000 | unavailable | 0000 |
| accounting.backend_before.collect_cache.misses | 0.00 | 0.00 | 0.00 | unavailable | 0000 | unavailable | 0000 |
| accounting.backend_before.collect_cache.retained_bytes | 648.00 | 648.00 | 648.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.pool_kv_pages | 1,024.00 | 1,024.00 | 1,024.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.pool_metadata_bound_bytes | 13,320,224.00 | 13,320,224.00 | 13,320,224.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.pool_reused_pages | 0.00 | 0.00 | 0.00 | unavailable | 0000 | unavailable | 0000 |
| accounting.backend_before.pool_rounded_mapping_bytes | 8,388,608.00 | 8,388,608.00 | 8,388,608.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.pool_tiny_pages | 1,024.00 | 1,024.00 | 1,024.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.private_admitted_capacity_hints.charged | 283,312.00 | 283,312.00 | 283,312.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.private_admitted_capacity_hints.frozen | 65,536.00 | 65,536.00 | 65,536.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.private_admitted_capacity_hints.index | 4,096.00 | 4,096.00 | 4,096.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.private_admitted_capacity_hints.packed | 212,992.00 | 212,992.00 | 212,992.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.private_current_global_budget_bytes | 53,788,640.00 | 53,788,640.00 | 53,788,640.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.private_current_global_reserved_bytes | 283,312.00 | 283,312.00 | 283,312.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.retained_frame_workspace_bytes | 528,528.00 | 528,528.00 | 528,528.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.retained_recipe_workspace_bytes | 7,168.00 | 7,168.00 | 7,168.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.sequence | 1.00 | 1.00 | 1.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.wal_capacity | 67,108,864.00 | 67,108,864.00 | 67,108,864.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.wal_selected_end | 725,136.00 | 725,136.00 | 725,136.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.lmdb.allocated_file_bytes | 729,088.00 | 729,088.00 | 729,088.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.lmdb.apparent_file_bytes | 733,184.00 | 733,184.00 | 733,184.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.lmdb.files | 2.00 | 2.00 | 2.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.lmdb.mapped_pss_kib | 712.00 | 712.00 | 712.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.lmdb.mapped_rss_kib | 712.00 | 712.00 | 712.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.lmdb.mapped_virtual_kib | 524,296.00 | 524,296.00 | 524,296.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.ulmdb.allocated_file_bytes | 67,108,864.00 | 67,108,864.00 | 67,108,864.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.ulmdb.apparent_file_bytes | 67,108,864.00 | 67,108,864.00 | 67,108,864.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.ulmdb.files | 2.00 | 2.00 | 2.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.whole_process_vm_rss_bytes | 15,084,544.00 | 15,166,464.00 | 18,565,120.00 | 0.54 | ++++ | 22.41 | ++++ |
| resource.before.lmdb.allocated_file_bytes | 352,256.00 | 352,256.00 | 352,256.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.lmdb.apparent_file_bytes | 356,352.00 | 356,352.00 | 356,352.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.lmdb.files | 2.00 | 2.00 | 2.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.lmdb.mapped_pss_kib | 12.00 | 12.00 | 12.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.lmdb.mapped_rss_kib | 12.00 | 12.00 | 12.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.lmdb.mapped_virtual_kib | 524,296.00 | 524,296.00 | 524,296.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.ulmdb.allocated_file_bytes | 67,108,864.00 | 67,108,864.00 | 67,108,864.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.ulmdb.apparent_file_bytes | 67,108,864.00 | 67,108,864.00 | 67,108,864.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.ulmdb.files | 2.00 | 2.00 | 2.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.whole_process_vm_rss_bytes | 13,683,712.00 | 13,732,864.00 | 16,861,184.00 | 0.36 | -+++ | 22.78 | ++++ |

Whole-process VmRSS includes BOTH engines and unrelated process memory. Resident pathname RSS/PSS remain unavailable; native mapped RSS/PSS are path-attributed. Tracker metadata is a per-certificate accounting bound excluding simultaneous old/pending state, scratch, allocator slack and RSS.

#### Ownership operation conservation

Each full process:513 Writes,11 Collects, zero ordinary retirement, identical normalized complete selected-frame payload sequence and Collect contexts. Actual matched current6 coarse O/C full-audit graph reads and T acknowledged full-audit reads agree: 7304. P/B counters are unavailable, not zero.

T seed has1 acknowledged Write; final has513; business interval has512 Writes and11 Collects. Cumulative certificate operations combine Write and Collect preparations. Dividing by513 total or512 business writes is an explicit normalization that still includes11 Collect preparations, not a measured Write-only CPU cost. Bootstrap/attempted/failed/aborted/empty-no-publication-Collect work remain unavailable.

All four counter rows and seed/business deltas are retained in analysis.json. All per-process scalar values and four paired signs/deltas are retained there and in all-process-metrics.csv, with all original report/trial/raw evidence preserved.

#### Next evidence gate

If continuous ownership maintenance has material repeated cost, profile its actual prepare/ensure/delta/allocation path before implementing an issuer; operation counters do not establish CPU dominance. Keep the new issuer disabled. Retain current source6 coarse begin/mutation/commit attribution separately. Any gain from removing full marks must later cover measured maintenance and setup costs while passing the full read/write/cold/ownership/durability gates.


### True globally pooled sampled tails for the tracking control

These pool all four raw files per arm. They are distinct from the mean per-process pooled p99 rows above. The write pools have 512 sampled commits per arm; aggregate commits have no per-commit samples. Every maximum remains.

|Case / backend|Arm|Samples|Global sampled p99 ns|Global sampled maximum ns|
|---|---|---:|---:|---:|
|read.ordered_kv_mixed.ulmdb|P|256,000|200|42,401|
|read.ordered_kv_mixed.ulmdb|B|256,000|211|23,836|
|read.ordered_kv_mixed.ulmdb|T|256,000|210|9,098|
|read.ordered_kv_mixed.lmdb|P|256,000|340|48,191|
|read.ordered_kv_mixed.lmdb|B|256,000|341|29,126|
|read.ordered_kv_mixed.lmdb|T|256,000|341|37,612|
|read.ordered_kv_hits.ulmdb|P|127,360|200|5,721|
|read.ordered_kv_hits.ulmdb|B|127,360|201|27,953|
|read.ordered_kv_hits.ulmdb|T|127,360|210|30,568|
|read.ordered_kv_hits.lmdb|P|127,360|340|77,227|
|read.ordered_kv_hits.lmdb|B|127,360|331|30,418|
|read.ordered_kv_hits.lmdb|T|127,360|341|13,867|
|read.ordered_kv_misses.ulmdb|P|128,640|200|9,338|
|read.ordered_kv_misses.ulmdb|B|128,640|201|6,683|
|read.ordered_kv_misses.ulmdb|T|128,640|201|10,650|
|read.ordered_kv_misses.lmdb|P|128,640|341|45,666|
|read.ordered_kv_misses.lmdb|B|128,640|341|7,174|
|read.ordered_kv_misses.lmdb|T|128,640|341|8,607|
|read.ipv4_lpm_mixed.ulmdb|P|256,000|421|25,799|
|read.ipv4_lpm_mixed.ulmdb|B|256,000|420|25,799|
|read.ipv4_lpm_mixed.ulmdb|T|256,000|411|42,270|
|read.ipv4_lpm_mixed.lmdb|P|256,000|772|26,090|
|read.ipv4_lpm_mixed.lmdb|B|256,000|762|26,400|
|read.ipv4_lpm_mixed.lmdb|T|256,000|752|13,987|
|read.ipv4_lpm_hits.ulmdb|P|128,000|430|47,921|
|read.ipv4_lpm_hits.ulmdb|B|128,000|430|25,408|
|read.ipv4_lpm_hits.ulmdb|T|128,000|431|25,138|
|read.ipv4_lpm_hits.lmdb|P|128,000|792|41,669|
|read.ipv4_lpm_hits.lmdb|B|128,000|772|99,589|
|read.ipv4_lpm_hits.lmdb|T|128,000|791|42,882|
|read.ipv4_lpm_misses.ulmdb|P|128,000|121|5,410|
|read.ipv4_lpm_misses.ulmdb|B|128,000|120|6,873|
|read.ipv4_lpm_misses.ulmdb|T|128,000|120|8,506|
|read.ipv4_lpm_misses.lmdb|P|128,000|611|24,136|
|read.ipv4_lpm_misses.lmdb|B|128,000|612|26,330|
|read.ipv4_lpm_misses.lmdb|T|128,000|621|12,503|
|read.ipv4_lpm_outside_cover_misses.ulmdb|P|64,000|41|4,038|
|read.ipv4_lpm_outside_cover_misses.ulmdb|B|64,000|41|5,340|
|read.ipv4_lpm_outside_cover_misses.ulmdb|T|64,000|50|23,685|
|read.ipv4_lpm_outside_cover_misses.lmdb|P|64,000|41|3,847|
|read.ipv4_lpm_outside_cover_misses.lmdb|B|64,000|50|3,788|
|read.ipv4_lpm_outside_cover_misses.lmdb|T|64,000|50|6,192|
|read.ipv4_lpm_inside_gap_misses.ulmdb|P|64,000|121|4,258|
|read.ipv4_lpm_inside_gap_misses.ulmdb|B|64,000|120|5,461|
|read.ipv4_lpm_inside_gap_misses.ulmdb|T|64,000|140|72,448|
|read.ipv4_lpm_inside_gap_misses.lmdb|P|64,000|621|10,129|
|read.ipv4_lpm_inside_gap_misses.lmdb|B|64,000|611|6,943|
|read.ipv4_lpm_inside_gap_misses.lmdb|T|64,000|641|9,809|
|read.ipv6_lpm_mixed.ulmdb|P|256,000|521|20,930|
|read.ipv6_lpm_mixed.ulmdb|B|256,000|511|23,625|
|read.ipv6_lpm_mixed.ulmdb|T|256,000|511|23,625|
|read.ipv6_lpm_mixed.lmdb|P|256,000|1,223|57,058|
|read.ipv6_lpm_mixed.lmdb|B|256,000|1,272|48,051|
|read.ipv6_lpm_mixed.lmdb|T|256,000|1,282|58,942|
|read.ipv6_lpm_hits.ulmdb|P|128,000|531|24,486|
|read.ipv6_lpm_hits.ulmdb|B|128,000|531|25,458|
|read.ipv6_lpm_hits.ulmdb|T|128,000|521|34,606|
|read.ipv6_lpm_hits.lmdb|P|128,000|1,232|29,075|
|read.ipv6_lpm_hits.lmdb|B|128,000|1,292|53,762|
|read.ipv6_lpm_hits.lmdb|T|128,000|1,293|28,374|
|read.ipv6_lpm_misses.ulmdb|P|128,000|141|41,880|
|read.ipv6_lpm_misses.ulmdb|B|128,000|141|26,841|
|read.ipv6_lpm_misses.ulmdb|T|128,000|141|22,974|
|read.ipv6_lpm_misses.lmdb|P|128,000|1,072|24,968|
|read.ipv6_lpm_misses.lmdb|B|128,000|1,102|71,766|
|read.ipv6_lpm_misses.lmdb|T|128,000|1,103|21,932|
|read.ipv6_lpm_outside_cover_misses.ulmdb|P|64,000|41|6,392|
|read.ipv6_lpm_outside_cover_misses.ulmdb|B|64,000|41|4,719|
|read.ipv6_lpm_outside_cover_misses.ulmdb|T|64,000|41|5,771|
|read.ipv6_lpm_outside_cover_misses.lmdb|P|64,000|50|4,729|
|read.ipv6_lpm_outside_cover_misses.lmdb|B|64,000|50|6,442|
|read.ipv6_lpm_outside_cover_misses.lmdb|T|64,000|41|5,220|
|read.ipv6_lpm_inside_gap_misses.ulmdb|P|64,000|141|4,228|
|read.ipv6_lpm_inside_gap_misses.ulmdb|B|64,000|141|5,400|
|read.ipv6_lpm_inside_gap_misses.ulmdb|T|64,000|141|4,549|
|read.ipv6_lpm_inside_gap_misses.lmdb|P|64,000|1,093|48,683|
|read.ipv6_lpm_inside_gap_misses.lmdb|B|64,000|1,112|161,126|
|read.ipv6_lpm_inside_gap_misses.lmdb|T|64,000|1,122|12,945|
|write.batch1.ulmdb|P|512|12,344|16,391|
|write.batch1.ulmdb|B|512|14,748|17,674|
|write.batch1.ulmdb|T|512|64,953|80,934|
|write.batch1.lmdb|P|512|8,416|12,393|
|write.batch1.lmdb|B|512|8,577|13,305|
|write.batch1.lmdb|T|512|9,488|9,758|
|write.batch64.ulmdb|P|512|257,860|293,127|
|write.batch64.ulmdb|B|512|248,041|270,434|
|write.batch64.ulmdb|T|512|765,295|815,841|
|write.batch64.lmdb|P|512|94,570|100,631|
|write.batch64.lmdb|B|512|97,405|121,711|
|write.batch64.lmdb|T|512|97,385|104,318|

### All four acknowledged tracking-operation rows

Each row is an identical logical journey. Seed includes the acknowledged fixture Write; business deltas include 512 Writes plus 11 Collect preparations. These are operation counts, not CPU time. Attempted/failed/aborted/bootstrap work and empty non-publishing Collect work are unavailable, not zero. Metadata is a per-certificate accounting bound, excluding simultaneous old/pending state, scratch, allocator slack and RSS.

|Process|Seed reads / work|Final reads / work|Business expanded objects|Business checked reads|Business work|Full-audit reads|Peak accounted certificate bytes|
|---|---:|---:|---:|---:|---:|---:|---:|
|bench-r1-s3-T|13,025 / 9,787|31,533 / 102,406|10,022|18,508|92,619|7,304|9,301,752|
|bench-r2-s2-T|13,025 / 9,787|31,533 / 102,406|10,022|18,508|92,619|7,304|9,301,752|
|bench-r3-s1-T|13,025 / 9,787|31,533 / 102,406|10,022|18,508|92,619|7,304|9,301,752|
|bench-r4-s2-T|13,025 / 9,787|31,533 / 102,406|10,022|18,508|92,619|7,304|9,301,752|

Each tracking journey preserves 513 total Writes, 11 Collects, zero ordinary retirement, identical selected-frame payloads/contexts, and 7,304 acknowledged full-audit reads, matching all eight coarse OFF/ON captures. This rules out silently reducing maintenance as an explanation for the comparison. The full generic tracker instead adds substantial ordinary work and memory: fixture load for both engines +80.17%, whole-process post-write RSS +22.41%, and single sampled trial/pooled p99 +429.49%/+372.07%. Whole-process RSS includes both engines; it is not isolated ULMDB memory.

**Subsequent controls are complete below.** The compact 80-byte profile still fails cumulative native perturbation (7.66% combined singles). The narrower five-clock Journal control passes its declared center gate and selects publication for the next investigation. The separate pure hierarchical recipe scanner completed its fixed39 campaign and is HELD for insufficient benefit and repeated read costs. Source-confirmed full-4KiB rescanning in RecipeBuilder::diff is not CPU attribution. Generic tracking remains rejected; no certificate issuer is implemented. Reopen, allocation and sampled-tail goals remain unresolved.

[Coarse receipt](benchmark/work/optimization-20261105/next-retained6-stages/retained6-coarse-v1/analysis-v1/report-receipt.json), [tracking receipt](benchmark/work/optimization-20261105/next-incremental-ownership/tracking-only-v1/controls-v1/analysis-v1/analysis-receipt.json), [tracking all-process metrics](benchmark/work/optimization-20261105/next-incremental-ownership/tracking-only-v1/controls-v1/analysis-v1/all-process-metrics.csv), [ownership design and measured rejection](LMDB_WRITE_ARCHITECTURE.md).

## Latest commit diagnosis: compact gate FAIL; Journal center gate PASS

**Retained 6 is unchanged; neither profiler is an engine promotion.** Both fixed two-smoke/12-full journeys pass every original semantic/source/ELF/native/raw/cold guard. All process observations, adverse cells and sampled maxima remain. Quiet boundaries do not establish continuous host isolation, and a passing center-overhead gate does not establish equivalence.

The compact profile removes all business counters and records 80-byte rows. It still fails the declared 5% gate: native ON/pristine single costs rise 7.48% aggregate / 7.66% combined; ULMDB rises 2.86% / 2.94%. Its instrumented post-cold 40-row single subset reports recipe 0.947us, Journal 6.034us, freeze 0.652us and commit 8.998us. These failed-gate descriptive values support narrowing the measurement, not a production fraction or scanner gain.

The five-clock Journal control passes all predeclared primary center gates: maximum absolute P/O/C change is 2.666754%, with every individual adverse pair retained. It captures only borrowed Resident business Journal intervals; native and owned Collect have zero clocks while all original work stays charged. For 1,024 measured single-write rows, encode/state/ready/publish means are 0.868441/0.193740/0.149989/5.079746us; sum 6.291917us. Publish p95/p99 are 5.651/6.653us and maximum 10.600us. These are adjacent instrumented wall-clock intervals; later intervals include prior boundary-recording bookkeeping.

**Next target: the publication envelope.** The result does not split witness reads/comparisons, fresh capacity query, witness construction/hash, WAL/data fence, witness write/fence and activation, nor establish a kernel-time fraction. Any finer control must keep bytes, ordering, error/quarantine behavior, both fences and all original costs. The hierarchical scanner has no demonstrated qualifying whole-engine gain in its complete39 comparison; generic ownership tracking remains rejected and no certificate issuer is implemented.

### Compact 80-byte timing: complete report

### Compact retained6 commit timing: completed diagnostic

**The predeclared 5% perturbation gate fails.** Native single-commit C/P cost is +7.481% aggregate and +7.659% combined. ULMDB singles remain within5% for all three center comparisons. These stage observations remain descriptive measurements of the instrumented ELF; they are not qualified production phase shares and receive no overhead subtraction.

All two original smokes and twelve fixed P/O/C full RAM processes passed every workload, raw latency, full value, old reader, reopen, source, native, resource and row-bound check. All twelve timed process boundaries observed quiet. No retries, exclusions, calibration runs or engine changes occurred. Pre/post quiet observations do not prove continuous host isolation.

P is the original retained6 ELF; O and C are the same compact ELF with clocks off/on. All1024 native80-byte rows were allocated and touched before fixture creation; operation counters were compiled out. The original timers still charge recorder work. The changed generated transaction/finish code removes the prior bulk row memset/memcpy calls, but this does not establish the cause of either recorder’s perturbation.

#### Full workload rates and costs

Rates and costs are geometric centers of four complete processes. Native rate differences compare ULMDB to LMDB within the same labeled population. Aggregate and combined rates retain all first/later maintenance charges.

| Batch | Measure | Label | ULMDB ns/commit | ULMDB records/s | LMDB ns/commit | LMDB records/s | ULMDB rate vs LMDB | Verdict |
|---|---|---|---:|---:|---:|---:|---:|---|
| 1 | aggregate | P | 15,934.945 | 62,755.157 | 7,412.926 | 134,899.500 | -53.480% | slower |
| 1 | aggregate | O | 16,001.049 | 62,495.902 | 7,607.463 | 131,449.866 | -52.456% | slower |
| 1 | aggregate | C | 16,390.302 | 61,011.688 | 7,967.507 | 125,509.774 | -51.389% | slower |
| 1 | combined | P | 13,234.057 | 75,562.618 | 7,196.710 | 138,952.384 | -45.620% | slower |
| 1 | combined | O | 13,455.555 | 74,318.749 | 7,429.669 | 134,595.498 | -44.784% | slower |
| 1 | combined | C | 13,622.918 | 73,405.710 | 7,747.887 | 129,067.450 | -43.126% | slower |
| 64 | aggregate | P | 65,082.749 | 983,363.498 | 84,919.809 | 753,652.191 | +30.480% | faster |
| 64 | aggregate | O | 65,588.508 | 975,780.689 | 85,449.389 | 748,981.364 | +30.281% | faster |
| 64 | aggregate | C | 65,779.648 | 972,945.313 | 85,286.071 | 750,415.625 | +29.654% | faster |
| 64 | combined | P | 64,514.041 | 992,032.112 | 84,053.002 | 761,424.319 | +30.286% | faster |
| 64 | combined | O | 65,011.115 | 984,447.044 | 84,472.429 | 757,643.656 | +29.935% | faster |
| 64 | combined | C | 65,224.953 | 981,219.559 | 84,456.412 | 757,787.345 | +29.485% | faster |

#### Perturbation gate

Positive changes below are higher cost. All paired process changes remain in METRIC_TABLES.md/CSV and analysis.json; the center gate is not a filter that discards adverse trials.

| Metric | O/P cost | C/O cost | C/P cost | Any center exceeds5% |
|---|---:|---:|---:|---|
| write.batch1.lmdb.aggregate_cost | +2.624% | +4.733% | +7.481% | True |
| write.batch1.lmdb.combined_cost | +3.237% | +4.283% | +7.659% | True |
| write.batch1.ulmdb.aggregate_cost | +0.415% | +2.433% | +2.858% | False |
| write.batch1.ulmdb.combined_cost | +1.674% | +1.244% | +2.938% | False |
| write.batch64.lmdb.aggregate_cost | +0.624% | -0.191% | +0.431% | False |
| write.batch64.lmdb.combined_cost | +0.499% | -0.019% | +0.480% | False |
| write.batch64.ulmdb.aggregate_cost | +0.777% | +0.291% | +1.071% | False |
| write.batch64.ulmdb.combined_cost | +0.770% | +0.329% | +1.102% | False |

#### Instrumented ordinary and maintenance costs

Arithmetic means below are microseconds per instrumented business transaction. Every sample, median, p95, p99, maximum and sum remains in analysis.json. The post-first singles population has only40 rows/backend across four processes. Labels follow observed Resident Collect scopes; counters are compiled out, so this capture does not independently label cache hits or full-audit reads. Native rows are aligned to the corresponding Resident context and do not contain a Native Collect.

| Backend | Batch | Resident context | Rows | Business | Begin | Mutation | Commit | Freeze | Recipe build | Whole Journal | Commit outside three envelopes | Collect inside Begin |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| lmdb | 1 | first_observed_collect | 4 | 7.196 | 0.087 | 0.832 | 6.106 | null | null | null | null | null |
| lmdb | 1 | ordinary_after_first_collect | 40 | 7.429 | 0.105 | 0.919 | 6.231 | null | null | null | null | null |
| lmdb | 1 | ordinary_before_first_collect | 980 | 7.678 | 0.106 | 0.963 | 6.437 | null | null | null | null | null |
| lmdb | 64 | later_observed_collect | 40 | 83.884 | 0.134 | 34.396 | 49.174 | null | null | null | null | null |
| lmdb | 64 | ordinary_after_first_collect | 984 | 84.386 | 0.120 | 34.759 | 49.326 | null | null | null | null | null |
| ulmdb | 1 | first_observed_collect | 4 | 654.768 | 642.893 | 1.057 | 10.620 | 0.719 | 1.054 | 6.723 | 2.124 | 642.637 |
| ulmdb | 1 | ordinary_after_first_collect | 40 | 10.126 | 0.146 | 0.807 | 8.998 | 0.652 | 0.947 | 6.034 | 1.366 | null |
| ulmdb | 1 | ordinary_before_first_collect | 980 | 11.059 | 0.149 | 1.352 | 9.380 | 0.634 | 0.936 | 6.133 | 1.677 | null |
| ulmdb | 64 | later_observed_collect | 40 | 247.176 | 187.750 | 27.165 | 32.052 | 2.498 | 10.743 | 16.992 | 1.819 | 187.513 |
| ulmdb | 64 | ordinary_after_first_collect | 984 | 57.738 | 0.160 | 25.416 | 31.981 | 2.687 | 10.589 | 16.919 | 1.786 | null |

Collect is nested in Begin/Business. Freeze, whole recipe and whole Journal are distinct portions inside Commit. Journal includes encoding, state preparation, ready callback, guards, both fences, transition activation and workspace return; it is not an I/O-only measurement. The remaining Commit duration includes facade/catalogue/activation/release work and must not be called pure preparation.

The selected next diagnostic target is the whole Journal envelope: its descriptive post-first ordinary-single mean is6.034µs, compared with0.947µs for whole recipe construction. This is not an uninstrumented CPU-share claim. A minimal five-boundary Journal-only control is proposed to distinguish encode, state preparation, ready callback and publication while removing all Native and outer phase clocks. No additional profiler or engine change has run, and no gain is predicted from these observations.

#### Complete evidence

- [All354 metric cells, including read/tail/creation/cold/RSS/resource costs](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/analysis-v1/METRIC_TABLES.md), [all process values and paired deltas](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/analysis-v1/METRIC_PAIRS.csv).
- [All120 stratified phase groups](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/analysis-v1/PHASE_TABLES.md), [complete analysis and8192 stage rows](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/analysis-v1/analysis.json).
- [Fixed schedule and all reports](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/controls-v1/summary.json), [sealed source/native/ELF/tools](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/screen-seal-v1.json), [qualified source and build](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/build-qualification-v1-receipt.json).
- [Compiled recorder evidence](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/codegen-v1/FINDINGS.md), [independent source/tool review](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1-independent-review/final-review-v1.json).


Complete adverse-preserving tables: [all 354 metric cells](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/analysis-v1/METRIC_TABLES.md), [every process and pair](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/analysis-v1/METRIC_PAIRS.csv), [all phase strata](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/analysis-v1/PHASE_TABLES.md), [all rows/populations](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/analysis-v1/analysis.json), [analysis receipt](benchmark/work/optimization-20261105/next-retained6-stages/retained6-compact-v1/analysis-v1/report-receipt.json).

### Five-boundary Journal timing: complete report

### Five-boundary retained6 Journal timing

**The predeclared5% perturbation gate passes for all aggregate/combined P/O/C cost centers.** All two original smokes and twelve fixed full RAM processes passed every source/native/raw/full-value/old-reader/reopen/resource/context guard. All twelve timed boundaries observed quiet; no retries, exclusions or extra engine processes occurred. This is a diagnostic control, not a new runtime candidate or accepted performance result.

Native update is byte-identical to pristine and has no diagnostic clock. Owned Collect has no clock and retains its complete original benchmark charge. Each captured Resident borrowed business Journal uses exactly five chained timestamps and four adjacent intervals. OFF has zero diagnostic clocks/null durations. All1024 native80-byte rows were allocated and touched before fixture creation; each full process records512 Resident rows. Pre/post quiet observations are not continuous host isolation.

#### Full workload costs and native comparison

Geometric centers of four processes; all first/later maintenance remains charged. Rates compare ULMDB with LMDB in the same P/O/C population.

| Batch | Measure | Label | ULMDB ns/commit | ULMDB records/s | LMDB ns/commit | LMDB records/s | ULMDB rate vs native | Verdict |
|---|---|---|---:|---:|---:|---:|---:|---|
| 1 | aggregate | P | 15,898.623 | 62,898.530 | 7,374.353 | 135,605.123 | -53.616% | slower |
| 1 | aggregate | O | 16,058.966 | 62,270.512 | 7,498.552 | 133,359.083 | -53.306% | slower |
| 1 | aggregate | C | 16,298.891 | 61,353.866 | 7,506.407 | 133,219.524 | -53.945% | slower |
| 1 | combined | P | 13,179.594 | 75,874.872 | 7,168.521 | 139,498.787 | -45.609% | slower |
| 1 | combined | O | 13,487.197 | 74,144.389 | 7,315.909 | 136,688.422 | -45.757% | slower |
| 1 | combined | C | 13,531.061 | 73,904.034 | 7,317.796 | 136,653.166 | -45.919% | slower |
| 64 | aggregate | P | 64,943.866 | 985,466.436 | 85,445.707 | 749,013.644 | +31.569% | faster |
| 64 | aggregate | O | 65,630.416 | 975,157.615 | 85,226.189 | 750,942.884 | +29.858% | faster |
| 64 | aggregate | C | 66,189.525 | 966,920.365 | 85,403.492 | 749,383.880 | +29.029% | faster |
| 64 | combined | P | 64,321.353 | 995,003.938 | 84,690.084 | 755,696.499 | +31.667% | faster |
| 64 | combined | O | 65,278.569 | 980,413.648 | 84,071.159 | 761,259.872 | +28.788% | faster |
| 64 | combined | C | 65,527.627 | 976,687.280 | 84,581.297 | 756,668.466 | +29.077% | faster |

#### Perturbation gate

Positive changes are higher cost; all individual adverse pairs are retained.

| Metric | O/P cost | C/O cost | C/P cost | Exceeds5% center |
|---|---:|---:|---:|---|
| write.batch1.lmdb.aggregate_cost | +1.684% | +0.105% | +1.791% | False |
| write.batch1.lmdb.combined_cost | +2.056% | +0.026% | +2.082% | False |
| write.batch1.ulmdb.aggregate_cost | +1.009% | +1.494% | +2.518% | False |
| write.batch1.ulmdb.combined_cost | +2.334% | +0.325% | +2.667% | False |
| write.batch64.lmdb.aggregate_cost | -0.257% | +0.208% | -0.049% | False |
| write.batch64.lmdb.combined_cost | -0.731% | +0.607% | -0.128% | False |
| write.batch64.ulmdb.aggregate_cost | +1.057% | +0.852% | +1.918% | False |
| write.batch64.ulmdb.combined_cost | +1.488% | +0.382% | +1.875% | False |

#### Borrowed business Journal intervals

The1024 observations per batch combine four clock-on processes. Mean and quantiles below are nanoseconds. These are measured instrumented wall-clock intervals with a passing overhead bound, not CPU sample shares or subtraction-based estimates. Later intervals include prior boundary-recording/scalar bookkeeping.

| Batch | Interval | Count | Mean ns | Median ns | p95 ns | p99 ns | Maximum ns |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | encode | 1024 | 868.441 | 842.000 | 1,002.000 | 1,112.000 | 3,837.000 |
| 1 | state_prepare | 1024 | 193.740 | 190.000 | 230.000 | 261.000 | 682.000 |
| 1 | ready | 1024 | 149.989 | 141.000 | 181.000 | 210.000 | 711.000 |
| 1 | publish | 1024 | 5,079.746 | 4,900.000 | 5,651.000 | 6,653.000 | 10,600.000 |
| 1 | four_interval_sum | 1024 | 6,291.917 | 6,091.000 | 7,013.000 | 8,125.000 | 13,466.000 |
| 64 | encode | 1024 | 9,505.482 | 9,183.000 | 11,822.000 | 14,297.000 | 35,809.000 |
| 64 | state_prepare | 1024 | 999.684 | 992.000 | 1,102.000 | 1,283.000 | 1,453.000 |
| 64 | ready | 1024 | 198.761 | 190.000 | 240.000 | 270.000 | 5,380.000 |
| 64 | publish | 1024 | 6,503.181 | 6,312.000 | 7,454.000 | 8,336.000 | 12,073.000 |
| 64 | four_interval_sum | 1024 | 17,207.107 | 17,168.000 | 20,569.000 | 23,735.000 | 43,573.000 |

The confirmed next target is `journal_v2::publish`: its single-write mean is5079.746ns out of6291.917ns across the four measured intervals. That function includes two completed witness reads/comparisons, fresh capacity check, witness construction/hash, WAL append/data fence, witness write/fence and transition activation. The present experiment does not separate those operations or establish a syscall/kernel fraction. Full-page recipe diff is outside this measured Journal and is not the next selected target. No engine change is implemented by this diagnostic.

The inner chain starts after encode-workspace take, poison/selected-end/capacity-limit checks and ends before workspace return. Its sum is not the entire commit_borrowed function. Boundary bookkeeping is charged to the following interval.

Canonical encoding means full validation/length/serialization/frame digest. State preparation retains selected-parent/free-generation/Copy-source/catalogue checks and transition construction. Ready retains root checks and Snapshot preparation. Existing guards, error/poison outcomes, durability fences, bytes/formats and recovery behavior are unchanged.

#### Complete evidence

- [All354 read/write/tail/creation/cold/RSS/resource metrics](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/analysis-v1/METRIC_TABLES.md), [every process and paired delta](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/analysis-v1/METRIC_PAIRS.csv).
- [All256 trial/phase/order strata](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/analysis-v1/PHASE_TABLES.md), [all4096 stage rows and complete populations](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/analysis-v1/analysis.json).
- [Fixed schedule and all reports](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/controls-v1/summary.json), [sealed artifacts/source/native/tools](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/screen-seal-v1.json), [focused qualification and build](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/build-qualification-v1-receipt.json).
- The prelaunch availability wait is recorded separately and excluded from child wall time; original workloads were launched once after the barrier cleared.


Complete adverse-preserving tables: [all 354 metric cells](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/analysis-v1/METRIC_TABLES.md), [every process and pair](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/analysis-v1/METRIC_PAIRS.csv), [all phase strata](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/analysis-v1/PHASE_TABLES.md), [all rows/populations](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/analysis-v1/analysis.json), [analysis receipt](benchmark/work/optimization-20261105/next-retained6-stages/retained6-journal-v1/analysis-v1/report-receipt.json).

## Pure hierarchical recipe scanner: HELD after complete39; retained6 remains current

All 39 original processes passed the source/native/ELF/SQLite/WAL/read/write/raw/cold guards; exit1 is a comparison screen result, not engine failure. The scanner gives no material RAM-single gain (-0.378%) and only +0.780% loading against repeated RAM KV reductions of 2.443-2.935%, update -1.313% and adverse disk64 tails. No confirmation reruns or promotion follow this unfavorable primary result. The root runtime and published retained6 scorecard are unchanged.

**Availability is explicit:**38 children observed quiet boundaries. SQLite step028 observed an unrelated qualification cargo/rustc and remains unqualified, shared identically in all captures. Keep its observation without parity inference, exclusion, retry or normalization. Boundary checks do not prove continuous isolation. No other exposure was observed.

Disk64 +12.149% is the geometric change in process-pooled sampled p99. The distinct true global 512-sample p99 changes +11.845%, 379.111 to424.016us, with maximum606.924 to641.791us. Both adverse observations remain; they must not be relabelled as the same statistic.

### Pure hierarchical scanner: HOLD / reject promotion after complete39

**Decision: keep the scanner private and retain optimization baseline6.** All39 processes passed every original semantic/source/native/ELF/SQLite/WAL/raw guard. Exit1 is the comparison screen result, not an engine failure. The candidate has no material RAM-single gain and too little load benefit to offset repeated KV-read and update losses. No confirmation reruns are justified for this unfavorable result, and none were performed. No production runtime change is applied.

All810 workspace tests and focused codec/fault controls passed before the benchmark. Independent review reproduced both237-row comparisons and verified all24 raw files,172,898,304 bytes,11,544 spans and21,612,288 samples, including full coverage, readp50/p95/p99, writep99 and every combined sum. Candidate source is the exact sealed471-file engine snapshot within545 total inputs; initial and accepted source/ELFs stay unchanged. Actual full/million native metadata differs from historical parsed TOML ONLY in the verified archive hash; current returned strings remain strict and original reports are not edited.

**Availability:**38 processes observed quiet boundaries; SQLite step028 observed an unrelated qualification cargo/rustc after child return and is retained as unqualified. It appears identically in all three captures as a shared control. Its report/raw/provenance are preserved; do not infer SQLite parity from it. No other exposure was observed. Boundary checks do not prove continuous host isolation. No retry/exclusion/normalization took place.

#### Principal rates and direct local LMDB comparison

Geometric process centers; rates/s. Within2% of native is a descriptive roughly-on-par label, not equivalence. These candidate results do not replace the retained6 published scorecard.

|Operation|Retained6|Private scanner|Change|LMDB|Candidate versus LMDB|Status|
|---|---:|---:|---:|---:|---:|---|
|millions/disk/ulmdb/load/throughput|1,402,006.815|1,412,943.532|+0.780%|1,518,731.002|-6.966%|slower|
|millions/disk/ulmdb/read/throughput|2,796,750.496|2,801,877.511|+0.183%|1,694,797.402|+65.322%|faster|
|millions/disk/ulmdb/update/throughput|145,908.554|143,992.462|-1.313%|128,986.150|+11.634%|faster|
|full/disk/ulmdb/durable_batch1/throughput|15,088.349|14,820.944|-1.772%|10,449.137|+41.839%|faster|
|full/disk/ulmdb/durable_batch1/combined_throughput|15,674.896|15,729.261|+0.347%|11,183.397|+40.648%|faster|
|full/disk/ulmdb/durable_batch64/throughput|482,773.151|484,486.157|+0.355%|177,520.410|+172.919%|faster|
|full/disk/ulmdb/durable_batch64/combined_throughput|492,580.544|494,716.934|+0.434%|183,640.121|+169.395%|faster|
|full/ram/ulmdb/durable_batch1/throughput|61,049.243|60,818.221|-0.378%|132,434.903|-54.077%|slower|
|full/ram/ulmdb/durable_batch1/combined_throughput|74,352.319|74,689.152|+0.453%|136,708.222|-45.366%|slower|
|full/ram/ulmdb/durable_batch64/throughput|982,042.235|970,126.199|-1.213%|746,873.033|+29.892%|faster|
|full/ram/ulmdb/durable_batch64/combined_throughput|990,345.171|981,568.454|-0.886%|755,350.054|+29.949%|faster|

Load+0.780% combines +1.196%,+1.294% and−0.143% pairs; it still trails native6.966% and needs7.487% more throughput for observed parity. Million updates lose1.313% in all3 pairs. RAM singles change−0.378% aggregate and still trail native54.077%. No large target gain supports accepting the read costs.

#### Every full-suite read throughput change

|Read|Filesystem|6→candidate|Adverse pairs /4|Candidate/s|LMDB/s|Candidate versus native|
|---|---|---:|---:|---:|---:|---:|
|ipv4_lpm_hits/throughput|disk|-0.243%|3|3,190,805.366|2,122,588.266|+50.326%|
|ipv4_lpm_inside_gap_misses/throughput|disk|-0.393%|2|11,556,000.802|1,991,091.763|+480.385%|
|ipv4_lpm_misses/throughput|disk|-1.779%|3|21,396,797.164|3,923,508.988|+445.348%|
|ipv4_lpm_mixed/throughput|disk|-0.713%|3|5,496,726.697|2,675,061.372|+105.480%|
|ipv4_lpm_outside_cover_misses/throughput|disk|-0.059%|3|99,921,198.213|94,865,349.488|+5.329%|
|ipv6_lpm_hits/throughput|disk|+0.374%|0|2,474,277.856|1,248,170.365|+98.232%|
|ipv6_lpm_inside_gap_misses/throughput|disk|+0.168%|2|9,377,067.593|1,132,601.984|+727.923%|
|ipv6_lpm_misses/throughput|disk|+1.495%|0|17,355,330.634|2,232,854.975|+677.271%|
|ipv6_lpm_mixed/throughput|disk|+0.391%|1|4,300,935.031|1,570,901.809|+173.788%|
|ipv6_lpm_outside_cover_misses/throughput|disk|+0.145%|2|97,412,680.889|95,141,815.086|+2.387%|
|ordered_kv_hits/throughput|disk|-1.813%|4|7,447,323.373|4,197,283.230|+77.432%|
|ordered_kv_misses/throughput|disk|-1.130%|2|7,530,108.322|4,025,747.725|+87.049%|
|ordered_kv_mixed/throughput|disk|-1.271%|3|7,152,980.121|4,105,707.568|+74.220%|
|ipv4_lpm_hits/throughput|ram|+0.319%|1|3,199,124.026|2,107,248.304|+51.815%|
|ipv4_lpm_inside_gap_misses/throughput|ram|+0.005%|1|11,558,020.364|1,918,560.919|+502.432%|
|ipv4_lpm_misses/throughput|ram|-0.430%|2|21,598,862.523|3,740,817.968|+477.383%|
|ipv4_lpm_mixed/throughput|ram|+1.200%|0|5,515,018.979|2,611,966.002|+111.144%|
|ipv4_lpm_outside_cover_misses/throughput|ram|+2.919%|2|100,015,050.225|92,382,892.290|+8.261%|
|ipv6_lpm_hits/throughput|ram|+0.620%|1|2,482,573.754|1,234,924.988|+101.030%|
|ipv6_lpm_inside_gap_misses/throughput|ram|+0.132%|2|9,372,817.447|1,116,721.894|+739.315%|
|ipv6_lpm_misses/throughput|ram|+1.896%|0|17,374,931.709|2,209,095.131|+686.518%|
|ipv6_lpm_mixed/throughput|ram|+0.018%|2|4,303,934.651|1,541,977.477|+179.118%|
|ipv6_lpm_outside_cover_misses/throughput|ram|+0.644%|2|97,443,066.610|91,533,214.262|+6.457%|
|ordered_kv_hits/throughput|ram|-2.935%|4|7,526,620.656|4,220,345.142|+78.341%|
|ordered_kv_misses/throughput|ram|-2.443%|4|7,583,333.538|4,024,150.053|+88.446%|
|ordered_kv_mixed/throughput|ram|-2.469%|4|7,232,832.662|4,112,240.056|+75.885%|

RAM ordered KV hits/misses/mixed lose2.935%/2.443%/2.469%, adverse in all4 pairs each. Disk KV hits lose1.813% in all4 pairs; disk misses/mixed lose1.130%/1.271%. The broad adjusted intervals remain inconclusive; that does not demonstrate equivalence or erase the repeated adverse point estimates. No code-layout or I/O cause is proven.

#### Lifecycle and allocation gaps remain

|Metric|Retained6|Candidate|Change|Local LMDB|Candidate/native cost|
|---|---:|---:|---:|---:|---:|
|setup ms|406.330|401.911|-1.088%|1.321|304.325x|
|post-update reopen ms|7,094.077|7,074.479|-0.276%|3.778|1,872.520x|
|allocated MiB|256.003|256.000|-0.001%|33.044|7.747x|

#### All sampled write tails

Each original write trial has8 separately sampled commits; aggregate commits have no per-commit samples. Trial-p99 and the geometric mean of process-pooledp99 differ from a single global pooledp99. All three and maxima remain below; no production-percentile guarantee follows.

|Filesystem/batch|Trialp99 change|Process-pooledp99 change|True global512-samplep99 6→C ns|Global change|Sampled maximum6→C ns|
|---|---:|---:|---:|---:|---:|
|disk/1|-2.960%|-2.056%|315,610→312,575|-0.962%|405,902→350,436|
|disk/64|+1.410%|+12.149%|379,111→424,016|+11.845%|606,924→641,791|
|ram/1|-4.016%|-19.365%|15,610→11,893|-23.812%|35,698→16,111|
|ram/64|+0.914%|+0.830%|257,149→258,091|+0.366%|277,327→288,679|

Disk64 process-pooledp99 rises12.149%; the distinct actual globalp99 rises11.845% (379.111→424.016us), with maximum606.924→641.791us. RAM-single tails improve, including globalp99−23.812%, but no single-write throughput improvement or material load benefit is established. All native tail pools and every process/round/maximum/offset remain in global-write-tails.json and raw-review.json.

#### Complete evidence

- [Every237 metric row versus initial and retained6](benchmark/work/optimization-20261105/next-hierarchical-diff/retained6-v1/campaign-independent-review-v1/FULL_TABLES.md).
- [All103 target/native/gap rows](benchmark/work/optimization-20261105/next-hierarchical-diff/retained6-v1/campaign-independent-review-v1/NATIVE_SCORECARD.md) and [structured native scorecard](benchmark/work/optimization-20261105/next-hierarchical-diff/retained6-v1/campaign-independent-review-v1/native-scorecard.json).
- [All original raw values and write-tail locations](benchmark/work/optimization-20261105/next-hierarchical-diff/retained6-v1/campaign-independent-review-v1/raw-review.json), [global and per-process tails](benchmark/work/optimization-20261105/next-hierarchical-diff/retained6-v1/campaign-independent-review-v1/global-write-tails.json), [exact availability context](benchmark/work/optimization-20261105/next-hierarchical-diff/retained6-v1/campaign-independent-review-v1/availability-review.json).
- [Unchanged initial comparison](benchmark/work/optimization-20261105/next-hierarchical-diff/retained6-v1/campaign-v1/initial-vs-candidate.md) and [unchanged retained6 comparison](benchmark/work/optimization-20261105/next-hierarchical-diff/retained6-v1/campaign-v1/accepted-vs-candidate.md) retain all observations, flags, uncertainty and unavailable cells.
- [Sealed qualification/source/native inputs](benchmark/work/optimization-20261105/next-hierarchical-diff/retained6-v1/campaign-inputs-v1.json), [fixed39 manifest](benchmark/work/optimization-20261105/next-hierarchical-diff/retained6-v1/campaign-v1/campaign.json) and [review receipt](benchmark/work/optimization-20261105/next-hierarchical-diff/retained6-v1/campaign-independent-review-v1/review-receipt.json).

Next work remains the independently measured publication envelope and separately reviewed architecture. Do not splice this private scanner into the retained6 version or claim that byte-equivalent recipe scanning removes the confirmed publication cost.


### Private hierarchical scanner: all103 target metrics directly versus local LMDB

Candidate is HELD; retained6 remains current. Native advantage is signed positively for higher throughput or lower time/space. Roughly on par is an observed within2% label, not equivalence. Gap is required throughput increase or cost reduction at these centers. All39 observations remain, including the separately labelled exposed SQLite control; local LMDB boundaries were observed quiet. Unavailable full-lifecycle native metrics stay unmeasured.

|Metric|Unit|Retained6|Candidate|Change versus6|Local LMDB|Native advantage|Status|Required gap|
|---|---|---:|---:|---:|---:|---:|---|---:|
|full/disk/ulmdb/durable_batch1/combined_throughput|records/s|15,674.896|15,729.261|+0.347%|11,183.397|+40.65%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99|ns|262,528.434|257,131.136|-2.056%|326,251.678|+21.19%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/sampled_p99|ns|89,228.405|86,587.415|-2.960%|189,928.797|+54.41%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/throughput|records/s|15,088.349|14,820.944|-1.772%|10,449.137|+41.84%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/combined_throughput|records/s|492,580.544|494,716.934|+0.434%|183,640.121|+169.39%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99|ns|384,796.637|431,543.978|+12.149%|519,092.299|+16.87%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/sampled_p99|ns|218,074.621|221,149.968|+1.410%|457,394.578|+51.65%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/throughput|records/s|482,773.151|484,486.157|+0.355%|177,520.410|+172.92%|faster / better|0.00%|
|full/disk/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|67,109,887.977|+0.002%|729,088.000|-9104.63%|slower / worse|98.91%|
|full/disk/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.000%|733,184.000|-9053.07%|slower / worse|98.91%|
|full/disk/ulmdb/footprint/after/files|files|2.000|2.000|+0.000%|2.000|-0.00%|roughly on par|0.00%|
|full/disk/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|67,109,887.977|+0.002%|352,256.000|-18951.45%|slower / worse|99.48%|
|full/disk/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.000%|356,352.000|-18732.18%|slower / worse|99.47%|
|full/disk/ulmdb/footprint/before/files|files|2.000|2.000|+0.000%|2.000|-0.00%|roughly on par|0.00%|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99|ns|427.371|434.386|+1.641%|785.950|+44.73%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,198,572.690|3,190,805.366|-0.243%|2,122,588.266|+50.33%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|121.385|124.633|+2.676%|607.625|+79.49%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,601,546.031|11,556,000.802|-0.393%|1,991,091.763|+480.39%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99|ns|120.750|124.716|+3.285%|608.728|+79.51%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,784,315.778|21,396,797.164|-1.779%|3,923,508.988|+445.35%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|423.376|427.376|+0.945%|778.807|+45.12%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,536,195.694|5,496,726.697|-0.713%|2,675,061.372|+105.48%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.140|41.822|+1.657%|47.935|+12.75%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|99,980,258.037|99,921,198.213|-0.059%|94,865,349.488|+5.33%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99|ns|528.737|524.303|-0.839%|1,233.624|+57.50%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,465,062.062|2,474,277.856|+0.374%|1,248,170.365|+98.23%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|144.460|141.853|-1.805%|1,094.189|+87.04%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,361,314.063|9,377,067.593|+0.168%|1,132,601.984|+727.92%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99|ns|145.096|141.413|-2.539%|1,060.169|+86.66%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,099,670.905|17,355,330.634|+1.495%|2,232,854.975|+677.27%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|522.714|518.919|-0.726%|1,234.997|+57.98%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,284,185.357|4,300,935.031|+0.391%|1,570,901.809|+173.79%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.000|41.140|+0.342%|44.332|+7.20%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|97,271,185.074|97,412,680.889|+0.145%|95,141,815.086|+2.39%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99|ns|201.078|202.254|+0.585%|335.186|+39.66%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/throughput|queries/s|7,584,847.235|7,447,323.373|-1.813%|4,197,283.230|+77.43%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99|ns|202.705|201.909|-0.393%|346.065|+41.66%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/throughput|queries/s|7,616,139.043|7,530,108.322|-1.130%|4,025,747.725|+87.05%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99|ns|205.637|206.813|+0.572%|345.246|+40.10%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/throughput|queries/s|7,245,067.595|7,152,980.121|-1.271%|4,105,707.568|+74.22%|faster / better|0.00%|
|full/disk/ulmdb/reopen/complete_cold_check_ns|ns|96,049,066.765|96,537,992.040|+0.509%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/environment_drop_ns|ns|95,482.712|100,604.969|+5.365%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/experimental_reenable_ns|ns|2,233.660|2,161.053|-3.251%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/open_ns|ns|85,409,290.589|85,989,296.878|+0.679%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/validation_ns|ns|6,233,243.827|6,199,502.798|-0.541%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/setup/create|ns|106,398,966.313|103,371,214.314|-2.846%|210,759.466|-48947.01%|slower / worse|99.80%|
|full/ram/ulmdb/durable_batch1/combined_throughput|records/s|74,352.319|74,689.152|+0.453%|136,708.222|-45.37%|slower / worse|83.04%|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99|ns|14,826.842|11,955.620|-19.365%|9,377.878|-27.49%|slower / worse|21.56%|
|full/ram/ulmdb/durable_batch1/sampled_p99|ns|11,783.646|11,310.416|-4.016%|8,231.024|-37.41%|slower / worse|27.23%|
|full/ram/ulmdb/durable_batch1/throughput|records/s|61,049.243|60,818.221|-0.378%|132,434.903|-54.08%|slower / worse|117.76%|
|full/ram/ulmdb/durable_batch64/combined_throughput|records/s|990,345.171|981,568.454|-0.886%|755,350.054|+29.95%|faster / better|0.00%|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99|ns|256,230.877|258,358.250|+0.830%|97,766.109|-164.26%|slower / worse|62.16%|
|full/ram/ulmdb/durable_batch64/sampled_p99|ns|102,046.646|102,978.929|+0.914%|91,795.601|-12.18%|slower / worse|10.86%|
|full/ram/ulmdb/durable_batch64/throughput|records/s|982,042.235|970,126.199|-1.213%|746,873.033|+29.89%|faster / better|0.00%|
|full/ram/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.000%|729,088.000|-9104.49%|slower / worse|98.91%|
|full/ram/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.000%|733,184.000|-9053.07%|slower / worse|98.91%|
|full/ram/ulmdb/footprint/after/files|files|2.000|2.000|+0.000%|2.000|-0.00%|roughly on par|0.00%|
|full/ram/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.000%|352,256.000|-18951.16%|slower / worse|99.48%|
|full/ram/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.000%|356,352.000|-18732.18%|slower / worse|99.47%|
|full/ram/ulmdb/footprint/before/files|files|2.000|2.000|+0.000%|2.000|-0.00%|roughly on par|0.00%|
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99|ns|432.237|431.917|-0.074%|785.526|+45.02%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,188,938.204|3,199,124.026|+0.319%|2,107,248.304|+51.82%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|122.147|121.717|-0.351%|652.985|+81.36%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,557,392.491|11,558,020.364|+0.005%|1,918,560.919|+502.43%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99|ns|120.561|122.407|+1.531%|641.844|+80.93%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,692,072.521|21,598,862.523|-0.430%|3,740,817.968|+477.38%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|418.871|424.309|+1.298%|766.050|+44.61%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,449,629.798|5,515,018.979|+1.200%|2,611,966.002|+111.14%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.422|41.383|-0.093%|45.291|+8.63%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|97,178,182.324|100,015,050.225|+2.919%|92,382,892.290|+8.26%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99|ns|535.517|525.100|-1.945%|1,263.108|+58.43%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,467,266.774|2,482,573.754|+0.620%|1,234,924.988|+101.03%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|142.599|143.699|+0.772%|1,102.636|+86.97%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,360,464.427|9,372,817.447|+0.132%|1,116,721.894|+739.32%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99|ns|141.966|143.099|+0.798%|1,086.819|+86.83%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,051,676.139|17,374,931.709|+1.896%|2,209,095.131|+686.52%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|523.750|517.054|-1.279%|1,259.899|+58.96%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,303,175.891|4,303,934.651|+0.018%|1,541,977.477|+179.12%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.127|41.140|+0.031%|43.650|+5.75%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|96,819,420.303|97,443,066.610|+0.644%|91,533,214.262|+6.46%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99|ns|196.564|200.377|+1.940%|335.576|+40.29%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/throughput|queries/s|7,754,180.701|7,526,620.656|-2.935%|4,220,345.142|+78.34%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99|ns|195.611|201.120|+2.816%|342.728|+41.32%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/throughput|queries/s|7,773,238.284|7,583,333.538|-2.443%|4,024,150.053|+88.45%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99|ns|201.198|205.983|+2.379%|342.365|+39.84%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/throughput|queries/s|7,415,940.056|7,232,832.662|-2.469%|4,112,240.056|+75.89%|faster / better|0.00%|
|full/ram/ulmdb/reopen/complete_cold_check_ns|ns|53,438,266.315|53,306,886.723|-0.246%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/environment_drop_ns|ns|90,929.771|93,936.959|+3.307%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/experimental_reenable_ns|ns|2,220.056|2,321.678|+4.577%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/open_ns|ns|42,879,036.426|42,735,889.644|-0.334%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/validation_ns|ns|6,262,227.377|6,254,722.025|-0.120%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/setup/create|ns|48,456,582.750|48,327,696.034|-0.266%|126,931.749|-37973.77%|slower / worse|99.74%|
|millions/disk/ulmdb/close_reopen_ns|ns|7,094,076,603.298|7,074,479,085.096|-0.276%|3,778,052.171|-187152.02%|slower / worse|99.95%|
|millions/disk/ulmdb/load/throughput|operations/s|1,402,006.815|1,412,943.532|+0.780%|1,518,731.002|-6.97%|slower / worse|7.49%|
|millions/disk/ulmdb/read/throughput|operations/s|2,796,750.496|2,801,877.511|+0.183%|1,694,797.402|+65.32%|faster / better|0.00%|
|millions/disk/ulmdb/setup_ns|ns|406,329,717.213|401,910,848.317|-1.088%|1,320,664.112|-30332.48%|slower / worse|99.67%|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|bytes|268,438,186.660|268,435,456.000|-0.001%|34,649,429.280|-674.72%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_after_close_reopen/apparent_bytes|bytes|268,435,456.000|268,435,456.000|+0.000%|34,652,160.000|-674.66%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_after_close_reopen/files|files|2.000|2.000|+0.000%|2.000|-0.00%|roughly on par|0.00%|
|millions/disk/ulmdb/storage_before_close/allocated_bytes|bytes|268,438,186.660|268,435,456.000|-0.001%|34,649,429.280|-674.72%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_before_close/apparent_bytes|bytes|268,435,456.000|268,435,456.000|+0.000%|34,652,160.000|-674.66%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_before_close/files|files|2.000|2.000|+0.000%|2.000|-0.00%|roughly on par|0.00%|
|millions/disk/ulmdb/update/throughput|operations/s|145,908.554|143,992.462|-1.313%|128,986.150|+11.63%|faster / better|0.00%|


### Complete hierarchical-scanner standard39 tables

All original observations remain, including every adverse cell. Four full process rounds/filesystem and three ordinary-million rounds; source/native/SQLite/raw/oracles checked. Contemporary million LMDB/SQLite controls are explicitly shared across arms. No native normalization or exclusions.

Actual processes39; observed quiet38; exposed1. Boundary checks do not prove continuous isolation. Exposed tags and identities are in availability-review.json. No SQLite parity claim follows from an exposed point.

Rates/cost centers are geometric means of process metrics. Positive native advantage means more throughput or less cost. Within2% is a descriptive roughly-on-par label, not equivalence; remaining gap is required throughput increase or cost reduction to match native at the observed centers.

#### Candidate versus initial: every metric

|Metric|Unit|Reference|Candidate|Candidate change|Local LMDB|Native advantage|Status|Required increase / cost reduction|
|---|---|---:|---:|---:|---:|---:|---|---:|
|full/disk/harness/setup/after_setup_ns|ns|1,616.106|1,750.338|+8.31%|unavailable|unavailable|not measured|unavailable|
|full/disk/harness/setup/dataset_ns|ns|236,546,674.548|235,069,365.989|-0.62%|unavailable|unavailable|not measured|unavailable|
|full/disk/harness/setup/fixture_load_both_ns|ns|19,826,033.695|10,200,099.663|-48.55%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/combined_throughput|records/s|11,518.760|11,183.397|-2.91%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/pooled_sampled_p99|ns|323,887.358|326,251.678|+0.73%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/sampled_p99|ns|190,701.992|189,928.797|-0.41%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/throughput|records/s|11,043.692|10,449.137|-5.38%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/combined_throughput|records/s|182,643.450|183,640.121|+0.55%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/pooled_sampled_p99|ns|525,474.857|519,092.299|-1.21%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/sampled_p99|ns|455,119.362|457,394.578|+0.50%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/throughput|records/s|176,704.966|177,520.410|+0.46%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/allocated_file_bytes|bytes|729,088.000|729,088.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/apparent_file_bytes|bytes|733,184.000|733,184.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_pss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_rss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/allocated_file_bytes|bytes|352,256.000|352,256.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/apparent_file_bytes|bytes|356,352.000|356,352.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_pss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_rss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_hits/sampled_p99|ns|780.120|785.950|+0.75%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_hits/throughput|queries/s|2,138,471.369|2,122,588.266|-0.74%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|630.864|607.625|-3.68%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|1,983,006.429|1,991,091.763|+0.41%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_misses/sampled_p99|ns|623.874|608.728|-2.43%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_misses/throughput|queries/s|3,861,805.287|3,923,508.988|+1.60%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_mixed/sampled_p99|ns|764.101|778.807|+1.92%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_mixed/throughput|queries/s|2,655,734.026|2,675,061.372|+0.73%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|45.524|47.935|+5.30%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|90,385,539.135|94,865,349.488|+4.96%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_hits/sampled_p99|ns|1,286.877|1,233.624|-4.14%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_hits/throughput|queries/s|1,220,775.733|1,248,170.365|+2.24%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|1,110.982|1,094.189|-1.51%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|1,129,427.928|1,132,601.984|+0.28%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_misses/sampled_p99|ns|1,079.979|1,060.169|-1.83%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_misses/throughput|queries/s|2,224,519.759|2,232,854.975|+0.37%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_mixed/sampled_p99|ns|1,266.535|1,234.997|-2.49%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_mixed/throughput|queries/s|1,549,119.466|1,570,901.809|+1.41%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|42.992|44.332|+3.12%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|90,477,290.846|95,141,815.086|+5.16%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_hits/sampled_p99|ns|339.965|335.186|-1.41%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_hits/throughput|queries/s|4,169,301.669|4,197,283.230|+0.67%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_misses/sampled_p99|ns|341.499|346.065|+1.34%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_misses/throughput|queries/s|4,027,367.347|4,025,747.725|-0.04%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_mixed/sampled_p99|ns|344.557|345.246|+0.20%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_mixed/throughput|queries/s|4,109,234.055|4,105,707.568|-0.09%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/setup/create|ns|210,505.412|210,759.466|+0.12%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/durable_batch1/combined_throughput|records/s|16,203.955|15,729.261|-2.93%|11,183.397|+40.65%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99|ns|313,160.462|257,131.136|-17.89%|326,251.678|+21.19%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/sampled_p99|ns|84,632.678|86,587.415|+2.31%|189,928.797|+54.41%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/throughput|records/s|15,586.283|14,820.944|-4.91%|10,449.137|+41.84%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/combined_throughput|records/s|476,543.281|494,716.934|+3.81%|183,640.121|+169.39%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99|ns|488,936.838|431,543.978|-11.74%|519,092.299|+16.87%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/sampled_p99|ns|251,535.939|221,149.968|-12.08%|457,394.578|+51.65%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/throughput|records/s|472,862.879|484,486.157|+2.46%|177,520.410|+172.92%|faster / better|0.00%|
|full/disk/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|67,109,887.977|+0.00%|729,088.000|-9104.63%|slower / worse|98.91%|
|full/disk/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|733,184.000|-9053.07%|slower / worse|98.91%|
|full/disk/ulmdb/footprint/after/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/disk/ulmdb/footprint/after/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/after/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/after/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|67,109,887.977|+0.00%|352,256.000|-18951.45%|slower / worse|99.48%|
|full/disk/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|356,352.000|-18732.18%|slower / worse|99.47%|
|full/disk/ulmdb/footprint/before/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/disk/ulmdb/footprint/before/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99|ns|433.698|434.386|+0.16%|785.950|+44.73%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,186,693.966|3,190,805.366|+0.13%|2,122,588.266|+50.33%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|118.587|124.633|+5.10%|607.625|+79.49%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,414,800.286|11,556,000.802|+1.24%|1,991,091.763|+480.39%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99|ns|120.289|124.716|+3.68%|608.728|+79.51%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,538,110.758|21,396,797.164|-0.66%|3,923,508.988|+445.35%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|423.481|427.376|+0.92%|778.807|+45.12%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,489,212.932|5,496,726.697|+0.14%|2,675,061.372|+105.48%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.563|41.822|+0.62%|47.935|+12.75%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|98,563,159.174|99,921,198.213|+1.38%|94,865,349.488|+5.33%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99|ns|528.821|524.303|-0.85%|1,233.624|+57.50%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,499,155.653|2,474,277.856|-1.00%|1,248,170.365|+98.23%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|142.979|141.853|-0.79%|1,094.189|+87.04%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,148,007.341|9,377,067.593|+2.50%|1,132,601.984|+727.92%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99|ns|143.141|141.413|-1.21%|1,060.169|+86.66%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/throughput|queries/s|16,973,385.870|17,355,330.634|+2.25%|2,232,854.975|+677.27%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|517.160|518.919|+0.34%|1,234.997|+57.98%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,340,015.056|4,300,935.031|-0.90%|1,570,901.809|+173.79%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.140|41.140|+0.00%|44.332|+7.20%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|96,955,772.719|97,412,680.889|+0.47%|95,141,815.086|+2.39%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99|ns|232.892|202.254|-13.16%|335.186|+39.66%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/throughput|queries/s|6,386,881.664|7,447,323.373|+16.60%|4,197,283.230|+77.43%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99|ns|233.530|201.909|-13.54%|346.065|+41.66%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/throughput|queries/s|6,340,287.919|7,530,108.322|+18.77%|4,025,747.725|+87.05%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99|ns|241.186|206.813|-14.25%|345.246|+40.10%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/throughput|queries/s|6,089,385.759|7,152,980.121|+17.47%|4,105,707.568|+74.22%|faster / better|0.00%|
|full/disk/ulmdb/reopen/complete_cold_check_ns|ns|98,085,133.580|96,537,992.040|-1.58%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/environment_drop_ns|ns|94,644.649|100,604.969|+6.30%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/experimental_reenable_ns|ns|2,086.411|2,161.053|+3.58%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/open_ns|ns|87,194,262.429|85,989,296.878|-1.38%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/validation_ns|ns|6,589,028.450|6,199,502.798|-5.91%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/setup/create|ns|105,326,035.678|103,371,214.314|-1.86%|210,759.466|-48947.01%|slower / worse|99.80%|
|full/ram/harness/setup/after_setup_ns|ns|1,726.969|4,281.379|+147.91%|unavailable|unavailable|not measured|unavailable|
|full/ram/harness/setup/dataset_ns|ns|235,179,720.598|232,534,638.392|-1.12%|unavailable|unavailable|not measured|unavailable|
|full/ram/harness/setup/fixture_load_both_ns|ns|18,476,764.227|9,128,266.016|-50.60%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/combined_throughput|records/s|137,360.222|136,708.222|-0.47%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/pooled_sampled_p99|ns|8,626.176|9,377.878|+8.71%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/sampled_p99|ns|8,097.045|8,231.024|+1.65%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/throughput|records/s|132,483.950|132,434.903|-0.04%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/combined_throughput|records/s|751,820.840|755,350.054|+0.47%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/pooled_sampled_p99|ns|101,474.077|97,766.109|-3.65%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/sampled_p99|ns|92,587.480|91,795.601|-0.86%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/throughput|records/s|741,764.381|746,873.033|+0.69%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/allocated_file_bytes|bytes|729,088.000|729,088.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/apparent_file_bytes|bytes|733,184.000|733,184.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_pss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_rss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/allocated_file_bytes|bytes|352,256.000|352,256.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/apparent_file_bytes|bytes|356,352.000|356,352.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_pss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_rss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_hits/sampled_p99|ns|778.897|785.526|+0.85%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_hits/throughput|queries/s|2,150,066.547|2,107,248.304|-1.99%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|603.018|652.985|+8.29%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|2,055,311.498|1,918,560.919|-6.65%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_misses/sampled_p99|ns|598.466|641.844|+7.25%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_misses/throughput|queries/s|3,926,940.037|3,740,817.968|-4.74%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_mixed/sampled_p99|ns|767.133|766.050|-0.14%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_mixed/throughput|queries/s|2,694,057.103|2,611,966.002|-3.05%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|47.580|45.291|-4.81%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|95,617,828.506|92,382,892.290|-3.38%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_hits/sampled_p99|ns|1,251.118|1,263.108|+0.96%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_hits/throughput|queries/s|1,249,129.602|1,234,924.988|-1.14%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|1,113.308|1,102.636|-0.96%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|1,143,832.422|1,116,721.894|-2.37%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_misses/sampled_p99|ns|1,074.524|1,086.819|+1.14%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_misses/throughput|queries/s|2,262,498.099|2,209,095.131|-2.36%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_mixed/sampled_p99|ns|1,245.914|1,259.899|+1.12%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_mixed/throughput|queries/s|1,578,420.110|1,541,977.477|-2.31%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|44.760|43.650|-2.48%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|94,662,053.077|91,533,214.262|-3.31%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_hits/sampled_p99|ns|338.761|335.576|-0.94%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_hits/throughput|queries/s|4,176,330.333|4,220,345.142|+1.05%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_misses/sampled_p99|ns|344.617|342.728|-0.55%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_misses/throughput|queries/s|3,998,938.227|4,024,150.053|+0.63%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_mixed/sampled_p99|ns|341.319|342.365|+0.31%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_mixed/throughput|queries/s|4,080,556.946|4,112,240.056|+0.78%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/setup/create|ns|141,318.595|126,931.749|-10.18%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/durable_batch1/combined_throughput|records/s|68,305.175|74,689.152|+9.35%|136,708.222|-45.37%|slower / worse|83.04%|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99|ns|14,619.935|11,955.620|-18.22%|9,377.878|-27.49%|slower / worse|21.56%|
|full/ram/ulmdb/durable_batch1/sampled_p99|ns|12,640.261|11,310.416|-10.52%|8,231.024|-37.41%|slower / worse|27.23%|
|full/ram/ulmdb/durable_batch1/throughput|records/s|56,146.313|60,818.221|+8.32%|132,434.903|-54.08%|slower / worse|117.76%|
|full/ram/ulmdb/durable_batch64/combined_throughput|records/s|926,752.356|981,568.454|+5.91%|755,350.054|+29.95%|faster / better|0.00%|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99|ns|320,085.223|258,358.250|-19.28%|97,766.109|-164.26%|slower / worse|62.16%|
|full/ram/ulmdb/durable_batch64/sampled_p99|ns|114,461.808|102,978.929|-10.03%|91,795.601|-12.18%|slower / worse|10.86%|
|full/ram/ulmdb/durable_batch64/throughput|records/s|924,528.673|970,126.199|+4.93%|746,873.033|+29.89%|faster / better|0.00%|
|full/ram/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|729,088.000|-9104.49%|slower / worse|98.91%|
|full/ram/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|733,184.000|-9053.07%|slower / worse|98.91%|
|full/ram/ulmdb/footprint/after/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/ram/ulmdb/footprint/after/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/after/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/after/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|352,256.000|-18951.16%|slower / worse|99.48%|
|full/ram/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|356,352.000|-18732.18%|slower / worse|99.47%|
|full/ram/ulmdb/footprint/before/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/ram/ulmdb/footprint/before/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99|ns|435.823|431.917|-0.90%|785.526|+45.02%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,179,220.306|3,199,124.026|+0.63%|2,107,248.304|+51.82%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|119.829|121.717|+1.58%|652.985|+81.36%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,470,760.014|11,558,020.364|+0.76%|1,918,560.919|+502.43%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99|ns|119.390|122.407|+2.53%|641.844|+80.93%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,624,970.829|21,598,862.523|-0.12%|3,740,817.968|+477.38%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|427.939|424.309|-0.85%|766.050|+44.61%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,466,325.818|5,515,018.979|+0.89%|2,611,966.002|+111.14%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.000|41.383|+0.93%|45.291|+8.63%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|99,928,661.186|100,015,050.225|+0.09%|92,382,892.290|+8.26%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99|ns|526.004|525.100|-0.17%|1,263.108|+58.43%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,511,770.282|2,482,573.754|-1.16%|1,234,924.988|+101.03%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|141.426|143.699|+1.61%|1,102.636|+86.97%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,172,439.272|9,372,817.447|+2.18%|1,116,721.894|+739.32%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99|ns|141.426|143.099|+1.18%|1,086.819|+86.83%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,071,495.529|17,374,931.709|+1.78%|2,209,095.131|+686.52%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|519.976|517.054|-0.56%|1,259.899|+58.96%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,359,996.637|4,303,934.651|-1.29%|1,541,977.477|+179.12%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.809|41.140|-1.60%|43.650|+5.75%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|96,644,084.285|97,443,066.610|+0.83%|91,533,214.262|+6.46%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99|ns|233.397|200.377|-14.15%|335.576|+40.29%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/throughput|queries/s|6,393,889.328|7,526,620.656|+17.72%|4,220,345.142|+78.34%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99|ns|232.501|201.120|-13.50%|342.728|+41.32%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/throughput|queries/s|6,400,904.199|7,583,333.538|+18.47%|4,024,150.053|+88.45%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99|ns|243.649|205.983|-15.46%|342.365|+39.84%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/throughput|queries/s|6,079,175.198|7,232,832.662|+18.98%|4,112,240.056|+75.89%|faster / better|0.00%|
|full/ram/ulmdb/reopen/complete_cold_check_ns|ns|55,023,907.700|53,306,886.723|-3.12%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/environment_drop_ns|ns|89,908.187|93,936.959|+4.48%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/experimental_reenable_ns|ns|2,100.421|2,321.678|+10.53%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/open_ns|ns|44,019,657.103|42,735,889.644|-2.92%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/validation_ns|ns|6,633,324.109|6,254,722.025|-5.71%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/setup/create|ns|48,476,733.087|48,327,696.034|-0.31%|126,931.749|-37973.77%|slower / worse|99.74%|
|millions/disk/lmdb/close_reopen_ns|ns|3,778,052.171|3,778,052.171|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/load/throughput|operations/s|1,518,731.002|1,518,731.002|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/read/throughput|operations/s|1,694,797.402|1,694,797.402|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/setup_ns|ns|1,320,664.112|1,320,664.112|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/allocated_bytes|bytes|34,649,429.280|34,649,429.280|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/apparent_bytes|bytes|34,652,160.000|34,652,160.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/allocated_bytes|bytes|34,649,429.280|34,649,429.280|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/apparent_bytes|bytes|34,652,160.000|34,652,160.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/update/throughput|operations/s|128,986.150|128,986.150|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/close_reopen_ns|ns|9,330,639.735|9,330,639.735|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/load/throughput|operations/s|820,508.069|820,508.069|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/read/throughput|operations/s|461,905.377|461,905.377|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/setup_ns|ns|2,702,740.552|2,702,740.552|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/allocated_bytes|bytes|25,317,376.000|25,317,376.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/apparent_bytes|bytes|25,317,376.000|25,317,376.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/files|files|1.000|1.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/allocated_bytes|bytes|33,103,872.000|33,103,872.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/apparent_bytes|bytes|33,099,896.000|33,099,896.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/files|files|3.000|3.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/update/throughput|operations/s|58,496.614|58,496.614|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/ulmdb/close_reopen_ns|ns|9,622,422,698.254|7,074,479,085.096|-26.48%|3,778,052.171|-187152.02%|slower / worse|99.95%|
|millions/disk/ulmdb/load/throughput|operations/s|542,846.530|1,412,943.532|+160.28%|1,518,731.002|-6.97%|slower / worse|7.49%|
|millions/disk/ulmdb/read/throughput|operations/s|2,540,831.403|2,801,877.511|+10.27%|1,694,797.402|+65.32%|faster / better|0.00%|
|millions/disk/ulmdb/setup_ns|ns|407,791,045.002|401,910,848.317|-1.44%|1,320,664.112|-30332.48%|slower / worse|99.67%|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|bytes|268,436,821.326|268,435,456.000|-0.00%|34,649,429.280|-674.72%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_after_close_reopen/apparent_bytes|bytes|268,435,456.000|268,435,456.000|+0.00%|34,652,160.000|-674.66%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_after_close_reopen/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|millions/disk/ulmdb/storage_before_close/allocated_bytes|bytes|268,436,821.326|268,435,456.000|-0.00%|34,649,429.280|-674.72%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_before_close/apparent_bytes|bytes|268,435,456.000|268,435,456.000|+0.00%|34,652,160.000|-674.66%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_before_close/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|millions/disk/ulmdb/update/throughput|operations/s|103,510.316|143,992.462|+39.11%|128,986.150|+11.63%|faster / better|0.00%|

Complete confidence intervals, all process values/signs and insufficient-repeat flags remain in the unchanged original comparison JSON/Markdown.

#### Candidate versus accepted: every metric

|Metric|Unit|Reference|Candidate|Candidate change|Local LMDB|Native advantage|Status|Required increase / cost reduction|
|---|---|---:|---:|---:|---:|---:|---|---:|
|full/disk/harness/setup/after_setup_ns|ns|1,934.184|1,750.338|-9.51%|unavailable|unavailable|not measured|unavailable|
|full/disk/harness/setup/dataset_ns|ns|235,630,104.028|235,069,365.989|-0.24%|unavailable|unavailable|not measured|unavailable|
|full/disk/harness/setup/fixture_load_both_ns|ns|10,337,649.696|10,200,099.663|-1.33%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/combined_throughput|records/s|10,881.845|11,183.397|+2.77%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/pooled_sampled_p99|ns|342,056.435|326,251.678|-4.62%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/sampled_p99|ns|200,209.042|189,928.797|-5.13%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/throughput|records/s|10,318.880|10,449.137|+1.26%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/combined_throughput|records/s|180,602.945|183,640.121|+1.68%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/pooled_sampled_p99|ns|524,357.679|519,092.299|-1.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/sampled_p99|ns|485,338.077|457,394.578|-5.76%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/throughput|records/s|175,326.818|177,520.410|+1.25%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/allocated_file_bytes|bytes|729,088.000|729,088.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/apparent_file_bytes|bytes|733,184.000|733,184.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_pss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_rss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/allocated_file_bytes|bytes|352,256.000|352,256.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/apparent_file_bytes|bytes|356,352.000|356,352.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_pss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_rss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_hits/sampled_p99|ns|778.447|785.950|+0.96%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_hits/throughput|queries/s|2,131,750.563|2,122,588.266|-0.43%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|623.107|607.625|-2.48%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|1,990,633.544|1,991,091.763|+0.02%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_misses/sampled_p99|ns|617.519|608.728|-1.42%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_misses/throughput|queries/s|3,889,136.496|3,923,508.988|+0.88%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_mixed/sampled_p99|ns|777.724|778.807|+0.14%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_mixed/throughput|queries/s|2,634,826.562|2,675,061.372|+1.53%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|45.432|47.935|+5.51%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|89,480,210.503|94,865,349.488|+6.02%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_hits/sampled_p99|ns|1,265.143|1,233.624|-2.49%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_hits/throughput|queries/s|1,239,059.211|1,248,170.365|+0.74%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|1,124.490|1,094.189|-2.69%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|1,118,932.826|1,132,601.984|+1.22%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_misses/sampled_p99|ns|1,107.641|1,060.169|-4.29%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_misses/throughput|queries/s|2,211,440.103|2,232,854.975|+0.97%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_mixed/sampled_p99|ns|1,264.925|1,234.997|-2.37%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_mixed/throughput|queries/s|1,550,952.265|1,570,901.809|+1.29%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|43.219|44.332|+2.58%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|91,114,641.076|95,141,815.086|+4.42%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_hits/sampled_p99|ns|342.530|335.186|-2.14%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_hits/throughput|queries/s|4,162,214.273|4,197,283.230|+0.84%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_misses/sampled_p99|ns|344.296|346.065|+0.51%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_misses/throughput|queries/s|3,990,756.066|4,025,747.725|+0.88%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_mixed/sampled_p99|ns|342.294|345.246|+0.86%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_mixed/throughput|queries/s|4,113,506.414|4,105,707.568|-0.19%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/setup/create|ns|201,043.573|210,759.466|+4.83%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/durable_batch1/combined_throughput|records/s|15,674.896|15,729.261|+0.35%|11,183.397|+40.65%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99|ns|262,528.434|257,131.136|-2.06%|326,251.678|+21.19%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/sampled_p99|ns|89,228.405|86,587.415|-2.96%|189,928.797|+54.41%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/throughput|records/s|15,088.349|14,820.944|-1.77%|10,449.137|+41.84%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/combined_throughput|records/s|492,580.544|494,716.934|+0.43%|183,640.121|+169.39%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99|ns|384,796.637|431,543.978|+12.15%|519,092.299|+16.87%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/sampled_p99|ns|218,074.621|221,149.968|+1.41%|457,394.578|+51.65%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/throughput|records/s|482,773.151|484,486.157|+0.35%|177,520.410|+172.92%|faster / better|0.00%|
|full/disk/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|67,109,887.977|+0.00%|729,088.000|-9104.63%|slower / worse|98.91%|
|full/disk/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|733,184.000|-9053.07%|slower / worse|98.91%|
|full/disk/ulmdb/footprint/after/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/disk/ulmdb/footprint/after/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/after/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/after/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|67,109,887.977|+0.00%|352,256.000|-18951.45%|slower / worse|99.48%|
|full/disk/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|356,352.000|-18732.18%|slower / worse|99.47%|
|full/disk/ulmdb/footprint/before/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/disk/ulmdb/footprint/before/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99|ns|427.371|434.386|+1.64%|785.950|+44.73%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,198,572.690|3,190,805.366|-0.24%|2,122,588.266|+50.33%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|121.385|124.633|+2.68%|607.625|+79.49%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,601,546.031|11,556,000.802|-0.39%|1,991,091.763|+480.39%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99|ns|120.750|124.716|+3.28%|608.728|+79.51%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,784,315.778|21,396,797.164|-1.78%|3,923,508.988|+445.35%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|423.376|427.376|+0.94%|778.807|+45.12%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,536,195.694|5,496,726.697|-0.71%|2,675,061.372|+105.48%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.140|41.822|+1.66%|47.935|+12.75%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|99,980,258.037|99,921,198.213|-0.06%|94,865,349.488|+5.33%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99|ns|528.737|524.303|-0.84%|1,233.624|+57.50%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,465,062.062|2,474,277.856|+0.37%|1,248,170.365|+98.23%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|144.460|141.853|-1.81%|1,094.189|+87.04%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,361,314.063|9,377,067.593|+0.17%|1,132,601.984|+727.92%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99|ns|145.096|141.413|-2.54%|1,060.169|+86.66%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,099,670.905|17,355,330.634|+1.50%|2,232,854.975|+677.27%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|522.714|518.919|-0.73%|1,234.997|+57.98%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,284,185.357|4,300,935.031|+0.39%|1,570,901.809|+173.79%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.000|41.140|+0.34%|44.332|+7.20%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|97,271,185.074|97,412,680.889|+0.15%|95,141,815.086|+2.39%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99|ns|201.078|202.254|+0.58%|335.186|+39.66%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/throughput|queries/s|7,584,847.235|7,447,323.373|-1.81%|4,197,283.230|+77.43%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99|ns|202.705|201.909|-0.39%|346.065|+41.66%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/throughput|queries/s|7,616,139.043|7,530,108.322|-1.13%|4,025,747.725|+87.05%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99|ns|205.637|206.813|+0.57%|345.246|+40.10%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/throughput|queries/s|7,245,067.595|7,152,980.121|-1.27%|4,105,707.568|+74.22%|faster / better|0.00%|
|full/disk/ulmdb/reopen/complete_cold_check_ns|ns|96,049,066.765|96,537,992.040|+0.51%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/environment_drop_ns|ns|95,482.712|100,604.969|+5.36%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/experimental_reenable_ns|ns|2,233.660|2,161.053|-3.25%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/open_ns|ns|85,409,290.589|85,989,296.878|+0.68%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/validation_ns|ns|6,233,243.827|6,199,502.798|-0.54%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/setup/create|ns|106,398,966.313|103,371,214.314|-2.85%|210,759.466|-48947.01%|slower / worse|99.80%|
|full/ram/harness/setup/after_setup_ns|ns|2,836.839|4,281.379|+50.92%|unavailable|unavailable|not measured|unavailable|
|full/ram/harness/setup/dataset_ns|ns|236,166,518.381|232,534,638.392|-1.54%|unavailable|unavailable|not measured|unavailable|
|full/ram/harness/setup/fixture_load_both_ns|ns|9,035,207.652|9,128,266.016|+1.03%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/combined_throughput|records/s|137,957.774|136,708.222|-0.91%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/pooled_sampled_p99|ns|9,472.997|9,377.878|-1.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/sampled_p99|ns|7,980.814|8,231.024|+3.14%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/throughput|records/s|133,655.308|132,434.903|-0.91%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/combined_throughput|records/s|753,730.700|755,350.054|+0.21%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/pooled_sampled_p99|ns|99,414.301|97,766.109|-1.66%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/sampled_p99|ns|92,588.990|91,795.601|-0.86%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/throughput|records/s|746,348.436|746,873.033|+0.07%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/allocated_file_bytes|bytes|729,088.000|729,088.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/apparent_file_bytes|bytes|733,184.000|733,184.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_pss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_rss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/allocated_file_bytes|bytes|352,256.000|352,256.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/apparent_file_bytes|bytes|356,352.000|356,352.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_pss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_rss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_hits/sampled_p99|ns|792.275|785.526|-0.85%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_hits/throughput|queries/s|2,134,559.044|2,107,248.304|-1.28%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|647.898|652.985|+0.79%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|1,993,433.446|1,918,560.919|-3.76%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_misses/sampled_p99|ns|647.621|641.844|-0.89%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_misses/throughput|queries/s|3,833,183.044|3,740,817.968|-2.41%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_mixed/sampled_p99|ns|787.119|766.050|-2.68%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_mixed/throughput|queries/s|2,632,777.146|2,611,966.002|-0.79%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|43.394|45.291|+4.37%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|85,480,842.660|92,382,892.290|+8.07%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_hits/sampled_p99|ns|1,271.972|1,263.108|-0.70%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_hits/throughput|queries/s|1,248,641.381|1,234,924.988|-1.10%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|1,122.113|1,102.636|-1.74%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|1,125,766.895|1,116,721.894|-0.80%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_misses/sampled_p99|ns|1,090.747|1,086.819|-0.36%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_misses/throughput|queries/s|2,217,385.028|2,209,095.131|-0.37%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_mixed/sampled_p99|ns|1,270.976|1,259.899|-0.87%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_mixed/throughput|queries/s|1,559,131.311|1,541,977.477|-1.10%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|42.436|43.650|+2.86%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|87,223,815.694|91,533,214.262|+4.94%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_hits/sampled_p99|ns|344.970|335.576|-2.72%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_hits/throughput|queries/s|4,169,076.602|4,220,345.142|+1.23%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_misses/sampled_p99|ns|347.656|342.728|-1.42%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_misses/throughput|queries/s|3,985,722.530|4,024,150.053|+0.96%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_mixed/sampled_p99|ns|342.798|342.365|-0.13%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_mixed/throughput|queries/s|4,103,890.571|4,112,240.056|+0.20%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/setup/create|ns|166,683.577|126,931.749|-23.85%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/durable_batch1/combined_throughput|records/s|74,352.319|74,689.152|+0.45%|136,708.222|-45.37%|slower / worse|83.04%|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99|ns|14,826.842|11,955.620|-19.37%|9,377.878|-27.49%|slower / worse|21.56%|
|full/ram/ulmdb/durable_batch1/sampled_p99|ns|11,783.646|11,310.416|-4.02%|8,231.024|-37.41%|slower / worse|27.23%|
|full/ram/ulmdb/durable_batch1/throughput|records/s|61,049.243|60,818.221|-0.38%|132,434.903|-54.08%|slower / worse|117.76%|
|full/ram/ulmdb/durable_batch64/combined_throughput|records/s|990,345.171|981,568.454|-0.89%|755,350.054|+29.95%|faster / better|0.00%|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99|ns|256,230.877|258,358.250|+0.83%|97,766.109|-164.26%|slower / worse|62.16%|
|full/ram/ulmdb/durable_batch64/sampled_p99|ns|102,046.646|102,978.929|+0.91%|91,795.601|-12.18%|slower / worse|10.86%|
|full/ram/ulmdb/durable_batch64/throughput|records/s|982,042.235|970,126.199|-1.21%|746,873.033|+29.89%|faster / better|0.00%|
|full/ram/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|729,088.000|-9104.49%|slower / worse|98.91%|
|full/ram/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|733,184.000|-9053.07%|slower / worse|98.91%|
|full/ram/ulmdb/footprint/after/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/ram/ulmdb/footprint/after/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/after/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/after/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|352,256.000|-18951.16%|slower / worse|99.48%|
|full/ram/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|356,352.000|-18732.18%|slower / worse|99.47%|
|full/ram/ulmdb/footprint/before/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/ram/ulmdb/footprint/before/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99|ns|432.237|431.917|-0.07%|785.526|+45.02%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,188,938.204|3,199,124.026|+0.32%|2,107,248.304|+51.82%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|122.147|121.717|-0.35%|652.985|+81.36%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,557,392.491|11,558,020.364|+0.01%|1,918,560.919|+502.43%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99|ns|120.561|122.407|+1.53%|641.844|+80.93%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,692,072.521|21,598,862.523|-0.43%|3,740,817.968|+477.38%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|418.871|424.309|+1.30%|766.050|+44.61%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,449,629.798|5,515,018.979|+1.20%|2,611,966.002|+111.14%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.422|41.383|-0.09%|45.291|+8.63%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|97,178,182.324|100,015,050.225|+2.92%|92,382,892.290|+8.26%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99|ns|535.517|525.100|-1.95%|1,263.108|+58.43%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,467,266.774|2,482,573.754|+0.62%|1,234,924.988|+101.03%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|142.599|143.699|+0.77%|1,102.636|+86.97%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,360,464.427|9,372,817.447|+0.13%|1,116,721.894|+739.32%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99|ns|141.966|143.099|+0.80%|1,086.819|+86.83%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,051,676.139|17,374,931.709|+1.90%|2,209,095.131|+686.52%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|523.750|517.054|-1.28%|1,259.899|+58.96%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,303,175.891|4,303,934.651|+0.02%|1,541,977.477|+179.12%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.127|41.140|+0.03%|43.650|+5.75%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|96,819,420.303|97,443,066.610|+0.64%|91,533,214.262|+6.46%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99|ns|196.564|200.377|+1.94%|335.576|+40.29%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/throughput|queries/s|7,754,180.701|7,526,620.656|-2.93%|4,220,345.142|+78.34%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99|ns|195.611|201.120|+2.82%|342.728|+41.32%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/throughput|queries/s|7,773,238.284|7,583,333.538|-2.44%|4,024,150.053|+88.45%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99|ns|201.198|205.983|+2.38%|342.365|+39.84%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/throughput|queries/s|7,415,940.056|7,232,832.662|-2.47%|4,112,240.056|+75.89%|faster / better|0.00%|
|full/ram/ulmdb/reopen/complete_cold_check_ns|ns|53,438,266.315|53,306,886.723|-0.25%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/environment_drop_ns|ns|90,929.771|93,936.959|+3.31%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/experimental_reenable_ns|ns|2,220.056|2,321.678|+4.58%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/open_ns|ns|42,879,036.426|42,735,889.644|-0.33%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/validation_ns|ns|6,262,227.377|6,254,722.025|-0.12%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/setup/create|ns|48,456,582.750|48,327,696.034|-0.27%|126,931.749|-37973.77%|slower / worse|99.74%|
|millions/disk/lmdb/close_reopen_ns|ns|3,778,052.171|3,778,052.171|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/load/throughput|operations/s|1,518,731.002|1,518,731.002|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/read/throughput|operations/s|1,694,797.402|1,694,797.402|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/setup_ns|ns|1,320,664.112|1,320,664.112|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/allocated_bytes|bytes|34,649,429.280|34,649,429.280|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/apparent_bytes|bytes|34,652,160.000|34,652,160.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/allocated_bytes|bytes|34,649,429.280|34,649,429.280|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/apparent_bytes|bytes|34,652,160.000|34,652,160.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/update/throughput|operations/s|128,986.150|128,986.150|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/close_reopen_ns|ns|9,330,639.735|9,330,639.735|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/load/throughput|operations/s|820,508.069|820,508.069|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/read/throughput|operations/s|461,905.377|461,905.377|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/setup_ns|ns|2,702,740.552|2,702,740.552|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/allocated_bytes|bytes|25,317,376.000|25,317,376.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/apparent_bytes|bytes|25,317,376.000|25,317,376.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/files|files|1.000|1.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/allocated_bytes|bytes|33,103,872.000|33,103,872.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/apparent_bytes|bytes|33,099,896.000|33,099,896.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/files|files|3.000|3.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/update/throughput|operations/s|58,496.614|58,496.614|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/ulmdb/close_reopen_ns|ns|7,094,076,603.298|7,074,479,085.096|-0.28%|3,778,052.171|-187152.02%|slower / worse|99.95%|
|millions/disk/ulmdb/load/throughput|operations/s|1,402,006.815|1,412,943.532|+0.78%|1,518,731.002|-6.97%|slower / worse|7.49%|
|millions/disk/ulmdb/read/throughput|operations/s|2,796,750.496|2,801,877.511|+0.18%|1,694,797.402|+65.32%|faster / better|0.00%|
|millions/disk/ulmdb/setup_ns|ns|406,329,717.213|401,910,848.317|-1.09%|1,320,664.112|-30332.48%|slower / worse|99.67%|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|bytes|268,438,186.660|268,435,456.000|-0.00%|34,649,429.280|-674.72%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_after_close_reopen/apparent_bytes|bytes|268,435,456.000|268,435,456.000|+0.00%|34,652,160.000|-674.66%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_after_close_reopen/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|millions/disk/ulmdb/storage_before_close/allocated_bytes|bytes|268,438,186.660|268,435,456.000|-0.00%|34,649,429.280|-674.72%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_before_close/apparent_bytes|bytes|268,435,456.000|268,435,456.000|+0.00%|34,652,160.000|-674.66%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_before_close/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|millions/disk/ulmdb/update/throughput|operations/s|145,908.554|143,992.462|-1.31%|128,986.150|+11.63%|faster / better|0.00%|

Complete confidence intervals, all process values/signs and insufficient-repeat flags remain in the unchanged original comparison JSON/Markdown.



[Independent source/raw/availability/metric receipt](benchmark/work/optimization-20261105/next-hierarchical-diff/retained6-v1/campaign-independent-review-v1/review-receipt.json). The core audit independently reproduced both237-row comparisons and checked172,898,304 raw bytes,11,544 spans and21,612,288 samples. All103 target/native rows, cost gaps, flags, uncertainty and unavailable observations remain; a roughly-on-par label is descriptive only.

## Latest complete engine candidate: O_DSYNC witness publication HELD

**Root decision: HOLD, no apply. Retained6 remains current.** All 39 original processes passed correctness/provenance and both 237-cell comparisons were independently reproduced. RAM singles improve 1.63%, but RAM64 loses 2.94% in all four pairs and RAM KV reads lose 1.74-1.86%. That small gain does not justify the tradeoff. The actual OS fault suite passed all seven workers, including O_DSYNC flags/fences, before/half/complete witness failures, acquisition fallback and data-failure retry; correctness qualification does not establish performance benefit.

**Exposure is retained, not normalized:** accepted-reference steps 001 (full disk r0),025 (million r0) and031 (million r1) observed external qualification activity. All 39 remain; 36 boundaries were observed quiet. Two of three million references are exposed, so accepted6-to-candidate million deltas cannot qualify a gain. Candidate and local native boundaries were observed quiet; direct candidate/native centers remain separately labelled descriptive observations.

### O_DSYNC witness publication: completed standard39 review

Root retention decision: **HOLD this private candidate, no apply**; independent review agrees. Keep retained iteration6 unchanged. RAM single-write throughput improves 1.63% (all four pairs), but RAM batch64 throughput falls 2.94% (all four), its combined rate falls 2.84% with the adjusted regression interval flagged, and all RAM KV read centers fall 1.74–1.86%. The measured single-write gain does not yet offset these costs or close the remaining 53.21% direct LMDB rate deficit. This is a performance decision, not an engine failure or a finding against the publication protocol.

All39 original processes passed workload/cold/oracle/source/native/SQLite checks. Both237-cell comparisons and103 direct-native target cells are retained. All24 raw latency files were independently rechecked, including quantiles, complete span coverage, combined sums and every sampled write value. Source snapshot473 is the unchanged built471 plus exactly two reviewed integration-test additions; all547 actual source/support inputs and sealed binaries/tools are bound. The wrapper's exit1 is the comparison screen, not a workload error.

Availability:36 observed-quiet boundaries and three exposed **accepted-reference** processes:001 full/disk/r0,025 million/ULMDB/r0 and031 million/ULMDB/r1. Every observation remains; no retry, exclusion or normalization. Disk reference comparisons include one exposed pair. Million reference deltas contain two exposed pairs and cannot qualify an accepted6-to-candidate load/update/read/setup/reopen gain. Candidate and contemporary LMDB/SQLite boundaries were observed quiet; those direct comparisons are described separately. Boundary checks cannot prove continuous host isolation.

Centers below are geometric means of process metrics. Positive native advantage means faster throughput or lower cost; within2% is a descriptive roughly-on-par label, not equivalence. Rates and costs retain their own percentage directions.

|Metric|Accepted6|Candidate|Candidate vs6|Local LMDB|Candidate native advantage|Native status|Context|
|---|---:|---:|---:|---:|---:|---|---|
|full/ram/ulmdb/durable_batch1/throughput|62,481.958|63,501.275|+1.63%|135,729.615|-53.21%|slower / worse|all four arm boundaries observed quiet|
|full/ram/ulmdb/durable_batch1/combined_throughput|75,068.290|76,929.775|+2.48%|139,713.868|-44.94%|slower / worse|all four arm boundaries observed quiet|
|full/ram/ulmdb/durable_batch64/throughput|986,136.534|957,157.145|-2.94%|753,443.239|+27.04%|faster / better|all four arm boundaries observed quiet|
|full/ram/ulmdb/durable_batch64/combined_throughput|994,734.581|966,526.338|-2.84%|761,062.424|+27.00%|faster / better|all four arm boundaries observed quiet|
|full/disk/ulmdb/durable_batch1/throughput|15,288.139|15,607.295|+2.09%|10,825.596|+44.17%|faster / better|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/durable_batch1/combined_throughput|16,131.466|16,253.198|+0.75%|11,355.984|+43.12%|faster / better|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/durable_batch64/throughput|464,090.387|482,645.850|+4.00%|180,207.279|+167.83%|faster / better|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/durable_batch64/combined_throughput|481,862.063|490,838.589|+1.86%|184,825.366|+165.57%|faster / better|1/4 accepted references exposed; all retained|
|millions/disk/ulmdb/load/throughput|1,404,162.100|1,415,627.982|+0.82%|1,522,677.986|-7.03%|slower / worse|2/3 accepted references exposed; delta unqualified|
|millions/disk/ulmdb/read/throughput|2,809,320.152|2,776,897.863|-1.15%|1,704,468.902|+62.92%|faster / better|2/3 accepted references exposed; delta unqualified|
|millions/disk/ulmdb/update/throughput|146,643.984|145,321.167|-0.90%|128,260.031|+13.30%|faster / better|2/3 accepted references exposed; delta unqualified|
|millions/disk/ulmdb/setup_ns|398,977,312.399|427,430,356.885|+7.13%|1,655,265.379|-25722.47%|slower / worse|2/3 accepted references exposed; delta unqualified|
|millions/disk/ulmdb/close_reopen_ns|7,061,978,065.283|6,957,078,605.035|-1.49%|3,840,144.834|-181067.09%|slower / worse|2/3 accepted references exposed; delta unqualified|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|268,435,456.000|268,436,821.326|+0.00%|34,648,064.000|-674.75%|slower / worse|2/3 accepted references exposed; delta unqualified|

#### All13 full read cases, retained signs and tails

|Filesystem/case|Rate vs6|Four rate pairs|Trial p99 vs6|Candidate rate/s|Native rate/s|Native advantage|Context|
|---|---:|---|---:|---:|---:|---:|---|
|disk/ipv4_lpm_hits|-1.77%|-0.57%, -2.83%, -1.58%, -2.08%|+1.33%|3,145,769.555|2,121,372.512|+48.29%|1/4 accepted references exposed; all retained|
|disk/ipv4_lpm_inside_gap_misses|-1.39%|+0.50%, -2.51%, -0.38%, -3.14%|-0.28%|11,476,443.122|2,062,659.767|+456.39%|1/4 accepted references exposed; all retained|
|disk/ipv4_lpm_misses|-1.65%|-0.79%, -1.68%, -0.63%, -3.46%|-0.54%|21,408,062.881|3,979,691.075|+437.93%|1/4 accepted references exposed; all retained|
|disk/ipv4_lpm_mixed|-1.29%|+0.55%, -2.32%, -1.53%, -1.82%|+2.14%|5,461,237.621|2,651,306.875|+105.98%|1/4 accepted references exposed; all retained|
|disk/ipv4_lpm_outside_cover_misses|+0.73%|-1.12%, +2.79%, +1.33%, -0.03%|+0.03%|100,629,999.313|90,324,860.268|+11.41%|1/4 accepted references exposed; all retained|
|disk/ipv6_lpm_hits|-1.14%|+0.14%, -2.26%, -2.13%, -0.27%|-0.83%|2,466,851.173|1,233,833.832|+99.93%|1/4 accepted references exposed; all retained|
|disk/ipv6_lpm_inside_gap_misses|-0.90%|+0.60%, -3.32%, -0.57%, -0.29%|+0.46%|9,341,756.974|1,128,963.831|+727.46%|1/4 accepted references exposed; all retained|
|disk/ipv6_lpm_misses|+1.63%|+0.13%, -0.77%, +3.64%, +3.61%|-0.19%|17,303,175.751|2,230,253.570|+675.84%|1/4 accepted references exposed; all retained|
|disk/ipv6_lpm_mixed|-0.19%|+1.03%, -0.90%, -0.21%, -0.66%|-0.92%|4,301,823.154|1,553,648.665|+176.89%|1/4 accepted references exposed; all retained|
|disk/ipv6_lpm_outside_cover_misses|+1.52%|+1.38%, -0.10%, +0.39%, +4.48%|-0.93%|97,520,591.355|91,608,943.432|+6.45%|1/4 accepted references exposed; all retained|
|disk/ordered_kv_hits|-3.24%|-2.11%, +0.60%, -5.78%, -5.53%|+3.97%|7,371,338.854|4,112,093.108|+79.26%|1/4 accepted references exposed; all retained|
|disk/ordered_kv_misses|-2.92%|-1.45%, +1.26%, -5.56%, -5.74%|+3.88%|7,453,687.149|3,931,216.303|+89.60%|1/4 accepted references exposed; all retained|
|disk/ordered_kv_mixed|-2.61%|-1.67%, +0.72%, -5.52%, -3.85%|+2.75%|7,103,888.146|4,013,859.840|+76.98%|1/4 accepted references exposed; all retained|
|ram/ipv4_lpm_hits|-1.98%|-1.64%, -1.68%, -3.24%, -1.33%|-0.10%|3,124,644.868|2,124,961.847|+47.04%|all four arm boundaries observed quiet|
|ram/ipv4_lpm_inside_gap_misses|+0.47%|+0.36%, -0.82%, +2.32%, +0.04%|-0.79%|11,641,370.361|1,999,025.140|+482.35%|all four arm boundaries observed quiet|
|ram/ipv4_lpm_misses|-1.11%|-4.20%, -1.55%, +2.20%, -0.79%|+0.61%|21,382,850.517|3,871,080.484|+452.37%|all four arm boundaries observed quiet|
|ram/ipv4_lpm_mixed|-0.87%|-0.15%, -0.40%, -2.32%, -0.60%|-0.91%|5,449,685.127|2,620,830.141|+107.94%|all four arm boundaries observed quiet|
|ram/ipv4_lpm_outside_cover_misses|-0.06%|+0.09%, +0.19%, -0.56%, +0.04%|-1.26%|100,818,514.820|95,363,770.876|+5.72%|all four arm boundaries observed quiet|
|ram/ipv6_lpm_hits|-0.41%|+0.08%, -1.06%, -0.52%, -0.12%|-0.55%|2,481,891.753|1,222,107.092|+103.08%|all four arm boundaries observed quiet|
|ram/ipv6_lpm_inside_gap_misses|+0.34%|+1.93%, -1.67%, +0.61%, +0.52%|-1.29%|9,392,112.919|1,116,249.221|+741.40%|all four arm boundaries observed quiet|
|ram/ipv6_lpm_misses|+0.80%|+2.16%, +0.32%, -0.04%, +0.77%|-0.79%|17,308,473.085|2,214,079.752|+681.75%|all four arm boundaries observed quiet|
|ram/ipv6_lpm_mixed|+1.41%|+2.14%, +0.95%, +0.70%, +1.85%|-2.19%|4,339,401.251|1,539,744.938|+181.83%|all four arm boundaries observed quiet|
|ram/ipv6_lpm_outside_cover_misses|-0.26%|+0.31%, -0.41%, -0.55%, -0.38%|-0.31%|97,172,775.242|94,926,956.061|+2.37%|all four arm boundaries observed quiet|
|ram/ordered_kv_hits|-1.86%|-1.99%, -1.97%, -2.16%, -1.32%|+2.88%|7,401,734.989|4,121,502.389|+79.59%|all four arm boundaries observed quiet|
|ram/ordered_kv_misses|-1.74%|-1.87%, -2.89%, -1.63%, -0.56%|+0.70%|7,440,030.722|3,924,041.555|+89.60%|all four arm boundaries observed quiet|
|ram/ordered_kv_mixed|-1.82%|-1.90%, -2.04%, -1.59%, -1.73%|+4.82%|7,093,263.051|3,993,381.864|+77.63%|all four arm boundaries observed quiet|

#### Write tails: trial centers, process pools and true global pools

The process-pooled p99 center is the geometric mean of four process p99s. The global p99 pools all512 raw samples. They answer different questions; neither replaces the other. All maxima remain.

|Filesystem/batch|Trial p99 vs6|Four trial pairs|Process-pooled p99 vs6|Global512 p99 accepted→candidate ns|Global p99 change|Maximum accepted→candidate ns|Context|
|---|---:|---|---:|---:|---:|---:|---|
|disk/1|+2.36%|+6.86%, +12.20%, -16.13%, +9.20%|-18.19%|310,219→313,416|+1.03%|330,799→407,696|1/4 accepted references exposed; all retained|
|disk/64|+9.50%|-9.83%, +27.10%, +40.08%, -10.45%|+2.95%|452,690→445,767|-1.53%|603,708→617,835|1/4 accepted references exposed; all retained|
|ram/1|-6.57%|-4.09%, -6.89%, -4.59%, -10.56%|-12.73%|15,891→13,115|-17.47%|33,864→18,054|all four arm boundaries observed quiet|
|ram/64|+1.45%|+1.67%, +0.96%, +0.37%, +2.80%|-1.28%|258,281→250,427|-3.04%|266,727→273,179|all four arm boundaries observed quiet|

Disk batch64 trial p99 rises9.50% and process-pooled p99 rises2.95%, while the true global p99 falls1.53%; its maximum rises603708→617835ns. Disk single process-pooled p99 falls18.19% but the global p99 rises1.03% and maximum330799→407696ns. Keep all these observations; no pooling choice dismisses an adverse value.

#### Every adverse target point estimate versus accepted6

|Metric|Change|All paired changes|Context|
|---|---:|---|---|
|full/disk/ulmdb/durable_batch1/sampled_p99|+2.36%|+6.86%, +12.20%, -16.13%, +9.20%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99|+2.95%|-13.17%, +3.19%, +6.45%, +17.77%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/durable_batch64/sampled_p99|+9.50%|-9.83%, +27.10%, +40.08%, -10.45%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/footprint/after/allocated_file_bytes|+0.00%|+0.01%, +0.00%, +0.00%, +0.00%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/footprint/before/allocated_file_bytes|+0.00%|+0.01%, +0.00%, +0.00%, +0.00%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99|+1.33%|+1.12%, +0.17%, +0.73%, +3.34%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ipv4_lpm_hits/throughput|-1.77%|-0.57%, -2.83%, -1.58%, -2.08%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput|-1.39%|+0.50%, -2.51%, -0.38%, -3.14%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ipv4_lpm_misses/throughput|-1.65%|-0.79%, -1.68%, -0.63%, -3.46%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99|+2.14%|-0.32%, +1.57%, +2.76%, +4.60%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput|-1.29%|+0.55%, -2.32%, -1.53%, -1.82%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|+0.03%|+1.25%, +0.12%, -1.23%, +0.00%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ipv6_lpm_hits/throughput|-1.14%|+0.14%, -2.26%, -2.13%, -0.27%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|+0.46%|-1.93%, +1.03%, -0.32%, +3.13%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput|-0.90%|+0.60%, -3.32%, -0.57%, -0.29%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput|-0.19%|+1.03%, -0.90%, -0.21%, -0.66%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99|+3.97%|+2.28%, +2.30%, +6.50%, +4.86%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ordered_kv_hits/throughput|-3.24%|-2.11%, +0.60%, -5.78%, -5.53%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99|+3.88%|+1.13%, +2.55%, +6.53%, +5.37%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ordered_kv_misses/throughput|-2.92%|-1.45%, +1.26%, -5.56%, -5.74%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99|+2.75%|+1.71%, +2.63%, +6.24%, +0.48%|1/4 accepted references exposed; all retained|
|full/disk/ulmdb/ordered_kv_mixed/throughput|-2.61%|-1.67%, +0.72%, -5.52%, -3.85%|1/4 accepted references exposed; all retained|
|full/ram/ulmdb/durable_batch64/combined_throughput|-2.84%|-2.55%, -2.90%, -2.76%, -3.13%|all four arm boundaries observed quiet|
|full/ram/ulmdb/durable_batch64/sampled_p99|+1.45%|+1.67%, +0.96%, +0.37%, +2.80%|all four arm boundaries observed quiet|
|full/ram/ulmdb/durable_batch64/throughput|-2.94%|-2.30%, -3.23%, -2.83%, -3.39%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ipv4_lpm_hits/throughput|-1.98%|-1.64%, -1.68%, -3.24%, -1.33%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99|+0.61%|-2.16%, +1.50%, +1.07%, +2.10%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ipv4_lpm_misses/throughput|-1.11%|-4.20%, -1.55%, +2.20%, -0.79%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput|-0.87%|-0.15%, -0.40%, -2.32%, -0.60%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput|-0.06%|+0.09%, +0.19%, -0.56%, +0.04%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ipv6_lpm_hits/throughput|-0.41%|+0.08%, -1.06%, -0.52%, -0.12%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput|-0.26%|+0.31%, -0.41%, -0.55%, -0.38%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99|+2.88%|+6.25%, +2.06%, +2.66%, +0.62%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ordered_kv_hits/throughput|-1.86%|-1.99%, -1.97%, -2.16%, -1.32%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99|+0.70%|-1.48%, +1.57%, +0.92%, +1.80%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ordered_kv_misses/throughput|-1.74%|-1.87%, -2.89%, -1.63%, -0.56%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99|+4.82%|+5.49%, +4.75%, +4.22%, +4.81%|all four arm boundaries observed quiet|
|full/ram/ulmdb/ordered_kv_mixed/throughput|-1.82%|-1.90%, -2.04%, -1.59%, -1.73%|all four arm boundaries observed quiet|
|full/ram/ulmdb/reopen/experimental_reenable_ns|+1.83%|-8.70%, +3.75%, -10.42%, +26.72%|all four arm boundaries observed quiet|
|full/ram/ulmdb/reopen/validation_ns|+1.53%|+2.24%, +2.68%, +0.94%, +0.27%|all four arm boundaries observed quiet|
|millions/disk/ulmdb/read/throughput|-1.15%|-2.94%, -0.85%, +0.36%|2/3 accepted references exposed; delta unqualified|
|millions/disk/ulmdb/setup_ns|+7.13%|+17.89%, +2.55%, +1.70%|2/3 accepted references exposed; delta unqualified|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|+0.00%|+0.00%, +0.00%, +0.00%|2/3 accepted references exposed; delta unqualified|
|millions/disk/ulmdb/storage_before_close/allocated_bytes|+0.00%|+0.00%, +0.00%, +0.00%|2/3 accepted references exposed; delta unqualified|
|millions/disk/ulmdb/update/throughput|-0.90%|-0.83%, -0.89%, -0.98%|2/3 accepted references exposed; delta unqualified|

#### Initial-baseline context and evidence

All initial observations had quiet boundary snapshots. Initial-to-candidate comparisons are preserved in full; they do not override the regression check against retained6. No rate, read, tail, reopen, setup or file observation is omitted.

|Metric|Initial|Candidate|Candidate vs initial|
|---|---:|---:|---:|
|full/ram/ulmdb/durable_batch1/throughput|57,258.702|63,501.275|+10.90%|
|full/ram/ulmdb/durable_batch1/combined_throughput|69,046.383|76,929.775|+11.42%|
|full/ram/ulmdb/durable_batch64/throughput|931,619.152|957,157.145|+2.74%|
|full/ram/ulmdb/durable_batch64/combined_throughput|931,951.408|966,526.338|+3.71%|
|full/disk/ulmdb/durable_batch1/throughput|15,872.236|15,607.295|-1.67%|
|full/disk/ulmdb/durable_batch1/combined_throughput|16,614.960|16,253.198|-2.18%|
|full/disk/ulmdb/durable_batch64/throughput|487,013.045|482,645.850|-0.90%|
|full/disk/ulmdb/durable_batch64/combined_throughput|495,864.004|490,838.589|-1.01%|
|millions/disk/ulmdb/load/throughput|546,129.445|1,415,627.982|+159.21%|
|millions/disk/ulmdb/read/throughput|2,579,537.096|2,776,897.863|+7.65%|
|millions/disk/ulmdb/update/throughput|103,933.308|145,321.167|+39.82%|
|millions/disk/ulmdb/setup_ns|406,664,359.720|427,430,356.885|+5.11%|
|millions/disk/ulmdb/close_reopen_ns|9,608,457,169.232|6,957,078,605.035|-27.59%|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|268,435,456.000|268,436,821.326|+0.00%|

[Every237 metric against each baseline](benchmark/work/optimization-20261105/next-witness-dsync/retained6-v1/campaign-independent-review-v1/FULL_TABLES.md), [all103 direct LMDB targets](benchmark/work/optimization-20261105/next-witness-dsync/retained6-v1/campaign-independent-review-v1/NATIVE_SCORECARD.md), [original accepted comparison](benchmark/work/optimization-20261105/next-witness-dsync/retained6-v1/campaign-v1/accepted-vs-candidate.json), [original initial comparison](benchmark/work/optimization-20261105/next-witness-dsync/retained6-v1/campaign-v1/initial-vs-candidate.json), [raw write observations](benchmark/work/optimization-20261105/next-witness-dsync/retained6-v1/campaign-independent-review-v1/raw-review.json), [availability identities](benchmark/work/optimization-20261105/next-witness-dsync/retained6-v1/campaign-independent-review-v1/availability-review.json), [global write pools](benchmark/work/optimization-20261105/next-witness-dsync/retained6-v1/campaign-independent-review-v1/global-write-tails.json), [independent review receipt](benchmark/work/optimization-20261105/next-witness-dsync/retained6-v1/campaign-independent-review-v1/review-receipt.json).


### O_DSYNC candidate: all103 targets directly versus contemporary LMDB

This private candidate is held by the independent review. Retained6 stays published. All candidate/native boundary snapshots were observed quiet;36/39 campaign processes were observed quiet, with only accepted-reference001/025/031 exposed. No observations were excluded or normalized. Same workload/durability/oracles; shared million controls disclosed. These are descriptive process centers, not equivalence or a continuous-isolation guarantee.

Positive native advantage is better. Gap is the required throughput increase or cost reduction to match LMDB. Within2% is only a descriptive roughly-on-par label. All metric identifiers retain rates/cost units; file counts have no speed implication.

|Metric|Unit|Candidate|LMDB|Candidate native advantage|Verdict|Required increase / cost reduction|
|---|---|---:|---:|---:|---|---:|
|full/disk/ulmdb/durable_batch1/combined_throughput|records/s|16,253.198|11,355.984|+43.12%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99|ns|256,876.133|326,080.607|+21.22%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/sampled_p99|ns|84,626.699|195,760.295|+56.77%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/throughput|records/s|15,607.295|10,825.596|+44.17%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/combined_throughput|records/s|490,838.589|184,825.366|+165.57%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99|ns|427,583.595|525,262.676|+18.60%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/sampled_p99|ns|233,378.634|475,379.751|+50.91%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/throughput|records/s|482,645.850|180,207.279|+167.83%|faster / better|0.00%|
|full/disk/ulmdb/footprint/after/allocated_file_bytes|bytes|67,109,887.977|729,088.000|-9104.63%|slower / worse|98.91%|
|full/disk/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|733,184.000|-9053.07%|slower / worse|98.91%|
|full/disk/ulmdb/footprint/after/files|files|2.000|2.000|-0.00%|roughly on par|0.00%|
|full/disk/ulmdb/footprint/before/allocated_file_bytes|bytes|67,109,887.977|352,256.000|-18951.45%|slower / worse|99.48%|
|full/disk/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|356,352.000|-18732.18%|slower / worse|99.47%|
|full/disk/ulmdb/footprint/before/files|files|2.000|2.000|-0.00%|roughly on par|0.00%|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99|ns|436.127|782.577|+44.27%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,145,769.555|2,121,372.512|+48.29%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|122.062|604.493|+79.81%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,476,443.122|2,062,659.767|+456.39%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99|ns|121.897|601.197|+79.72%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,408,062.881|3,979,691.075|+437.93%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|426.635|765.790|+44.29%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,461,237.621|2,651,306.875|+105.98%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.409|45.291|+8.57%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|100,629,999.313|90,324,860.268|+11.41%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99|ns|535.200|1,257.165|+57.43%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,466,851.173|1,233,833.832|+99.93%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|143.190|1,100.889|+86.99%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,341,756.974|1,128,963.831|+727.46%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99|ns|142.007|1,083.245|+86.89%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,303,175.751|2,230,253.570|+675.84%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|515.009|1,249.096|+58.77%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,301,823.154|1,553,648.665|+176.89%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.409|43.394|+4.57%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|97,520,591.355|91,608,943.432|+6.45%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99|ns|205.063|332.430|+38.31%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/throughput|queries/s|7,371,338.854|4,112,093.108|+79.26%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99|ns|203.505|337.880|+39.77%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/throughput|queries/s|7,453,687.149|3,931,216.303|+89.60%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99|ns|210.646|333.328|+36.81%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/throughput|queries/s|7,103,888.146|4,013,859.840|+76.98%|faster / better|0.00%|
|full/disk/ulmdb/reopen/complete_cold_check_ns|ns|95,047,162.795|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/environment_drop_ns|ns|91,986.704|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/experimental_reenable_ns|ns|2,086.637|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/open_ns|ns|84,549,613.512|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/validation_ns|ns|6,166,500.229|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/setup/create|ns|102,910,026.138|194,178.531|-52897.63%|slower / worse|99.81%|
|full/ram/ulmdb/durable_batch1/combined_throughput|records/s|76,929.775|139,713.868|-44.94%|slower / worse|81.61%|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99|ns|13,746.129|9,220.314|-49.09%|slower / worse|32.92%|
|full/ram/ulmdb/durable_batch1/sampled_p99|ns|11,285.353|7,896.467|-42.92%|slower / worse|30.03%|
|full/ram/ulmdb/durable_batch1/throughput|records/s|63,501.275|135,729.615|-53.21%|slower / worse|113.74%|
|full/ram/ulmdb/durable_batch64/combined_throughput|records/s|966,526.338|761,062.424|+27.00%|faster / better|0.00%|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99|ns|249,606.516|97,030.649|-157.25%|slower / worse|61.13%|
|full/ram/ulmdb/durable_batch64/sampled_p99|ns|103,357.267|90,668.206|-14.00%|slower / worse|12.28%|
|full/ram/ulmdb/durable_batch64/throughput|records/s|957,157.145|753,443.239|+27.04%|faster / better|0.00%|
|full/ram/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|729,088.000|-9104.49%|slower / worse|98.91%|
|full/ram/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|733,184.000|-9053.07%|slower / worse|98.91%|
|full/ram/ulmdb/footprint/after/files|files|2.000|2.000|-0.00%|roughly on par|0.00%|
|full/ram/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|352,256.000|-18951.16%|slower / worse|99.48%|
|full/ram/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|356,352.000|-18732.18%|slower / worse|99.47%|
|full/ram/ulmdb/footprint/before/files|files|2.000|2.000|-0.00%|roughly on par|0.00%|
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99|ns|434.658|784.859|+44.62%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,124,644.868|2,124,961.847|+47.04%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|121.566|624.556|+80.54%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,641,370.361|1,999,025.140|+482.35%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99|ns|122.559|630.143|+80.55%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,382,850.517|3,871,080.484|+452.37%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|425.426|766.387|+44.49%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,449,685.127|2,620,830.141|+107.94%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.281|47.893|+13.81%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|100,818,514.820|95,363,770.876|+5.72%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99|ns|530.385|1,258.493|+57.86%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,481,891.753|1,222,107.092|+103.08%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|141.619|1,112.726|+87.27%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,392,112.919|1,116,249.221|+741.40%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99|ns|141.679|1,077.411|+86.85%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,308,473.085|2,214,079.752|+681.75%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|513.784|1,254.272|+59.04%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,339,401.251|1,539,744.938|+181.83%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.140|43.908|+6.30%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|97,172,775.242|94,926,956.061|+2.37%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99|ns|205.665|331.943|+38.04%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/throughput|queries/s|7,401,734.989|4,121,502.389|+79.59%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99|ns|202.941|340.431|+40.39%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/throughput|queries/s|7,440,030.722|3,924,041.555|+89.60%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99|ns|211.260|336.107|+37.15%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/throughput|queries/s|7,093,263.051|3,993,381.864|+77.63%|faster / better|0.00%|
|full/ram/ulmdb/reopen/complete_cold_check_ns|ns|53,105,089.051|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/environment_drop_ns|ns|86,882.887|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/experimental_reenable_ns|ns|2,125.256|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/open_ns|ns|42,603,602.427|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/validation_ns|ns|6,217,550.624|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/setup/create|ns|47,226,436.834|123,054.575|-38278.45%|slower / worse|99.74%|
|millions/disk/ulmdb/close_reopen_ns|ns|6,957,078,605.035|3,840,144.834|-181067.09%|slower / worse|99.94%|
|millions/disk/ulmdb/load/throughput|operations/s|1,415,627.982|1,522,677.986|-7.03%|slower / worse|7.56%|
|millions/disk/ulmdb/read/throughput|operations/s|2,776,897.863|1,704,468.902|+62.92%|faster / better|0.00%|
|millions/disk/ulmdb/setup_ns|ns|427,430,356.885|1,655,265.379|-25722.47%|slower / worse|99.61%|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|bytes|268,436,821.326|34,648,064.000|-674.75%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_after_close_reopen/apparent_bytes|bytes|268,435,456.000|34,652,160.000|-674.66%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_after_close_reopen/files|files|2.000|2.000|-0.00%|roughly on par|0.00%|
|millions/disk/ulmdb/storage_before_close/allocated_bytes|bytes|268,436,821.326|34,648,064.000|-674.75%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_before_close/apparent_bytes|bytes|268,435,456.000|34,652,160.000|-674.66%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_before_close/files|files|2.000|2.000|-0.00%|roughly on par|0.00%|
|millions/disk/ulmdb/update/throughput|operations/s|145,321.167|128,260.031|+13.30%|faster / better|0.00%|

The load candidate remains7.03% slower at observed centers and needs7.56% more throughput to reach native. RAM single writes remain53.21% slower and need113.74% more throughput. Post-update reopen remains roughly1812× native and allocation roughly7.75×; O_DSYNC is a publication optimization, with no recovery/checkpoint or allocation change in this candidate.


### Complete witness DSYNC standard39 tables

All original observations remain, including every adverse cell. Four full process rounds/filesystem and three ordinary-million rounds; source/native/SQLite/raw/oracles checked. Contemporary million LMDB/SQLite controls are explicitly shared across arms. No native normalization or exclusions.

Actual processes39; observed quiet36; exposed3. Boundary checks do not prove continuous isolation. Exposed tags and identities are in availability-review.json. The exposed observations are accepted-reference full disk r0 and accepted-reference million r0/r1. Candidate/native boundary observations are quiet; reference deltas are not normalized, filtered or qualified by those candidate boundaries.

Rates/cost centers are geometric means of process metrics. Positive native advantage means more throughput or less cost. Within2% is a descriptive roughly-on-par label, not equivalence; remaining gap is required throughput increase or cost reduction to match native at the observed centers.

#### Candidate versus initial: every metric

|Metric|Unit|Reference|Candidate|Candidate change|Local LMDB|Native advantage|Status|Required increase / cost reduction|
|---|---|---:|---:|---:|---:|---:|---|---:|
|full/disk/harness/setup/after_setup_ns|ns|906.046|1,649.664|+82.07%|unavailable|unavailable|not measured|unavailable|
|full/disk/harness/setup/dataset_ns|ns|237,791,179.024|235,169,728.106|-1.10%|unavailable|unavailable|not measured|unavailable|
|full/disk/harness/setup/fixture_load_both_ns|ns|19,651,609.948|10,171,425.331|-48.24%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/combined_throughput|records/s|11,351.411|11,355.984|+0.04%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/pooled_sampled_p99|ns|324,518.368|326,080.607|+0.48%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/sampled_p99|ns|179,469.481|195,760.295|+9.08%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/throughput|records/s|10,526.497|10,825.596|+2.84%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/combined_throughput|records/s|182,566.028|184,825.366|+1.24%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/pooled_sampled_p99|ns|525,074.343|525,262.676|+0.04%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/sampled_p99|ns|455,785.411|475,379.751|+4.30%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/throughput|records/s|177,467.527|180,207.279|+1.54%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/allocated_file_bytes|bytes|729,088.000|729,088.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/apparent_file_bytes|bytes|733,184.000|733,184.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_pss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_rss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/allocated_file_bytes|bytes|352,256.000|352,256.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/apparent_file_bytes|bytes|356,352.000|356,352.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_pss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_rss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_hits/sampled_p99|ns|787.350|782.577|-0.61%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_hits/throughput|queries/s|2,128,857.516|2,121,372.512|-0.35%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|639.802|604.493|-5.52%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|2,037,117.155|2,062,659.767|+1.25%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_misses/sampled_p99|ns|624.192|601.197|-3.68%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_misses/throughput|queries/s|3,972,847.252|3,979,691.075|+0.17%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_mixed/sampled_p99|ns|776.068|765.790|-1.32%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_mixed/throughput|queries/s|2,684,056.722|2,651,306.875|-1.22%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|46.070|45.291|-1.69%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|88,298,990.926|90,324,860.268|+2.29%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_hits/sampled_p99|ns|1,291.762|1,257.165|-2.68%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_hits/throughput|queries/s|1,193,544.178|1,233,833.832|+3.38%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|1,077.359|1,100.889|+2.18%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|1,129,093.879|1,128,963.831|-0.01%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_misses/sampled_p99|ns|1,052.875|1,083.245|+2.88%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_misses/throughput|queries/s|2,225,305.279|2,230,253.570|+0.22%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_mixed/sampled_p99|ns|1,298.405|1,249.096|-3.80%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_mixed/throughput|queries/s|1,507,577.038|1,553,648.665|+3.06%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|43.922|43.394|-1.20%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|90,875,863.167|91,608,943.432|+0.81%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_hits/sampled_p99|ns|335.019|332.430|-0.77%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_hits/throughput|queries/s|4,176,085.634|4,112,093.108|-1.53%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_misses/sampled_p99|ns|339.837|337.880|-0.58%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_misses/throughput|queries/s|4,047,407.797|3,931,216.303|-2.87%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_mixed/sampled_p99|ns|336.857|333.328|-1.05%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_mixed/throughput|queries/s|4,098,810.541|4,013,859.840|-2.07%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/setup/create|ns|209,804.189|194,178.531|-7.45%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/durable_batch1/combined_throughput|records/s|16,614.960|16,253.198|-2.18%|11,355.984|+43.12%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99|ns|194,932.985|256,876.133|+31.78%|326,080.607|+21.22%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/sampled_p99|ns|78,543.032|84,626.699|+7.75%|195,760.295|+56.77%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/throughput|records/s|15,872.236|15,607.295|-1.67%|10,825.596|+44.17%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/combined_throughput|records/s|495,864.004|490,838.589|-1.01%|184,825.366|+165.57%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99|ns|460,635.275|427,583.595|-7.18%|525,262.676|+18.60%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/sampled_p99|ns|214,452.676|233,378.634|+8.83%|475,379.751|+50.91%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/throughput|records/s|487,013.045|482,645.850|-0.90%|180,207.279|+167.83%|faster / better|0.00%|
|full/disk/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|67,109,887.977|+0.00%|729,088.000|-9104.63%|slower / worse|98.91%|
|full/disk/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|733,184.000|-9053.07%|slower / worse|98.91%|
|full/disk/ulmdb/footprint/after/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/disk/ulmdb/footprint/after/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/after/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/after/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|67,109,887.977|+0.00%|352,256.000|-18951.45%|slower / worse|99.48%|
|full/disk/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|356,352.000|-18732.18%|slower / worse|99.47%|
|full/disk/ulmdb/footprint/before/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/disk/ulmdb/footprint/before/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99|ns|433.385|436.127|+0.63%|782.577|+44.27%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,163,402.447|3,145,769.555|-0.56%|2,121,372.512|+48.29%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|117.303|122.062|+4.06%|604.493|+79.81%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,427,794.481|11,476,443.122|+0.43%|2,062,659.767|+456.39%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99|ns|116.772|121.897|+4.39%|601.197|+79.72%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,520,252.860|21,408,062.881|-0.52%|3,979,691.075|+437.93%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|425.755|426.635|+0.21%|765.790|+44.29%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,468,249.870|5,461,237.621|-0.13%|2,651,306.875|+105.98%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.679|41.409|-0.65%|45.291|+8.57%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|95,939,588.060|100,629,999.313|+4.89%|90,324,860.268|+11.41%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99|ns|557.753|535.200|-4.04%|1,257.165|+57.43%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,422,564.799|2,466,851.173|+1.83%|1,233,833.832|+99.93%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|142.281|143.190|+0.64%|1,100.889|+86.99%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,279,608.620|9,341,756.974|+0.67%|1,128,963.831|+727.46%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99|ns|142.536|142.007|-0.37%|1,083.245|+86.89%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,007,951.348|17,303,175.751|+1.74%|2,230,253.570|+675.84%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|550.729|515.009|-6.49%|1,249.096|+58.77%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,248,518.116|4,301,823.154|+1.25%|1,553,648.665|+176.89%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.525|41.409|-0.28%|43.394|+4.57%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|95,336,334.028|97,520,591.355|+2.29%|91,608,943.432|+6.45%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99|ns|231.386|205.063|-11.38%|332.430|+38.31%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/throughput|queries/s|6,412,917.661|7,371,338.854|+14.95%|4,112,093.108|+79.26%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99|ns|231.972|203.505|-12.27%|337.880|+39.77%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/throughput|queries/s|6,379,869.986|7,453,687.149|+16.83%|3,931,216.303|+89.60%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99|ns|238.500|210.646|-11.68%|333.328|+36.81%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/throughput|queries/s|6,046,125.302|7,103,888.146|+17.49%|4,013,859.840|+76.98%|faster / better|0.00%|
|full/disk/ulmdb/reopen/complete_cold_check_ns|ns|94,962,314.021|95,047,162.795|+0.09%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/environment_drop_ns|ns|91,639.387|91,986.704|+0.38%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/experimental_reenable_ns|ns|2,382.680|2,086.637|-12.42%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/open_ns|ns|84,116,236.760|84,549,613.512|+0.52%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/validation_ns|ns|6,555,390.978|6,166,500.229|-5.93%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/setup/create|ns|103,341,012.351|102,910,026.138|-0.42%|194,178.531|-52897.63%|slower / worse|99.81%|
|full/ram/harness/setup/after_setup_ns|ns|994.444|2,961.729|+197.83%|unavailable|unavailable|not measured|unavailable|
|full/ram/harness/setup/dataset_ns|ns|237,648,020.489|234,823,927.698|-1.19%|unavailable|unavailable|not measured|unavailable|
|full/ram/harness/setup/fixture_load_both_ns|ns|18,437,877.920|9,069,576.775|-50.81%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/combined_throughput|records/s|138,482.602|139,713.868|+0.89%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/pooled_sampled_p99|ns|8,593.051|9,220.314|+7.30%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/sampled_p99|ns|8,145.069|7,896.467|-3.05%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/throughput|records/s|134,347.299|135,729.615|+1.03%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/combined_throughput|records/s|750,719.912|761,062.424|+1.38%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/pooled_sampled_p99|ns|98,357.465|97,030.649|-1.35%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/sampled_p99|ns|92,229.327|90,668.206|-1.69%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/throughput|records/s|743,071.660|753,443.239|+1.40%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/allocated_file_bytes|bytes|729,088.000|729,088.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/apparent_file_bytes|bytes|733,184.000|733,184.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_pss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_rss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/allocated_file_bytes|bytes|352,256.000|352,256.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/apparent_file_bytes|bytes|356,352.000|356,352.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_pss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_rss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_hits/sampled_p99|ns|767.550|784.859|+2.26%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_hits/throughput|queries/s|2,146,780.351|2,124,961.847|-1.02%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|603.492|624.556|+3.49%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|2,022,579.374|1,999,025.140|-1.16%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_misses/sampled_p99|ns|598.673|630.143|+5.26%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_misses/throughput|queries/s|3,950,232.220|3,871,080.484|-2.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_mixed/sampled_p99|ns|753.979|766.387|+1.65%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_mixed/throughput|queries/s|2,681,509.247|2,620,830.141|-2.26%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|45.291|47.893|+5.75%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|89,392,231.281|95,363,770.876|+6.68%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_hits/sampled_p99|ns|1,245.155|1,258.493|+1.07%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_hits/throughput|queries/s|1,240,935.442|1,222,107.092|-1.52%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|1,076.710|1,112.726|+3.34%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|1,141,645.050|1,116,249.221|-2.22%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_misses/sampled_p99|ns|1,045.430|1,077.411|+3.06%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_misses/throughput|queries/s|2,255,708.954|2,214,079.752|-1.85%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_mixed/sampled_p99|ns|1,238.684|1,254.272|+1.26%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_mixed/throughput|queries/s|1,566,434.786|1,539,744.938|-1.70%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|43.528|43.908|+0.87%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|91,503,104.153|94,926,956.061|+3.74%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_hits/sampled_p99|ns|336.077|331.943|-1.23%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_hits/throughput|queries/s|4,155,701.671|4,121,502.389|-0.82%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_misses/sampled_p99|ns|339.415|340.431|+0.30%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_misses/throughput|queries/s|4,024,430.822|3,924,041.555|-2.49%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_mixed/sampled_p99|ns|336.747|336.107|-0.19%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_mixed/throughput|queries/s|4,117,269.027|3,993,381.864|-3.01%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/setup/create|ns|137,375.707|123,054.575|-10.42%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/durable_batch1/combined_throughput|records/s|69,046.383|76,929.775|+11.42%|139,713.868|-44.94%|slower / worse|81.61%|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99|ns|16,631.804|13,746.129|-17.35%|9,220.314|-49.09%|slower / worse|32.92%|
|full/ram/ulmdb/durable_batch1/sampled_p99|ns|12,837.080|11,285.353|-12.09%|7,896.467|-42.92%|slower / worse|30.03%|
|full/ram/ulmdb/durable_batch1/throughput|records/s|57,258.702|63,501.275|+10.90%|135,729.615|-53.21%|slower / worse|113.74%|
|full/ram/ulmdb/durable_batch64/combined_throughput|records/s|931,951.408|966,526.338|+3.71%|761,062.424|+27.00%|faster / better|0.00%|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99|ns|328,555.683|249,606.516|-24.03%|97,030.649|-157.25%|slower / worse|61.13%|
|full/ram/ulmdb/durable_batch64/sampled_p99|ns|113,516.289|103,357.267|-8.95%|90,668.206|-14.00%|slower / worse|12.28%|
|full/ram/ulmdb/durable_batch64/throughput|records/s|931,619.152|957,157.145|+2.74%|753,443.239|+27.04%|faster / better|0.00%|
|full/ram/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|729,088.000|-9104.49%|slower / worse|98.91%|
|full/ram/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|733,184.000|-9053.07%|slower / worse|98.91%|
|full/ram/ulmdb/footprint/after/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/ram/ulmdb/footprint/after/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/after/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/after/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|352,256.000|-18951.16%|slower / worse|99.48%|
|full/ram/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|356,352.000|-18732.18%|slower / worse|99.47%|
|full/ram/ulmdb/footprint/before/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/ram/ulmdb/footprint/before/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99|ns|432.556|434.658|+0.49%|784.859|+44.62%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,197,741.370|3,124,644.868|-2.29%|2,124,961.847|+47.04%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|115.069|121.566|+5.65%|624.556|+80.54%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,557,721.128|11,641,370.361|+0.72%|1,999,025.140|+482.35%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99|ns|113.505|122.559|+7.98%|630.143|+80.55%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,673,971.177|21,382,850.517|-1.34%|3,871,080.484|+452.37%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|423.850|425.426|+0.37%|766.387|+44.49%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,493,827.275|5,449,685.127|-0.80%|2,620,830.141|+107.94%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.409|41.281|-0.31%|47.893|+13.81%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|100,277,537.939|100,818,514.820|+0.54%|95,363,770.876|+5.72%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99|ns|534.499|530.385|-0.77%|1,258.493|+57.86%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,509,662.656|2,481,891.753|-1.11%|1,222,107.092|+103.08%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|143.430|141.619|-1.26%|1,112.726|+87.27%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,261,083.302|9,392,112.919|+1.41%|1,116,249.221|+741.40%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99|ns|143.531|141.679|-1.29%|1,077.411|+86.85%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/throughput|queries/s|16,988,740.666|17,308,473.085|+1.88%|2,214,079.752|+681.75%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|520.434|513.784|-1.28%|1,254.272|+59.04%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,358,568.905|4,339,401.251|-0.44%|1,539,744.938|+181.83%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.396|41.140|-0.62%|43.908|+6.30%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|95,144,888.376|97,172,775.242|+2.13%|94,926,956.061|+2.37%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99|ns|234.032|205.665|-12.12%|331.943|+38.04%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/throughput|queries/s|6,421,930.414|7,401,734.989|+15.26%|4,121,502.389|+79.59%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99|ns|231.952|202.941|-12.51%|340.431|+40.39%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/throughput|queries/s|6,393,086.439|7,440,030.722|+16.38%|3,924,041.555|+89.60%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99|ns|239.458|211.260|-11.78%|336.107|+37.15%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/throughput|queries/s|6,092,931.821|7,093,263.051|+16.42%|3,993,381.864|+77.63%|faster / better|0.00%|
|full/ram/ulmdb/reopen/complete_cold_check_ns|ns|54,769,420.169|53,105,089.051|-3.04%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/environment_drop_ns|ns|87,982.157|86,882.887|-1.25%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/experimental_reenable_ns|ns|2,202.541|2,125.256|-3.51%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/open_ns|ns|44,007,140.757|42,603,602.427|-3.19%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/validation_ns|ns|6,463,180.716|6,217,550.624|-3.80%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/setup/create|ns|47,716,545.556|47,226,436.834|-1.03%|123,054.575|-38278.45%|slower / worse|99.74%|
|millions/disk/lmdb/close_reopen_ns|ns|3,840,144.834|3,840,144.834|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/load/throughput|operations/s|1,522,677.986|1,522,677.986|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/read/throughput|operations/s|1,704,468.902|1,704,468.902|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/setup_ns|ns|1,655,265.379|1,655,265.379|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/allocated_bytes|bytes|34,648,064.000|34,648,064.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/apparent_bytes|bytes|34,652,160.000|34,652,160.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/allocated_bytes|bytes|34,648,064.000|34,648,064.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/apparent_bytes|bytes|34,652,160.000|34,652,160.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/update/throughput|operations/s|128,260.031|128,260.031|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/close_reopen_ns|ns|9,134,084.374|9,134,084.374|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/load/throughput|operations/s|829,450.862|829,450.862|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/read/throughput|operations/s|465,251.101|465,251.101|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/setup_ns|ns|2,515,565.601|2,515,565.601|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/allocated_bytes|bytes|25,320,106.593|25,320,106.593|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/apparent_bytes|bytes|25,317,376.000|25,317,376.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/files|files|1.000|1.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/allocated_bytes|bytes|33,106,602.610|33,106,602.610|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/apparent_bytes|bytes|33,099,896.000|33,099,896.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/files|files|3.000|3.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/update/throughput|operations/s|58,739.188|58,739.188|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/ulmdb/close_reopen_ns|ns|9,608,457,169.232|6,957,078,605.035|-27.59%|3,840,144.834|-181067.09%|slower / worse|99.94%|
|millions/disk/ulmdb/load/throughput|operations/s|546,129.445|1,415,627.982|+159.21%|1,522,677.986|-7.03%|slower / worse|7.56%|
|millions/disk/ulmdb/read/throughput|operations/s|2,579,537.096|2,776,897.863|+7.65%|1,704,468.902|+62.92%|faster / better|0.00%|
|millions/disk/ulmdb/setup_ns|ns|406,664,359.720|427,430,356.885|+5.11%|1,655,265.379|-25722.47%|slower / worse|99.61%|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|bytes|268,435,456.000|268,436,821.326|+0.00%|34,648,064.000|-674.75%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_after_close_reopen/apparent_bytes|bytes|268,435,456.000|268,435,456.000|+0.00%|34,652,160.000|-674.66%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_after_close_reopen/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|millions/disk/ulmdb/storage_before_close/allocated_bytes|bytes|268,435,456.000|268,436,821.326|+0.00%|34,648,064.000|-674.75%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_before_close/apparent_bytes|bytes|268,435,456.000|268,435,456.000|+0.00%|34,652,160.000|-674.66%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_before_close/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|millions/disk/ulmdb/update/throughput|operations/s|103,933.308|145,321.167|+39.82%|128,260.031|+13.30%|faster / better|0.00%|

Complete confidence intervals, all process values/signs and insufficient-repeat flags remain in the unchanged original comparison JSON/Markdown.

#### Candidate versus accepted: every metric

|Metric|Unit|Reference|Candidate|Candidate change|Local LMDB|Native advantage|Status|Required increase / cost reduction|
|---|---|---:|---:|---:|---:|---:|---|---:|
|full/disk/harness/setup/after_setup_ns|ns|1,689.452|1,649.664|-2.36%|unavailable|unavailable|not measured|unavailable|
|full/disk/harness/setup/dataset_ns|ns|235,167,361.131|235,169,728.106|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/harness/setup/fixture_load_both_ns|ns|10,314,796.816|10,171,425.331|-1.39%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/combined_throughput|records/s|11,188.334|11,355.984|+1.50%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/pooled_sampled_p99|ns|326,143.553|326,080.607|-0.02%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/sampled_p99|ns|195,880.679|195,760.295|-0.06%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/throughput|records/s|10,580.295|10,825.596|+2.32%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/combined_throughput|records/s|179,967.089|184,825.366|+2.70%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/pooled_sampled_p99|ns|554,613.931|525,262.676|-5.29%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/sampled_p99|ns|489,555.352|475,379.751|-2.90%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/throughput|records/s|176,117.224|180,207.279|+2.32%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/allocated_file_bytes|bytes|729,088.000|729,088.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/apparent_file_bytes|bytes|733,184.000|733,184.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_pss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_rss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/allocated_file_bytes|bytes|352,256.000|352,256.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/apparent_file_bytes|bytes|356,352.000|356,352.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_pss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_rss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_hits/sampled_p99|ns|801.279|782.577|-2.33%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_hits/throughput|queries/s|2,127,138.777|2,121,372.512|-0.27%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|617.845|604.493|-2.16%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|2,039,413.287|2,062,659.767|+1.14%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_misses/sampled_p99|ns|619.854|601.197|-3.01%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_misses/throughput|queries/s|3,905,798.170|3,979,691.075|+1.89%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_mixed/sampled_p99|ns|777.438|765.790|-1.50%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_mixed/throughput|queries/s|2,659,457.624|2,651,306.875|-0.31%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|43.666|45.291|+3.72%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|87,591,052.382|90,324,860.268|+3.12%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_hits/sampled_p99|ns|1,239.461|1,257.165|+1.43%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_hits/throughput|queries/s|1,245,845.674|1,233,833.832|-0.96%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|1,112.620|1,100.889|-1.05%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|1,128,436.226|1,128,963.831|+0.05%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_misses/sampled_p99|ns|1,093.279|1,083.245|-0.92%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_misses/throughput|queries/s|2,233,028.340|2,230,253.570|-0.12%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_mixed/sampled_p99|ns|1,232.046|1,249.096|+1.38%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_mixed/throughput|queries/s|1,562,109.660|1,553,648.665|-0.54%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|42.357|43.394|+2.45%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|87,663,800.185|91,608,943.432|+4.50%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_hits/sampled_p99|ns|335.464|332.430|-0.90%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_hits/throughput|queries/s|4,181,249.531|4,112,093.108|-1.65%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_misses/sampled_p99|ns|337.458|337.880|+0.13%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_misses/throughput|queries/s|4,032,483.142|3,931,216.303|-2.51%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_mixed/sampled_p99|ns|341.296|333.328|-2.33%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_mixed/throughput|queries/s|4,115,491.011|4,013,859.840|-2.47%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/setup/create|ns|192,729.673|194,178.531|+0.75%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/durable_batch1/combined_throughput|records/s|16,131.466|16,253.198|+0.75%|11,355.984|+43.12%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99|ns|313,986.409|256,876.133|-18.19%|326,080.607|+21.22%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/sampled_p99|ns|82,671.855|84,626.699|+2.36%|195,760.295|+56.77%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch1/throughput|records/s|15,288.139|15,607.295|+2.09%|10,825.596|+44.17%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/combined_throughput|records/s|481,862.063|490,838.589|+1.86%|184,825.366|+165.57%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99|ns|415,336.551|427,583.595|+2.95%|525,262.676|+18.60%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/sampled_p99|ns|213,134.679|233,378.634|+9.50%|475,379.751|+50.91%|faster / better|0.00%|
|full/disk/ulmdb/durable_batch64/throughput|records/s|464,090.387|482,645.850|+4.00%|180,207.279|+167.83%|faster / better|0.00%|
|full/disk/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|67,109,887.977|+0.00%|729,088.000|-9104.63%|slower / worse|98.91%|
|full/disk/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|733,184.000|-9053.07%|slower / worse|98.91%|
|full/disk/ulmdb/footprint/after/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/disk/ulmdb/footprint/after/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/after/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/after/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|67,109,887.977|+0.00%|352,256.000|-18951.45%|slower / worse|99.48%|
|full/disk/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|356,352.000|-18732.18%|slower / worse|99.47%|
|full/disk/ulmdb/footprint/before/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/disk/ulmdb/footprint/before/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99|ns|430.389|436.127|+1.33%|782.577|+44.27%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,202,457.409|3,145,769.555|-1.77%|2,121,372.512|+48.29%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|122.409|122.062|-0.28%|604.493|+79.81%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,638,384.534|11,476,443.122|-1.39%|2,062,659.767|+456.39%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99|ns|122.562|121.897|-0.54%|601.197|+79.72%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,766,517.152|21,408,062.881|-1.65%|3,979,691.075|+437.93%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|417.715|426.635|+2.14%|765.790|+44.29%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,532,329.418|5,461,237.621|-1.29%|2,651,306.875|+105.98%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.396|41.409|+0.03%|45.291|+8.57%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|99,897,691.807|100,629,999.313|+0.73%|90,324,860.268|+11.41%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99|ns|539.659|535.200|-0.83%|1,257.165|+57.43%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,495,202.014|2,466,851.173|-1.14%|1,233,833.832|+99.93%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|142.532|143.190|+0.46%|1,100.889|+86.99%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,427,048.797|9,341,756.974|-0.90%|1,128,963.831|+727.46%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99|ns|142.281|142.007|-0.19%|1,083.245|+86.89%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,025,173.004|17,303,175.751|+1.63%|2,230,253.570|+675.84%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|519.795|515.009|-0.92%|1,249.096|+58.77%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,309,999.729|4,301,823.154|-0.19%|1,553,648.665|+176.89%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.796|41.409|-0.93%|43.394|+4.57%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|96,057,974.939|97,520,591.355|+1.52%|91,608,943.432|+6.45%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99|ns|197.238|205.063|+3.97%|332.430|+38.31%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/throughput|queries/s|7,618,083.850|7,371,338.854|-3.24%|4,112,093.108|+79.26%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99|ns|195.912|203.505|+3.88%|337.880|+39.77%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/throughput|queries/s|7,677,730.911|7,453,687.149|-2.92%|3,931,216.303|+89.60%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99|ns|205.018|210.646|+2.75%|333.328|+36.81%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/throughput|queries/s|7,294,105.429|7,103,888.146|-2.61%|4,013,859.840|+76.98%|faster / better|0.00%|
|full/disk/ulmdb/reopen/complete_cold_check_ns|ns|96,059,097.303|95,047,162.795|-1.05%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/environment_drop_ns|ns|93,051.649|91,986.704|-1.14%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/experimental_reenable_ns|ns|2,214.211|2,086.637|-5.76%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/open_ns|ns|85,491,933.895|84,549,613.512|-1.10%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/validation_ns|ns|6,202,879.213|6,166,500.229|-0.59%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/setup/create|ns|104,394,160.957|102,910,026.138|-1.42%|194,178.531|-52897.63%|slower / worse|99.81%|
|full/ram/harness/setup/after_setup_ns|ns|1,034.709|2,961.729|+186.24%|unavailable|unavailable|not measured|unavailable|
|full/ram/harness/setup/dataset_ns|ns|237,041,377.266|234,823,927.698|-0.94%|unavailable|unavailable|not measured|unavailable|
|full/ram/harness/setup/fixture_load_both_ns|ns|9,185,379.117|9,069,576.775|-1.26%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/combined_throughput|records/s|138,689.641|139,713.868|+0.74%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/pooled_sampled_p99|ns|8,322.336|9,220.314|+10.79%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/sampled_p99|ns|7,782.111|7,896.467|+1.47%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/throughput|records/s|134,979.246|135,729.615|+0.56%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/combined_throughput|records/s|759,346.397|761,062.424|+0.23%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/pooled_sampled_p99|ns|95,292.997|97,030.649|+1.82%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/sampled_p99|ns|90,991.854|90,668.206|-0.36%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/throughput|records/s|751,132.271|753,443.239|+0.31%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/allocated_file_bytes|bytes|729,088.000|729,088.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/apparent_file_bytes|bytes|733,184.000|733,184.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_pss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_rss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/allocated_file_bytes|bytes|352,256.000|352,256.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/apparent_file_bytes|bytes|356,352.000|356,352.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_pss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_rss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_hits/sampled_p99|ns|796.131|784.859|-1.42%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_hits/throughput|queries/s|2,112,033.627|2,124,961.847|+0.61%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|670.156|624.556|-6.80%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|1,956,187.518|1,999,025.140|+2.19%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_misses/sampled_p99|ns|659.353|630.143|-4.43%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_misses/throughput|queries/s|3,750,437.887|3,871,080.484|+3.22%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_mixed/sampled_p99|ns|782.496|766.387|-2.06%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_mixed/throughput|queries/s|2,616,245.366|2,620,830.141|+0.18%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|50.174|47.893|-4.54%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|100,609,147.785|95,363,770.876|-5.21%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_hits/sampled_p99|ns|1,252.297|1,258.493|+0.49%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_hits/throughput|queries/s|1,235,255.187|1,222,107.092|-1.06%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|1,122.755|1,112.726|-0.89%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|1,121,480.651|1,116,249.221|-0.47%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_misses/sampled_p99|ns|1,099.205|1,077.411|-1.98%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_misses/throughput|queries/s|2,206,560.061|2,214,079.752|+0.34%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_mixed/sampled_p99|ns|1,246.285|1,254.272|+0.64%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_mixed/throughput|queries/s|1,548,800.974|1,539,744.938|-0.58%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|44.622|43.908|-1.60%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|98,418,838.055|94,926,956.061|-3.55%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_hits/sampled_p99|ns|338.547|331.943|-1.95%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_hits/throughput|queries/s|4,204,304.044|4,121,502.389|-1.97%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_misses/sampled_p99|ns|345.802|340.431|-1.55%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_misses/throughput|queries/s|3,995,651.943|3,924,041.555|-1.79%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_mixed/sampled_p99|ns|338.708|336.107|-0.77%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_mixed/throughput|queries/s|4,120,949.687|3,993,381.864|-3.10%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/setup/create|ns|120,583.238|123,054.575|+2.05%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/durable_batch1/combined_throughput|records/s|75,068.290|76,929.775|+2.48%|139,713.868|-44.94%|slower / worse|81.61%|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99|ns|15,751.781|13,746.129|-12.73%|9,220.314|-49.09%|slower / worse|32.92%|
|full/ram/ulmdb/durable_batch1/sampled_p99|ns|12,078.943|11,285.353|-6.57%|7,896.467|-42.92%|slower / worse|30.03%|
|full/ram/ulmdb/durable_batch1/throughput|records/s|62,481.958|63,501.275|+1.63%|135,729.615|-53.21%|slower / worse|113.74%|
|full/ram/ulmdb/durable_batch64/combined_throughput|records/s|994,734.581|966,526.338|-2.84%|761,062.424|+27.00%|faster / better|0.00%|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99|ns|252,838.667|249,606.516|-1.28%|97,030.649|-157.25%|slower / worse|61.13%|
|full/ram/ulmdb/durable_batch64/sampled_p99|ns|101,882.629|103,357.267|+1.45%|90,668.206|-14.00%|slower / worse|12.28%|
|full/ram/ulmdb/durable_batch64/throughput|records/s|986,136.534|957,157.145|-2.94%|753,443.239|+27.04%|faster / better|0.00%|
|full/ram/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|729,088.000|-9104.49%|slower / worse|98.91%|
|full/ram/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|733,184.000|-9053.07%|slower / worse|98.91%|
|full/ram/ulmdb/footprint/after/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/ram/ulmdb/footprint/after/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/after/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/after/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|352,256.000|-18951.16%|slower / worse|99.48%|
|full/ram/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,108,864.000|+0.00%|356,352.000|-18732.18%|slower / worse|99.47%|
|full/ram/ulmdb/footprint/before/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|full/ram/ulmdb/footprint/before/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99|ns|435.081|434.658|-0.10%|784.859|+44.62%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,187,662.871|3,124,644.868|-1.98%|2,124,961.847|+47.04%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|122.535|121.566|-0.79%|624.556|+80.54%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,586,796.280|11,641,370.361|+0.47%|1,999,025.140|+482.35%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99|ns|121.811|122.559|+0.61%|630.143|+80.55%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,623,137.382|21,382,850.517|-1.11%|3,871,080.484|+452.37%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|429.349|425.426|-0.91%|766.387|+44.49%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,497,573.720|5,449,685.127|-0.87%|2,620,830.141|+107.94%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.809|41.281|-1.26%|47.893|+13.81%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|100,878,930.934|100,818,514.820|-0.06%|95,363,770.876|+5.72%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99|ns|533.333|530.385|-0.55%|1,258.493|+57.86%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,492,006.082|2,481,891.753|-0.41%|1,222,107.092|+103.08%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|143.468|141.619|-1.29%|1,112.726|+87.27%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,360,619.719|9,392,112.919|+0.34%|1,116,249.221|+741.40%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99|ns|142.812|141.679|-0.79%|1,077.411|+86.85%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,171,190.921|17,308,473.085|+0.80%|2,214,079.752|+681.75%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|525.281|513.784|-2.19%|1,254.272|+59.04%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,279,244.909|4,339,401.251|+1.41%|1,539,744.938|+181.83%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.268|41.140|-0.31%|43.908|+6.30%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|97,424,359.017|97,172,775.242|-0.26%|94,926,956.061|+2.37%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99|ns|199.917|205.665|+2.88%|331.943|+38.04%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/throughput|queries/s|7,541,900.411|7,401,734.989|-1.86%|4,121,502.389|+79.59%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99|ns|201.539|202.941|+0.70%|340.431|+40.39%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/throughput|queries/s|7,571,764.419|7,440,030.722|-1.74%|3,924,041.555|+89.60%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99|ns|201.550|211.260|+4.82%|336.107|+37.15%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/throughput|queries/s|7,224,475.214|7,093,263.051|-1.82%|3,993,381.864|+77.63%|faster / better|0.00%|
|full/ram/ulmdb/reopen/complete_cold_check_ns|ns|53,552,081.522|53,105,089.051|-0.83%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/environment_drop_ns|ns|89,823.788|86,882.887|-3.27%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/experimental_reenable_ns|ns|2,087.057|2,125.256|+1.83%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/open_ns|ns|43,169,085.428|42,603,602.427|-1.31%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/validation_ns|ns|6,123,917.668|6,217,550.624|+1.53%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/setup/create|ns|47,289,576.384|47,226,436.834|-0.13%|123,054.575|-38278.45%|slower / worse|99.74%|
|millions/disk/lmdb/close_reopen_ns|ns|3,840,144.834|3,840,144.834|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/load/throughput|operations/s|1,522,677.986|1,522,677.986|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/read/throughput|operations/s|1,704,468.902|1,704,468.902|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/setup_ns|ns|1,655,265.379|1,655,265.379|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/allocated_bytes|bytes|34,648,064.000|34,648,064.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/apparent_bytes|bytes|34,652,160.000|34,652,160.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/allocated_bytes|bytes|34,648,064.000|34,648,064.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/apparent_bytes|bytes|34,652,160.000|34,652,160.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/update/throughput|operations/s|128,260.031|128,260.031|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/close_reopen_ns|ns|9,134,084.374|9,134,084.374|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/load/throughput|operations/s|829,450.862|829,450.862|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/read/throughput|operations/s|465,251.101|465,251.101|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/setup_ns|ns|2,515,565.601|2,515,565.601|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/allocated_bytes|bytes|25,320,106.593|25,320,106.593|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/apparent_bytes|bytes|25,317,376.000|25,317,376.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/files|files|1.000|1.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/allocated_bytes|bytes|33,106,602.610|33,106,602.610|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/apparent_bytes|bytes|33,099,896.000|33,099,896.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/files|files|3.000|3.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/update/throughput|operations/s|58,739.188|58,739.188|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/ulmdb/close_reopen_ns|ns|7,061,978,065.283|6,957,078,605.035|-1.49%|3,840,144.834|-181067.09%|slower / worse|99.94%|
|millions/disk/ulmdb/load/throughput|operations/s|1,404,162.100|1,415,627.982|+0.82%|1,522,677.986|-7.03%|slower / worse|7.56%|
|millions/disk/ulmdb/read/throughput|operations/s|2,809,320.152|2,776,897.863|-1.15%|1,704,468.902|+62.92%|faster / better|0.00%|
|millions/disk/ulmdb/setup_ns|ns|398,977,312.399|427,430,356.885|+7.13%|1,655,265.379|-25722.47%|slower / worse|99.61%|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|bytes|268,435,456.000|268,436,821.326|+0.00%|34,648,064.000|-674.75%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_after_close_reopen/apparent_bytes|bytes|268,435,456.000|268,435,456.000|+0.00%|34,652,160.000|-674.66%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_after_close_reopen/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|millions/disk/ulmdb/storage_before_close/allocated_bytes|bytes|268,435,456.000|268,436,821.326|+0.00%|34,648,064.000|-674.75%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_before_close/apparent_bytes|bytes|268,435,456.000|268,435,456.000|+0.00%|34,652,160.000|-674.66%|slower / worse|87.09%|
|millions/disk/ulmdb/storage_before_close/files|files|2.000|2.000|+0.00%|2.000|-0.00%|roughly on par|0.00%|
|millions/disk/ulmdb/update/throughput|operations/s|146,643.984|145,321.167|-0.90%|128,260.031|+13.30%|faster / better|0.00%|

Complete confidence intervals, all process values/signs and insufficient-repeat flags remain in the unchanged original comparison JSON/Markdown.



[Final source/build/fault qualification](benchmark/work/optimization-20261105/next-witness-dsync/retained6-v1/build-qualification-v4-receipt.json), [independent raw/source/metric receipt](benchmark/work/optimization-20261105/next-witness-dsync/retained6-v1/campaign-independent-review-v1/review-receipt.json) and [decision presentation receipt](benchmark/work/optimization-20261105/next-witness-dsync/retained6-v1/campaign-independent-review-v1/decision-receipt.json). The source 473 context is built 471 plus exactly two test-only additions; all 547 source/support inputs are bound. Initial/retained ELFs and native contexts remain unchanged; only the verified candidate archive hash differs in parsed native metadata.

## Typed-KV shadow: diagnostic correctness and cost control PASS; no issuer

### Typed-KV shadow cost: fixed14 outcome

All2 smoke +12 complete original full-suite processes passed source/native/cold/oracles/raw/WAL guards. All12 timed boundary observations were quiet; no retry, exclusion or normalization. All four T runs completed11/11 predictions matching the original full auditor with zero unavailable/mismatch. Original513Write/11Collect, WriteRetire0 and every normalized WAL payload/Copy binding remain exact. This measures the extra maintenance cost while the original full audits still run; it proves no issuer, audit elimination or net production speedup.

|ULMDB cost metric|OFF B ns/commit|TRACK T ns/commit|Added cost|Four paired changes|
|---|---:|---:|---:|---|
|batch1 aggregate|15,952.066|16,302.688|+2.198%|+0.13%, +2.99%, +2.43%, +3.28%|
|batch1 combined|13,238.106|13,551.817|+2.370%|+1.80%, +2.21%, +2.39%, +3.08%|
|batch64 aggregate|65,231.215|68,297.887|+4.701%|+4.24%, +4.08%, +5.76%, +4.72%|
|batch64 combined|64,663.763|67,816.216|+4.875%|+4.57%, +4.80%, +5.60%, +4.54%|

Batch64 mean process-pooled p99 also rises7.298% and sampled max7.216%, all four pairs adverse. Singles process-pooled p99 rises0.959% with mixed signs. Every raw global pool and maximum is retained in the complete report. The contemporaneous Native B→T singles cost also rises2.218% aggregate/2.900% combined in all four pairs; those observations stay unadjusted and do not establish how much of the ULMDB cost is causal.

All six arrays together reserve33792B; total bounded metadata charge38400B includes state and the conservative4096B stack envelope. Allocator slack/RSS is reported separately. All T runs begin with zero seed orphans and finish with KV live35/dead755. `known` reports prediction coverage, not a full seed-validity certificate: the structural seed walk alone does not validate every key, zero tail or semantic bound. The original full auditor remains mandatory. This original full fixture supports the narrow typed path; it does not prove million-load coverage or general alias correctness.

The narrow shadow is far less costly than the rejected generic tracker, but it still has recurring write and batch-tail costs. Any next issuer must independently prove event authority, conservative fallback, quotas and unchanged corruption/reopen behavior, then demonstrate a net gain under the complete benchmark inventory. Retained6 stays published.

[Complete239 metrics, all13 reads, native controls, tails, creation/cold/resource values](benchmark/work/optimization-20261105/next-typed-kv-ledger/private-v1/controls-v1/analysis-v1/REPORT.md); [machine-readable metrics and every pair](benchmark/work/optimization-20261105/next-typed-kv-ledger/private-v1/controls-v1/analysis-v1/analysis.json); [independent arithmetic verification](benchmark/work/optimization-20261105/next-typed-kv-ledger/private-v1/controls-v1/analysis-v1/independent-arithmetic-check.json).


### Typed-KV shadow: complete fixed14 cost control

All12 full observations remain. P/B/T are frozen retained6 and one same-ELF shadow OFF/ON. Tables use arithmetic means of four process values, including mean process rates; cost and rate percentage changes keep their own directions. P→B is compiled/control context, B→T is added shadow cost. Native variation is retained, never normalized or subtracted. No performance promotion, issuer or CPU-fraction claim.

Observed quiet boundaries are not continuous isolation. Unqualified tags: none observed. No observations excluded.

#### Principal writes directly versus local LMDB

|Batch|Arm|ULMDB aggregate/s|Native aggregate/s|ULMDB native advantage|ULMDB combined/s|Native combined/s|ULMDB native advantage|

|---|---|---:|---:|---:|---:|---:|---:|

|1|P|63,365.27|125,992.40|-49.71%|76,232.99|133,185.85|-42.76%|

|1|B|62,691.98|135,631.10|-53.78%|75,540.76|139,055.74|-45.68%|

|1|T|61,343.30|132,699.35|-53.77%|73,793.37|135,149.54|-45.40%|

|64|P|987,525.83|743,871.49|32.75%|996,176.79|756,580.95|31.67%|

|64|B|981,143.01|758,334.04|29.38%|989,749.65|764,965.65|29.38%|

|64|T|937,086.51|753,070.18|24.44%|943,734.49|760,650.59|24.07%|

#### All13 reads and native controls

| Metric | P | B | T | P→B % | signs | B→T % | signs |
|---|---:|---:|---:|---:|:---:|---:|:---:|
| read.ipv4_lpm_hits.lmdb.mean_ns_per_operation | 467.36 | 475.35 | 461.69 | 1.71 | ++++ | -2.87 | ---- |
| read.ipv4_lpm_hits.lmdb.mean_trial_p99_ns | 795.67 | 805.75 | 778.11 | 1.27 | +-++ | -3.43 | -+-- |
| read.ipv4_lpm_hits.lmdb.operations_per_second | 2,139,706.24 | 2,103,723.01 | 2,167,041.47 | -1.68 | ---- | 3.01 | ++++ |
| read.ipv4_lpm_hits.lmdb.pooled_sampled_p99_ns | 794.50 | 806.25 | 779.00 | 1.48 | 0-++ | -3.38 | -+-- |
| read.ipv4_lpm_hits.lmdb.sampled_max_ns | 5,893.50 | 5,969.00 | 9,761.00 | 1.28 | --++ | 63.53 | +-++ |
| read.ipv4_lpm_hits.ulmdb.mean_ns_per_operation | 311.76 | 312.11 | 312.47 | 0.11 | -++- | 0.11 | +--+ |
| read.ipv4_lpm_hits.ulmdb.mean_trial_p99_ns | 428.03 | 432.62 | 427.05 | 1.07 | ++++ | -1.29 | ---- |
| read.ipv4_lpm_hits.ulmdb.operations_per_second | 3,207,684.45 | 3,203,989.21 | 3,200,332.98 | -0.12 | +--+ | -0.11 | -++- |
| read.ipv4_lpm_hits.ulmdb.pooled_sampled_p99_ns | 428.50 | 431.00 | 428.25 | 0.58 | 00+0 | -0.64 | 0--0 |
| read.ipv4_lpm_hits.ulmdb.sampled_max_ns | 19,727.25 | 30,215.00 | 10,227.50 | 53.16 | --+- | -66.15 | +--+ |
| read.ipv4_lpm_inside_gap_misses.lmdb.mean_ns_per_operation | 494.42 | 496.98 | 506.77 | 0.52 | --+- | 1.97 | ++-+ |
| read.ipv4_lpm_inside_gap_misses.lmdb.mean_trial_p99_ns | 631.25 | 599.70 | 602.38 | -5.00 | --+- | 0.45 | -+-+ |
| read.ipv4_lpm_inside_gap_misses.lmdb.operations_per_second | 2,024,629.94 | 2,015,769.35 | 1,977,760.55 | -0.44 | ++-+ | -1.89 | --+- |
| read.ipv4_lpm_inside_gap_misses.lmdb.pooled_sampled_p99_ns | 633.75 | 603.50 | 611.00 | -4.77 | --+- | 1.24 | -+-+ |
| read.ipv4_lpm_inside_gap_misses.lmdb.sampled_max_ns | 5,267.50 | 5,888.50 | 5,460.00 | 11.79 | ++++ | -7.28 | +--+ |
| read.ipv4_lpm_inside_gap_misses.ulmdb.mean_ns_per_operation | 85.92 | 85.89 | 86.12 | -0.03 | -++- | 0.27 | ++-+ |
| read.ipv4_lpm_inside_gap_misses.ulmdb.mean_trial_p99_ns | 122.53 | 122.05 | 121.31 | -0.40 | -++- | -0.60 | -0-0 |
| read.ipv4_lpm_inside_gap_misses.ulmdb.operations_per_second | 11,639,191.51 | 11,643,364.14 | 11,612,150.62 | 0.04 | +--+ | -0.27 | --+- |
| read.ipv4_lpm_inside_gap_misses.ulmdb.pooled_sampled_p99_ns | 128.25 | 125.75 | 121.00 | -1.95 | -0+0 | -3.78 | 00-0 |
| read.ipv4_lpm_inside_gap_misses.ulmdb.sampled_max_ns | 3,487.00 | 4,694.00 | 4,658.75 | 34.61 | +++- | -0.75 | +--+ |
| read.ipv4_lpm_misses.lmdb.mean_ns_per_operation | 260.18 | 257.67 | 260.80 | -0.96 | -++- | 1.21 | ++-+ |
| read.ipv4_lpm_misses.lmdb.mean_trial_p99_ns | 633.28 | 590.72 | 595.80 | -6.72 | --+- | 0.86 | -+-+ |
| read.ipv4_lpm_misses.lmdb.operations_per_second | 3,853,671.36 | 3,886,416.40 | 3,841,449.92 | 0.85 | +--+ | -1.16 | --+- |
| read.ipv4_lpm_misses.lmdb.pooled_sampled_p99_ns | 633.75 | 596.00 | 603.50 | -5.96 | -0+- | 1.26 | -+-+ |
| read.ipv4_lpm_misses.lmdb.sampled_max_ns | 23,567.25 | 7,807.75 | 5,979.00 | -66.87 | ---+ | -23.42 | +--- |
| read.ipv4_lpm_misses.ulmdb.mean_ns_per_operation | 45.86 | 46.34 | 46.21 | 1.03 | -+++ | -0.28 | +--+ |
| read.ipv4_lpm_misses.ulmdb.mean_trial_p99_ns | 121.64 | 121.41 | 121.75 | -0.19 | -+++ | 0.28 | 0-+- |
| read.ipv4_lpm_misses.ulmdb.operations_per_second | 21,803,971.91 | 21,583,299.04 | 21,641,144.66 | -1.01 | +--- | 0.27 | -++- |
| read.ipv4_lpm_misses.ulmdb.pooled_sampled_p99_ns | 125.50 | 125.75 | 121.00 | 0.20 | -++0 | -3.78 | 0-00 |
| read.ipv4_lpm_misses.ulmdb.sampled_max_ns | 5,222.25 | 4,799.25 | 5,327.50 | -8.10 | ---+ | 11.01 | ++-+ |
| read.ipv4_lpm_mixed.lmdb.mean_ns_per_operation | 375.00 | 378.80 | 374.38 | 1.01 | -++- | -1.17 | ---+ |
| read.ipv4_lpm_mixed.lmdb.mean_trial_p99_ns | 771.14 | 790.03 | 764.28 | 2.45 | ++++ | -3.26 | ---- |
| read.ipv4_lpm_mixed.lmdb.operations_per_second | 2,667,341.10 | 2,640,779.63 | 2,671,791.00 | -1.00 | +--+ | 1.17 | +++- |
| read.ipv4_lpm_mixed.lmdb.pooled_sampled_p99_ns | 771.75 | 793.75 | 766.25 | 2.85 | ++++ | -3.46 | ---- |
| read.ipv4_lpm_mixed.lmdb.sampled_max_ns | 15,361.75 | 22,996.25 | 64,412.75 | 49.70 | -++- | 180.10 | ++++ |
| read.ipv4_lpm_mixed.ulmdb.mean_ns_per_operation | 180.53 | 181.67 | 181.21 | 0.63 | +++- | -0.25 | +--+ |
| read.ipv4_lpm_mixed.ulmdb.mean_trial_p99_ns | 419.84 | 423.83 | 418.30 | 0.95 | -+++ | -1.31 | ---- |
| read.ipv4_lpm_mixed.ulmdb.operations_per_second | 5,539,281.57 | 5,504,850.71 | 5,518,462.96 | -0.62 | ---+ | 0.25 | -++- |
| read.ipv4_lpm_mixed.ulmdb.pooled_sampled_p99_ns | 420.50 | 423.25 | 420.50 | 0.65 | 00+0 | -0.65 | 0--0 |
| read.ipv4_lpm_mixed.ulmdb.sampled_max_ns | 6,249.25 | 11,191.25 | 4,947.00 | 79.08 | -+++ | -55.80 | ---- |
| read.ipv4_lpm_outside_cover_misses.lmdb.mean_ns_per_operation | 10.05 | 13.41 | 11.17 | 33.42 | ++++ | -16.69 | ---+ |
| read.ipv4_lpm_outside_cover_misses.lmdb.mean_trial_p99_ns | 50.02 | 41.00 | 45.64 | -18.03 | ---- | 11.32 | ++0+ |
| read.ipv4_lpm_outside_cover_misses.lmdb.operations_per_second | 99,535,924.00 | 76,850,421.45 | 90,690,942.30 | -22.79 | ---- | 18.01 | +++- |
| read.ipv4_lpm_outside_cover_misses.lmdb.pooled_sampled_p99_ns | 50.00 | 41.00 | 45.50 | -18.00 | ---- | 10.98 | ++00 |
| read.ipv4_lpm_outside_cover_misses.lmdb.sampled_max_ns | 1,172.75 | 3,764.75 | 3,740.25 | 221.02 | -+++ | -0.65 | +--+ |
| read.ipv4_lpm_outside_cover_misses.ulmdb.mean_ns_per_operation | 9.92 | 9.98 | 9.91 | 0.57 | +++- | -0.71 | ---+ |
| read.ipv4_lpm_outside_cover_misses.ulmdb.mean_trial_p99_ns | 41.30 | 41.28 | 41.30 | -0.04 | -++- | 0.04 | +0-0 |
| read.ipv4_lpm_outside_cover_misses.ulmdb.operations_per_second | 100,815,869.94 | 100,259,244.76 | 100,957,839.55 | -0.55 | ---+ | 0.70 | +++- |
| read.ipv4_lpm_outside_cover_misses.ulmdb.pooled_sampled_p99_ns | 43.25 | 41.00 | 43.25 | -5.20 | -000 | 5.49 | +000 |
| read.ipv4_lpm_outside_cover_misses.ulmdb.sampled_max_ns | 9,055.00 | 1,556.00 | 2,297.50 | -82.82 | -+-- | 47.65 | --+0 |
| read.ipv6_lpm_hits.lmdb.mean_ns_per_operation | 800.89 | 828.01 | 841.44 | 3.39 | ++++ | 1.62 | ++-+ |
| read.ipv6_lpm_hits.lmdb.mean_trial_p99_ns | 1,250.33 | 1,269.17 | 1,284.97 | 1.51 | -+++ | 1.24 | +++- |
| read.ipv6_lpm_hits.lmdb.operations_per_second | 1,248,744.26 | 1,207,847.01 | 1,188,584.99 | -3.28 | ---- | -1.59 | --+- |
| read.ipv6_lpm_hits.lmdb.pooled_sampled_p99_ns | 1,247.50 | 1,267.50 | 1,285.00 | 1.60 | -+++ | 1.38 | +++- |
| read.ipv6_lpm_hits.lmdb.sampled_max_ns | 20,887.00 | 20,812.00 | 18,387.50 | -0.36 | -++- | -11.65 | +--+ |
| read.ipv6_lpm_hits.ulmdb.mean_ns_per_operation | 405.04 | 394.93 | 395.88 | -2.49 | ---- | 0.24 | -+++ |
| read.ipv6_lpm_hits.ulmdb.mean_trial_p99_ns | 528.20 | 521.47 | 522.23 | -1.27 | +--- | 0.15 | -0++ |
| read.ipv6_lpm_hits.ulmdb.operations_per_second | 2,468,928.26 | 2,532,081.67 | 2,526,019.13 | 2.56 | ++++ | -0.24 | +--- |
| read.ipv6_lpm_hits.ulmdb.pooled_sampled_p99_ns | 531.00 | 521.00 | 523.50 | -1.88 | 0--- | 0.48 | -0++ |
| read.ipv6_lpm_hits.ulmdb.sampled_max_ns | 25,744.00 | 10,109.25 | 19,995.50 | -60.73 | ---- | 97.79 | ++-- |
| read.ipv6_lpm_inside_gap_misses.lmdb.mean_ns_per_operation | 880.55 | 920.74 | 933.58 | 4.56 | ++++ | 1.39 | ++-+ |
| read.ipv6_lpm_inside_gap_misses.lmdb.mean_trial_p99_ns | 1,107.89 | 1,120.09 | 1,142.83 | 1.10 | -+++ | 2.03 | +--+ |
| read.ipv6_lpm_inside_gap_misses.lmdb.operations_per_second | 1,135,995.43 | 1,086,382.38 | 1,071,897.44 | -4.37 | ---- | -1.33 | --+- |
| read.ipv6_lpm_inside_gap_misses.lmdb.pooled_sampled_p99_ns | 1,099.75 | 1,114.75 | 1,139.50 | 1.36 | 0+++ | 2.22 | +0-+ |
| read.ipv6_lpm_inside_gap_misses.lmdb.sampled_max_ns | 39,688.25 | 11,752.50 | 11,531.75 | -70.39 | ---- | -1.88 | +-++ |
| read.ipv6_lpm_inside_gap_misses.ulmdb.mean_ns_per_operation | 106.61 | 107.37 | 106.26 | 0.72 | ++-- | -1.03 | ---- |
| read.ipv6_lpm_inside_gap_misses.ulmdb.mean_trial_p99_ns | 140.98 | 144.69 | 143.05 | 2.63 | ++-- | -1.13 | --++ |
| read.ipv6_lpm_inside_gap_misses.ulmdb.operations_per_second | 9,380,240.11 | 9,314,333.41 | 9,410,638.71 | -0.70 | --++ | 1.03 | ++++ |
| read.ipv6_lpm_inside_gap_misses.ulmdb.pooled_sampled_p99_ns | 141.00 | 148.00 | 153.00 | 4.96 | ++00 | 3.38 | +-++ |
| read.ipv6_lpm_inside_gap_misses.ulmdb.sampled_max_ns | 4,541.00 | 5,237.50 | 3,800.00 | 15.34 | ++++ | -27.45 | 0-++ |
| read.ipv6_lpm_misses.lmdb.mean_ns_per_operation | 445.80 | 466.49 | 476.00 | 4.64 | ++++ | 2.04 | +-++ |
| read.ipv6_lpm_misses.lmdb.mean_trial_p99_ns | 1,084.67 | 1,103.83 | 1,131.30 | 1.77 | ++++ | 2.49 | +-++ |
| read.ipv6_lpm_misses.lmdb.operations_per_second | 2,243,827.80 | 2,144,232.18 | 2,103,320.53 | -4.44 | ---- | -1.91 | -+-- |
| read.ipv6_lpm_misses.lmdb.pooled_sampled_p99_ns | 1,084.75 | 1,102.00 | 1,127.25 | 1.59 | ++-+ | 2.29 | +-++ |
| read.ipv6_lpm_misses.lmdb.sampled_max_ns | 37,784.25 | 14,540.00 | 21,268.00 | -61.52 | ---- | 46.27 | ++-+ |
| read.ipv6_lpm_misses.ulmdb.mean_ns_per_operation | 58.13 | 57.78 | 57.46 | -0.59 | +--- | -0.56 | -++- |
| read.ipv6_lpm_misses.ulmdb.mean_trial_p99_ns | 143.48 | 143.30 | 141.44 | -0.13 | +-+- | -1.30 | -+-- |
| read.ipv6_lpm_misses.ulmdb.operations_per_second | 17,204,219.27 | 17,307,185.66 | 17,402,502.97 | 0.60 | -+++ | 0.55 | +--+ |
| read.ipv6_lpm_misses.ulmdb.pooled_sampled_p99_ns | 155.50 | 145.75 | 145.50 | -6.27 | -0-- | -0.17 | -0-+ |
| read.ipv6_lpm_misses.ulmdb.sampled_max_ns | 7,950.25 | 46,826.25 | 4,826.75 | 488.99 | ++-- | -89.69 | --++ |
| read.ipv6_lpm_mixed.lmdb.mean_ns_per_operation | 637.10 | 660.31 | 671.59 | 3.64 | ++++ | 1.71 | ++-+ |
| read.ipv6_lpm_mixed.lmdb.mean_trial_p99_ns | 1,244.81 | 1,261.58 | 1,273.70 | 1.35 | -+++ | 0.96 | +++- |
| read.ipv6_lpm_mixed.lmdb.operations_per_second | 1,569,774.07 | 1,514,576.25 | 1,489,233.18 | -3.52 | ---- | -1.67 | --+- |
| read.ipv6_lpm_mixed.lmdb.pooled_sampled_p99_ns | 1,242.50 | 1,264.50 | 1,272.50 | 1.77 | -+++ | 0.63 | +0+- |
| read.ipv6_lpm_mixed.lmdb.sampled_max_ns | 27,599.75 | 15,634.75 | 25,636.25 | -43.35 | -+-- | 63.97 | ++++ |
| read.ipv6_lpm_mixed.ulmdb.mean_ns_per_operation | 232.63 | 227.01 | 227.51 | -2.42 | ---- | 0.22 | ++-+ |
| read.ipv6_lpm_mixed.ulmdb.mean_trial_p99_ns | 516.62 | 508.80 | 510.52 | -1.52 | ---- | 0.34 | -+++ |
| read.ipv6_lpm_mixed.ulmdb.operations_per_second | 4,298,629.39 | 4,405,090.24 | 4,395,393.30 | 2.48 | ++++ | -0.22 | --+- |
| read.ipv6_lpm_mixed.ulmdb.pooled_sampled_p99_ns | 516.00 | 508.50 | 508.50 | -1.45 | 0--- | 0.00 | -00+ |
| read.ipv6_lpm_mixed.ulmdb.sampled_max_ns | 17,491.00 | 9,671.25 | 14,289.50 | -44.71 | --+- | 47.75 | ++-+ |
| read.ipv6_lpm_outside_cover_misses.lmdb.mean_ns_per_operation | 10.08 | 13.25 | 11.12 | 31.50 | ++++ | -16.04 | --+- |
| read.ipv6_lpm_outside_cover_misses.lmdb.mean_trial_p99_ns | 46.08 | 41.45 | 43.56 | -10.04 | ---- | 5.09 | ++-+ |
| read.ipv6_lpm_outside_cover_misses.lmdb.operations_per_second | 99,313,233.77 | 77,746,749.72 | 90,347,245.94 | -21.72 | ---- | 16.21 | ++-+ |
| read.ipv6_lpm_outside_cover_misses.lmdb.pooled_sampled_p99_ns | 50.00 | 43.25 | 47.75 | -13.50 | ---0 | 10.40 | ++00 |
| read.ipv6_lpm_outside_cover_misses.lmdb.sampled_max_ns | 4,693.75 | 3,263.75 | 1,102.75 | -30.47 | -+-+ | -66.21 | ---- |
| read.ipv6_lpm_outside_cover_misses.ulmdb.mean_ns_per_operation | 10.26 | 10.27 | 10.34 | 0.11 | +-+- | 0.68 | +++- |
| read.ipv6_lpm_outside_cover_misses.ulmdb.mean_trial_p99_ns | 41.30 | 41.28 | 41.44 | -0.04 | 0-0+ | 0.38 | +0+- |
| read.ipv6_lpm_outside_cover_misses.ulmdb.operations_per_second | 97,506,733.45 | 97,403,768.58 | 96,751,690.58 | -0.11 | -+-+ | -0.67 | ---+ |
| read.ipv6_lpm_outside_cover_misses.ulmdb.pooled_sampled_p99_ns | 41.00 | 41.00 | 43.25 | 0.00 | 0000 | 5.49 | 00+0 |
| read.ipv6_lpm_outside_cover_misses.ulmdb.sampled_max_ns | 2,169.25 | 53.25 | 2,332.25 | -97.55 | +--+ | 4,279.81 | +++- |
| read.ordered_kv_hits.lmdb.mean_ns_per_operation | 239.44 | 241.50 | 242.44 | 0.86 | ++++ | 0.39 | -+-+ |
| read.ordered_kv_hits.lmdb.mean_trial_p99_ns | 339.11 | 339.25 | 341.58 | 0.04 | ++-+ | 0.69 | -+++ |
| read.ordered_kv_hits.lmdb.operations_per_second | 4,176,375.36 | 4,140,743.03 | 4,124,845.46 | -0.85 | ---- | -0.38 | +-+- |
| read.ordered_kv_hits.lmdb.pooled_sampled_p99_ns | 340.25 | 340.50 | 345.50 | 0.07 | 000+ | 1.47 | 0+0+ |
| read.ordered_kv_hits.lmdb.sampled_max_ns | 15,922.75 | 19,356.50 | 11,424.50 | 21.57 | --++ | -40.98 | -+-- |
| read.ordered_kv_hits.ulmdb.mean_ns_per_operation | 132.19 | 133.84 | 136.14 | 1.24 | -++- | 1.73 | ++-+ |
| read.ordered_kv_hits.ulmdb.mean_trial_p99_ns | 200.83 | 199.70 | 202.44 | -0.56 | -++- | 1.37 | ++-+ |
| read.ordered_kv_hits.ulmdb.operations_per_second | 7,564,714.75 | 7,475,478.50 | 7,347,485.44 | -1.18 | +--+ | -1.71 | --+- |
| read.ordered_kv_hits.ulmdb.pooled_sampled_p99_ns | 200.25 | 200.75 | 200.75 | 0.25 | +0+0 | 0.00 | 0+-+ |
| read.ordered_kv_hits.ulmdb.sampled_max_ns | 16,589.00 | 11,153.75 | 10,512.25 | -32.76 | -++- | -5.75 | ---+ |
| read.ordered_kv_misses.lmdb.mean_ns_per_operation | 248.36 | 253.80 | 253.77 | 2.19 | ++++ | -0.01 | ---+ |
| read.ordered_kv_misses.lmdb.mean_trial_p99_ns | 340.17 | 348.67 | 343.52 | 2.50 | ++++ | -1.48 | -+-- |
| read.ordered_kv_misses.lmdb.operations_per_second | 4,026,400.16 | 3,940,109.70 | 3,940,568.04 | -2.14 | ---- | 0.01 | +++- |
| read.ordered_kv_misses.lmdb.pooled_sampled_p99_ns | 340.75 | 353.25 | 343.25 | 3.67 | +0+0 | -2.83 | -0-0 |
| read.ordered_kv_misses.lmdb.sampled_max_ns | 12,050.25 | 10,833.25 | 9,685.75 | -10.10 | -+-- | -10.59 | +++- |
| read.ordered_kv_misses.ulmdb.mean_ns_per_operation | 131.71 | 132.54 | 134.05 | 0.63 | -++- | 1.14 | ++-+ |
| read.ordered_kv_misses.ulmdb.mean_trial_p99_ns | 199.80 | 200.00 | 201.52 | 0.10 | --+- | 0.76 | ++-+ |
| read.ordered_kv_misses.ulmdb.operations_per_second | 7,592,663.56 | 7,549,550.14 | 7,463,956.63 | -0.57 | +--+ | -1.13 | --+- |
| read.ordered_kv_misses.ulmdb.pooled_sampled_p99_ns | 200.00 | 205.50 | 201.00 | 2.75 | +++0 | -2.19 | 0+-+ |
| read.ordered_kv_misses.ulmdb.sampled_max_ns | 10,460.00 | 11,985.25 | 15,341.75 | 14.58 | -+++ | 28.01 | --++ |
| read.ordered_kv_mixed.lmdb.mean_ns_per_operation | 243.88 | 249.55 | 249.97 | 2.33 | ++++ | 0.17 | -+-+ |
| read.ordered_kv_mixed.lmdb.mean_trial_p99_ns | 341.05 | 344.91 | 346.25 | 1.13 | +++- | 0.39 | ++-- |
| read.ordered_kv_mixed.lmdb.operations_per_second | 4,100,436.72 | 4,007,193.67 | 4,000,444.37 | -2.27 | ---- | -0.17 | +-+- |
| read.ordered_kv_mixed.lmdb.pooled_sampled_p99_ns | 343.00 | 346.00 | 345.75 | 0.87 | +0+- | -0.07 | 0+-0 |
| read.ordered_kv_mixed.lmdb.sampled_max_ns | 8,729.25 | 7,133.75 | 13,685.75 | -18.28 | -+-- | 91.85 | +++- |
| read.ordered_kv_mixed.ulmdb.mean_ns_per_operation | 138.07 | 139.01 | 140.95 | 0.68 | -++- | 1.39 | ++-+ |
| read.ordered_kv_mixed.ulmdb.mean_trial_p99_ns | 204.47 | 203.09 | 205.98 | -0.67 | -++- | 1.42 | ++-+ |
| read.ordered_kv_mixed.ulmdb.operations_per_second | 7,242,945.46 | 7,196,994.02 | 7,098,612.22 | -0.63 | +--+ | -1.37 | --+- |
| read.ordered_kv_mixed.ulmdb.pooled_sampled_p99_ns | 205.75 | 202.75 | 207.75 | -1.46 | --+- | 2.47 | ++-+ |
| read.ordered_kv_mixed.ulmdb.sampled_max_ns | 17,565.75 | 21,934.50 | 10,375.25 | 24.87 | +++- | -52.70 | ---- |

#### All writes: aggregate, sampled, combined and tails

| Metric | P | B | T | P→B % | signs | B→T % | signs |
|---|---:|---:|---:|---:|:---:|---:|:---:|
| write.batch1.lmdb.aggregate_ns_per_commit | 8,046.78 | 7,372.95 | 7,536.46 | -8.37 | -+-- | 2.22 | ++++ |
| write.batch1.lmdb.aggregate_records_per_second | 125,992.40 | 135,631.10 | 132,699.35 | 7.65 | +-++ | -2.16 | ---- |
| write.batch1.lmdb.combined_ns_per_commit | 7,538.66 | 7,191.64 | 7,400.17 | -4.60 | ---+ | 2.90 | ++++ |
| write.batch1.lmdb.combined_records_per_second | 133,185.85 | 139,055.74 | 135,149.54 | 4.41 | +++- | -2.81 | ---- |
| write.batch1.lmdb.mean_trial_sampled_p99_ns | 8,306.75 | 8,061.83 | 8,918.27 | -2.95 | ---+ | 10.62 | +++- |
| write.batch1.lmdb.pooled_sampled_p99_ns | 12,037.75 | 9,365.25 | 10,517.25 | -22.20 | ---- | 12.30 | +++- |
| write.batch1.lmdb.sampled_max_ns | 12,220.75 | 11,624.50 | 18,400.25 | -4.88 | ---+ | 58.29 | +++- |
| write.batch1.lmdb.sampled_ns_per_commit | 7,030.54 | 7,010.33 | 7,263.88 | -0.29 | ---+ | 3.62 | ++++ |
| write.batch1.lmdb.sampled_records_per_second | 142,238.47 | 142,667.33 | 137,695.31 | 0.30 | +++- | -3.49 | ---- |
| write.batch1.ulmdb.aggregate_ns_per_commit | 15,781.70 | 15,952.07 | 16,302.69 | 1.08 | ++++ | 2.20 | ++++ |
| write.batch1.ulmdb.aggregate_records_per_second | 63,365.27 | 62,691.98 | 61,343.30 | -1.06 | ---- | -2.15 | ---- |
| write.batch1.ulmdb.combined_ns_per_commit | 13,118.20 | 13,238.11 | 13,551.82 | 0.91 | ++++ | 2.37 | ++++ |
| write.batch1.ulmdb.combined_records_per_second | 76,232.99 | 75,540.76 | 73,793.37 | -0.91 | ---- | -2.31 | ---- |
| write.batch1.ulmdb.mean_trial_sampled_p99_ns | 11,735.20 | 11,834.69 | 12,059.00 | 0.85 | -+++ | 1.90 | +-+- |
| write.batch1.ulmdb.pooled_sampled_p99_ns | 11,822.25 | 14,340.00 | 14,477.50 | 21.30 | -+++ | 0.96 | +-+- |
| write.batch1.ulmdb.sampled_max_ns | 21,591.00 | 16,010.25 | 16,105.50 | -25.85 | -+++ | 0.59 | +--- |
| write.batch1.ulmdb.sampled_ns_per_commit | 10,454.71 | 10,524.15 | 10,800.95 | 0.66 | -+++ | 2.63 | ++++ |
| write.batch1.ulmdb.sampled_records_per_second | 95,667.14 | 95,029.68 | 92,586.50 | -0.67 | +--- | -2.57 | ---- |
| write.batch64.lmdb.aggregate_ns_per_commit | 86,044.38 | 84,400.14 | 84,995.48 | -1.91 | ---- | 0.71 | +--+ |
| write.batch64.lmdb.aggregate_records_per_second | 743,871.49 | 758,334.04 | 753,070.18 | 1.94 | ++++ | -0.69 | -++- |
| write.batch64.lmdb.combined_ns_per_commit | 84,598.88 | 83,672.22 | 84,148.34 | -1.10 | -+-- | 0.57 | +-++ |
| write.batch64.lmdb.combined_records_per_second | 756,580.95 | 764,965.65 | 760,650.59 | 1.11 | +-++ | -0.56 | -+-- |
| write.batch64.lmdb.mean_trial_sampled_p99_ns | 91,081.23 | 92,594.52 | 91,007.55 | 1.66 | -+-+ | -1.71 | +--- |
| write.batch64.lmdb.pooled_sampled_p99_ns | 100,120.25 | 102,780.25 | 94,952.75 | 2.66 | ++-+ | -7.62 | +--- |
| write.batch64.lmdb.sampled_max_ns | 102,121.75 | 121,223.00 | 105,908.75 | 18.70 | -+-+ | -12.63 | +--- |
| write.batch64.lmdb.sampled_ns_per_commit | 83,153.38 | 82,944.30 | 83,301.21 | -0.25 | -+-+ | 0.43 | +-++ |
| write.batch64.lmdb.sampled_records_per_second | 769,891.02 | 771,726.92 | 768,387.68 | 0.24 | +-+- | -0.43 | -+-- |
| write.batch64.ulmdb.aggregate_ns_per_commit | 64,810.27 | 65,231.21 | 68,297.89 | 0.65 | ++-+ | 4.70 | ++++ |
| write.batch64.ulmdb.aggregate_records_per_second | 987,525.83 | 981,143.01 | 937,086.51 | -0.65 | --+- | -4.49 | ---- |
| write.batch64.ulmdb.combined_ns_per_commit | 64,247.37 | 64,663.76 | 67,816.22 | 0.65 | ++-+ | 4.88 | ++++ |
| write.batch64.ulmdb.combined_records_per_second | 996,176.79 | 989,749.65 | 943,734.49 | -0.65 | --+- | -4.65 | ---- |
| write.batch64.ulmdb.mean_trial_sampled_p99_ns | 129,824.59 | 130,974.77 | 138,721.14 | 0.89 | +--+ | 5.91 | ++++ |
| write.batch64.ulmdb.pooled_sampled_p99_ns | 251,658.25 | 253,907.75 | 272,438.00 | 0.89 | ---+ | 7.30 | ++++ |
| write.batch64.ulmdb.sampled_max_ns | 267,824.25 | 262,499.00 | 281,440.00 | -1.99 | ---+ | 7.22 | ++++ |
| write.batch64.ulmdb.sampled_ns_per_commit | 63,684.47 | 64,096.31 | 67,334.54 | 0.65 | ++-+ | 5.05 | ++++ |
| write.batch64.ulmdb.sampled_records_per_second | 1,004,980.91 | 998,513.95 | 950,485.71 | -0.64 | --+- | -4.81 | ---- |

All write p99/maxima are from the original separate sampled commits; aggregate per-commit tails are unavailable. Mean per-process pooled p99 and true global pools differ; all global p99/maxima remain in analysis.json.

#### Every setup and cold cost

| Metric | P | B | T | P→B % | signs | B→T % | signs |
|---|---:|---:|---:|---:|:---:|---:|:---:|
| cold.complete_cold_check_ns | 53,020,561.25 | 53,104,318.25 | 52,669,969.75 | 0.16 | ++-- | -0.82 | ---+ |
| cold.environment_drop_ns | 86,805.00 | 85,069.50 | 93,174.50 | -2.00 | ++-- | 9.53 | ++++ |
| cold.experimental_reenable_ns | 2,109.00 | 2,362.25 | 2,071.50 | 12.01 | ++-+ | -12.31 | --+- |
| cold.open_ns | 42,493,705.00 | 42,580,497.50 | 42,216,831.00 | 0.20 | ++-- | -0.85 | ---+ |
| cold.validation_ns | 6,168,057.25 | 6,218,114.75 | 6,198,723.50 | 0.81 | ++++ | -0.31 | +++- |
| creation.after_setup_ns | 977.00 | 6,224.50 | 1,072.00 | 537.10 | ++++ | -82.78 | ---- |
| creation.dataset_ns | 264,969,025.25 | 267,495,169.50 | 267,480,554.00 | 0.95 | ++++ | -0.01 | -+-+ |
| creation.fixture_load_both_ns | 9,156,873.50 | 9,042,671.00 | 9,361,244.50 | -1.25 | -+-- | 3.52 | ++++ |
| creation.lmdb_create_ns | 122,916.25 | 123,757.75 | 127,530.00 | 0.68 | -+-+ | 3.05 | +--+ |
| creation.sum_reported_setup_ns | 320,722,303.75 | 324,088,457.75 | 324,123,874.50 | 1.05 | ++++ | 0.01 | ---+ |
| creation.ulmdb_create_ns | 46,472,511.75 | 47,420,635.00 | 47,153,474.00 | 2.04 | ++-+ | -0.56 | ---+ |

Creation charges shadow allocation before fixture loading. Fixture load includes BOTH engines. Reopen uses the original full historical and selected-graph checks; reopened environments do not enable the shadow. Setup sums are reported categories, not total process wall time.

#### Every resource and ordinary accounting value

| Metric | P | B | T | P→B % | signs | B→T % | signs |
|---|---:|---:|---:|---:|:---:|---:|:---:|
| accounting.backend_after.auto_collect_below | 256.00 | 256.00 | 256.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.collect_cache.entries | 2.00 | 2.00 | 2.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.collect_cache.hits | 2.00 | 2.00 | 2.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.collect_cache.misses | 0.00 | 0.00 | 0.00 | unavailable | 0000 | unavailable | 0000 |
| accounting.backend_after.collect_cache.retained_bytes | 2,368.00 | 2,368.00 | 2,368.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.pool_kv_pages | 1,024.00 | 1,024.00 | 1,024.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.pool_metadata_bound_bytes | 13,320,224.00 | 13,320,224.00 | 13,320,224.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.pool_reused_pages | 7,722.00 | 7,722.00 | 7,722.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.pool_rounded_mapping_bytes | 8,388,608.00 | 8,388,608.00 | 8,388,608.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.pool_tiny_pages | 1,024.00 | 1,024.00 | 1,024.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.private_admitted_capacity_hints.charged | 283,312.00 | 283,312.00 | 283,312.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.private_admitted_capacity_hints.frozen | 65,536.00 | 65,536.00 | 65,536.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.private_admitted_capacity_hints.index | 4,096.00 | 4,096.00 | 4,096.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.private_admitted_capacity_hints.packed | 212,992.00 | 212,992.00 | 212,992.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.private_current_global_budget_bytes | 53,788,640.00 | 53,788,640.00 | 53,788,640.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.private_current_global_reserved_bytes | 283,312.00 | 283,312.00 | 283,312.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.retained_frame_workspace_bytes | 528,528.00 | 528,528.00 | 528,528.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.retained_recipe_workspace_bytes | 9,216.00 | 9,216.00 | 9,216.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.sequence | 513.00 | 513.00 | 513.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.wal_capacity | 67,108,864.00 | 67,108,864.00 | 67,108,864.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_after.wal_selected_end | 2,205,392.00 | 2,205,392.00 | 2,205,392.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.auto_collect_below | 256.00 | 256.00 | 256.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.collect_cache.entries | 0.00 | 0.00 | 0.00 | unavailable | 0000 | unavailable | 0000 |
| accounting.backend_before.collect_cache.hits | 0.00 | 0.00 | 0.00 | unavailable | 0000 | unavailable | 0000 |
| accounting.backend_before.collect_cache.misses | 0.00 | 0.00 | 0.00 | unavailable | 0000 | unavailable | 0000 |
| accounting.backend_before.collect_cache.retained_bytes | 648.00 | 648.00 | 648.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.pool_kv_pages | 1,024.00 | 1,024.00 | 1,024.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.pool_metadata_bound_bytes | 13,320,224.00 | 13,320,224.00 | 13,320,224.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.pool_reused_pages | 0.00 | 0.00 | 0.00 | unavailable | 0000 | unavailable | 0000 |
| accounting.backend_before.pool_rounded_mapping_bytes | 8,388,608.00 | 8,388,608.00 | 8,388,608.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.pool_tiny_pages | 1,024.00 | 1,024.00 | 1,024.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.private_admitted_capacity_hints.charged | 283,312.00 | 283,312.00 | 283,312.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.private_admitted_capacity_hints.frozen | 65,536.00 | 65,536.00 | 65,536.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.private_admitted_capacity_hints.index | 4,096.00 | 4,096.00 | 4,096.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.private_admitted_capacity_hints.packed | 212,992.00 | 212,992.00 | 212,992.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.private_current_global_budget_bytes | 53,788,640.00 | 53,788,640.00 | 53,788,640.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.private_current_global_reserved_bytes | 283,312.00 | 283,312.00 | 283,312.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.retained_frame_workspace_bytes | 528,528.00 | 528,528.00 | 528,528.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.retained_recipe_workspace_bytes | 7,168.00 | 7,168.00 | 7,168.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.sequence | 1.00 | 1.00 | 1.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.wal_capacity | 67,108,864.00 | 67,108,864.00 | 67,108,864.00 | 0.00 | 0000 | 0.00 | 0000 |
| accounting.backend_before.wal_selected_end | 725,136.00 | 725,136.00 | 725,136.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.lmdb.allocated_file_bytes | 729,088.00 | 729,088.00 | 729,088.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.lmdb.apparent_file_bytes | 733,184.00 | 733,184.00 | 733,184.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.lmdb.files | 2.00 | 2.00 | 2.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.lmdb.mapped_pss_kib | 712.00 | 712.00 | 712.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.lmdb.mapped_rss_kib | 712.00 | 712.00 | 712.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.lmdb.mapped_virtual_kib | 524,296.00 | 524,296.00 | 524,296.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.ulmdb.allocated_file_bytes | 67,108,864.00 | 67,108,864.00 | 67,108,864.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.ulmdb.apparent_file_bytes | 67,108,864.00 | 67,108,864.00 | 67,108,864.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.ulmdb.files | 2.00 | 2.00 | 2.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.after.whole_process_vm_rss_bytes | 15,058,944.00 | 15,133,696.00 | 15,174,656.00 | 0.50 | ++++ | 0.27 | ++++ |
| resource.before.lmdb.allocated_file_bytes | 352,256.00 | 352,256.00 | 352,256.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.lmdb.apparent_file_bytes | 356,352.00 | 356,352.00 | 356,352.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.lmdb.files | 2.00 | 2.00 | 2.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.lmdb.mapped_pss_kib | 12.00 | 12.00 | 12.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.lmdb.mapped_rss_kib | 12.00 | 12.00 | 12.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.lmdb.mapped_virtual_kib | 524,296.00 | 524,296.00 | 524,296.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.ulmdb.allocated_file_bytes | 67,108,864.00 | 67,108,864.00 | 67,108,864.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.ulmdb.apparent_file_bytes | 67,108,864.00 | 67,108,864.00 | 67,108,864.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.ulmdb.files | 2.00 | 2.00 | 2.00 | 0.00 | 0000 | 0.00 | 0000 |
| resource.before.whole_process_vm_rss_bytes | 13,641,728.00 | 13,700,096.00 | 13,736,960.00 | 0.43 | ++++ | 0.27 | ++++ |

Whole-process RSS includes both engines, query/oracle/latency buffers and process heap; it is not isolated Resident RSS. Per-array payload and bounded shadow charge below include all six arrays and simultaneous old/pending state. Actual allocator/RSS slack remains separate.

#### Shadow coverage and separate bounded metadata

|Process|Seed known/orphans|Full comparisons/matches|Unavailable/mismatch|Payload bytes|Charged bytes|Last live/dead KV|

|---|---|---|---|---:|---:|---|

|bench-r1-s3-T|True/0|11/11|0/0|33792|38400|35/755|

|bench-r2-s2-T|True/0|11/11|0/0|33792|38400|35/755|

|bench-r3-s1-T|True/0|11/11|0/0|33792|38400|35/755|

|bench-r4-s2-T|True/0|11/11|0/0|33792|38400|35/755|

Each full process retains513 Writes,11 original full Collects, zero ordinary Write retirement and identical normalized selected-frame payloads/Copy-source bindings/retirement contexts. T requires11 known complete predictions matching the independently computed original full-auditor projection, zero unavailable/mismatches. This is coverage truth, not authority to skip any audit or retire anything. No numerical full-auditor read count is invented for the new shadow.

Full source/native/ELF/workload/raw/cold/resource/oracle and evidence preservation checks pass only when every original process passes. All negative values and four pair signs are retained above and in all-process-metrics.csv. No confirmation or calibration is added.

#### Next gate

Assess all measured tracking/setup/read/tail/resource costs before any ownership issuer design. A successful shadow still runs the original complete auditors and proves no fastGC or whole-engine speedup. A later issuer needs independent validity/alias/retirement/error/budget and complete workload qualification.


[All 239 metrics/every pair](benchmark/work/optimization-20261105/next-typed-kv-ledger/private-v1/controls-v1/analysis-v1/analysis.json), [all-process CSV](benchmark/work/optimization-20261105/next-typed-kv-ledger/private-v1/controls-v1/analysis-v1/all-process-metrics.csv), [independent arithmetic check](benchmark/work/optimization-20261105/next-typed-kv-ledger/private-v1/controls-v1/analysis-v1/independent-arithmetic-check.json), [qualification](benchmark/work/optimization-20261105/next-typed-kv-ledger/private-v1/qualification-v4-receipt.json), [sealed release](benchmark/work/optimization-20261105/next-typed-kv-ledger/private-v1/full-release-v5-receipt.json) and [fixed14 source/native/ELF/tool seal](benchmark/work/optimization-20261105/next-typed-kv-ledger/private-v1/screen-seal-v1.json). Nine default and ten all-feature/fault focused tests plus the canonical 11/11 full trace pass. This diagnostic leaves original full audits, retirement decisions and cadence authoritative; no root runtime change or issuer is adopted.

## Parallel controlled proof: first-cold-prefix constructor certificate

Prepare a private constructor certificate control for the first cold prefix graph. Keep the original KV validation, complete typed audits, Collect retirement decisions, WriteRetire0 and cadence initially. Freeze an allocation/birth proof before the independently computed full audit, then compare the predicted typed objects, edges and multiplicity against that auditor. Constructor provenance must be bound to the actual private Resident arena, selected lineage and generation; a structural seed walk or shadow `known` flag is not a validity certificate. Unsupported aliases, source ownership or budget admission use the unchanged fallback. No sorted-key or sorted-insertion assumption is introduced: ordinary and shuffled updates, old readers, failures and corruption keep their checks. Only a subsequent independent safety proof and complete measured net gain can justify an authoritative issuer.

**io_uring availability is separately confirmed, with no engine performance result.** The actual setup-only probe returns success (features 16383, SQ 8, CQ 16); it performs no data I/O or engine timing. A future transport control must preserve ordered data/witness fences, byte equality, error completion/drain and buffer lifetime. Availability is not evidence of faster commits. [Actual probe receipt](benchmark/work/optimization-20261105/next-io-uring-study/receipt.json).

## RW3 full-image foundation: complete39 rejected, all observations retained

### RW3 full-image foundation: REJECT promotion / HOLD after complete39

**Root decision: keep retained6 and hold this private foundation.** The new architecture reduces setup90.27%, drop+open95.73% and physical allocation88.58% against contemporary retained6; allocation is11.50% below local LMDB. Those are real improvements. They do not justify load−47.80%, updates−21.76%, RAM-single combined throughput−70.38% and RAM64 combined−51.30%. All three million write pairs and all four RAM combined write pairs are adverse. No confirmation reruns or production apply follow this clearly considerable regression.

All39 original semantic/cold/source/ELF/native/SQLite checks pass, with551 complete source inputs and483 engine snapshot entries unchanged. Independent review reproduces both237-cell comparisons and checks all24 raw files (172898304B,11544 spans,21612288 samples), exact quantiles/coverage and combined sums. Correctness qualification passed832 tests across108 groups; that is separate from performance rejection. The wrapper exit1 is the comparison screen, not an engine error.

**Availability:**37 observed-quiet boundaries, two exposed full/disk processes:013 initial/r2 and023 candidate/r3. All observations stay. Disk candidate/native and candidate/reference centers include that exposed candidate point; initial disk also includes its exposed r2. All RAM and ordinary-million target/native boundaries were observed quiet. This is not continuous-isolation proof, equivalence, a reason to exclude points or permission to normalize Native variation.

#### Complete headline: aggregate and combined both lead

Centers are geometric means of process metrics. Combined includes every original128 aggregate plus128 separately sampled commits per batch. All checkpoint work remains charged. Native advantage is positive for more throughput or lower cost; within2% status is descriptive only.

|Metric|Retained6|RW3 candidate|Change vs6|LMDB|Native advantage|Native status|Context|
|---|---:|---:|---:|---:|---:|---|---|
|ram batch1 aggregate records/s|63,057.996|62,574.543|-0.77%|135,196.400|-53.72%|slower / worse|all target/control boundary observations quiet|
|ram batch1 COMBINED records/s|76,009.357|22,516.199|-70.38%|139,244.336|-83.83%|slower / worse|all target/control boundary observations quiet|
|ram batch64 aggregate records/s|990,701.486|496,965.865|-49.84%|754,442.542|-34.13%|slower / worse|all target/control boundary observations quiet|
|ram batch64 COMBINED records/s|998,852.630|486,480.803|-51.30%|757,805.419|-35.80%|slower / worse|all target/control boundary observations quiet|
|disk batch1 aggregate records/s|15,402.150|8,776.996|-43.01%|11,051.118|-20.58%|slower / worse|C disk r3 exposed; all four observations retained|
|disk batch1 COMBINED records/s|16,282.953|6,021.058|-63.02%|11,366.353|-47.03%|slower / worse|C disk r3 exposed; all four observations retained|
|disk batch64 aggregate records/s|494,496.394|139,969.066|-71.69%|179,376.658|-21.97%|slower / worse|C disk r3 exposed; all four observations retained|
|disk batch64 COMBINED records/s|503,707.604|144,641.868|-71.28%|182,566.239|-20.77%|slower / worse|C disk r3 exposed; all four observations retained|
|Million load records/s|1,399,603.376|730,614.105|-47.80%|1,561,772.001|-53.22%|slower / worse|all target/control boundary observations quiet|
|Million reads queries/s|2,803,538.045|2,787,037.670|-0.59%|1,709,820.439|+63.00%|faster / better|all target/control boundary observations quiet|
|Million updates records/s|145,552.760|113,883.559|-21.76%|128,507.223|-11.38%|slower / worse|all target/control boundary observations quiet|
|Setup ms|401.854|39.118|-90.27%|1.068|-3564.24%|slower / worse|all target/control boundary observations quiet|
|Drop+open ms|7,083.864|302.330|-95.73%|3.828|-7798.74%|slower / worse|all target/control boundary observations quiet|
|Allocated after reopen MiB|256.000|29.242|-88.58%|33.043|+11.50%|faster / better|all target/control boundary observations quiet|
|Apparent after reopen MiB|256.000|282.702|+10.43%|33.047|-755.46%|slower / worse|all target/control boundary observations quiet|
|Files after reopen|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|all target/control boundary observations quiet|

#### Setup+load+update+reopen task budget, with limits

Sum those original four costs inside EACH process first, then take the process geometric mean; do not sum geometric means of separate metrics. This excludes timed reads, all external oracles and generation. It does not excuse a regression in any individual workload.

|Retained6 seconds|Candidate seconds|Native seconds|C cost vs6|C cost vs native|
|---:|---:|---:|---:|---:|
|15.070755|10.493659|8.426882|-30.371%|+24.526%|

The30.37% task-budget reduction comes from lifecycle savings; it is not an overall no-regression result. Candidate setup remains36.64× native and reopen78.99×. Apparent files remain282.702MiB because fixed-capacity sparse WAL length remains; two additional image/selector files bring the count to4 versus Native2. Physical allocation is29.242MiB versus33.043MiB, and is explicitly distinguished from apparent length.

#### Every read and native control

|FS/case|C rate vs6|Four paired rate changes|C rate/s|Native rate/s|Native advantage|Trial p99 vs6|Context|
|---|---:|---|---:|---:|---:|---:|---|
|disk/ipv4_lpm_hits|-0.38%|-0.45%, -0.64%, -0.10%, -0.34%|3,192,852.719|2,128,437.825|+50.01%|+0.10%|C disk r3 exposed; all four observations retained|
|disk/ipv4_lpm_inside_gap_misses|+0.21%|-0.51%, +1.30%, -0.02%, +0.10%|11,626,054.375|2,011,627.890|+477.94%|-4.98%|C disk r3 exposed; all four observations retained|
|disk/ipv4_lpm_misses|-0.72%|-1.57%, -0.57%, -0.34%, -0.38%|21,554,940.317|3,941,739.635|+446.84%|-5.50%|C disk r3 exposed; all four observations retained|
|disk/ipv4_lpm_mixed|-0.12%|+0.21%, -0.26%, -0.15%, -0.28%|5,517,564.056|2,638,488.516|+109.12%|-0.83%|C disk r3 exposed; all four observations retained|
|disk/ipv4_lpm_outside_cover_misses|-1.91%|+0.01%, -0.94%, -0.04%, -6.54%|99,001,229.536|89,917,945.567|+10.10%|+1.34%|C disk r3 exposed; all four observations retained|
|disk/ipv6_lpm_hits|+1.20%|+1.60%, +1.15%, +0.46%, +1.60%|2,505,658.666|1,239,682.044|+102.12%|-2.34%|C disk r3 exposed; all four observations retained|
|disk/ipv6_lpm_inside_gap_misses|-0.41%|+0.38%, -0.82%, +0.14%, -1.33%|9,381,297.831|1,140,361.127|+722.66%|+0.23%|C disk r3 exposed; all four observations retained|
|disk/ipv6_lpm_misses|-1.15%|-0.67%, -1.11%, -1.19%, -1.62%|17,046,827.555|2,266,887.461|+651.99%|+0.27%|C disk r3 exposed; all four observations retained|
|disk/ipv6_lpm_mixed|+1.69%|+1.70%, +1.19%, +1.99%, +1.88%|4,361,309.720|1,566,524.770|+178.41%|-2.58%|C disk r3 exposed; all four observations retained|
|disk/ipv6_lpm_outside_cover_misses|+0.66%|-0.35%, +5.31%, -1.75%, -0.41%|96,607,273.710|87,932,689.705|+9.87%|+0.37%|C disk r3 exposed; all four observations retained|
|disk/ordered_kv_hits|+4.87%|+4.15%, +2.97%, +3.60%, +8.85%|7,600,099.209|4,184,184.435|+81.64%|-1.11%|C disk r3 exposed; all four observations retained|
|disk/ordered_kv_misses|+4.46%|+4.85%, +2.81%, +3.03%, +7.21%|7,621,595.359|4,014,435.054|+89.85%|-0.95%|C disk r3 exposed; all four observations retained|
|disk/ordered_kv_mixed|+5.00%|+4.47%, +3.63%, +4.03%, +7.93%|7,320,924.406|4,106,947.228|+78.26%|-0.38%|C disk r3 exposed; all four observations retained|
|ram/ipv4_lpm_hits|-0.52%|+0.52%, -0.71%, -0.70%, -1.18%|3,193,622.415|2,132,098.833|+49.79%|+0.23%|all target/control boundary observations quiet|
|ram/ipv4_lpm_inside_gap_misses|-0.25%|+1.50%, -1.12%, -1.74%, +0.41%|11,583,523.695|1,989,566.136|+482.21%|-4.90%|all target/control boundary observations quiet|
|ram/ipv4_lpm_misses|-1.01%|+0.30%, -1.03%, -0.56%, -2.73%|21,444,939.775|3,824,374.828|+460.74%|-3.04%|all target/control boundary observations quiet|
|ram/ipv4_lpm_mixed|-0.58%|+0.02%, -0.93%, +0.06%, -1.46%|5,493,913.176|2,616,073.467|+110.01%|+0.64%|all target/control boundary observations quiet|
|ram/ipv4_lpm_outside_cover_misses|+0.26%|-0.03%, -0.01%, +1.09%, -0.03%|100,945,739.058|96,699,542.630|+4.39%|+0.00%|all target/control boundary observations quiet|
|ram/ipv6_lpm_hits|+2.02%|+3.07%, +1.77%, +1.92%, +1.31%|2,513,507.239|1,225,382.362|+105.12%|-2.31%|all target/control boundary observations quiet|
|ram/ipv6_lpm_inside_gap_misses|+0.43%|+0.69%, +0.38%, +0.03%, +0.62%|9,377,734.149|1,133,594.937|+727.26%|-0.52%|all target/control boundary observations quiet|
|ram/ipv6_lpm_misses|-0.20%|-0.23%, +2.39%, -0.99%, -1.91%|17,026,197.197|2,247,220.940|+657.66%|-0.06%|all target/control boundary observations quiet|
|ram/ipv6_lpm_mixed|+2.26%|+2.22%, +2.47%, +2.73%, +1.62%|4,376,978.412|1,548,229.230|+182.71%|-2.77%|all target/control boundary observations quiet|
|ram/ipv6_lpm_outside_cover_misses|+0.56%|+2.45%, -0.09%, -0.17%, +0.07%|97,242,053.570|95,235,440.693|+2.11%|+0.34%|all target/control boundary observations quiet|
|ram/ordered_kv_hits|+1.57%|-5.85%, +4.43%, +3.99%, +4.10%|7,534,044.360|4,189,822.731|+79.82%|+1.53%|all target/control boundary observations quiet|
|ram/ordered_kv_misses|+1.55%|-5.79%, +4.96%, +4.05%, +3.36%|7,584,892.784|4,042,242.524|+87.64%|+1.90%|all target/control boundary observations quiet|
|ram/ordered_kv_mixed|+1.63%|-4.89%, +4.00%, +3.50%, +4.20%|7,241,558.442|4,120,179.029|+75.76%|+0.89%|all target/control boundary observations quiet|

#### Sampled tails: no aggregate-only hiding

|FS/batch|Trial p99 change|Process-pooled p99 change|Global512 p99 R→C ns|Maximum R→C ns|
|---|---:|---:|---:|---:|
|disk/1|+625.31%|+2106.38%|310,991→4,282,241|572,248→10,259,977|
|disk/64|+200.83%|+152.83%|372,888→959,615|566,107→14,465,081|
|ram/1|+132.96%|+15642.26%|12,584→1,931,252|17,473→6,218,022|
|ram/64|+30.01%|+11.14%|247,210→275,334|274,973→8,749,184|

RAM-single true global p99 rises12.584µs→1.931252ms (about153.47×), maximum17.473µs→6.218022ms. Disk64 maximum rises566.107µs→14.465081ms. Trial geometric, process-pooled geometric and global pooled p99s are different statistics; all remain. The original sampled groups are small diagnostic samples, not production percentile guarantees.

#### Actual charged checkpoint work

Every full process completes4 periodic checkpoints and9583712 encoded image bytes. RAM elapsed is23.911–24.167ms, disk39.186–41.042ms. Each million process completes18 checkpoints:7 in load (104566808B) and11 in update (308661704B), total413228512B. Their charged total checkpoint wall times are1.682272/1.568224/1.498580s; no mean or universal1.68s claim replaces those three observations. Original reads trigger zero checkpoints. Periodic counters exclude genesis and recovery clearing, which remain charged in setup/open. Logical discarded spans are not physical-device bytes. All cleanup flags and per-phase/count/byte/elapsed snapshots are in [CHECKPOINTS.md](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/CHECKPOINTS.md).

#### Next evidence cycle

Profile actual capture/binding, full typed audit, slot/page export, encode/validation/hash, image I/O/sync/link, selector I/O/sync/rename, prefix punch-or-zero and old-image cleanup before another runtime implementation. Preserve the existing total clocks, all normal maintenance charges and negative controls. The intended next architecture direction is persistent changed-page COW plus bounded metadata work, subject to independent safety proof and a measured complete39 gain. No expected speedup or final-design claim follows from this source direction. Retained6 stays unchanged.

[Every237 metric vs both initial and retained6](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/FULL_TABLES.md), [all103 direct-native targets](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/NATIVE_SCORECARD.md), [all checkpoint observations](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/checkpoint-costs.json), [combined task budget](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/combined-task-budget.json), [all raw write points/offsets](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/raw-review.json), [global write pools](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/global-write-tails.json), [exact exposures](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/availability-review.json), [source/raw review receipt](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/review-receipt.json).


### RW3 candidate: all103 direct-native target observations

Root decision: REJECT promotion / HOLD the private full-image foundation. Retained6 stays accepted. Positive advantage means more throughput or lower cost. Within2% is descriptive roughly-on-par, not equivalence. Gap is required throughput increase or cost reduction.

All39 remain; observed-quiet boundary count37. Exposed process steps[13, 23]. Exact arm/engine/round context is retained separately; no native normalization, exclusion or continuous-isolation claim.

|Metric|Unit|Candidate|LMDB|Native advantage|Verdict|Required increase/cost reduction|
|---|---|---:|---:|---:|---|---:|
|full/disk/ulmdb/durable_batch1/combined_throughput|records/s|6,021.058|11,366.353|-47.03%|slower / worse|88.78%|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99|ns|4,255,126.430|328,231.060|-1196.38%|slower / worse|92.29%|
|full/disk/ulmdb/durable_batch1/sampled_p99|ns|570,090.435|200,558.297|-184.25%|slower / worse|64.82%|
|full/disk/ulmdb/durable_batch1/throughput|records/s|8,776.996|11,051.118|-20.58%|slower / worse|25.91%|
|full/disk/ulmdb/durable_batch64/combined_throughput|records/s|144,641.868|182,566.239|-20.77%|slower / worse|26.22%|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99|ns|943,520.165|528,181.478|-78.64%|slower / worse|44.02%|
|full/disk/ulmdb/durable_batch64/sampled_p99|ns|654,452.483|475,171.439|-37.73%|slower / worse|27.39%|
|full/disk/ulmdb/durable_batch64/throughput|records/s|139,969.066|179,376.658|-21.97%|slower / worse|28.15%|
|full/disk/ulmdb/footprint/after/allocated_file_bytes|bytes|3,399,680.000|729,088.000|-366.29%|slower / worse|78.55%|
|full/disk/ulmdb/footprint/after/apparent_file_bytes|bytes|70,399,088.000|733,184.000|-9501.83%|slower / worse|98.96%|
|full/disk/ulmdb/footprint/after/files|files|4.000|2.000|-100.00%|slower / worse|50.00%|
|full/disk/ulmdb/footprint/before/allocated_file_bytes|bytes|585,728.000|352,256.000|-66.28%|slower / worse|39.86%|
|full/disk/ulmdb/footprint/before/apparent_file_bytes|bytes|67,142,240.000|356,352.000|-18741.55%|slower / worse|99.47%|
|full/disk/ulmdb/footprint/before/files|files|4.000|2.000|-100.00%|slower / worse|50.00%|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99|ns|428.015|799.828|+46.49%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,192,852.719|2,128,437.825|+50.01%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|117.013|614.053|+80.94%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,626,054.375|2,011,627.890|+477.94%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99|ns|115.986|599.424|+80.65%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,554,940.317|3,941,739.635|+446.84%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|417.607|785.841|+46.86%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,517,564.056|2,638,488.516|+109.12%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.550|43.488|+4.46%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|99,001,229.536|89,917,945.567|+10.10%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99|ns|526.132|1,244.617|+57.73%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,505,658.666|1,239,682.044|+102.12%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|143.234|1,101.924|+87.00%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,381,297.831|1,140,361.127|+722.66%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99|ns|142.669|1,066.962|+86.63%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,046,827.555|2,266,887.461|+651.99%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|511.276|1,243.096|+58.87%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,361,309.720|1,566,524.770|+178.41%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.550|42.832|+2.99%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|96,607,273.710|87,932,689.705|+9.87%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99|ns|198.182|335.488|+40.93%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/throughput|queries/s|7,600,099.209|4,184,184.435|+81.64%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99|ns|197.866|342.231|+42.18%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/throughput|queries/s|7,621,595.359|4,014,435.054|+89.85%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99|ns|200.511|337.969|+40.67%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/throughput|queries/s|7,320,924.406|4,106,947.228|+78.26%|faster / better|0.00%|
|full/disk/ulmdb/reopen/complete_cold_check_ns|ns|24,882,840.083|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/environment_drop_ns|ns|106,192.091|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/experimental_reenable_ns|ns|1,439.410|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/open_ns|ns|15,641,571.283|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/validation_ns|ns|6,152,818.927|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/setup/create|ns|11,109,184.362|165,497.333|-6612.61%|slower / worse|98.51%|
|full/ram/ulmdb/durable_batch1/combined_throughput|records/s|22,516.199|139,244.336|-83.83%|slower / worse|518.42%|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99|ns|1,939,479.529|9,303.974|-20745.71%|slower / worse|99.52%|
|full/ram/ulmdb/durable_batch1/sampled_p99|ns|26,542.450|8,005.244|-231.56%|slower / worse|69.84%|
|full/ram/ulmdb/durable_batch1/throughput|records/s|62,574.543|135,196.400|-53.72%|slower / worse|116.06%|
|full/ram/ulmdb/durable_batch64/combined_throughput|records/s|486,480.803|757,805.419|-35.80%|slower / worse|55.77%|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99|ns|274,601.392|101,487.083|-170.58%|slower / worse|63.04%|
|full/ram/ulmdb/durable_batch64/sampled_p99|ns|131,915.043|93,040.959|-41.78%|slower / worse|29.47%|
|full/ram/ulmdb/durable_batch64/throughput|records/s|496,965.865|754,442.542|-34.13%|slower / worse|51.81%|
|full/ram/ulmdb/footprint/after/allocated_file_bytes|bytes|3,395,584.000|729,088.000|-365.73%|slower / worse|78.53%|
|full/ram/ulmdb/footprint/after/apparent_file_bytes|bytes|70,399,088.000|733,184.000|-9501.83%|slower / worse|98.96%|
|full/ram/ulmdb/footprint/after/files|files|4.000|2.000|-100.00%|slower / worse|50.00%|
|full/ram/ulmdb/footprint/before/allocated_file_bytes|bytes|585,728.000|352,256.000|-66.28%|slower / worse|39.86%|
|full/ram/ulmdb/footprint/before/apparent_file_bytes|bytes|67,142,240.000|356,352.000|-18741.55%|slower / worse|99.47%|
|full/ram/ulmdb/footprint/before/files|files|4.000|2.000|-100.00%|slower / worse|50.00%|
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99|ns|427.374|766.618|+44.25%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,193,622.415|2,132,098.833|+49.79%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|115.620|603.849|+80.85%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,583,523.695|1,989,566.136|+482.21%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99|ns|116.775|605.775|+80.72%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,444,939.775|3,824,374.828|+460.74%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|420.638|757.032|+44.44%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,493,913.176|2,616,073.467|+110.01%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.255|46.993|+12.21%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|100,945,739.058|96,699,542.630|+4.39%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99|ns|525.227|1,223.050|+57.06%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,513,507.239|1,225,382.362|+105.12%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|142.555|1,079.820|+86.80%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,377,734.149|1,133,594.937|+727.26%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99|ns|143.491|1,064.375|+86.52%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,026,197.197|2,247,220.940|+657.66%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|511.662|1,216.257|+57.93%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,376,978.412|1,548,229.230|+182.71%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.281|45.305|+8.88%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|97,242,053.570|95,235,440.693|+2.11%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99|ns|200.001|337.046|+40.66%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/throughput|queries/s|7,534,044.360|4,189,822.731|+79.82%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99|ns|198.883|340.864|+41.65%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/throughput|queries/s|7,584,892.784|4,042,242.524|+87.64%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99|ns|202.168|335.943|+39.82%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/throughput|queries/s|7,241,558.442|4,120,179.029|+75.76%|faster / better|0.00%|
|full/ram/ulmdb/reopen/complete_cold_check_ns|ns|23,896,800.898|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/environment_drop_ns|ns|91,575.347|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/experimental_reenable_ns|ns|1,323.475|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/open_ns|ns|14,845,101.689|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/validation_ns|ns|5,983,086.003|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/setup/create|ns|9,105,365.055|89,836.312|-10035.51%|slower / worse|99.01%|
|millions/disk/ulmdb/close_reopen_ns|ns|302,329,760.600|3,827,569.391|-7798.74%|slower / worse|98.73%|
|millions/disk/ulmdb/load/throughput|operations/s|730,614.105|1,561,772.001|-53.22%|slower / worse|113.76%|
|millions/disk/ulmdb/read/throughput|operations/s|2,787,037.670|1,709,820.439|+63.00%|faster / better|0.00%|
|millions/disk/ulmdb/setup_ns|ns|39,118,386.379|1,067,570.297|-3564.24%|slower / worse|97.27%|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|bytes|30,662,656.000|34,648,064.000|+11.50%|faster / better|0.00%|
|millions/disk/ulmdb/storage_after_close_reopen/apparent_bytes|bytes|296,434,816.000|34,652,160.000|-755.46%|slower / worse|88.31%|
|millions/disk/ulmdb/storage_after_close_reopen/files|files|4.000|2.000|-100.00%|slower / worse|50.00%|
|millions/disk/ulmdb/storage_before_close/allocated_bytes|bytes|30,732,288.000|34,648,064.000|+11.30%|faster / better|0.00%|
|millions/disk/ulmdb/storage_before_close/apparent_bytes|bytes|296,434,816.000|34,652,160.000|-755.46%|slower / worse|88.31%|
|millions/disk/ulmdb/storage_before_close/files|files|4.000|2.000|-100.00%|slower / worse|50.00%|
|millions/disk/ulmdb/update/throughput|operations/s|113,883.559|128,507.223|-11.38%|slower / worse|12.84%|


### Complete RW3 current-image standard39 tables

All original observations remain, including every adverse cell. Four full process rounds/filesystem and three ordinary-million rounds; source/native/SQLite/raw/oracles checked. Contemporary million LMDB/SQLite controls are explicitly shared across arms. No native normalization or exclusions.

Actual processes39; observed quiet37; exposed2. Boundary checks do not prove continuous isolation. Exposed tags and identities are in availability-review.json. Exposure qualification follows each actual arm/engine/round in availability-review.json; no arm is presumed quiet before completion.

Rates/cost centers are geometric means of process metrics. Positive native advantage means more throughput or less cost. Within2% is a descriptive roughly-on-par label, not equivalence; remaining gap is required throughput increase or cost reduction to match native at the observed centers.

#### Candidate versus initial: every metric

|Metric|Unit|Reference|Candidate|Candidate change|Local LMDB|Native advantage|Status|Required increase / cost reduction|
|---|---|---:|---:|---:|---:|---:|---|---:|
|full/disk/harness/setup/after_setup_ns|ns|2,848.323|1,121.819|-60.61%|unavailable|unavailable|not measured|unavailable|
|full/disk/harness/setup/dataset_ns|ns|230,548,418.433|230,857,635.561|+0.13%|unavailable|unavailable|not measured|unavailable|
|full/disk/harness/setup/fixture_load_both_ns|ns|20,095,465.371|11,167,335.721|-44.43%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/combined_throughput|records/s|11,088.309|11,366.353|+2.51%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/pooled_sampled_p99|ns|325,869.209|328,231.060|+0.72%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/sampled_p99|ns|200,877.370|200,558.297|-0.16%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/throughput|records/s|10,509.880|11,051.118|+5.15%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/combined_throughput|records/s|183,584.672|182,566.239|-0.55%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/pooled_sampled_p99|ns|523,723.690|528,181.478|+0.85%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/sampled_p99|ns|459,354.935|475,171.439|+3.44%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/throughput|records/s|177,211.315|179,376.658|+1.22%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/allocated_file_bytes|bytes|729,088.000|729,088.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/apparent_file_bytes|bytes|733,184.000|733,184.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_pss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_rss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/allocated_file_bytes|bytes|352,256.000|352,256.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/apparent_file_bytes|bytes|356,352.000|356,352.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_pss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_rss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_hits/sampled_p99|ns|777.935|799.828|+2.81%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_hits/throughput|queries/s|2,171,198.140|2,128,437.825|-1.97%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|616.142|614.053|-0.34%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|2,027,074.335|2,011,627.890|-0.76%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_misses/sampled_p99|ns|596.963|599.424|+0.41%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_misses/throughput|queries/s|3,985,932.210|3,941,739.635|-1.11%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_mixed/sampled_p99|ns|763.738|785.841|+2.89%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_mixed/throughput|queries/s|2,695,153.970|2,638,488.516|-2.10%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|47.730|43.488|-8.89%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|97,659,102.083|89,917,945.567|-7.93%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_hits/sampled_p99|ns|1,252.786|1,244.617|-0.65%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_hits/throughput|queries/s|1,239,423.565|1,239,682.044|+0.02%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|1,098.806|1,101.924|+0.28%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|1,147,872.749|1,140,361.127|-0.65%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_misses/sampled_p99|ns|1,057.624|1,066.962|+0.88%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_misses/throughput|queries/s|2,271,722.005|2,266,887.461|-0.21%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_mixed/sampled_p99|ns|1,252.510|1,243.096|-0.75%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_mixed/throughput|queries/s|1,568,834.937|1,566,524.770|-0.15%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|43.935|42.832|-2.51%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|94,890,444.466|87,932,689.705|-7.33%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_hits/sampled_p99|ns|332.319|335.488|+0.95%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_hits/throughput|queries/s|4,191,396.634|4,184,184.435|-0.17%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_misses/sampled_p99|ns|340.205|342.231|+0.60%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_misses/throughput|queries/s|4,036,740.384|4,014,435.054|-0.55%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_mixed/sampled_p99|ns|336.085|337.969|+0.56%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_mixed/throughput|queries/s|4,109,330.805|4,106,947.228|-0.06%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/setup/create|ns|209,974.600|165,497.333|-21.18%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/durable_batch1/combined_throughput|records/s|16,105.334|6,021.058|-62.61%|11,366.353|-47.03%|slower / worse|88.78%|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99|ns|311,974.060|4,255,126.430|+1263.94%|328,231.060|-1196.38%|slower / worse|92.29%|
|full/disk/ulmdb/durable_batch1/sampled_p99|ns|83,964.759|570,090.435|+578.96%|200,558.297|-184.25%|slower / worse|64.82%|
|full/disk/ulmdb/durable_batch1/throughput|records/s|15,352.257|8,776.996|-42.83%|11,051.118|-20.58%|slower / worse|25.91%|
|full/disk/ulmdb/durable_batch64/combined_throughput|records/s|494,687.523|144,641.868|-70.76%|182,566.239|-20.77%|slower / worse|26.22%|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99|ns|445,960.184|943,520.165|+111.57%|528,181.478|-78.64%|slower / worse|44.02%|
|full/disk/ulmdb/durable_batch64/sampled_p99|ns|222,259.291|654,452.483|+194.45%|475,171.439|-37.73%|slower / worse|27.39%|
|full/disk/ulmdb/durable_batch64/throughput|records/s|485,702.704|139,969.066|-71.18%|179,376.658|-21.97%|slower / worse|28.15%|
|full/disk/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|3,399,680.000|-94.93%|729,088.000|-366.29%|slower / worse|78.55%|
|full/disk/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|70,399,088.000|+4.90%|733,184.000|-9501.83%|slower / worse|98.96%|
|full/disk/ulmdb/footprint/after/files|files|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|50.00%|
|full/disk/ulmdb/footprint/after/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/after/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/after/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|585,728.000|-99.13%|352,256.000|-66.28%|slower / worse|39.86%|
|full/disk/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,142,240.000|+0.05%|356,352.000|-18741.55%|slower / worse|99.47%|
|full/disk/ulmdb/footprint/before/files|files|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|50.00%|
|full/disk/ulmdb/footprint/before/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99|ns|430.428|428.015|-0.56%|799.828|+46.49%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,178,783.514|3,192,852.719|+0.44%|2,128,437.825|+50.01%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|118.924|117.013|-1.61%|614.053|+80.94%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,473,790.915|11,626,054.375|+1.33%|2,011,627.890|+477.94%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99|ns|119.419|115.986|-2.87%|599.424|+80.65%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,573,003.730|21,554,940.317|-0.08%|3,941,739.635|+446.84%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|423.024|417.607|-1.28%|785.841|+46.86%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,478,454.490|5,517,564.056|+0.71%|2,638,488.516|+109.12%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.255|41.550|+0.72%|43.488|+4.46%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|100,922,040.964|99,001,229.536|-1.90%|89,917,945.567|+10.10%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99|ns|534.462|526.132|-1.56%|1,244.617|+57.73%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,499,615.024|2,505,658.666|+0.24%|1,239,682.044|+102.12%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|142.129|143.234|+0.78%|1,101.924|+87.00%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,182,615.373|9,381,297.831|+2.16%|1,140,361.127|+722.66%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99|ns|142.268|142.669|+0.28%|1,066.962|+86.63%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/throughput|queries/s|16,984,612.246|17,046,827.555|+0.37%|2,266,887.461|+651.99%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|521.817|511.276|-2.02%|1,243.096|+58.87%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,354,629.062|4,361,309.720|+0.15%|1,566,524.770|+178.41%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.409|41.550|+0.34%|42.832|+2.99%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|96,866,110.984|96,607,273.710|-0.27%|87,932,689.705|+9.87%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99|ns|232.017|198.182|-14.58%|335.488|+40.93%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/throughput|queries/s|6,427,481.140|7,600,099.209|+18.24%|4,184,184.435|+81.64%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99|ns|230.874|197.866|-14.30%|342.231|+42.18%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/throughput|queries/s|6,414,801.346|7,621,595.359|+18.81%|4,014,435.054|+89.85%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99|ns|240.475|200.511|-16.62%|337.969|+40.67%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/throughput|queries/s|6,088,429.910|7,320,924.406|+20.24%|4,106,947.228|+78.26%|faster / better|0.00%|
|full/disk/ulmdb/reopen/complete_cold_check_ns|ns|94,834,419.550|24,882,840.083|-73.76%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/environment_drop_ns|ns|93,289.289|106,192.091|+13.83%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/experimental_reenable_ns|ns|2,302.024|1,439.410|-37.47%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/open_ns|ns|84,056,839.146|15,641,571.283|-81.39%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/validation_ns|ns|6,529,891.776|6,152,818.927|-5.77%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/setup/create|ns|103,311,676.499|11,109,184.362|-89.25%|165,497.333|-6612.61%|slower / worse|98.51%|
|full/ram/harness/setup/after_setup_ns|ns|886.090|1,060.011|+19.63%|unavailable|unavailable|not measured|unavailable|
|full/ram/harness/setup/dataset_ns|ns|236,228,697.879|232,794,817.203|-1.45%|unavailable|unavailable|not measured|unavailable|
|full/ram/harness/setup/fixture_load_both_ns|ns|18,478,843.480|9,218,186.079|-50.11%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/combined_throughput|records/s|137,743.332|139,244.336|+1.09%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/pooled_sampled_p99|ns|9,241.918|9,303.974|+0.67%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/sampled_p99|ns|8,464.588|8,005.244|-5.43%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/throughput|records/s|136,814.485|135,196.400|-1.18%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/combined_throughput|records/s|757,973.431|757,805.419|-0.02%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/pooled_sampled_p99|ns|94,343.579|101,487.083|+7.57%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/sampled_p99|ns|90,721.282|93,040.959|+2.56%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/throughput|records/s|750,144.947|754,442.542|+0.57%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/allocated_file_bytes|bytes|729,088.000|729,088.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/apparent_file_bytes|bytes|733,184.000|733,184.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_pss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_rss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/allocated_file_bytes|bytes|352,256.000|352,256.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/apparent_file_bytes|bytes|356,352.000|356,352.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_pss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_rss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_hits/sampled_p99|ns|770.443|766.618|-0.50%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_hits/throughput|queries/s|2,161,399.356|2,132,098.833|-1.36%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|597.679|603.849|+1.03%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|2,007,438.397|1,989,566.136|-0.89%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_misses/sampled_p99|ns|596.202|605.775|+1.61%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_misses/throughput|queries/s|3,951,404.335|3,824,374.828|-3.21%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_mixed/sampled_p99|ns|752.804|757.032|+0.56%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_mixed/throughput|queries/s|2,708,755.589|2,616,073.467|-3.42%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|47.742|46.993|-1.57%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|94,488,780.740|96,699,542.630|+2.34%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_hits/sampled_p99|ns|1,211.423|1,223.050|+0.96%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_hits/throughput|queries/s|1,234,263.034|1,225,382.362|-0.72%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|1,083.678|1,079.820|-0.36%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|1,137,054.233|1,133,594.937|-0.30%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_misses/sampled_p99|ns|1,044.473|1,064.375|+1.91%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_misses/throughput|queries/s|2,256,000.587|2,247,220.940|-0.39%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_mixed/sampled_p99|ns|1,206.037|1,216.257|+0.85%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_mixed/throughput|queries/s|1,559,506.560|1,548,229.230|-0.72%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|44.622|45.305|+1.53%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|95,431,764.307|95,235,440.693|-0.21%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_hits/sampled_p99|ns|333.589|337.046|+1.04%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_hits/throughput|queries/s|4,193,214.815|4,189,822.731|-0.08%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_misses/sampled_p99|ns|343.267|340.864|-0.70%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_misses/throughput|queries/s|4,019,461.058|4,042,242.524|+0.57%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_mixed/sampled_p99|ns|335.219|335.943|+0.22%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_mixed/throughput|queries/s|4,111,500.134|4,120,179.029|+0.21%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/setup/create|ns|134,423.128|89,836.312|-33.17%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/durable_batch1/combined_throughput|records/s|69,481.441|22,516.199|-67.59%|139,244.336|-83.83%|slower / worse|518.42%|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99|ns|13,832.114|1,939,479.529|+13921.57%|9,303.974|-20745.71%|slower / worse|99.52%|
|full/ram/ulmdb/durable_batch1/sampled_p99|ns|12,511.443|26,542.450|+112.15%|8,005.244|-231.56%|slower / worse|69.84%|
|full/ram/ulmdb/durable_batch1/throughput|records/s|57,619.389|62,574.543|+8.60%|135,196.400|-53.72%|slower / worse|116.06%|
|full/ram/ulmdb/durable_batch64/combined_throughput|records/s|933,335.618|486,480.803|-47.88%|757,805.419|-35.80%|slower / worse|55.77%|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99|ns|318,418.042|274,601.392|-13.76%|101,487.083|-170.58%|slower / worse|63.04%|
|full/ram/ulmdb/durable_batch64/sampled_p99|ns|112,933.595|131,915.043|+16.81%|93,040.959|-41.78%|slower / worse|29.47%|
|full/ram/ulmdb/durable_batch64/throughput|records/s|933,410.987|496,965.865|-46.76%|754,442.542|-34.13%|slower / worse|51.81%|
|full/ram/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|3,395,584.000|-94.94%|729,088.000|-365.73%|slower / worse|78.53%|
|full/ram/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|70,399,088.000|+4.90%|733,184.000|-9501.83%|slower / worse|98.96%|
|full/ram/ulmdb/footprint/after/files|files|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|50.00%|
|full/ram/ulmdb/footprint/after/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/after/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/after/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|585,728.000|-99.13%|352,256.000|-66.28%|slower / worse|39.86%|
|full/ram/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,142,240.000|+0.05%|356,352.000|-18741.55%|slower / worse|99.47%|
|full/ram/ulmdb/footprint/before/files|files|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|50.00%|
|full/ram/ulmdb/footprint/before/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99|ns|434.520|427.374|-1.64%|766.618|+44.25%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,181,346.468|3,193,622.415|+0.39%|2,132,098.833|+49.79%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|120.734|115.620|-4.24%|603.849|+80.85%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,468,287.851|11,583,523.695|+1.00%|1,989,566.136|+482.21%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99|ns|118.581|116.775|-1.52%|605.775|+80.72%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,523,061.889|21,444,939.775|-0.36%|3,824,374.828|+460.74%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|422.160|420.638|-0.36%|757.032|+44.44%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,483,037.454|5,493,913.176|+0.20%|2,616,073.467|+110.01%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.127|41.255|+0.31%|46.993|+12.21%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|100,968,987.419|100,945,739.058|-0.02%|96,699,542.630|+4.39%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99|ns|526.845|525.227|-0.31%|1,223.050|+57.06%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,511,735.186|2,513,507.239|+0.07%|1,225,382.362|+105.12%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|143.141|142.555|-0.41%|1,079.820|+86.80%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,196,375.352|9,377,734.149|+1.97%|1,133,594.937|+727.26%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99|ns|143.115|143.491|+0.26%|1,064.375|+86.52%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,010,161.078|17,026,197.197|+0.09%|2,247,220.940|+657.66%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|513.725|511.662|-0.40%|1,216.257|+57.93%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,369,344.695|4,376,978.412|+0.17%|1,548,229.230|+182.71%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.140|41.281|+0.34%|45.305|+8.88%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|96,794,235.776|97,242,053.570|+0.46%|95,235,440.693|+2.11%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99|ns|232.679|200.001|-14.04%|337.046|+40.66%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/throughput|queries/s|6,381,757.056|7,534,044.360|+18.06%|4,189,822.731|+79.82%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99|ns|232.735|198.883|-14.55%|340.864|+41.65%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/throughput|queries/s|6,364,441.680|7,584,892.784|+19.18%|4,042,242.524|+87.64%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99|ns|238.662|202.168|-15.29%|335.943|+39.82%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/throughput|queries/s|6,112,463.803|7,241,558.442|+18.47%|4,120,179.029|+75.76%|faster / better|0.00%|
|full/ram/ulmdb/reopen/complete_cold_check_ns|ns|54,263,003.837|23,896,800.898|-55.96%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/environment_drop_ns|ns|90,942.058|91,575.347|+0.70%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/experimental_reenable_ns|ns|2,238.979|1,323.475|-40.89%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/open_ns|ns|43,573,784.246|14,845,101.689|-65.93%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/validation_ns|ns|6,480,837.570|5,983,086.003|-7.68%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/setup/create|ns|46,777,591.454|9,105,365.055|-80.53%|89,836.312|-10035.51%|slower / worse|99.01%|
|millions/disk/lmdb/close_reopen_ns|ns|3,827,569.391|3,827,569.391|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/load/throughput|operations/s|1,561,772.001|1,561,772.001|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/read/throughput|operations/s|1,709,820.439|1,709,820.439|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/setup_ns|ns|1,067,570.297|1,067,570.297|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/allocated_bytes|bytes|34,648,064.000|34,648,064.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/apparent_bytes|bytes|34,652,160.000|34,652,160.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/allocated_bytes|bytes|34,648,064.000|34,648,064.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/apparent_bytes|bytes|34,652,160.000|34,652,160.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/update/throughput|operations/s|128,507.223|128,507.223|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/close_reopen_ns|ns|9,285,716.715|9,285,716.715|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/load/throughput|operations/s|828,305.775|828,305.775|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/read/throughput|operations/s|459,690.425|459,690.425|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/setup_ns|ns|2,593,787.143|2,593,787.143|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/allocated_bytes|bytes|25,317,376.000|25,317,376.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/apparent_bytes|bytes|25,317,376.000|25,317,376.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/files|files|1.000|1.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/allocated_bytes|bytes|33,103,872.000|33,103,872.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/apparent_bytes|bytes|33,099,896.000|33,099,896.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/files|files|3.000|3.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/update/throughput|operations/s|58,334.525|58,334.525|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/ulmdb/close_reopen_ns|ns|9,720,795,808.443|302,329,760.600|-96.89%|3,827,569.391|-7798.74%|slower / worse|98.73%|
|millions/disk/ulmdb/load/throughput|operations/s|550,121.033|730,614.105|+32.81%|1,561,772.001|-53.22%|slower / worse|113.76%|
|millions/disk/ulmdb/read/throughput|operations/s|2,565,216.824|2,787,037.670|+8.65%|1,709,820.439|+63.00%|faster / better|0.00%|
|millions/disk/ulmdb/setup_ns|ns|410,097,124.700|39,118,386.379|-90.46%|1,067,570.297|-3564.24%|slower / worse|97.27%|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|bytes|268,435,456.000|30,662,656.000|-88.58%|34,648,064.000|+11.50%|faster / better|0.00%|
|millions/disk/ulmdb/storage_after_close_reopen/apparent_bytes|bytes|268,435,456.000|296,434,816.000|+10.43%|34,652,160.000|-755.46%|slower / worse|88.31%|
|millions/disk/ulmdb/storage_after_close_reopen/files|files|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|50.00%|
|millions/disk/ulmdb/storage_before_close/allocated_bytes|bytes|268,435,456.000|30,732,288.000|-88.55%|34,648,064.000|+11.30%|faster / better|0.00%|
|millions/disk/ulmdb/storage_before_close/apparent_bytes|bytes|268,435,456.000|296,434,816.000|+10.43%|34,652,160.000|-755.46%|slower / worse|88.31%|
|millions/disk/ulmdb/storage_before_close/files|files|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|50.00%|
|millions/disk/ulmdb/update/throughput|operations/s|103,378.334|113,883.559|+10.16%|128,507.223|-11.38%|slower / worse|12.84%|

Complete confidence intervals, all process values/signs and insufficient-repeat flags remain in the unchanged original comparison JSON/Markdown.

#### Candidate versus accepted: every metric

|Metric|Unit|Reference|Candidate|Candidate change|Local LMDB|Native advantage|Status|Required increase / cost reduction|
|---|---|---:|---:|---:|---:|---:|---|---:|
|full/disk/harness/setup/after_setup_ns|ns|1,610.155|1,121.819|-30.33%|unavailable|unavailable|not measured|unavailable|
|full/disk/harness/setup/dataset_ns|ns|232,557,927.077|230,857,635.561|-0.73%|unavailable|unavailable|not measured|unavailable|
|full/disk/harness/setup/fixture_load_both_ns|ns|10,342,691.584|11,167,335.721|+7.97%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/combined_throughput|records/s|11,432.373|11,366.353|-0.58%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/pooled_sampled_p99|ns|325,665.359|328,231.060|+0.79%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/sampled_p99|ns|192,633.066|200,558.297|+4.11%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch1/throughput|records/s|10,897.374|11,051.118|+1.41%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/combined_throughput|records/s|180,066.634|182,566.239|+1.39%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/pooled_sampled_p99|ns|595,120.438|528,181.478|-11.25%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/sampled_p99|ns|474,426.666|475,171.439|+0.16%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/durable_batch64/throughput|records/s|176,120.755|179,376.658|+1.85%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/allocated_file_bytes|bytes|729,088.000|729,088.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/apparent_file_bytes|bytes|733,184.000|733,184.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_pss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_rss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/after/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/allocated_file_bytes|bytes|352,256.000|352,256.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/apparent_file_bytes|bytes|356,352.000|356,352.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_pss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_rss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/footprint/before/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_hits/sampled_p99|ns|781.307|799.828|+2.37%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_hits/throughput|queries/s|2,122,255.142|2,128,437.825|+0.29%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|625.007|614.053|-1.75%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|2,018,202.297|2,011,627.890|-0.33%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_misses/sampled_p99|ns|612.558|599.424|-2.14%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_misses/throughput|queries/s|3,932,034.902|3,941,739.635|+0.25%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_mixed/sampled_p99|ns|764.723|785.841|+2.76%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_mixed/throughput|queries/s|2,640,360.119|2,638,488.516|-0.07%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|45.291|43.488|-3.98%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|89,504,450.931|89,917,945.567|+0.46%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_hits/sampled_p99|ns|1,251.536|1,244.617|-0.55%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_hits/throughput|queries/s|1,246,765.028|1,239,682.044|-0.57%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|1,102.496|1,101.924|-0.05%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|1,132,399.585|1,140,361.127|+0.70%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_misses/sampled_p99|ns|1,077.023|1,066.962|-0.93%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_misses/throughput|queries/s|2,244,298.301|2,266,887.461|+1.01%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_mixed/sampled_p99|ns|1,236.751|1,243.096|+0.51%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_mixed/throughput|queries/s|1,569,503.737|1,566,524.770|-0.19%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|43.353|42.832|-1.20%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|91,322,753.174|87,932,689.705|-3.71%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_hits/sampled_p99|ns|334.197|335.488|+0.39%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_hits/throughput|queries/s|4,197,846.614|4,184,184.435|-0.33%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_misses/sampled_p99|ns|338.668|342.231|+1.05%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_misses/throughput|queries/s|4,018,537.523|4,014,435.054|-0.10%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_mixed/sampled_p99|ns|337.343|337.969|+0.19%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/ordered_kv_mixed/throughput|queries/s|4,127,314.268|4,106,947.228|-0.49%|unavailable|unavailable|not measured|unavailable|
|full/disk/lmdb/setup/create|ns|196,899.723|165,497.333|-15.95%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/durable_batch1/combined_throughput|records/s|16,282.953|6,021.058|-63.02%|11,366.353|-47.03%|slower / worse|88.78%|
|full/disk/ulmdb/durable_batch1/pooled_sampled_p99|ns|192,855.607|4,255,126.430|+2106.38%|328,231.060|-1196.38%|slower / worse|92.29%|
|full/disk/ulmdb/durable_batch1/sampled_p99|ns|78,599.515|570,090.435|+625.31%|200,558.297|-184.25%|slower / worse|64.82%|
|full/disk/ulmdb/durable_batch1/throughput|records/s|15,402.150|8,776.996|-43.01%|11,051.118|-20.58%|slower / worse|25.91%|
|full/disk/ulmdb/durable_batch64/combined_throughput|records/s|503,707.604|144,641.868|-71.28%|182,566.239|-20.77%|slower / worse|26.22%|
|full/disk/ulmdb/durable_batch64/pooled_sampled_p99|ns|373,179.576|943,520.165|+152.83%|528,181.478|-78.64%|slower / worse|44.02%|
|full/disk/ulmdb/durable_batch64/sampled_p99|ns|217,548.359|654,452.483|+200.83%|475,171.439|-37.73%|slower / worse|27.39%|
|full/disk/ulmdb/durable_batch64/throughput|records/s|494,496.394|139,969.066|-71.69%|179,376.658|-21.97%|slower / worse|28.15%|
|full/disk/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|3,399,680.000|-94.93%|729,088.000|-366.29%|slower / worse|78.55%|
|full/disk/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|70,399,088.000|+4.90%|733,184.000|-9501.83%|slower / worse|98.96%|
|full/disk/ulmdb/footprint/after/files|files|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|50.00%|
|full/disk/ulmdb/footprint/after/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/after/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/after/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|585,728.000|-99.13%|352,256.000|-66.28%|slower / worse|39.86%|
|full/disk/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,142,240.000|+0.05%|356,352.000|-18741.55%|slower / worse|99.47%|
|full/disk/ulmdb/footprint/before/files|files|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|50.00%|
|full/disk/ulmdb/footprint/before/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/footprint/before/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/disk/ulmdb/ipv4_lpm_hits/sampled_p99|ns|427.592|428.015|+0.10%|799.828|+46.49%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,205,124.315|3,192,852.719|-0.38%|2,128,437.825|+50.01%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|123.147|117.013|-4.98%|614.053|+80.94%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,601,229.004|11,626,054.375|+0.21%|2,011,627.890|+477.94%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/sampled_p99|ns|122.731|115.986|-5.50%|599.424|+80.65%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,710,424.165|21,554,940.317|-0.72%|3,941,739.635|+446.84%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|421.097|417.607|-0.83%|785.841|+46.86%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,524,162.993|5,517,564.056|-0.12%|2,638,488.516|+109.12%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.000|41.550|+1.34%|43.488|+4.46%|faster / better|0.00%|
|full/disk/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|100,933,799.179|99,001,229.536|-1.91%|89,917,945.567|+10.10%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/sampled_p99|ns|538.733|526.132|-2.34%|1,244.617|+57.73%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,475,899.757|2,505,658.666|+1.20%|1,239,682.044|+102.12%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|142.912|143.234|+0.23%|1,101.924|+87.00%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,419,683.678|9,381,297.831|-0.41%|1,140,361.127|+722.66%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/sampled_p99|ns|142.281|142.669|+0.27%|1,066.962|+86.63%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,244,647.030|17,046,827.555|-1.15%|2,266,887.461|+651.99%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|524.836|511.276|-2.58%|1,243.096|+58.87%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,288,868.740|4,361,309.720|+1.69%|1,566,524.770|+178.41%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.396|41.550|+0.37%|42.832|+2.99%|faster / better|0.00%|
|full/disk/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|95,971,879.215|96,607,273.710|+0.66%|87,932,689.705|+9.87%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/sampled_p99|ns|200.400|198.182|-1.11%|335.488|+40.93%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_hits/throughput|queries/s|7,247,100.894|7,600,099.209|+4.87%|4,184,184.435|+81.64%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/sampled_p99|ns|199.759|197.866|-0.95%|342.231|+42.18%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_misses/throughput|queries/s|7,296,310.642|7,621,595.359|+4.46%|4,014,435.054|+89.85%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/sampled_p99|ns|201.275|200.511|-0.38%|337.969|+40.67%|faster / better|0.00%|
|full/disk/ulmdb/ordered_kv_mixed/throughput|queries/s|6,972,243.092|7,320,924.406|+5.00%|4,106,947.228|+78.26%|faster / better|0.00%|
|full/disk/ulmdb/reopen/complete_cold_check_ns|ns|93,981,984.923|24,882,840.083|-73.52%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/environment_drop_ns|ns|94,272.531|106,192.091|+12.64%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/experimental_reenable_ns|ns|2,085.615|1,439.410|-30.98%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/open_ns|ns|83,620,924.786|15,641,571.283|-81.29%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/reopen/validation_ns|ns|6,108,018.311|6,152,818.927|+0.73%|unavailable|unavailable|not measured|unavailable|
|full/disk/ulmdb/setup/create|ns|104,154,937.847|11,109,184.362|-89.33%|165,497.333|-6612.61%|slower / worse|98.51%|
|full/ram/harness/setup/after_setup_ns|ns|2,940.912|1,060.011|-63.96%|unavailable|unavailable|not measured|unavailable|
|full/ram/harness/setup/dataset_ns|ns|235,666,349.293|232,794,817.203|-1.22%|unavailable|unavailable|not measured|unavailable|
|full/ram/harness/setup/fixture_load_both_ns|ns|9,006,148.433|9,218,186.079|+2.35%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/combined_throughput|records/s|138,756.420|139,244.336|+0.35%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/pooled_sampled_p99|ns|11,345.321|9,303.974|-17.99%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/sampled_p99|ns|8,150.535|8,005.244|-1.78%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch1/throughput|records/s|135,253.282|135,196.400|-0.04%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/combined_throughput|records/s|754,186.924|757,805.419|+0.48%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/pooled_sampled_p99|ns|101,666.019|101,487.083|-0.18%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/sampled_p99|ns|93,060.706|93,040.959|-0.02%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/durable_batch64/throughput|records/s|748,423.558|754,442.542|+0.80%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/allocated_file_bytes|bytes|729,088.000|729,088.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/apparent_file_bytes|bytes|733,184.000|733,184.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_pss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_rss_kib|KiB|712.000|712.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/after/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/allocated_file_bytes|bytes|352,256.000|352,256.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/apparent_file_bytes|bytes|356,352.000|356,352.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_pss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_rss_kib|KiB|12.000|12.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/footprint/before/mapped_virtual_kib|KiB|524,296.000|524,296.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_hits/sampled_p99|ns|791.541|766.618|-3.15%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_hits/throughput|queries/s|2,156,600.400|2,132,098.833|-1.14%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|624.843|603.849|-3.36%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|1,965,435.268|1,989,566.136|+1.23%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_misses/sampled_p99|ns|618.200|605.775|-2.01%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_misses/throughput|queries/s|3,830,539.615|3,824,374.828|-0.16%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_mixed/sampled_p99|ns|773.795|757.032|-2.17%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_mixed/throughput|queries/s|2,672,711.408|2,616,073.467|-2.12%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|43.219|46.993|+8.73%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|87,203,589.989|96,699,542.630|+10.89%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_hits/sampled_p99|ns|1,251.755|1,223.050|-2.29%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_hits/throughput|queries/s|1,244,933.810|1,225,382.362|-1.57%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|1,106.277|1,079.820|-2.39%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|1,121,959.591|1,133,594.937|+1.04%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_misses/sampled_p99|ns|1,081.121|1,064.375|-1.55%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_misses/throughput|queries/s|2,224,145.850|2,247,220.940|+1.04%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_mixed/sampled_p99|ns|1,262.889|1,216.257|-3.69%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_mixed/throughput|queries/s|1,558,424.493|1,548,229.230|-0.65%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.666|45.305|+8.73%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|87,396,790.268|95,235,440.693|+8.97%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_hits/sampled_p99|ns|337.595|337.046|-0.16%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_hits/throughput|queries/s|4,195,401.921|4,189,822.731|-0.13%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_misses/sampled_p99|ns|340.710|340.864|+0.05%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_misses/throughput|queries/s|4,011,055.621|4,042,242.524|+0.78%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_mixed/sampled_p99|ns|340.514|335.943|-1.34%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/ordered_kv_mixed/throughput|queries/s|4,095,919.534|4,120,179.029|+0.59%|unavailable|unavailable|not measured|unavailable|
|full/ram/lmdb/setup/create|ns|125,484.279|89,836.312|-28.41%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/durable_batch1/combined_throughput|records/s|76,009.357|22,516.199|-70.38%|139,244.336|-83.83%|slower / worse|518.42%|
|full/ram/ulmdb/durable_batch1/pooled_sampled_p99|ns|12,320.213|1,939,479.529|+15642.26%|9,303.974|-20745.71%|slower / worse|99.52%|
|full/ram/ulmdb/durable_batch1/sampled_p99|ns|11,393.801|26,542.450|+132.96%|8,005.244|-231.56%|slower / worse|69.84%|
|full/ram/ulmdb/durable_batch1/throughput|records/s|63,057.996|62,574.543|-0.77%|135,196.400|-53.72%|slower / worse|116.06%|
|full/ram/ulmdb/durable_batch64/combined_throughput|records/s|998,852.630|486,480.803|-51.30%|757,805.419|-35.80%|slower / worse|55.77%|
|full/ram/ulmdb/durable_batch64/pooled_sampled_p99|ns|247,079.791|274,601.392|+11.14%|101,487.083|-170.58%|slower / worse|63.04%|
|full/ram/ulmdb/durable_batch64/sampled_p99|ns|101,462.253|131,915.043|+30.01%|93,040.959|-41.78%|slower / worse|29.47%|
|full/ram/ulmdb/durable_batch64/throughput|records/s|990,701.486|496,965.865|-49.84%|754,442.542|-34.13%|slower / worse|51.81%|
|full/ram/ulmdb/footprint/after/allocated_file_bytes|bytes|67,108,864.000|3,395,584.000|-94.94%|729,088.000|-365.73%|slower / worse|78.53%|
|full/ram/ulmdb/footprint/after/apparent_file_bytes|bytes|67,108,864.000|70,399,088.000|+4.90%|733,184.000|-9501.83%|slower / worse|98.96%|
|full/ram/ulmdb/footprint/after/files|files|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|50.00%|
|full/ram/ulmdb/footprint/after/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/after/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|712.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/after/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/allocated_file_bytes|bytes|67,108,864.000|585,728.000|-99.13%|352,256.000|-66.28%|slower / worse|39.86%|
|full/ram/ulmdb/footprint/before/apparent_file_bytes|bytes|67,108,864.000|67,142,240.000|+0.05%|356,352.000|-18741.55%|slower / worse|99.47%|
|full/ram/ulmdb/footprint/before/files|files|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|50.00%|
|full/ram/ulmdb/footprint/before/mapped_pss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/mapped_rss_kib|KiB|unavailable|unavailable|unavailable|12.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/footprint/before/mapped_virtual_kib|KiB|unavailable|unavailable|unavailable|524,296.000|unavailable|not measured|unavailable|
|full/ram/ulmdb/ipv4_lpm_hits/sampled_p99|ns|426.375|427.374|+0.23%|766.618|+44.25%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_hits/throughput|queries/s|3,210,287.456|3,193,622.415|-0.52%|2,132,098.833|+49.79%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/sampled_p99|ns|121.580|115.620|-4.90%|603.849|+80.85%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_inside_gap_misses/throughput|queries/s|11,612,062.287|11,583,523.695|-0.25%|1,989,566.136|+482.21%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/sampled_p99|ns|120.440|116.775|-3.04%|605.775|+80.72%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_misses/throughput|queries/s|21,663,895.835|21,444,939.775|-1.01%|3,824,374.828|+460.74%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/sampled_p99|ns|417.964|420.638|+0.64%|757.032|+44.44%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_mixed/throughput|queries/s|5,525,942.649|5,493,913.176|-0.58%|2,616,073.467|+110.01%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/sampled_p99|ns|41.255|41.255|+0.00%|46.993|+12.21%|faster / better|0.00%|
|full/ram/ulmdb/ipv4_lpm_outside_cover_misses/throughput|queries/s|100,688,323.424|100,945,739.058|+0.26%|96,699,542.630|+4.39%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/sampled_p99|ns|537.646|525.227|-2.31%|1,223.050|+57.06%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_hits/throughput|queries/s|2,463,834.718|2,513,507.239|+2.02%|1,225,382.362|+105.12%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/sampled_p99|ns|143.307|142.555|-0.52%|1,079.820|+86.80%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_inside_gap_misses/throughput|queries/s|9,337,444.739|9,377,734.149|+0.43%|1,133,594.937|+727.26%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/sampled_p99|ns|143.573|143.491|-0.06%|1,064.375|+86.52%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_misses/throughput|queries/s|17,060,060.129|17,026,197.197|-0.20%|2,247,220.940|+657.66%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/sampled_p99|ns|526.216|511.662|-2.77%|1,216.257|+57.93%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_mixed/throughput|queries/s|4,280,268.447|4,376,978.412|+2.26%|1,548,229.230|+182.71%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/sampled_p99|ns|41.140|41.281|+0.34%|45.305|+8.88%|faster / better|0.00%|
|full/ram/ulmdb/ipv6_lpm_outside_cover_misses/throughput|queries/s|96,700,573.653|97,242,053.570|+0.56%|95,235,440.693|+2.11%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/sampled_p99|ns|196.979|200.001|+1.53%|337.046|+40.66%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_hits/throughput|queries/s|7,417,454.691|7,534,044.360|+1.57%|4,189,822.731|+79.82%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/sampled_p99|ns|195.180|198.883|+1.90%|340.864|+41.65%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_misses/throughput|queries/s|7,469,226.787|7,584,892.784|+1.55%|4,042,242.524|+87.64%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/sampled_p99|ns|200.388|202.168|+0.89%|335.943|+39.82%|faster / better|0.00%|
|full/ram/ulmdb/ordered_kv_mixed/throughput|queries/s|7,125,572.725|7,241,558.442|+1.63%|4,120,179.029|+75.76%|faster / better|0.00%|
|full/ram/ulmdb/reopen/complete_cold_check_ns|ns|53,012,214.171|23,896,800.898|-54.92%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/environment_drop_ns|ns|89,100.477|91,575.347|+2.78%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/experimental_reenable_ns|ns|2,202.130|1,323.475|-39.90%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/open_ns|ns|42,620,078.902|14,845,101.689|-65.17%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/reopen/validation_ns|ns|6,136,722.790|5,983,086.003|-2.50%|unavailable|unavailable|not measured|unavailable|
|full/ram/ulmdb/setup/create|ns|46,771,021.235|9,105,365.055|-80.53%|89,836.312|-10035.51%|slower / worse|99.01%|
|millions/disk/lmdb/close_reopen_ns|ns|3,827,569.391|3,827,569.391|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/load/throughput|operations/s|1,561,772.001|1,561,772.001|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/read/throughput|operations/s|1,709,820.439|1,709,820.439|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/setup_ns|ns|1,067,570.297|1,067,570.297|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/allocated_bytes|bytes|34,648,064.000|34,648,064.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/apparent_bytes|bytes|34,652,160.000|34,652,160.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_after_close_reopen/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/allocated_bytes|bytes|34,648,064.000|34,648,064.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/apparent_bytes|bytes|34,652,160.000|34,652,160.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/storage_before_close/files|files|2.000|2.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/lmdb/update/throughput|operations/s|128,507.223|128,507.223|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/close_reopen_ns|ns|9,285,716.715|9,285,716.715|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/load/throughput|operations/s|828,305.775|828,305.775|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/read/throughput|operations/s|459,690.425|459,690.425|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/setup_ns|ns|2,593,787.143|2,593,787.143|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/allocated_bytes|bytes|25,317,376.000|25,317,376.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/apparent_bytes|bytes|25,317,376.000|25,317,376.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_after_close_reopen/files|files|1.000|1.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/allocated_bytes|bytes|33,103,872.000|33,103,872.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/apparent_bytes|bytes|33,099,896.000|33,099,896.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/storage_before_close/files|files|3.000|3.000|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/sqlite/update/throughput|operations/s|58,334.525|58,334.525|+0.00%|unavailable|unavailable|not measured|unavailable|
|millions/disk/ulmdb/close_reopen_ns|ns|7,083,864,235.168|302,329,760.600|-95.73%|3,827,569.391|-7798.74%|slower / worse|98.73%|
|millions/disk/ulmdb/load/throughput|operations/s|1,399,603.376|730,614.105|-47.80%|1,561,772.001|-53.22%|slower / worse|113.76%|
|millions/disk/ulmdb/read/throughput|operations/s|2,803,538.045|2,787,037.670|-0.59%|1,709,820.439|+63.00%|faster / better|0.00%|
|millions/disk/ulmdb/setup_ns|ns|401,853,682.795|39,118,386.379|-90.27%|1,067,570.297|-3564.24%|slower / worse|97.27%|
|millions/disk/ulmdb/storage_after_close_reopen/allocated_bytes|bytes|268,435,456.000|30,662,656.000|-88.58%|34,648,064.000|+11.50%|faster / better|0.00%|
|millions/disk/ulmdb/storage_after_close_reopen/apparent_bytes|bytes|268,435,456.000|296,434,816.000|+10.43%|34,652,160.000|-755.46%|slower / worse|88.31%|
|millions/disk/ulmdb/storage_after_close_reopen/files|files|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|50.00%|
|millions/disk/ulmdb/storage_before_close/allocated_bytes|bytes|268,435,456.000|30,732,288.000|-88.55%|34,648,064.000|+11.30%|faster / better|0.00%|
|millions/disk/ulmdb/storage_before_close/apparent_bytes|bytes|268,435,456.000|296,434,816.000|+10.43%|34,652,160.000|-755.46%|slower / worse|88.31%|
|millions/disk/ulmdb/storage_before_close/files|files|2.000|4.000|+100.00%|2.000|-100.00%|slower / worse|50.00%|
|millions/disk/ulmdb/update/throughput|operations/s|145,552.760|113,883.559|-21.76%|128,507.223|-11.38%|slower / worse|12.84%|

Complete confidence intervals, all process values/signs and insufficient-repeat flags remain in the unchanged original comparison JSON/Markdown.



### RW3 automatic checkpoint costs: all candidate observations

All original phase time remains charged; checkpoint elapsed is never subtracted. These are measured wall envelopes and logical encoded/discarded bytes, not CPU shares, total device traffic or DRAM bandwidth. Initial mandatory image creation is in setup but excluded from periodic counters. Recovery clearing is in open but excluded from post-ready discard counters. Reopen resets process counters. Cleanup pending is retained as reported; no observations are selected or excluded.

#### Full original workload: before/after counters

|FS/round/step|Boundary quiet|Periodic count|Image bytes|Checkpoint ns|Logical discarded bytes|BaseLSN/tail|Cleanup pending|ULMDB all charged write ns|
|---|---|---:|---:|---:|---:|---|---|---:|
|disk/0/002|True|4|9,583,712|39,186,340|1,937,888|512/12|False|155,435,215|
|ram/0/004|True|4|9,583,712|24,167,268|1,937,888|512/12|False|45,136,060|
|ram/1/006|True|4|9,583,712|24,161,076|1,937,888|512/12|False|45,199,249|
|disk/1/010|True|4|9,583,712|39,273,707|1,937,888|512/12|False|150,341,410|
|disk/2/012|True|4|9,583,712|39,475,632|1,937,888|512/12|False|157,281,614|
|ram/2/017|True|4|9,583,712|23,911,019|1,937,888|512/12|False|44,843,013|
|ram/3/019|True|4|9,583,712|24,011,241|1,937,888|512/12|False|45,016,970|
|disk/3/023|False|4|9,583,712|41,042,057|1,937,888|512/12|False|160,311,190|

Full checkpoint snapshots cover all reads and both original write cases; there are no checkpoints in ordinary reads. Individual checkpoint-to-sampled-transaction associations are not invented from aggregate counters. All original raw write tails and exact offsets remain in raw-review.json.

#### Ordinary million workload: per-phase checkpoint deltas

|Round/step/phase|Boundary quiet|Periodic count|Image bytes|Checkpoint ns|Logical discarded bytes|Original phase elapsed ns|Image bytes/user payload ratio|
|---|---|---:|---:|---:|---:|---:|---:|
|0/026/load|True|7|104,566,808|408,461,971|17,979,160|1,355,687,473|6.535×|
|0/026/read|True|0|0|0|0|3,575,148,593|not applicable|
|0/026/update|True|11|308,661,704|1,273,810,232|105,542,312|9,497,350,290|19.291×|
|1/032/load|True|7|104,566,808|441,758,829|17,979,160|1,401,609,863|6.535×|
|1/032/read|True|0|0|0|0|3,616,706,354|not applicable|
|1/032/update|True|11|308,661,704|1,126,464,691|105,542,312|8,569,568,116|19.291×|
|2/035/load|True|7|104,566,808|407,337,575|17,979,160|1,349,426,128|6.535×|
|2/035/read|True|0|0|0|0|3,572,433,370|not applicable|
|2/035/update|True|11|308,661,704|1,091,242,456|105,542,312|8,318,701,043|19.291×|

The image/payload ratio divides periodic image bytes by the original write phase records×16B; it is not total device-write amplification. All original WAL data, selectors, genesis image, sparse punching/zero fallback and synchronization remain charged in actual benchmark time. Image-only volume is a lower bound on maintenance output. Post-reopen process counts reset, so negative cross-reset deltas are not manufactured.

#### Every setup/reopen/file observation

|Round/step|Setup ns|Drop+open ns|Allocated before|Allocated after|Apparent after|Files after|Reopened base/tail|
|---|---:|---:|---:|---:|---:|---:|---|
|0/026|38,088,456|306,341,324|30,732,288|30,662,656|296,434,816|4|2304/31|
|1/032|41,144,536|299,968,247|30,732,288|30,662,656|296,434,816|4|2304/31|
|2/035|38,197,704|300,719,666|30,732,288|30,662,656|296,434,816|4|2304/31|


[Source/raw review receipt](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/review-receipt.json), [decision receipt](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/decision-receipt.json), [all checkpoint phase snapshots](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/campaign-independent-review-v1/checkpoint-costs.json), [all source/test/build qualification](benchmark/work/optimization-20261105/next-resident-current-image/private-v1/build-qualification-v1-receipt.json). No engine observation is retried, discarded or normalized. Retained6 scorecard and accepted assets remain unchanged.

## Completed checkpoint diagnosis: RAM qualified; million artifact gate FAIL

All 26 fixed controls pass original correctness, source/native, raw, canonical-state and profile-conservation guards. A is the pristine held RW3 artifact; B/P are the same rebuilt ELF with new clocks off/on. All six RAM combined-cost gates pass the declared 5% center limit, including native controls and cumulative A→P effects. This qualifies the checkpoint wall intervals within that RAM control; it neither promotes the rejected full-image design nor establishes CPU or uninstrumented-production shares.

| Qualified RAM interval | Mean per periodic checkpoint, ms |
| --- | ---: |
| Typed audit | 0.523733 |
| Slots and all-live export | 0.664467 |
| Encode validation / serialization / SHA | 2.842732 |
| Image / selector / cleanup I/O | 1.839076 |

Those means cover all 16 periodic RAM rows. Across each four-checkpoint full process the original outer clock exceeds the inner row sum by 0.145–0.159ms; the unassigned residual stays in the original cost. Eight genesis rows remain separate and charged inside setup. All 78 rows conserve their seven intervals exactly.

**Million qualification fails.** All five compiled A→B/A→P center gates fail, with the first pristine A observation much slower across load/read/update; its cause is unconfirmed and the point is retained. B→P centers stay within 5%, but the first enabled read pair costs +10.265% (+2.040% center). The 54 periodic million rows and 8.249–8.405ms per-process outer residuals are descriptive only. Native L children 18 and 24 observed external rustc work; both direct ratios stay exposed/unqualified. No timing, point or residual is excluded, normalized or subtracted. Boundaries do not prove continuous host isolation.

**Next architecture priority remains persistent current-page COW with bounded metadata.** The measured RAM envelopes support avoiding repeated all-live encoding and publication. Subsequent io_uring and TMPFS self-VM transport controls are rejected below: all 18 paired costs rise. They demonstrate no engine gain.

[All 373 full metrics, reads/tails/resources](benchmark/work/optimization-20261105/next-resident-current-image/profile-v1/analysis-v1/FULL_TABLES.md), [all 77 million metrics and points](benchmark/work/optimization-20261105/next-resident-current-image/profile-v1/analysis-v1/analysis.json), [all direct-native observations](benchmark/work/optimization-20261105/next-resident-current-image/profile-v1/analysis-v1/NATIVE_TABLES.md), [independent peer arithmetic](benchmark/work/optimization-20261105/next-resident-current-image/profile-v1/independent-peer-review-v1/review.json).

### Fixed26 checkpoint phase control

descriptive instrumented intervals only; no uninstrumented CPU-share claim. All observations and adverse pairs are retained. No overhead or native subtraction.

Boundary quiet: False. All perturbation gates pass: False.

| Suite / metric | B vs A cost | P vs B cost | P vs A cost | All centers ≤5% |
| --- | ---: | ---: | ---: | --- |
| full / write.batch1.lmdb.combined_cost | +0.319% | -0.994% | -0.678% | True |
| full / write.batch1.ulmdb.combined_cost | -0.311% | +0.404% | +0.091% | True |
| full / write.batch64.lmdb.combined_cost | +0.669% | -0.532% | +0.133% | True |
| full / write.batch64.ulmdb.combined_cost | -0.878% | -0.672% | -1.544% | True |
| full / write.both_batches.lmdb.combined_cost | +0.641% | -0.569% | +0.069% | True |
| full / write.both_batches.ulmdb.combined_cost | -0.729% | -0.402% | -1.128% | True |
| millions / complete_work.elapsed_ns | -16.150% | +0.577% | -15.666% | False |
| millions / load.cost | -17.833% | +0.176% | -17.688% | False |
| millions / load_update.elapsed_ns | -13.709% | +0.021% | -13.691% | False |
| millions / read.cost | -22.037% | +2.040% | -20.447% | False |
| millions / update.cost | -12.926% | -0.005% | -12.931% | False |

Every full read/write/tail/creation/cold/resource metric and every ordinary million phase/storage/accounting value is in analysis.json, with all process values and pair directions. Native million controls are one real shared observation per round.

| Profile / scope | Rows | Audit ms | Export ms | Encode+SHA ms | Publication IO ms | Outer unassigned ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 01-full-r0-s2-P / non-genesis | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| 04-full-r1-s3-P / non-genesis | 4 | 2.093 | 2.819 | 11.368 | 7.576 | 0.146 |
| 06-full-r2-s2-P / non-genesis | 4 | 2.075 | 2.693 | 11.488 | 7.437 | 0.145 |
| 08-full-r3-s1-P / non-genesis | 4 | 2.109 | 2.530 | 11.111 | 7.023 | 0.159 |
| 12-full-r4-s2-P / non-genesis | 4 | 2.104 | 2.590 | 11.516 | 7.389 | 0.147 |
| 16-millions-r1-s3-P / non-genesis | 18 | 172.569 | 77.718 | 458.720 | 747.627 | 8.405 |
| 20-millions-r2-s3-P / non-genesis | 18 | 172.386 | 76.936 | 455.521 | 744.710 | 8.276 |
| 22-millions-r3-s1-P / non-genesis | 18 | 172.528 | 78.111 | 458.027 | 743.272 | 8.249 |

Genesis durations and all seven phase intervals, exact lineage/page/image-byte rows and original outer totals remain separately retained. No residual cost is assigned a mechanism. No additional profiler or optimization is automatically authorized.


### Interpretation and exposure qualification

All26 once-only processes passed original engine oracles, canonical image/WAL state, raw timing accounting, row bounds/conservation, native provenance and source guards. No rerun, excluded point, native normalization or overhead subtraction occurred. The pristine J3 candidate remains unaccepted by this profiling control.

| Population | Children | Observed exposed at return | ≤5% center gates | Interpretation |
| --- | ---: | ---: | --- | --- |
| RAM full A/B/P | 12 | 0 | PASS for all declared combined U/native cost gates | Measured checkpoint wall intervals; no CPU-share claim |
| Disk-million U A/B/P | 9 | 0 | B/P centers pass; A/B and A/P compiled-artifact gates FAIL | Descriptive instrumented intervals; no unperturbed-production-share claim |
| Disk-million native L | 3 | 2 | No fabricated arm-specific native samples | Keep every direct ratio; exposed L2/L3 remain unqualified |
| Empty B/P smokes | 2 | 0 | Not timing evidence | Empty report arrays and complete original smoke oracles pass |

Boundary observations do not prove continuous host isolation. The overall quiet and perturbation gates fail; per-suite boundaries are described separately, without changing the declared all-result decision.

#### Million adverse points and first pristine observation

The first A observation is much slower across all three original phases. The result does not establish a compiler/layout/cache cause. It remains in every center and verdict.

| Cost | A points | B/A pairs % | P/B pairs % |
| --- | --- | --- | --- |
| load.cost | 2,487.31, 1,336.41, 1,330.75 | -46.067, +1.254, +1.585 | +2.915, -1.220, -1.113 |
| read.cost | 780.758, 365.752, 357.058 | -53.315, -2.837, +4.468 | +10.265, -0.595, -3.068 |
| update.cost | 12,449.3, 8,449.48, 8,396.87 | -33.320, -1.036, +0.043 | +0.716, -0.139, -0.587 |
| complete_work.elapsed_ns | 2.3121e+10, 1.37842e+10, 1.36381e+10 | -41.077, -1.290, +1.361 | +3.512, -0.388, -1.328 |

The first enabled-profile read pair costs+10.265% versus its off control even though the three-round center is+2.040%. This adverse pair is retained.

#### Every child availability observation

| Child | Post-child boundary | Actual observed external jobs |
| --- | --- | --- |
| 00-full-r0-s1-B | observed quiet | none |
| 01-full-r0-s2-P | observed quiet | none |
| 02-full-r1-s1-A | observed quiet | none |
| 03-full-r1-s2-B | observed quiet | none |
| 04-full-r1-s3-P | observed quiet | none |
| 05-full-r2-s1-B | observed quiet | none |
| 06-full-r2-s2-P | observed quiet | none |
| 07-full-r2-s3-A | observed quiet | none |
| 08-full-r3-s1-P | observed quiet | none |
| 09-full-r3-s2-A | observed quiet | none |
| 10-full-r3-s3-B | observed quiet | none |
| 11-full-r4-s1-A | observed quiet | none |
| 12-full-r4-s2-P | observed quiet | none |
| 13-full-r4-s3-B | observed quiet | none |
| 14-millions-r1-s1-A | observed quiet | none |
| 15-millions-r1-s2-B | observed quiet | none |
| 16-millions-r1-s3-P | observed quiet | none |
| 17-millions-r1-s4-L | observed quiet | none |
| 18-millions-r2-s1-L | EXPOSED / unqualified | rustc PID1380468 stateSl cwd=/mnt/data2/webexe |
| 19-millions-r2-s2-B | observed quiet | none |
| 20-millions-r2-s3-P | observed quiet | none |
| 21-millions-r2-s4-A | observed quiet | none |
| 22-millions-r3-s1-P | observed quiet | none |
| 23-millions-r3-s2-A | observed quiet | none |
| 24-millions-r3-s3-L | EXPOSED / unqualified | rustc PID1384443 stateSl cwd=/mnt/data2/webexe |
| 25-millions-r3-s4-B | observed quiet | none |

#### Saved evidence

- [Full373 metrics](benchmark/work/optimization-20261105/next-resident-current-image/profile-v1/analysis-v1/FULL_TABLES.md), including all13 read cases, U/native write costs and all measured tails.
- [Direct native tables](benchmark/work/optimization-20261105/next-resident-current-image/profile-v1/analysis-v1/NATIVE_TABLES.md), with all three real million-native rounds and explicit exposed qualifications.
- [All seven phases](benchmark/work/optimization-20261105/next-resident-current-image/profile-v1/analysis-v1/PHASE_TABLES.md), genesis and periodic work separate, with original outer durations and unassigned residual.
- [Machine-readable analysis](benchmark/work/optimization-20261105/next-resident-current-image/profile-v1/analysis-v1/analysis.json), every stored process point and paired direction.
- [Independent arithmetic/bindings](benchmark/work/optimization-20261105/next-resident-current-image/profile-v1/analysis-v1/independent-arithmetic-and-binding-v1.json):1257 centers recomputed,78 rows checked, raw/report/sidecar hashes verified.

No engine optimization or subsequent profiler is authorized by this report. The strongest measured design direction is avoiding repeated all-Live image serialization/publication, while preserving normal durability, current-image validation and unchanged reads.


### Seven checkpoint intervals and unassigned outer residual

All numbers below are wall-time intervals from the instrumented P artifact. They are not uninstrumented CPU shares. Full RAM perturbation centers pass; million cumulative compiled-artifact gates fail. Boundary observations do not prove continuous isolation. Genesis is already charged inside original creation and is separate from completed periodic counters.

| Suite / scope | Rows | Guard/residual ms | Typed audit ms | Export ms | Precleanup ms | Encode+SHA ms | Publication IO ms | Punch/activation ms | Row total ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| full / measured / batch1 | 8 | 0.038422 | 4.065290 | 5.448531 | 0.160485 | 12.536137 | 9.404594 | 0.579511 | 32.232970 |
| full / measured / batch64 | 8 | 0.035118 | 4.314432 | 5.182936 | 0.232501 | 32.947570 | 20.020618 | 1.056891 | 63.790066 |
| full / measured / genesis/setup | 4 | 0.019947 | 0.118417 | 0.105640 | 0.025369 | 0.167277 | 0.399650 | 0.061448 | 0.897748 |
| full / smoke / genesis/setup | 1 | 0.004819 | 0.027052 | 0.025979 | 0.006483 | 0.041128 | 0.094139 | 0.015349 | 0.214949 |
| millions / measured / genesis/setup | 3 | 0.021220 | 0.087727 | 0.315350 | 0.033474 | 0.496583 | 3.468327 | 0.053079 | 4.475760 |
| millions / measured / load | 21 | 0.089392 | 122.437511 | 79.998556 | 1.779884 | 399.908994 | 587.564686 | 18.933561 | 1210.712584 |
| millions / measured / update | 33 | 0.154329 | 395.045748 | 152.766408 | 5.580280 | 972.358661 | 1648.044120 | 96.483205 | 3270.432751 |

| P process | Periodic CP count | Original outer ms | Sum inner rows ms | Unassigned outer ms |
| --- | ---: | ---: | ---: | ---: |
| 01-full-r0-s2-P | 0 | 0.000000 | 0.000000 | 0.000000 |
| 04-full-r1-s3-P | 4 | 24.532648 | 24.386791 | 0.145857 |
| 06-full-r2-s2-P | 4 | 24.383175 | 24.237867 | 0.145308 |
| 08-full-r3-s1-P | 4 | 23.445563 | 23.286469 | 0.159094 |
| 12-full-r4-s2-P | 4 | 24.258778 | 24.111909 | 0.146869 |
| 16-millions-r1-s3-P | 18 | 1507.635266 | 1499.230203 | 8.405063 |
| 20-millions-r2-s3-P | 18 | 1499.733833 | 1491.457532 | 8.276301 |
| 22-millions-r3-s1-P | 18 | 1498.706708 | 1490.457600 | 8.249108 |

The residual is preserved without assigning a mechanism. It includes work outside the inner row and observer/wrapper differences under the original outer scope. No benchmark timing has been reduced by it. For full case assignment, a checkpoint selected before the next business write is grouped by selected sequence≤256 for batch1; all later periodic checkpoints are batch64. The sidecar retains exact sequence/LSN rather than inventing per-write clocks.

Observed dominant envelopes: full RAM encode validation/serialization/SHA; disk-million publication image/selector/cleanup IO. The existing extra checkpoint audit is a smaller interval in these captures. These observations support choosing a bounded-data/metadata persistence experiment rather than claiming audit removal alone solves the architecture gap. No change is implemented or accepted by this analysis.



## Ruled-out transport controls: io_uring and TMPFS self-VM

Both tested transports are rejected for runtime port; all 18 paired costs are adverse. For opaque 544/4711-byte frames, io_uring geometric mean cost increases 433.71%/346.00% on RAM and 50.25%/43.46% on disk. TMPFS self-VM median cost increases 23.44%/56.59%. All saved byte checks and three process-return quiet observations pass; every pair remains. These are transport diagnostics, not database/native speed claims or progress toward acceptance.

The io_uring control retains both ordered data/witness fences; direct and ring capacity-query masks differ, and kernel-worker CPU use is unmeasured. Self-VM includes admission/remap/locking/teardown but has only volatile TMPFS semantics, with no production recovery proof. No engine runtime changes or benchmark scorecard changes follow. [All 18 pair values, centers, seal checks and limitations](benchmark/work/optimization-20261105/transport-independent-review-v1/REPORT.md); [machine-readable independent review](benchmark/work/optimization-20261105/transport-independent-review-v1/review.json).


## New profiling findings

An isolated frozen candidate-4 source mirror completed 22 diagnostic processes with all original trace and oracle checks. Every coarse batch-1 process has one Collect at aggregate trial 15, ordinal 5, costing 650-672 us. That contributes about 5.19 us per aggregate transaction over 128 aggregate commits (about 2.6 us over all 256 business commits). Sampled groups contain no Collect. Coarse instrumentation adds 6.38% versus capture-off, so these are diagnostic values, never accepted rates. About 75% of the first event is within the full audit; its KV/prefix split is not yet established. [Diagnostic report](benchmark/work/optimization-20261105/next-current-profile/candidate4-stage-v1/runs-1/DIAGNOSTIC_REPORT.md).

Exact matched load library-operation counts identify 999,999 4-KiB leaf scratch zeroes, 999,999 4-KiB leaf copies and 1,006,973 4-KiB Permit copies. These account for 12.317 billion argument bytes of 13.566 billion total, versus LMDB 136.6 million total. Memmove contributes only 192,792 bytes. Counts are logical operation volume, not CPU shares or physical memory bandwidth. [Exact callsites](benchmark/work/optimization-20261105/next-current-copy/README.md).

**Private append confirmation:** all focused configurations and six matched TMPFS loads pass. Load elapsed decreases 13.6-16.3% on TMPFS; the separate nine-process disk/native control improves about 15.93% median throughput, but still takes 14.73% longer than LMDB in that diagnostic. Mechanism counters fall from 13.566 GB to 1.335 GB total logical arguments; leaf full clears/copies fall 999,999 to 3,937 each and Permit full copies 1,006,973 to 11,903. All 16 default full-screen processes pass correctness, but RAM ordered-KV rates decline 3.44-4.55% in every pair. **HELD, root runtime unchanged.** [All load/read/tail results](benchmark/work/optimization-20261105/next-append-patch/candidate5-prototype-v1/full-screen-v2-1/REPORT.md).

**First-Collect attribution:** all 19 diagnostic processes pass; first-audit census is identical across four runs: 6,554 checked reads, 123 new live keys and 6,431 duplicate insert attempts. Prefix captures account for 6,479 reads and 6,390 duplicates. Zero-length binding work is zero in this fixture. Off/pristine overhead exceeds the declared 5% gate, so family times are instrumented observations, not qualified production fractions. [Report](benchmark/work/optimization-20261105/next-current-profile/first-collect-v1/controls-1/REPORT.md).

**Fresh post-update reopen attribution:** six exact current5 diagnostic processes pass. History replay including historical audits consumes about 97.5% of reopen; linear free-queue position code accounts for 19.3-19.9% of history samples, while identified audit code is 56.8-57.6% as a disjoint PC union. Pool creation is about 38 ms, final audit 12 ms, unused-suffix clear plus sync about 119 ms. These do not explain the multi-second history CPU. Unused suffix is 52.9% of WAL capacity; sparse allocation alone cannot establish LMDB parity. [Full evidence](benchmark/work/optimization-20261105/next-reopen-allocation/plan-v1/measurements-v1/FINDINGS.md).

The first fixed-row local-validation kernel was rejected: 2,160,158 differential cases passed but empty/small pages materially regressed. Production is unchanged by that experiment.

## Private Capture-proof collection screen, not accepted

All 16 disk/RAM processes pass. RAM single-write aggregate throughput improves 5.38% and combined business-write throughput 3.39%; batch64 is essentially unchanged. This remains 65,800/s versus LMDB 134,649/s. The RAM IPv4-miss pooled p99 change from 121 to 130 ns is adverse and is not dismissed. Full three-arm acceptance is still required.

### Complete private screen measurements

All16 predeclared processes passed every full semantic/cold/old-reader oracle, source/ELF/native provenance/workload/trace guard, numeric/policy backend accounting and full-file/every-slice raw latency hash check. No retry or exclusion occurred. The root runtime was untouched; this is a private source5 screen, not acceptance.

Rates are geometric means of four paired candidate/pristine throughput ratios; absolute variant rates use the reciprocal mean per-operation cost. Every paired change and contemporaneous native result is retained. Positive tail changes are slower. Mean trial p99 and all-process pooled raw p99 describe distinct populations.

The measured business gain is modest but material for RAM single-record commits:
aggregate throughput improves5.38% in all four pairs (+4.25% through+6.51%), and
complete combined throughput improves3.39% in all four (+2.73% through+4.03%).
Combined mean cost falls13.279→12.844us, about0.435us saved per business commit.
Batch64 shows no improvement. This does not close LMDB parity: candidate RAM
single aggregate throughput is65,800/s versus contemporaneous LMDB134,649/s.

Serving throughput does not show the repeated multi-percent ordered-KV loss
that held append6: RAM mixed/hit/miss changes are-0.12%/-0.30%/+0.40%. An adverse
read-tail point remains: RAM IPv4 misses pooled p99 is121→130ns (+7.44%), while
mean-trial p99 is+0.90%. Those populations differ and neither is dismissed. RAM
single-write mean-trial p99 improves5.12%, pooled raw p99 improves20.38%; native
pooled write p99 simultaneously worsens22.21%, which is also retained.

Persistent-disk single aggregate throughput changes+2.13%, while native changes
+2.15%; an isolated persistent gain is not established. Disk batch64 aggregate
changes-0.76% with three adverse pairs and its combined result is+0.17% with mixed
signs. Disk cold reopen observations are also adverse on average. All cells and
pair signs remain below; no source/code-layout or host-cause explanation is
claimed, and no production or statistical nonregression acceptance follows.

## Persistent workspace disk

| Case | Pristine ULMDB/s | Candidate ULMDB/s | Pristine LMDB/s | Candidate LMDB/s | Candidate/LMDB | Paired ULMDB change | Four paired changes | Native change |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| ordered_kv_mixed | 6,351,387 | 6,375,056 | 4,102,921 | 4,076,607 | 1.56× | +0.37% | -0.00%, +1.69%, -0.14%, -0.05% | -0.64% |
| ordered_kv_hits | 6,657,623 | 6,626,788 | 4,184,353 | 4,183,761 | 1.58× | -0.46% | -0.26%, +0.01%, +0.06%, -1.63% | -0.01% |
| ordered_kv_misses | 6,588,503 | 6,662,435 | 4,010,230 | 3,997,673 | 1.67× | +1.12% | +0.06%, +2.40%, +1.27%, +0.78% | -0.31% |
| ipv4_lpm_mixed | 5,449,334 | 5,509,082 | 2,720,266 | 2,645,512 | 2.08× | +1.10% | +0.28%, +2.27%, +1.37%, +0.48% | -2.74% |
| ipv4_lpm_hits | 3,139,240 | 3,177,041 | 2,166,078 | 2,130,421 | 1.49× | +1.21% | +1.53%, +1.17%, +1.51%, +0.61% | -1.64% |
| ipv4_lpm_misses | 21,392,855 | 21,739,684 | 4,034,674 | 3,990,592 | 5.45× | +1.62% | +1.52%, +2.12%, +1.57%, +1.27% | -1.06% |
| ipv4_lpm_outside_cover_misses | 99,796,664 | 100,778,355 | 89,612,635 | 85,541,713 | 1.18× | +0.97% | -0.22%, +0.08%, +3.90%, +0.18% | -4.66% |
| ipv4_lpm_inside_gap_misses | 11,445,271 | 11,634,796 | 2,086,869 | 2,039,201 | 5.71× | +1.66% | +2.18%, +1.65%, +2.01%, +0.78% | -2.23% |
| ipv6_lpm_mixed | 4,341,333 | 4,371,542 | 1,554,633 | 1,561,510 | 2.80× | +0.70% | +1.31%, +0.80%, +0.57%, +0.12% | +0.45% |
| ipv6_lpm_hits | 2,497,207 | 2,503,048 | 1,227,016 | 1,246,140 | 2.01× | +0.24% | +0.84%, +0.39%, -0.36%, +0.07% | +1.57% |
| ipv6_lpm_misses | 16,948,538 | 17,160,682 | 2,232,066 | 2,226,631 | 7.71× | +1.25% | +1.36%, +2.32%, +0.35%, +0.99% | -0.24% |
| ipv6_lpm_outside_cover_misses | 96,368,126 | 95,500,437 | 90,043,446 | 85,366,683 | 1.12× | -0.88% | -0.03%, -5.22%, -0.05%, +1.91% | -5.37% |
| ipv6_lpm_inside_gap_misses | 9,162,395 | 9,334,086 | 1,132,142 | 1,129,229 | 8.27× | +1.87% | +2.31%, +1.63%, +1.93%, +1.63% | -0.25% |
| write_batch1 | 15,199 | 15,531 | 10,766 | 10,998 | 1.41× | +2.13% | +0.54%, +8.22%, -2.69%, +2.73% | +2.15% |
| write_batch64 | 490,854 | 487,132 | 176,756 | 173,908 | 2.80× | -0.76% | -3.55%, -0.68%, +6.39%, -4.81% | -1.60% |

| Case | Trial p99 change | ULMDB pooled p99 pristine→candidate ns | Pooled change | LMDB pooled p99 pristine→candidate ns | Native pooled change |
|---|---:|---:|---:|---:|---:|
| ordered_kv_mixed | -0.90% | 240→240 | +0.00% | 340→341 | +0.29% |
| ordered_kv_hits | -0.71% | 231→231 | +0.00% | 331→340 | +2.72% |
| ordered_kv_misses | -0.86% | 231→231 | +0.00% | 341→341 | +0.00% |
| ipv4_lpm_mixed | +0.43% | 431→431 | +0.00% | 762→771 | +1.18% |
| ipv4_lpm_hits | +1.10% | 431→431 | +0.00% | 782→791 | +1.15% |
| ipv4_lpm_misses | -0.59% | 121→121 | +0.00% | 611→621 | +1.64% |
| ipv4_lpm_outside_cover_misses | -6.89% | 41→41 | +0.00% | 50→50 | +0.00% |
| ipv4_lpm_inside_gap_misses | -0.36% | 121→121 | +0.00% | 621→631 | +1.61% |
| ipv6_lpm_mixed | +1.16% | 521→521 | +0.00% | 1242→1252 | +0.81% |
| ipv6_lpm_hits | +1.48% | 541→551 | +1.85% | 1252→1272 | +1.60% |
| ipv6_lpm_misses | -1.37% | 141→141 | +0.00% | 1072→1072 | +0.00% |
| ipv6_lpm_outside_cover_misses | -1.00% | 41→41 | +0.00% | 50→41 | -18.00% |
| ipv6_lpm_inside_gap_misses | -0.25% | 141→141 | +0.00% | 1092→1102 | +0.92% |
| write_batch1 | -10.50% | 311382→309979 | -0.45% | 323996→327031 | +0.94% |
| write_batch64 | +0.66% | 370044→375244 | +1.41% | 525680→544596 | +3.60% |

## TMPFS/RAM

| Case | Pristine ULMDB/s | Candidate ULMDB/s | Pristine LMDB/s | Candidate LMDB/s | Candidate/LMDB | Paired ULMDB change | Four paired changes | Native change |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| ordered_kv_mixed | 6,362,791 | 6,355,335 | 4,117,479 | 4,096,660 | 1.55× | -0.12% | -0.67%, +0.29%, -0.41%, +0.34% | -0.51% |
| ordered_kv_hits | 6,684,810 | 6,664,921 | 4,196,505 | 4,181,134 | 1.59× | -0.30% | +0.24%, -0.12%, -1.09%, -0.22% | -0.37% |
| ordered_kv_misses | 6,644,826 | 6,671,564 | 4,062,310 | 4,029,306 | 1.66× | +0.40% | +1.01%, +0.17%, +0.41%, +0.03% | -0.81% |
| ipv4_lpm_mixed | 5,473,449 | 5,533,762 | 2,677,135 | 2,623,574 | 2.11× | +1.10% | +1.25%, +1.35%, +0.90%, +0.91% | -2.02% |
| ipv4_lpm_hits | 3,169,553 | 3,208,985 | 2,140,797 | 2,126,847 | 1.51× | +1.24% | +1.33%, +1.83%, +1.26%, +0.57% | -0.66% |
| ipv4_lpm_misses | 21,313,966 | 21,771,251 | 3,914,326 | 3,867,393 | 5.63× | +2.14% | +3.38%, +2.32%, +1.10%, +1.78% | -1.25% |
| ipv4_lpm_outside_cover_misses | 100,958,475 | 100,899,424 | 85,962,926 | 82,213,284 | 1.23× | -0.06% | -0.01%, -0.03%, -0.05%, -0.14% | -1.81% |
| ipv4_lpm_inside_gap_misses | 11,336,009 | 11,698,660 | 2,023,475 | 2,000,378 | 5.85× | +3.16% | +8.08%, +2.43%, +1.77%, +0.54% | -1.25% |
| ipv6_lpm_mixed | 4,344,456 | 4,386,898 | 1,564,228 | 1,562,543 | 2.81× | +0.98% | +1.08%, +0.76%, +1.11%, +0.96% | -0.11% |
| ipv6_lpm_hits | 2,505,265 | 2,517,395 | 1,242,018 | 1,243,858 | 2.02× | +0.48% | +0.18%, +0.37%, +0.47%, +0.92% | +0.16% |
| ipv6_lpm_misses | 16,802,167 | 17,154,305 | 2,243,982 | 2,228,437 | 7.70× | +2.09% | +1.82%, +2.57%, +3.82%, +0.21% | -0.70% |
| ipv6_lpm_outside_cover_misses | 96,585,404 | 97,196,046 | 86,604,807 | 80,934,285 | 1.20× | +0.62% | -0.03%, +2.48%, -0.24%, +0.31% | -3.96% |
| ipv6_lpm_inside_gap_misses | 9,149,756 | 9,341,161 | 1,133,708 | 1,134,993 | 8.23× | +2.09% | +2.51%, +1.38%, +2.63%, +1.84% | +0.10% |
| write_batch1 | 62,441 | 65,800 | 132,412 | 134,649 | 0.49× | +5.38% | +5.38%, +4.25%, +6.51%, +5.40% | +1.70% |
| write_batch64 | 986,139 | 984,859 | 739,126 | 731,608 | 1.35× | -0.13% | +0.93%, -0.04%, +0.29%, -1.68% | -1.01% |

| Case | Trial p99 change | ULMDB pooled p99 pristine→candidate ns | Pooled change | LMDB pooled p99 pristine→candidate ns | Native pooled change |
|---|---:|---:|---:|---:|---:|
| ordered_kv_mixed | -0.67% | 240→240 | +0.00% | 340→340 | +0.00% |
| ordered_kv_hits | -0.24% | 230→230 | +0.00% | 331→340 | +2.72% |
| ordered_kv_misses | -0.24% | 231→230 | -0.43% | 341→341 | +0.00% |
| ipv4_lpm_mixed | -0.04% | 421→421 | +0.00% | 761→771 | +1.31% |
| ipv4_lpm_hits | -0.16% | 431→431 | +0.00% | 772→772 | +0.00% |
| ipv4_lpm_misses | +0.90% | 121→130 | +7.44% | 611→611 | +0.00% |
| ipv4_lpm_outside_cover_misses | -4.23% | 41→41 | +0.00% | 51→50 | -1.96% |
| ipv4_lpm_inside_gap_misses | +0.84% | 121→121 | +0.00% | 621→621 | +0.00% |
| ipv6_lpm_mixed | -0.51% | 521→521 | +0.00% | 1232→1233 | +0.08% |
| ipv6_lpm_hits | -0.68% | 531→531 | +0.00% | 1233→1252 | +1.54% |
| ipv6_lpm_misses | +0.65% | 141→141 | +0.00% | 1062→1062 | +0.00% |
| ipv6_lpm_outside_cover_misses | -0.72% | 41→41 | +0.00% | 51→50 | -1.96% |
| ipv6_lpm_inside_gap_misses | -0.11% | 141→141 | +0.00% | 1072→1082 | +0.93% |
| write_batch1 | -5.12% | 16572→13195 | -20.38% | 8526→10420 | +22.21% |
| write_batch64 | +0.35% | 247621→247671 | +0.02% | 97285→99028 | +1.79% |

## Complete business-write accounting

Combined rates include all 256 business commits per batch: aggregate phase plus separately sampled transaction latencies, preserving automatic Collect work. They exclude per-trial validation/dispatch gaps, as documented by the unchanged harness.

| Filesystem | Batch | Paired combined rate change | Four paired changes |
|---|---:|---:|---|
| disk | 1 | +2.05% | -2.23%, +6.70%, +4.22%, -0.23% |
| disk | 64 | +0.17% | -0.78%, +3.01%, +1.98%, -3.39% |
| ram | 1 | +3.39% | +4.03%, +2.73%, +3.77%, +3.03% |
| ram | 64 | -0.16% | +0.17%, -0.19%, -0.19%, -0.44% |

## Complete cold-check open observations

The full suite opens its small three-database fixture and preserves old readers; it does not represent the separate million-key post-update reopen workload. All old/new KV/IP/cursor/pointer oracles passed.

| Filesystem | Pair | Pristine open ms | Candidate open ms |
|---|---:|---:|---:|
| disk | 1 | 82.495 | 85.637 |
| disk | 2 | 80.958 | 81.981 |
| disk | 3 | 81.468 | 84.958 |
| disk | 4 | 82.622 | 81.863 |
| ram | 1 | 43.278 | 42.403 |
| ram | 2 | 43.765 | 43.665 |
| ram | 3 | 42.309 | 42.254 |
| ram | 4 | 42.664 | 44.010 |

Machine-readable metrics.json and metrics.csv retain exact rates, tails, every paired ratio, native controls and cold observations. summary.json retains all process metrics and source/build provenance. All reports/raw samples/commands/stdout/stderr/receipts remain. First-Collect family timing overhead exceeded its gate, and no instrumented share is used to interpret these ordinary measurements. Separate append/load controls are not merged into this source5 screen. No statistical nonregression, complete acceptance or LMDB-parity claim is made.

## Full combined/native cost and rate accounting

All256 business commits per batch are included. Absolute costs average the four
process costs; paired rate changes use geometric means of all four pair ratios.

| FS | Batch | ULMDB pristine→candidate ns/commit | ULMDB pristine→candidate records/s | LMDB pristine→candidate records/s | ULMDB paired change | Native paired change |
|---|---:|---:|---:|---:|---:|---:|
| disk | 1 | 62457→61204 | 16,011→16,339 | 11,293→11,473 | +2.05% | +1.59% |
| disk | 64 | 128547→128354 | 497,871→498,620 | 181,613→180,235 | +0.17% | -0.75% |
| ram | 1 | 13279→12844 | 75,305→77,857 | 137,277→138,563 | +3.39% | +0.94% |
| ram | 64 | 64356→64461 | 994,462→992,852 | 748,731→741,341 | -0.16% | -0.99% |

## Full cold-check cost accounting

| FS | Field | Pristine mean ms | Candidate mean ms | Paired change | All pair changes |
|---|---|---:|---:|---:|---|
| disk | environment_drop_ns | 0.098 | 0.097 | -1.43% | -11.60%, +1.97%, -0.87%, +5.62% |
| disk | open_ns | 81.886 | 83.610 | +2.09% | +3.81%, +1.26%, +4.28%, -0.92% |
| disk | experimental_reenable_ns | 0.002 | 0.002 | -10.49% | -19.41%, -4.87%, -6.54%, -10.42% |
| disk | validation_ns | 6.547 | 6.485 | -0.95% | -1.82%, -2.06%, -0.02%, +0.14% |
| disk | complete_cold_check_ns | 92.638 | 94.306 | +1.79% | +3.17%, +1.01%, +3.79%, -0.77% |
| ram | environment_drop_ns | 0.095 | 0.098 | +2.57% | +1.87%, +7.61%, -1.45%, +2.44% |
| ram | open_ns | 43.004 | 43.083 | +0.18% | -2.02%, -0.23%, -0.13%, +3.16% |
| ram | experimental_reenable_ns | 0.002 | 0.002 | +13.06% | -0.91%, +54.46%, +15.78%, -7.81% |
| ram | validation_ns | 6.572 | 6.563 | -0.13% | -2.69%, -2.00%, +2.42%, +1.84% |
| ram | complete_cold_check_ns | 53.863 | 54.104 | +0.44% | -1.97%, +0.13%, +0.29%, +3.38% |

## Preserved state, footprint and unavailable observations

Every numeric/policy backend_before/after field matched exactly across all16
processes; per-fixture opaque commit IDs are retained without equality claims.
Sequence advances1→513; selected WAL bytes advance725,136→2,205,392; pool reused
pages finish7,722. Configured pools, metadata budgets/charges, retained workspaces,
cache entries/bytes and policy labels are unchanged. Original footprint, setup,
memory and cold fields remain in all raw reports; null pathname-attributed
Resident RSS/PSS is unavailable, never zero. Exact first Collect ordinal and
release insertion count are not exposed by these uninstrumented reports.

all-trial-tails.json retains every p50/p95/p99 for both engines: reads are original
reported/raw-verified values; write p50/p95 are derived from each unchanged eight
raw samples, and p99 matches the original report. metrics.json retains every
paired/native/aggregate/combined/cold value. No adverse observation was excluded.


## Retained iteration 6: previous accepted versus current, complete matrix

All 39 process runs completed with passing oracles. Full suite: 4 pairs/filesystem; million suite: 3 ULMDB pairs plus contemporary LMDB/SQLite controls.

Throughput is operations/s; higher is better. Rates are geometric means of process rates. Change compares candidate with the frozen ULMDB baseline.

|Filesystem / operation|ULMDB baseline/s|ULMDB candidate/s|Change|Local LMDB/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|
|disk / ordered_kv_mixed|6,243,148|7,330,137|+17.41%|4,076,748|+79.80%|
|disk / ordered_kv_hits|6,649,409|7,652,713|+15.09%|4,142,882|+84.72%|
|disk / ordered_kv_misses|6,479,541|7,708,110|+18.96%|3,978,780|+93.73%|
|disk / ipv4_lpm_mixed|5,469,619|5,513,008|+0.79%|2,680,988|+105.63%|
|disk / ipv4_lpm_hits|3,174,180|3,188,948|+0.47%|2,153,764|+48.06%|
|disk / ipv4_lpm_misses|21,482,051|21,655,868|+0.81%|3,879,159|+458.26%|
|disk / ipv4_lpm_outside_cover_misses|100,325,640|100,917,093|+0.59%|92,286,003|+9.35%|
|disk / ipv4_lpm_inside_gap_misses|11,466,178|11,606,005|+1.22%|2,015,409|+475.86%|
|disk / ipv6_lpm_mixed|4,392,354|4,284,398|-2.46%|1,556,785|+175.21%|
|disk / ipv6_lpm_hits|2,516,201|2,468,934|-1.88%|1,241,271|+98.90%|
|disk / ipv6_lpm_misses|17,237,117|17,149,138|-0.51%|2,211,360|+675.50%|
|disk / ipv6_lpm_outside_cover_misses|97,226,115|97,491,465|+0.27%|91,485,018|+6.57%|
|disk / ipv6_lpm_inside_gap_misses|9,220,200|9,332,632|+1.22%|1,120,666|+732.78%|
|disk / durable_batch1|14,927|15,826|+6.02%|10,741|+47.35%|
|disk / durable_batch64|493,000|484,010|-1.82%|178,232|+171.56%|
|ram / ordered_kv_mixed|6,250,079|7,120,201|+13.92%|4,052,865|+75.68%|
|ram / ordered_kv_hits|6,651,004|7,469,252|+12.30%|4,134,827|+80.64%|
|ram / ordered_kv_misses|6,497,453|7,495,281|+15.36%|3,977,680|+88.43%|
|ram / ipv4_lpm_mixed|5,462,122|5,492,890|+0.56%|2,630,134|+108.84%|
|ram / ipv4_lpm_hits|3,157,172|3,171,227|+0.45%|2,100,200|+51.00%|
|ram / ipv4_lpm_misses|21,409,697|21,543,522|+0.63%|3,906,181|+451.52%|
|ram / ipv4_lpm_outside_cover_misses|100,693,970|99,939,994|-0.75%|94,153,863|+6.15%|
|ram / ipv4_lpm_inside_gap_misses|11,579,041|11,551,516|-0.24%|1,990,091|+480.45%|
|ram / ipv6_lpm_mixed|4,378,140|4,271,910|-2.43%|1,550,202|+175.57%|
|ram / ipv6_lpm_hits|2,513,207|2,460,443|-2.10%|1,233,707|+99.43%|
|ram / ipv6_lpm_misses|17,278,087|17,141,014|-0.79%|2,217,213|+673.09%|
|ram / ipv6_lpm_outside_cover_misses|97,561,460|97,385,014|-0.18%|94,383,549|+3.18%|
|ram / ipv6_lpm_inside_gap_misses|9,339,062|9,300,426|-0.41%|1,121,264|+729.46%|
|ram / durable_batch1|58,600|62,048|+5.88%|135,053|-54.06%|
|ram / durable_batch64|957,219|967,352|+1.06%|739,504|+30.81%|

### Million-key workload

|Operation|ULMDB baseline/s|ULMDB candidate/s|Change|LMDB/s|SQLite/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|---:|
|load|1,161,940|1,396,940|+20.22%|1,539,753|817,306|-9.28%|
|read|2,565,550|2,776,989|+8.24%|1,660,288|455,490|+67.26%|
|update|144,676|145,257|+0.40%|128,143|58,247|+13.36%|

### Sampled latency

Nanoseconds; lower is better. These are geometric means of trial-sampled p99s. The original write trial has only eight samples; this is not a population tail guarantee.

|Filesystem / operation|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / ordered_kv_mixed|233|204|-12.42%|343|
|disk / ordered_kv_hits|226|200|-11.47%|342|
|disk / ordered_kv_misses|229|199|-13.28%|344|
|disk / ipv4_lpm_mixed|423|424|+0.25%|780|
|disk / ipv4_lpm_hits|434|434|+0.04%|791|
|disk / ipv4_lpm_misses|115|124|+7.63%|636|
|disk / ipv4_lpm_outside_cover_misses|41|42|+0.97%|45|
|disk / ipv4_lpm_inside_gap_misses|117|124|+6.38%|650|
|disk / ipv6_lpm_mixed|514|524|+1.87%|1,240|
|disk / ipv6_lpm_hits|541|544|+0.51%|1,258|
|disk / ipv6_lpm_misses|144|142|-1.32%|1,072|
|disk / ipv6_lpm_outside_cover_misses|41|42|+0.97%|43|
|disk / ipv6_lpm_inside_gap_misses|145|150|+3.55%|1,137|
|disk / durable_batch1|82,013|86,678|+5.69%|207,657|
|disk / durable_batch64|227,456|199,543|-12.27%|471,235|
|ram / ordered_kv_mixed|238|212|-10.85%|350|
|ram / ordered_kv_hits|232|211|-8.89%|354|
|ram / ordered_kv_misses|234|212|-9.52%|358|
|ram / ipv4_lpm_mixed|430|433|+0.52%|785|
|ram / ipv4_lpm_hits|443|452|+2.13%|798|
|ram / ipv4_lpm_misses|121|128|+5.59%|664|
|ram / ipv4_lpm_outside_cover_misses|42|42|-0.31%|48|
|ram / ipv4_lpm_inside_gap_misses|119|126|+6.16%|694|
|ram / ipv6_lpm_mixed|531|542|+2.10%|1,259|
|ram / ipv6_lpm_hits|556|560|+0.75%|1,286|
|ram / ipv6_lpm_misses|148|149|+0.53%|1,124|
|ram / ipv6_lpm_outside_cover_misses|42|42|-0.65%|45|
|ram / ipv6_lpm_inside_gap_misses|148|153|+3.15%|1,189|
|ram / durable_batch1|12,855|12,155|-5.45%|7,936|
|ram / durable_batch64|107,056|104,316|-2.56%|94,415|

### Pooled write samples

Geometric mean of each process pooled sampled p99; nanoseconds, lower is better. These preserve adverse tails separately from the trial-sampled estimates above.

|Filesystem / batch|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / durable_batch1|313,043|311,200|-0.59%|326,784|
|disk / durable_batch64|386,553|381,839|-1.22%|522,676|
|ram / durable_batch1|15,891|14,594|-8.16%|9,030|
|ram / durable_batch64|254,644|263,298|+3.40%|105,897|

### Lifecycle and file costs

|Metric|Baseline|Candidate|Change|
|---|---:|---:|---:|
|Disk bare open, ms|83.806|84.822|+1.21%|
|RAM bare open, ms|42.362|44.034|+3.95%|
|Million close/reopen, ms|7,058.407|7,088.012|+0.42%|
|Million apparent file bytes|268,435,456|268,435,456|+0.00%|
|Million allocated file bytes|268,435,456|268,436,821|+0.00%|

Million-workload costs against contemporaneous native engines:

|Metric|ULMDB|LMDB|SQLite|
|---|---:|---:|---:|
|Close/reopen, ms|7,088.012|3.892|9.380|
|Apparent files, MiB|256.000|33.047|24.145|
|Allocated files, MiB|256.001|33.043|24.145|
|File count|2|2|1|

Complete 237-metric comparison: [benchmark/work/optimization-20261105/iteration-6-three-arm/accepted-vs-candidate.md](benchmark/work/optimization-20261105/iteration-6-three-arm/accepted-vs-candidate.md). It includes combined write windows, pooled sampled tails, setup, validation, resources, all individual observations and uncertainty. Negative means remain visible; statistical overlap does not establish equivalence.

## Separate private recovery-index control

The private cold-index diagnostic ran nine ordinary, uninstrumented processes: three rotating rounds of source5 ULMDB, source5 with the temporary recovery index, and native LMDB. Each used the complete 1M-load/10M-read/1M-update workload, batch1000/seed73, CPU0, persistent disk and normal durability. All nine passed every original oracle and exact setting/counter/source/binary guard; no retries or exclusions were used.

Geometric means; the same three contemporaneous native observations are shared controls, not six independent measurements:

|Metric|Source5|Source5 + cold index|Change vs source5|Native LMDB|
|---|---:|---:|---:|---:|
|Load, operations/s|1,160,666|1,190,115|+2.537%|1,563,729|
|Read, queries/s|2,528,388|2,575,312|+1.856%|1,691,025|
|Update, operations/s|145,056|144,666|−0.269%|128,281|
|Setup, ms|413.518|423.747|+2.474% cost|0.982|
|Close/reopen, ms|7,007.708|5,501.373|−21.495% cost|3.802|
|Allocated bytes, before and after reopen|268,435,456|268,435,456|0%|34,648,064|
|Apparent bytes, before and after reopen|268,435,456|268,435,456|0%|34,652,160|
|Files, before and after reopen|2|2|0%|2|

Reopen, load and read improved in all three pairs. Update throughput and setup retain adverse overall point estimates, each adverse in two of three pairs. All eleven target metrics retain insufficient-repeat flags under the comparator's default minimum of four; these three pairs establish neither equivalence nor nonregression. This diagnostic covers no RAM single/batch writes or sampled tails, makes no retention claim, and leaves large reopen/allocation gaps to LMDB.

All nine engine runs passed. A subsequent full-suite comparison parser rejected the intentionally absent SQLite controls; its failure is preserved. An explicitly scoped, offline two-engine adapter compared the same captures without adding observations or rerunning workloads; the public comparator was unchanged. Full observations and all 22 target/control cells are retained in `next-replay-index/candidate5-v1/ordinary-millions-v2/FINDINGS.md` and `diagnostic-comparison.{json,md}`.


## Historical iteration 5: HELD after calibration

All 72 A/A/B processes passed with identical-byte accepted controls. The full [calibration report](benchmark/work/optimization-20261105/iteration-5-aa-confirmation/README.md) keeps all three 204-metric comparisons and every observation. Context variability does not explain away the adverse candidate results.

# Iteration 5: full result, HELD after identical-binary calibration

39 processes passed all oracles with sources/binaries unchanged. Higher throughput is better; lower sampled p99 and lifecycle cost is better. Previous is retained iteration3, rerun in this campaign.

|Filesystem / case|Previous ULMDB/s|Candidate ULMDB/s|Change|LMDB/s|Previous p99 ns|Candidate p99 ns|p99 change|LMDB p99 ns|
|---|---:|---:|---:|---:|---:|---:|---:|---:|
|disk / ordered_kv_mixed|6,265,829|6,355,897|+1.44%|4,095,584|233|232|-0.31%|335|
|disk / ordered_kv_hits|6,683,539|6,688,785|+0.08%|4,160,598|223|223|-0.12%|333|
|disk / ordered_kv_misses|6,509,986|6,718,349|+3.20%|4,039,263|230|224|-2.50%|337|
|disk / ipv4_lpm_mixed|5,471,599|5,506,544|+0.64%|2,653,423|422|420|-0.67%|762|
|disk / ipv4_lpm_hits|3,165,261|3,188,590|+0.74%|2,137,455|433|427|-1.59%|777|
|disk / ipv4_lpm_misses|21,542,519|21,518,270|-0.11%|3,916,010|117|114|-2.14%|588|
|disk / ipv4_lpm_outside_cover_misses|100,135,414|100,934,599|+0.80%|86,348,967|41|41|+0.97%|43|
|disk / ipv4_lpm_inside_gap_misses|11,488,484|11,560,822|+0.63%|2,000,289|119|114|-4.10%|605|
|disk / ipv6_lpm_mixed|4,378,566|4,376,183|-0.05%|1,544,322|516|512|-0.75%|1,247|
|disk / ipv6_lpm_hits|2,523,562|2,519,103|-0.18%|1,221,234|527|523|-0.84%|1,263|
|disk / ipv6_lpm_misses|17,279,757|17,259,535|-0.12%|2,213,662|142|142|-0.10%|1,077|
|disk / ipv6_lpm_outside_cover_misses|97,504,672|97,330,200|-0.18%|86,707,791|41|41|+0.00%|42|
|disk / ipv6_lpm_inside_gap_misses|9,324,489|9,287,542|-0.40%|1,119,906|144|143|-0.74%|1,102|
|disk / durable_batch1|15,655|15,628|-0.17%|10,955|80,028|84,901|+6.09%|191,338|
|disk / durable_batch64|492,663|487,836|-0.98%|177,968|205,432|220,634|+7.40%|448,833|
|ram / ordered_kv_mixed|6,256,869|6,349,718|+1.48%|4,067,555|236|232|-1.67%|344|
|ram / ordered_kv_hits|6,667,721|6,689,443|+0.33%|4,142,512|224|224|+0.14%|341|
|ram / ordered_kv_misses|6,479,037|6,696,503|+3.36%|4,016,530|231|223|-3.27%|344|
|ram / ipv4_lpm_mixed|5,499,101|5,497,201|-0.03%|2,601,231|422|420|-0.57%|767|
|ram / ipv4_lpm_hits|3,183,503|3,192,860|+0.29%|2,099,239|430|429|-0.25%|780|
|ram / ipv4_lpm_misses|21,551,870|21,644,010|+0.43%|3,735,427|112|114|+2.38%|605|
|ram / ipv4_lpm_outside_cover_misses|98,979,009|100,913,617|+1.95%|95,696,127|41|41|+0.31%|48|
|ram / ipv4_lpm_inside_gap_misses|11,536,443|11,573,873|+0.32%|1,921,715|113|112|-0.14%|624|
|ram / ipv6_lpm_mixed|4,407,042|4,369,571|-0.85%|1,542,487|512|520|+1.63%|1,251|
|ram / ipv6_lpm_hits|2,532,981|2,515,017|-0.71%|1,224,034|528|528|-0.04%|1,261|
|ram / ipv6_lpm_misses|17,386,398|17,249,567|-0.79%|2,201,253|142|143|+0.77%|1,103|
|ram / ipv6_lpm_outside_cover_misses|97,516,724|97,457,610|-0.06%|95,052,563|41|41|+0.00%|44|
|ram / ipv6_lpm_inside_gap_misses|9,318,822|9,193,072|-1.35%|1,119,596|143|143|+0.11%|1,110|
|ram / durable_batch1|59,093|62,056|+5.01%|133,734|12,768|11,664|-8.65%|8,222|
|ram / durable_batch64|955,935|985,940|+3.14%|740,522|104,449|102,543|-1.83%|93,726|

|Million operation|Previous ULMDB/s|Candidate ULMDB/s|Change|LMDB/s|SQLite/s|
|---|---:|---:|---:|---:|---:|
|load|1,188,909|1,171,288|-1.48%|1,588,200|826,102|
|read|2,584,743|2,562,927|-0.84%|1,691,977|458,550|
|update|145,676|145,129|-0.38%|128,771|57,966|

|Pooled sampled write p99|Previous ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk batch1|210,160|314,871|+49.82%|324,784|
|disk batch64|371,615|397,543|+6.98%|520,264|
|ram batch1|15,389|13,069|-15.08%|10,854|
|ram batch64|254,702|257,345|+1.04%|104,502|

|Lifecycle / files|Previous ULMDB|Candidate ULMDB|LMDB|SQLite|
|---|---:|---:|---:|---:|
|Close/reopen ms|7,072.209|7,037.209|3.777|9.254|
|Apparent MiB|256.000|256.000|33.047|24.145|
|Allocated MiB|256.000|256.000|33.043|24.145|


## Iteration 4 complete confirmation and decision

**Uniform candidate held; RAM-specific next candidate under qualification.** The separate primary and confirmation campaigns remain intact; the following table is descriptive pooling of all 12 process pairs per filesystem. Full tail/ordering/LMDB-control analysis: [decision review](benchmark/work/optimization-20261105/iteration-4-decision-review/README.md).

Combined descriptive result: all primary and supplemental observations; 12 process pairs/filesystem. Original campaigns remain separate. Throughput is ops/s; p99 is trial-sampled ns. Deltas compare candidate4 with accepted3.

|Filesystem / workload|ULMDB/s|LMDB/s|Throughput change|ULMDB p99 ns|LMDB p99 ns|p99 change|
|---|---:|---:|---:|---:|---:|---:|
|disk / KV mixed|6,358,992|4,041,180|+1.58%|231|344|-0.28%|
|disk / KV hits|6,677,655|4,141,801|-0.10%|222|342|-0.17%|
|disk / KV misses|6,653,926|3,992,617|+2.21%|223|344|-1.96%|
|disk / IPv4 mixed|5,496,182|2,599,754|+0.31%|419|764|-0.63%|
|disk / IPv4 hits|3,182,535|2,093,567|+0.14%|428|778|-0.18%|
|disk / IPv4 misses|21,615,900|3,726,747|+0.52%|116|610|+2.02%|
|disk / IPv4 outside cover misses|100,337,762|88,632,454|+0.52%|41|45|+0.23%|
|disk / IPv4 inside gap misses|11,569,252|1,919,653|+0.03%|116|619|+1.12%|
|disk / IPv6 mixed|4,362,607|1,540,930|-0.63%|511|1,242|+0.30%|
|disk / IPv6 hits|2,513,370|1,229,350|-0.27%|524|1,246|+0.01%|
|disk / IPv6 misses|17,233,946|2,206,876|-0.46%|139|1,073|-1.22%|
|disk / IPv6 outside cover misses|97,067,635|87,566,589|+0.41%|41|44|+0.21%|
|disk / IPv6 inside gap misses|9,401,492|1,117,370|+0.73%|141|1,093|-0.54%|
|disk / Writes batch 1|15,873|10,692|+3.42%|85,429|199,251|+7.46%|
|disk / Writes batch 64|495,773|179,111|+0.61%|216,894|462,486|+2.01%|
|ram / KV mixed|6,381,390|4,056,842|+1.48%|231|344|-0.29%|
|ram / KV hits|6,677,537|4,148,550|+0.09%|223|344|+0.35%|
|ram / KV misses|6,652,543|4,005,568|+2.00%|223|345|-1.20%|
|ram / IPv4 mixed|5,519,266|2,586,456|+0.45%|417|764|-0.79%|
|ram / IPv4 hits|3,196,060|2,093,201|+0.37%|426|779|-1.09%|
|ram / IPv4 misses|21,591,149|3,705,527|-0.01%|113|623|-1.25%|
|ram / IPv4 outside cover misses|100,902,507|91,946,377|+0.55%|41|45|-0.22%|
|ram / IPv4 inside gap misses|11,577,852|1,905,628|+0.16%|113|627|-2.06%|
|ram / IPv6 mixed|4,372,096|1,543,034|-0.57%|511|1,242|-0.17%|
|ram / IPv6 hits|2,518,802|1,230,032|-0.34%|524|1,251|+0.11%|
|ram / IPv6 misses|17,261,057|2,211,437|-0.23%|141|1,073|-0.15%|
|ram / IPv6 outside cover misses|95,731,906|91,238,872|-1.26%|41|44|-0.23%|
|ram / IPv6 inside gap misses|9,372,601|1,117,668|+0.95%|141|1,092|-0.17%|
|ram / Writes batch 1|62,678|134,277|+6.06%|11,790|8,034|-7.07%|
|ram / Writes batch 64|973,146|748,036|+0.97%|102,789|91,823|-0.92%|


## Historical iteration 4: held for confirmation

Iteration 3 remains the last accepted build. These tables show iteration 4 against iteration 3 and local LMDB; no RAM parity or universal no-regression claim follows from them.

## Iteration 4: all workloads, HELD pending disk-tail confirmation

All 39 process runs completed with passing oracles. Full suite: 4 pairs/filesystem; million suite: 3 ULMDB pairs plus contemporary LMDB/SQLite controls.

Throughput is operations/s; higher is better. Rates are geometric means of process rates. Change compares candidate with the frozen ULMDB baseline.

|Filesystem / operation|ULMDB baseline/s|ULMDB candidate/s|Change|Local LMDB/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|
|disk / ordered_kv_mixed|6,253,183|6,376,907|+1.98%|4,060,226|+57.06%|
|disk / ordered_kv_hits|6,671,970|6,696,538|+0.37%|4,145,333|+61.54%|
|disk / ordered_kv_misses|6,492,685|6,687,139|+2.99%|4,022,699|+66.24%|
|disk / ipv4_lpm_mixed|5,478,216|5,472,663|-0.10%|2,611,929|+109.53%|
|disk / ipv4_lpm_hits|3,177,915|3,172,898|-0.16%|2,100,843|+51.03%|
|disk / ipv4_lpm_misses|21,478,688|21,501,533|+0.11%|3,779,542|+468.89%|
|disk / ipv4_lpm_outside_cover_misses|99,724,538|100,954,972|+1.23%|95,714,471|+5.48%|
|disk / ipv4_lpm_inside_gap_misses|11,556,149|11,537,602|-0.16%|1,930,120|+497.77%|
|disk / ipv6_lpm_mixed|4,391,453|4,353,350|-0.87%|1,543,460|+182.05%|
|disk / ipv6_lpm_hits|2,521,036|2,512,243|-0.35%|1,231,974|+103.92%|
|disk / ipv6_lpm_misses|17,237,940|17,265,179|+0.16%|2,210,712|+680.98%|
|disk / ipv6_lpm_outside_cover_misses|96,383,964|97,234,188|+0.88%|94,210,393|+3.21%|
|disk / ipv6_lpm_inside_gap_misses|9,348,669|9,351,642|+0.03%|1,117,784|+736.62%|
|disk / durable_batch1|15,454|15,923|+3.04%|10,613|+50.03%|
|disk / durable_batch64|488,647|492,584|+0.81%|178,909|+175.33%|
|ram / ordered_kv_mixed|6,272,757|6,385,717|+1.80%|4,049,911|+57.68%|
|ram / ordered_kv_hits|6,612,602|6,674,467|+0.94%|4,139,931|+61.22%|
|ram / ordered_kv_misses|6,532,890|6,648,406|+1.77%|4,003,807|+66.05%|
|ram / ipv4_lpm_mixed|5,493,085|5,528,427|+0.64%|2,569,476|+115.16%|
|ram / ipv4_lpm_hits|3,182,007|3,201,224|+0.60%|2,087,255|+53.37%|
|ram / ipv4_lpm_misses|21,574,606|21,601,258|+0.12%|3,692,820|+484.95%|
|ram / ipv4_lpm_outside_cover_misses|100,064,632|100,822,774|+0.76%|92,970,193|+8.45%|
|ram / ipv4_lpm_inside_gap_misses|11,518,033|11,616,515|+0.86%|1,877,321|+518.78%|
|ram / ipv6_lpm_mixed|4,398,821|4,371,808|-0.61%|1,539,121|+184.05%|
|ram / ipv6_lpm_hits|2,525,598|2,515,511|-0.40%|1,227,741|+104.89%|
|ram / ipv6_lpm_misses|17,242,050|17,239,446|-0.02%|2,210,908|+679.75%|
|ram / ipv6_lpm_outside_cover_misses|95,956,139|93,855,397|-2.19%|90,696,579|+3.48%|
|ram / ipv6_lpm_inside_gap_misses|9,326,029|9,362,750|+0.39%|1,118,175|+737.32%|
|ram / durable_batch1|59,508|63,040|+5.94%|134,748|-53.22%|
|ram / durable_batch64|966,664|975,027|+0.87%|745,917|+30.72%|

### Million-key workload

|Operation|ULMDB baseline/s|ULMDB candidate/s|Change|LMDB/s|SQLite/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|---:|
|load|1,194,233|1,192,221|-0.17%|1,565,723|826,611|-23.85%|
|read|2,548,647|2,578,466|+1.17%|1,674,954|451,679|+53.94%|
|update|145,659|143,959|-1.17%|128,557|57,979|+11.98%|

### Sampled latency

Nanoseconds; lower is better. These are geometric means of trial-sampled p99s. The original write trial has only eight samples; this is not a population tail guarantee.

|Filesystem / operation|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / ordered_kv_mixed|233|232|-0.68%|344|
|disk / ordered_kv_hits|224|222|-0.94%|344|
|disk / ordered_kv_misses|229|223|-2.47%|344|
|disk / ipv4_lpm_mixed|421|420|-0.26%|753|
|disk / ipv4_lpm_hits|429|429|+0.08%|766|
|disk / ipv4_lpm_misses|111|115|+3.35%|599|
|disk / ipv4_lpm_outside_cover_misses|41|41|-0.34%|48|
|disk / ipv4_lpm_inside_gap_misses|113|115|+2.19%|618|
|disk / ipv6_lpm_mixed|510|511|+0.20%|1,235|
|disk / ipv6_lpm_hits|523|525|+0.24%|1,239|
|disk / ipv6_lpm_misses|141|141|-0.10%|1,067|
|disk / ipv6_lpm_outside_cover_misses|41|41|+0.65%|45|
|disk / ipv6_lpm_inside_gap_misses|143|141|-1.55%|1,092|
|disk / durable_batch1|80,601|83,942|+4.15%|192,155|
|disk / durable_batch64|209,879|233,949|+11.47%|474,704|
|ram / ordered_kv_mixed|232|232|-0.32%|344|
|ram / ordered_kv_hits|222|223|+0.14%|345|
|ram / ordered_kv_misses|226|222|-1.66%|345|
|ram / ipv4_lpm_mixed|421|416|-1.21%|766|
|ram / ipv4_lpm_hits|430|424|-1.20%|780|
|ram / ipv4_lpm_misses|114|115|+0.30%|627|
|ram / ipv4_lpm_outside_cover_misses|41|41|-0.31%|45|
|ram / ipv4_lpm_inside_gap_misses|116|114|-1.11%|642|
|ram / ipv6_lpm_mixed|512|510|-0.42%|1,247|
|ram / ipv6_lpm_hits|523|523|+0.03%|1,256|
|ram / ipv6_lpm_misses|142|141|-0.89%|1,075|
|ram / ipv6_lpm_outside_cover_misses|42|41|-0.68%|43|
|ram / ipv6_lpm_inside_gap_misses|143|142|-0.45%|1,101|
|ram / durable_batch1|12,641|11,576|-8.42%|7,931|
|ram / durable_batch64|102,945|102,101|-0.82%|92,642|

### Pooled write samples

Geometric mean of each process pooled sampled p99; nanoseconds, lower is better. These preserve adverse tails separately from the trial-sampled estimates above.

|Filesystem / batch|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / durable_batch1|234,195|258,869|+10.54%|327,158|
|disk / durable_batch64|438,112|414,579|-5.37%|519,892|
|ram / durable_batch1|14,557|12,752|-12.40%|8,388|
|ram / durable_batch64|253,094|250,040|-1.21%|99,505|

### Lifecycle and file costs

|Metric|Baseline|Candidate|Change|
|---|---:|---:|---:|
|Disk bare open, ms|80.996|82.010|+1.25%|
|RAM bare open, ms|41.997|42.016|+0.05%|
|Million close/reopen, ms|7,074.633|6,856.621|-3.08%|
|Million apparent file bytes|268,435,456|268,435,456|+0.00%|
|Million allocated file bytes|268,435,456|268,435,456|+0.00%|

Million-workload costs against contemporaneous native engines:

|Metric|ULMDB|LMDB|SQLite|
|---|---:|---:|---:|
|Close/reopen, ms|6,856.621|3.922|9.327|
|Apparent files, MiB|256.000|33.047|24.145|
|Allocated files, MiB|256.000|33.043|24.145|
|File count|2|2|1|

Complete 237-metric comparison: [benchmark/work/optimization-20261105/iteration-4-three-arm/accepted-vs-candidate.md](benchmark/work/optimization-20261105/iteration-4-three-arm/accepted-vs-candidate.md). It includes combined write windows, pooled sampled tails, setup, validation, resources, all individual observations and uncertainty. Negative means remain visible; statistical overlap does not establish equivalence.

## Historical retained iteration 3

These complete tables compare accepted iteration 1, retained iteration 3 and contemporaneous LMDB. The source/binary receipts identify the measured build. Initial-baseline comparison: [initial-vs-candidate.md](benchmark/work/optimization-20261105/iteration-3-three-arm/initial-vs-candidate.md).

## Iteration 3: previous accepted versus current, all workloads

All 39 process runs completed with passing oracles. Full suite: 4 pairs/filesystem; million suite: 3 ULMDB pairs plus contemporary LMDB/SQLite controls.

Throughput is operations/s; higher is better. Rates are geometric means of process rates. Change compares candidate with the frozen ULMDB baseline.

|Filesystem / operation|ULMDB baseline/s|ULMDB candidate/s|Change|Local LMDB/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|
|disk / ordered_kv_mixed|6,087,377|6,252,268|+2.71%|4,050,329|+54.36%|
|disk / ordered_kv_hits|6,397,958|6,710,655|+4.89%|4,130,712|+62.46%|
|disk / ordered_kv_misses|6,342,764|6,526,850|+2.90%|4,010,309|+62.75%|
|disk / ipv4_lpm_mixed|5,414,202|5,448,787|+0.64%|2,572,513|+111.81%|
|disk / ipv4_lpm_hits|3,155,002|3,168,609|+0.43%|2,091,522|+51.50%|
|disk / ipv4_lpm_misses|21,483,897|21,471,907|-0.06%|3,740,411|+474.05%|
|disk / ipv4_lpm_outside_cover_misses|100,877,362|100,907,544|+0.03%|90,780,464|+11.16%|
|disk / ipv4_lpm_inside_gap_misses|11,584,778|11,528,905|-0.48%|1,942,157|+493.61%|
|disk / ipv6_lpm_mixed|4,386,652|4,374,380|-0.28%|1,537,127|+184.58%|
|disk / ipv6_lpm_hits|2,515,870|2,517,638|+0.07%|1,222,250|+105.98%|
|disk / ipv6_lpm_misses|17,199,296|17,297,478|+0.57%|2,174,241|+695.56%|
|disk / ipv6_lpm_outside_cover_misses|95,967,239|97,173,972|+1.26%|85,365,271|+13.83%|
|disk / ipv6_lpm_inside_gap_misses|9,342,411|9,326,590|-0.17%|1,096,840|+750.31%|
|disk / durable_batch1|15,498|15,481|-0.11%|10,581|+46.31%|
|disk / durable_batch64|482,000|486,923|+1.02%|178,440|+172.88%|
|ram / ordered_kv_mixed|6,068,010|6,289,098|+3.64%|4,058,471|+54.96%|
|ram / ordered_kv_hits|6,367,392|6,700,157|+5.23%|4,147,782|+61.54%|
|ram / ordered_kv_misses|6,347,950|6,505,066|+2.48%|4,012,080|+62.14%|
|ram / ipv4_lpm_mixed|5,391,877|5,480,386|+1.64%|2,563,181|+113.81%|
|ram / ipv4_lpm_hits|3,122,685|3,177,262|+1.75%|2,062,272|+54.07%|
|ram / ipv4_lpm_misses|21,515,915|21,457,731|-0.27%|3,679,082|+483.24%|
|ram / ipv4_lpm_outside_cover_misses|100,949,079|100,917,264|-0.03%|92,910,076|+8.62%|
|ram / ipv4_lpm_inside_gap_misses|11,452,990|11,542,885|+0.78%|1,890,619|+510.53%|
|ram / ipv6_lpm_mixed|4,378,683|4,408,950|+0.69%|1,548,755|+184.68%|
|ram / ipv6_lpm_hits|2,522,857|2,527,099|+0.17%|1,233,693|+104.84%|
|ram / ipv6_lpm_misses|17,398,032|17,216,652|-1.04%|2,185,469|+687.78%|
|ram / ipv6_lpm_outside_cover_misses|96,627,923|95,552,643|-1.11%|90,992,613|+5.01%|
|ram / ipv6_lpm_inside_gap_misses|9,266,233|9,365,162|+1.07%|1,113,029|+741.41%|
|ram / durable_batch1|57,091|59,280|+3.84%|133,396|-55.56%|
|ram / durable_batch64|914,614|962,866|+5.28%|742,257|+29.72%|

### Million-key workload

|Operation|ULMDB baseline/s|ULMDB candidate/s|Change|LMDB/s|SQLite/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|---:|
|load|816,603|1,169,017|+43.16%|1,580,608|827,605|-26.04%|
|read|2,521,575|2,608,859|+3.46%|1,667,588|458,522|+56.45%|
|update|103,176|145,078|+40.61%|128,015|58,018|+13.33%|

### Sampled latency

Nanoseconds; lower is better. These are geometric means of trial-sampled p99s. The original write trial has only eight samples; this is not a population tail guarantee.

|Filesystem / operation|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / ordered_kv_mixed|238|231|-2.84%|342|
|disk / ordered_kv_hits|232|223|-3.85%|341|
|disk / ordered_kv_misses|231|225|-2.87%|342|
|disk / ipv4_lpm_mixed|420|424|+0.94%|776|
|disk / ipv4_lpm_hits|428|433|+1.14%|787|
|disk / ipv4_lpm_misses|118|115|-2.90%|633|
|disk / ipv4_lpm_outside_cover_misses|41|41|-0.03%|45|
|disk / ipv4_lpm_inside_gap_misses|117|117|+0.05%|640|
|disk / ipv6_lpm_mixed|513|517|+0.70%|1,245|
|disk / ipv6_lpm_hits|526|530|+0.87%|1,257|
|disk / ipv6_lpm_misses|141|142|+0.59%|1,112|
|disk / ipv6_lpm_outside_cover_misses|41|41|-0.34%|44|
|disk / ipv6_lpm_inside_gap_misses|141|143|+1.38%|1,126|
|disk / durable_batch1|81,108|86,702|+6.90%|199,464|
|disk / durable_batch64|242,872|191,194|-21.28%|451,520|
|ram / ordered_kv_mixed|237|231|-2.49%|342|
|ram / ordered_kv_hits|231|222|-3.81%|340|
|ram / ordered_kv_misses|231|224|-2.82%|341|
|ram / ipv4_lpm_mixed|419|422|+0.80%|772|
|ram / ipv4_lpm_hits|425|431|+1.44%|789|
|ram / ipv4_lpm_misses|120|115|-3.85%|604|
|ram / ipv4_lpm_outside_cover_misses|41|41|-0.03%|45|
|ram / ipv4_lpm_inside_gap_misses|118|114|-3.99%|613|
|ram / ipv6_lpm_mixed|512|508|-0.82%|1,236|
|ram / ipv6_lpm_hits|527|524|-0.67%|1,241|
|ram / ipv6_lpm_misses|140|140|+0.04%|1,084|
|ram / ipv6_lpm_outside_cover_misses|41|41|+0.03%|42|
|ram / ipv6_lpm_inside_gap_misses|142|142|-0.08%|1,093|
|ram / durable_batch1|12,587|12,536|-0.41%|8,292|
|ram / durable_batch64|116,358|106,244|-8.69%|94,466|

### Pooled write samples

Geometric mean of each process pooled sampled p99; nanoseconds, lower is better. These preserve adverse tails separately from the trial-sampled estimates above.

|Filesystem / batch|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / durable_batch1|257,876|312,363|+21.13%|328,182|
|disk / durable_batch64|447,657|373,503|-16.56%|528,305|
|ram / durable_batch1|14,315|15,377|+7.42%|11,158|
|ram / durable_batch64|321,757|249,739|-22.38%|104,982|

### Lifecycle and file costs

|Metric|Baseline|Candidate|Change|
|---|---:|---:|---:|
|Disk bare open, ms|84.024|83.646|-0.45%|
|RAM bare open, ms|43.439|42.564|-2.01%|
|Million close/reopen, ms|9,691.977|7,050.552|-27.25%|
|Million apparent file bytes|268,435,456|268,435,456|+0.00%|
|Million allocated file bytes|268,435,456|268,435,456|+0.00%|

Million-workload costs against contemporaneous native engines:

|Metric|ULMDB|LMDB|SQLite|
|---|---:|---:|---:|
|Close/reopen, ms|7,050.552|3.775|9.321|
|Apparent files, MiB|256.000|33.047|24.145|
|Allocated files, MiB|256.000|33.043|24.145|
|File count|2|2|1|

Complete 237-metric comparison: [benchmark/work/optimization-20261105/iteration-3-three-arm/accepted-vs-candidate.md](benchmark/work/optimization-20261105/iteration-3-three-arm/accepted-vs-candidate.md). It includes combined write windows, pooled sampled tails, setup, validation, resources, all individual observations and uncertainty. Negative means remain visible; statistical overlap does not establish equivalence.

## Superseded candidate

Iteration 2 was held because small KV reads fell 4-6%. It was not published as the accepted engine; iteration 3 resolves those throughput reductions.

## Iteration 2 historical tables, adverse results retained

All 39 process runs completed with passing oracles. Full suite: 4 pairs/filesystem; million suite: 3 ULMDB pairs plus contemporary LMDB/SQLite controls.

Throughput is operations/s; higher is better. Rates are geometric means of process rates. Change compares candidate with the frozen ULMDB baseline.

|Filesystem / operation|ULMDB baseline/s|ULMDB candidate/s|Change|Local LMDB/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|
|disk / ordered_kv_mixed|6,061,505|5,796,938|-4.36%|3,987,658|+45.37%|
|disk / ordered_kv_hits|6,365,765|6,038,231|-5.15%|4,061,086|+48.69%|
|disk / ordered_kv_misses|6,339,083|6,008,984|-5.21%|3,929,994|+52.90%|
|disk / ipv4_lpm_mixed|5,407,609|5,478,820|+1.32%|2,660,143|+105.96%|
|disk / ipv4_lpm_hits|3,147,789|3,191,384|+1.38%|2,133,009|+49.62%|
|disk / ipv4_lpm_misses|21,461,758|21,688,050|+1.05%|4,026,780|+438.60%|
|disk / ipv4_lpm_outside_cover_misses|99,966,692|99,206,323|-0.76%|90,146,815|+10.05%|
|disk / ipv4_lpm_inside_gap_misses|11,564,308|11,557,515|-0.06%|2,051,759|+463.30%|
|disk / ipv6_lpm_mixed|4,396,044|4,357,188|-0.88%|1,556,360|+179.96%|
|disk / ipv6_lpm_hits|2,526,214|2,505,796|-0.81%|1,238,983|+102.25%|
|disk / ipv6_lpm_misses|17,386,129|17,199,917|-1.07%|2,241,883|+667.21%|
|disk / ipv6_lpm_outside_cover_misses|96,458,801|96,616,753|+0.16%|88,018,643|+9.77%|
|disk / ipv6_lpm_inside_gap_misses|9,413,841|9,157,659|-2.72%|1,135,258|+706.66%|
|disk / durable_batch1|15,458|15,463|+0.03%|10,700|+44.52%|
|disk / durable_batch64|472,901|488,175|+3.23%|176,866|+176.01%|
|ram / ordered_kv_mixed|6,077,385|5,771,402|-5.03%|3,994,760|+44.47%|
|ram / ordered_kv_hits|6,325,563|6,035,392|-4.59%|4,097,473|+47.30%|
|ram / ordered_kv_misses|6,371,185|5,996,827|-5.88%|3,917,321|+53.08%|
|ram / ipv4_lpm_mixed|5,421,857|5,414,596|-0.13%|2,708,011|+99.95%|
|ram / ipv4_lpm_hits|3,181,774|3,144,221|-1.18%|2,183,594|+43.99%|
|ram / ipv4_lpm_misses|21,534,228|21,295,800|-1.11%|4,036,162|+427.63%|
|ram / ipv4_lpm_outside_cover_misses|99,227,470|100,873,298|+1.66%|99,214,265|+1.67%|
|ram / ipv4_lpm_inside_gap_misses|11,597,825|11,395,237|-1.75%|2,077,821|+448.42%|
|ram / ipv6_lpm_mixed|4,388,819|4,309,106|-1.82%|1,579,433|+172.83%|
|ram / ipv6_lpm_hits|2,525,246|2,482,840|-1.68%|1,255,088|+97.82%|
|ram / ipv6_lpm_misses|17,428,337|16,902,509|-3.02%|2,265,657|+646.03%|
|ram / ipv6_lpm_outside_cover_misses|97,386,165|97,501,912|+0.12%|98,270,346|-0.78%|
|ram / ipv6_lpm_inside_gap_misses|9,297,089|9,063,625|-2.51%|1,148,752|+689.00%|
|ram / durable_batch1|57,528|59,326|+3.13%|134,058|-55.75%|
|ram / durable_batch64|927,551|964,833|+4.02%|744,308|+29.63%|

### Million-key workload

|Operation|ULMDB baseline/s|ULMDB candidate/s|Change|LMDB/s|SQLite/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|---:|
|load|819,640|1,146,557|+39.89%|1,597,629|824,091|-28.23%|
|read|2,589,587|2,540,140|-1.91%|1,677,046|455,778|+51.47%|
|update|103,531|145,817|+40.84%|128,782|58,174|+13.23%|

### Sampled latency

Nanoseconds; lower is better. These are geometric means of trial-sampled p99s. The original write trial has only eight samples; this is not a population tail guarantee.

|Filesystem / operation|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / ordered_kv_mixed|239|242|+1.01%|335|
|disk / ordered_kv_hits|232|236|+1.66%|334|
|disk / ordered_kv_misses|232|232|+0.27%|340|
|disk / ipv4_lpm_mixed|417|421|+0.95%|766|
|disk / ipv4_lpm_hits|427|431|+0.95%|780|
|disk / ipv4_lpm_misses|116|120|+3.40%|590|
|disk / ipv4_lpm_outside_cover_misses|41|41|-0.62%|45|
|disk / ipv4_lpm_inside_gap_misses|117|121|+3.44%|603|
|disk / ipv6_lpm_mixed|510|513|+0.50%|1,230|
|disk / ipv6_lpm_hits|522|525|+0.39%|1,240|
|disk / ipv6_lpm_misses|141|141|+0.08%|1,062|
|disk / ipv6_lpm_outside_cover_misses|41|41|+0.00%|43|
|disk / ipv6_lpm_inside_gap_misses|141|141|-0.03%|1,089|
|disk / durable_batch1|90,088|88,364|-1.91%|191,798|
|disk / durable_batch64|243,477|225,235|-7.49%|448,207|
|ram / ordered_kv_mixed|238|242|+1.65%|335|
|ram / ordered_kv_hits|232|238|+2.24%|335|
|ram / ordered_kv_misses|233|233|+0.17%|342|
|ram / ipv4_lpm_mixed|416|429|+3.25%|762|
|ram / ipv4_lpm_hits|424|438|+3.41%|776|
|ram / ipv4_lpm_misses|117|121|+3.93%|581|
|ram / ipv4_lpm_outside_cover_misses|41|41|+0.34%|50|
|ram / ipv4_lpm_inside_gap_misses|116|122|+4.72%|586|
|ram / ipv6_lpm_mixed|511|521|+1.90%|1,218|
|ram / ipv6_lpm_hits|523|536|+2.47%|1,221|
|ram / ipv6_lpm_misses|140|144|+2.48%|1,054|
|ram / ipv6_lpm_outside_cover_misses|41|41|+0.34%|45|
|ram / ipv6_lpm_inside_gap_misses|141|144|+2.15%|1,071|
|ram / durable_batch1|12,655|12,824|+1.33%|8,095|
|ram / durable_batch64|114,314|104,536|-8.55%|93,210|

### Pooled write samples

Geometric mean of each process pooled sampled p99; nanoseconds, lower is better. These preserve adverse tails separately from the trial-sampled estimates above.

|Filesystem / batch|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / durable_batch1|316,216|311,805|-1.39%|327,308|
|disk / durable_batch64|491,177|399,402|-18.68%|524,438|
|ram / durable_batch1|15,724|16,276|+3.51%|9,221|
|ram / durable_batch64|317,189|248,855|-21.54%|96,866|

### Lifecycle and file costs

|Metric|Baseline|Candidate|Change|
|---|---:|---:|---:|
|Disk bare open, ms|82.004|82.614|+0.74%|
|RAM bare open, ms|43.246|42.359|-2.05%|
|Million close/reopen, ms|9,662.532|6,831.525|-29.30%|
|Million apparent file bytes|268,435,456|268,435,456|+0.00%|
|Million allocated file bytes|268,435,456|268,435,456|+0.00%|

Million-workload costs against contemporaneous native engines:

|Metric|ULMDB|LMDB|SQLite|
|---|---:|---:|---:|
|Close/reopen, ms|6,831.525|3.785|9.141|
|Apparent files, MiB|256.000|33.047|24.145|
|Allocated files, MiB|256.000|33.043|24.145|
|File count|2|2|1|

Complete 237-metric comparison: [benchmark/work/optimization-20261105/iteration-2-three-arm/accepted-vs-candidate.md](benchmark/work/optimization-20261105/iteration-2-three-arm/accepted-vs-candidate.md). It includes combined write windows, pooled sampled tails, setup, validation, resources, all individual observations and uncertainty. Negative means remain visible; statistical overlap does not establish equivalence.

## Primary result: all three additional complete campaigns

All 84 process runs completed with passing oracles. Full suite: 12 pairs/filesystem; million suite: 9 ULMDB pairs plus contemporary LMDB/SQLite controls.

Throughput is operations/s; higher is better. Rates are geometric means of process rates. Change compares candidate with the frozen ULMDB baseline.

|Filesystem / operation|ULMDB baseline/s|ULMDB candidate/s|Change|Local LMDB/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|
|disk / ordered_kv_mixed|6,060,163|6,079,072|+0.31%|4,088,791|+48.68%|
|disk / ordered_kv_hits|6,382,268|6,381,378|-0.01%|4,174,553|+52.86%|
|disk / ordered_kv_misses|6,354,990|6,361,850|+0.11%|4,004,521|+58.87%|
|disk / ipv4_lpm_mixed|5,469,772|5,387,818|-1.50%|2,687,312|+100.49%|
|disk / ipv4_lpm_hits|3,170,384|3,131,944|-1.21%|2,159,658|+45.02%|
|disk / ipv4_lpm_misses|21,524,808|21,406,141|-0.55%|3,909,691|+447.51%|
|disk / ipv4_lpm_outside_cover_misses|100,588,453|100,316,340|-0.27%|89,815,450|+11.69%|
|disk / ipv4_lpm_inside_gap_misses|11,451,782|11,523,750|+0.63%|2,025,495|+468.93%|
|disk / ipv6_lpm_mixed|4,340,744|4,373,541|+0.76%|1,563,440|+179.74%|
|disk / ipv6_lpm_hits|2,500,388|2,510,733|+0.41%|1,244,908|+101.68%|
|disk / ipv6_lpm_misses|16,950,058|17,397,399|+2.64%|2,233,644|+678.88%|
|disk / ipv6_lpm_outside_cover_misses|97,025,757|96,705,485|-0.33%|88,881,493|+8.80%|
|disk / ipv6_lpm_inside_gap_misses|9,208,120|9,338,102|+1.41%|1,131,545|+725.25%|
|disk / durable_batch1|15,277|15,394|+0.77%|10,952|+40.56%|
|disk / durable_batch64|476,133|470,771|-1.13%|178,912|+163.13%|
|ram / ordered_kv_mixed|6,058,557|6,089,188|+0.51%|4,084,854|+49.07%|
|ram / ordered_kv_hits|6,383,601|6,359,854|-0.37%|4,197,032|+51.53%|
|ram / ordered_kv_misses|6,345,636|6,378,642|+0.52%|4,023,862|+58.52%|
|ram / ipv4_lpm_mixed|5,481,753|5,399,364|-1.50%|2,652,394|+103.57%|
|ram / ipv4_lpm_hits|3,175,411|3,152,299|-0.73%|2,140,516|+47.27%|
|ram / ipv4_lpm_misses|21,561,511|21,477,671|-0.39%|3,864,299|+455.80%|
|ram / ipv4_lpm_outside_cover_misses|100,455,504|100,570,051|+0.11%|92,907,133|+8.25%|
|ram / ipv4_lpm_inside_gap_misses|11,420,402|11,533,783|+0.99%|1,970,951|+485.19%|
|ram / ipv6_lpm_mixed|4,342,357|4,381,042|+0.89%|1,565,978|+179.76%|
|ram / ipv6_lpm_hits|2,498,895|2,522,014|+0.93%|1,246,961|+102.25%|
|ram / ipv6_lpm_misses|16,994,395|17,376,439|+2.25%|2,240,677|+675.50%|
|ram / ipv6_lpm_outside_cover_misses|95,307,111|97,168,765|+1.95%|92,403,278|+5.16%|
|ram / ipv6_lpm_inside_gap_misses|9,220,029|9,364,555|+1.57%|1,133,555|+726.12%|
|ram / durable_batch1|56,950|57,259|+0.54%|133,982|-57.26%|
|ram / durable_batch64|924,667|922,807|-0.20%|745,575|+23.77%|

### Million-key workload

|Operation|ULMDB baseline/s|ULMDB candidate/s|Change|LMDB/s|SQLite/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|---:|
|load|546,253|812,643|+48.77%|1,634,683|830,649|-50.29%|
|read|2,511,960|2,570,021|+2.31%|1,695,515|461,936|+51.58%|
|update|103,376|103,508|+0.13%|130,183|58,633|-20.49%|

### Sampled latency

Nanoseconds; lower is better. These are geometric means of trial-sampled p99s. The original write trial has only eight samples; this is not a population tail guarantee.

|Filesystem / operation|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / ordered_kv_mixed|239|238|-0.40%|337|
|disk / ordered_kv_hits|232|232|-0.01%|337|
|disk / ordered_kv_misses|233|233|+0.14%|342|
|disk / ipv4_lpm_mixed|422|420|-0.49%|768|
|disk / ipv4_lpm_hits|433|430|-0.54%|784|
|disk / ipv4_lpm_misses|115|117|+1.08%|595|
|disk / ipv4_lpm_outside_cover_misses|41|41|+0.10%|45|
|disk / ipv4_lpm_inside_gap_misses|116|119|+2.06%|603|
|disk / ipv6_lpm_mixed|515|515|-0.04%|1,239|
|disk / ipv6_lpm_hits|528|530|+0.36%|1,251|
|disk / ipv6_lpm_misses|143|141|-1.10%|1,069|
|disk / ipv6_lpm_outside_cover_misses|41|41|+0.10%|43|
|disk / ipv6_lpm_inside_gap_misses|142|142|+0.12%|1,101|
|disk / durable_batch1|81,934|81,285|-0.79%|184,963|
|disk / durable_batch64|240,169|227,606|-5.23%|470,840|
|ram / ordered_kv_mixed|240|238|-0.72%|338|
|ram / ordered_kv_hits|234|232|-0.59%|335|
|ram / ordered_kv_misses|233|232|-0.49%|341|
|ram / ipv4_lpm_mixed|423|417|-1.45%|763|
|ram / ipv4_lpm_hits|432|426|-1.44%|778|
|ram / ipv4_lpm_misses|115|117|+1.42%|618|
|ram / ipv4_lpm_outside_cover_misses|41|41|-0.21%|46|
|ram / ipv4_lpm_inside_gap_misses|116|117|+0.83%|629|
|ram / ipv6_lpm_mixed|517|512|-0.95%|1,236|
|ram / ipv6_lpm_hits|532|525|-1.23%|1,244|
|ram / ipv6_lpm_misses|143|141|-1.20%|1,070|
|ram / ipv6_lpm_outside_cover_misses|41|41|-0.30%|44|
|ram / ipv6_lpm_inside_gap_misses|142|142|-0.43%|1,105|
|ram / durable_batch1|12,986|13,042|+0.43%|8,267|
|ram / durable_batch64|115,118|114,505|-0.53%|92,907|

### Pooled write samples

Geometric mean of each process pooled sampled p99; nanoseconds, lower is better. These preserve adverse tails separately from the trial-sampled estimates above.

|Filesystem / batch|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / durable_batch1|284,487|258,533|-9.12%|327,876|
|disk / durable_batch64|483,559|469,523|-2.90%|525,202|
|ram / durable_batch1|16,528|16,403|-0.76%|10,092|
|ram / durable_batch64|326,020|321,082|-1.51%|101,219|

### Lifecycle and file costs

|Metric|Baseline|Candidate|Change|
|---|---:|---:|---:|
|Disk bare open, ms|84.308|83.711|-0.71%|
|RAM bare open, ms|44.551|43.912|-1.44%|
|Million close/reopen, ms|9,639.248|9,657.544|+0.19%|
|Million apparent file bytes|268,435,456|268,435,456|+0.00%|
|Million allocated file bytes|268,435,911|268,435,456|-0.00%|

Million-workload costs against contemporaneous native engines:

|Metric|ULMDB|LMDB|SQLite|
|---|---:|---:|---:|
|Close/reopen, ms|9,657.544|3.686|9.271|
|Apparent files, MiB|256.000|33.047|24.145|
|Allocated files, MiB|256.000|33.043|24.145|
|File count|2|2|1|

Complete 237-metric comparison: [benchmark/work/optimization-20261105/iteration-1-combined-repeats/comparison.md](benchmark/work/optimization-20261105/iteration-1-combined-repeats/comparison.md). It includes combined write windows, pooled sampled tails, setup, validation, resources, all individual observations and uncertainty. Negative means remain visible; statistical overlap does not establish equivalence.

## Sensitivity result: initial plus all three additional campaigns

All 112 process runs completed with passing oracles. Full suite: 16 pairs/filesystem; million suite: 12 ULMDB pairs plus contemporary LMDB/SQLite controls.

Throughput is operations/s; higher is better. Rates are geometric means of process rates. Change compares candidate with the frozen ULMDB baseline.

|Filesystem / operation|ULMDB baseline/s|ULMDB candidate/s|Change|Local LMDB/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|
|disk / ordered_kv_mixed|6,060,065|6,075,895|+0.26%|4,087,674|+48.64%|
|disk / ordered_kv_hits|6,379,234|6,383,729|+0.07%|4,173,998|+52.94%|
|disk / ordered_kv_misses|6,355,450|6,358,389|+0.05%|4,004,143|+58.80%|
|disk / ipv4_lpm_mixed|5,476,818|5,388,097|-1.62%|2,672,748|+101.59%|
|disk / ipv4_lpm_hits|3,171,692|3,137,109|-1.09%|2,152,184|+45.76%|
|disk / ipv4_lpm_misses|21,542,610|21,388,362|-0.72%|3,887,531|+450.18%|
|disk / ipv4_lpm_outside_cover_misses|100,682,181|100,471,182|-0.21%|91,240,170|+10.12%|
|disk / ipv4_lpm_inside_gap_misses|11,437,755|11,525,051|+0.76%|2,006,875|+474.28%|
|disk / ipv6_lpm_mixed|4,343,989|4,371,159|+0.63%|1,564,354|+179.42%|
|disk / ipv6_lpm_hits|2,502,208|2,510,378|+0.33%|1,246,257|+101.43%|
|disk / ipv6_lpm_misses|16,944,220|17,406,361|+2.73%|2,238,968|+677.43%|
|disk / ipv6_lpm_outside_cover_misses|96,842,102|96,631,470|-0.22%|90,382,154|+6.91%|
|disk / ipv6_lpm_inside_gap_misses|9,206,588|9,345,288|+1.51%|1,132,657|+725.08%|
|disk / durable_batch1|15,260|15,450|+1.25%|10,906|+41.67%|
|disk / durable_batch64|476,519|471,550|-1.04%|178,739|+163.82%|
|ram / ordered_kv_mixed|6,060,075|6,086,824|+0.44%|4,083,164|+49.07%|
|ram / ordered_kv_hits|6,387,378|6,364,291|-0.36%|4,200,190|+51.52%|
|ram / ordered_kv_misses|6,352,259|6,367,839|+0.25%|4,012,849|+58.69%|
|ram / ipv4_lpm_mixed|5,478,406|5,387,878|-1.65%|2,650,277|+103.29%|
|ram / ipv4_lpm_hits|3,176,354|3,151,811|-0.77%|2,139,613|+47.31%|
|ram / ipv4_lpm_misses|21,501,827|21,489,227|-0.06%|3,861,778|+456.46%|
|ram / ipv4_lpm_outside_cover_misses|100,568,408|100,332,234|-0.23%|91,156,093|+10.07%|
|ram / ipv4_lpm_inside_gap_misses|11,402,399|11,565,759|+1.43%|1,976,306|+485.22%|
|ram / ipv6_lpm_mixed|4,342,870|4,378,776|+0.83%|1,566,921|+179.45%|
|ram / ipv6_lpm_hits|2,500,213|2,516,644|+0.66%|1,246,488|+101.90%|
|ram / ipv6_lpm_misses|16,998,352|17,369,435|+2.18%|2,244,541|+673.85%|
|ram / ipv6_lpm_outside_cover_misses|94,940,927|97,231,157|+2.41%|91,020,073|+6.82%|
|ram / ipv6_lpm_inside_gap_misses|9,206,797|9,371,825|+1.79%|1,135,334|+725.47%|
|ram / durable_batch1|57,050|57,036|-0.03%|133,999|-57.44%|
|ram / durable_batch64|925,885|921,915|-0.43%|746,255|+23.54%|

### Million-key workload

|Operation|ULMDB baseline/s|ULMDB candidate/s|Change|LMDB/s|SQLite/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|---:|
|load|546,907|810,913|+48.27%|1,629,254|829,977|-50.23%|
|read|2,510,655|2,568,446|+2.30%|1,692,652|462,085|+51.74%|
|update|103,379|103,490|+0.11%|129,826|58,501|-20.29%|

### Sampled latency

Nanoseconds; lower is better. These are geometric means of trial-sampled p99s. The original write trial has only eight samples; this is not a population tail guarantee.

|Filesystem / operation|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / ordered_kv_mixed|239|239|-0.07%|337|
|disk / ordered_kv_hits|233|232|-0.18%|337|
|disk / ordered_kv_misses|233|233|+0.10%|341|
|disk / ipv4_lpm_mixed|423|420|-0.67%|769|
|disk / ipv4_lpm_hits|433|431|-0.47%|784|
|disk / ipv4_lpm_misses|116|117|+1.06%|605|
|disk / ipv4_lpm_outside_cover_misses|41|41|+0.30%|45|
|disk / ipv4_lpm_inside_gap_misses|116|118|+2.23%|612|
|disk / ipv6_lpm_mixed|516|515|-0.13%|1,243|
|disk / ipv6_lpm_hits|529|530|+0.21%|1,253|
|disk / ipv6_lpm_misses|142|142|-0.46%|1,066|
|disk / ipv6_lpm_outside_cover_misses|41|41|+0.08%|44|
|disk / ipv6_lpm_inside_gap_misses|142|142|-0.24%|1,099|
|disk / durable_batch1|82,277|82,694|+0.51%|186,222|
|disk / durable_batch64|237,160|227,291|-4.16%|468,656|
|ram / ordered_kv_mixed|240|238|-0.74%|337|
|ram / ordered_kv_hits|233|232|-0.55%|335|
|ram / ordered_kv_misses|233|232|-0.44%|341|
|ram / ipv4_lpm_mixed|423|418|-1.30%|764|
|ram / ipv4_lpm_hits|433|428|-1.17%|778|
|ram / ipv4_lpm_misses|116|116|+0.72%|618|
|ram / ipv4_lpm_outside_cover_misses|41|41|+0.33%|46|
|ram / ipv4_lpm_inside_gap_misses|117|117|+0.40%|628|
|ram / ipv6_lpm_mixed|517|513|-0.72%|1,235|
|ram / ipv6_lpm_hits|531|528|-0.65%|1,244|
|ram / ipv6_lpm_misses|143|141|-1.02%|1,069|
|ram / ipv6_lpm_outside_cover_misses|41|41|-0.23%|43|
|ram / ipv6_lpm_inside_gap_misses|142|142|-0.52%|1,103|
|ram / durable_batch1|13,009|13,025|+0.12%|8,227|
|ram / durable_batch64|115,331|114,779|-0.48%|92,633|

### Pooled write samples

Geometric mean of each process pooled sampled p99; nanoseconds, lower is better. These preserve adverse tails separately from the trial-sampled estimates above.

|Filesystem / batch|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / durable_batch1|291,313|271,624|-6.76%|327,224|
|disk / durable_batch64|471,315|459,497|-2.51%|523,748|
|ram / durable_batch1|17,019|16,699|-1.88%|9,822|
|ram / durable_batch64|324,847|319,968|-1.50%|101,270|

### Lifecycle and file costs

|Metric|Baseline|Candidate|Change|
|---|---:|---:|---:|
|Disk bare open, ms|84.426|83.749|-0.80%|
|RAM bare open, ms|44.397|43.731|-1.50%|
|Million close/reopen, ms|9,636.566|9,657.876|+0.22%|
|Million apparent file bytes|268,435,456|268,435,456|+0.00%|
|Million allocated file bytes|268,435,797|268,435,456|-0.00%|

Million-workload costs against contemporaneous native engines:

|Metric|ULMDB|LMDB|SQLite|
|---|---:|---:|---:|
|Close/reopen, ms|9,657.876|3.739|9.266|
|Apparent files, MiB|256.000|33.047|24.145|
|Allocated files, MiB|256.000|33.043|24.145|
|File count|2|2|1|

Complete 237-metric comparison: [benchmark/work/optimization-20261105/iteration-1-combined-all-four/comparison.md](benchmark/work/optimization-20261105/iteration-1-combined-all-four/comparison.md). It includes combined write windows, pooled sampled tails, setup, validation, resources, all individual observations and uncertainty. Negative means remain visible; statistical overlap does not establish equivalence.

## Initial complete campaign

All 28 process runs completed with passing oracles. Full suite: 4 pairs/filesystem; million suite: 3 ULMDB pairs plus contemporary LMDB/SQLite controls.

Throughput is operations/s; higher is better. Rates are geometric means of process rates. Change compares candidate with the frozen ULMDB baseline.

|Filesystem / operation|ULMDB baseline/s|ULMDB candidate/s|Change|Local LMDB/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|
|disk / ordered_kv_mixed|6,059,772|6,066,374|+0.11%|4,084,327|+48.53%|
|disk / ordered_kv_hits|6,370,138|6,390,785|+0.32%|4,172,335|+53.17%|
|disk / ordered_kv_misses|6,356,830|6,348,018|-0.14%|4,003,011|+58.58%|
|disk / ipv4_lpm_mixed|5,498,012|5,388,936|-1.98%|2,629,527|+104.94%|
|disk / ipv4_lpm_hits|3,175,619|3,152,655|-0.72%|2,129,917|+48.02%|
|disk / ipv4_lpm_misses|21,596,104|21,335,111|-1.21%|3,821,801|+458.25%|
|disk / ipv4_lpm_outside_cover_misses|100,963,890|100,937,145|-0.03%|95,651,368|+5.53%|
|disk / ipv4_lpm_inside_gap_misses|11,395,777|11,528,956|+1.17%|1,952,034|+490.61%|
|disk / ipv6_lpm_mixed|4,353,738|4,364,023|+0.24%|1,567,097|+178.48%|
|disk / ipv6_lpm_hits|2,507,677|2,509,315|+0.07%|1,250,314|+100.69%|
|disk / ipv6_lpm_misses|16,926,720|17,433,276|+2.99%|2,255,014|+673.09%|
|disk / ipv6_lpm_outside_cover_misses|96,293,220|96,409,764|+0.12%|95,037,874|+1.44%|
|disk / ipv6_lpm_inside_gap_misses|9,201,994|9,366,880|+1.79%|1,136,000|+724.55%|
|disk / durable_batch1|15,211|15,620|+2.69%|10,769|+45.05%|
|disk / durable_batch64|477,678|473,897|-0.79%|178,220|+165.91%|
|ram / ordered_kv_mixed|6,064,633|6,079,739|+0.25%|4,078,100|+49.08%|
|ram / ordered_kv_hits|6,398,722|6,377,622|-0.33%|4,209,677|+51.50%|
|ram / ordered_kv_misses|6,372,167|6,335,542|-0.57%|3,979,991|+59.18%|
|ram / ipv4_lpm_mixed|5,468,379|5,353,564|-2.10%|2,643,938|+102.48%|
|ram / ipv4_lpm_hits|3,179,184|3,150,345|-0.91%|2,136,905|+47.43%|
|ram / ipv4_lpm_misses|21,323,765|21,523,930|+0.94%|3,854,226|+458.45%|
|ram / ipv4_lpm_outside_cover_misses|100,907,882|99,622,153|-1.27%|86,098,509|+15.71%|
|ram / ipv4_lpm_inside_gap_misses|11,348,559|11,662,223|+2.76%|1,992,458|+485.32%|
|ram / ipv6_lpm_mixed|4,344,412|4,371,985|+0.63%|1,569,753|+178.51%|
|ram / ipv6_lpm_hits|2,504,174|2,500,602|-0.14%|1,245,071|+100.84%|
|ram / ipv6_lpm_misses|17,010,229|17,348,440|+1.99%|2,256,172|+668.93%|
|ram / ipv6_lpm_outside_cover_misses|93,850,795|97,418,575|+3.80%|86,993,456|+11.98%|
|ram / ipv6_lpm_inside_gap_misses|9,167,215|9,393,669|+2.47%|1,140,689|+723.51%|
|ram / durable_batch1|57,352|56,369|-1.71%|134,051|-57.95%|
|ram / durable_batch64|929,549|919,244|-1.11%|748,297|+22.84%|

### Million-key workload

|Operation|ULMDB baseline/s|ULMDB candidate/s|Change|LMDB/s|SQLite/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|---:|
|load|548,876|805,745|+46.80%|1,613,076|827,963|-50.05%|
|read|2,506,747|2,563,729|+2.27%|1,684,091|462,533|+52.23%|
|update|103,388|103,438|+0.05%|128,760|58,104|-19.67%|

### Sampled latency

Nanoseconds; lower is better. These are geometric means of trial-sampled p99s. The original write trial has only eight samples; this is not a population tail guarantee.

|Filesystem / operation|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / ordered_kv_mixed|240|242|+0.95%|337|
|disk / ordered_kv_hits|234|232|-0.68%|335|
|disk / ordered_kv_misses|233|233|-0.05%|337|
|disk / ipv4_lpm_mixed|424|419|-1.21%|771|
|disk / ipv4_lpm_hits|434|433|-0.26%|786|
|disk / ipv4_lpm_misses|118|119|+1.00%|637|
|disk / ipv4_lpm_outside_cover_misses|41|42|+0.90%|48|
|disk / ipv4_lpm_inside_gap_misses|115|118|+2.72%|641|
|disk / ipv6_lpm_mixed|518|516|-0.39%|1,253|
|disk / ipv6_lpm_hits|531|530|-0.24%|1,261|
|disk / ipv6_lpm_misses|141|144|+1.50%|1,058|
|disk / ipv6_lpm_outside_cover_misses|41|41|+0.01%|45|
|disk / ipv6_lpm_inside_gap_misses|143|141|-1.29%|1,096|
|disk / durable_batch1|83,315|87,069|+4.51%|190,051|
|disk / durable_batch64|228,357|226,351|-0.88%|462,164|
|ram / ordered_kv_mixed|239|237|-0.79%|334|
|ram / ordered_kv_hits|231|230|-0.44%|334|
|ram / ordered_kv_misses|232|231|-0.28%|341|
|ram / ipv4_lpm_mixed|422|419|-0.85%|765|
|ram / ipv4_lpm_hits|434|432|-0.37%|780|
|ram / ipv4_lpm_misses|117|115|-1.36%|619|
|ram / ipv4_lpm_outside_cover_misses|41|42|+1.97%|43|
|ram / ipv4_lpm_inside_gap_misses|118|117|-0.86%|624|
|ram / ipv6_lpm_mixed|516|516|-0.02%|1,232|
|ram / ipv6_lpm_hits|531|537|+1.14%|1,246|
|ram / ipv6_lpm_misses|143|142|-0.45%|1,065|
|ram / ipv6_lpm_outside_cover_misses|41|41|-0.03%|42|
|ram / ipv6_lpm_inside_gap_misses|143|142|-0.80%|1,096|
|ram / durable_batch1|13,077|12,974|-0.79%|8,108|
|ram / durable_batch64|115,975|115,605|-0.32%|91,814|

### Pooled write samples

Geometric mean of each process pooled sampled p99; nanoseconds, lower is better. These preserve adverse tails separately from the trial-sampled estimates above.

|Filesystem / batch|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / durable_batch1|312,790|315,012|+0.71%|325,274|
|disk / durable_batch64|436,412|430,686|-1.31%|519,410|
|ram / durable_batch1|18,580|17,620|-5.16%|9,054|
|ram / durable_batch64|321,351|316,650|-1.46%|101,424|

### Lifecycle and file costs

|Metric|Baseline|Candidate|Change|
|---|---:|---:|---:|
|Disk bare open, ms|84.780|83.863|-1.08%|
|RAM bare open, ms|43.940|43.194|-1.70%|
|Million close/reopen, ms|9,628.526|9,658.873|+0.32%|
|Million apparent file bytes|268,435,456|268,435,456|+0.00%|
|Million allocated file bytes|268,435,456|268,435,456|+0.00%|

Million-workload costs against contemporaneous native engines:

|Metric|ULMDB|LMDB|SQLite|
|---|---:|---:|---:|
|Close/reopen, ms|9,658.873|3.902|9.250|
|Apparent files, MiB|256.000|33.047|24.145|
|Allocated files, MiB|256.000|33.043|24.145|
|File count|2|2|1|

Complete 237-metric comparison: [benchmark/work/optimization-20261105/iteration-1/comparison.md](benchmark/work/optimization-20261105/iteration-1/comparison.md). It includes combined write windows, pooled sampled tails, setup, validation, resources, all individual observations and uncertainty. Negative means remain visible; statistical overlap does not establish equivalence.

## Additional complete campaign 1

All 28 process runs completed with passing oracles. Full suite: 4 pairs/filesystem; million suite: 3 ULMDB pairs plus contemporary LMDB/SQLite controls.

Throughput is operations/s; higher is better. Rates are geometric means of process rates. Change compares candidate with the frozen ULMDB baseline.

|Filesystem / operation|ULMDB baseline/s|ULMDB candidate/s|Change|Local LMDB/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|
|disk / ordered_kv_mixed|6,076,537|6,083,509|+0.11%|4,092,215|+48.66%|
|disk / ordered_kv_hits|6,365,612|6,392,573|+0.42%|4,177,566|+53.02%|
|disk / ordered_kv_misses|6,365,267|6,372,541|+0.11%|4,009,589|+58.93%|
|disk / ipv4_lpm_mixed|5,474,364|5,388,330|-1.57%|2,709,915|+98.84%|
|disk / ipv4_lpm_hits|3,167,213|3,133,107|-1.08%|2,160,965|+44.99%|
|disk / ipv4_lpm_misses|21,563,486|21,419,383|-0.67%|4,022,422|+432.50%|
|disk / ipv4_lpm_outside_cover_misses|99,957,443|100,812,817|+0.86%|94,530,121|+6.65%|
|disk / ipv4_lpm_inside_gap_misses|11,328,156|11,537,469|+1.85%|2,067,375|+458.07%|
|disk / ipv6_lpm_mixed|4,338,060|4,379,377|+0.95%|1,570,898|+178.78%|
|disk / ipv6_lpm_hits|2,500,770|2,512,151|+0.46%|1,250,838|+100.84%|
|disk / ipv6_lpm_misses|17,002,734|17,361,577|+2.11%|2,249,996|+671.63%|
|disk / ipv6_lpm_outside_cover_misses|97,371,110|97,163,649|-0.21%|95,070,201|+2.20%|
|disk / ipv6_lpm_inside_gap_misses|9,204,742|9,334,691|+1.41%|1,139,804|+718.97%|
|disk / durable_batch1|14,853|15,011|+1.06%|10,706|+40.20%|
|disk / durable_batch64|474,076|467,765|-1.33%|177,515|+163.51%|
|ram / ordered_kv_mixed|6,086,795|6,100,841|+0.23%|4,087,217|+49.27%|
|ram / ordered_kv_hits|6,408,648|6,388,342|-0.32%|4,195,891|+52.25%|
|ram / ordered_kv_misses|6,357,217|6,391,689|+0.54%|4,025,558|+58.78%|
|ram / ipv4_lpm_mixed|5,489,907|5,402,753|-1.59%|2,632,058|+105.27%|
|ram / ipv4_lpm_hits|3,174,316|3,132,184|-1.33%|2,124,640|+47.42%|
|ram / ipv4_lpm_misses|21,498,105|21,463,451|-0.16%|3,853,935|+456.92%|
|ram / ipv4_lpm_outside_cover_misses|99,490,792|100,954,975|+1.47%|92,839,885|+8.74%|
|ram / ipv4_lpm_inside_gap_misses|11,423,730|11,523,902|+0.88%|1,967,443|+485.73%|
|ram / ipv6_lpm_mixed|4,337,248|4,387,258|+1.15%|1,567,306|+179.92%|
|ram / ipv6_lpm_hits|2,496,851|2,521,388|+0.98%|1,248,011|+102.03%|
|ram / ipv6_lpm_misses|16,969,005|17,368,468|+2.35%|2,243,572|+674.14%|
|ram / ipv6_lpm_outside_cover_misses|91,893,386|97,366,936|+5.96%|90,661,148|+7.40%|
|ram / ipv6_lpm_inside_gap_misses|9,177,603|9,379,295|+2.20%|1,135,947|+725.68%|
|ram / durable_batch1|56,290|57,437|+2.04%|134,699|-57.36%|
|ram / durable_batch64|923,218|925,030|+0.20%|741,426|+24.76%|

### Million-key workload

|Operation|ULMDB baseline/s|ULMDB candidate/s|Change|LMDB/s|SQLite/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|---:|
|load|542,323|813,311|+49.97%|1,626,321|828,643|-49.99%|
|read|2,514,800|2,593,901|+3.15%|1,692,704|460,015|+53.24%|
|update|103,318|103,616|+0.29%|129,470|58,453|-19.97%|

### Sampled latency

Nanoseconds; lower is better. These are geometric means of trial-sampled p99s. The original write trial has only eight samples; this is not a population tail guarantee.

|Filesystem / operation|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / ordered_kv_mixed|238|238|+0.11%|335|
|disk / ordered_kv_hits|231|232|+0.22%|336|
|disk / ordered_kv_misses|232|231|-0.27%|340|
|disk / ipv4_lpm_mixed|421|420|-0.17%|763|
|disk / ipv4_lpm_hits|432|429|-0.74%|774|
|disk / ipv4_lpm_misses|117|117|-0.14%|600|
|disk / ipv4_lpm_outside_cover_misses|41|41|+0.00%|48|
|disk / ipv4_lpm_inside_gap_misses|119|118|-0.16%|603|
|disk / ipv6_lpm_mixed|515|515|-0.03%|1,231|
|disk / ipv6_lpm_hits|528|530|+0.41%|1,243|
|disk / ipv6_lpm_misses|142|142|-0.22%|1,066|
|disk / ipv6_lpm_outside_cover_misses|41|41|+0.00%|45|
|disk / ipv6_lpm_inside_gap_misses|142|143|+0.42%|1,092|
|disk / durable_batch1|80,247|79,945|-0.38%|187,653|
|disk / durable_batch64|220,183|242,365|+10.07%|480,141|
|ram / ordered_kv_mixed|239|237|-0.66%|336|
|ram / ordered_kv_hits|234|231|-1.42%|336|
|ram / ordered_kv_misses|231|231|+0.18%|341|
|ram / ipv4_lpm_mixed|422|415|-1.71%|759|
|ram / ipv4_lpm_hits|431|424|-1.72%|771|
|ram / ipv4_lpm_misses|116|115|-0.67%|609|
|ram / ipv4_lpm_outside_cover_misses|41|41|-0.65%|46|
|ram / ipv4_lpm_inside_gap_misses|118|119|+0.49%|617|
|ram / ipv6_lpm_mixed|517|511|-1.02%|1,233|
|ram / ipv6_lpm_hits|532|523|-1.67%|1,241|
|ram / ipv6_lpm_misses|142|142|+0.07%|1,064|
|ram / ipv6_lpm_outside_cover_misses|41|41|+0.31%|43|
|ram / ipv6_lpm_inside_gap_misses|142|143|+1.05%|1,093|
|ram / durable_batch1|12,779|12,953|+1.36%|8,208|
|ram / durable_batch64|115,533|115,095|-0.38%|92,973|

### Pooled write samples

Geometric mean of each process pooled sampled p99; nanoseconds, lower is better. These preserve adverse tails separately from the trial-sampled estimates above.

|Filesystem / batch|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / durable_batch1|310,169|312,501|+0.75%|327,806|
|disk / durable_batch64|449,219|438,143|-2.47%|528,063|
|ram / durable_batch1|13,801|17,281|+25.22%|9,645|
|ram / durable_batch64|325,582|324,382|-0.37%|100,896|

### Lifecycle and file costs

|Metric|Baseline|Candidate|Change|
|---|---:|---:|---:|
|Disk bare open, ms|84.469|84.131|-0.40%|
|RAM bare open, ms|44.345|43.855|-1.10%|
|Million close/reopen, ms|9,626.972|9,650.591|+0.25%|
|Million apparent file bytes|268,435,456|268,435,456|+0.00%|
|Million allocated file bytes|268,435,456|268,435,456|+0.00%|

Million-workload costs against contemporaneous native engines:

|Metric|ULMDB|LMDB|SQLite|
|---|---:|---:|---:|
|Close/reopen, ms|9,650.591|3.710|9.179|
|Apparent files, MiB|256.000|33.047|24.145|
|Allocated files, MiB|256.000|33.043|24.145|
|File count|2|2|1|

Complete 237-metric comparison: [benchmark/work/optimization-20261105/iteration-1-repeat-1/comparison.md](benchmark/work/optimization-20261105/iteration-1-repeat-1/comparison.md). It includes combined write windows, pooled sampled tails, setup, validation, resources, all individual observations and uncertainty. Negative means remain visible; statistical overlap does not establish equivalence.

## Additional complete campaign 2

All 28 process runs completed with passing oracles. Full suite: 4 pairs/filesystem; million suite: 3 ULMDB pairs plus contemporary LMDB/SQLite controls.

Throughput is operations/s; higher is better. Rates are geometric means of process rates. Change compares candidate with the frozen ULMDB baseline.

|Filesystem / operation|ULMDB baseline/s|ULMDB candidate/s|Change|Local LMDB/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|
|disk / ordered_kv_mixed|6,028,001|6,099,158|+1.18%|4,090,508|+49.11%|
|disk / ordered_kv_hits|6,379,792|6,386,322|+0.10%|4,181,969|+52.71%|
|disk / ordered_kv_misses|6,316,841|6,344,015|+0.43%|4,019,628|+57.83%|
|disk / ipv4_lpm_mixed|5,481,861|5,366,968|-2.10%|2,682,510|+100.07%|
|disk / ipv4_lpm_hits|3,170,973|3,125,488|-1.43%|2,150,398|+45.34%|
|disk / ipv4_lpm_misses|21,503,887|21,398,662|-0.49%|3,929,439|+444.57%|
|disk / ipv4_lpm_outside_cover_misses|100,871,700|99,618,649|-1.24%|90,598,738|+9.96%|
|disk / ipv4_lpm_inside_gap_misses|11,479,122|11,564,860|+0.75%|2,036,878|+467.77%|
|disk / ipv6_lpm_mixed|4,337,932|4,356,443|+0.43%|1,570,247|+177.44%|
|disk / ipv6_lpm_hits|2,498,539|2,505,227|+0.27%|1,249,514|+100.50%|
|disk / ipv6_lpm_misses|16,809,373|17,419,337|+3.63%|2,248,717|+674.63%|
|disk / ipv6_lpm_outside_cover_misses|96,609,144|96,237,260|-0.38%|88,154,089|+9.17%|
|disk / ipv6_lpm_inside_gap_misses|9,233,979|9,304,500|+0.76%|1,139,995|+716.19%|
|disk / durable_batch1|15,431|15,727|+1.92%|11,057|+42.23%|
|disk / durable_batch64|474,573|471,364|-0.68%|179,628|+162.41%|
|ram / ordered_kv_mixed|6,055,343|6,081,706|+0.44%|4,073,542|+49.30%|
|ram / ordered_kv_hits|6,392,073|6,320,097|-1.13%|4,193,516|+50.71%|
|ram / ordered_kv_misses|6,334,248|6,356,676|+0.35%|4,016,522|+58.26%|
|ram / ipv4_lpm_mixed|5,475,511|5,374,111|-1.85%|2,665,953|+101.58%|
|ram / ipv4_lpm_hits|3,178,220|3,174,180|-0.13%|2,151,100|+47.56%|
|ram / ipv4_lpm_misses|21,585,423|21,385,973|-0.92%|3,900,898|+448.23%|
|ram / ipv4_lpm_outside_cover_misses|100,928,073|99,903,754|-1.01%|89,367,255|+11.79%|
|ram / ipv4_lpm_inside_gap_misses|11,350,726|11,570,751|+1.94%|1,989,329|+481.64%|
|ram / ipv6_lpm_mixed|4,355,241|4,371,739|+0.38%|1,564,319|+179.47%|
|ram / ipv6_lpm_hits|2,501,336|2,519,998|+0.75%|1,248,372|+101.86%|
|ram / ipv6_lpm_misses|16,984,294|17,458,569|+2.79%|2,234,902|+681.18%|
|ram / ipv6_lpm_outside_cover_misses|96,989,302|97,111,733|+0.13%|91,391,776|+6.26%|
|ram / ipv6_lpm_inside_gap_misses|9,239,481|9,355,041|+1.25%|1,131,431|+726.83%|
|ram / durable_batch1|57,510|57,257|-0.44%|133,914|-57.24%|
|ram / durable_batch64|923,306|919,642|-0.40%|748,386|+22.88%|

### Million-key workload

|Operation|ULMDB baseline/s|ULMDB candidate/s|Change|LMDB/s|SQLite/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|---:|
|load|548,616|813,180|+48.22%|1,626,847|834,369|-50.01%|
|read|2,497,033|2,547,172|+2.01%|1,697,124|463,237|+50.09%|
|update|103,309|103,477|+0.16%|131,843|59,011|-21.52%|

### Sampled latency

Nanoseconds; lower is better. These are geometric means of trial-sampled p99s. The original write trial has only eight samples; this is not a population tail guarantee.

|Filesystem / operation|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / ordered_kv_mixed|240|238|-0.92%|337|
|disk / ordered_kv_hits|233|234|+0.52%|338|
|disk / ordered_kv_misses|233|235|+0.92%|342|
|disk / ipv4_lpm_mixed|422|422|+0.13%|772|
|disk / ipv4_lpm_hits|433|432|-0.23%|789|
|disk / ipv4_lpm_misses|114|116|+1.75%|600|
|disk / ipv4_lpm_outside_cover_misses|41|42|+0.62%|45|
|disk / ipv4_lpm_inside_gap_misses|114|118|+3.78%|611|
|disk / ipv6_lpm_mixed|515|518|+0.70%|1,233|
|disk / ipv6_lpm_hits|529|531|+0.35%|1,242|
|disk / ipv6_lpm_misses|143|141|-1.43%|1,068|
|disk / ipv6_lpm_outside_cover_misses|41|41|+0.00%|43|
|disk / ipv6_lpm_inside_gap_misses|141|141|-0.02%|1,099|
|disk / durable_batch1|82,443|80,533|-2.32%|182,947|
|disk / durable_batch64|240,911|220,383|-8.52%|463,723|
|ram / ordered_kv_mixed|240|239|-0.51%|340|
|ram / ordered_kv_hits|234|232|-0.80%|335|
|ram / ordered_kv_misses|234|232|-0.72%|342|
|ram / ipv4_lpm_mixed|425|419|-1.35%|763|
|ram / ipv4_lpm_hits|434|429|-1.20%|779|
|ram / ipv4_lpm_misses|113|117|+3.46%|608|
|ram / ipv4_lpm_outside_cover_misses|41|42|+0.37%|46|
|ram / ipv4_lpm_inside_gap_misses|115|116|+0.73%|618|
|ram / ipv6_lpm_mixed|517|513|-0.76%|1,234|
|ram / ipv6_lpm_hits|532|527|-0.91%|1,241|
|ram / ipv6_lpm_misses|141|141|-0.58%|1,070|
|ram / ipv6_lpm_outside_cover_misses|42|41|-0.90%|43|
|ram / ipv6_lpm_inside_gap_misses|142|141|-1.13%|1,104|
|ram / durable_batch1|13,134|13,030|-0.79%|8,293|
|ram / durable_batch64|115,001|114,362|-0.56%|93,060|

### Pooled write samples

Geometric mean of each process pooled sampled p99; nanoseconds, lower is better. These preserve adverse tails separately from the trial-sampled estimates above.

|Filesystem / batch|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / durable_batch1|311,295|235,126|-24.47%|325,711|
|disk / durable_batch64|547,464|487,181|-11.01%|522,377|
|ram / durable_batch1|18,809|14,613|-22.31%|10,869|
|ram / durable_batch64|324,066|317,615|-1.99%|99,473|

### Lifecycle and file costs

|Metric|Baseline|Candidate|Change|
|---|---:|---:|---:|
|Disk bare open, ms|84.566|84.527|-0.05%|
|RAM bare open, ms|45.135|44.507|-1.39%|
|Million close/reopen, ms|9,637.240|9,660.791|+0.24%|
|Million apparent file bytes|268,435,456|268,435,456|+0.00%|
|Million allocated file bytes|268,436,821|268,435,456|-0.00%|

Million-workload costs against contemporaneous native engines:

|Metric|ULMDB|LMDB|SQLite|
|---|---:|---:|---:|
|Close/reopen, ms|9,660.791|3.660|9.304|
|Apparent files, MiB|256.000|33.047|24.145|
|Allocated files, MiB|256.000|33.043|24.146|
|File count|2|2|1|

Complete 237-metric comparison: [benchmark/work/optimization-20261105/iteration-1-repeat-2/comparison.md](benchmark/work/optimization-20261105/iteration-1-repeat-2/comparison.md). It includes combined write windows, pooled sampled tails, setup, validation, resources, all individual observations and uncertainty. Negative means remain visible; statistical overlap does not establish equivalence.

## Additional complete campaign 3

All 28 process runs completed with passing oracles. Full suite: 4 pairs/filesystem; million suite: 3 ULMDB pairs plus contemporary LMDB/SQLite controls.

Throughput is operations/s; higher is better. Rates are geometric means of process rates. Change compares candidate with the frozen ULMDB baseline.

|Filesystem / operation|ULMDB baseline/s|ULMDB candidate/s|Change|Local LMDB/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|
|disk / ordered_kv_mixed|6,076,079|6,054,632|-0.35%|4,083,653|+48.27%|
|disk / ordered_kv_hits|6,401,452|6,365,271|-0.57%|4,164,145|+52.86%|
|disk / ordered_kv_misses|6,383,047|6,369,031|-0.22%|3,984,427|+59.85%|
|disk / ipv4_lpm_mixed|5,453,131|5,408,234|-0.82%|2,669,669|+102.58%|
|disk / ipv4_lpm_hits|3,172,969|3,137,248|-1.13%|2,167,645|+44.73%|
|disk / ipv4_lpm_misses|21,507,105|21,400,385|-0.50%|3,781,022|+465.99%|
|disk / ipv4_lpm_outside_cover_misses|100,939,211|100,521,425|-0.41%|84,598,134|+18.82%|
|disk / ipv4_lpm_inside_gap_misses|11,549,186|11,469,131|-0.69%|1,973,374|+481.19%|
|disk / ipv6_lpm_mixed|4,346,246|4,384,853|+0.89%|1,549,273|+183.03%|
|disk / ipv6_lpm_hits|2,501,855|2,514,830|+0.52%|1,234,438|+103.72%|
|disk / ipv6_lpm_misses|17,038,967|17,411,338|+2.19%|2,202,548|+690.51%|
|disk / ipv6_lpm_outside_cover_misses|97,098,555|96,717,767|-0.39%|83,781,312|+15.44%|
|disk / ipv6_lpm_inside_gap_misses|9,185,702|9,375,250|+2.06%|1,115,020|+740.81%|
|disk / durable_batch1|15,555|15,454|-0.65%|11,098|+39.25%|
|disk / durable_batch64|479,771|473,200|-1.37%|179,600|+163.47%|
|ram / ordered_kv_mixed|6,033,650|6,085,033|+0.85%|4,093,829|+48.64%|
|ram / ordered_kv_hits|6,350,224|6,371,321|+0.33%|4,201,694|+51.64%|
|ram / ordered_kv_misses|6,345,464|6,387,618|+0.66%|4,029,517|+58.52%|
|ram / ipv4_lpm_mixed|5,479,849|5,421,335|-1.07%|2,659,292|+103.86%|
|ram / ipv4_lpm_hits|3,173,699|3,150,674|-0.73%|2,145,900|+46.82%|
|ram / ipv4_lpm_misses|21,601,149|21,584,053|-0.08%|3,838,337|+462.33%|
|ram / ipv4_lpm_outside_cover_misses|100,954,653|100,854,777|-0.10%|96,657,190|+4.34%|
|ram / ipv4_lpm_inside_gap_misses|11,487,159|11,506,791|+0.17%|1,956,225|+488.21%|
|ram / ipv6_lpm_mixed|4,334,610|4,384,145|+1.14%|1,566,311|+179.90%|
|ram / ipv6_lpm_hits|2,498,499|2,524,659|+1.05%|1,244,502|+102.87%|
|ram / ipv6_lpm_misses|17,029,945|17,302,634|+1.60%|2,243,569|+671.21%|
|ram / ipv6_lpm_outside_cover_misses|97,133,230|97,027,945|-0.11%|95,221,233|+1.90%|
|ram / ipv6_lpm_inside_gap_misses|9,243,150|9,359,347|+1.26%|1,133,290|+725.86%|
|ram / durable_batch1|57,056|57,085|+0.05%|133,335|-57.19%|
|ram / durable_batch64|927,482|923,759|-0.40%|746,932|+23.67%|

### Million-key workload

|Operation|ULMDB baseline/s|ULMDB candidate/s|Change|LMDB/s|SQLite/s|Candidate vs LMDB|
|---|---:|---:|---:|---:|---:|---:|
|load|547,841|811,439|+48.12%|1,651,003|828,948|-50.85%|
|read|2,524,122|2,569,201|+1.79%|1,696,722|462,562|+51.42%|
|update|103,500|103,430|-0.07%|129,251|58,438|-19.98%|

### Sampled latency

Nanoseconds; lower is better. These are geometric means of trial-sampled p99s. The original write trial has only eight samples; this is not a population tail guarantee.

|Filesystem / operation|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / ordered_kv_mixed|239|239|-0.40%|339|
|disk / ordered_kv_hits|234|232|-0.77%|338|
|disk / ordered_kv_misses|234|234|-0.22%|344|
|disk / ipv4_lpm_mixed|424|418|-1.43%|769|
|disk / ipv4_lpm_hits|433|430|-0.65%|788|
|disk / ipv4_lpm_misses|115|117|+1.65%|584|
|disk / ipv4_lpm_outside_cover_misses|41|41|-0.31%|41|
|disk / ipv4_lpm_inside_gap_misses|116|119|+2.61%|595|
|disk / ipv6_lpm_mixed|517|513|-0.78%|1,253|
|disk / ipv6_lpm_hits|529|530|+0.32%|1,267|
|disk / ipv6_lpm_misses|143|141|-1.66%|1,073|
|disk / ipv6_lpm_outside_cover_misses|41|41|+0.31%|42|
|disk / ipv6_lpm_inside_gap_misses|142|142|-0.05%|1,110|
|disk / durable_batch1|83,140|83,420|+0.34%|184,321|
|disk / durable_batch64|261,163|220,750|-15.47%|468,805|
|ram / ordered_kv_mixed|241|238|-1.00%|339|
|ram / ordered_kv_hits|232|233|+0.46%|335|
|ram / ordered_kv_misses|235|233|-0.93%|340|
|ram / ipv4_lpm_mixed|423|417|-1.29%|767|
|ram / ipv4_lpm_hits|432|426|-1.39%|784|
|ram / ipv4_lpm_misses|117|119|+1.52%|637|
|ram / ipv4_lpm_outside_cover_misses|41|41|-0.34%|48|
|ram / ipv4_lpm_inside_gap_misses|116|118|+1.27%|652|
|ram / ipv6_lpm_mixed|517|512|-1.06%|1,243|
|ram / ipv6_lpm_hits|532|526|-1.12%|1,249|
|ram / ipv6_lpm_misses|145|141|-3.07%|1,075|
|ram / ipv6_lpm_outside_cover_misses|41|41|-0.31%|44|
|ram / ipv6_lpm_inside_gap_misses|143|141|-1.20%|1,117|
|ram / durable_batch1|13,047|13,144|+0.74%|8,298|
|ram / durable_batch64|114,820|114,060|-0.66%|92,689|

### Pooled write samples

Geometric mean of each process pooled sampled p99; nanoseconds, lower is better. These preserve adverse tails separately from the trial-sampled estimates above.

|Filesystem / batch|Baseline ns|Candidate ns|Change|LMDB ns|
|---|---:|---:|---:|---:|
|disk / durable_batch1|238,460|235,178|-1.38%|330,126|
|disk / durable_batch64|459,765|484,913|+5.47%|525,182|
|ram / durable_batch1|17,393|17,474|+0.47%|9,806|
|ram / durable_batch64|328,427|321,284|-2.17%|103,324|

### Lifecycle and file costs

|Metric|Baseline|Candidate|Change|
|---|---:|---:|---:|
|Disk bare open, ms|83.890|82.490|-1.67%|
|RAM bare open, ms|44.179|43.380|-1.81%|
|Million close/reopen, ms|9,653.551|9,661.254|+0.08%|
|Million apparent file bytes|268,435,456|268,435,456|+0.00%|
|Million allocated file bytes|268,435,456|268,435,456|+0.00%|

Million-workload costs against contemporaneous native engines:

|Metric|ULMDB|LMDB|SQLite|
|---|---:|---:|---:|
|Close/reopen, ms|9,661.254|3.688|9.331|
|Apparent files, MiB|256.000|33.047|24.145|
|Allocated files, MiB|256.000|33.043|24.145|
|File count|2|2|1|

Complete 237-metric comparison: [benchmark/work/optimization-20261105/iteration-1-repeat-3/comparison.md](benchmark/work/optimization-20261105/iteration-1-repeat-3/comparison.md). It includes combined write windows, pooled sampled tails, setup, validation, resources, all individual observations and uncertainty. Negative means remain visible; statistical overlap does not establish equivalence.
