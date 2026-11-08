/* Observation before expected assertions: re-arm a loaded clean named alias
 * by making MAIN nonempty after clear. No database handle is closed dirty. */
#define main frozen_named_fold_main
#include "named_fold_frozen.h"
#undef main

static void point(MDB_txn *t, MDB_dbi d, const char *key, const char *label) {
    MDB_val k = val(key), v = {0, NULL};
    int rc = mdb_get(t, d, &k, &v);
    printf("%s.rc=%d", label, rc);
    if (!rc) printf(" value=%.*s", (int)v.mv_size, (char *)v.mv_data);
    putchar('\n');
}
static int write_point(MDB_txn *t, MDB_dbi d, const char *key, const char *text, const char *label) {
    MDB_val k = val(key), v = val(text);
    int rc = mdb_put(t, d, &k, &v, 0);
    printf("%s.rc=%d\n", label, rc);
    return rc;
}
static void positioned(MDB_cursor *c, const char *label) {
    MDB_val k = {0, NULL}, v = {0, NULL};
    int rc = mdb_cursor_get(c, &k, &v, MDB_GET_CURRENT);
    printf("%s.rc=%d", label, rc);
    if (!rc) printf(" value=%.*s", (int)v.mv_size, (char *)v.mv_data);
    putchar('\n');
}
static MDB_dbi aliases(MDB_txn *t, MDB_dbi *lower, MDB_dbi *upper) {
    MDB_dbi main_db;
    ok(mdb_dbi_open(t, NULL, 0, &main_db), "MAIN open before comparator/data");
    ok(mdb_set_compare(t, main_db, fold), "MAIN comparator before catalog");
    ok(mdb_dbi_open(t, "x", 0, lower), "lower alias");
    ok(mdb_set_compare(t, *lower, lower_cmp), "lower comparator before data");
    ok(mdb_dbi_open(t, "X", 0, upper), "upper alias");
    ok(mdb_set_compare(t, *upper, upper_cmp), "upper comparator before data");
    need(*lower != *upper, "distinct public DBIs");
    return main_db;
}
static void main_keys(MDB_txn *t, MDB_dbi main_db, const char *label) {
    MDB_cursor *c;
    ok(mdb_cursor_open(t, main_db, &c), "MAIN cursor");
    MDB_val k = {0, NULL}, v = {0, NULL};
    int rc = mdb_cursor_get(c, &k, &v, MDB_FIRST);
    unsigned count = 0;
    while (!rc) {
        printf("%s.key%u.hex=", label, count++);
        for (size_t i = 0; i != k.mv_size; ++i) printf("%02x", ((unsigned char *)k.mv_data)[i]);
        putchar('\n');
        rc = mdb_cursor_get(c, &k, &v, MDB_NEXT);
    }
    printf("%s.end.rc=%d\n", label, rc);
    mdb_cursor_close(c);
}
static void reopen(const char *path) {
    MDB_env *e = env(path, MDB_RDONLY);
    MDB_txn *t = begin(e, MDB_RDONLY);
    MDB_dbi lower, upper, main_db = aliases(t, &lower, &upper);
    point(t, lower, "k", "reopen.lower");
    point(t, upper, "k", "reopen.upper");
    point(t, main_db, "anchor", "reopen.anchor");
    main_keys(t, main_db, "reopen.main");
    mdb_txn_abort(t); mdb_env_close(e);
}
int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    Dl_info provider;
    need(dladdr((void *)mdb_env_open, &provider) && provider.dli_fname, "provider");
    fprintf(stderr, "PROVIDER %s\n", provider.dli_fname);
    if (argc != 3 || strcmp(argv[1], "0")) return 2;
    const char *path = argv[2];
    need(!mkdir(path, 0700), "fresh database");
    MDB_env *e = env(path, 0);
    MDB_txn *t = begin(e, 0);
    main_fold(t);
    MDB_dbi upper, lower;
    ok(mdb_dbi_open(t, "X", MDB_CREATE, &upper), "seed upper");
    ok(mdb_set_compare(t, upper, upper_cmp), "seed comparator before data");
    ok(write_point(t, upper, "k", "B", "seed.put"), "seed put");
    ok(mdb_txn_commit(t), "seed commit");
    t = begin(e, MDB_RDONLY);
    (void)aliases(t, &lower, &upper);
    ok(mdb_txn_commit(t), "export clean aliases");
    t = begin(e, 0);
    MDB_dbi main_db = aliases(t, &lower, &upper);
    printf("ids.lower=%u upper=%u\n", lower, upper);
    point(t, lower, "k", "preload.lower");
    point(t, upper, "k", "preload.upper");
    MDB_cursor *lc, *uc;
    ok(mdb_cursor_open(t, lower, &lc), "lower cursor");
    ok(mdb_cursor_open(t, upper, &uc), "upper cursor");
    MDB_val k = val("k"), v;
    ok(mdb_cursor_get(lc, &k, &v, MDB_SET), "position lower");
    k = val("k"); ok(mdb_cursor_get(uc, &k, &v, MDB_SET), "position upper");
    puts("scenario=clean-alias-rearmed-by-ordinary-MAIN-row");
    printf("main.clear.rc=%d\n", mdb_drop(t, main_db, 0));
    int rc = write_point(t, main_db, "anchor", "VISIBLE", "main.anchor.put");
    if (!rc) {
        point(t, main_db, "anchor", "main.anchor.get");
        (void)write_point(t, lower, "k", "REARMED", "lower.rearmed.put");
        point(t, lower, "k", "after.lower"); point(t, upper, "k", "after.upper");
        positioned(lc, "after.lower.cursor"); positioned(uc, "after.upper.cursor");
        main_keys(t, main_db, "before_commit.main");
    }
    mdb_cursor_close(lc); mdb_cursor_close(uc);
    if (rc) {
        mdb_txn_abort(t); mdb_env_close(e);
        fputs("ordinary MAIN insertion failed; owned writer aborted\n", stderr);
        return 2;
    }
    rc = mdb_txn_commit(t);
    printf("outer.commit.rc=%d\n", rc);
    mdb_env_close(e);
    if (!rc) reopen(path);
    puts("clear-rearm-observation-complete");
    return rc ? 2 : 0;
}
