/* This exact source is compiled unchanged against LMDB and ULMDB. */
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define REQUIRE(condition) do { if (!(condition)) { \
    fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #condition); exit(1); \
} } while (0)
#define EXPECT(expression, expected) do { int result_ = (expression); \
    if (result_ != (expected)) { \
        fprintf(stderr, "%s:%d: %s returned %d (%s), expected %d\n", \
                __FILE__, __LINE__, #expression, result_, mdb_strerror(result_), (expected)); \
        exit(1); \
    } \
} while (0)
#define OK(expression) EXPECT((expression), MDB_SUCCESS)

static MDB_val value(const void *bytes, size_t size) {
    MDB_val result = {size, (void *)bytes};
    return result;
}

static MDB_val text(const char *bytes) { return value(bytes, strlen(bytes)); }

static void equal_at(MDB_val actual, const void *bytes, size_t size, int line) {
    if (actual.mv_size != size || (size && memcmp(actual.mv_data, bytes, size))) {
        fprintf(stderr, "%s:%d: value mismatch (actual %zu bytes, expected %zu)\n",
                __FILE__, line, actual.mv_size, size);
        exit(1);
    }
}
#define equal(actual, bytes, size) equal_at((actual), (bytes), (size), __LINE__)

static void put(MDB_txn *txn, MDB_dbi dbi, const char *key, const char *data) {
    MDB_val k = text(key), v = text(data);
    OK(mdb_put(txn, dbi, &k, &v, 0));
}

static void get(MDB_txn *txn, MDB_dbi dbi, const char *key, const char *expected) {
    MDB_val k = text(key), v = {0, NULL};
    OK(mdb_get(txn, dbi, &k, &v));
    equal(v, expected, strlen(expected));
}

static void missing(MDB_txn *txn, MDB_dbi dbi, const char *key) {
    MDB_val k = text(key), v = {0, NULL};
    EXPECT(mdb_get(txn, dbi, &k, &v), MDB_NOTFOUND);
}

static void position(MDB_cursor *cursor, MDB_cursor_op operation,
                     const char *key, const char *data) {
    /* DUP operations may leave the key argument untouched in upstream LMDB. */
    MDB_val k = text(key), v = {0, NULL};
    OK(mdb_cursor_get(cursor, &k, &v, operation));
    equal(k, key, strlen(key));
    equal(v, data, strlen(data));
}

static void hex(MDB_val bytes) {
    const unsigned char *data = bytes.mv_data;
    for (size_t i = 0; i < bytes.mv_size; ++i) printf("%02x", data[i]);
}

static void dump(MDB_txn *txn, MDB_dbi dbi, const char *label) {
    MDB_cursor *cursor;
    MDB_val key = {0, NULL}, data = {0, NULL};
    int result;
    OK(mdb_cursor_open(txn, dbi, &cursor));
    result = mdb_cursor_get(cursor, &key, &data, MDB_FIRST);
    while (result == MDB_SUCCESS) {
        printf("%s ", label);
        hex(key);
        putchar('=');
        hex(data);
        putchar('\n');
        result = mdb_cursor_get(cursor, &key, &data, MDB_NEXT);
    }
    REQUIRE(result == MDB_NOTFOUND);
    mdb_cursor_close(cursor);
}

static MDB_env *open_environment(const char *path, unsigned flags) {
    MDB_env *env;
    unsigned readers;
    const char *reported_path;
    OK(mdb_env_create(&env));
    OK(mdb_env_set_mapsize(env, 64u * 1024u * 1024u));
    OK(mdb_env_set_maxdbs(env, 16));
    OK(mdb_env_set_maxreaders(env, 32));
    OK(mdb_env_get_maxreaders(env, &readers));
    REQUIRE(readers == 32);
    OK(mdb_env_open(env, path, flags | MDB_NOTLS, 0600));
    OK(mdb_env_get_path(env, &reported_path));
    REQUIRE(strcmp(path, reported_path) == 0);
    REQUIRE(mdb_env_get_maxkeysize(env) >= 511);
    return env;
}

int main(int argc, char **argv) {
    REQUIRE(argc == 2);
    MDB_env *env = open_environment(argv[1], 0);
    MDB_txn *write, *read, *child;
    MDB_dbi main_db, kv, duplicates, scratch;
    MDB_cursor *cursor, *renewed;
    MDB_stat stat;
    unsigned flags;
    MDB_val key, data;
    const unsigned char binary_key[] = {0, 255, 0};
    const unsigned char binary_value[] = {255, 0, 128, 1};

    OK(mdb_env_set_userctx(env, env));
    REQUIRE(mdb_env_get_userctx(env) == env);
    OK(mdb_txn_begin(env, NULL, 0, &write));
    REQUIRE(mdb_txn_env(write) == env);
    OK(mdb_dbi_open(write, NULL, 0, &main_db));
    OK(mdb_dbi_open(write, "kv", MDB_CREATE, &kv));
    OK(mdb_dbi_open(write, "duplicates", MDB_CREATE | MDB_DUPSORT, &duplicates));
    OK(mdb_dbi_flags(write, duplicates, &flags));
    REQUIRE(flags == MDB_DUPSORT);
    put(write, main_db, "main", "main-value");
    put(write, kv, "a", "one");
    put(write, kv, "b", "two");
    put(write, kv, "d", "four");
    put(write, kv, "empty", "");
    key = value(binary_key, sizeof binary_key);
    data = value(binary_value, sizeof binary_value);
    OK(mdb_put(write, kv, &key, &data, 0));
    key = text("a");
    data = text("replacement");
    EXPECT(mdb_put(write, kv, &key, &data, MDB_NOOVERWRITE), MDB_KEYEXIST);
    equal(data, "one", 3);
    put(write, duplicates, "group", "c");
    put(write, duplicates, "group", "a");
    put(write, duplicates, "group", "b");
    put(write, duplicates, "group", "b");
    put(write, duplicates, "other", "x");
    key = text("group");
    data = text("b");
    EXPECT(mdb_put(write, duplicates, &key, &data, MDB_NODUPDATA), MDB_KEYEXIST);
    OK(mdb_txn_commit(write));

    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    get(read, kv, "a", "one");
    get(read, kv, "empty", "");
    missing(read, kv, "missing");
    key = value(binary_key, sizeof binary_key);
    OK(mdb_get(read, kv, &key, &data));
    equal(data, binary_value, sizeof binary_value);
    OK(mdb_stat(read, kv, &stat));
    REQUIRE(stat.ms_entries == 5);
    OK(mdb_stat(read, duplicates, &stat));
    REQUIRE(stat.ms_entries == 4);

    OK(mdb_cursor_open(read, kv, &cursor));
    REQUIRE(mdb_cursor_txn(cursor) == read);
    REQUIRE(mdb_cursor_dbi(cursor) == kv);
    OK(mdb_cursor_get(cursor, &key, &data, MDB_FIRST));
    equal(key, binary_key, sizeof binary_key);
    position(cursor, MDB_NEXT, "a", "one");
    OK(mdb_cursor_get(cursor, &key, &data, MDB_PREV));
    equal(key, binary_key, sizeof binary_key);
    position(cursor, MDB_LAST, "empty", "");
    key = text("c");
    OK(mdb_cursor_get(cursor, &key, &data, MDB_SET_RANGE));
    equal(key, "d", 1);
    equal(data, "four", 4);
    key = text("b");
    OK(mdb_cursor_get(cursor, &key, &data, MDB_SET));
    position(cursor, MDB_GET_CURRENT, "b", "two");
    key = text("z");
    EXPECT(mdb_cursor_get(cursor, &key, &data, MDB_SET_RANGE), MDB_NOTFOUND);
    mdb_cursor_close(cursor);

    OK(mdb_cursor_open(read, duplicates, &cursor));
    position(cursor, MDB_FIRST, "group", "a");
    position(cursor, MDB_LAST_DUP, "group", "c");
    position(cursor, MDB_PREV_DUP, "group", "b");
    position(cursor, MDB_NEXT_DUP, "group", "c");
    EXPECT(mdb_cursor_get(cursor, &key, &data, MDB_NEXT_DUP), MDB_NOTFOUND);
    position(cursor, MDB_NEXT_NODUP, "other", "x");
    position(cursor, MDB_PREV_NODUP, "group", "c");
    key = text("group");
    data = text("b");
    OK(mdb_cursor_get(cursor, &key, &data, MDB_GET_BOTH));
    equal(data, "b", 1);
    key = text("group");
    data = text("bb");
    OK(mdb_cursor_get(cursor, &key, &data, MDB_GET_BOTH_RANGE));
    equal(data, "c", 1);
    size_t count;
    OK(mdb_cursor_count(cursor, &count));
    REQUIRE(count == 3);
    position(cursor, MDB_FIRST_DUP, "group", "a");
    mdb_cursor_close(cursor);
    dump(read, kv, "initial-kv");
    dump(read, duplicates, "initial-dups");

    /* A live read snapshot and a borrowed result survive nested writer commits. */
    key = text("a");
    OK(mdb_get(read, kv, &key, &data));
    MDB_val borrowed = data;
    OK(mdb_cursor_open(read, kv, &renewed));
    OK(mdb_txn_begin(env, NULL, 0, &write));
    put(write, kv, "a", "updated");
    put(write, kv, "outer", "kept");
    OK(mdb_txn_begin(env, write, 0, &child));
    put(child, kv, "a", "aborted");
    put(child, kv, "aborted-child", "absent");
    mdb_txn_abort(child);
    get(write, kv, "a", "updated");
    missing(write, kv, "aborted-child");
    OK(mdb_txn_begin(env, write, 0, &child));
    put(child, kv, "child", "kept");
    OK(mdb_txn_commit(child));
    get(write, kv, "child", "kept");
    OK(mdb_txn_commit(write));
    equal(borrowed, "one", 3);
    get(read, kv, "a", "one");
    missing(read, kv, "child");
    mdb_txn_reset(read);
    OK(mdb_txn_renew(read));
    OK(mdb_cursor_renew(read, renewed));
    key = text("a");
    OK(mdb_cursor_get(renewed, &key, &data, MDB_SET));
    equal(data, "updated", 7);
    mdb_cursor_close(renewed);
    get(read, kv, "child", "kept");
    mdb_txn_abort(read);

    OK(mdb_txn_begin(env, NULL, 0, &write));
    put(write, kv, "aborted-writer", "absent");
    mdb_txn_abort(write);
    OK(mdb_txn_begin(env, NULL, 0, &write));
    missing(write, kv, "aborted-writer");
    key = text("b");
    OK(mdb_del(write, kv, &key, NULL));
    EXPECT(mdb_del(write, kv, &key, NULL), MDB_NOTFOUND);
    key = text("group");
    data = text("b");
    OK(mdb_del(write, duplicates, &key, &data));
    EXPECT(mdb_del(write, duplicates, &key, &data), MDB_NOTFOUND);
    OK(mdb_cursor_open(write, duplicates, &cursor));
    position(cursor, MDB_FIRST, "group", "a");
    OK(mdb_cursor_del(cursor, 0));
    position(cursor, MDB_NEXT, "group", "c");
    mdb_cursor_close(cursor);
    OK(mdb_cursor_open(write, kv, &cursor));
    key = text("a");
    OK(mdb_cursor_get(cursor, &key, &data, MDB_SET));
    key = text("a");
    data = text("cursor");
    OK(mdb_cursor_put(cursor, &key, &data, MDB_CURRENT));
    mdb_cursor_close(cursor);
    get(write, kv, "a", "cursor");
    OK(mdb_dbi_open(write, "scratch", MDB_CREATE, &scratch));
    put(write, scratch, "temporary", "value");
    OK(mdb_drop(write, scratch, 0));
    OK(mdb_stat(write, scratch, &stat));
    REQUIRE(stat.ms_entries == 0);
    put(write, scratch, "temporary", "again");
    OK(mdb_drop(write, scratch, 1));
    EXPECT(mdb_dbi_open(write, "scratch", 0, &scratch), MDB_NOTFOUND);
    OK(mdb_txn_commit(write));

    /* Aborted provisional database handles must not consume maxdbs forever. */
    for (unsigned i = 0; i < 40; ++i) {
        OK(mdb_txn_begin(env, NULL, 0, &write));
        OK(mdb_dbi_open(write, "aborted-slot", MDB_CREATE, &scratch));
        put(write, scratch, "key", "aborted");
        mdb_txn_abort(write);
    }
    /* Exceeding maxdbs must not create the database behind a failed open. */
    OK(mdb_txn_begin(env, NULL, 0, &write));
    MDB_dbi first_capacity = 0;
    for (unsigned i = 0; i < 14; ++i) {
        char name[32];
        snprintf(name, sizeof name, "capacity-%02u", i);
        OK(mdb_dbi_open(write, name, MDB_CREATE, &scratch));
        if (i == 0) first_capacity = scratch;
    }
    EXPECT(mdb_dbi_open(write, "overflow", MDB_CREATE, &scratch), MDB_DBS_FULL);
    /* LMDB checks handle capacity even when opening an absent name read-only. */
    OK(mdb_drop(write, first_capacity, 1));
    EXPECT(mdb_dbi_open(write, "overflow", 0, &scratch), MDB_NOTFOUND);
    mdb_txn_abort(write);

    /* APPENDDUP orders data for one key, independently of global key order. */
    OK(mdb_txn_begin(env, NULL, 0, &write));
    OK(mdb_dbi_open(write, "append-dups", MDB_CREATE | MDB_DUPSORT, &scratch));
    put(write, scratch, "a", "1");
    put(write, scratch, "z", "1");
    key = text("a");
    data = text("2");
    OK(mdb_put(write, scratch, &key, &data, MDB_APPENDDUP));
    data = text("1");
    EXPECT(mdb_put(write, scratch, &key, &data, MDB_APPENDDUP), MDB_KEYEXIST);
    key = text("b");
    OK(mdb_put(write, scratch, &key, &data, MDB_APPENDDUP));
    key = text("z");
    data = text("2");
    OK(mdb_put(write, scratch, &key, &data, MDB_APPENDDUP));
    EXPECT(mdb_put(write, scratch, &key, &data, MDB_APPENDDUP), MDB_KEYEXIST);
    key = text("x");
    EXPECT(mdb_put(write, scratch, &key, &data, MDB_APPEND), MDB_KEYEXIST);
    key = text("zz");
    OK(mdb_put(write, scratch, &key, &data, MDB_APPEND | MDB_APPENDDUP));
    /* Ordinary KV does not impose data append ordering for APPENDDUP. */
    key = text("a");
    data = text("temporary-overwrite");
    OK(mdb_put(write, kv, &key, &data, MDB_APPENDDUP));
    key = text("0-before-first-text-key");
    OK(mdb_put(write, kv, &key, &data, MDB_APPENDDUP));
    get(write, kv, "a", "temporary-overwrite");
    mdb_txn_abort(write);

    /* Same-size CURRENT duplicate replacement preserves strict sorted position.
       Keep this scenario rollback-only so the persisted transcript is stable. */
    OK(mdb_txn_begin(env, NULL, 0, &write));
    OK(mdb_dbi_open(write, "current-dups", MDB_CREATE | MDB_DUPSORT, &scratch));
    put(write, scratch, "key", "aa");
    put(write, scratch, "key", "mm");
    put(write, scratch, "key", "zz");
    OK(mdb_cursor_open(write, scratch, &cursor));
    key = text("key");
    data = text("mm");
    OK(mdb_cursor_get(cursor, &key, &data, MDB_GET_BOTH));
    key = text("key");
    data = text("nn");
    OK(mdb_cursor_put(cursor, &key, &data, MDB_CURRENT));
    position(cursor, MDB_GET_CURRENT, "key", "nn");
    position(cursor, MDB_PREV_DUP, "key", "aa");
    position(cursor, MDB_NEXT_DUP, "key", "nn");
    position(cursor, MDB_NEXT_DUP, "key", "zz");
    position(cursor, MDB_PREV_DUP, "key", "nn");
    key = text("key");
    data = text("bb");
    OK(mdb_cursor_put(cursor, &key, &data, MDB_CURRENT));
    position(cursor, MDB_GET_CURRENT, "key", "bb");
    data = text("bb");
    OK(mdb_cursor_put(cursor, &key, &data, MDB_CURRENT));
    OK(mdb_cursor_count(cursor, &count));
    REQUIRE(count == 3);
    position(cursor, MDB_FIRST_DUP, "key", "aa");
    /* CURRENT may receive bytes borrowed from this writer's own storage. */
    key = text("key");
    OK(mdb_get(write, scratch, &key, &data));
    OK(mdb_cursor_put(cursor, &key, &data, MDB_CURRENT));
    position(cursor, MDB_GET_CURRENT, "key", "aa");
    key = text("key");
    data = text("ab");
    OK(mdb_cursor_put(cursor, &key, &data, MDB_CURRENT));
    position(cursor, MDB_GET_CURRENT, "key", "ab");
    position(cursor, MDB_LAST_DUP, "key", "zz");
    key = text("key");
    data = text("zy");
    OK(mdb_cursor_put(cursor, &key, &data, MDB_CURRENT));
    position(cursor, MDB_GET_CURRENT, "key", "zy");
    OK(mdb_cursor_count(cursor, &count));
    REQUIRE(count == 3);
    mdb_cursor_close(cursor);
    mdb_txn_abort(write);

    /* Committing a parent commits its active writer child and frees cursors. */
    OK(mdb_txn_begin(env, NULL, 0, &write));
    OK(mdb_cursor_open(write, kv, &cursor));
    key = text("a");
    OK(mdb_cursor_get(cursor, &key, &data, MDB_SET));
    OK(mdb_txn_begin(env, write, 0, &child));
    put(child, kv, "a", "cursor");
    OK(mdb_cursor_open(child, kv, &cursor));
    key = text("a");
    OK(mdb_cursor_get(cursor, &key, &data, MDB_SET));
    OK(mdb_txn_commit(write));
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    get(read, kv, "a", "cursor");
    mdb_txn_abort(read);
    OK(mdb_env_sync(env, 1));
    mdb_env_close(env);

    env = open_environment(argv[1], MDB_RDONLY);
    /* LMDB 0.9.x uses EACCES; LMDB 1.0.2 introduces MDB_IS_READONLY. */
    REQUIRE(mdb_txn_begin(env, NULL, 0, &write) != MDB_SUCCESS);
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    OK(mdb_dbi_open(read, NULL, 0, &main_db));
    OK(mdb_dbi_open(read, "kv", 0, &kv));
    OK(mdb_dbi_open(read, "duplicates", 0, &duplicates));
    get(read, main_db, "main", "main-value");
    get(read, kv, "a", "cursor");
    missing(read, kv, "b");
    missing(read, kv, "aborted-child");
    missing(read, kv, "aborted-writer");
    key = value(binary_key, sizeof binary_key);
    OK(mdb_get(read, kv, &key, &data));
    equal(data, binary_value, sizeof binary_value);
    OK(mdb_stat(read, kv, &stat));
    REQUIRE(stat.ms_entries == 6);
    OK(mdb_stat(read, duplicates, &stat));
    REQUIRE(stat.ms_entries == 2);
    dump(read, kv, "final-kv");
    dump(read, duplicates, "final-dups");
    mdb_txn_abort(read);
    mdb_dbi_close(env, duplicates);
    mdb_dbi_close(env, kv);
    mdb_env_close(env);
    puts("compatibility-ok");
    return 0;
}
