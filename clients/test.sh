#!/usr/bin/env bash
# Run every client against the same local ULMDB engine. Needs Node headers and
# the pinned LMDB sources (see clients/c/README.md), and Docker for PHP.
set -euo pipefail
cd "$(dirname "$0")/.."
export PYTHONDONTWRITEBYTECODE=1
cargo build -p ulmdb-c --locked
cargo test -p ulmdb-rs -p ulmdb-c --locked
cargo test -p ulmdb --locked --test client_snapshot --test client_cursor
cargo test -p ulmdb --locked --test client_export
cargo test -p ulmdb --locked --test incremental --test page_geometry --test ordering --test value_reservation --test reservation_ordering --test transforms
cargo test -p ulmdb --locked --test physical_stats
cargo test -p ulmdb --locked --test write_map --test no_mem_init --test fixed_map --test fixed_checksum --test fixed_layout_policy --test fixed_required --test checksum_async --test chunked_read --test cached_file_capture --test catalog_capacity --test jumbo_values
cargo test -p ulmdb-storage --locked --test chunk_cache
cargo test -p ulmdb --locked --test resident_savepoints --test resident_duplicates --test resident_overflow --test resident_duplicate_overflow --test resident_export --test resident_readonly_capture --test resident_checkpoint --test resident_transforms --test resident_ordering
cargo test -p ulmdb --locked --test automatic_maintenance --test resumable_reclaim --test file_access --test external_synchronization --test external_copy --test external_incremental --test external_previous
python3 clients/c/tests/compare.py --no-build
python3 clients/c/tests/test_initialization.py
python3 clients/c/tests/multiprocess.py
python3 clients/c/tests/readers.py
python3 clients/c/tests/map_size.py
python3 clients/c/tests/streaming.py
make -C clients/lmdb-ulmdb package ARGS='--no-cargo'
python3 clients/lmdb-ulmdb/test_abi_profiles.py --package target/lmdb-ulmdb
python3 clients/c/tests/extensions.py --package target/lmdb-ulmdb
python3 clients/lmdb-ulmdb/test_operator.py --package target/lmdb-ulmdb
python3 clients/lmdb-ulmdb/test_operator_large.py --package target/lmdb-ulmdb
python3 clients/c/tests/read_ahead.py --package target/lmdb-ulmdb
make -C clients/lmdb-ulmdb test
cmake -S clients/cpp -B target/ulmdb-cpp -DULMDB_C_LIBRARY="$PWD/target/debug/libulmdb_c.so"
cmake --build target/ulmdb-cpp
ctest --test-dir target/ulmdb-cpp --output-on-failure
PYTHONPATH=clients/python python3 -m unittest discover -s clients/python/tests -v
npm --prefix clients/node run build
npm --prefix clients/node test
./clients/php/test-docker.sh
python3 clients/tests/interoperability.py
