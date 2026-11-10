#!/usr/bin/env python3
"""Focused FDW behavior checks against an isolated, already running server."""
import os
import pathlib
import tempfile
import ctypes
import ctypes.util

import psycopg2
from psycopg2 import sql


def reverse_fixture(path, custom=False):
    class Val(ctypes.Structure):
        _fields_ = [("size", ctypes.c_size_t), ("data", ctypes.c_void_p)]
    library = os.environ.get("ULMDB_PG_TEST_LIBRARY") or ctypes.util.find_library("ulmdb_c")
    if not library:
        library = str(pathlib.Path(__file__).resolve().parents[2] / "target/release/libulmdb_c.so")
    lib = ctypes.CDLL(library)
    pointer = ctypes.c_void_p
    def bind(name, args, result=ctypes.c_int):
        function = getattr(lib, name); function.argtypes = args; function.restype = result
        return function
    create = bind("mdb_env_create", [ctypes.POINTER(pointer)])
    maxdbs = bind("mdb_env_set_maxdbs", [pointer, ctypes.c_uint])
    mapsize = bind("mdb_env_set_mapsize", [pointer, ctypes.c_size_t])
    open_env = bind("mdb_env_open", [pointer, ctypes.c_char_p, ctypes.c_uint, ctypes.c_uint])
    close = bind("mdb_env_close", [pointer], None)
    begin = bind("mdb_txn_begin", [pointer, pointer, ctypes.c_uint, ctypes.POINTER(pointer)])
    abort = bind("mdb_txn_abort", [pointer], None)
    commit = bind("mdb_txn_commit", [pointer])
    open_db = bind("mdb_dbi_open", [pointer, ctypes.c_char_p, ctypes.c_uint, ctypes.POINTER(ctypes.c_uint)])
    put = bind("mdb_put", [pointer, ctypes.c_uint, ctypes.POINTER(Val), ctypes.POINTER(Val), ctypes.c_uint])
    set_compare = bind("mdb_set_compare", [pointer, ctypes.c_uint, pointer])
    @ctypes.CFUNCTYPE(ctypes.c_int, ctypes.POINTER(Val), ctypes.POINTER(Val))
    def descending(left, right):
        left = ctypes.string_at(left.contents.data, left.contents.size)
        right = ctypes.string_at(right.contents.data, right.contents.size)
        return (right > left) - (right < left)
    env, txn, dbi = pointer(), pointer(), ctypes.c_uint()
    path.mkdir(mode=0o700)
    try:
        assert create(ctypes.byref(env)) == 0
        assert maxdbs(env, 128) == 0
        assert mapsize(env, 1 << 30) == 0
        assert open_env(env, os.fsencode(path), 0x200000, 0o600) == 0
        assert begin(env, None, 0, ctypes.byref(txn)) == 0
        assert open_db(txn, b"custom" if custom else b"reverse", 0x40000 | (0 if custom else 2), ctypes.byref(dbi)) == 0
        if custom:
            assert set_compare(txn, dbi, ctypes.cast(descending, pointer)) == 0
        # Reverse-byte ordering emits 256 before1, unlike signed SQL ordering.
        for id in (1, 256):
            key_bytes = ((id ^ (1 << 63)).to_bytes(8, "big"))
            key = ctypes.create_string_buffer(key_bytes); value = ctypes.create_string_buffer(b"external")
            k = Val(8, ctypes.cast(key, pointer)); v = Val(8, ctypes.cast(value, pointer))
            assert put(txn, dbi, ctypes.byref(k), ctypes.byref(v), 0) == 0
        current, txn = txn, pointer()
        assert commit(current) == 0
    finally:
        if txn.value: abort(txn)
        if env.value: close(env)


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
        # Negative DBI admission must not hide creation by the same SQL writer.
        cur.execute("CREATE FOREIGN TABLE ulmdb_empty_kv (id bigint NOT NULL, value bytea NOT NULL) SERVER ulmdb_test OPTIONS (database 'empty_v1')")
        cur.execute("BEGIN")
        cur.execute("SELECT count(*) FROM ulmdb_empty_kv")
        assert cur.fetchone()[0] == 0
        cur.execute("INSERT INTO ulmdb_empty_kv VALUES (1, '\\x01')")
        cur.execute("SELECT value FROM ulmdb_empty_kv WHERE id=1")
        assert bytes(cur.fetchone()[0]) == b"\x01"
        cur.execute("COMMIT")
        cur.execute("INSERT INTO ulmdb_empty_kv VALUES (2,decode(repeat('78',8192),'hex')),(3,'\\x'),(4,decode(repeat('7a',257),'hex'))")
        cur.execute("SELECT array_agg(value ORDER BY id) FROM ulmdb_empty_kv")
        assert [bytes(value) for value in cur.fetchone()[0]] == [b"\x01", b"x" * 8192, b"", b"z" * 257]
        cur.execute("SELECT id,value FROM ulmdb_empty_kv ORDER BY value DESC")
        assert [(id, bytes(value)) for id, value in cur.fetchall()] == [(4, b"z" * 257), (2, b"x" * 8192), (1, b"\x01"), (3, b"")]
        cur.execute("INSERT INTO ulmdb_empty_kv SELECT n,decode(lpad(to_hex(n%256),2,'0'),'hex') FROM generate_series(10,620) n")
        cur.execute("SELECT id,value FROM ulmdb_empty_kv ORDER BY id")
        expected_pages = [(1,b"\x01"),(2,b"x"*8192),(3,b""),(4,b"z"*257)] + [(n,bytes([n%256])) for n in range(10,621)]
        assert [(id,bytes(value)) for id,value in cur.fetchall()] == expected_pages
        if os.environ.get("ULMDB_PG_EXPECT_SQL_BATCH") == "1":
            cur.execute("EXPLAIN (ANALYZE, FORMAT JSON) SELECT id,value FROM ulmdb_empty_kv ORDER BY id")
            assert cur.fetchone()[0][0]["Plan"]["ULMDB batch refills"] >= 3
        if os.environ.get("ULMDB_PG_EXPECT_BATCH_REFUSAL") == "1":
            cur.execute("EXPLAIN (ANALYZE, FORMAT JSON) SELECT id,value FROM ulmdb_empty_kv ORDER BY id")
            refills = cur.fetchone()[0][0]["Plan"].get("ULMDB batch refills", 0)
            assert refills == (1 if os.environ.get("ULMDB_PG_BATCH_REFUSE_REFILL") == "1" else 0)
        cur.execute("PREPARE ulmdb_bulk(bigint[]) AS SELECT id,value FROM ulmdb_empty_kv WHERE id=ANY($1) ORDER BY id")
        cur.execute("SET plan_cache_mode=force_generic_plan")
        # Leading miss-only chunks, duplicate/NULL inputs and a final miss.
        bulk_keys = list(reversed(range(1,621))) + list(range(-300,0)) + [None,1,42,90000]
        cur.execute("EXECUTE ulmdb_bulk(%s)", (bulk_keys,))
        assert [(id,bytes(value)) for id,value in cur.fetchall()] == expected_pages
        for keys in ([], None, [None], [-1,-2,90000]):
            cur.execute("EXECUTE ulmdb_bulk(%s)", (keys,))
            assert cur.fetchall() == []
        cur.execute("EXPLAIN (ANALYZE, FORMAT JSON) EXECUTE ulmdb_bulk(%s)", (list(range(1,621)),))
        lookup_refills = cur.fetchone()[0][0]["Plan"].get("ULMDB batch refills", 0)
        if os.environ.get("ULMDB_PG_EXPECT_LOOKUP_BATCH") == "1":
            assert lookup_refills >= 3
        if os.environ.get("ULMDB_PG_EXPECT_LOOKUP_REFUSAL") == "1":
            assert lookup_refills == (1 if os.environ.get("ULMDB_PG_BATCH_REFUSE_REFILL") == "1" else 0)
        cur.execute("BEGIN")
        cur.execute("UPDATE ulmdb_empty_kv SET value='\\xAB' WHERE id=1")
        cur.execute("EXECUTE ulmdb_bulk(ARRAY[1,1,2,NULL])")
        assert [(id,bytes(value)) for id,value in cur.fetchall()] == [(1,b"\xab"),(2,b"x"*8192)]
        cur.execute("EXPLAIN (ANALYZE, FORMAT JSON) EXECUTE ulmdb_bulk(ARRAY[1,2])")
        assert "ULMDB batch refills" not in cur.fetchone()[0][0]["Plan"]
        cur.execute("ROLLBACK")
        cur.execute("DEALLOCATE ulmdb_bulk")
        cur.execute("RESET plan_cache_mode")
        cur.execute("BEGIN")
        cur.execute("INSERT INTO ulmdb_test_kv VALUES (-9223372036854775808, '\\x00'), (-7, '\\x0102'), (0, '\\x'), (7, '\\xff'), (9223372036854775807, '\\x1234')")
        cur.execute("SELECT id FROM ulmdb_test_kv ORDER BY id")
        assert [x[0] for x in cur.fetchall()] == [-(2**63), -7, 0, 7, 2**63-1]
        cur.execute("COMMIT")
        expected = [-(2**63), -7, 0, 7, 2**63-1]
        def explain(query):
            cur.execute("EXPLAIN (FORMAT JSON) " + query)
            return cur.fetchone()[0][0]["Plan"]

        def nodes(plan):
            yield plan
            for child in plan.get("Plans", []):
                yield from nodes(child)

        # Native full and batch scans already produce signed ascending keys.
        for suffix in ("", " WHERE id IN (7,-7,7,NULL,0,99999)"):
            plan = explain("SELECT id,value FROM ulmdb_test_kv" + suffix + " ORDER BY id")
            assert not any(node["Node Type"] in ("Sort", "Incremental Sort") for node in nodes(plan)), plan
        plan = explain("SELECT id FROM ulmdb_test_kv ORDER BY id DESC")
        assert any(node["Node Type"] == "Sort" for node in nodes(plan)), plan
        cur.execute("SELECT id FROM ulmdb_test_kv ORDER BY id DESC")
        assert [row[0] for row in cur.fetchall()] == list(reversed(expected))
        cur.execute("SELECT id FROM ulmdb_test_kv ORDER BY id NULLS FIRST")
        assert [row[0] for row in cur.fetchall()] == expected
        cur.execute("SELECT array_agg(value ORDER BY id) FROM ulmdb_test_kv")
        assert [bytes(value) for value in cur.fetchone()[0]] == [b"\x00", b"\x01\x02", b"", b"\xff", b"\x12\x34"]
        cur.execute("SELECT q.n,k.id,octet_length(k.value) FROM (VALUES (1),(2)) q(n) CROSS JOIN LATERAL (SELECT id,value FROM ulmdb_test_kv WHERE id>=q.n-100 OFFSET 0) k ORDER BY q.n,k.id")
        assert cur.fetchall() == [(n, id, length) for n in (1, 2) for id, length in ((-7, 2), (0, 0), (7, 1), (2**63-1, 2))]
        cur.execute("SELECT k FROM ulmdb_test_kv k WHERE id=7")
        assert cur.fetchone()[0].startswith("(7,"), "whole-row projection lost value"
        # Only the selected exact predicate may disappear from normal quals.
        plan = explain("SELECT id FROM ulmdb_test_kv WHERE id IN (7,-7,0) AND octet_length(value)>1 ORDER BY id")
        foreign = next(node for node in nodes(plan) if node["Node Type"] == "Foreign Scan")
        assert "octet_length" in foreign["Filter"] and "ANY" not in foreign["Filter"], plan
        cur.execute("SELECT id FROM ulmdb_test_kv WHERE id IN (7,-7,0) AND octet_length(value)>1 ORDER BY id")
        assert cur.fetchall() == [(-7,)]
        cur.execute("SELECT id FROM ulmdb_test_kv WHERE id IN (7,-7,0) AND id IN (-7,99999) ORDER BY id")
        assert cur.fetchall() == [(-7,)]
        for predicate in ("7::smallint=id", "id=7::integer", "id=7::bigint"):
            plan = explain("SELECT value FROM ulmdb_test_kv WHERE " + predicate)
            foreign = next(node for node in nodes(plan) if node["Node Type"] == "Foreign Scan")
            assert "Filter" not in foreign, plan
            cur.execute("SELECT value FROM ulmdb_test_kv WHERE " + predicate)
            assert bytes(cur.fetchone()[0]) == b"\xff"
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
        plan = explain("EXECUTE ulmdb_batch(ARRAY[7,-7]::bigint[])")
        assert not any(node["Node Type"] == "Sort" for node in nodes(plan)), plan
        foreign = next(node for node in nodes(plan) if node["Node Type"] == "Foreign Scan")
        assert "Filter" not in foreign, plan
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
        plan = explain("EXECUTE ulmdb_lookup(7)")
        foreign = next(node for node in nodes(plan) if node["Node Type"] == "Foreign Scan")
        assert "Filter" not in foreign, plan
        cur.execute("DEALLOCATE ulmdb_lookup")
        # Transaction-local metadata follows table/server/descriptor invalidation.
        cur.execute("PREPARE ulmdb_metadata(bigint) AS SELECT value FROM ulmdb_test_kv WHERE id=$1")
        cur.execute("BEGIN")
        cur.execute("EXECUTE ulmdb_metadata(7)")
        assert bytes(cur.fetchone()[0]) == b"\xff"
        cur.execute("ALTER FOREIGN TABLE ulmdb_test_kv OPTIONS (SET database 'empty_v1')")
        cur.execute("EXECUTE ulmdb_metadata(1)")
        assert bytes(cur.fetchone()[0]) == b"\x01"
        cur.execute("ALTER FOREIGN TABLE ulmdb_test_kv OPTIONS (SET database 'sql_kv_v1')")
        cur.execute("EXECUTE ulmdb_metadata(7)")
        assert bytes(cur.fetchone()[0]) == b"\xff"
        # A changed spelling of the same path is valid; another environment is not.
        cur.execute(sql.SQL("ALTER SERVER ulmdb_test OPTIONS (SET path {})").format(sql.Literal(str(root) + "/.")))
        cur.execute("EXECUTE ulmdb_metadata(7)")
        assert bytes(cur.fetchone()[0]) == b"\xff"
        alternate = root / "other-environment"
        alternate.mkdir(mode=0o700)
        changes = (
            ("ALTER FOREIGN TABLE ulmdb_test_kv ALTER COLUMN value DROP NOT NULL", "HV004"),
            (sql.SQL("ALTER SERVER ulmdb_test OPTIONS (SET path {})").format(sql.Literal(str(root / "missing"))), "58P01"),
            (sql.SQL("ALTER SERVER ulmdb_test OPTIONS (SET path {})").format(sql.Literal(str(alternate))), "0A000"),
        )
        for change, expected_code in changes:
            cur.execute("SAVEPOINT metadata_change")
            cur.execute(change)
            try:
                cur.execute("EXECUTE ulmdb_metadata(7)")
                raise AssertionError("stale metadata ignored ALTER OPTIONS/schema")
            except psycopg2.Error as error:
                assert error.pgcode == expected_code, (change, error)
            cur.execute("ROLLBACK TO metadata_change")
            cur.execute("EXECUTE ulmdb_metadata(7)")
            assert bytes(cur.fetchone()[0]) == b"\xff"
            cur.execute("RELEASE metadata_change")
        cur.execute("ROLLBACK")
        cur.execute("DEALLOCATE ulmdb_metadata")
        # Metadata admission must never cache the caller's superuser privilege.
        cur.execute("CREATE ROLE ulmdb_access_test")
        cur.execute("GRANT USAGE ON FOREIGN SERVER ulmdb_test TO ulmdb_access_test")
        cur.execute("GRANT SELECT ON ulmdb_test_kv TO ulmdb_access_test")
        cur.execute("BEGIN")
        cur.execute("SELECT value FROM ulmdb_test_kv WHERE id=7")
        assert bytes(cur.fetchone()[0]) == b"\xff"
        cur.execute("SAVEPOINT access_change")
        cur.execute("SET LOCAL ROLE ulmdb_access_test")
        try:
            cur.execute("SELECT value FROM ulmdb_test_kv WHERE id=7")
            raise AssertionError("metadata cache bypassed superuser access check")
        except psycopg2.errors.InsufficientPrivilege:
            pass
        cur.execute("ROLLBACK TO access_change")
        cur.execute("COMMIT")
        cur.execute("DROP OWNED BY ulmdb_access_test")
        cur.execute("DROP ROLE ulmdb_access_test")
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
        # Incompatible external formats must fail before an ordered row escapes.
        reverse_path = root / "reverse-format"
        reverse_fixture(reverse_path)
        cur.execute(sql.SQL("CREATE SERVER ulmdb_reverse FOREIGN DATA WRAPPER ulmdb_fdw OPTIONS (path {})").format(sql.Literal(str(reverse_path))))
        cur.execute("CREATE FOREIGN TABLE ulmdb_reverse_kv (id bigint NOT NULL,value bytea NOT NULL) SERVER ulmdb_reverse OPTIONS (database 'reverse')")
        for statement in ("SELECT id FROM ulmdb_reverse_kv ORDER BY id", "INSERT INTO ulmdb_reverse_kv VALUES (0,'\\x00')"):
            try:
                cur.execute(statement)
                raise AssertionError("incompatible reverse-key SQL table was accepted")
            except psycopg2.Error as error:
                assert error.pgcode == "HV004" and "incompatible flags 0x2" in str(error), error
        cur.execute("DROP SERVER ulmdb_reverse CASCADE")
        cur.execute("EXPLAIN (FORMAT JSON) SELECT * FROM ulmdb_test_kv")
        native_format_checks = cur.fetchone()[0][0]["Plan"]["ULMDB native format checks"]
        if native_format_checks:
            # flags0 does not imply physical bytewise ordering. Reopening the
            # persisted Custom tree without its callback used to lose exact
            # keys even though generic full scans were correctly reordered.
            custom_path = root / "custom-format"
            reverse_fixture(custom_path, custom=True)
            cur.execute(sql.SQL("CREATE SERVER ulmdb_custom FOREIGN DATA WRAPPER ulmdb_fdw OPTIONS (path {})").format(sql.Literal(str(custom_path))))
            cur.execute("CREATE FOREIGN TABLE ulmdb_custom_kv (id bigint NOT NULL,value bytea NOT NULL) SERVER ulmdb_custom OPTIONS (database 'custom')")
            for statement in (
                "SELECT value FROM ulmdb_custom_kv WHERE id=1",
                "SELECT id,value FROM ulmdb_custom_kv WHERE id=ANY(ARRAY[1,256]::bigint[])",
                "SELECT id FROM ulmdb_custom_kv ORDER BY id",
                "INSERT INTO ulmdb_custom_kv VALUES(128,'\\x00')",
            ):
                try:
                    cur.execute(statement)
                    raise AssertionError("persisted native Custom format was accepted")
                except psycopg2.Error as error:
                    assert error.pgcode == "HV004" and "unsupported native format or comparator" in str(error), error
                    assert "migrate records" in str(error), error
            cur.execute("DROP SERVER ulmdb_custom CASCADE")
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
