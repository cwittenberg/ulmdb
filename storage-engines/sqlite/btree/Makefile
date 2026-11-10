ULMDB_ROOT ?= ../../..
ULMDB_LIBDIR ?= $(abspath $(ULMDB_ROOT)/target/release)
SQLITE_SRC ?= $(abspath $(ULMDB_ROOT)/benchmark/sql-storage-engines/sqlite-3.54.1/sqlite-amalgamation-3540100)
BUILD ?= $(abspath $(ULMDB_ROOT)/target/sqlite-btree)
CC ?= cc
CFLAGS ?= -O3 -g
ULMDB_BTREE_NATIVE_FEATURES ?= btree
ULMDB_BTREE_NATIVE_TEST_FLAGS ?=
ULMDB_BTREE_NATIVE_TARGET ?= $(abspath $(ULMDB_ROOT)/target/sqlite-btree-native)
ULMDB_BTREE_NATIVE_BUILD := 0
ifeq ($(origin ULMDB_BTREE_NATIVE_LIB), undefined)
ULMDB_BTREE_NATIVE_LIB = $(ULMDB_BTREE_NATIVE_TARGET)/release/libulmdb_sqlite_native.a
ULMDB_BTREE_NATIVE_BUILD := 1
endif
PORT_FLAGS = -DSQLITE_OMIT_SHARED_CACHE -DSQLITE_OMIT_AUTOVACUUM -DSQLITE_OMIT_WAL \
  -DSQLITE_OMIT_DESERIALIZE -DSQLITE_THREADSAFE=1 -DLUMO_LMDB_FIXED_ROWID

.PHONY: all native native-prepare test test-native test-native-read-hash test-native-blocks test-writer-cache clean
all: $(BUILD)/sqlite3-ulmdb $(BUILD)/libsqlite3_ulmdb.so

native: $(BUILD)/sqlite3-ulmdb-native $(BUILD)/libsqlite3_ulmdb_native.so

native-prepare:
ifeq ($(ULMDB_BTREE_NATIVE_BUILD),1)
	cargo build --release --locked --manifest-path ../native/Cargo.toml --features $(ULMDB_BTREE_NATIVE_FEATURES) --target-dir $(ULMDB_BTREE_NATIVE_TARGET)
else
	test -f $(ULMDB_BTREE_NATIVE_LIB)
endif

$(BUILD)/sqlite3-ulmdb.c: generate.py unsupported.c read_paths.c read_holds.c writer_carrier.c writer_cache.c lumosql/btree.c lumosql/pager.c lumosql/ephemeral.c lumosql/provenance.json $(SQLITE_SRC)/sqlite3.c
	python3 generate.py $(SQLITE_SRC)/sqlite3.c $@

$(BUILD)/sqlite3-ulmdb.o: $(BUILD)/sqlite3-ulmdb.c
	$(CC) $(CPPFLAGS) $(CFLAGS) -fPIC $(PORT_FLAGS) -I$(ULMDB_ROOT)/clients/c/include -I$(SQLITE_SRC) -c $< -o $@

$(BUILD)/sqlite3-ulmdb-native.o: $(BUILD)/sqlite3-ulmdb.c native_backend.h
	$(CC) $(CPPFLAGS) $(CFLAGS) -fPIC $(PORT_FLAGS) -DULMDB_BTREE_NATIVE -I$(CURDIR) -I$(ULMDB_ROOT)/clients/c/include -I$(SQLITE_SRC) -c $< -o $@

$(BUILD)/libsqlite3_ulmdb_native.so: $(BUILD)/sqlite3-ulmdb-native.o native.exports native-prepare
	$(CC) -shared $< $(ULMDB_BTREE_NATIVE_LIB) -Wl,-z,nodelete -Wl,--version-script=native.exports -Wl,-Bsymbolic-functions -ldl -lpthread -lm -lrt -lutil $(LDFLAGS) -o $@

$(BUILD)/sqlite3-ulmdb-native: $(SQLITE_SRC)/shell.c $(BUILD)/sqlite3-ulmdb-native.o native-prepare
	$(CC) $(CPPFLAGS) $(CFLAGS) $(PORT_FLAGS) -I$(SQLITE_SRC) $(SQLITE_SRC)/shell.c $(BUILD)/sqlite3-ulmdb-native.o $(ULMDB_BTREE_NATIVE_LIB) -ldl -lpthread -lm -lrt -lutil $(LDFLAGS) -o $@

$(BUILD)/libsqlite3_ulmdb.so: $(BUILD)/sqlite3-ulmdb.o
	$(CC) -shared $< -L$(ULMDB_LIBDIR) -lulmdb_c -Wl,-rpath,$(ULMDB_LIBDIR) -ldl -lpthread -lm $(LDFLAGS) -o $@

$(BUILD)/sqlite3-ulmdb: $(SQLITE_SRC)/shell.c $(BUILD)/sqlite3-ulmdb.o
	$(CC) $(CPPFLAGS) $(CFLAGS) $(PORT_FLAGS) -I$(SQLITE_SRC) $^ -L$(ULMDB_LIBDIR) -lulmdb_c -Wl,-rpath,$(ULMDB_LIBDIR) -ldl -lpthread -lm $(LDFLAGS) -o $@

$(BUILD)/test-api: test_api.c $(BUILD)/libsqlite3_ulmdb.so
	$(CC) $(CPPFLAGS) $(CFLAGS) -I$(SQLITE_SRC) $< -L$(BUILD) -lsqlite3_ulmdb -Wl,-rpath,$(BUILD) -o $@

$(BUILD)/test-api-native: test_api.c $(BUILD)/libsqlite3_ulmdb_native.so
	$(CC) $(CPPFLAGS) $(CFLAGS) -I$(SQLITE_SRC) $< -L$(BUILD) -lsqlite3_ulmdb_native -Wl,-rpath,$(BUILD) -o $@

test-native: native $(BUILD)/test-api-native
	python3 test.py $(BUILD)/sqlite3-ulmdb-native --native $(ULMDB_BTREE_NATIVE_TEST_FLAGS)
	$(BUILD)/test-api-native
	python3 test_runtime.py $(BUILD)/libsqlite3_ulmdb_native.so --native
	python3 test_root_savepoint.py $(BUILD)/libsqlite3_ulmdb_native.so --native
	python3 test_writes.py $(BUILD)/libsqlite3_ulmdb_native.so
	python3 test_writer_cache.py $(BUILD)/libsqlite3_ulmdb_native.so

test-native-read-hash: test-native
	python3 test_read_hash.py $(BUILD)/libsqlite3_ulmdb_native.so

test-native-blocks: ULMDB_BTREE_NATIVE_FEATURES = btree-blocks
test-native-blocks: ULMDB_BTREE_NATIVE_TEST_FLAGS = --blocks
test-native-blocks: test-native-read-hash

$(BUILD)/test-writer-cache: test_writer_cache.c writer_cache.c
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Wall -Wextra -Werror $< -o $@

test-writer-cache: $(BUILD)/test-writer-cache
	$(BUILD)/test-writer-cache

test: all $(BUILD)/test-api
	python3 test.py $(BUILD)/sqlite3-ulmdb
	$(BUILD)/test-api
	python3 test_runtime.py $(BUILD)/libsqlite3_ulmdb.so
	python3 test_root_savepoint.py $(BUILD)/libsqlite3_ulmdb.so
	python3 test_writes.py $(BUILD)/libsqlite3_ulmdb.so

clean:
	rm -f $(BUILD)/sqlite3-ulmdb.c $(BUILD)/sqlite3-ulmdb.o $(BUILD)/sqlite3-ulmdb $(BUILD)/libsqlite3_ulmdb.so
