#!/bin/sh
set -eu
data=${SQL_DATA_ROOT:-/sql-data}
mkdir -p "$data/socket"
initdb -D "$data/pgdata" -U benchmark --auth-local=trust --auth-host=reject --no-locale > "$data/initdb.log"
cat >> "$data/pgdata/postgresql.conf" <<EOF
listen_addresses = ''
unix_socket_directories = '$data/socket'
fsync = on
synchronous_commit = on
full_page_writes = on
shared_buffers = '128MB'
max_prepared_transactions = 4
EOF
pg_ctl -D "$data/pgdata" -l "$data/postgres.log" -w start
trap 'pg_ctl -D "$data/pgdata" -m fast -w stop' EXIT INT TERM
export PGHOST="$data/socket" PGUSER=benchmark PGDATABASE=postgres
python3 /src/storage-engines/sqlite/test.py /src/storage-engines/sqlite/ulmdb_sqlite.so
LD_PRELOAD=/src/storage-engines/sqlite/test_commit_failure.so \
  ULMDB_SQLITE_TEST_LIBRARY=/usr/local/lib/libulmdb_c.so \
  python3 /src/storage-engines/sqlite/test.py /src/storage-engines/sqlite/ulmdb_sqlite.so --commit-failure
python3 /src/storage-engines/sqlite/test.py /src/storage-engines/sqlite/ulmdb_sqlite_owned.so --module ulmdb_owned
python3 /src/storage-engines/sqlite/test_owned.py /src/storage-engines/sqlite/ulmdb_sqlite_owned.so
python3 /src/storage-engines/sqlite/test_owned_commit.py /src/storage-engines/sqlite/ulmdb_sqlite_owned.so
python3 /src/storage-engines/postgresql/test.py
python3 /src/storage-engines/test_interop.py \
  --sqlite-extension /src/storage-engines/sqlite/ulmdb_sqlite.so --data-root "$data/interop"
