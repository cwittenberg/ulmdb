ROOT := $(abspath ../..)
TOOLS := $(ROOT)/target/lmdb-ulmdb-tools
PACKAGER := $(TOOLS)/ulmdb-adapter-build
OUTPUT ?= $(ROOT)/target/lmdb-ulmdb
PACKAGE_OUTPUT := $(abspath $(OUTPUT))
TEST_RUNNER := $(ROOT)/target/lmdb-ulmdb-tests
PARITY_RUNNER := $(ROOT)/target/lmdb-ulmdb-parity
CC ?= cc
CFLAGS ?= -O2
ifeq ($(shell uname -s),Darwin)
WARNINGS := -std=c11 -D_DARWIN_C_SOURCE -Wall -Wextra -Werror
DL_LIBS :=
else
WARNINGS := -std=c11 -D_GNU_SOURCE -Wall -Wextra -Werror
DL_LIBS := -ldl
endif
BUILD_SOURCES := build.c build_support.c build_pins.c build_hash.c
TEST_SOURCES := tests/test_adapter.c tests/support.c build_support.c build_pins.c build_hash.c
PARITY_SOURCES := tests/parity_runner.c tests/parity_report.c tests/support.c build_support.c build_pins.c build_hash.c

.PHONY: all package builder test parity features clean
all: package

builder: $(PACKAGER)

$(PACKAGER): $(BUILD_SOURCES) build_support.h exports.map exports.darwin darwin_tools.h Makefile
	mkdir -p "$(TOOLS)"
	$(CC) $(CFLAGS) $(WARNINGS) '-DULMDB_SOURCE_ROOT="$(ROOT)"' $(BUILD_SOURCES) -o "$@"

package: $(PACKAGER)
	CC='$(CC)' "$(PACKAGER)" --output "$(PACKAGE_OUTPUT)" $(ARGS)

$(TEST_RUNNER): $(TEST_SOURCES) tests/support.h build_support.h Makefile
	mkdir -p "$(ROOT)/target"
	$(CC) $(CFLAGS) $(WARNINGS) $(TEST_SOURCES) -o "$@"

test: $(TEST_RUNNER)
	"$(TEST_RUNNER)" --no-build --package "$(PACKAGE_OUTPUT)" $(TEST_ARGS)

$(PARITY_RUNNER): $(PARITY_SOURCES) tests/parity_report.h tests/support.h build_support.h Makefile
	mkdir -p "$(ROOT)/target"
	$(CC) $(CFLAGS) $(WARNINGS) $(PARITY_SOURCES) $(DL_LIBS) -o "$@"

parity: package $(TEST_RUNNER) $(PARITY_RUNNER)
	cd "$(ROOT)" && "$(PARITY_RUNNER)" --package "$(PACKAGE_OUTPUT)" $(PARITY_ARGS)

features: package $(TEST_RUNNER) $(PARITY_RUNNER)
	cd "$(ROOT)" && "$(PARITY_RUNNER)" --package "$(PACKAGE_OUTPUT)" --profile features-v2 $(PARITY_ARGS)

clean:
	rm -f "$(PACKAGER)" "$(TEST_RUNNER)" "$(PARITY_RUNNER)"
