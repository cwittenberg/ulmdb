/* Compiled once against the real upstream lmdb.h and liblmdb.so.0. */
#include <lmdb.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define REQUIRE(test) do { if (!(test)) { fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #test); return 1; } } while (0)
#define OK(call) do { int code_ = (call); if (code_) { fprintf(stderr, "%s:%d: %s: %d %s\n", __FILE__, __LINE__, #call, code_, mdb_strerror(code_)); return 1; } } while (0)

static MDB_val text(const char *text) {
    MDB_val value = {strlen(text), (void *)text};
    return value;
}
static int put(MDB_txn *txn, MDB_dbi db, const void *key, size_t key_size, const void *value, size_t value_size) {
    MDB_val k = {key_size, (void *)key}, v = {value_size, (void *)value};
    return mdb_put(txn, db, &k, &v, 0);
}
static int matches(MDB_txn *txn, MDB_dbi db, const void *key, size_t key_size, const void *expected, size_t expected_size) {
    MDB_val k = {key_size, (void *)key}, value;
    int code = mdb_get(txn, db, &k, &value);
    return code == 0 && value.mv_size == expected_size && (!expected_size || memcmp(value.mv_data, expected, expected_size) == 0);
}
int main(int argc, char **argv) {
    REQUIRE(argc == 3);
    const char *action = argv[1], *directory = argv[2];
    if (strcmp(action, "backend") == 0) {
        puts(mdb_version(NULL, NULL, NULL));
        return 0;
    }
    MDB_env *env;
    MDB_txn *txn;
    MDB_dbi main_db, plain, duplicates;
    OK(mdb_env_create(&env));
    OK(mdb_env_set_maxdbs(env, 16));
    OK(mdb_env_set_mapsize(env, 64 * 1024 * 1024));
    int file_mode = strcmp(action, "seed-file") == 0 || strcmp(action, "check-file") == 0;
    OK(mdb_env_open(env, directory, MDB_NOTLS | (file_mode ? MDB_NOSUBDIR : 0), 0600));
    int seed = strcmp(action, "seed") == 0 || strcmp(action, "seed-flags") == 0 || strcmp(action, "seed-file") == 0;
    int update = strcmp(action, "update") == 0;
    OK(mdb_txn_begin(env, NULL, seed || update ? 0 : MDB_RDONLY, &txn));
    OK(mdb_dbi_open(txn, NULL, 0, &main_db));
    OK(mdb_dbi_open(txn, "plain", seed ? MDB_CREATE : 0, &plain));
    OK(mdb_dbi_open(txn, "duplicates", seed ? MDB_CREATE | MDB_DUPSORT : 0, &duplicates));
    const unsigned char binary_key[] = {0, 255, 1};
    const unsigned char binary_value[] = {255, 0, 128, 1};
#if MDB_VERSION_MAJOR >= 1
    const size_t neighbor_plain = 5, neighbor_duplicates = 10;
#else
    /* Legacy catalog keys omit the NUL; binary user keys can include it. */
    const size_t neighbor_plain = 6, neighbor_duplicates = 11;
#endif
    if (seed) {
        OK(put(txn, main_db, "main-user", 9, "main-value", 10));
        OK(put(txn, main_db, "plain", neighbor_plain, "catalog-neighbor", 16));
        OK(put(txn, main_db, "duplicates", neighbor_duplicates, "also-user", 9));
        OK(put(txn, plain, "hello", 5, "world", 5));
        OK(put(txn, plain, binary_key, sizeof binary_key, binary_value, sizeof binary_value));
        OK(put(txn, plain, "empty", 5, "", 0));
        OK(put(txn, duplicates, "group", 5, "third", 5));
        OK(put(txn, duplicates, "group", 5, "first", 5));
        OK(put(txn, duplicates, "group", 5, "second", 6));
    } else {
        REQUIRE(matches(txn, main_db, "main-user", 9, "main-value", 10));
        REQUIRE(matches(txn, main_db, "plain", neighbor_plain, "catalog-neighbor", 16));
        REQUIRE(matches(txn, main_db, "duplicates", neighbor_duplicates, "also-user", 9));
        MDB_stat main_stat;
        OK(mdb_stat(txn, main_db, &main_stat));
        REQUIRE(main_stat.ms_entries >= 5); /* Other flag scenarios add names. */
        MDB_val catalog_key = {11 - neighbor_plain, (void *)"plain"}, catalog_value;
        OK(mdb_get(txn, main_db, &catalog_key, &catalog_value));
        REQUIRE(catalog_value.mv_size > 0);
        REQUIRE(matches(txn, plain, "hello", 5, "world", 5));
        REQUIRE(matches(txn, plain, binary_key, sizeof binary_key, binary_value, sizeof binary_value));
        REQUIRE(matches(txn, plain, "empty", 5, "", 0));
        REQUIRE(matches(txn, duplicates, "group", 5, "first", 5));
        MDB_cursor *cursor;
        MDB_val key = text("group"), value;
        OK(mdb_cursor_open(txn, duplicates, &cursor));
        OK(mdb_cursor_get(cursor, &key, &value, MDB_SET));
        size_t count;
        OK(mdb_cursor_count(cursor, &count));
        REQUIRE(count == 3);
        mdb_cursor_close(cursor);
    }
    if (strcmp(action, "seed-flags") == 0 || strcmp(action, "check-flags") == 0) {
        MDB_dbi integer, fixed, reverse;
        OK(mdb_dbi_open(txn, "integer", seed ? MDB_CREATE | MDB_INTEGERKEY : 0, &integer));
        OK(mdb_dbi_open(txn, "fixed", seed ? MDB_CREATE | MDB_DUPSORT | MDB_DUPFIXED | MDB_INTEGERDUP : 0, &fixed));
        OK(mdb_dbi_open(txn, "reverse", seed ? MDB_CREATE | MDB_REVERSEKEY : 0, &reverse));
        uint32_t one = 1, two = 2, big = 256;
        if (seed) {
            OK(put(txn, integer, &big, sizeof big, "big", 3));
            OK(put(txn, integer, &one, sizeof one, "one", 3));
            OK(put(txn, fixed, "group", 5, &big, sizeof big));
            OK(put(txn, fixed, "group", 5, &one, sizeof one));
            OK(put(txn, fixed, "group", 5, &two, sizeof two));
            OK(put(txn, reverse, "az", 2, "az", 2));
            OK(put(txn, reverse, "za", 2, "za", 2));
        } else {
            unsigned flags;
            OK(mdb_dbi_flags(txn, integer, &flags));
            REQUIRE(flags == MDB_INTEGERKEY);
            OK(mdb_dbi_flags(txn, fixed, &flags));
            REQUIRE(flags == (MDB_DUPSORT | MDB_DUPFIXED | MDB_INTEGERDUP));
            REQUIRE(matches(txn, integer, &big, sizeof big, "big", 3));
            REQUIRE(matches(txn, fixed, "group", 5, &one, sizeof one));
            MDB_cursor *cursor;
            MDB_val key, value;
            OK(mdb_cursor_open(txn, integer, &cursor));
            OK(mdb_cursor_get(cursor, &key, &value, MDB_FIRST));
            REQUIRE(key.mv_size == sizeof one && memcmp(key.mv_data, &one, sizeof one) == 0);
            mdb_cursor_close(cursor);
            OK(mdb_cursor_open(txn, reverse, &cursor));
            OK(mdb_cursor_get(cursor, &key, &value, MDB_FIRST));
            REQUIRE(key.mv_size == 2 && memcmp(key.mv_data, "za", 2) == 0);
            mdb_cursor_close(cursor);
        }
    }
    if (update) {
        OK(put(txn, plain, "adapter", 7, "native-ulmdb", 12));
    } else if (strcmp(action, "check-updated") == 0) {
        REQUIRE(matches(txn, plain, "adapter", 7, "native-ulmdb", 12));
    }
    if (seed || update) OK(mdb_txn_commit(txn)); else mdb_txn_abort(txn);
    mdb_env_close(env);
    puts("application-ok");
    return 0;
}
