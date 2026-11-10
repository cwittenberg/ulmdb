EXTENSION = ulmdb_fdw
MODULE_big = ulmdb_fdw
OBJS = ulmdb_fdw.o
DATA = ulmdb_fdw--0.1.0.sql
ULMDB_ROOT ?= $(abspath ../..)
ULMDB_LIBDIR ?= $(ULMDB_ROOT)/target/release
ULMDB_SQL_BATCH ?= 0
PG_CPPFLAGS += -I$(ULMDB_ROOT)/clients/c/include
ifeq ($(ULMDB_SQL_BATCH),1)
PG_CPPFLAGS += -DULMDB_FDW_SQL_BATCH
endif
SHLIB_LINK += -L$(ULMDB_LIBDIR) -lulmdb_c -Wl,-rpath,$(ULMDB_LIBDIR)
PG_CONFIG ?= pg_config
PGXS := $(shell $(PG_CONFIG) --pgxs)
include $(PGXS)

# Rebuild in either flag state; switching back must remove the optional symbol.
.PHONY: force-fdw-build
ulmdb_fdw.o ulmdb_fdw.bc: force-fdw-build

test_batch_refusal.so: test_batch_refusal.c native/sql_batch.h
	$(CC) $(CFLAGS) $(CPPFLAGS) -fPIC -shared -fvisibility=default $< -ldl -o $@
EXTRA_CLEAN += test_batch_refusal.so
