/* LMDB 1.0 readonly children read the parent's frozen, unpublished pages. */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <lmdb.h>
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

static void require(int condition, const char *operation) {
    if (!condition) { fprintf(stderr, "%s failed\n", operation); exit(1); }
}
static void ok(int code, const char *operation) {
    if (code) { fprintf(stderr, "%s: %s (%d)\n", operation, mdb_strerror(code), code); exit(1); }
}
static void put(MDB_txn *txn, MDB_dbi db, const char *key_text, const char *text) {
    MDB_val key = {strlen(key_text), (void *)key_text};
    MDB_val value = {strlen(text), (void *)text};
    ok(mdb_put(txn, db, &key, &value, 0), "put");
}
static MDB_val get(MDB_txn *txn, MDB_dbi db, const char *key_text, const char *expected) {
    MDB_val key = {strlen(key_text), (void *)key_text}, value;
    ok(mdb_get(txn, db, &key, &value), "get");
    require(value.mv_size == strlen(expected) && !memcmp(value.mv_data, expected, value.mv_size), "value");
    return value;
}
static void readonly_flag(MDB_txn *txn) {
    unsigned flags = 0;
    ok(mdb_txn_flags(txn, &flags), "txn_flags");
    require(flags == MDB_RDONLY, "readonly flag");
}
static void readers(MDB_env *env, const char *label) {
    MDB_envinfo info;
    unsigned flags;
    ok(mdb_env_info(env, &info), "env_info");
    ok(mdb_env_get_flags(env, &flags), "env_flags");
    printf("readers.%s=%u\n", label, info.me_numreaders);
    require(info.me_numreaders == (flags & MDB_NOLOCK ? 0u : 1u), "children consume no reader slots");
}
struct worker { MDB_txn *txn; MDB_dbi db; pthread_barrier_t *barrier; };
static void *thread_child(void *opaque) {
    struct worker *worker = opaque;
    (void)pthread_barrier_wait(worker->barrier);
    for (unsigned i = 0; i < 512; ++i)
        (void)get(worker->txn, worker->db, "key", "private");
    readonly_flag(worker->txn);
    mdb_txn_abort(worker->txn);
    return NULL;
}
static MDB_env *environment(const char *path, unsigned flags) {
    MDB_env *env;
    ok(mdb_env_create(&env), "env_create");
    ok(mdb_env_set_mapsize(env, 16u * 1024u * 1024u), "mapsize");
    ok(mdb_env_set_maxdbs(env, 8), "maxdbs");
    ok(mdb_env_set_maxreaders(env, 1), "maxreaders");
    ok(mdb_env_open(env, path, flags, 0600), "env_open");
    return env;
}
static void scenario(const char *path, unsigned flags) {
    MDB_env *env = environment(path, flags);
    MDB_txn *parent, *first, *second, *third, *reader, *nested;
    MDB_dbi db, duplicates, private_db;
    ok(mdb_txn_begin(env, NULL, 0, &parent), "seed_begin");
    ok(mdb_dbi_open(parent, "records", MDB_CREATE, &db), "seed_db");
    ok(mdb_dbi_open(parent, "duplicates", MDB_CREATE | MDB_DUPSORT, &duplicates), "seed_dups");
    put(parent, db, "key", "committed");
    ok(mdb_txn_commit(parent), "seed_commit");
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &reader), "occupy_reader_slot");
    readers(env, "before");
    ok(mdb_txn_begin(env, NULL, 0, &parent), "parent_begin");
    put(parent, db, "key", "private");
    put(parent, duplicates, "d", "c");
    put(parent, duplicates, "d", "a");
    put(parent, duplicates, "d", "b");
    ok(mdb_dbi_open(parent, "private", MDB_CREATE, &private_db), "private_db");
    put(parent, private_db, "key", "unpublished");
    MDB_val reserve_key = {8, "reserved"}, reserve = {8, NULL};
    ok(mdb_put(parent, db, &reserve_key, &reserve, MDB_RESERVE), "reserve");
    memcpy(reserve.mv_data, "reserved", 8);

    puts("scenario=G02-positive-private-sibling-views");
    ok(mdb_txn_begin(env, parent, MDB_RDONLY, &first), "first_child");
    ok(mdb_txn_begin(env, parent, MDB_RDONLY, &second), "second_child");
    require(mdb_txn_id(first) == mdb_txn_id(parent), "child transaction id");
    readonly_flag(first); readonly_flag(second);
    MDB_val pinned = get(first, db, "key", "private");
    MDB_val same_page = get(second, db, "key", "private");
    require(pinned.mv_data == same_page.mv_data, "siblings borrow the same private value");
    (void)get(second, db, "reserved", "reserved");
    (void)get(first, private_db, "key", "unpublished");
    readers(env, "siblings");
    MDB_cursor *cursor;
    ok(mdb_cursor_open(first, duplicates, &cursor), "child_cursor");
    MDB_val key = {1, "d"}, value;
    ok(mdb_cursor_get(cursor, &key, &value, MDB_SET), "cursor_set");
    size_t count = 0;
    ok(mdb_cursor_count(cursor, &count), "cursor_count");
    require(count == 3, "duplicate count");
    printf("child.duplicates=%zu:", count);
    for (unsigned i = 0; i < 3; ++i) {
        require(value.mv_size == 1, "duplicate width");
        printf("%c", *(char *)value.mv_data);
        if (i != 2) ok(mdb_cursor_get(cursor, &key, &value, MDB_NEXT_DUP), "cursor_next_dup");
    }
    putchar('\n');
    mdb_cursor_close(cursor);

    puts("scenario=G02-negative-parent-and-readonly-operations");
    key = (MDB_val){3, "key"}; value = (MDB_val){1, "x"};
    printf("parent.get=%d\n", mdb_get(parent, db, &key, &value));
    printf("parent.put=%d\n", mdb_put(parent, db, &key, &value, 0));
    printf("child.put=%d\n", mdb_put(first, db, &key, &value, 0));
    nested = NULL;
    printf("readonly-parent.read-child=%d\n", mdb_txn_begin(env, first, MDB_RDONLY, &nested));
    require(nested == NULL, "readonly parent produced child");
    nested = NULL;
    printf("readonly-parent.write-child=%d\n", mdb_txn_begin(env, first, 0, &nested));
    require(nested == NULL, "readonly parent produced writer");
    nested = NULL;
    printf("siblings.write-child=%d\n", mdb_txn_begin(env, parent, 0, &nested));
    require(nested == NULL, "write child alongside read children");
    mdb_txn_reset(first); /* Nested readonly reset is a documented no-op. */
    (void)get(first, db, "key", "private");
    printf("child.renew=%d\n", mdb_txn_renew(first));

    if (flags & MDB_NOTLS) {
        pthread_barrier_t barrier;
        require(!pthread_barrier_init(&barrier, NULL, 2), "barrier_init");
        struct worker worker = {second, db, &barrier};
        pthread_t thread;
        require(!pthread_create(&thread, NULL, thread_child, &worker), "pthread_create");
        (void)pthread_barrier_wait(&barrier);
        for (unsigned i = 0; i < 512; ++i)
            (void)get(first, db, "key", "private");
        require(!pthread_join(thread, NULL), "pthread_join");
        require(!pthread_barrier_destroy(&barrier), "barrier_destroy");
        puts("child.notls-concurrent-siblings=pass");
    } else {
        ok(mdb_txn_commit(second), "readonly_child_commit");
        puts("child.commit=pass");
    }
    require(pinned.mv_size == 7 && !memcmp(pinned.mv_data, "private", 7), "pinned value after sibling ends");
    ok(mdb_txn_begin(env, parent, MDB_RDONLY, &third), "replacement_sibling");
    (void)get(third, db, "key", "private");
    mdb_txn_abort(first);
    (void)get(third, db, "key", "private");
    mdb_txn_abort(third);
    readers(env, "children-ended");
    (void)get(parent, db, "key", "private");
    put(parent, db, "key", "published");
    ok(mdb_txn_commit(parent), "parent_commit");
    (void)get(reader, db, "key", "committed");
    mdb_txn_abort(reader);

    puts("scenario=G02-nested-writer-private-view");
    ok(mdb_txn_begin(env, NULL, 0, &parent), "outer_begin");
    put(parent, db, "key", "outer");
    if (flags & MDB_WRITEMAP) {
        nested = NULL;
        int code = mdb_txn_begin(env, parent, 0, &nested);
        printf("writemap.write-child=%d\n", code);
        require(code == EINVAL && nested == NULL, "writemap write child rejection");
        ok(mdb_txn_begin(env, parent, MDB_RDONLY, &first), "writemap_read_child");
        (void)get(first, db, "key", "outer");
        mdb_txn_abort(first);
    } else {
        ok(mdb_txn_begin(env, parent, 0, &nested), "nested_write_begin");
        put(nested, db, "key", "inner");
        ok(mdb_txn_begin(env, nested, MDB_RDONLY, &first), "grandchild_first");
        ok(mdb_txn_begin(env, nested, MDB_RDONLY, &second), "grandchild_second");
        (void)get(first, db, "key", "inner");
        (void)get(second, db, "key", "inner");
        mdb_txn_abort(first); mdb_txn_abort(second);
        mdb_txn_abort(nested);
    }
    (void)get(parent, db, "key", "outer");
    mdb_txn_abort(parent);
    mdb_env_close(env);

    puts("scenario=G02-reopen-published-parent-only");
    env = environment(path, MDB_RDONLY | flags);
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &reader), "reopen_begin");
    ok(mdb_dbi_open(reader, "records", 0, &db), "reopen_db");
    ok(mdb_dbi_open(reader, "private", 0, &private_db), "reopen_private_db");
    (void)get(reader, db, "key", "published");
    (void)get(reader, private_db, "key", "unpublished");
    (void)get(reader, db, "reserved", "reserved");
    mdb_txn_abort(reader); mdb_env_close(env);
    puts("readonly-children-complete");
}
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    if (mkdir(argv[1], 0700) && errno != EEXIST) return 1;
    char path[4096];
    const unsigned modes[] = {0, MDB_NOTLS, MDB_NOTLS | MDB_WRITEMAP, MDB_NOTLS | MDB_NOLOCK};
    const char *names[] = {"tls", "notls", "writemap", "nolock"};
    for (unsigned mode = 0; mode < sizeof modes / sizeof *modes; ++mode) {
        int count = snprintf(path, sizeof path, "%s/%s", argv[1], names[mode]);
        require(count > 0 && (size_t)count < sizeof path, "path length");
        require(!mkdir(path, 0700), "scenario directory");
        printf("mode=%s\n", names[mode]);
        scenario(path, modes[mode]);
    }
    return 0;
}
