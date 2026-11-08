/* Pinned-reference observation, with no guessed winner for two dirty aliases. */
#define main frozen_named_fold_main
#include "named_fold_frozen.h"
#undef main

static void data_trace(MDB_txn *t, MDB_dbi db, const char *label) {
    const char *keys[] = {"k", "l", "u"};
    for (unsigned i = 0; i != 3; ++i) {
        MDB_val k = val(keys[i]), v = {0, NULL};
        int rc = mdb_get(t, db, &k, &v);
        printf("%s.%s.rc=%d", label, keys[i], rc);
        if (!rc) printf(" len=%zu value=%.*s", v.mv_size, (int)v.mv_size, (char *)v.mv_data);
        putchar('\n');
    }
}
static void cursor_trace(MDB_txn *t, MDB_dbi db, const char *label) {
    MDB_cursor *c = NULL;
    int rc = mdb_cursor_open(t, db, &c);
    printf("%s.cursor_open.rc=%d\n", label, rc);
    if (rc) return;
    MDB_val k = val("k"), v = {0, NULL};
    rc = mdb_cursor_get(c, &k, &v, MDB_SET);
    printf("%s.cursor_set.rc=%d", label, rc);
    if (!rc) printf(" len=%zu value=%.*s", v.mv_size, (int)v.mv_size, (char *)v.mv_data);
    putchar('\n');
    if (!rc) {
        rc = mdb_cursor_get(c, &k, &v, MDB_GET_CURRENT);
        printf("%s.cursor_current.rc=%d", label, rc);
        if (!rc) printf(" len=%zu value=%.*s", v.mv_size, (int)v.mv_size, (char *)v.mv_data);
        putchar('\n');
    }
    mdb_cursor_close(c);
}
static void both_trace(MDB_txn *t, MDB_dbi lower, MDB_dbi upper, const char *phase, int cursors) {
    char text[128];
    printf("%s.ids.lower=%u upper=%u distinct=%d\n", phase, lower, upper, lower != upper);
    snprintf(text, sizeof text, "%s.lower", phase); data_trace(t, lower, text);
    if (cursors) cursor_trace(t, lower, text);
    snprintf(text, sizeof text, "%s.upper", phase); data_trace(t, upper, text);
    if (cursors) cursor_trace(t, upper, text);
}
static int write_alias(MDB_txn *t, MDB_dbi db, int lower) {
    MDB_val k = val("k"), v = val(lower ? "LOWER" : "UPPER");
    int rc = mdb_put(t, db, &k, &v, 0);
    printf("write.%s.k.rc=%d\n", lower ? "lower" : "upper", rc);
    if (rc) return rc;
    k = val(lower ? "l" : "u");
    rc = mdb_put(t, db, &k, &v, 0);
    printf("write.%s.extra.rc=%d\n", lower ? "lower" : "upper", rc);
    return rc;
}
static void aliases(MDB_txn *t, MDB_dbi *lower, MDB_dbi *upper) {
    main_fold(t);
    ok(mdb_dbi_open(t, "x", 0, lower), "lower alias open");
    ok(mdb_set_compare(t, *lower, lower_cmp), "lower comparator before data");
    ok(mdb_dbi_open(t, "X", 0, upper), "upper alias open");
    ok(mdb_set_compare(t, *upper, upper_cmp), "upper comparator before data");
    need(*lower != *upper, "public aliases remain distinct");
}
static void after_commit(MDB_env *e, const char *phase) {
    MDB_txn *t = begin(e, MDB_RDONLY); MDB_dbi lower, upper;
    aliases(t, &lower, &upper); both_trace(t, lower, upper, phase, 1);
    mdb_txn_abort(t);
}
static void run_case(const char *path, int kind) {
    need(!mkdir(path, 0700), "fresh case");
    MDB_env *e = env(path, 0); MDB_txn *parent = begin(e, 0); main_fold(parent);
    MDB_dbi upper, lower;
    ok(mdb_dbi_open(parent, "X", MDB_CREATE, &upper), "seed X");
    ok(mdb_set_compare(parent, upper, upper_cmp), "seed comparator before data");
    MDB_val k = val("k"), v = val("B"); ok(mdb_put(parent, upper, &k, &v, 0), "seed B");
    ok(mdb_txn_commit(parent), "seed commit");
    parent = begin(e, 0); aliases(parent, &lower, &upper);
    both_trace(parent, lower, upper, "parent.preload", 0);
    MDB_txn *work = parent;
    if (kind >= 2) {
        int rc = mdb_txn_begin(e, parent, 0, &work);
        printf("child.begin.rc=%d\n", rc);
        if (rc) { mdb_txn_abort(parent); mdb_env_close(e); return; }
        /* Parent operations remain suspended until child commit/abort. */
        both_trace(work, lower, upper, "child.preload", 0);
    }
    int first_lower = kind != 1;
    int rc = write_alias(work, first_lower ? lower : upper, first_lower);
    if (!rc) {
        both_trace(work, lower, upper, "after.first", 0);
        rc = write_alias(work, first_lower ? upper : lower, !first_lower);
    }
    if (rc) {
        puts("write-error-abort-owned-transactions");
        mdb_txn_abort(work); if (work != parent) mdb_txn_abort(parent);
        mdb_env_close(e); return;
    }
    both_trace(work, lower, upper, "after.both", 1);
    if (kind >= 2) {
        if (kind == 3) { mdb_txn_abort(work); puts("child.abort.done=1"); }
        else {
            rc = mdb_txn_commit(work); printf("child.commit.rc=%d\n", rc);
            if (rc) { mdb_txn_abort(parent); mdb_env_close(e); return; }
        }
        both_trace(parent, lower, upper, "parent.after_child", 1);
    }
    rc = mdb_txn_commit(parent); printf("outer.commit.rc=%d\n", rc);
    if (rc) { mdb_env_close(e); return; }
    after_commit(e, "new_txn"); mdb_env_close(e);
    e = env(path, MDB_RDONLY); after_commit(e, "reopen"); mdb_env_close(e);
}
int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    Dl_info provider; need(dladdr((void *)mdb_env_open, &provider) && provider.dli_fname, "provider");
    fprintf(stderr, "PROVIDER %s\n", provider.dli_fname);
    if (argc != 3) return 2;
    char *end; long kind = strtol(argv[1], &end, 10);
    if (*end || kind < 0 || kind > 3) return 2;
    const char *names[] = {"lower-then-upper", "upper-then-lower", "child-lower-upper-release", "child-lower-upper-abort"};
    printf("scenario=%s\n", names[kind]); run_case(argv[2], (int)kind);
    puts("dual-writes-probe-complete"); return 0;
}
