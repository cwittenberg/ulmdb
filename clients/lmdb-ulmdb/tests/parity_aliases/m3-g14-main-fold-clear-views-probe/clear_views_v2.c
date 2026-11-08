/* Observational proper-use reference control. No guessed post-clear rights.
 * Both named aliases stay live; no dirty-handle close occurs. */
#define main frozen_named_fold_main
#include "named_fold_frozen.h"
#undef main

static void point(MDB_txn *t, MDB_dbi d, const char *key, const char *label) {
    MDB_val k = val(key), v = {0, NULL};
    int rc = mdb_get(t, d, &k, &v);
    printf("%s.rc=%d", label, rc);
    if (!rc) printf(" len=%zu value=%.*s", v.mv_size, (int)v.mv_size, (char *)v.mv_data);
    putchar('\n');
}
static void position(MDB_cursor *c, const char *label) {
    MDB_val k = {0, NULL}, v = {0, NULL};
    int rc = mdb_cursor_get(c, &k, &v, MDB_GET_CURRENT);
    printf("%s.rc=%d", label, rc);
    if (!rc) printf(" key=%.*s value=%.*s", (int)k.mv_size, (char *)k.mv_data,
                    (int)v.mv_size, (char *)v.mv_data);
    putchar('\n');
}
static int put_trace(MDB_txn *t, MDB_dbi d, const char *key, const char *text, const char *label) {
    MDB_val k = val(key), v = val(text);
    int rc = mdb_put(t, d, &k, &v, 0);
    printf("%s.rc=%d\n", label, rc);
    return rc;
}
static MDB_dbi aliases(MDB_txn *t, MDB_dbi *lower, MDB_dbi *upper) {
    MDB_dbi main_db;
    ok(mdb_dbi_open(t, NULL, 0, &main_db), "MAIN open before comparator/catalog");
    ok(mdb_set_compare(t, main_db, fold), "MAIN fold before catalog access");
    ok(mdb_dbi_open(t, "x", 0, lower), "lower alias");
    ok(mdb_set_compare(t, *lower, lower_cmp), "lower byte-equivalent callback before data");
    ok(mdb_dbi_open(t, "X", 0, upper), "upper alias");
    ok(mdb_set_compare(t, *upper, upper_cmp), "upper byte-equivalent callback before data");
    need(*lower != *upper, "distinct aliases");
    return main_db;
}
static void read_after_reopen(const char *path) {
    MDB_env *e = env(path, MDB_RDONLY);
    MDB_txn *t = begin(e, MDB_RDONLY);
    main_fold(t);
    const char *names[] = {"x", "X"};
    for (unsigned i = 0; i != 2; ++i) {
        MDB_dbi d = 0;
        int rc = mdb_dbi_open(t, names[i], 0, &d);
        printf("reopen.%s.open.rc=%d\n", names[i], rc);
        if (!rc) {
            ok(mdb_set_compare(t, d, i ? upper_cmp : lower_cmp), "reopen comparator before data");
            char label[80];
            snprintf(label, sizeof label, "reopen.%s.k", names[i]); point(t, d, "k", label);
            snprintf(label, sizeof label, "reopen.%s.q", names[i]); point(t, d, "q", label);
        }
    }
    mdb_txn_abort(t);
    mdb_env_close(e);
}
static int run_case(const char *path, int kind) {
    need(!mkdir(path, 0700), "fresh case");
    MDB_env *e = env(path, 0);
    MDB_txn *t = begin(e, 0);
    main_fold(t);
    MDB_dbi upper, lower;
    ok(mdb_dbi_open(t, "X", MDB_CREATE, &upper), "seed upper");
    ok(mdb_set_compare(t, upper, upper_cmp), "seed comparator before data");
    MDB_val k = val("k"), v = val("B");
    ok(mdb_put(t, upper, &k, &v, 0), "seed B");
    ok(mdb_txn_commit(t), "seed commit");
    /* Export clean lower alias before the measured writer so begin marks both
     * globally valid DBIs stale. No closed handle is reused. */
    t = begin(e, MDB_RDONLY);
    (void)aliases(t, &lower, &upper);
    ok(mdb_txn_commit(t), "clean alias export");
    t = begin(e, 0);
    MDB_dbi main_db = aliases(t, &lower, &upper);
    point(t, lower, "k", "preload.lower");
    point(t, upper, "k", "preload.upper");
    MDB_cursor *lc = NULL, *uc = NULL;
    ok(mdb_cursor_open(t, lower, &lc), "lower cursor");
    ok(mdb_cursor_open(t, upper, &uc), "upper cursor");
    k = val("k"); ok(mdb_cursor_get(lc, &k, &v, MDB_SET), "position lower");
    k = val("k"); ok(mdb_cursor_get(uc, &k, &v, MDB_SET), "position upper");
    MDB_dbi dirty = kind == 1 ? lower : upper;
    MDB_dbi clean = kind == 1 ? upper : lower;
    printf("ids.lower=%u upper=%u dirty=%u clean=%u\n", lower, upper, dirty, clean);
    MDB_txn *work = t;
    if (kind >= 2) ok(mdb_txn_begin(e, t, 0, &work), "child begin");
    if (put_trace(work, dirty, "k", "DIRTY", "before_clear.dirty.put")) {
        if (work != t) mdb_txn_abort(work);
        mdb_txn_abort(t); mdb_env_close(e);
        fputs("pre-clear mutation failed; remaining owned transactions aborted\n", stderr);
        return 2;
    }
    if (kind == 2) { mdb_txn_abort(work); puts("child.abort.done=1"); }
    if (kind == 3) {
        int rc = mdb_txn_commit(work);
        printf("child.release.rc=%d\n", rc);
        if (rc) {
            mdb_txn_abort(t); mdb_env_close(e);
            fputs("child release failed; remaining parent aborted\n", stderr);
            return 2;
        }
    }
    /* No parent operation occurs while the nested writer is active. */
    point(t, lower, "k", "before_clear.lower");
    point(t, upper, "k", "before_clear.upper");
    printf("main.clear.rc=%d\n", mdb_drop(t, main_db, 0));
    point(t, lower, "k", "after_clear.lower");
    point(t, upper, "k", "after_clear.upper");
    position(lc, "after_clear.lower.cursor");
    position(uc, "after_clear.upper.cursor");
    /* The exact small-path put fails directly from cursor_touch if the cleared
     * MAIN descriptor is absent. It returns without poisoning the transaction.
     * No oversized pages, spill, invalid handle or dirty close are involved. */
    put_trace(t, clean, "k", "CLEAN", "after_clear.clean.put");
    point(t, clean, "k", "after_clean_put.clean");
    position(kind == 1 ? uc : lc, "after_clean_put.clean.cursor");
    put_trace(t, dirty, "q", "LATE", "after_clear.dirty.put");
    point(t, dirty, "k", "after_dirty_put.dirty.k");
    point(t, dirty, "q", "after_dirty_put.dirty.q");
    position(kind == 1 ? lc : uc, "after_dirty_put.dirty.cursor");
    mdb_cursor_close(lc); mdb_cursor_close(uc);
    printf("outer.commit.rc=%d\n", mdb_txn_commit(t));
    mdb_env_close(e);
    read_after_reopen(path);
    return 0;
}
int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    Dl_info provider;
    need(dladdr((void *)mdb_env_open, &provider) && provider.dli_fname, "provider");
    fprintf(stderr, "PROVIDER %s\n", provider.dli_fname);
    if (argc != 3) return 2;
    char *end;
    long kind = strtol(argv[1], &end, 10);
    if (*end || kind < 0 || kind > 3) return 2;
    const char *names[] = {"dirty-upper-clean-lower", "dirty-lower-clean-upper",
                           "child-dirty-abort-clean-both", "child-dirty-release"};
    printf("scenario=%s\n", names[kind]);
    int rc = run_case(argv[2], (int)kind);
    if (rc) return rc;
    puts("clean-sibling-clear-observation-complete");
    return 0;
}
