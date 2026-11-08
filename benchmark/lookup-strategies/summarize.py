"""Summarize fresh-process medians, retaining every slower result."""
from collections import defaultdict
from pathlib import Path
import argparse
import json
import statistics

MEDIAN = statistics.median


def normalize(document):
    for run in document["runs"]:
        suite, size, process = run["suite"], run["size"], run["process"]
        data = run["report"]
        if suite == "prefix":
            for row in data["rows"]:
                if row["kind"] == "lookup":
                    yield (suite, size, row["family"], "", row["operation"], row["distribution"]), process, row["algorithm"], float(row["ns_per_op"])
        elif suite == "exact":
            for row in data["rows"]:
                if row["type"] == "sample":
                    yield (suite, size, run["backend"], run["key_kind"], row["workload"], ""), process, row["strategy"], row["ns_per_op"]
        else:
            for result in data["results"]:
                assert result["validation_passed"] and result["old_snapshot_after_write_passed"]
                for row in result["measurements"]:
                    yield (suite, size, "legacy_default_features", result["key_kind"], row["workload"], ""), process, row["strategy"], row["ns_per_op"]


def summarize(document):
    groups = defaultdict(lambda: defaultdict(lambda: defaultdict(list)))
    for key, process, strategy, elapsed in normalize(document):
        groups[key][strategy][process].append(elapsed)
    output = []
    for key, strategies in sorted(groups.items()):
        baseline = {"prefix": "binary_scalar", "exact": "btree", "ordered": "ulmdb_legacy_prepared"}[key[0]]
        assert baseline in strategies, (key, strategies.keys())
        base = {p: MEDIAN(values) for p, values in strategies[baseline].items()}
        for candidate, samples in sorted(strategies.items()):
            if candidate == baseline:
                continue
            candidate_values = {p: MEDIAN(values) for p, values in samples.items()}
            assert set(base) == set(candidate_values)
            ratios = [base[p] / candidate_values[p] for p in sorted(base)]
            output.append(dict(zip(("suite", "keys", "backend_or_family", "key_kind", "operation", "distribution"), key)) |
                          {"baseline": baseline, "candidate": candidate,
                           "baseline_ns": MEDIAN(base.values()), "candidate_ns": MEDIAN(candidate_values.values()),
                           "paired_process_speedup_median": MEDIAN(ratios), "paired_process_speedup_min": min(ratios),
                           "paired_process_speedup_max": max(ratios), "process_speedups": ratios,
                           "material_all_processes": min(ratios) >= 1.10,
                           "slower_over_5_percent_any_process": min(ratios) < 1 / 1.05})
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("results", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    document = json.loads(args.results.read_text())
    assert document["complete"], "partial campaigns cannot be summarized as complete"
    rows = summarize(document)
    args.output.write_text(json.dumps({"schema": 1, "source_results": str(args.results),
                                      "method": "median of within-process samples, then paired speedup across fresh processes; range is observed, not a confidence interval",
                                      "rows": rows}, indent=2) + "\n")
    for row in rows:
        fields = [str(row[k]) for k in ("suite", "keys", "backend_or_family", "key_kind", "operation", "distribution", "candidate")]
        print(" / ".join(x for x in fields if x) + f": {row['paired_process_speedup_median']:.2f}x "
              f"[{row['paired_process_speedup_min']:.2f}, {row['paired_process_speedup_max']:.2f}]")


if __name__ == "__main__":
    main()
