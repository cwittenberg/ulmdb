CC ?= cc
PKG_CONFIG ?= pkg-config
CFLAGS ?= -O2
OUTPUT ?= ../../../target/crypto/libulmdb_aes256gcm.so
CPPFLAGS += -I../include $(shell $(PKG_CONFIG) --cflags libcrypto)
MODULE_FLAGS = -std=c11 -D_POSIX_C_SOURCE=200809L -Wall -Wextra -Werror -Wconversion -Wshadow -fPIC -fvisibility=hidden
.PHONY: all check-openssl clean
all: check-openssl $(OUTPUT)
check-openssl:
	$(PKG_CONFIG) --atleast-version=3.0 libcrypto
$(OUTPUT): openssl_aes256gcm.c ../include/lmdb.h Makefile
	mkdir -p "$(dir $(OUTPUT))"
	$(CC) $(CPPFLAGS) $(CFLAGS) $(MODULE_FLAGS) -shared openssl_aes256gcm.c -Wl,-z,defs -Wl,-soname,libulmdb_aes256gcm.so $(shell $(PKG_CONFIG) --libs libcrypto) -o "$@"
clean:
	rm -f "$(OUTPUT)"
