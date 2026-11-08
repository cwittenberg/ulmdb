#!/usr/bin/env python3
"""Compile every pinned header export and exercise explicit packaged ABI selection."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
PROBE = r'''
#include <lmdb.h>
#include <assert.h>
#include <errno.h>
#include <stdlib.h>
#include <string.h>
_Static_assert(MDB_VERSION_MAJOR == TEST_MAJOR, "header major");
_Static_assert(MDB_RESERVE == 0x10000, "reserve flag");
int main(int argc, char **argv) {
    assert(argc == 3);
    int modern = !strcmp(argv[2], "1");
    int major, minor, patch;
    assert(mdb_version(&major, &minor, &patch));
    const char *role = getenv("ULMDB_LMDB_ABI");
    int global_modern = !strcmp(role, "1.0.2");
    assert(major == global_modern && minor == (global_modern ? 0 : 9));
    assert(patch == (global_modern ? 2 : !strcmp(role, "0.9.36") ? 36 : 31));
    MDB_env *env;
    MDB_txn *parent, *child, *reader;
    MDB_dbi dbi;
    assert(!mdb_env_create(&env));
    assert(!mdb_env_set_maxdbs(env, 4));
    assert(!mdb_env_open(env, argv[1], MDB_NOTLS, 0600));
    assert(!mdb_txn_begin(env, NULL, 0, &parent));
    int rc = mdb_txn_begin(env, parent, MDB_RDONLY, &child);
    assert(rc == (modern ? 0 : MDB_BAD_TXN));
    if (!rc) mdb_txn_abort(child);
    assert(!mdb_dbi_open(parent, "alpha", MDB_CREATE, &dbi));
    assert(!mdb_txn_commit(parent));
    assert(!mdb_txn_begin(env, NULL, MDB_RDONLY, &reader));
#if TEST_MAJOR == 1
    unsigned flags;
    assert(!mdb_txn_flags(reader, &flags));
    assert(flags & MDB_RDONLY);
    assert(strstr(mdb_strerror(MDB_SHORT_WRITE), "MDB_SHORT_WRITE"));
    assert(strstr(mdb_strerror(MDB_ENV_BUSY), "MDB_ENV_BUSY"));
    assert(strstr(mdb_strerror(MDB_PROBLEM), "MDB_PROBLEM"));
#endif
    MDB_cursor *cursor;
    MDB_val key, value;
    assert(!mdb_cursor_open(reader, 1, &cursor));
    assert(!mdb_cursor_get(cursor, &key, &value, MDB_FIRST));
    assert(key.mv_size == (modern ? 6 : 5));
    assert(!memcmp(key.mv_data, "alpha", 5));
    if (modern) assert(!((char *)key.mv_data)[5]);
    mdb_cursor_close(cursor);
    mdb_txn_abort(reader);
    mdb_env_close(env);
    return 0;
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, default=ROOT / "target/lmdb-ulmdb")
    args = parser.parse_args()
    package = args.package.resolve()
    compiler = shlex.split(os.environ.get("CC", "cc"))
    launcher = package / "bin/convert-lmdb-to-ulmdb"
    env = dict(os.environ, LD_LIBRARY_PATH=str(package / "lib"))

    def run(arguments):
        result = subprocess.run([str(x) for x in arguments], env=env,
                                timeout=30, capture_output=True)
        assert result.returncode == 0, (result.args, result.returncode, result.stderr.decode(errors="replace"))
        return result

    with tempfile.TemporaryDirectory(prefix="ulmdb-abi-profiles-") as tmp:
        work = Path(tmp)
        common = None
        for major, abi, count in (("0.9", "0.9.31", 56), ("1.0", "1.0.2", 69)):
            include = package / "include" / ("lmdb-" + major)
            header = (include / "lmdb.h").read_bytes()
            pins = json.loads((HERE / ("legacy-source.json" if major == "0.9" else "modern-source.json")).read_text())
            pin = next(row for row in pins["files"] if row["path"] == "lmdb.h")
            assert len(header) == pin["bytes"] and hashlib.sha256(header).hexdigest() == pin["sha256"]
            clean = run(compiler + ["-E", "-P", "-x", "c", include / "lmdb.h"]).stdout.decode()
            symbols = sorted(set(re.findall(r"\b(mdb_[a-zA-Z0-9_]+)\s*\(", clean)))
            assert len(symbols) == count, (major, symbols)
            source = work / (major + ".c")
            # Typed volatile references require real relocations for every
            # declaration, including modern crypto and incremental functions.
            source.write_text("#include <lmdb.h>\n" + "\n".join(
                f"static __typeof__(&{name}) volatile p_{name} = &{name};" for name in symbols
            ) + "\nint main(void) { return " + " || ".join(f"!p_{name}" for name in symbols) + "; }\n")
            executable = work / major
            flags = compiler + ["-std=c11", "-Wall", "-Wextra", "-Werror", "-I", include]
            link = ["-L", package / "lib", "-llmdb"]
            run(flags + [source] + link + ["-o", executable])
            run([executable])
            abi_probe = work / (major + "-layout")
            run(flags + [HERE / "tests/parity_abi.c"] + link + ["-o", abi_probe])
            layout = run([abi_probe]).stdout
            assert common is None or layout == common
            common = layout
            probe = work / (major + "-profile.c")
            probe.write_text(PROBE)
            application = work / (major + "-profile")
            run(flags + ["-DTEST_MAJOR=" + major[0], probe] + link + ["-o", application])
            profiles = ("0.9.31", "0.9.36") if major == "0.9" else ("1.0.2",)
            for profile in profiles:
                destination = work / ("store-" + profile)
                destination.mkdir()
                # Explicit launcher selection wins over inherited settings.
                env["ULMDB_LMDB_ABI"] = "0.9.31" if major == "1.0" else "1.0.2"
                run([launcher, "--abi", profile, application, destination, str(int(major == "1.0"))])
                run([launcher, "--abi", profile, application, destination, str(int(major == "1.0"))])
                # Imported environments already carry this advisory marker.
                # Native-created environments need an explicit request on each
                # open unless their operator has installed the known ABI marker.
                (destination / "LMDB-ABI").write_text(profile + "\n")
                opposite = "0.9.31" if major == "1.0" else "1.0.2"
                run([launcher, "--abi", opposite, application, destination, str(int(major == "1.0"))])
            if major == "1.0":
                modern_source = work / "modern-layout.c"
                modern_source.write_text('#include <lmdb.h>\n#include <stddef.h>\n#include <stdio.h>\nint main(void) {\n' +
                    '\n'.join(f'printf("{field} %zu\\n", offsetof(MDB_crypto_funcs, {field}));' for field in
                              ('mcf_str2key', 'mcf_encfunc', 'mcf_sumfunc', 'mcf_keysize', 'mcf_esumsize', 'mcf_sumsize')) +
                    '\nprintf("crypto %zu %zu\\n", sizeof(MDB_crypto_funcs), _Alignof(MDB_crypto_funcs));\n' +
                    '\n'.join(f'printf("{name} %ld\\n", (long){name});' for name in
                              ('MDB_ENCRYPT', 'MDB_PREVSNAPSHOT', 'MDB_REMAP_CHUNKS', 'MDB_PROBLEM', 'MDB_BAD_CHECKSUM',
                               'MDB_CRYPTO_FAIL', 'MDB_ENV_ENCRYPTION', 'MDB_TXN_PENDING', 'MDB_CANT_ROLLBACK',
                               'MDB_DBIS_BUSY', 'MDB_SHORT_WRITE', 'MDB_ENV_BUSY', 'MDB_IS_READONLY', 'MDB_ADDR_BUSY')) +
                    '\nreturn 0; }\n')
                modern_probe = work / "modern-layout"
                run(flags + [modern_source] + link + ['-o', modern_probe])
                modern_layout = run([modern_probe]).stdout
                adapter_flags = compiler + ['-std=c11', '-Wall', '-Wextra', '-Werror', '-I', package / 'include']
                run(adapter_flags + [modern_source] + link + ['-o', modern_probe])
                assert run([modern_probe]).stdout == modern_layout
                run(adapter_flags + [HERE / 'tests/parity_abi.c'] + link + ['-o', abi_probe])
                assert run([abi_probe]).stdout == common
                # Genuine upstream VL32 declarations rename env_create. This
                # unimplemented ABI must fail to link instead of binding usize.
                bad = subprocess.run([str(x) for x in flags + ["-DMDB_VL32", source] + link + ["-o", work / "vl32"]],
                                     env=env, timeout=30, capture_output=True)
                assert bad.returncode and b"mdb_env_create_vl32" in bad.stderr
            print(f"{abi}: unchanged pinned header, all{count} exports, layout/profile/reopen: PASS")
        for arguments in (("--abi",), ("--abi", "1.0.2"), ("--abi", "unsupported", "/bin/true")):
            invalid = subprocess.run([str(launcher), *arguments], env=env, timeout=5, capture_output=True)
            assert invalid.returncode == 2
    print("explicit ABI headers/launcher and incompatible-profile controls: PASS")


if __name__ == "__main__":
    main()
