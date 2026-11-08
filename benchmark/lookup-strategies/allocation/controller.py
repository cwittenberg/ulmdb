#!/usr/bin/env python3
"""Linux allocator-refusal gate; this does not test OS page-reclaim pressure.

Only the owned child receives RLIMIT_AS. Its source and read transaction are
already loaded before the parent changes the soft limit. The original limit is
restored for an identical allocation retry in the same immutable snapshot.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import select
import subprocess
import sys
import time

MIB = 1 << 20


class Inconclusive(RuntimeError):
    pass


def require(condition, message):
    if not condition:
        raise ValueError(message)


def status_bytes(pid):
    result = {}
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        if line.startswith(("VmSize:", "VmRSS:", "VmData:", "VmHWM:")):
            name, value, unit = line.split()
            require(unit == "kB", "unexpected /proc memory unit")
            result[name[:-1]] = int(value) * 1024
    require(result.get("VmSize", 0) > 0, "missing owned-child VmSize")
    return result


def anonymous_address_bytes(pid):
    """Conservative malloc-reuse upper bound; even stacks are counted.

    Every private, writable, inode-zero mapping counts in full, including all
    allocated/used bytes. Real reusable allocator space can only be smaller.
    This prevents declaring success merely because malloc reused a large heap.
    """
    total = 0
    for line in Path(f"/proc/{pid}/maps").read_text().splitlines():
        fields = line.split(maxsplit=5)
        require(len(fields) >= 5, "malformed owned-child maps line")
        address, permissions, _, _, inode = fields[:5]
        if permissions.startswith("rw") and permissions[3:] == "p" and inode == "0":
            first, last = (int(x, 16) for x in address.split("-"))
            total += last - first
    return total


def validate_ready(ready, keys, budget):
    require(ready.get("type") == "ready" and ready.get("schema") == 1, "wrong child handshake")
    require(ready.get("keys") == keys and ready.get("max_entries") == keys, "entry admission mismatch")
    require(ready.get("value_bytes") == 8 and ready.get("max_value_bytes") == 8,
            "value admission mismatch")
    require(ready.get("max_total_value_bytes") == keys * 8, "logical value quota mismatch")
    required = ready.get("required_index_bytes")
    require(isinstance(required, int) and required > 0, "invalid requested allocation")
    require(ready.get("index_budget_bytes") == budget and required <= budget,
            "configured budget would reject before allocator")
    require(ready.get("full_source_verified") is True and ready.get("original_native_bound") is True, "source/original native reader not verified before limiting")
    require(isinstance(ready.get("sequence"), int) and isinstance(ready.get("commit"), str),
            "missing source identity")


def classify(ready, attempt, retry, guard, returncode):
    """Return a verdict without interpreting an unknown result as a pass."""
    if returncode != 0:
        return "fail", "child failed or panicked"
    for expected, row in (("attempt", attempt), ("retry", retry)):
        if row.get("type") != expected:
            return "fail", "missing allocation phase"
        for key in ("full_source_verified", "native_and_ordered_usable", "original_native_still_usable", "snapshot_unchanged"):
            if row.get(key) is not True:
                return "fail", f"{expected}: source usability proof missing"
        if (row.get("sequence"), row.get("commit")) != (ready["sequence"], ready["commit"]):
            return "fail", f"{expected}: source identity changed"
    first = attempt.get("outcome", {}).get("kind")
    second = retry.get("outcome", {})
    if first == "allocated":
        return "inconclusive", "allocation fit under the cap; allocator refusal was not established"
    if first != "limit_exceeded":
        return "fail", "unexpected constrained allocation result"
    if second.get("kind") != "allocated" or second.get("all_hash_values_verified") is not True:
        return "fail", "identical hash preparation did not recover after restoring the limit"
    if second.get("entries") != ready["keys"]:
        return "fail", "retry index has wrong cardinality"
    allocated = second.get("index_bytes")
    if not isinstance(allocated, int) or not ready["required_index_bytes"] <= allocated <= ready["index_budget_bytes"]:
        return "fail", "retry allocation size is inconsistent with admission"
    if not guard:
        return "inconclusive", "existing anonymous address space could hide allocator reuse"
    return "pass", "allocator refusal returned LimitExceeded; original native/Ordered source stayed usable and identical retry recovered"


class Channel:
    def __init__(self, process, raw, timeout):
        self.process, self.raw, self.timeout = process, raw, timeout
        self.buffer = b""

    def receive(self):
        deadline = time.monotonic() + self.timeout
        while b"\n" not in self.buffer:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("child protocol deadline exceeded")
            selected, _, _ = select.select([self.process.stdout], [], [], remaining)
            if not selected:
                raise TimeoutError("child protocol deadline exceeded")
            chunk = os.read(self.process.stdout.fileno(), 65536)
            if not chunk:
                raise EOFError("child exited before completing protocol")
            self.buffer += chunk
            require(len(self.buffer) <= 65536, "child protocol line too large")
        line, self.buffer = self.buffer.split(b"\n", 1)
        self.raw.write(line + b"\n")
        self.raw.flush()
        return json.loads(line)

    def send(self, message):
        self.process.stdin.write(message.encode() + b"\n")
        self.process.stdin.flush()


def execute(args):
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    binary = args.binary.resolve(strict=True)
    command = [str(binary), "--keys", str(args.keys), "--dir", str(output / "database"),
               "--index-budget-bytes", str(args.index_budget_mib * MIB)]
    report = {"schema": 1, "classification": "actual allocator refusal and recovery, not OS reclaim pressure",
              "status": "fail", "command": command, "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "headroom_bytes": args.headroom_mib * MIB, "scope": "owned Linux child; RLIMIT_AS soft cap only; same loaded source/read transaction before and after failure"}
    process = None
    original_limit = None
    restored = False
    try:
        with (output / "stderr.log").open("wb") as err, (output / "stdout.jsonl").open("wb") as raw:
            process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err)
            channel = Channel(process, raw, args.timeout)
            ready = channel.receive()
            report["ready"] = ready
            validate_ready(ready, args.keys, args.index_budget_mib * MIB)
            # Do not poll()/wait() before the last prlimit operation: keeping a
            # dead child unreaped prevents PID reuse while handling a race.
            exited = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            require(exited is None, "owned child exited before resource admission")
            before = status_bytes(process.pid)
            anonymous = anonymous_address_bytes(process.pid)
            original_limit = resource.prlimit(process.pid, resource.RLIMIT_AS)
            target = before["VmSize"] + args.headroom_mib * MIB
            hard = original_limit[1]
            if hard != resource.RLIM_INFINITY and target > hard:
                raise Inconclusive("existing hard AS limit cannot admit requested cap")
            original_soft = original_limit[0]
            if original_soft != resource.RLIM_INFINITY and original_soft < before["VmSize"] + ready["required_index_bytes"] + args.headroom_mib * MIB:
                raise Inconclusive("original AS limit has insufficient recovery headroom")
            resource.prlimit(process.pid, resource.RLIMIT_AS, (target, hard))
            applied = resource.prlimit(process.pid, resource.RLIMIT_AS)
            require(applied == (target, hard), "owned-child limit did not match requested cap")
            report["limit"] = {"original": list(original_limit), "applied": list(applied),
                               "source_process": before, "private_writable_anonymous_upper_bound_bytes": anonymous}
            guard = ready["required_index_bytes"] > anonymous + args.headroom_mib * MIB
            report["allocation_exceeds_all_existing_anonymous_space_plus_headroom"] = guard
            channel.send("attempt")
            attempt = channel.receive()
            report["attempt"] = attempt
            # Restore only this same owned child, retaining its existing source
            # and transaction. The program itself never changes resource limits.
            resource.prlimit(process.pid, resource.RLIMIT_AS, original_limit)
            restored = resource.prlimit(process.pid, resource.RLIMIT_AS) == original_limit
            require(restored, "original limit was not restored")
            report["restored_limit"] = list(original_limit)
            channel.send("retry")
            retry = channel.receive()
            report["retry"] = retry
            channel.send("finish")
            process.stdin.close()
            code = process.wait(timeout=args.timeout)
            report["child_returncode"] = code
            report["status"], report["reason"] = classify(ready, attempt, retry, guard, code)
    except Inconclusive as error:
        report["status"] = "inconclusive"
        report["reason"] = str(error)
    except Exception as error:
        report["reason"] = f"{type(error).__name__}: {error}"
    finally:
        if process is not None:
            if original_limit is not None and not restored:
                try:
                    resource.prlimit(process.pid, resource.RLIMIT_AS, original_limit)
                except OSError as error:
                    report["limit_restore_cleanup_error"] = str(error)
            # No more prlimit calls after reaping; Popen handles already-dead
            # owned children without signaling another process.
            if process.returncode is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            report.setdefault("child_returncode", process.returncode)
        (output / "result.json").write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
    print(json.dumps({"status": report["status"], "reason": report.get("reason"), "evidence": str(output / "result.json")}))
    return {"pass": 0, "inconclusive": 2}.get(report["status"], 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--keys", type=int, default=2_000_000)
    parser.add_argument("--index-budget-mib", type=int, default=512)
    parser.add_argument("--headroom-mib", type=int, default=8)
    parser.add_argument("--timeout", type=float, default=180)
    args = parser.parse_args()
    if not 1 <= args.keys <= 4_000_000 or args.index_budget_mib <= 0 or not 1 <= args.headroom_mib <= 64 or args.timeout <= 0:
        parser.error("invalid counts, limits or timeout")
    if not sys.platform.startswith("linux") or not hasattr(resource, "prlimit"):
        parser.error("Linux resource.prlimit and /proc required")
    return execute(args)


if __name__ == "__main__":
    raise SystemExit(main())
