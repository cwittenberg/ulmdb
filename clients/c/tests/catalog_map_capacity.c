/* Adapter logical-capacity contract; physical LMDB allocation thresholds differ. */
#include "lmdb.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#define OK(call) do { int rc_ = (call); if (rc_) { \
    fprintf(stderr, "%s=%d\n", #call, rc_); return 1; } } while (0)
static MDB_val text(const char *value) {
    MDB_val v = {strlen(value), (void *)value}; return v;
}
static int catalog(const char *path) {
    MDB_env *env; MDB_txn *txn, *old; MDB_dbi stable, db;
    OK(mdb_env_create(&env));
    OK(mdb_env_set_maxdbs(env, 512));
    OK(mdb_env_set_mapsize(env, 16384));
    OK(mdb_env_open(env, path, MDB_NOTLS, 0600));
    OK(mdb_txn_begin(env, NULL, 0, &txn));
    OK(mdb_dbi_open(txn, "stable", MDB_CREATE, &stable));
    MDB_val k = text("k"), v = text("kept");
    OK(mdb_put(txn, stable, &k, &v, 0));
    OK(mdb_txn_commit(txn));
    /* This reader never dbi_opens while writers open their own names. */
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &old));
    size_t before = mdb_txn_id(old);
    OK(mdb_get(old, stable, &k, &v));
    void *borrow = v.mv_data;
    OK(mdb_txn_begin(env, NULL, 0, &txn));
    unsigned count = 0; int rc = 0; char name[80];
    for (; count != 256; ++count) {
        snprintf(name, sizeof name, "empty-%04u-abcdefghijklmnopqrstuvwxyz-abcdefghijklmnopqrstuvwxyz", count);
        rc = mdb_dbi_open(txn, name, MDB_CREATE, &db);
        if (rc) break;
    }
    printf("empty-create-count=%u rc=%d\n", count, rc);
    fflush(stdout);
    if (rc != MDB_MAP_FULL || count == 256) {
        mdb_txn_abort(txn); mdb_txn_abort(old); mdb_env_close(env); return 2;
    }
    assert(mdb_txn_commit(txn) == MDB_BAD_TXN);
    assert(!memcmp(borrow, "kept", 4));
    mdb_txn_abort(old);
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn));
    assert(mdb_txn_id(txn) == before);
    OK(mdb_get(txn, stable, &k, &v));
    assert(v.mv_size == 4 && !memcmp(v.mv_data, "kept", 4));
    assert(mdb_dbi_open(txn, "empty-0000-abcdefghijklmnopqrstuvwxyz-abcdefghijklmnopqrstuvwxyz", 0, &db) == MDB_NOTFOUND);
    mdb_txn_abort(txn);
    OK(mdb_env_set_mapsize(env, 1u << 20));
    OK(mdb_txn_begin(env, NULL, 0, &txn));
    for (count = 0; count != 256; ++count) {
        snprintf(name, sizeof name, "empty-%04u-abcdefghijklmnopqrstuvwxyz-abcdefghijklmnopqrstuvwxyz", count);
        OK(mdb_dbi_open(txn, name, MDB_CREATE, &db));
    }
    OK(mdb_txn_commit(txn));
    mdb_env_close(env);
    OK(mdb_env_create(&env));
    OK(mdb_env_set_maxdbs(env, 512));
    OK(mdb_env_open(env, path, MDB_NOTLS, 0600));
    MDB_envinfo info; OK(mdb_env_info(env, &info)); assert(info.me_mapsize == (1u << 20));
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn));
    for (count = 0; count != 256; ++count) {
        snprintf(name, sizeof name, "empty-%04u-abcdefghijklmnopqrstuvwxyz-abcdefghijklmnopqrstuvwxyz", count);
        OK(mdb_dbi_open(txn, name, 0, &db));
    }
    OK(mdb_txn_commit(txn));
    mdb_env_close(env);
    puts("catalog-capacity-grow-reopen-ok"); return 0;
}
static int flags_only(const char *path, int prepare) {
    MDB_env *env; MDB_txn *txn; MDB_dbi main; unsigned flags;
    OK(mdb_env_create(&env));
    OK(mdb_env_set_mapsize(env, 8192));
    OK(mdb_env_open(env, path, MDB_NOTLS, 0600));
    OK(mdb_txn_begin(env, NULL, 0, &txn));
    OK(mdb_dbi_open(txn, NULL, MDB_DUPSORT, &main));
    int rc = prepare ? mdb_txn_prepare(txn) : mdb_txn_commit(txn);
    printf("flags-only-%s=%d\n", prepare ? "prepare" : "commit", rc);
    if (rc != MDB_MAP_FULL) {
        if (prepare && !rc) mdb_txn_abort(txn);
        mdb_env_close(env); return 2;
    }
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn));
    assert(mdb_txn_id(txn) == 0);
    OK(mdb_dbi_open(txn, NULL, 0, &main));
    OK(mdb_dbi_flags(txn, main, &flags)); assert(flags == 0);
    mdb_txn_abort(txn); mdb_env_close(env);
    puts("flags-only-nonpublication-ok"); return 0;
}
int main(int argc, char **argv) {
    assert(argc == 3);
    assert(!mkdir(argv[2], 0700));
    if (!strcmp(argv[1], "catalog")) return catalog(argv[2]);
    assert(!strcmp(argv[1], "commit") || !strcmp(argv[1], "prepare"));
    return flags_only(argv[2], !strcmp(argv[1], "prepare"));
}
