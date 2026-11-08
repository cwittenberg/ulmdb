"""Render standalone evidence tables and a plot from the retained campaign."""
from collections import defaultdict
from pathlib import Path
import argparse
import json
import statistics

from summarize import summarize

MEDIAN = statistics.median


def costs(document):
    groups = defaultdict(list)
    for run in document["runs"]:
        suite, size = run["suite"], run["size"]
        if suite == "prefix":
            for row in run["report"]["rows"]:
                if row["kind"] == "build":
                    groups[(suite, size, row["family"], row["algorithm"])].append(
                        (float(row["build_ms"]), int(row["arena_bytes"]), int(row["reachable_bytes"])))
        elif suite == "exact":
            build = next(row for row in run["report"]["rows"] if row["type"] == "hash_build")
            rebuild = next(row for row in run["report"]["rows"] if row["type"] == "snapshot_validation")
            groups[(suite, size, run["backend"], run["key_kind"])].append(
                (MEDIAN(build["elapsed_ns"]) / 1e6, build["estimated_extra_bytes"], rebuild["one_update_full_rebuild_ns"] / 1e6))
        else:
            for row in run["report"]["results"]:
                helper = row["bbtree_helper"]
                total = row["build"]["snapshot_enumeration_ns"] + row["build"]["bbtree_fixture_export_ns"] + helper["build"]["complete_helper_build_ns"]
                memory = helper["memory"]["bbtree_requested_accelerator_bytes_excluding_fixture"] + helper["memory"]["fixture_bytes"]
                groups[(suite, size, "legacy", row["key_kind"])].append((total / 1e6, memory, 0))
    return [{"suite": key[0], "keys": key[1], "variant": key[2], "algorithm_or_keys": key[3],
             "build_ms": MEDIAN(v[0] for v in values), "bytes": MEDIAN(v[1] for v in values),
             "reachable_bytes_or_rebuild_ms": None if key[0] == "ordered" else MEDIAN(v[2] for v in values)} for key, values in sorted(groups.items())]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("results", type=Path)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    document = json.loads(args.results.read_text())
    assert document["complete"]
    rows, construction = summarize(document), costs(document)
    out = args.output_dir
    out.mkdir(parents=True, exist_ok=True)
    (out / "summary.json").write_text(json.dumps({"schema": 1, "rows": rows, "costs": construction}, indent=2) + "\n")
    config, machine = document["configuration"], document["machine"]
    text = ["# Lookup strategy benchmark", "",
            "This campaign evaluates optional read accelerators. The experiment adds no production engine, dependency, default, file format or published performance-report changes.", "",
            f"Machine: {machine['cpu_model']}; Linux {machine['kernel']}; Rust 1.94.0; CPU {machine['cpu']}. Shared host, no exclusive CPU reservation. ",
            f"{config['processes']} fresh processes per configuration; {config['samples']} samples per process; seeds {config['seed']} through {config['seed'] + config['processes'] - 1}. In-process candidates alternate order; the ordered bbtree helper runs as a block before/after native samples, rotating by seed/key kind. All samples are retained. Values summarize process medians; ranges are observed process speedups, not confidence intervals.", "",
            "Speedup is baseline time / candidate time. Values below 1 are slower. The prespecified material-improvement criterion is at least 1.10x in every fresh process. This is a screening criterion, not statistical equivalence or a universal no-regression guarantee.", "",
            "## Comparable scope", "",
            "- Prefix: actual prepared scalar binary radix versus existing checked 4-bit multibit implementation, in the same checked contiguous arena. Index-only, warm, single thread; real persistence and transactions are outside these measurements. Exact, LPM, and complete ancestor results match byte-for-byte, including interior/exterior misses and nested hot chains.",
            "- Exact: actual file-backed prepared Legacy and Resident readers versus snapshot-bound std HashMap with randomized hashing, copied keys and borrowed values. Both backends are built with the accepted Resident feature set; Legacy here is feature-enabled. Complete cursor enumeration, allocation and copying are charged to index construction. Range scans use the original ordered reader.",
            "- Ordered: default-feature public Legacy, std BTreeMap and actual bbtree 0.2.0. Bbtree uses a separate helper over an exact snapshot export because its Verus dependency pins conflict with ULMDB. Helper startup/IPC is outside lookup samples; export/read/decode is listed in build costs. Its storage differs from a live ULMDB mmap, so this does not qualify in-process integration.",
            "- Exact and ordered use different key distributions, value sizes and compilation features. Compare each candidate with its own baseline, not absolute times between those two harnesses.",
            "- Fixed synthetic fixtures, warm snapshots and one core do not establish cold-cache, larger-than-RAM, concurrency, write/recovery or real IPDon end-to-end performance. PlanB's published benchmark was not reproduced.", "",
            "The hash comparison and scan fallback controls run with the side index already resident. They do not measure cache/RSS effects against a source-only process. Old/new snapshot checks prove byte isolation, not bounded retained generations or unchanged writer/reclamation cost. Build timings exclude later destruction of the old index, and a rebuild can retain both indexes plus their snapshots. Eager full rebuild after every commit would add the reported O(N) rebuild cost. Automatic promotion is not qualified by these results.", "",
            "## Lookup timings", "",
            "| Suite | Records | Backend/family | Keys | Operation | Distribution | Candidate | Baseline ns/query | Candidate ns/query | Speedup | Process range | Material in every process |",
            "|---|---:|---|---|---|---|---|---:|---:|---:|---|---|"]
    for row in rows:
        text.append("| " + " | ".join(str(row[k]) for k in ("suite", "keys", "backend_or_family", "key_kind", "operation", "distribution", "candidate")) +
                    f" | {row['baseline_ns']:.1f} | {row['candidate_ns']:.1f} | {row['paired_process_speedup_median']:.2f}x | {row['paired_process_speedup_min']:.2f}–{row['paired_process_speedup_max']:.2f}x | {'Yes' if row['material_all_processes'] else 'No'} |")
    text += ["", "Range-query times include consuming up to 64 (ordered) or 100 (exact fallback) records. Prefix all-covering consumes every matching ancestor; these are not single-winner timings.", "",
             "## Construction and memory", "",
             "Memory is requested or estimated storage, not RSS. Hash estimates exclude allocator overhead, source mappings and retained old snapshots. Prefix arena high-water includes abandoned private allocations; reachable size excludes them. Binary construction uses its sorted bulk loader; multibit uses its available incremental builder.", "",
             "| Suite | Records | Variant | Algorithm/keys | Build ms | Arena/additional MiB | Reachable MiB / hash rebuild ms |",
             "|---|---:|---|---|---:|---:|---:|"]
    for row in construction:
        extra = row["reachable_bytes_or_rebuild_ms"] / (1 << 20) if row["suite"] == "prefix" else row["reachable_bytes_or_rebuild_ms"]
        extra_text = "N/A" if row["suite"] == "ordered" else f"{extra:.2f}"
        text.append(f"| {row['suite']} | {row['keys']} | {row['variant']} | {row['algorithm_or_keys']} | {row['build_ms']:.2f} | {row['bytes'] / (1 << 20):.2f} | {extra_text} |")
    text += ["", "## Hash construction break-even", "",
             "Estimated lookups required to amortize one complete hash build on the mixed workload. This excludes extra-memory cost and generation retention, and assumes unchanged timing after integration.", "",
             "| Records | Backend | Keys | Build ms | Saved ns/query | Queries to break even |",
             "|---:|---|---|---:|---:|---:|"]
    for row in rows:
        if row["suite"] != "exact" or row["operation"] != "mixed50":
            continue
        cost = next(c for c in construction if (c["suite"], c["keys"], c["variant"], c["algorithm_or_keys"]) == ("exact", row["keys"], row["backend_or_family"], row["key_kind"]))
        saved = row["baseline_ns"] - row["candidate_ns"]
        count = f"{int(cost['build_ms'] * 1e6 / saved) + 1:,}" if saved > 0 else "Never at measured medians"
        text.append(f"| {row['keys']} | {row['backend_or_family']} | {row['key_kind']} | {cost['build_ms']:.2f} | {saved:.1f} | {count} |")
    text += ["", "## Provenance", "",
             "Per-run source revisions and executable hashes are retained in results.json. A setup correction capped the million-row Resident fixture's WAL request at its existing supported 256 MiB limit; smaller-fixture settings and all lookup algorithms stayed unchanged. Completed samples were retained under their original hashes, while the remaining jobs used rebuilt harnesses. The frozen engine sources are identical across both harness revisions. The rejected setup attempt and earlier development pilots remain recorded separately and contribute no successful timing rows.", "",
             "## Reproduce", "", "```sh",
             "python3 benchmark/lookup-strategies/run.py --suite all \\",
             f"  --sizes {config['sizes']} --queries {config['queries']} --samples {config['samples']} \\",
             f"  --prefix-iterations {config['prefix_iterations']} \\",
             f"  --processes {config['processes']} --cpu {config['cpu']} --seed {config['seed']} \\",
             "  --output benchmark/work/NEW_LOOKUP_CAMPAIGN", "```", "",
             "Use a fresh output path and an available CPU. The runner freezes the dirty source, builds locked dependencies, pins every process, rejects mismatched/missing results and retains all raw samples. Existing benchmark reports are untouched. Source and executable hashes are retained in results.json.", ""]
    (out / "REPORT.md").write_text("\n".join(text))
    print(out / "REPORT.md")


if __name__ == "__main__":
    main()
