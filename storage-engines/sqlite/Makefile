ULMDB_ROOT ?= ../..
ULMDB_LIBDIR ?= $(abspath $(ULMDB_ROOT)/target/release)
ULMDB_NATIVE_TARGET ?= $(abspath $(ULMDB_ROOT)/target/sqlite-native-owned-versioned)
ULMDB_NATIVE_FEATURES ?= versioned
ULMDB_PAGES_TARGET ?= $(abspath $(ULMDB_ROOT)/target/sqlite-native-vfs)
ULMDB_PAGES_FEATURES ?= vfs
ULMDB_PAGES_BUILD := 0
ifeq ($(origin ULMDB_PAGES_LIB), undefined)
ULMDB_PAGES_LIB = $(ULMDB_PAGES_TARGET)/release/libulmdb_sqlite_native.a
ULMDB_PAGES_BUILD := 1
endif
ULMDB_NATIVE_BUILD := 0
ifeq ($(origin ULMDB_NATIVE_LIB), undefined)
ULMDB_NATIVE_LIB = $(ULMDB_NATIVE_TARGET)/release/libulmdb_sqlite_native.a
ULMDB_NATIVE_BUILD := 1
endif
CC ?= cc
INCLUDES = -I$(ULMDB_ROOT)/clients/c/include
CFLAGS ?= -O2 -g
BASE_CFLAGS = -std=c11 -Wall -Wextra -Werror -fPIC

.PHONY: all owned owned-prepare vfs vfs-prepare clean test test-owned test-owned-capacity test-vfs
all: ulmdb_sqlite.so

owned: ulmdb_sqlite_owned.so

vfs: ulmdb_vfs.so

vfs-prepare:
ifeq ($(ULMDB_PAGES_BUILD),1)
	cargo build --release --locked --manifest-path native/Cargo.toml --target-dir $(ULMDB_PAGES_TARGET) $(if $(strip $(ULMDB_PAGES_FEATURES)),--features $(ULMDB_PAGES_FEATURES))
else
	test -f $(ULMDB_PAGES_LIB)
endif

ulmdb_vfs.so: ulmdb_vfs.c native/pages_backend.h vfs.exports vfs-prepare
	$(CC) $(CPPFLAGS) $(CFLAGS) $(BASE_CFLAGS) -shared $< $(ULMDB_PAGES_LIB) \
	  -Wl,-z,nodelete -Wl,--version-script=vfs.exports -Wl,-Bsymbolic-functions \
	  -ldl -lpthread -lm -lrt -lutil $(LDFLAGS) -o $@

owned-prepare:
ifeq ($(ULMDB_NATIVE_BUILD),1)
	cargo build --release --locked --manifest-path native/Cargo.toml --target-dir $(ULMDB_NATIVE_TARGET) $(if $(strip $(ULMDB_NATIVE_FEATURES)),--features $(ULMDB_NATIVE_FEATURES))
else
	test -f $(ULMDB_NATIVE_LIB)
endif

ulmdb_sqlite_owned.so: ulmdb_sqlite.c native/owned_backend.h owned.exports owned-prepare
	$(CC) $(CPPFLAGS) $(INCLUDES) $(CFLAGS) $(BASE_CFLAGS) -DULMDB_SQLITE_OWNED -shared $< \
	  $(ULMDB_NATIVE_LIB) -Wl,-z,nodelete -Wl,--version-script=owned.exports \
	  -Wl,-Bsymbolic-functions -ldl -lpthread -lm -lrt -lutil $(LDFLAGS) -o $@

ulmdb_sqlite.so: ulmdb_sqlite.c $(ULMDB_ROOT)/clients/c/include/lmdb.h
	$(CC) $(CPPFLAGS) $(INCLUDES) $(CFLAGS) $(BASE_CFLAGS) -shared $< -L$(ULMDB_LIBDIR) -lulmdb_c \
	  -Wl,-rpath,$(ULMDB_LIBDIR) $(LDFLAGS) -o $@

test_commit_failure.so: test_commit_failure.c $(ULMDB_ROOT)/clients/c/include/lmdb.h
	$(CC) $(CPPFLAGS) $(INCLUDES) $(CFLAGS) $(BASE_CFLAGS) -shared $< -ldl -o $@

test: all test_commit_failure.so
	python3 test.py $(abspath ulmdb_sqlite.so)
	LD_PRELOAD=$(abspath test_commit_failure.so) \
	  ULMDB_SQLITE_TEST_LIBRARY=$(ULMDB_LIBDIR)/libulmdb_c.so \
	  python3 test.py $(abspath ulmdb_sqlite.so) --commit-failure

test-owned: owned
	python3 test.py $(abspath ulmdb_sqlite_owned.so) --module ulmdb_owned
	python3 test_owned.py $(abspath ulmdb_sqlite_owned.so)

test-owned-capacity: owned
	python3 test_owned_commit.py $(abspath ulmdb_sqlite_owned.so)

test-vfs: vfs
	python3 test_vfs.py $(abspath ulmdb_vfs.so)

clean:
	rm -f ulmdb_sqlite.so ulmdb_sqlite_owned.so ulmdb_vfs.so test_commit_failure.so
