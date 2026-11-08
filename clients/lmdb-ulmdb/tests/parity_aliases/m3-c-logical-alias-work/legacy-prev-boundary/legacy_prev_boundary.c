#define _GNU_SOURCE
#include <lmdb.h>
#include <dlfcn.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#define OK(x) do { int r = (x); if (r) { fprintf(stderr, "%s=%d\n", #x, r); return 2; } } while (0)
#define NEED(x) do { if (!(x)) { fprintf(stderr, "%s failed\n", #x); return 2; } } while (0)

int main(int argc, char **argv) {
    NEED(argc == 3);
    Dl_info info;
    NEED(dladdr((void *)mdb_env_open, &info) && info.dli_fname);
    fprintf(stderr, "PROVIDER %s\n", info.dli_fname);
#ifdef MDB_PREVSNAPSHOT
    puts("header.previous=1");
#else
    puts("header.previous=0");
#endif
    MDB_env *env;
    OK(mdb_env_create(&env));
    OK(mdb_env_set_mapsize(env, 16 * 1024 * 1024));
    if (!strcmp(argv[1], "probe")) {
        /* Numeric unknown flag, not a purported legacy API declaration. */
        int rc = mdb_env_open(env, argv[2], MDB_NOTLS | 0x02000000u, 0600);
        printf("unknown.previous.open=%d\n", rc);
        NEED(rc == EINVAL);
        mdb_env_close(env);
        return 0;
    }
    int seed = !strcmp(argv[1], "seed");
    NEED(seed || !strcmp(argv[1], "check"));
    if (seed) NEED(!mkdir(argv[2], 0700));
    OK(mdb_env_open(env, argv[2], MDB_NOTLS, 0600));
    MDB_txn *txn;
    OK(mdb_txn_begin(env, NULL, seed ? 0 : MDB_RDONLY, &txn));
    MDB_dbi main;
    OK(mdb_dbi_open(txn, NULL, 0, &main));
    MDB_val key = {1, "k"}, data = {1, "A"};
    if (seed) {
        OK(mdb_put(txn, main, &key, &data, 0));
        OK(mdb_txn_commit(txn));
        puts("seed=A");
    } else {
        OK(mdb_get(txn, main, &key, &data));
        NEED(data.mv_size == 1 && !memcmp(data.mv_data, "A", 1));
        printf("checked.id=%zu value=A\n", (size_t)mdb_txn_id(txn));
        mdb_txn_abort(txn);
    }
    mdb_env_close(env);
    return 0;
}
