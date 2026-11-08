#include <lmdb.h>
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define REQUIRE(condition) do { if (!(condition)) { \
    fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #condition); exit(1); \
} } while (0)
#define OK(expression) do { int result_ = (expression); if (result_ != 0) { \
    fprintf(stderr, "%s:%d: %s: %d (%s)\n", __FILE__, __LINE__, #expression, \
            result_, mdb_strerror(result_)); exit(1); \
} } while (0)

static MDB_val text(const char *bytes) {
    MDB_val result = {strlen(bytes), (void *)bytes};
    return result;
}

static void equals(MDB_val value, const char *expected) {
    REQUIRE(value.mv_size == strlen(expected));
    REQUIRE(value.mv_size == 0 || memcmp(value.mv_data, expected, value.mv_size) == 0);
}

static MDB_env *create_environment(const char *path, MDB_dbi *db, unsigned flags) {
    MDB_env *env;
    MDB_txn *txn;
    MDB_val key = text("a"), value = text("value-a");
    OK(mdb_env_create(&env));
    OK(mdb_env_set_maxdbs(env, 4));
    OK(mdb_env_open(env, path, flags, 0600));
    OK(mdb_txn_begin(env, NULL, 0, &txn));
    OK(mdb_dbi_open(txn, "items", MDB_CREATE, db));
    OK(mdb_put(txn, *db, &key, &value, 0));
    key = text("b");
    value = text("value-b");
    OK(mdb_put(txn, *db, &key, &value, 0));
    OK(mdb_txn_commit(txn));
    return env;
}

static void *foreign_cursor_close(void *cursor) {
    /* Foreign-thread access is rejected without consuming the live cursor. */
    mdb_cursor_close((MDB_cursor *)cursor);
    return NULL;
}

struct migrated_reader {
    MDB_txn *txn;
    MDB_cursor *cursor;
    MDB_dbi db;
    int commit;
};

static void *migrate_notls_reader(void *argument) {
    struct migrated_reader *reader = argument;
    MDB_val key = text("a"), value;
    OK(mdb_get(reader->txn, reader->db, &key, &value));
    equals(value, "value-a");
    OK(mdb_cursor_get(reader->cursor, &key, &value, MDB_FIRST));
    mdb_txn_reset(reader->txn);
    OK(mdb_txn_renew(reader->txn));
    OK(mdb_cursor_renew(reader->txn, reader->cursor));
    OK(mdb_cursor_get(reader->cursor, &key, &value, MDB_LAST));
    equals(value, "value-b");
    mdb_cursor_close(reader->cursor);
    if (reader->commit) OK(mdb_txn_commit(reader->txn));
    else mdb_txn_abort(reader->txn);
    return NULL;
}

static void nested_database_handles(MDB_env *env) {
    MDB_txn *parent, *child, *read;
    MDB_dbi provisional, persistent, replacement;
    MDB_val key = text("key"), value = text("original"), found;

    /* Releasing a child does not publish a newly created database if its
       parent subsequently aborts. */
    OK(mdb_txn_begin(env, NULL, 0, &parent));
    OK(mdb_txn_begin(env, parent, 0, &child));
    OK(mdb_dbi_open(child, "temporary-nested", MDB_CREATE, &provisional));
    OK(mdb_put(child, provisional, &key, &value, 0));
    OK(mdb_txn_commit(child));
    mdb_txn_abort(parent);
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    REQUIRE(mdb_dbi_open(read, "temporary-nested", 0, &provisional) == MDB_NOTFOUND);
    mdb_txn_abort(read);

    OK(mdb_txn_begin(env, NULL, 0, &parent));
    OK(mdb_dbi_open(parent, "persistent-nested", MDB_CREATE, &persistent));
    OK(mdb_put(parent, persistent, &key, &value, 0));
    OK(mdb_txn_commit(parent));

    /* A child abort restores both the catalog entry and its committed DBI. */
    OK(mdb_txn_begin(env, NULL, 0, &parent));
    OK(mdb_txn_begin(env, parent, 0, &child));
    OK(mdb_drop(child, persistent, 1));
    mdb_txn_abort(child);
    OK(mdb_get(parent, persistent, &key, &found));
    equals(found, "original");
    mdb_txn_abort(parent);

    /* A released child may drop and recreate the same name, but parent abort
       must preserve the old committed handle and discard the replacement. */
    OK(mdb_txn_begin(env, NULL, 0, &parent));
    OK(mdb_txn_begin(env, parent, 0, &child));
    OK(mdb_drop(child, persistent, 1));
    OK(mdb_dbi_open(child, "persistent-nested", MDB_CREATE, &replacement));
    value = text("replacement");
    OK(mdb_put(child, replacement, &key, &value, 0));
    OK(mdb_txn_commit(child));
    mdb_txn_abort(parent);
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    OK(mdb_get(read, persistent, &key, &found));
    equals(found, "original");
    mdb_txn_abort(read);

    /* A committed parent closes the handle dropped by its released child. */
    OK(mdb_txn_begin(env, NULL, 0, &parent));
    OK(mdb_txn_begin(env, parent, 0, &child));
    OK(mdb_drop(child, persistent, 1));
    OK(mdb_txn_commit(child));
    OK(mdb_txn_commit(parent));
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    REQUIRE(mdb_get(read, persistent, &key, &found) == MDB_BAD_DBI);
    mdb_txn_abort(read);
}

static void duplicate_current_limits(MDB_env *env) {
    MDB_txn *writer;
    MDB_cursor *cursor;
    MDB_dbi db;
    MDB_val key = text("key"), data, found_key, found_value;
    const char *initial[] = {"aa", "mm", "zz"};
    OK(mdb_txn_begin(env, NULL, 0, &writer));
    OK(mdb_dbi_open(writer, "current-limits", MDB_CREATE | MDB_DUPSORT, &db));
    for (size_t index = 0; index < 3; ++index) {
        data = text(initial[index]);
        OK(mdb_put(writer, db, &key, &data, 0));
    }
    OK(mdb_cursor_open(writer, db, &cursor));
    data = text("mm");
    OK(mdb_cursor_get(cursor, &key, &data, MDB_GET_BOTH));
    const char *reordered[] = {"00", "aa", "zz", "~~", "zy"};
    for (size_t index = 0; index < 5; ++index) {
        key = text("key");
        data = text(reordered[index]);
        /* zy remains between aa and zz, so is a valid replacement. */
        if (index == 4) {
            OK(mdb_cursor_put(cursor, &key, &data, MDB_CURRENT));
        } else {
            REQUIRE(mdb_cursor_put(cursor, &key, &data, MDB_CURRENT) == EINVAL);
        }
        OK(mdb_cursor_get(cursor, &found_key, &found_value, MDB_GET_CURRENT));
        equals(found_value, index == 4 ? "zy" : "mm");
    }
    key = text("key");
    data = text("m");
    OK(mdb_cursor_put(cursor, &key, &data, MDB_CURRENT));
    data = text("mmmm");
    OK(mdb_cursor_put(cursor, &key, &data, MDB_CURRENT));
    size_t count;
    OK(mdb_cursor_count(cursor, &count));
    REQUIRE(count == 3);
    OK(mdb_cursor_get(cursor, &found_key, &found_value, MDB_FIRST));
    equals(found_value, "aa");
    OK(mdb_cursor_get(cursor, &found_key, &found_value, MDB_NEXT));
    equals(found_value, "mmmm");
    OK(mdb_cursor_get(cursor, &found_key, &found_value, MDB_NEXT));
    equals(found_value, "zz");
    mdb_cursor_close(cursor);
    mdb_txn_abort(writer);
}

int main(int argc, char **argv) {
    REQUIRE(argc == 3);
    MDB_dbi db_a, db_b;
    /* Multiple snapshots on this thread require transaction-owned reader slots. */
    MDB_env *env_a = create_environment(argv[1], &db_a, MDB_NOTLS);
    MDB_env *env_b = create_environment(argv[2], &db_b, 0);
    MDB_txn *read_a, *read_b, *read_next, *writer;
    MDB_cursor *cursor, *other;
    MDB_val key, value, retained_key, retained_value;

    OK(mdb_txn_begin(env_a, NULL, MDB_RDONLY, &read_a));
    OK(mdb_cursor_open(read_a, db_a, &cursor));
    OK(mdb_cursor_get(cursor, &key, &value, MDB_FIRST));
    equals(key, "a");
    equals(value, "value-a");
    REQUIRE(mdb_cursor_get(cursor, &key, &value, MDB_FIRST_DUP) == MDB_INCOMPATIBLE);
    REQUIRE(mdb_cursor_get(cursor, &key, &value, MDB_LAST_DUP) == MDB_INCOMPATIBLE);
    REQUIRE(mdb_cursor_get(cursor, &key, &value, MDB_GET_BOTH) == MDB_INCOMPATIBLE);
    REQUIRE(mdb_cursor_get(cursor, &key, &value, MDB_GET_BOTH_RANGE) == MDB_INCOMPATIBLE);
    REQUIRE(mdb_cursor_get(cursor, &key, &value, MDB_NEXT_DUP) == MDB_NOTFOUND);
    REQUIRE(mdb_cursor_get(cursor, &key, &value, MDB_PREV_DUP) == MDB_NOTFOUND);

    /* Renewing on the same live txn keeps buffers from prior cursor reads valid. */
    retained_key = key;
    retained_value = value;
    OK(mdb_cursor_renew(read_a, cursor));
    equals(retained_key, "a");
    equals(retained_value, "value-a");
    OK(mdb_cursor_get(cursor, &key, &value, MDB_LAST));
    equals(key, "b");

    /* The common reset + renew cycle reuses the exact same MDB_txn address. */
    mdb_txn_reset(read_a);
    OK(mdb_txn_renew(read_a));
    OK(mdb_cursor_renew(read_a, cursor));
    OK(mdb_cursor_get(cursor, &key, &value, MDB_FIRST));
    equals(value, "value-a");

    /* Reject another environment even when both have the same numeric DBI. */
    OK(mdb_txn_begin(env_b, NULL, MDB_RDONLY, &read_b));
    REQUIRE(mdb_cursor_renew(read_b, cursor) == EINVAL);
    REQUIRE(mdb_cursor_txn(cursor) == read_a);
    OK(mdb_cursor_get(cursor, &key, &value, MDB_LAST));
    equals(value, "value-b");

    pthread_t thread;
    MDB_cursor *thread_cursor;
    OK(mdb_cursor_open(read_b, db_b, &thread_cursor));
    REQUIRE(pthread_create(&thread, NULL, foreign_cursor_close, thread_cursor) == 0);
    REQUIRE(pthread_join(thread, NULL) == 0);
    OK(mdb_cursor_get(thread_cursor, &key, &value, MDB_FIRST));
    mdb_cursor_close(thread_cursor);
    OK(mdb_cursor_get(cursor, &key, &value, MDB_FIRST));

    /* Transfer to a second snapshot in the same environment. The old snapshot
       still owns buffers returned before the transfer. */
    retained_key = key;
    retained_value = value;
    OK(mdb_txn_begin(env_a, NULL, MDB_RDONLY, &read_next));
    OK(mdb_cursor_renew(read_next, cursor));
    equals(retained_key, "a");
    equals(retained_value, "value-a");
    mdb_txn_abort(read_a);
    OK(mdb_cursor_get(cursor, &key, &value, MDB_FIRST));

    /* Cursor close releases its registry entry but returned bytes survive. */
    retained_key = key;
    retained_value = value;
    mdb_cursor_close(cursor);
    equals(retained_key, "a");
    equals(retained_value, "value-a");
    OK(mdb_cursor_open(read_next, db_a, &other));
    OK(mdb_cursor_get(other, &key, &value, MDB_LAST));
    equals(retained_value, "value-a");
    OK(mdb_txn_commit(read_next));

    /* Read cursors remain closable and renewable after their txn ends. */
    OK(mdb_txn_begin(env_a, NULL, MDB_RDONLY, &read_a));
    OK(mdb_cursor_renew(read_a, other));
    OK(mdb_cursor_get(other, &key, &value, MDB_FIRST));
    equals(value, "value-a");
    mdb_txn_abort(read_a);
    mdb_cursor_close(other);
    mdb_txn_abort(read_b);

    /* Write cursors may be explicitly closed or left for transaction cleanup. */
    for (unsigned int index = 0; index < 64; ++index) {
        OK(mdb_txn_begin(env_a, NULL, 0, &writer));
        OK(mdb_cursor_open(writer, db_a, &cursor));
        OK(mdb_cursor_get(cursor, &key, &value, MDB_FIRST));
        if (index == 0) {
            /* KV cursor identity is the key even when another API replaces its
               value with bytes that sort before or after the previous value. */
            MDB_val replace_key = text("a"), replace_value = text("0");
            OK(mdb_put(writer, db_a, &replace_key, &replace_value, 0));
            OK(mdb_cursor_get(cursor, &key, &value, MDB_GET_CURRENT));
            equals(value, "0");
            OK(mdb_cursor_get(cursor, &key, &value, MDB_NEXT));
            equals(key, "b");
            replace_key = text("b");
            replace_value = text("zzz");
            OK(mdb_put(writer, db_a, &replace_key, &replace_value, 0));
            OK(mdb_cursor_get(cursor, &key, &value, MDB_GET_CURRENT));
            equals(value, "zzz");
            OK(mdb_cursor_get(cursor, &key, &value, MDB_PREV));
            equals(key, "a");
            replace_key = text("a");
            replace_value = text("value-a");
            OK(mdb_put(writer, db_a, &replace_key, &replace_value, 0));
            replace_key = text("b");
            replace_value = text("value-b");
            OK(mdb_put(writer, db_a, &replace_key, &replace_value, 0));
        }
        if (index % 2 == 0) mdb_cursor_close(cursor);
        OK(mdb_txn_commit(writer));
    }

    nested_database_handles(env_a);
    duplicate_current_limits(env_a);

    mdb_env_close(env_a);
    mdb_env_close(env_b);

    /* MDB_NOTLS permits sequential reader/cursor migration to another thread. */
    OK(mdb_env_create(&env_a));
    OK(mdb_env_set_maxdbs(env_a, 4));
    OK(mdb_env_open(env_a, argv[1], MDB_NOTLS, 0600));
    for (int commit = 0; commit < 2; ++commit) {
        struct migrated_reader reader;
        OK(mdb_txn_begin(env_a, NULL, MDB_RDONLY, &reader.txn));
        OK(mdb_dbi_open(reader.txn, "items", 0, &reader.db));
        OK(mdb_cursor_open(reader.txn, reader.db, &reader.cursor));
        reader.commit = commit;
        REQUIRE(pthread_create(&thread, NULL, migrate_notls_reader, &reader) == 0);
        REQUIRE(pthread_join(thread, NULL) == 0);
    }
    mdb_env_close(env_a);
    puts("lifetimes-ok");
    return 0;
}
