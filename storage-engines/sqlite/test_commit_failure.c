/* Test-only LD_PRELOAD fault injection: consume one writer and fail its commit. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdlib.h>
#include "lmdb.h"

static int armed;
void ulmdb_test_fail_commit_once(void) { armed = 1; }

int mdb_txn_commit(MDB_txn *txn) {
    static int (*real_commit)(MDB_txn *);
    static void (*real_abort)(MDB_txn *);
    if (!real_commit) {
        const char *path = getenv("ULMDB_SQLITE_TEST_LIBRARY");
        void *handle = path ? dlopen(path, RTLD_NOW | RTLD_NOLOAD) : 0;
        if (!handle) abort();
        real_commit = (int (*)(MDB_txn *))dlsym(handle, "mdb_txn_commit");
        real_abort = (void (*)(MDB_txn *))dlsym(handle, "mdb_txn_abort");
        if (!real_commit || !real_abort) abort();
    }
    if (armed) {
        armed = 0;
        real_abort(txn);
        return MDB_PANIC;
    }
    return real_commit(txn);
}
