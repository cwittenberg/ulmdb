"""Reject incomplete or semantically mismatched timing evidence."""
from collections import defaultdict


def validate(suite, data, size, samples, seed, backend=None, key_kind=None, queries=None):
    pairs = defaultdict(dict)
    if suite == "prefix":
        for row in data["rows"]:
            assert int(row["keys"]) == size
            if row["kind"] == "lookup":
                key = (row["family"], row["operation"], row["distribution"], int(row["round"]))
                assert row["algorithm"] not in pairs[key], "duplicate prefix sample"
                assert float(row["ns_per_op"]) > 0
                pairs[key][row["algorithm"]] = row["checksum"]
        expected = {(family, operation, distribution, sample)
                    for family in ("ipv4", "ipv6") for operation in ("exact", "lpm", "all_covering")
                    for distribution in ("hit", "mixed50", "nested_chain") for sample in range(samples)}
        assert set(pairs) == expected, "missing or unexpected prefix workload/sample"
        for values in pairs.values():
            assert set(values) == {"binary_scalar", "multibit4_checked"}
            assert len(set(values.values())) == 1, "prefix result mismatch"
    elif suite == "exact":
        configs = [r for r in data["rows"] if r["type"] == "config"]
        assert len(configs) == 1
        config = configs[0]
        assert (config["rows"], config["samples"], config["query_seed"], config["backend"], config["key_kind"]) == (size, samples, seed, backend, key_kind)
        assert data["rows"][-1]["type"] == "validation" and data["rows"][-1]["passed"]
        assert queries is None or config["queries"] == queries
        assert any(r["type"] == "snapshot_validation" and r["passed"] for r in data["rows"])
        for row in data["rows"]:
            if row["type"] == "sample":
                key = row["workload"], row["sample"]
                assert row["strategy"] not in pairs[key], "duplicate exact sample"
                assert row["ns_per_op"] > 0 and row["operations"] > 0
                if queries is not None:
                    assert row["operations"] == (min(2000, queries) if row["workload"] == "scan100" else queries)
                pairs[key][row["strategy"]] = row["checksum"]
        assert set(pairs) == {(workload, sample) for workload in ("hit", "miss", "mixed50", "hot90", "scan100") for sample in range(samples)}, "missing or unexpected exact workload/sample"
        for (workload, _), values in pairs.items():
            assert set(values) == {"btree", "hash_ordered_fallback" if workload == "scan100" else "hash"}
            assert len(set(values.values())) == 1, "exact result mismatch"
    else:
        assert (data["seed"], data["samples"]) == (seed, samples)
        assert queries is None or data["queries"] == queries
        assert {result["key_kind"] for result in data["results"]} == {"fixed16", "hostname"}
        for result in data["results"]:
            assert result["keys"] == size and result["validation_passed"] and result["old_snapshot_after_write_passed"]
            assert result["bbtree_helper"] is not None, "missing actual bbtree helper results"
            pairs.clear()
            for row in result["measurements"]:
                key = row["workload"], row["sample"]
                assert row["strategy"] not in pairs[key], "duplicate ordered sample"
                assert row["ns_per_op"] > 0
                if queries is not None:
                    assert row["queries"] == (min(4096, queries) if row["workload"] == "range64" else queries)
                pairs[key][row["strategy"]] = row["checksum"]
            assert set(pairs) == {(workload, sample) for workload in ("hit", "miss", "mixed_50_50", "range64") for sample in range(samples)}, "missing or unexpected ordered workload/sample"
            for values in pairs.values():
                assert set(values) == {"ulmdb_legacy_prepared", "bbtree_0_2_0_unsync", "std_btreemap"}
                assert len(set(values.values())) == 1, "ordered result mismatch"
