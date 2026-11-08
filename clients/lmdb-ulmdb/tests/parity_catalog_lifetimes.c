/* Unchanged application: version-specific main-clear and DBI cache lifetimes. */
#include <lmdb.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#ifndef MDB_DBIS_BUSY
#define MDB_DBIS_BUSY (-30773)
#endif
#define MODERN (MDB_VERSION_MAJOR >= 1)

struct fixture { MDB_env *env; MDB_dbi main, named, duplicates; const char *path; };
static void need(int yes, const char *operation) {
    if (!yes) { fprintf(stderr, "catalog oracle failed: %s\n", operation); exit(1); }
}
static void ok(int code, const char *operation) {
    if (code) { fprintf(stderr, "%s: %s (%d)\n", operation, mdb_strerror(code), code); exit(1); }
}
static MDB_val text(const char *value) { MDB_val result = {strlen(value), (void *)value}; return result; }
static int put(MDB_txn *txn, MDB_dbi db, const char *key_text, const char *value_text) {
    MDB_val key = text(key_text), value = text(value_text);
    return mdb_put(txn, db, &key, &value, 0);
}
static void get(MDB_txn *txn, MDB_dbi db, const char *label, const char *key_text,
                int expected_code, const char *expected_value) {
    MDB_val key = text(key_text), value;
    int code = mdb_get(txn, db, &key, &value);
    printf("%s=%d", label, code);
    if (!code) printf(" value=%.*s", (int)value.mv_size, (char *)value.mv_data);
    putchar('\n');
    need(code == expected_code, label);
    if (!code) need(value.mv_size == strlen(expected_value) &&
                    !memcmp(value.mv_data, expected_value, value.mv_size), label);
}
static void stats(MDB_txn *txn, MDB_dbi db, const char *label, size_t entries) {
    MDB_stat value;
    int code = mdb_stat(txn, db, &value);
    printf("%s=%d", label, code);
    if (!code) printf(" entries=%zu", value.ms_entries);
    putchar('\n');
    need(code == 0 && value.ms_entries == entries, label);
}
static void flags(MDB_txn *txn, MDB_dbi db, const char *label, unsigned expected) {
    unsigned value = ~0u;
    int code = mdb_dbi_flags(txn, db, &value);
    printf("%s=%d flags=%u\n", label, code, value);
    need(!code && value == expected, label);
}
static MDB_env *environment_limits(const char *path, unsigned options, unsigned maxdbs) {
    MDB_env *env;
    ok(mdb_env_create(&env), "env_create");
    ok(mdb_env_set_maxdbs(env, maxdbs), "maxdbs");
    ok(mdb_env_set_mapsize(env, 16u * 1024u * 1024u), "mapsize");
    ok(mdb_env_open(env, path, MDB_NOTLS | options, 0600), "env_open");
    return env;
}
static MDB_env *environment(const char *path, unsigned options) {
    return environment_limits(path, options, 16);
}
static struct fixture seed_limits(const char *path, unsigned maxdbs) {
    need(!mkdir(path, 0700), "fresh scenario directory");
    struct fixture f = {.env = environment_limits(path, 0, maxdbs), .path = path};
    MDB_txn *txn;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "seed_begin");
    ok(mdb_dbi_open(txn, NULL, 0, &f.main), "main_open");
    ok(mdb_dbi_open(txn, "named", MDB_CREATE, &f.named), "named_create");
    ok(mdb_dbi_open(txn, "duplicates", MDB_CREATE | MDB_DUPSORT, &f.duplicates), "duplicates_create");
    ok(put(txn, f.main, "0-user", "main"), "seed_main");
    ok(put(txn, f.named, "k", "old"), "seed_named");
    ok(put(txn, f.duplicates, "k", "a"), "seed_duplicate_a");
    ok(put(txn, f.duplicates, "k", "b"), "seed_duplicate_b");
    ok(mdb_txn_commit(txn), "seed_commit");
    return f;
}
static struct fixture seed(const char *path) { return seed_limits(path, 16); }
static int clear(MDB_txn *txn, MDB_dbi main, int delete) {
    int code = mdb_drop(txn, main, delete);
    printf("main.clear=%d\n", code);
    need(code == (MODERN ? MDB_DBIS_BUSY : 0), "version-specific clear result");
    return code;
}
static void catalog_after_reopen(struct fixture *f, int named_exists, int duplicates_exist,
                                 int user_exists, const char *named_value) {
    mdb_env_close(f->env);
    f->env = environment(f->path, MDB_RDONLY);
    MDB_txn *txn;
    MDB_dbi db;
    ok(mdb_txn_begin(f->env, NULL, MDB_RDONLY, &txn), "reopen_begin");
    ok(mdb_dbi_open(txn, NULL, 0, &db), "reopen_main");
    stats(txn, db, "reopen.main", (size_t)(named_exists + duplicates_exist + user_exists));
    get(txn, db, "reopen.user", "0-user", user_exists ? 0 : MDB_NOTFOUND, "main");
    int code = mdb_dbi_open(txn, "named", 0, &db);
    printf("reopen.named.open=%d\n", code);
    need(code == (named_exists ? 0 : MDB_NOTFOUND), "reopen named catalog");
    if (!code) get(txn, db, "reopen.named.get", "k", 0, named_value);
    code = mdb_dbi_open(txn, "duplicates", 0, &db);
    printf("reopen.duplicates.open=%d\n", code);
    need(code == (duplicates_exist ? 0 : MDB_NOTFOUND), "reopen duplicate catalog");
    if (!code) {
        get(txn, db, "reopen.duplicates.get", "k", 0, "a");
        stats(txn, db, "reopen.duplicates.stat", 2);
    }
    mdb_txn_abort(txn);
    mdb_env_close(f->env);
    f->env = NULL;
}
enum preload { UNTOUCHED, FLAGS, ALIAS, ALIAS_DUPSORT, ALIAS_INTEGERKEY, ALIAS_DUPFIXED,
               COMPARE, GET, GET_MISS, STAT, CURSOR_OPEN, INVALID_FLAGS, INVALID_KEY };
static const char *preload_names[] = {"untouched", "flags", "dbi-alias", "alias-dupsort", "alias-integerkey", "alias-dupfixed",
                                    "compare", "get", "get-miss", "stat", "cursor-open", "invalid-put-flags", "invalid-put-key"};
static int is_loaded(enum preload mode) {
    return mode == GET || mode == GET_MISS || mode == STAT || mode == CURSOR_OPEN || mode == INVALID_KEY;
}
static void preload_case(const char *path, enum preload mode, int duplicate) {
    struct fixture f = seed(path);
    MDB_dbi db = duplicate ? f.duplicates : f.named;
    const char *expected = duplicate ? "a" : "old";
    unsigned expected_flags = duplicate ? MDB_DUPSORT : 0;
    MDB_txn *txn;
    MDB_cursor *cursor = NULL;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "preload_begin");
    switch (mode) {
    case UNTOUCHED: break;
    case FLAGS: flags(txn, db, "before.flags", expected_flags); break;
    case ALIAS: {
        MDB_dbi alias;
        ok(mdb_dbi_open(txn, duplicate ? "duplicates" : "named", 0, &alias), "alias_open");
        printf("before.alias.same=%d\n", alias == db);
        need(alias == db, "same alias"); break;
    }
    case ALIAS_DUPSORT:
    case ALIAS_INTEGERKEY:
    case ALIAS_DUPFIXED: {
        unsigned requested = mode == ALIAS_DUPSORT ? MDB_DUPSORT :
            mode == ALIAS_INTEGERKEY ? MDB_INTEGERKEY : MDB_DUPSORT | MDB_DUPFIXED;
        MDB_dbi alias;
        int code = mdb_dbi_open(txn, duplicate ? "duplicates" : "named", requested, &alias);
        printf("before.alias.requested=%u code=%d same=%d\n", requested, code, !code && alias == db);
        need(!code && alias == db, "cached alias ignores different valid requested flags");
        flags(txn, alias, "before.alias.original-flags", expected_flags);
        break;
    }
    case COMPARE: {
        MDB_val first = text("a"), second = text("b");
        int order = mdb_cmp(txn, db, &first, &second);
        printf("before.cmp.sign=%d\n", (order > 0) - (order < 0));
        need(order < 0, "comparison order");
        if (duplicate) {
            order = mdb_dcmp(txn, db, &first, &second);
            printf("before.dcmp.sign=%d\n", (order > 0) - (order < 0));
            need(order < 0, "duplicate comparison order");
        }
        break;
    }
    case GET: get(txn, db, "before.get", "k", 0, expected); break;
    case GET_MISS: get(txn, db, "before.get-miss", "missing", MDB_NOTFOUND, NULL); break;
    case STAT: stats(txn, db, "before.stat", duplicate ? 2 : 1); break;
    case CURSOR_OPEN:
        ok(mdb_cursor_open(txn, db, &cursor), "preload_cursor_open");
        puts("before.cursor.open=0"); break;
    case INVALID_FLAGS: {
        MDB_val key = text("k"), value = text("x");
        int code = mdb_put(txn, db, &key, &value, 0x80000000u);
        printf("before.put.invalid-flags=%d\n", code);
        need(code == EINVAL, "invalid flags do not load DBI"); break;
    }
    case INVALID_KEY: {
        /* Size rejection occurs after cursor_init, but before any write or
           MDB_TXN_ERROR. The still-live DBI becomes loaded in these references. */
        MDB_val key = text(""), value = text("x");
        int code = mdb_put(txn, db, &key, &value, 0);
        printf("before.put.invalid-key=%d\n", code);
        need(code == MDB_BAD_VALSIZE, "invalid key loads without dirtying"); break;
    }
    }
    clear(txn, f.main, 0);
    stats(txn, f.main, "after.main.stat", MODERN ? 3 : 0);
    int available = MODERN || is_loaded(mode);
    get(txn, db, "after.named.get", "k", available ? 0 : MDB_NOTFOUND, expected);
    flags(txn, db, "after.named.flags", expected_flags);
    if (available) stats(txn, db, "after.named.stat", duplicate ? 2 : 1);
    else puts("after.named.stat=not-queried-stale-missing");
    /* mdb_stat ignores a failed stale refresh in these references; its fields
       then describe stale/zero txn memory and are deliberately not an oracle. */
    if (!cursor) ok(mdb_cursor_open(txn, db, &cursor), "after_cursor_open");
    MDB_val key, value;
    int code = mdb_cursor_get(cursor, &key, &value, MDB_FIRST);
    printf("after.named.cursor.first=%d\n", code);
    need(code == (available ? 0 : MDB_NOTFOUND), "cursor after main clear");
    if (!code && duplicate) {
        size_t count;
        ok(mdb_cursor_count(cursor, &count), "duplicate_count");
        printf("after.named.cursor.count=%zu\n", count);
        need(count == 2, "duplicate cache count");
    }
    mdb_cursor_close(cursor);
    mdb_txn_abort(txn);
    catalog_after_reopen(&f, 1, 1, 1, "old");
}
static void cursor_lifetimes(const char *path) {
    struct fixture f = seed(path);
    MDB_txn *txn;
    MDB_cursor *main, *named, *duplicates;
    MDB_val key, value;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "cursor_begin");
    ok(mdb_cursor_open(txn, f.main, &main), "main_cursor");
    ok(mdb_cursor_open(txn, f.named, &named), "named_cursor");
    ok(mdb_cursor_open(txn, f.duplicates, &duplicates), "duplicate_cursor");
    ok(mdb_cursor_get(main, &key, &value, MDB_FIRST), "main_first");
    ok(mdb_cursor_get(named, &key, &value, MDB_FIRST), "named_first");
    ok(mdb_cursor_get(duplicates, &key, &value, MDB_FIRST), "duplicate_first");
    clear(txn, f.main, 0);
    int code = mdb_cursor_get(main, &key, &value, MDB_GET_CURRENT);
    printf("main.cursor.current=%d\n", code);
    need(code == (MODERN ? 0 : EINVAL), "main cursor invalidation");
    code = mdb_cursor_get(main, &key, &value, MDB_FIRST);
    printf("main.cursor.first=%d\n", code);
    need(code == (MODERN ? 0 : MDB_NOTFOUND), "main cursor reuse");
    ok(mdb_cursor_get(named, &key, &value, MDB_GET_CURRENT), "named cursor remains positioned");
    need(value.mv_size == 3 && !memcmp(value.mv_data, "old", 3), "named current bytes");
    puts("named.cursor.current=0 value=old");
    ok(mdb_cursor_get(duplicates, &key, &value, MDB_NEXT_DUP), "duplicate cursor next");
    need(value.mv_size == 1 && !memcmp(value.mv_data, "b", 1), "duplicate next bytes");
    puts("duplicates.cursor.next=0 value=b");
    mdb_cursor_close(main); mdb_cursor_close(named); mdb_cursor_close(duplicates);
    mdb_txn_abort(txn);
    catalog_after_reopen(&f, 1, 1, 1, "old");
}
static void durable_clear_and_old_reader(const char *path) {
    struct fixture f = seed(path);
    MDB_txn *old, *writer, *fresh;
    ok(mdb_txn_begin(f.env, NULL, MDB_RDONLY, &old), "old_reader");
    get(old, f.named, "old.before.named", "k", 0, "old");
    get(old, f.duplicates, "old.before.duplicates", "k", 0, "a");
    ok(mdb_txn_begin(f.env, NULL, 0, &writer), "durable_writer");
    get(writer, f.named, "writer.load", "k", 0, "old");
    clear(writer, f.main, 0);
    get(writer, f.named, "writer.cached.after-clear", "k", 0, "old");
    ok(mdb_txn_commit(writer), "durable_commit");
    puts("parent.commit=0");
    get(old, f.named, "old.after.named", "k", 0, "old");
    get(old, f.duplicates, "old.after.duplicates", "k", 0, "a");
    mdb_txn_abort(old);
    ok(mdb_txn_begin(f.env, NULL, MDB_RDONLY, &fresh), "fresh_reader");
    MDB_dbi alias;
    ok(mdb_dbi_open(fresh, "named", 0, &alias), "global alias remains exported");
    puts("fresh.global-alias.open=0");
    get(fresh, alias, "fresh.global-alias.get", "k", MODERN ? 0 : MDB_NOTFOUND, "old");
    mdb_txn_abort(fresh);
    catalog_after_reopen(&f, MODERN, MODERN, MODERN, "old");
}
static void nested_case(const char *path, int parent_loads, int child_loads, int child_clears, int commit_child) {
    struct fixture f = seed(path);
    MDB_txn *parent, *child;
    ok(mdb_txn_begin(f.env, NULL, 0, &parent), "nested_parent");
    if (parent_loads) get(parent, f.named, "parent.load", "k", 0, "old");
    ok(mdb_txn_begin(f.env, parent, 0, &child), "nested_child");
    if (child_loads) get(child, f.named, "child.load", "k", 0, "old");
    if (child_clears) clear(child, f.main, 0);
    if (commit_child) { ok(mdb_txn_commit(child), "child_commit"); puts("child.commit=0"); }
    else { mdb_txn_abort(child); puts("child.abort=done"); }
    /* No parent operation is issued while its exclusive child is active. */
    if (!child_clears) clear(parent, f.main, 0);
    int main_exists = MODERN || (child_clears && !commit_child);
    int cached = MODERN || parent_loads || (child_loads && commit_child) || main_exists;
    stats(parent, f.main, "parent.main.stat", main_exists ? 3 : 0);
    get(parent, f.named, "parent.named.get", "k", cached ? 0 : MDB_NOTFOUND, "old");
    ok(mdb_txn_commit(parent), "nested_parent_commit");
    puts("parent.commit=0");
    catalog_after_reopen(&f, main_exists, main_exists, main_exists, "old");
}
static void failed_put_does_not_resurrect(const char *path) {
    struct fixture f = seed(path);
    MDB_txn *txn;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "failed_put_begin");
    get(txn, f.named, "named.load", "k", 0, "old");
    clear(txn, f.main, 0);
    if (MODERN) {
        puts("named.put.after-clear=not-run-busy");
    } else {
        int code = put(txn, f.named, "k", "new");
        printf("named.put.after-clear=%d\n", code);
        need(code == MDB_NOTFOUND, "missing catalog cannot be rewritten");
        /* In these exact 0.9 references mdb_cursor_touch returns NOTFOUND
           directly when the cleared main root is absent; no DB_DIRTY or
           MDB_TXN_ERROR is set. Commit immediately; do not reuse a failed
           cursor, perform another write, or generalize this to other errors. */
    }
    ok(mdb_txn_commit(txn), "failed_put_immediate_commit");
    puts("parent.commit=0");
    catalog_after_reopen(&f, MODERN, MODERN, MODERN, "old");
}
static void dirty_before_clear(const char *path) {
    struct fixture f = seed(path);
    MDB_txn *txn;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "dirty_begin");
    ok(put(txn, f.named, "k", "changed"), "dirty_named");
    clear(txn, f.main, 0);
    get(txn, f.named, "dirty.cached.get", "k", 0, "changed");
    stats(txn, f.main, "dirty.main.before-commit", MODERN ? 3 : 0);
    ok(mdb_txn_commit(txn), "dirty_commit");
    puts("parent.commit=0");
    /* 0.9 writes DB_DIRTY named descriptors back at final commit, recreating
       only the dirty named entry. Read-only cached entries stay unlinked. */
    catalog_after_reopen(&f, 1, MODERN, MODERN, "changed");
}
static void closed_handles_clear(const char *path, int delete) {
    struct fixture f = seed(path);
    /* Close only between transactions, before any new transaction snapshots
       those slots. Never use a closed handle or close one under a reader. */
    mdb_dbi_close(f.env, f.named); mdb_dbi_close(f.env, f.duplicates);
    MDB_txn *txn;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "closed_begin");
    int code = mdb_drop(txn, f.main, delete);
    printf("closed.main.clear.del%d=%d\n", delete, code);
    need(!code, "closed named handles permit clearing");
    stats(txn, f.main, "closed.main.stat", 0);
    ok(mdb_txn_commit(txn), "closed_commit");
    catalog_after_reopen(&f, 0, 0, 0, "old");
}
static void integer_put(MDB_txn *txn, MDB_dbi db, const char *contents) {
    uint32_t number = 7;
    MDB_val key = {sizeof number, &number}, value = text(contents);
    ok(mdb_put(txn, db, &key, &value, 0), "integer_put");
}
static void integer_get(MDB_txn *txn, MDB_dbi db, const char *label, const char *expected) {
    uint32_t number = 7;
    MDB_val key = {sizeof number, &number}, value;
    int code = mdb_get(txn, db, &key, &value);
    printf("%s=%d", label, code);
    if (!code) printf(" value=%.*s", (int)value.mv_size, (char *)value.mv_data);
    putchar('\n');
    need(!code && value.mv_size == strlen(expected) && !memcmp(value.mv_data, expected, value.mv_size), label);
}
static void full_alias_capacity(const char *path) {
    struct fixture f = seed_limits(path, 2);
    MDB_txn *txn;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "capacity_begin");
    clear(txn, f.main, 0);
    MDB_dbi alias;
    int code = mdb_dbi_open(txn, "named", MDB_DUPSORT | MDB_DUPFIXED, &alias);
    printf("capacity.existing-alias=%d same=%d\n", code, !code && alias == f.named);
    need(!code && alias == f.named, "full alias table retains existing name");
    flags(txn, alias, "capacity.alias.flags", 0);
    get(txn, alias, "capacity.alias.get", "k", MODERN ? 0 : MDB_NOTFOUND, "old");
    MDB_dbi missing = 0;
    code = mdb_dbi_open(txn, "new", 0, &missing);
    printf("capacity.missing.open=%d\n", code);
    need(code == MDB_DBS_FULL, "full table checked before missing catalog");
    code = mdb_dbi_open(txn, "new", MDB_CREATE | MDB_INTEGERKEY, &missing);
    printf("capacity.new.create=%d\n", code);
    need(code == MDB_DBS_FULL, "exported aliases still occupy slots after clear");
    ok(mdb_txn_commit(txn), "capacity_clear_commit");
    /* Free a slot only between transactions. The closed duplicate handle is
       never used again, and its number may legitimately be assigned anew. */
    mdb_dbi_close(f.env, f.duplicates);
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "capacity_reuse_begin");
    MDB_dbi fresh;
    ok(mdb_dbi_open(txn, "new", MDB_CREATE | MDB_INTEGERKEY, &fresh), "capacity_reuse_create");
    flags(txn, fresh, "capacity.new.flags", MDB_INTEGERKEY);
    integer_put(txn, fresh, "fresh");
    ok(mdb_txn_commit(txn), "capacity_reuse_commit");
    mdb_env_close(f.env);
    f.env = environment(path, MDB_RDONLY);
    ok(mdb_txn_begin(f.env, NULL, MDB_RDONLY, &txn), "capacity_reopen");
    ok(mdb_dbi_open(txn, "new", 0, &fresh), "capacity_reopen_new");
    flags(txn, fresh, "capacity.reopen.new.flags", MDB_INTEGERKEY);
    integer_get(txn, fresh, "capacity.reopen.new.get", "fresh");
    code = mdb_dbi_open(txn, "named", 0, &alias);
    printf("capacity.reopen.named=%d\n", code);
    need(code == (MODERN ? 0 : MDB_NOTFOUND), "capacity old named catalog");
    mdb_txn_abort(txn); mdb_env_close(f.env);
}
static void aborted_new_alias_flags(const char *path) {
    struct fixture f = seed(path);
    MDB_txn *txn;
    MDB_dbi ephemeral, alias;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "aborted_alias_begin");
    ok(mdb_dbi_open(txn, "ephemeral", MDB_CREATE | MDB_DUPSORT | MDB_DUPFIXED, &ephemeral), "ephemeral_create");
    ok(put(txn, ephemeral, "k", "aa"), "ephemeral_put");
    ok(mdb_dbi_open(txn, "ephemeral", MDB_INTEGERKEY, &alias), "ephemeral_alias_mismatch");
    need(alias == ephemeral, "ephemeral alias identity");
    flags(txn, alias, "ephemeral.alias.flags", MDB_DUPSORT | MDB_DUPFIXED);
    clear(txn, f.main, 0);
    flags(txn, alias, "ephemeral.after-clear.flags", MDB_DUPSORT | MDB_DUPFIXED);
    get(txn, alias, "ephemeral.after-clear.get", "k", 0, "aa");
    mdb_txn_abort(txn);
    /* Never use the aborted numeric handle. A fresh open receives a new valid
       capability and must not inherit the aborted duplicate/fixed settings. */
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "replacement_begin");
    ok(mdb_dbi_open(txn, "ephemeral", MDB_CREATE | MDB_INTEGERKEY, &ephemeral), "replacement_create");
    flags(txn, ephemeral, "replacement.flags", MDB_INTEGERKEY);
    integer_put(txn, ephemeral, "plain");
    ok(mdb_txn_commit(txn), "replacement_commit");
    mdb_env_close(f.env);
    f.env = environment(path, MDB_RDONLY);
    ok(mdb_txn_begin(f.env, NULL, MDB_RDONLY, &txn), "replacement_reopen");
    ok(mdb_dbi_open(txn, "ephemeral", 0, &ephemeral), "replacement_reopen_dbi");
    flags(txn, ephemeral, "replacement.reopen.flags", MDB_INTEGERKEY);
    integer_get(txn, ephemeral, "replacement.reopen.get", "plain");
    MDB_dbi original_named;
    ok(mdb_dbi_open(txn, "named", 0, &original_named), "replacement_reopen_original_named");
    get(txn, original_named, "replacement.original.named", "k", 0, "old");
    mdb_txn_abort(txn); mdb_env_close(f.env);
}
enum dirty_transition { KV_IDENTICAL, DUP_EQUIVALENT, KV_NOOVERWRITE,
                        DUP_NOOVERWRITE, DUP_NODUPDATA, KV_APPEND_REJECT, DUP_APPEND_REJECT };
static const char *transition_names[] = {"kv-identical-put", "duplicate-equivalent-put", "kv-nooverwrite",
    "duplicate-nooverwrite", "duplicate-nodupdata", "kv-append-rejection", "duplicate-append-rejection"};
static void dirty_transition_case(const char *path, enum dirty_transition mode) {
    struct fixture f = seed(path);
    MDB_txn *txn;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "transition_begin");
    int duplicate = mode == DUP_EQUIVALENT || mode == DUP_NOOVERWRITE || mode == DUP_NODUPDATA || mode == DUP_APPEND_REJECT;
    int dirty = mode == KV_IDENTICAL || mode == DUP_EQUIVALENT || mode == DUP_NODUPDATA || mode == DUP_APPEND_REJECT;
    unsigned options = mode == KV_NOOVERWRITE || mode == DUP_NOOVERWRITE ? MDB_NOOVERWRITE :
        mode == DUP_NODUPDATA ? MDB_NODUPDATA : mode == KV_APPEND_REJECT ? MDB_APPEND :
        mode == DUP_APPEND_REJECT ? MDB_APPENDDUP : 0;
    MDB_val key = text(mode == KV_APPEND_REJECT ? "a" : "k");
    MDB_val value = text(duplicate ? "a" : "old");
    MDB_dbi db = duplicate ? f.duplicates : f.named;
    int code = mdb_put(txn, db, &key, &value, options);
    printf("transition.put=%d\n", code);
    need(code == (mode == KV_IDENTICAL || mode == DUP_EQUIVALENT ? 0 : MDB_KEYEXIST), "transition return code");
    /* These bounded KEYEXIST branches return without TXN_ERROR. In 0.9,
       NOOVERWRITE and rejected APPEND exit before cursor_touch, whereas
       equivalent duplicates/NODUPDATA/APPENDDUP touch the outer named DB first.
       Never generalize continued use to allocation, I/O or corruption errors. */
    clear(txn, f.main, 0);
    stats(txn, f.main, "transition.main.before-commit", MODERN ? 3 : 0);
    get(txn, db, "transition.cached.get", "k", 0, duplicate ? "a" : "old");
    ok(mdb_txn_commit(txn), "transition_commit");
    puts("parent.commit=0");
    catalog_after_reopen(&f, MODERN || (dirty && !duplicate), MODERN || (dirty && duplicate), MODERN, "old");
}
static void cursor_write_after_clear(const char *path, int mode) {
    struct fixture f = seed(path);
    MDB_txn *txn;
    MDB_cursor *cursor;
    MDB_val key, value;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "cursor_write_begin");
    ok(mdb_cursor_open(txn, f.duplicates, &cursor), "cursor_write_open");
    ok(mdb_cursor_get(cursor, &key, &value, MDB_FIRST), "cursor_write_position");
    clear(txn, f.main, 0);
    int code;
    if (mode == 2) code = mdb_cursor_del(cursor, 0);
    else {
        /* A same-width replacement stays ordered before "b" and isolates the
           main-catalog guard from unrelated variable-width CURRENT behavior. */
        key = text("k"); value = text(mode == 0 ? "0" : "c");
        code = mdb_cursor_put(cursor, &key, &value, mode == 0 ? MDB_CURRENT : 0);
    }
    printf("cursor.write.after-clear=%d\n", code);
    need(code == (MODERN ? 0 : MDB_NOTFOUND), "cursor write after main clear");
    /* In the small 0.9 case, cursor_touch returns NOTFOUND before changing the
       named descriptor/page or setting TXN_ERROR, for both put and del. The
       original position, values and duplicate count must remain usable. */
    code = mdb_cursor_get(cursor, &key, &value, MDB_GET_CURRENT);
    printf("cursor.write.current=%d", code);
    if (!code) printf(" value=%.*s", (int)value.mv_size, (char *)value.mv_data);
    putchar('\n');
    const char *current = !MODERN ? "a" : mode == 0 ? "0" : mode == 1 ? "c" : "b";
    need(!code && value.mv_size == strlen(current) && !memcmp(value.mv_data, current, value.mv_size), "cursor position after write");
    const char *first = !MODERN ? "a" : mode == 0 ? "0" : mode == 1 ? "a" : "b";
    get(txn, f.duplicates, "cursor.write.named.get", "k", 0, first);
    size_t count;
    ok(mdb_cursor_count(cursor, &count), "cursor_write_count");
    size_t expected_count = !MODERN || mode == 0 ? 2 : mode == 1 ? 3 : 1;
    printf("cursor.write.count=%zu\n", count);
    need(count == expected_count, "cursor count after write");
    mdb_cursor_close(cursor);
    ok(mdb_txn_commit(txn), "cursor_write_commit");
    puts("parent.commit=0");
    mdb_env_close(f.env);
    f.env = environment(path, MDB_RDONLY);
    ok(mdb_txn_begin(f.env, NULL, MDB_RDONLY, &txn), "cursor_write_reopen");
    MDB_dbi db;
    ok(mdb_dbi_open(txn, NULL, 0, &db), "cursor_write_reopen_main");
    stats(txn, db, "cursor.write.reopen.main", MODERN ? 3 : 0);
    code = mdb_dbi_open(txn, "duplicates", 0, &db);
    printf("cursor.write.reopen.duplicates=%d\n", code);
    need(code == (MODERN ? 0 : MDB_NOTFOUND), "cursor write cannot recreate missing named catalog");
    if (!code) {
        get(txn, db, "cursor.write.reopen.get", "k", 0, first);
        stats(txn, db, "cursor.write.reopen.stat", expected_count);
    }
    mdb_txn_abort(txn); mdb_env_close(f.env);
}
static void named_delete_then_main_clear(const char *path) {
    struct fixture f = seed(path);
    /* Close the other handle before beginning. Deleting the remaining named
       DBI closes that alias immediately, leaving modern main-clear unblocked. */
    mdb_dbi_close(f.env, f.duplicates);
    MDB_txn *txn;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "named_delete_clear_begin");
    int code = mdb_drop(txn, f.named, 1);
    printf("named.delete=%d\n", code);
    need(!code, "named delete closes alias");
    /* Never use f.named or f.duplicates again after their closure. */
    code = mdb_drop(txn, f.main, 0);
    printf("named.delete.then-main-clear=%d\n", code);
    need(!code, "no valid named slots remain after delete");
    stats(txn, f.main, "named.delete.main.stat", 0);
    ok(mdb_txn_commit(txn), "named_delete_clear_commit");
    catalog_after_reopen(&f, 0, 0, 0, "old");
}
static void named_delete_abort_then_reopen(const char *path) {
    struct fixture f = seed(path);
    MDB_txn *txn;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "named_delete_abort_begin");
    ok(mdb_drop(txn, f.named, 1), "named_delete_before_abort");
    puts("named.delete=0");
    mdb_txn_abort(txn);
    /* Abort restores data, but does not resurrect the explicitly closed DBI.
       Obtain a fresh handle in a new transaction; never call through f.named. */
    ok(mdb_txn_begin(f.env, NULL, MDB_RDONLY, &txn), "named_delete_after_abort_begin");
    MDB_dbi restored;
    int code = mdb_dbi_open(txn, "named", 0, &restored);
    printf("named.delete.abort.new-open=%d\n", code);
    need(!code, "aborted deletion restores named catalog");
    flags(txn, restored, "named.delete.abort.flags", 0);
    get(txn, restored, "named.delete.abort.data", "k", 0, "old");
    stats(txn, f.main, "named.delete.abort.main", 3);
    ok(mdb_txn_commit(txn), "named_delete_restored_reader_commit");
    catalog_after_reopen(&f, 1, 1, 1, "old");
}
static void first_open_existing_flags(const char *path, int duplicate) {
    struct fixture f = seed(path);
    mdb_env_close(f.env);
    f.env = environment(path, 0);
    MDB_txn *txn;
    MDB_dbi main, existing;
    ok(mdb_txn_begin(f.env, NULL, 0, &txn), "first_open_begin");
    ok(mdb_dbi_open(txn, NULL, 0, &main), "first_open_main");
    unsigned requested = duplicate ? 0 : MDB_DUPSORT;
    int code = mdb_dbi_open(txn, duplicate ? "duplicates" : "named", requested, &existing);
    printf("first-open.requested=%u code=%d\n", requested, code);
    need(!code, "existing catalog kind overrides creation flags on first open");
    flags(txn, existing, "first-open.actual-flags", duplicate ? MDB_DUPSORT : 0);
    /* Unlike a cached same-name alias return, this first open has read the
       actual named catalog descriptor and leaves a loaded DBI in the txn. */
    clear(txn, main, 0);
    get(txn, existing, "first-open.cached.after-clear", "k", 0, duplicate ? "a" : "old");
    stats(txn, existing, "first-open.cached.stat", duplicate ? 2 : 1);
    flags(txn, existing, "first-open.after-clear.flags", duplicate ? MDB_DUPSORT : 0);
    mdb_txn_abort(txn);
    catalog_after_reopen(&f, 1, 1, 1, "old");
}
static void path_for(const char *parent, const char *name, char path[4096]) {
    int count = snprintf(path, 4096, "%s/%s", parent, name);
    need(count > 0 && count < 4096, "scenario path");
    printf("scenario=%s\n", name);
}
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    if (mkdir(argv[1], 0700) && errno != EEXIST) return 1;
    puts("catalog-lifetimes-version=1");
    char path[4096], name[128];
    for (int duplicate = 0; duplicate < 2; ++duplicate) {
        for (int mode = UNTOUCHED; mode <= INVALID_KEY; ++mode) {
            int count = snprintf(name, sizeof name, "%s-%s", duplicate ? "duplicates" : "kv", preload_names[mode]);
            need(count > 0 && (size_t)count < sizeof name, "scenario name");
            path_for(argv[1], name, path);
            preload_case(path, (enum preload)mode, duplicate);
        }
    }
    path_for(argv[1], "cursor-positions", path); cursor_lifetimes(path);
    path_for(argv[1], "durable-clear-old-reader", path); durable_clear_and_old_reader(path);
    path_for(argv[1], "parent-loaded-child-clear-abort", path); nested_case(path, 1, 0, 1, 0);
    path_for(argv[1], "parent-loaded-child-clear-commit", path); nested_case(path, 1, 0, 1, 1);
    path_for(argv[1], "child-load-commit-parent-clear", path); nested_case(path, 0, 1, 0, 1);
    path_for(argv[1], "child-load-abort-parent-clear", path); nested_case(path, 0, 1, 0, 0);
    path_for(argv[1], "failed-put-does-not-resurrect", path); failed_put_does_not_resurrect(path);
    path_for(argv[1], "dirty-before-clear", path); dirty_before_clear(path);
    path_for(argv[1], "closed-handles-clear", path); closed_handles_clear(path, 0);
    path_for(argv[1], "closed-handles-main-delete", path); closed_handles_clear(path, 1);
    path_for(argv[1], "full-exported-alias-capacity", path); full_alias_capacity(path);
    path_for(argv[1], "aborted-new-alias-flags", path); aborted_new_alias_flags(path);
    for (int mode = KV_IDENTICAL; mode <= DUP_APPEND_REJECT; ++mode) {
        path_for(argv[1], transition_names[mode], path);
        dirty_transition_case(path, (enum dirty_transition)mode);
    }
    path_for(argv[1], "cursor-current-after-clear", path); cursor_write_after_clear(path, 0);
    path_for(argv[1], "cursor-put-after-clear", path); cursor_write_after_clear(path, 1);
    path_for(argv[1], "cursor-delete-after-clear", path); cursor_write_after_clear(path, 2);
    path_for(argv[1], "named-delete-then-main-clear", path); named_delete_then_main_clear(path);
    path_for(argv[1], "named-delete-abort-then-reopen", path); named_delete_abort_then_reopen(path);
    path_for(argv[1], "first-open-existing-kv-request-dupsort", path); first_open_existing_flags(path, 0);
    path_for(argv[1], "first-open-existing-duplicates-request-zero", path); first_open_existing_flags(path, 1);
    puts("catalog-lifetimes-complete");
    return 0;
}
