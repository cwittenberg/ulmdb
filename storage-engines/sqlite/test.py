#!/usr/bin/env python3
"""Focused SQLite SQL-path behavior checks, without changing ULMDB core."""
import pathlib
import sqlite3
import sys
import tempfile
import ctypes
import argparse
import math


def commit_failure(extension):
    with tempfile.TemporaryDirectory(prefix="ulmdb-sqlite-failure-") as directory:
        root = pathlib.Path(directory)
        db = connect(root / "catalog.sqlite", extension)
        db.execute(f"CREATE VIRTUAL TABLE kv USING ulmdb('{root}/store')")
        db.execute("INSERT INTO kv VALUES(1,x'01')")
        ctypes.CDLL(None).ulmdb_test_fail_commit_once()
        try:
            db.commit()
        except sqlite3.OperationalError as error:
            assert "commit" in str(error) and "-30795" in str(error), error
        else:
            raise AssertionError("failed ULMDB commit was reported as SQL success")
        assert db.execute("SELECT id FROM kv WHERE id=1").fetchall() == []
        db.execute("INSERT INTO kv VALUES(2,x'02')")
        db.commit()
        assert db.execute("SELECT id FROM kv WHERE id=2").fetchall() == [(2,)]
        db.close()
    print("SQLite adapter: injected ULMDB commit failure reaches SQL caller and persists no rows")


def connect(catalog, extension, *, check_same_thread=True):
    db = sqlite3.connect(catalog, check_same_thread=check_same_thread)
    db.enable_load_extension(True)
    db.load_extension(str(extension))
    db.enable_load_extension(False)
    return db


def check_in_batches(db, rows, owned=False):
    db.execute("CREATE TEMP TABLE expected_keys(id INTEGER PRIMARY KEY, value BLOB NOT NULL)")
    db.executemany("INSERT INTO expected_keys VALUES(?,?)", rows)
    db.commit()
    affinity_values = [
        -(2**63), 2**63 - 1, float(-(2**63)), float(2**63),
        math.nextafter(float(2**63), 0.0),
        math.nextafter(float(-(2**63)), -math.inf),
        math.nextafter(float(-(2**63)), 0.0),
        -0.0, 5e-324, -5e-324, math.inf, -math.inf, math.nan,
        "3", " 3 ", "3.0", "\t+3e0\n", "03", "-7.0", "0x3",
        "3\x00trailing", "3e0\x00junk", "9223372036854775807",
        "9223372036854775808", "-9223372036854775809",
        "-9223372036854775808", "9223372036854775807.0",
        "-9223372036854775808.0", "+0.0", "1e-999", "-0e999",
        "nan", "NaN", "nonnumeric", b"3", b"", None,
    ]
    def real_min_boundary(value):
        if not isinstance(value, (float, str)):
            return False
        numeric, kind = db.execute(
            "SELECT CAST(:key AS NUMERIC),typeof(CAST(:key AS NUMERIC))",
            {"key": value}).fetchone()
        return kind == "real" and numeric == float(-(2**63))

    for value in affinity_values if owned else ():
        for key_column in ("id", "rowid"):
            query = "SELECT id,value,typeof(:key) FROM {} WHERE {}=:key"
            expected = db.execute(query.format("expected_keys", key_column), {"key": value}).fetchall()
            if real_min_boundary(value):
                # Observed in 3.40.1/3.45.1: indexed-rowid seeks reject
                # REAL(-2^63), while ordinary integer comparison accepts it.
                # Pinned 3.45.1 source: sqlite3VdbeIntegerAffinity deliberately
                # uses ix>SMALLEST_INT64; OP_SeekRowid requires MEM_Int.
                ordinary = db.execute(query.format("expected_keys", f"CAST({key_column} AS INTEGER)"), {"key": value}).fetchall()
                assert ordinary == [(-(2**63), b"minimum", "real" if isinstance(value, float) else "text")]
                if sqlite3.sqlite_version_info in ((3, 40, 1), (3, 45, 1)):
                    assert expected == [], (sqlite3.sqlite_version, expected)
                expected = ordinary
            assert db.execute(query.format("kv", key_column), {"key": value}).fetchall() == expected, repr(value)
    cases = [
        [3, -7, 0, 3, None, -(2**63), 2**63 - 1],
        [3.1, 2.9, -7.1, "0", "3.0", "nonnumeric", b"3", None],
        [float(2**63), str(2**63 - 1), "-9223372036854775808"],
        [None, 101, 102], [None], [],
    ]
    if owned:
        cases.extend([affinity_values + list(reversed(affinity_values)),
                      [float(-(2**63)), None, 3], ["-9223372036854775808.0", None, 3]])
    for keys in cases:
        parameters = ",".join("?" for _ in keys)
        for key_column in ("id", "rowid"):
            for direction in ("ASC", "DESC"):
                query = f"SELECT id,value FROM {{}} WHERE {key_column} IN ({parameters}) ORDER BY id {direction}"
                expected = db.execute(query.format("expected_keys"), keys).fetchall()
                if owned and any(real_min_boundary(value) for value in keys):
                    reference = f"SELECT id,value FROM expected_keys WHERE CAST({key_column} AS INTEGER) IN ({parameters}) ORDER BY id {direction}"
                    expected = db.execute(reference, keys).fetchall()
                assert db.execute(query.format("kv"), keys).fetchall() == expected, keys
    plan = db.execute("EXPLAIN QUERY PLAN SELECT id FROM kv WHERE id IN(3,-7,0) ORDER BY id").fetchall()
    assert any("INDEX 3:in-batch" in row[3] for row in plan), plan
    assert not any("TEMP B-TREE" in row[3] for row in plan), plan
    if owned:
        for predicate in ("id=3", "id IN(3,-7,0)", "rowid IN(3,-7,0)"):
            bytecode = db.execute(f"EXPLAIN SELECT value FROM kv WHERE {predicate}").fetchall()
            assert not any(row[1] == "VColumn" and row[3] == 0 for row in bytecode), bytecode
    query = "SELECT id,value FROM {} WHERE id IN(SELECT id FROM expected_keys WHERE id<=3) ORDER BY id"
    assert db.execute(query.format("kv")).fetchall() == db.execute(query.format("expected_keys")).fetchall()
    query = "SELECT id FROM {} WHERE id IN(3,-7,0) AND id=0"
    assert db.execute(query.format("kv")).fetchall() == [(0,)]
    try:
        db.execute("""WITH RECURSIVE candidates(id) AS (
            VALUES(0) UNION ALL SELECT id+1 FROM candidates WHERE id<65536
        ) SELECT id FROM kv WHERE id IN(SELECT id FROM candidates)""").fetchall()
    except sqlite3.DataError as error:
        assert "65536" in str(error), error
    else:
        raise AssertionError("unbounded IN key batch accepted")
    # Include the keys that the former numeric-prefix lower seek skipped.
    additional = [(1, b"one"), (2, b"two")]
    db.executemany("INSERT INTO kv VALUES(?,?)", additional)
    db.commit()
    db.executemany("INSERT INTO expected_keys VALUES(?,?)", additional)
    db.commit()
    range_values = affinity_values + ["3e-2", " 3e-2 ", "-3e-2", "0.3e1"]
    for value in range_values:
        for key_column in ("id", "rowid"):
            for operator in (">=", ">"):
                query = f"SELECT id,value FROM {{}} WHERE {key_column}{operator}:key ORDER BY id"
                expected = db.execute(query.format("expected_keys"), {"key": value}).fetchall()
                assert db.execute(query.format("kv"), {"key": value}).fetchall() == expected, (key_column, operator, repr(value))
    db.execute("DELETE FROM kv WHERE id IN(1,2)")
    db.commit()
    db.execute("DROP TABLE expected_keys")


def check_ignore_counters(root, extension, module):
    actual = connect(root / "ignore.sqlite", extension)
    reference = sqlite3.connect(":memory:")
    actual.execute(f"CREATE VIRTUAL TABLE kv USING {module}('{root}/ignore-store')")
    reference.execute("CREATE TABLE kv(id INTEGER PRIMARY KEY,value BLOB NOT NULL)")
    statements = [
        "INSERT INTO kv VALUES(7,x'07')",
        "INSERT OR IGNORE INTO kv VALUES(7,x'FF')",
        "INSERT INTO kv VALUES(9,x'09')",
        "UPDATE OR IGNORE kv SET id=7 WHERE id=9",
        "UPDATE kv SET value=x'44' WHERE id=9",
        "UPDATE OR IGNORE kv SET id=11 WHERE id=9",
        "INSERT OR IGNORE INTO kv VALUES(7,x'07'),(12,x'12'),(7,x'FF')",
        "INSERT OR REPLACE INTO kv VALUES(7,x'77')",
        "UPDATE OR IGNORE kv SET id=7 WHERE id=11",
        "UPDATE kv SET value=x'EE' WHERE id=11",
    ]
    for statement in statements:
        expected_cursor = reference.execute(statement)
        actual_cursor = actual.execute(statement)
        assert actual_cursor.rowcount == expected_cursor.rowcount, statement
        reference.commit()
        actual.commit()
        counters = "SELECT changes(),total_changes(),last_insert_rowid()"
        assert actual.execute(counters).fetchone() == reference.execute(counters).fetchone(), statement
        assert actual.execute("SELECT id,value FROM kv ORDER BY id").fetchall() == reference.execute("SELECT id,value FROM kv ORDER BY id").fetchall(), statement
    actual.close()
    reference.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("extension", type=pathlib.Path)
    parser.add_argument("--module", choices=("ulmdb", "ulmdb_owned"), default="ulmdb")
    parser.add_argument("--commit-failure", action="store_true")
    args = parser.parse_args()
    extension = args.extension.resolve()
    if args.commit_failure:
        if args.module != "ulmdb":
            parser.error("the commit fault injector targets the cooperative C library")
        commit_failure(extension)
        return
    with tempfile.TemporaryDirectory(prefix="ulmdb-sqlite-test-") as directory:
        root = pathlib.Path(directory)
        catalog = root / "catalog.sqlite"
        db = connect(catalog, extension)
        db.execute("PRAGMA journal_mode=WAL")
        db.execute(f"CREATE VIRTUAL TABLE kv USING {args.module}('{root}/store')")
        initial = [(-(2**63), b"minimum"), (-7, b"negative"), (0, b""),
                   (3, b"three"), (2**63 - 1, b"maximum")]
        db.executemany("INSERT INTO kv(id,value) VALUES(?,?)", initial)
        db.commit()
        assert db.execute("SELECT id,value FROM kv ORDER BY id").fetchall() == initial
        assert db.execute("SELECT rowid,id FROM kv WHERE id=3").fetchall() == [(3, 3)]
        assert db.execute("SELECT id FROM kv WHERE id>=0 AND id<9 ORDER BY id").fetchall() == [(0,), (3,)]
        assert db.execute("SELECT id FROM kv WHERE id> -7.5 ORDER BY id").fetchall() == [(-7,), (0,), (3,), (2**63 - 1,)]
        assert db.execute("SELECT id FROM kv WHERE id='nonnumeric'").fetchall() == []
        assert db.execute("SELECT id FROM kv WHERE id=NULL").fetchall() == []
        check_in_batches(db, initial, args.module == "ulmdb_owned")
        db.execute("BEGIN")
        db.execute("UPDATE kv SET value=x'AA' WHERE id=3")
        assert db.execute("SELECT value FROM kv WHERE id=3").fetchone() == (b"\xaa",)
        assert db.execute("SELECT id,value FROM kv WHERE id IN(3,-7,3,NULL) ORDER BY id").fetchall() == [(-7, b"negative"), (3, b"\xaa")]
        db.execute("SAVEPOINT outer_save")
        db.execute("INSERT INTO kv VALUES(4,x'04')")
        db.execute("SAVEPOINT inner_save")
        db.execute("DELETE FROM kv WHERE id=-7")
        db.execute("ROLLBACK TO inner_save")
        db.execute("RELEASE inner_save")
        db.execute("ROLLBACK TO outer_save")
        db.execute("RELEASE outer_save")
        assert db.execute("SELECT id FROM kv WHERE id=4").fetchall() == []
        assert db.execute("SELECT id FROM kv WHERE id=-7").fetchall() == [(-7,)]
        db.rollback()
        assert db.execute("SELECT id,value FROM kv ORDER BY id").fetchall() == initial
        db.execute("BEGIN")
        db.execute("INSERT INTO kv VALUES(10,x'10')")
        try:
            db.execute("INSERT INTO kv VALUES(11,x'11'),(3,x'33')")
        except sqlite3.IntegrityError:
            pass
        else:
            raise AssertionError("duplicate key accepted")
        assert db.execute("SELECT id FROM kv WHERE id=11").fetchall() == []
        assert db.execute("SELECT id FROM kv WHERE id=10").fetchall() == [(10,)]
        db.commit()
        db.execute("SAVEPOINT before_first_write")
        db.execute("INSERT INTO kv VALUES(12,x'12')")
        db.execute("ROLLBACK TO before_first_write")
        db.execute("RELEASE before_first_write")
        assert db.execute("SELECT id FROM kv WHERE id=12").fetchall() == []
        db.execute("UPDATE kv SET id=8,value=x'08' WHERE id=3")
        db.commit()
        assert db.execute("SELECT id,value FROM kv WHERE id=8").fetchall() == [(8, b"\x08")]
        assert db.execute("SELECT id FROM kv WHERE id=3").fetchall() == []
        db.execute("INSERT OR IGNORE INTO kv VALUES(8,x'FF')")
        db.execute("INSERT OR REPLACE INTO kv VALUES(8,x'88')")
        db.commit()
        if args.module == "ulmdb_owned":
            # Duplicate-key IGNORE precedes native payload-size admission.
            db.execute("INSERT OR IGNORE INTO kv VALUES(8,zeroblob(16777217))")
            db.commit()
            assert db.execute("SELECT value FROM kv WHERE id=8").fetchone() == (b"\x88",)
            db.execute("UPDATE OR IGNORE kv SET id=8 WHERE id=0")
            db.commit()
            assert db.execute("SELECT value FROM kv WHERE id=0").fetchone() == (b"",)
        reader = connect(catalog, extension)
        scan = reader.execute("SELECT id,value FROM kv ORDER BY id")
        assert scan.fetchone() == initial[0]
        db.execute("UPDATE kv SET value=x'99' WHERE id=8")
        db.commit()
        assert (8, b"\x88") in scan.fetchall()
        assert reader.execute("SELECT value FROM kv WHERE id=8").fetchone() == (b"\x99",)
        batch = reader.execute("SELECT id,value FROM kv WHERE id IN(10,8,-7,0,-9223372036854775808,9223372036854775807) ORDER BY id")
        assert batch.fetchone() == initial[0]
        db.execute("UPDATE kv SET value=x'AB' WHERE id=8")
        db.commit()
        assert (8, b"\x99") in batch.fetchall()
        assert reader.execute("SELECT value FROM kv WHERE id=8").fetchone() == (b"\xab",)
        reader.close()
        expected = db.execute("SELECT id,value FROM kv ORDER BY id").fetchall()
        db.close()
        db = connect(catalog, extension)
        assert db.execute("SELECT id,value FROM kv ORDER BY id").fetchall() == expected
        try:
            db.execute(f"CREATE VIRTUAL TABLE second USING {args.module}('{root}/second')")
        except sqlite3.OperationalError as error:
            assert "one ULMDB" in str(error)
        else:
            raise AssertionError("second attachment accepted")
        db.execute("DROP TABLE kv")
        db.execute(f"CREATE VIRTUAL TABLE kv USING {args.module}('{root}/store')")
        assert db.execute("SELECT id,value FROM kv ORDER BY id").fetchall() == expected
        db.close()
        check_ignore_counters(root, extension, args.module)
    print(f"SQLite {args.module} adapter: SQL CRUD, signed keys, IN batches/types/order/bounds, planner, read snapshots, transactions, nested savepoints, statement rollback, reopen and detach passed")


if __name__ == "__main__":
    main()
