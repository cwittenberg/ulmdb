/* Newer 0.9 rejects empty duplicate values; ordinary empty values remain valid. */
#include <lmdb.h>
#include <stdio.h>
#include <stdlib.h>

static void ok(int result) {
    if (result) { fprintf(stderr, "%s\n", mdb_strerror(result)); exit(1); }
}
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    MDB_env *env;
    MDB_txn *txn;
    MDB_dbi plain, duplicates;
    ok(mdb_env_create(&env));
    ok(mdb_env_set_maxdbs(env, 4));
    ok(mdb_env_set_mapsize(env, 8u * 1024u * 1024u));
    ok(mdb_env_open(env, argv[1], MDB_NOTLS, 0600));
    ok(mdb_txn_begin(env, NULL, 0, &txn));
    ok(mdb_dbi_open(txn, "plain", MDB_CREATE, &plain));
    ok(mdb_dbi_open(txn, "duplicates", MDB_CREATE | MDB_DUPSORT, &duplicates));
    char byte = 'k';
    MDB_val key = {1, &byte}, value = {0, &byte};
    ok(mdb_put(txn, plain, &key, &value, 0));
    puts("ordinary.empty=accepted");
    int result = mdb_put(txn, duplicates, &key, &value, 0);
    printf("duplicates.empty=%d\n", result);
    if (result != MDB_BAD_VALSIZE) return 1;
    ok(mdb_txn_commit(txn));
    mdb_env_close(env);
    ok(mdb_env_create(&env));
    ok(mdb_env_set_maxdbs(env, 4));
    ok(mdb_env_open(env, argv[1], MDB_RDONLY | MDB_NOTLS, 0600));
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn));
    ok(mdb_dbi_open(txn, "plain", 0, &plain));
    ok(mdb_get(txn, plain, &key, &value));
    if (value.mv_size) return 1;
    ok(mdb_dbi_open(txn, "duplicates", 0, &duplicates));
    result = mdb_get(txn, duplicates, &key, &value);
    printf("reopen.duplicates=%d\n", result);
    if (result != MDB_NOTFOUND) return 1;
    mdb_txn_abort(txn);
    mdb_env_close(env);
    return 0;
}
