#!/usr/bin/env python3
"""First-milestone SQL/persistence tests for the isolated B-tree prototype."""
import json
import os
from pathlib import Path
import subprocess
import sqlite3
import sys
import tempfile

shell = str(Path(sys.argv[1]).resolve())
environment = os.environ.copy()
native_backend = "--native" in sys.argv[2:]
blocks_backend = "--blocks" in sys.argv[2:]
assert not blocks_backend or native_backend


def sql(path, source, success=True):
    result = subprocess.run([shell, "-batch", str(path)],
                            input=".bail on\n" + source,
                            text=True, capture_output=True, env=environment)
    if (result.returncode == 0) != success:
        raise AssertionError((result.returncode, source, result.stdout, result.stderr))
    return result.stdout.strip()


with tempfile.TemporaryDirectory(prefix="ulmdb-btree-sql-") as tmp:
    environment["SQLITE_TMPDIR"] = tmp
    db = Path(tmp) / "rows.db"
    actual = sql(db, """
CREATE TABLE kv(id INTEGER PRIMARY KEY, value BLOB NOT NULL, note TEXT);
BEGIN;
INSERT INTO kv VALUES(-9223372036854775808,x'0001','min'),(-7,x'02','negative'),
 (0,x'0300','zero'),(1,x'04','one'),(9223372036854775807,x'ff','max');
COMMIT;
SELECT id||':'||hex(value)||':'||note FROM kv ORDER BY id;
""")
    assert actual.splitlines() == [
        "-9223372036854775808:0001:min", "-7:02:negative", "0:0300:zero",
        "1:04:one", "9223372036854775807:FF:max"], actual
    assert sql(db, "SELECT id FROM kv WHERE id IN (1,-7,1,NULL,999) ORDER BY id;") == "-7\n1"
    assert not list(Path(tmp).glob("dbml.etilqs.*")), "temporary environment was retained"
    assert sql(db, "SELECT id FROM kv WHERE id>=-7 AND id<9223372036854775807 ORDER BY id DESC;") == "1\n0\n-7"
    assert sql(db, "BEGIN; UPDATE kv SET note='discarded' WHERE id=1; ROLLBACK; SELECT note FROM kv WHERE id=1;") == "one"
    assert sql(db, "BEGIN; UPDATE kv SET note='saved' WHERE id=1; SAVEPOINT s; DELETE FROM kv WHERE id=0; ROLLBACK TO s; RELEASE s; COMMIT; SELECT count(*)||':'||max(note='saved') FROM kv;") == "5:1"
    # Unsupported durable record-comparison paths must fail without creating
    # an apparent index or silently changing the comparison order.
    sql(db, "CREATE INDEX bad ON kv(note);", success=False)
    assert sql(db, "SELECT count(*) FROM sqlite_schema WHERE name='bad';") == "0"
    sql(db, "CREATE TABLE bad(a TEXT PRIMARY KEY) WITHOUT ROWID;", success=False)
    assert sql(db, "SELECT count(*) FROM kv;") == "5"
    sql(db, f"ATTACH '{tmp}/second.db' AS second;", success=False)
    assert sql(db, "PRAGMA journal_mode=WAL;") == "off"
    # A separate process opens the same durable rows, including signed limits.
    assert sql(db, "SELECT hex(value) FROM kv WHERE id=-9223372036854775808;") == "0001"
    if native_backend:
        assert db.is_dir()
        expected_format = (b"ULMDB-SQLITE-NATIVE-BLOCKS-01\n" if blocks_backend
                           else b"ULMDB-SQLITE-NATIVE-BTREE-01\n")
        assert (db / "ULMDB-SQLITE-BTREE").read_bytes() == expected_format
        assert (db / "SNAPSHOTLOG").exists()
    else:
        assert db.read_bytes() == b"ULMDB-LMDB-ADAPTER\n"
        assert Path(str(db) + ".ulmdb/CURRENT").exists()
    foreign = Path(tmp) / "ordinary.db"
    native = sqlite3.connect(foreign)
    native.execute("CREATE TABLE preserved(value)")
    native.close()
    prior = foreign.read_bytes()
    sql(foreign, "CREATE TABLE refused(value);", success=False)
    assert foreign.read_bytes() == prior
    assert not Path(str(foreign) + ".ulmdb").exists()
    print(json.dumps({"basic_sql": "pass", "signed_rowids": "pass",
                      "reopen": "pass", "transactions_savepoints": "pass",
                      "unsupported_indexes_attach": "refused"}))
