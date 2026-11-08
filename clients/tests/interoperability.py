#!/usr/bin/env python3
"""Exercise Rust -> Python -> C -> Node -> Rust on one native ULMDB directory.

Run after building libulmdb_c and the Node addon. Each client closes the
environment before the next client opens it; this does not imply concurrent
multi-process access. No third-party Python or npm package is needed.
"""

from __future__ import annotations

import ctypes as C
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "clients/python"))


def run(command: list[str]) -> None:
    subprocess.run(command, cwd=ROOT, check=True)


def library_path() -> Path:
    configured = os.environ.get("ULMDB_LIBRARY")
    if configured:
        result = Path(configured).resolve()
    else:
        suffix = ".dylib" if sys.platform == "darwin" else ".so"
        target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
        if not target.is_absolute():
            target = ROOT / target
        result = target / "debug" / f"libulmdb_c{suffix}"
    if not result.is_file():
        raise RuntimeError(f"Build the ULMDB C library first; missing {result}")
    return result


def rust(action: str, directory: Path) -> None:
    run(["cargo", "run", "--quiet", "--locked", "-p", "ulmdb-rs", "--example", "interoperability", "--", action, str(directory)])


def python_updates(directory: Path, library: Path) -> None:
    from ulmdb import Environment

    # Rust environments have no persisted LMDB main database. A read-only
    # adapter open exposes an empty default without a catalog write.
    with Environment(directory, readonly=True, library=str(library)) as env:
        with env.begin() as txn:
            main = txn.open_db()
            assert txn.get(main, b"missing") is None

    with Environment(directory, library=str(library)) as env:
        with env.begin() as txn:
            kv = txn.open_db("interop-kv")
            v4 = txn.open_db("interop-v4", kind="ipv4")
            v6 = txn.open_db("interop-v6", kind="ipv6")
            assert txn.get(kv, b"\0rust\xff") == b"rust\0value\xff"
            assert txn.longest_match(v4, "192.0.2.200") == ("192.0.2.0/24", b"rust-v4")
            assert txn.longest_match(v6, "2001:db8:1:2::1") == ("2001:db8::/32", b"rust-v6")
        with env.begin(write=True) as txn:
            txn.put(kv, b"\0python\xfe", b"python\0value\xfe")
            txn.prefix_put(v4, "192.0.2.128/25", b"python-v4")
            txn.prefix_put(v6, "2001:db8:1::/48", b"python-v6")
    print("Python read Rust records and committed binary KV and IPv4/IPv6 prefixes", flush=True)


def c_reads(directory: Path, library: Path) -> None:
    """Call the public C ABI directly, independently of the Python client API."""
    class Value(C.Structure):
        _fields_ = [("size", C.c_size_t), ("data", C.c_void_p)]

    lib = C.CDLL(str(library))
    handle = C.c_void_p
    signatures = {
        "mdb_env_create": ([C.POINTER(handle)], C.c_int),
        "mdb_env_set_maxdbs": ([handle, C.c_uint], C.c_int),
        "mdb_env_open": ([handle, C.c_char_p, C.c_uint, C.c_uint], C.c_int),
        "mdb_env_close": ([handle], None),
        "mdb_txn_begin": ([handle, handle, C.c_uint, C.POINTER(handle)], C.c_int),
        "mdb_txn_abort": ([handle], None),
        "ulmdb_dbi_open": ([handle, C.c_char_p, C.c_uint, C.c_uint, C.POINTER(C.c_uint)], C.c_int),
        "mdb_get": ([handle, C.c_uint, C.POINTER(Value), C.POINTER(Value)], C.c_int),
        "ulmdb_longest_match": ([handle, C.c_uint, C.c_char_p, C.POINTER(Value), C.POINTER(Value)], C.c_int),
    }
    for name, (args, result) in signatures.items():
        function = getattr(lib, name)
        function.argtypes, function.restype = args, result

    def check(code: int) -> None:
        if code:
            raise RuntimeError(f"Direct C ABI interoperability operation failed: {code}")

    def output(value: Value) -> bytes:
        return C.string_at(value.data, value.size)

    env, txn = handle(), handle()
    check(lib.mdb_env_create(C.byref(env)))
    try:
        check(lib.mdb_env_set_maxdbs(env, 8))
        check(lib.mdb_env_open(env, os.fsencode(directory), 0x20000 | 0x200000, 0o600))
        check(lib.mdb_txn_begin(env, None, 0x20000, C.byref(txn)))
        try:
            kv, v4, v6 = C.c_uint(), C.c_uint(), C.c_uint()
            for name, kind, db in [(b"interop-kv", 0, kv), (b"interop-v4", 4, v4), (b"interop-v6", 6, v6)]:
                check(lib.ulmdb_dbi_open(txn, name, kind, 0, C.byref(db)))
            for key, expected in [(b"\0rust\xff", b"rust\0value\xff"), (b"\0python\xfe", b"python\0value\xfe")]:
                storage = C.create_string_buffer(key, len(key))
                key_value = Value(len(key), C.cast(storage, C.c_void_p))
                data = Value()
                check(lib.mdb_get(txn, kv.value, C.byref(key_value), C.byref(data)))
                assert output(data) == expected
            for db, ip, expected_cidr, expected_value in [
                (v4, b"192.0.2.200", b"192.0.2.128/25", b"python-v4"),
                (v6, b"2001:db8:1:2::1", b"2001:db8:1::/48", b"python-v6"),
            ]:
                matched, data = Value(), Value()
                check(lib.ulmdb_longest_match(txn, db.value, ip, C.byref(matched), C.byref(data)))
                assert (output(matched), output(data)) == (expected_cidr, expected_value)
        finally:
            if txn:
                lib.mdb_txn_abort(txn)
    finally:
        lib.mdb_env_close(env)
    print("Direct C ABI read Rust/Python binary KV and longest-prefix results", flush=True)


NODE = r"""
'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { Environment } = require(path.join(process.cwd(), 'clients/node'));
const env = new Environment(process.argv[1]);
try {
  env.read(tx => {
    const kv = tx.openDatabase('interop-kv');
    assert.deepEqual(kv.get(Buffer.from([0, ...Buffer.from('rust'), 255])), Buffer.from([ ...Buffer.from('rust'), 0, ...Buffer.from('value'), 255 ]));
    assert.deepEqual(kv.get(Buffer.from([0, ...Buffer.from('python'), 254])), Buffer.from([ ...Buffer.from('python'), 0, ...Buffer.from('value'), 254 ]));
    assert.deepEqual(tx.openDatabase('interop-v4', {kind: 'ipv4'}).longestMatch('192.0.2.200'), {
      prefix: '192.0.2.128/25', value: Buffer.from('python-v4')
    });
    assert.deepEqual(tx.openDatabase('interop-v6', {kind: 'ipv6'}).longestMatch('2001:db8:1:2::1'), {
      prefix: '2001:db8:1::/48', value: Buffer.from('python-v6')
    });
  });
  env.write(tx => {
    tx.openDatabase('interop-kv').put(Buffer.from([0, ...Buffer.from('node'), 253]), Buffer.from([ ...Buffer.from('node'), 0, ...Buffer.from('value'), 253 ]));
    tx.openDatabase('interop-v4', {kind: 'ipv4'}).putPrefix('192.0.2.192/26', 'node-v4');
    tx.openDatabase('interop-v6', {kind: 'ipv6'}).putPrefix('2001:db8:1:2::/64', 'node-v6');
  });
} finally { env.close(); }
console.log('Node read Python records and committed binary KV and IPv4/IPv6 prefixes');
"""


def main() -> None:
    if not shutil.which("node"):
        raise RuntimeError("Node.js must be available on PATH for the interoperability test")
    library = library_path()
    os.environ["ULMDB_LIBRARY"] = str(library)
    with tempfile.TemporaryDirectory(prefix="ulmdb-client-interop-") as temporary:
        directory = Path(temporary) / "database"
        rust("seed", directory)
        python_updates(directory, library)
        c_reads(directory, library)
        run(["node", "-e", NODE, str(directory)])
        rust("verify", directory)
    print("PASS: Rust, Python, C and Node exchanged persisted records in one ULMDB environment")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
