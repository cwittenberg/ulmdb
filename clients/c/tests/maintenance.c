#include "ulmdb.h"
#include <assert.h>
#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

int main(int argc, char **argv) {
    assert(argc >= 2 && argc <= 4);
    unsigned int flags = argc >= 3 ? (unsigned int)strtoul(argv[2], NULL, 0) : 0;
    int automatic = argc == 4 && strcmp(argv[3], "default") == 0;
    unsigned int writes = automatic ? 320 : 128;
    MDB_env *env = NULL;
    assert(mkdir(argv[1], 0700) == 0);
    assert(mdb_env_create(&env) == 0);
    assert(mdb_env_set_mapsize(env, 4U << 20) == 0);
    assert(mdb_env_open(env, argv[1], MDB_NOTLS | flags, 0600) == 0);
    if (!automatic) assert(ulmdb_env_set_maintenance(env, 2, 64U << 10, 8) == 0);
    MDB_txn *txn = NULL;
    MDB_dbi db;
    assert(mdb_txn_begin(env, NULL, 0, &txn) == 0);
    assert(mdb_dbi_open(txn, NULL, 0, &db) == 0);
    unsigned char value[4096] = {0};
    for (unsigned int n = 0; n < 64; ++n) {
        unsigned char encoded[4] = {0, 0, 0, (unsigned char)n};
        MDB_val key = {sizeof encoded, encoded}, data = {sizeof value, value};
        assert(mdb_put(txn, db, &key, &data, 0) == 0);
    }
    assert(mdb_txn_commit(txn) == 0);
    MDB_txn *old = NULL;
    assert(mdb_txn_begin(env, NULL, MDB_RDONLY, &old) == 0);
    for (unsigned int n = 0; n < writes; ++n) {
        assert(mdb_txn_begin(env, NULL, 0, &txn) == 0);
        unsigned char encoded[4] = {0, 0, 0, 0};
        memset(value, (int)n, sizeof value);
        MDB_val key = {sizeof encoded, encoded}, data = {sizeof value, value};
        assert(mdb_put(txn, db, &key, &data, 0) == 0);
        assert(mdb_txn_commit(txn) == 0);
    }
    unsigned char encoded[4] = {0, 0, 0, 0};
    MDB_val key = {sizeof encoded, encoded}, data = {0, NULL};
    assert(mdb_get(old, db, &key, &data) == 0);
    assert(data.mv_size == sizeof value && ((unsigned char *)data.mv_data)[0] == 0);
    mdb_txn_abort(old);
    assert(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn) == 0);
    assert(mdb_get(txn, db, &key, &data) == 0);
    assert(((unsigned char *)data.mv_data)[0] == (unsigned char)(writes - 1));
    mdb_txn_abort(txn);
    char path[4096];
    assert(snprintf(path, sizeof path, "%s/segments", argv[1]) < (int)sizeof path);
    DIR *directory = opendir(path);
    assert(directory != NULL);
    unsigned int files = 0;
    struct dirent *entry;
    while ((entry = readdir(directory)) != NULL) {
        size_t length = strlen(entry->d_name);
        if (length > 4 && strcmp(entry->d_name + length - 4, ".seg") == 0) ++files;
    }
    assert(closedir(directory) == 0);
    assert(files < (automatic ? 192U : 100U)); /* No manual cleanup calls. */
    assert(ulmdb_env_set_maintenance(env, 0, 0, 0) == 0);
    mdb_env_close(env);
    return 0;
}
