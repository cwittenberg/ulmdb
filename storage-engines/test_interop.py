#!/usr/bin/env python3
"""Exercise the shared row format through the two real SQL adapters, sequentially."""
import argparse
from pathlib import Path
import sqlite3

import psycopg2
from psycopg2 import sql


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sqlite-extension", type=Path, required=True)
    parser.add_argument("--data-root", type=Path, required=True)
    args = parser.parse_args()
    args.data_root.mkdir(parents=True)
    env = args.data_root / "shared"
    env.mkdir()
    metadata = args.data_root / "metadata.sqlite"
    expected = {-(2**63): b"minimum", -1: b"\x00\xff", 0: b"", 2**63 - 1: b"maximum"}

    def sqlite_open():
        conn = sqlite3.connect(metadata)
        conn.enable_load_extension(True)
        conn.load_extension(str(args.sqlite_extension))
        conn.enable_load_extension(False)
        return conn

    conn = sqlite_open()
    path = str(env).replace("'", "''")
    conn.execute(f"CREATE VIRTUAL TABLE kv USING ulmdb('{path}')")
    conn.executemany("INSERT INTO kv(id,value) VALUES(?,?)", expected.items())
    conn.commit()
    conn.close()

    pg = psycopg2.connect("")
    pg.autocommit = True
    cur = pg.cursor()
    cur.execute("CREATE EXTENSION IF NOT EXISTS ulmdb_fdw")
    cur.execute(sql.SQL("CREATE SERVER ulmdb_interop FOREIGN DATA WRAPPER ulmdb_fdw OPTIONS(path {})").format(sql.Literal(str(env))))
    cur.execute("CREATE FOREIGN TABLE ulmdb_interop_kv(id bigint NOT NULL,value bytea NOT NULL) SERVER ulmdb_interop OPTIONS(database 'sql_kv_v1')")
    cur.execute("SELECT id,value FROM ulmdb_interop_kv ORDER BY id")
    assert [(k, bytes(v)) for k, v in cur.fetchall()] == sorted(expected.items())
    cur.execute("INSERT INTO ulmdb_interop_kv VALUES(%s,%s)", (-5, b"from-postgresql"))
    expected[-5] = b"from-postgresql"
    cur.execute("BEGIN")
    cur.execute("UPDATE ulmdb_interop_kv SET value=%s WHERE id=%s", (b"aborted", 0))
    cur.execute("ROLLBACK")
    cur.close()
    pg.close()

    conn = sqlite_open()
    assert conn.execute("SELECT id,value FROM kv ORDER BY id").fetchall() == sorted(expected.items())
    conn.execute("UPDATE kv SET value=? WHERE id=?", (b"from-sqlite", -5))
    conn.commit()
    conn.close()

    # A new backend sees the committed SQLite change, including the binary format.
    pg = psycopg2.connect("")
    pg.autocommit = True
    cur = pg.cursor()
    cur.execute("SELECT value FROM ulmdb_interop_kv WHERE id=-5")
    assert bytes(cur.fetchone()[0]) == b"from-sqlite"
    cur.execute("DROP FOREIGN TABLE ulmdb_interop_kv")
    cur.execute("DROP SERVER ulmdb_interop")
    cur.close()
    pg.close()
    print("SQLite/PostgreSQL shared-format interoperability: passed", flush=True)


if __name__ == "__main__":
    main()
