#include "lmdb.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef CATALOG_DATABASES
#define CATALOG_DATABASES 5005
#endif
enum { DATABASES = CATALOG_DATABASES };
#define OK(call) do { \
    int rc = (call); \
    if (rc) { fprintf(stderr, "%s=%d\n", #call, rc); return 1; } \
} while (0)

static int cursor_catalog(MDB_txn *txn) {
    MDB_cursor *cursor;
    MDB_val key, value;
    OK(mdb_cursor_open(txn, 1, &cursor));
    unsigned named = 0, ordinary = 0;
    int rc = mdb_cursor_get(cursor, &key, &value, MDB_FIRST);
    while (!rc) {
        if (key.mv_size == 8 && !memcmp(key.mv_data, "db02500x", 8)) {
            assert(!mdb_cursor_is_db(cursor));
            assert(value.mv_size == 1 && *(char *)value.mv_data == 'v');
            ordinary++;
        } else {
            char expected[32];
            snprintf(expected, sizeof expected, "db%05u", named++);
            size_t length = strlen(expected);
            assert(key.mv_size == length || key.mv_size == length + 1);
            assert(!memcmp(key.mv_data, expected, length));
            if (key.mv_size > length) assert(((char *)key.mv_data)[length] == 0);
            assert(mdb_cursor_is_db(cursor));
        }
        rc = mdb_cursor_get(cursor, &key, &value, MDB_NEXT);
    }
    assert(rc == MDB_NOTFOUND && named == DATABASES && ordinary == 1);
    unsigned count = 0;
    rc = mdb_cursor_get(cursor, &key, &value, MDB_LAST);
    while (!rc) {
        count++;
        rc = mdb_cursor_get(cursor, &key, &value, MDB_PREV);
    }
    assert(rc == MDB_NOTFOUND && count == DATABASES + 1);
    key.mv_data = "db04000x";
    key.mv_size = 8;
    OK(mdb_cursor_get(cursor, &key, &value, MDB_SET_RANGE));
    assert(key.mv_size >= 7 && !memcmp(key.mv_data, "db04001", 7));
    OK(mdb_cursor_get(cursor, &key, &value, MDB_PREV));
    assert(key.mv_size >= 7 && !memcmp(key.mv_data, "db04000", 7));
    mdb_cursor_close(cursor);
    return 0;
}

int main(int argc, char **argv) {
    assert(argc == 3 || argc == 4);
    int seed = !strcmp(argv[1], "seed");
    assert(seed || !strcmp(argv[1], "cold"));
    MDB_env *env;
    OK(mdb_env_create(&env));
    OK(mdb_env_set_maxdbs(env, DATABASES));
    OK(mdb_env_set_mapsize(env, 128u << 20));
    if (seed) {
        /* The extended native profile reserves eight internal catalog slots. */
        assert(mdb_env_set_maxdbs(env, 65529) == EINVAL);
        OK(mdb_env_set_maxdbs(env, DATABASES));
    }
    unsigned open_flags = argc == 4 ? (unsigned)strtoul(argv[3], NULL, 0) : 0;
    OK(mdb_env_open(env, argv[2], MDB_NOTLS | open_flags | (seed ? 0 : MDB_RDONLY), 0600));
    MDB_txn *txn;
    MDB_dbi db[DATABASES];
    char name[32];
    MDB_val key = {1, "k"}, value = {1, "v"};
    if (seed) {
        OK(mdb_txn_begin(env, NULL, 0, &txn));
        for (int n = 0; n < DATABASES; ++n) {
            snprintf(name, sizeof name, "db%05d", n);
            OK(mdb_dbi_open(txn, name, MDB_CREATE, &db[n]));
            if (n == 4096 || n == DATABASES - 1)
                OK(mdb_put(txn, db[n], &key, &value, 0));
        }
        MDB_dbi excess;
        assert(mdb_dbi_open(txn, "excess", MDB_CREATE, &excess) == MDB_DBS_FULL);
        MDB_val ordinary = {8, "db02500x"};
        OK(mdb_put(txn, 1, &ordinary, &value, 0));
        OK(mdb_txn_commit(txn));
    }
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn));
    for (int n = 0; n < DATABASES; ++n) {
        snprintf(name, sizeof name, "db%05d", n);
        OK(mdb_dbi_open(txn, name, 0, &db[n]));
        unsigned flags;
        OK(mdb_dbi_flags(txn, db[n], &flags));
        assert(flags == 0);
        if (n == 4096 || n == DATABASES - 1) {
            MDB_val got;
            OK(mdb_get(txn, db[n], &key, &got));
            assert(got.mv_size == 1 && *(char *)got.mv_data == 'v');
        }
    }
    OK(cursor_catalog(txn));
    OK(mdb_txn_commit(txn));
    mdb_env_close(env);
    printf("catalog%u-ok:%s\n", (unsigned)DATABASES, argv[1]);
    return 0;
}
