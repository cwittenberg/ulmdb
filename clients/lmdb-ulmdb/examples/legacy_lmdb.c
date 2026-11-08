/* Ordinary LMDB API; compile against upstream lmdb.h and -llmdb unchanged. */
#include <lmdb.h>
#include <stdio.h>
#include <string.h>

#define CHECK(call) do { int status = (call); if (status) { \
    fprintf(stderr, "%s: %s\n", #call, mdb_strerror(status)); return 1; \
} } while (0)

int main(int argc, char **argv) {
    if (argc != 2) { fprintf(stderr, "usage: %s DATABASE_DIRECTORY\n", argv[0]); return 2; }
    MDB_env *env;
    MDB_txn *txn;
    MDB_dbi db;
    CHECK(mdb_env_create(&env));
    CHECK(mdb_env_open(env, argv[1], 0, 0600));
    CHECK(mdb_txn_begin(env, NULL, 0, &txn));
    CHECK(mdb_dbi_open(txn, NULL, 0, &db));
    MDB_val key = {5, (void *)"hello"}, value = {5, (void *)"world"};
    CHECK(mdb_put(txn, db, &key, &value, 0));
    CHECK(mdb_txn_commit(txn));
    CHECK(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn));
    CHECK(mdb_get(txn, db, &key, &value));
    printf("%.*s = %.*s\n", (int)key.mv_size, (char *)key.mv_data,
           (int)value.mv_size, (char *)value.mv_data);
    mdb_txn_abort(txn);
    mdb_env_close(env);
    return 0;
}
