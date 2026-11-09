#!/usr/bin/env python3
"""Focused FDW behavior checks against an isolated, already running server."""
import os
import pathlib
import tempfile

import psycopg2
from psycopg2 import sql


def run():
    conn = psycopg2.connect("")
    conn.autocommit = True
    root = pathlib.Path(tempfile.mkdtemp(prefix="ulmdb-pg-", dir=os.environ.get("ULMDB_PG_TEST_ROOT", "/tmp")))
    # The test runner and PostgreSQL server must share this local filesystem.
    # Run as the service's OS account: ULMDB enforces environment ownership.
    root.chmod(0o700)
    with conn.cursor() as cur:
        cur.execute("CREATE EXTENSION IF NOT EXISTS ulmdb_fdw")
        cur.execute("DROP SERVER IF EXISTS ulmdb_test CASCADE")
        cur.execute(sql.SQL("CREATE SERVER ulmdb_test FOREIGN DATA WRAPPER ulmdb_fdw OPTIONS (path {})").format(sql.Literal(str(root))))
        cur.execute("CREATE FOREIGN TABLE ulmdb_test_kv (id bigint NOT NULL, value bytea NOT NULL) SERVER ulmdb_test OPTIONS (database 'sql_kv_v1')")
        cur.execute("SELECT count(*) FROM ulmdb_test_kv")
        assert cur.fetchone()[0] == 0
        cur.execute("BEGIN")
        cur.execute("INSERT INTO ulmdb_test_kv VALUES (-9223372036854775808, '\\x00'), (-7, '\\x0102'), (0, '\\x'), (7, '\\xff'), (9223372036854775807, '\\x1234')")
        cur.execute("SELECT id FROM ulmdb_test_kv ORDER BY id")
        assert [x[0] for x in cur.fetchall()] == [-(2**63), -7, 0, 7, 2**63-1]
        cur.execute("COMMIT")
        expected = [-(2**63), -7, 0, 7, 2**63-1]
        cur.execute("EXPLAIN SELECT id FROM ulmdb_test_kv WHERE id IN (7,-7,7,0)")
        assert "primary-key batch" in "\n".join(x[0] for x in cur.fetchall())
        cur.execute("SELECT id FROM ulmdb_test_kv WHERE id IN (7,-7,7,NULL,0,99999) ORDER BY id")
        assert [row[0] for row in cur.fetchall()] == [-7, 0, 7]
        cur.execute("PREPARE ulmdb_batch(bigint[]) AS SELECT id FROM ulmdb_test_kv WHERE id=ANY($1) ORDER BY id")
        cur.execute("SET plan_cache_mode=force_generic_plan")
        for candidates, wanted in (
            ([2**63-1, 7, None, -(2**63), -7, 0, 7, 99999], expected),
            ([], []),
            (None, []),
            ([None, None, 99999], []),
        ):
            cur.execute("EXECUTE ulmdb_batch(%s)", (candidates,))
            assert [row[0] for row in cur.fetchall()] == wanted
        cur.execute("EXPLAIN EXECUTE ulmdb_batch(ARRAY[7,-7]::bigint[])")
        assert "primary-key batch" in "\n".join(x[0] for x in cur.fetchall())
        cur.execute("DEALLOCATE ulmdb_batch")
        for array_type in ("smallint", "integer", "bigint"):
            cur.execute(f"SELECT id FROM ulmdb_test_kv WHERE id=ANY(ARRAY[7,-7,NULL,7]::{array_type}[]) ORDER BY id")
            assert cur.fetchall() == [(-7,), (7,)]
        placeholders = ",".join("bigint" for _ in range(100))
        parameters = ",".join(f"${index}" for index in range(1, 101))
        cur.execute(f"PREPARE ulmdb_in100({placeholders}) AS SELECT id FROM ulmdb_test_kv WHERE id IN ({parameters}) ORDER BY id")
        cur.execute("EXECUTE ulmdb_in100(" + ",".join(["7", "-7", "NULL", "7"] + ["99999"] * 96) + ")")
        assert cur.fetchall() == [(-7,), (7,)]
        cur.execute("EXPLAIN EXECUTE ulmdb_in100(" + ",".join(["7"] * 100) + ")")
        assert "primary-key batch" in "\n".join(x[0] for x in cur.fetchall())
        cur.execute("DEALLOCATE ulmdb_in100")
        cur.execute("EXPLAIN SELECT value FROM ulmdb_test_kv WHERE id=7")
        assert "primary-key lookup" in "\n".join(x[0] for x in cur.fetchall())
        cur.execute("PREPARE ulmdb_lookup(bigint) AS SELECT value FROM ulmdb_test_kv WHERE id=$1")
        cur.execute("SET plan_cache_mode=force_generic_plan")
        cur.execute("EXECUTE ulmdb_lookup(7)")
        assert bytes(cur.fetchone()[0]) == b"\xff"
        cur.execute("EXECUTE ulmdb_lookup(NULL)")
        assert cur.fetchall() == []
        cur.execute("DEALLOCATE ulmdb_lookup")
        # Completed scans must not accumulate wrappers in the transaction context.
        cur.execute("PREPARE ulmdb_repeat AS SELECT value FROM ulmdb_test_kv WHERE id=7")
        cur.execute("BEGIN")
        cur.execute("EXECUTE ulmdb_repeat")
        assert bytes(cur.fetchone()[0]) == b"\xff"
        cur.execute("SELECT used_bytes FROM pg_backend_memory_contexts WHERE name='TopTransactionContext'")
        memory_before = cur.fetchone()[0]
        for _ in range(2048):
            cur.execute("EXECUTE ulmdb_repeat")
            assert bytes(cur.fetchone()[0]) == b"\xff"
        cur.execute("SELECT used_bytes FROM pg_backend_memory_contexts WHERE name='TopTransactionContext'")
        assert cur.fetchone()[0] - memory_before <= 65536
        cur.execute("COMMIT")
        cur.execute("DEALLOCATE ulmdb_repeat")
        # Other backends may publish while this transaction retains its reader.
        other = psycopg2.connect("")
        other.autocommit = True
        with other.cursor() as remote:
            cur.execute("BEGIN")
            cur.execute("SELECT value FROM ulmdb_test_kv WHERE id=7")
            assert bytes(cur.fetchone()[0]) == b"\xff"
            remote.execute("UPDATE ulmdb_test_kv SET value='\\xaa' WHERE id=7")
            cur.execute("SELECT value FROM ulmdb_test_kv WHERE id=7")
            assert bytes(cur.fetchone()[0]) == b"\xff"
            cur.execute("COMMIT")
            cur.execute("SELECT value FROM ulmdb_test_kv WHERE id=7")
            assert bytes(cur.fetchone()[0]) == b"\xaa"
            cur.execute("BEGIN")
            cur.execute("SELECT value FROM ulmdb_test_kv WHERE id=7")
            assert bytes(cur.fetchone()[0]) == b"\xaa"
            remote.execute("UPDATE ulmdb_test_kv SET value='\\xbb' WHERE id=7")
            try:
                cur.execute("UPDATE ulmdb_test_kv SET value='\\xcc' WHERE id=7")
                raise AssertionError("stale read-to-write upgrade was accepted")
            except psycopg2.errors.SerializationFailure:
                pass
            cur.execute("ROLLBACK")
            cur.execute("SELECT value FROM ulmdb_test_kv WHERE id=7")
            assert bytes(cur.fetchone()[0]) == b"\xbb"
            remote.execute("UPDATE ulmdb_test_kv SET value='\\xff' WHERE id=7")
        other.close()
        cur.execute("BEGIN")
        cur.execute("SELECT value FROM ulmdb_test_kv WHERE id=7")
        assert bytes(cur.fetchone()[0]) == b"\xff"
        cur.execute("UPDATE ulmdb_test_kv SET value='\\xab' WHERE id=7 RETURNING id, value")
        assert (lambda row: (row[0], bytes(row[1])))(cur.fetchone()) == (7, b"\xab")
        cur.execute("SELECT value FROM ulmdb_test_kv WHERE id=7")
        assert bytes(cur.fetchone()[0]) == b"\xab"
        cur.execute("ROLLBACK")
        cur.execute("SELECT value FROM ulmdb_test_kv WHERE id=7")
        assert bytes(cur.fetchone()[0]) == b"\xff"
        for statement in (
            "UPDATE ulmdb_test_kv SET id=8 WHERE id=7",
            "SELECT * FROM ulmdb_test_kv FOR UPDATE",
            "INSERT INTO ulmdb_test_kv VALUES (7, '\\x00') ON CONFLICT DO NOTHING",
        ):
            try:
                cur.execute(statement)
                raise AssertionError(f"unsupported operation accepted: {statement}")
            except psycopg2.errors.FeatureNotSupported:
                pass
        cur.execute("BEGIN")
        cur.execute("INSERT INTO ulmdb_test_kv VALUES (99, '\\x99')")
        try:
            cur.execute("PREPARE TRANSACTION 'ulmdb_test_prepared'")
            raise AssertionError("prepared transaction accepted")
        except psycopg2.errors.FeatureNotSupported:
            pass
        cur.execute("ROLLBACK")
        cur.execute("SELECT * FROM ulmdb_test_kv WHERE id=99")
        assert cur.fetchall() == []
        cur.execute("BEGIN READ ONLY")
        try:
            cur.execute("INSERT INTO ulmdb_test_kv VALUES (99, '\\x99')")
            raise AssertionError("read-only write accepted")
        except psycopg2.errors.ReadOnlySqlTransaction:
            pass
        cur.execute("ROLLBACK")
        cur.execute("BEGIN")
        cur.execute("INSERT INTO ulmdb_test_kv VALUES (11, '\\x11')")
        cur.execute("SAVEPOINT nested")
        try:
            cur.execute("INSERT INTO ulmdb_test_kv VALUES (12, '\\x12')")
            raise AssertionError("nested write was accepted")
        except psycopg2.errors.FeatureNotSupported:
            pass
        cur.execute("ROLLBACK TO nested")
        cur.execute("COMMIT")
        cur.execute("SELECT id FROM ulmdb_test_kv WHERE id IN (11,12) ORDER BY id")
        assert cur.fetchall() == [(11,)]
        try:
            cur.execute("INSERT INTO ulmdb_test_kv VALUES (7, '\\x00')")
            raise AssertionError("duplicate key was accepted")
        except psycopg2.errors.UniqueViolation:
            pass
        cur.execute("SELECT value FROM ulmdb_test_kv WHERE id=7")
        assert bytes(cur.fetchone()[0]) == b"\xff"
        cur.execute("DELETE FROM ulmdb_test_kv WHERE id=-7 RETURNING id,value")
        row = cur.fetchone()
        assert row[0] == -7 and bytes(row[1]) == b"\x01\x02"
        cur.execute("UPDATE ulmdb_test_kv SET value='\\xcd'")
        assert cur.rowcount == 5
        cur.execute("SELECT count(*) FROM ulmdb_test_kv WHERE value='\\xcd'")
        assert cur.fetchone()[0] == 5
        cur.execute("DELETE FROM ulmdb_test_kv")
        assert cur.rowcount == 5
        cur.execute("SELECT count(*) FROM ulmdb_test_kv")
        assert cur.fetchone()[0] == 0
        cur.execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
        try:
            cur.execute("SELECT * FROM ulmdb_test_kv")
            raise AssertionError("unsupported isolation accepted")
        except psycopg2.errors.FeatureNotSupported:
            pass
        cur.execute("ROLLBACK")
        cur.execute("DROP SERVER ulmdb_test CASCADE")
    conn.close()
    print("PostgreSQL FDW: transactions, signed keys, point plans, parameters, mutations and limits passed")


if __name__ == "__main__":
    run()
