#!/usr/bin/env python3
"""Validate the declarative LMDB backlog and render its evidence status.

This command checks claims; it does not run the database or create evidence.
Recorded C differential and explicitly scoped native-test results use separate rules.
"""
from __future__ import annotations

import argparse
import json
import re
import sys

sys.dont_write_bytecode = True
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
DEFAULT_MATRIX = Path(__file__).with_name("matrix.json")
DEFAULT_REPORT = Path(__file__).with_name("STATUS.md")
sys.path.insert(0, str(ROOT / "clients/lmdb-ulmdb/qualification"))
import native_evidence

STATES = {"supported", "emulated", "unsupported", "unqualified"}
EXPECTED_IDS = {f"F{i:02}" for i in range(1, 13)} | {f"G{i:02}" for i in range(1, 21)}
HASH = re.compile(r"[0-9a-f]{64}\Z")


class Invalid(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise Invalid(message)


def repository_path(value, root=ROOT):
    require(isinstance(value, str) and value, "path must be a nonempty string")
    path = (root / value).resolve()
    require(path.is_relative_to(root.resolve()), f"path escapes repository: {value}")
    return path


def read_json(path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise Invalid(f"cannot read {path}: {error}") from error


def check_receipt(receipt, label):
    require(receipt.get("schema") == 1, f"{label}: unsupported receipt schema")
    require(receipt.get("complete") is True and receipt.get("passed") is True,
            f"{label}: receipt is incomplete or failed")
    require(HASH.fullmatch(receipt.get("adapter", {}).get("sha256", "")),
            f"{label}: missing adapter artifact hash")
    symbols = {}
    for group in ("common_symbols", "modern_additional_symbols"):
        inventory = receipt.get(group, {})
        rows = inventory.get("symbols", [])
        require(inventory.get("declared") == len(rows), f"{label}: {group} count mismatch")
        require(inventory.get("exported") == sum(row.get("exported") is True for row in rows),
                f"{label}: {group} exported count mismatch")
        for row in rows:
            name = row.get("name")
            require(isinstance(name, str) and name not in symbols,
                    f"{label}: duplicate or invalid export {name}")
            symbols[name] = row.get("exported") is True
    # Missing modern extensions are expected backlog; common exports are mandatory.
    require(all(row.get("exported") is True for row in receipt.get("common_symbols", {}).get("symbols", [])),
            f"{label}: missing common export")
    require(receipt.get("common_symbols", {}).get("declared", 0) > 0,
            f"{label}: empty common export inventory")
    results = receipt.get("results", [])
    require(results, f"{label}: no recorded results")
    seen = set()
    for row in results:
        key = (row.get("scenario"), row.get("reference"), row.get("seed", 0))
        require(key not in seen, f"{label}: duplicate result selector {key}")
        seen.add(key)
        require(row.get("passed") is True and row.get("reference_exit") == 0 and row.get("adapter_exit") == 0,
                f"{label}: failed result {key}")
        reference_hash = row.get("reference_trace_sha256", "")
        adapter_hash = row.get("adapter_trace_sha256", "")
        # Symbol-only rows and deployment-only rows may have no reference trace.
        # They are valid context but cannot qualify a differential target below.
        if reference_hash:
            require(HASH.fullmatch(reference_hash) and reference_hash == adapter_hash,
                    f"{label}: trace mismatch for {key}")
            require(HASH.fullmatch(row.get("executable_sha256", "")) and
                    row.get("executable_sha256") == row.get("adapter_executable_sha256"),
                    f"{label}: changed executable for {key}")
    return symbols


def result_for(test, receipts, feature_id):
    alias = test.get("receipt")
    require(alias in receipts, f"{feature_id}: recorded test has no available receipt {alias}")
    selector = test.get("selector", {})
    if receipts[alias].get("kind") == "native-test":
        require(feature_id == "G05", f"{feature_id}: native receipts cannot qualify public LMDB APIs or other features")
        require(set(selector) == {"suite", "test", "backend"}, f"{feature_id}: invalid native result selector")
        matches = [row for row in receipts[alias]["results"]
                   if all(row.get(key) == value for key, value in selector.items())]
        require(len(matches) == 1, f"{feature_id}: native selector matches {len(matches)} results")
        row = matches[0]
        require(test.get("role") in row["roles"], f"{feature_id}: native case does not qualify requested role")
        require(test.get("backends") == [row["backend"]], f"{feature_id}: native evidence backend mismatch")
        suite = next(item for item in receipts[alias]["suites"] if item["id"] == row["suite"])
        require(test.get("path") == suite["source"], f"{feature_id}: native test source mismatch")
        return row
    require(set(selector) >= {"scenario", "reference"}, f"{feature_id}: incomplete result selector")
    matches = [row for row in receipts[alias]["results"]
               if all(row.get(key, 0 if key == "seed" else None) == value
                      for key, value in selector.items())]
    require(len(matches) == 1, f"{feature_id}: selector matches {len(matches)} results: {selector}")
    row = matches[0]
    require(HASH.fullmatch(row.get("reference_trace_sha256", "")),
            f"{feature_id}: target needs a behavioral differential trace")
    return row


def validate(matrix, root=ROOT, receipt_overrides=None):
    require(matrix.get("schema") == 1, "unsupported matrix schema")
    backends = matrix.get("backends", {})
    require(backends and all(row.get("state") in STATES for row in backends.values()),
            "invalid backend state")
    receipts, exports = {}, {}
    receipt_overrides = receipt_overrides or {}
    require(set(receipt_overrides) <= {row["id"] for row in matrix.get("receipts", [])},
            "unknown receipt override alias")
    for row in matrix.get("receipts", []):
        alias = row["id"]
        require(alias not in exports, f"duplicate receipt alias {alias}")
        path = Path(receipt_overrides[alias]) if alias in receipt_overrides else repository_path(row["path"], root)
        if not path.exists() and not row.get("required", True):
            continue
        receipt = read_json(path)
        require(receipt.get("profile") == row["profile"], f"{alias}: receipt profile mismatch")
        kind = row.get("kind", "differential")
        require(receipt.get("kind", "differential") == kind, f"{alias}: receipt kind mismatch")
        if kind == "native-test":
            try:
                native_evidence.check_native(receipt, alias, root)
            except native_evidence.Invalid as error:
                raise Invalid(str(error)) from error
            exports[alias] = {}
        else:
            require(kind == "differential", f"{alias}: unsupported receipt kind")
            exports[alias] = check_receipt(receipt, alias)
        receipts[alias] = receipt
    features = matrix.get("features", [])
    ids = [row.get("id") for row in features]
    require(set(ids) == EXPECTED_IDS and len(ids) == len(EXPECTED_IDS),
            "matrix must contain exactly F01-F12 and G01-G20 once")
    header = repository_path(matrix["adapter_header"], root).read_text()
    for feature in features:
        fid, state = feature["id"], feature.get("state")
        require(state in STATES, f"{fid}: invalid state")
        require(feature.get("implementation") in {"missing", "partial", "implemented"},
                f"{fid}: invalid implementation status")
        require(feature.get("backends") and set(feature["backends"]) <= set(backends),
                f"{fid}: unknown or missing backend")
        require(feature.get("target") and feature.get("limits"), f"{fid}: target/limits required")
        if fid == "G05":
            require(feature.get("requires_performance") is True,
                    "G05: native coordination requires explicit matched performance acceptance")
        require(feature.get("sources"), f"{fid}: source audit pointers required")
        for path in feature["sources"]:
            require(repository_path(path, root).is_file(), f"{fid}: missing source {path}")
        for declaration in feature.get("declarations", []):
            name, declared = declaration["name"], declaration["status"]
            require(declared in {"present", "missing"}, f"{fid}: invalid declaration status")
            if declared == "present":
                require(re.search(r"\b" + re.escape(name) + r"\b", header),
                        f"{fid}: declaration absent from adapter header: {name}")
        roles = Counter()
        for test in feature.get("tests", []):
            require(test.get("role") in {"positive", "negative", "reopen", "resource"},
                    f"{fid}: invalid test role")
            status = test.get("status")
            require(status in {"planned", "existing", "recorded"}, f"{fid}: invalid test status")
            require(test.get("case"), f"{fid}: test needs a concrete case")
            if status != "planned":
                require(repository_path(test["path"], root).is_file(), f"{fid}: test path does not exist")
            if status == "recorded":
                result_for(test, receipts, fid)
                require(set(test.get("backends", [])) <= set(feature["backends"]),
                        f"{fid}: evidence backend outside feature scope")
                roles[test["role"]] += 1
            else:
                require("receipt" not in test and "selector" not in test,
                        f"{fid}: planned/existing test cannot claim recorded evidence")
        require(any(test["role"] == "positive" for test in feature.get("tests", [])) and
                any(test["role"] == "negative" for test in feature.get("tests", [])),
                f"{fid}: positive and negative test targets required")
        for context in feature.get("historical_context", []):
            result_for(context, receipts, fid)
        if state == "supported":
            require(feature["implementation"] == "implemented", f"{fid}: supported target is not implemented")
            require(all(row["status"] == "present" for row in feature.get("declarations", [])),
                    f"{fid}: supported target has missing declarations")
            require(all(roles[role] for role in ("positive", "negative", "reopen")),
                    f"{fid}: supported target lacks recorded positive/negative/reopen evidence")
            requirements = feature.get("qualification_requirements")
            if requirements:
                require({item["backend"] for item in requirements} == set(feature["backends"]),
                        f"{fid}: requirements do not cover advertised backends")
                for requirement in requirements:
                    require(requirement["kind"] in {"native-test", "differential"}, f"{fid}: invalid evidence requirement")
                    references = requirement.get("references", [None])
                    for reference in references:
                        for role in requirement["roles"]:
                            covered = [test for test in feature["tests"] if test["status"] == "recorded"
                                       and test["role"] == role and requirement["backend"] in test.get("backends", [])
                                       and receipts[test["receipt"]].get("kind", "differential") == requirement["kind"]
                                       and (reference is None or test["selector"].get("reference") == reference)]
                            require(covered, f"{fid}: no {role} {requirement['kind']} evidence for {requirement['backend']}")
            else:
                require(feature.get("references"), f"{fid}: supported target needs explicit reference versions")
                for backend in feature["backends"]:
                    for reference in feature["references"]:
                        for role in ("positive", "negative", "reopen"):
                            covered = [test for test in feature["tests"] if test["status"] == "recorded"
                                       and test["role"] == role and backend in test.get("backends", [])
                                       and test["selector"]["reference"] == reference]
                            require(covered, f"{fid}: no {role} evidence for {backend}/{reference}")
            if feature.get("requires_performance"):
                # The native assertion profile supplies no matched-baseline
                # performance decision. A later typed comparison gate must add
                # its own validator before an accepted performance claim exists.
                require(False, f"{fid}: matched performance acceptance is pending")
            for declaration in feature.get("declarations", []):
                if declaration.get("kind") == "function":
                    require(any(exports[test["receipt"]].get(declaration["name"]) is True
                                for test in feature["tests"] if test["status"] == "recorded"),
                            f"{fid}: missing export {declaration['name']}")
    return receipts


def render(matrix, receipts):
    states = Counter(row["state"] for row in matrix["features"])
    lines = ["# LMDB feature backlog status", "", "Generated from [matrix.json](matrix.json) by [registry.py](registry.py).",
             "This is a source/evidence inventory, not a new runtime qualification.", ""]
    summary = matrix.get("implementation_summary", [])
    require(isinstance(summary, list) and len(summary) <= 16
            and all(isinstance(item, str) and 0 < len(item) <= 4096 for item in summary),
            "invalid implementation summary")
    for paragraph in summary:
        lines += [paragraph, ""]
    lines += ["; ".join(f"**{states[state]} {state}**" for state in ("supported", "emulated", "unsupported", "unqualified")) + ".",
             "Counts describe these 32 backlog targets, not percentages of all LMDB features.", "",
             "`emulated` preserves logical behavior with compatibility work; the target improvement remains open.",
             "`unqualified` includes partial or newly implemented work without sufficient target evidence.",
             "Existing/planned test paths never count as recorded passes.", "", "| ID | Target | State | Implementation | Next milestone | Recorded target tests |",
             "| --- | --- | --- | --- | --- | ---: |"]
    for feature in matrix["features"]:
        count = sum(test["status"] == "recorded" for test in feature["tests"])
        lines.append(f"| {feature['id']} | {feature['target']} | {feature['state']} | {feature['implementation']} | {feature['milestone']} | {count} |")
    lines += ["", "## Backend boundaries", "", "| Backend | Qualification state | Boundary |", "| --- | --- | --- |"]
    for name, backend in matrix["backends"].items():
        lines.append(f"| {name} | {backend['state']} | {backend['boundary']} |")
    lines += ["", "## Evidence inputs", ""]
    for alias, receipt in receipts.items():
        if receipt.get("kind") == "native-test":
            lines.append(f"- `{alias}`: `{receipt['profile']}`, native assertion/process/filesystem oracle, source SHA-256 `{receipt['source']['sha256']}`; {len(receipt['results'])} scoped results. This supplies no LMDB differential or performance acceptance.")
        else:
            lines.append(f"- `{alias}`: `{receipt['profile']}`, generated `{receipt.get('generated_utc', 'unknown')}`, adapter SHA-256 `{receipt['adapter']['sha256']}`; {len(receipt['results'])} recorded checks.")
    lines += ["", "Historical common-profile checks are context only for the backlog rows. They do not qualify missing extensions, native backend behavior, durability or performance.", "",
              "Run `python3 clients/lmdb-ulmdb/features/registry.py check` from the repository root.", ""]
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "render"))
    parser.add_argument("--matrix", type=Path, default=DEFAULT_MATRIX)
    parser.add_argument("--output", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--receipt", action="append", default=[], metavar="ALIAS=PATH",
                        help="use a newly generated receipt without changing registry paths")
    args = parser.parse_args(argv)
    try:
        overrides = dict(item.split("=", 1) for item in args.receipt)
        matrix = read_json(args.matrix)
        receipts = validate(matrix, receipt_overrides=overrides)
        content = render(matrix, receipts)
        if args.command == "render":
            args.output.write_text(content)
            print(f"Wrote {args.output}")
        else:
            require(args.output.is_file() and args.output.read_text() == content,
                    f"generated status is stale; run {Path(__file__).name} render")
            print(f"PASS: {len(matrix['features'])} backlog targets; {len(receipts)} validated receipts; generated status current")
    except (Invalid, KeyError, TypeError, ValueError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
