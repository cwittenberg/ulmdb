#!/usr/bin/env python3
"""Run frozen native file-coordination cases and preserve typed evidence."""
from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import signal
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

sys.dont_write_bytecode = True
import native_evidence as evidence

ROOT = evidence.ROOT


def utc():
    return datetime.now(timezone.utc).isoformat()


def communicate_owned(command, environment, timeout):
    """Own the whole session, including test workers, until exit or cancellation."""
    process = subprocess.Popen(command, cwd=ROOT, env=environment, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True, encoding="utf-8",
                               errors="strict", start_new_session=True)

    def send(sig):
        try:
            os.killpg(process.pid, sig)
        except ProcessLookupError:
            pass

    try:
        stdout, stderr = process.communicate(timeout=timeout)
        return stdout, stderr, process.returncode, None
    except (subprocess.TimeoutExpired, KeyboardInterrupt) as error:
        reason = "timeout" if isinstance(error, subprocess.TimeoutExpired) else "interrupted"
        send(signal.SIGTERM)
        try:
            stdout, stderr = process.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            send(signal.SIGKILL)
            stdout, stderr = process.communicate()
        finally:
            # A worker may close its captured pipes before exiting. Do not leave
            # that owned process group alive merely because communicate finished.
            send(signal.SIGKILL)
        return stdout, stderr, -1, reason
    except BaseException:
        send(signal.SIGKILL)
        process.wait()
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-dir", type=Path, required=True, help="new directory under repository target/")
    parser.add_argument("--adapter", type=Path, required=True, help="actual candidate shared library, copied and hashed before testing")
    parser.add_argument("--timeout", type=int, default=300, help="per-command deadline in seconds")
    args = parser.parse_args()
    directory = args.run_dir.resolve()
    if not directory.is_relative_to((ROOT / "target").resolve()) or directory.exists() or args.timeout <= 0:
        parser.error("run-dir must be new and inside target/; timeout must be positive")
    directory.mkdir(parents=True)
    (directory / "logs").mkdir()
    (directory / "bin").mkdir()
    receipt = {"schema": 1, "kind": evidence.KIND, "profile": evidence.PROFILE, "generated_utc": utc(),
               "capture_root": str(ROOT),
               "complete": False, "passed": False,
               "oracle": "native assertions and observed process/filesystem state",
               "platform": {"system": platform.system(), "machine": platform.machine(), "release": platform.release()},
               "suites": [], "results": []}
    serial = 0
    environment = {key: value for key, value in os.environ.items() if not key.startswith("ULMDB_")}
    environment["RUST_BACKTRACE"] = "0"
    receipt["environment"] = {"RUST_BACKTRACE": "0", "removed_variable_names": sorted(set(os.environ) - set(environment))}

    def save():
        (directory / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")

    def log(name, text):
        path = directory / "logs" / name
        path.write_text(text)
        return {"path": str(path.relative_to(ROOT)), "bytes": len(text.encode()), "sha256": evidence.text_digest(text), "text": text}

    def run(name, command):
        nonlocal serial
        serial += 1
        print(f"RUN {name}", flush=True)
        start = time.monotonic()
        stdout, stderr, code, cancellation = communicate_owned(command, environment, args.timeout)
        step = {"command": command, "environment": {"RUST_BACKTRACE": "0", "ULMDB_COORDINATION_DYLIB": environment.get("ULMDB_COORDINATION_DYLIB")}, "exit_code": code, "passed": code == 0, "seconds": time.monotonic() - start,
                "stdout": log(f"{serial:03}-{name}.stdout.log", stdout),
                "stderr": log(f"{serial:03}-{name}.stderr.log", stderr)}
        if cancellation:
            step["cancellation"] = cancellation
        receipt.setdefault("steps", []).append(step)
        save()
        evidence.need(code == 0, f"command failed: {name}; see retained logs")
        return step

    try:
        evidence.need(platform.system() == "Linux", "native coordination qualification requires Linux")
        adapter_input = args.adapter.resolve()
        evidence.need(adapter_input.is_file(), "candidate adapter shared library does not exist")
        frozen_adapter = directory / "bin" / "liblmdb-native-qualification.so"
        shutil.copy2(adapter_input, frozen_adapter)
        receipt["adapter_input"] = {"path": str(frozen_adapter.relative_to(ROOT)), "bytes": frozen_adapter.stat().st_size,
                                    "sha256": evidence.digest(frozen_adapter), "provided_path": str(adapter_input)}
        receipt["adapter_policy"] = "process-lifetime RTLD_NODELETE; no dynamic-unload qualification"
        environment["ULMDB_COORDINATION_DYLIB"] = str(frozen_adapter)
        receipt["source"] = evidence.source_snapshot()
        receipt["policy_sha256"] = evidence.digest(evidence.POLICY)
        receipt["revision"] = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True).stdout.strip()
        receipt["toolchain"] = {name: subprocess.run([name, "--version"], cwd=ROOT, check=True, capture_output=True, text=True).stdout.strip()
                                for name in ("rustc", "cargo")}
        policy = evidence.load_policy()
        save()
        for spec in policy["suites"]:
            command = evidence.build_command(spec)
            build = run(spec["id"] + "-build", command)
            artifacts = []
            for line in build["stdout"]["text"].splitlines():
                try:
                    message = json.loads(line)
                except ValueError:
                    continue
                if message.get("reason") == "compiler-artifact" and message.get("target", {}).get("name") == spec.get("artifact_target", spec["target"]) and message.get("executable"):
                    artifacts.append(Path(message["executable"]))
            evidence.need(len(artifacts) == 1, "missing/ambiguous native test executable")
            frozen = directory / "bin" / spec["id"]
            shutil.copy2(artifacts[0], frozen)
            suite = {"id": spec["id"], "source": spec["source"], "source_sha256": evidence.digest(ROOT / spec["source"]),
                     "executable": {"path": str(frozen.relative_to(ROOT)), "bytes": frozen.stat().st_size, "sha256": evidence.digest(frozen)},
                     "build": build}
            suite["list"] = run(spec["id"] + "-list", [str(frozen), "--list", "--format", "terse"])
            receipt["suites"].append(suite)
            save()
            listed = {line[:-6] for line in suite["list"]["stdout"]["text"].splitlines()
                      if line.endswith(": test") and not line[:-6].endswith("_worker")}
            expected = {case["test"] for case in spec["cases"]}
            evidence.need((expected <= listed if spec.get("enumeration") == "selected" else listed == expected),
                          f"{spec['id']}: executable cases differ from reviewed policy")
            for case in spec["cases"]:
                step = run(spec["id"] + "-" + case["test"],
                           [str(frozen), "--exact", case["test"], "--nocapture", "--test-threads=1"])
                for backend in case["backends"]:
                    receipt["results"].append({"suite": spec["id"], "test": case["test"], "backend": backend,
                                               "roles": case["roles"], "source_sha256": suite["source_sha256"],
                                               "executable_sha256": suite["executable"]["sha256"], "run": step})
                save()
        evidence.need(evidence.digest(adapter_input) == receipt["adapter_input"]["sha256"], "provided adapter input changed during run")
        receipt.update(complete=True, passed=True, finished_utc=utc())
        evidence.check_native(receipt, root=ROOT, require_artifacts=True)
        save()
        print(f"PASS: {len(receipt['results'])} scoped native results; {directory / 'receipt.json'}", flush=True)
        return 0
    except Exception as error:
        receipt.update(complete=False, passed=False, error=str(error), finished_utc=utc())
        save()
        print(f"FAIL: {error}; incomplete receipt {directory / 'receipt.json'}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
