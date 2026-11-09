EXTENSION = ulmdb_fdw
MODULE_big = ulmdb_fdw
OBJS = ulmdb_fdw.o
DATA = ulmdb_fdw--0.1.0.sql
ULMDB_ROOT ?= $(abspath ../..)
ULMDB_LIBDIR ?= $(ULMDB_ROOT)/target/release
PG_CPPFLAGS += -I$(ULMDB_ROOT)/clients/c/include
SHLIB_LINK += -L$(ULMDB_LIBDIR) -lulmdb_c -Wl,-rpath,$(ULMDB_LIBDIR)
PG_CONFIG ?= pg_config
PGXS := $(shell $(PG_CONFIG) --pgxs)
include $(PGXS)
