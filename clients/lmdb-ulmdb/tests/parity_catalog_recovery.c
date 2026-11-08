/* Proper LMDB alias recovery; no API call uses an already-closed old DBI. */
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#ifndef MDB_DBIS_BUSY
#define MDB_DBIS_BUSY (-30773)
#endif
#define MODERN (MDB_VERSION_MAJOR >= 1)

struct fixture {
    MDB_env *env;
    MDB_dbi main, named, other;
    unsigned named_flags;
    const char *path, *other_name, *other_value;
};
static unsigned compare_calls;
static void need(int condition, const char *label) {
    if (!condition) { fprintf(stderr, "catalog recovery oracle: %s\n", label); exit(1); }
}
static void ok(int code, const char *label) {
    if (code) { fprintf(stderr, "%s: %s (%d)\n", label, mdb_strerror(code), code); exit(1); }
}
static MDB_val text(const char *value) { MDB_val v = {strlen(value), (void *)value}; return v; }
static int byte_compare(const MDB_val *left, const MDB_val *right) {
    ++compare_calls;
    size_t size = left->mv_size < right->mv_size ? left->mv_size : right->mv_size;
    int order = memcmp(left->mv_data, right->mv_data, size);
    return order ? order : (left->mv_size > right->mv_size) - (left->mv_size < right->mv_size);
}
static MDB_env *environment(const char *path, unsigned maximum) {
    MDB_env *env;
    ok(mdb_env_create(&env), "env_create");
    ok(mdb_env_set_maxdbs(env, maximum), "maxdbs");
    ok(mdb_env_set_mapsize(env, 16u * 1024u * 1024u), "mapsize");
    ok(mdb_env_open(env, path, MDB_NOTLS, 0600), "env_open");
    return env;
}
static void put(MDB_txn *txn, MDB_dbi db, const char *contents) {
    MDB_val key = text("k"), value = text(contents);
    ok(mdb_put(txn, db, &key, &value, 0), "put");
}
static void get(MDB_txn *txn, MDB_dbi db, const char *label, int expected, const char *contents) {
    MDB_val key = text("k"), value;
    int code = mdb_get(txn, db, &key, &value);
    printf("%s=%d", label, code);
    if (!code) printf(" value=%.*s", (int)value.mv_size, (char *)value.mv_data);
    putchar('\n');
    need(code == expected, label);
    if (!code) need(value.mv_size == strlen(contents) && !memcmp(value.mv_data, contents, value.mv_size), label);
}
static void flags(MDB_txn *txn, MDB_dbi db, const char *label, unsigned expected) {
    unsigned actual = ~0u;
    int code = mdb_dbi_flags(txn, db, &actual);
    printf("%s=%d flags=%u\n", label, code, actual);
    need(!code && actual == expected, label);
}
static void stats(MDB_txn *txn, MDB_dbi db, const char *label, size_t entries) {
    MDB_stat stat;
    int code = mdb_stat(txn, db, &stat);
    printf("%s=%d", label, code);
    if (!code) printf(" entries=%zu", stat.ms_entries);
    putchar('\n');
    need(!code && stat.ms_entries == entries, label);
}
static struct fixture seed(const char *path, unsigned maximum, unsigned named_flags, int callback) {
    need(!mkdir(path, 0700), "fresh scenario");
    struct fixture f = {.env = environment(path, maximum), .named_flags = named_flags, .path = path};
    MDB_txn *txn;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "seed_begin");
    ok(mdb_dbi_open(txn, NULL, 0, &f.main), "seed_main");
    ok(mdb_dbi_open(txn, "named", MDB_CREATE | named_flags, &f.named), "seed_named");
    if (callback) ok(mdb_set_compare(txn, f.named, byte_compare), "seed_comparator_before_data");
    put(txn, f.named, named_flags & MDB_DUPSORT ? "aa" : "old");
    if (named_flags & MDB_DUPSORT) put(txn, f.named, "bb");
    if (maximum > 1) {
        f.other_name = "spare"; f.other_value = "spare";
        ok(mdb_dbi_open(txn, f.other_name, MDB_CREATE, &f.other), "seed_spare");
        put(txn, f.other, f.other_value);
    }
    ok(mdb_txn_commit(txn), "seed_commit");
    if (f.other_name) mdb_dbi_close(f.env, f.other);
    return f;
}
static void delete_abort(struct fixture *f) {
    MDB_txn *txn;
    ok(mdb_txn_begin(f->env, NULL, 0, &txn), "delete_begin");
    ok(mdb_drop(txn, f->named, 1), "delete_named");
    puts("named.delete=0");
    mdb_txn_abort(txn);
    puts("named.delete.abort=done");
    /* f->named is now closed permanently, despite rollback of catalog data. */
}
static void verify_reopen(struct fixture *f, int present) {
    mdb_env_close(f->env);
    f->env = environment(f->path, 16);
    MDB_txn *txn;
    MDB_dbi main, named;
    ok(mdb_txn_begin(f->env, NULL, MDB_RDONLY, &txn), "verify_begin");
    ok(mdb_dbi_open(txn, NULL, 0, &main), "verify_main");
    stats(txn, main, "reopen.main", present ? 1 + !!f->other_name : 0);
    int code = mdb_dbi_open(txn, "named", 0, &named);
    printf("reopen.named=%d\n", code);
    need(code == (present ? 0 : MDB_NOTFOUND), "reopen named catalog");
    if (!code) {
        flags(txn, named, "reopen.named.flags", f->named_flags);
        get(txn, named, "reopen.named.data", 0, f->named_flags & MDB_DUPSORT ? "aa" : "old");
    }
    if (f->other_name) {
        code = mdb_dbi_open(txn, f->other_name, 0, &named);
        printf("reopen.other=%d\n", code);
        need(code == (present ? 0 : MDB_NOTFOUND), "reopen other catalog");
        if (!code) get(txn, named, "reopen.other.data", 0, f->other_value);
    }
    mdb_txn_abort(txn); mdb_env_close(f->env); f->env = NULL;
}
enum recovery { NO_REOPEN, WRITE_REOPEN_CLEAR, WRITE_REOPEN_ABORT, READ_REOPEN_ABORT, READ_REOPEN_COMMIT };
static void recovery_case(const char *path, enum recovery mode) {
    struct fixture f = seed(path, 2, 0, 0);
    delete_abort(&f);
    MDB_txn *txn;
    MDB_dbi fresh = 0;
    if (mode != NO_REOPEN) {
        unsigned readonly = mode == READ_REOPEN_ABORT || mode == READ_REOPEN_COMMIT ? MDB_RDONLY : 0;
        ok(mdb_txn_begin(f.env, NULL, readonly, &txn), "recovery_reopen_begin");
        ok(mdb_dbi_open(txn, "named", 0, &fresh), "proper_explicit_reopen");
        puts("named.explicit-reopen=0");
        flags(txn, fresh, "named.explicit.flags", 0);
        get(txn, fresh, "named.explicit.data", 0, "old");
        if (mode == WRITE_REOPEN_CLEAR) {
            int code = mdb_drop(txn, f.main, 0);
            printf("explicit-writer.main.clear=%d\n", code);
            need(code == (MODERN ? MDB_DBIS_BUSY : 0), "explicit writer DBI validity");
            get(txn, fresh, "explicit-writer.cached.data", 0, "old");
            ok(mdb_txn_commit(txn), "explicit_writer_commit");
            verify_reopen(&f, MODERN); return;
        }
        if (mode == READ_REOPEN_COMMIT) {
            ok(mdb_txn_commit(txn), "explicit_reader_commit");
            puts("named.explicit.reader.commit=0");
        } else {
            mdb_txn_abort(txn);
            puts("named.explicit.abort=done");
            /* fresh is closed now; no subsequent call uses it. */
        }
    }
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "post_recovery_writer");
    stats(txn, f.main, "post-recovery.main.before", 2);
    int busy = MODERN && mode == READ_REOPEN_COMMIT;
    int code = mdb_drop(txn, f.main, 0);
    printf("post-recovery.main.clear=%d\n", code);
    need(code == (busy ? MDB_DBIS_BUSY : 0), "closed aliases cannot keep modern main busy");
    stats(txn, f.main, "post-recovery.main.after", busy ? 2 : 0);
    if (mode == READ_REOPEN_COMMIT)
        get(txn, fresh, "exported-reader-alias.data", busy ? 0 : MDB_NOTFOUND, "old");
    ok(mdb_txn_commit(txn), "post_recovery_commit");
    verify_reopen(&f, busy);
}
static void readonly_reset_recovery(const char *path) {
    struct fixture f = seed(path, 2, 0, 0);
    /* Only aliases opened in the reader are reset here. The committed seed
       alias is explicitly closed before that reader starts. */
    mdb_dbi_close(f.env, f.named);
    MDB_txn *reader, *writer;
    MDB_dbi fresh;
    ok(mdb_txn_begin(f.env, NULL, MDB_RDONLY, &reader), "reset_reader_begin");
    ok(mdb_dbi_open(reader, "named", 0, &fresh), "reset_reader_new_alias");
    flags(reader, fresh, "reset-reader.named.flags", 0);
    get(reader, fresh, "reset-reader.named.data", 0, "old");
    mdb_txn_reset(reader);
    puts("reader.reset=done");
    /* Reset ends this registration without exporting it. Neither numeric
       named DBI is used again, and the inactive reader is aborted below. */
    ok(mdb_txn_begin(f.env, NULL, 0, &writer), "reset_fresh_writer");
    stats(writer, f.main, "reset-writer.main.before", 2);
    int code = mdb_drop(writer, f.main, 0);
    printf("reset-writer.main.clear=%d\n", code);
    need(!code, "reset new alias must not keep modern main busy");
    stats(writer, f.main, "reset-writer.main.after", 0);
    ok(mdb_txn_commit(writer), "reset_writer_commit");
    mdb_txn_abort(reader);
    puts("inactive-reader.abort=done");
    verify_reopen(&f, 0);
}
static void nested_recovery(const char *path, int recreate_release) {
    struct fixture f = seed(path, 2, 0, 0);
    MDB_txn *parent, *child;
    ok(mdb_txn_begin(f.env, NULL, 0, &parent), "nested_parent");
    ok(mdb_txn_begin(f.env, parent, 0, &child), "nested_child");
    ok(mdb_drop(child, f.named, 1), "nested_delete");
    puts("nested.named.delete=0");
    if (recreate_release) {
        MDB_dbi replacement;
        ok(mdb_dbi_open(child, "named", MDB_CREATE, &replacement), "nested_recreate");
        put(child, replacement, "replacement");
        ok(mdb_txn_commit(child), "nested_release");
        puts("nested.child.commit=0");
    } else {
        mdb_txn_abort(child); puts("nested.child.abort=done");
    }
    mdb_txn_abort(parent); puts("nested.parent.abort=done");
    /* Both the old deleted DBI and the aborted replacement DBI stay closed.
       The next transaction uses only MAIN, not either numeric named handle. */
    MDB_txn *txn;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "nested_fresh_writer");
    stats(txn, f.main, "nested.restored.main", 2);
    int code = mdb_drop(txn, f.main, 0);
    printf("nested.fresh.main.clear=%d\n", code);
    need(!code, "nested rollback does not export closed aliases");
    ok(mdb_txn_commit(txn), "nested_fresh_commit");
    verify_reopen(&f, 0);
}
static void capacity_recovery(const char *path) {
    struct fixture f = seed(path, 1, 0, 0);
    delete_abort(&f);
    MDB_txn *txn;
    MDB_dbi other, fresh;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "capacity_new_begin");
    ok(mdb_dbi_open(txn, "fresh", MDB_CREATE, &other), "capacity_create_other");
    put(txn, other, "new");
    ok(mdb_txn_commit(txn), "capacity_new_commit");
    puts("capacity.new-alias=0");
    ok(mdb_txn_begin(f.env, NULL, MDB_RDONLY, &txn), "capacity_full_reader");
    int code = mdb_dbi_open(txn, "named", 0, &fresh);
    printf("capacity.closed-name.reopen=%d\n", code);
    need(code == MDB_DBS_FULL, "recovery must respect other active alias capacity");
    mdb_txn_abort(txn);
    mdb_dbi_close(f.env, other);
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "capacity_available_writer");
    ok(mdb_dbi_open(txn, "named", 0, &fresh), "capacity_proper_reopen");
    flags(txn, fresh, "capacity.recovered.flags", 0);
    get(txn, fresh, "capacity.recovered.data", 0, "old");
    ok(mdb_txn_commit(txn), "capacity_recovered_commit");
    f.other_name = "fresh"; f.other_value = "new";
    verify_reopen(&f, 1);
}
static void callback_recovery(const char *path) {
    /* Equivalent byte ordering is installed before the first data access. No
       tree is accessed under an incompatible comparison order. */
    struct fixture f = seed(path, 2, 0, 1);
    MDB_txn *txn;
    MDB_val a = text("a"), b = text("b");
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "callback_before_begin");
    unsigned before = compare_calls;
    need(mdb_cmp(txn, f.named, &a, &b) < 0, "registered callback order");
    printf("callback.before.called=%d\n", compare_calls > before);
    need(compare_calls > before, "registered comparator used");
    ok(mdb_drop(txn, f.named, 1), "callback_delete");
    mdb_txn_abort(txn);
    MDB_dbi fresh;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "callback_reopen_begin");
    ok(mdb_dbi_open(txn, "named", 0, &fresh), "callback_proper_reopen");
    before = compare_calls;
    need(mdb_cmp(txn, fresh, &a, &b) < 0, "default comparison order");
    printf("callback.reopened.old-callback-called=%d\n", compare_calls > before);
    need(compare_calls == before, "proper reopen reinstalls default comparator");
    flags(txn, fresh, "callback.reopened.flags", 0);
    get(txn, fresh, "callback.reopened.data", 0, "old");
    mdb_txn_abort(txn);
    verify_reopen(&f, 1);
}
static void fixed_flags_recovery(const char *path) {
    struct fixture f = seed(path, 2, MDB_DUPSORT | MDB_DUPFIXED, 0);
    delete_abort(&f);
    MDB_txn *txn;
    MDB_dbi fresh;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "fixed_reopen_begin");
    ok(mdb_dbi_open(txn, "named", MDB_INTEGERKEY, &fresh), "fixed_reopen_ignores_creation_flags");
    flags(txn, fresh, "fixed.reopened.flags", MDB_DUPSORT | MDB_DUPFIXED);
    get(txn, fresh, "fixed.reopened.data", 0, "aa");
    stats(txn, fresh, "fixed.reopened.stat", 2);
    int code = mdb_drop(txn, f.main, 0);
    printf("fixed.reopened.main.clear=%d\n", code);
    need(code == (MODERN ? MDB_DBIS_BUSY : 0), "fixed reopened cache validity");
    get(txn, fresh, "fixed.cached.after-clear", 0, "aa");
    flags(txn, fresh, "fixed.after-clear.flags", MDB_DUPSORT | MDB_DUPFIXED);
    mdb_txn_abort(txn);
    verify_reopen(&f, 1);
}
static void scenario(const char *root, const char *name, char path[4096]) {
    int count = snprintf(path, 4096, "%s/%s", root, name);
    need(count > 0 && count < 4096, "scenario path");
    printf("scenario=%s\n", name);
}
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    if (mkdir(argv[1], 0700) && errno != EEXIST) return 1;
    puts("catalog-recovery-version=1");
    char path[4096];
    scenario(argv[1], "delete-abort-clear-without-reopen", path); recovery_case(path, NO_REOPEN);
    scenario(argv[1], "delete-abort-writer-reopen-clear", path); recovery_case(path, WRITE_REOPEN_CLEAR);
    scenario(argv[1], "delete-abort-writer-reopen-abort-clear", path); recovery_case(path, WRITE_REOPEN_ABORT);
    scenario(argv[1], "delete-abort-reader-reopen-abort-clear", path); recovery_case(path, READ_REOPEN_ABORT);
    scenario(argv[1], "delete-abort-reader-reopen-commit-clear", path); recovery_case(path, READ_REOPEN_COMMIT);
    scenario(argv[1], "reader-new-alias-reset-clear", path); readonly_reset_recovery(path);
    scenario(argv[1], "nested-delete-abort-clear", path); nested_recovery(path, 0);
    scenario(argv[1], "nested-recreate-release-outer-abort-clear", path); nested_recovery(path, 1);
    scenario(argv[1], "dormant-alias-capacity", path); capacity_recovery(path);
    scenario(argv[1], "callback-reset-on-proper-reopen", path); callback_recovery(path);
    scenario(argv[1], "fixed-flags-after-delete-abort", path); fixed_flags_recovery(path);
    puts("catalog-recovery-complete");
    return 0;
}
