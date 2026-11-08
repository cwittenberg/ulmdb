/* Reference groundwork: public comparator-equal aliases and private writes.
 * The helper is an unchanged frozen copy of the prior proper-lifecycle probe. */
#define main frozen_named_fold_main
#include "named_fold_frozen.h"
#undef main

static void trace_value(MDB_txn *t, MDB_dbi db, const char *key, const char *label) {
    MDB_val k = val(key), v = {0, NULL};
    int rc = mdb_get(t, db, &k, &v);
    printf("%s.rc=%d", label, rc);
    if (!rc) printf(" len=%zu value=%.*s", v.mv_size, (int)v.mv_size, (char *)v.mv_data);
    putchar('\n');
}
static void trace_alias(MDB_txn *t, MDB_dbi db, const char *label) {
    unsigned flags = ~0u;
    int rc = mdb_dbi_flags(t, db, &flags);
    printf("%s.flags.rc=%d", label, rc);
    if (!rc) printf(" value=%u", flags);
    putchar('\n');
    char text[128];
    snprintf(text, sizeof text, "%s.k", label); trace_value(t, db, "k", text);
    snprintf(text, sizeof text, "%s.n", label); trace_value(t, db, "n", text);
}
static void put_text(MDB_txn *t, MDB_dbi db, const char *key, const char *value, const char *label) {
    MDB_val k = val(key), v = val(value);
    int rc = mdb_put(t, db, &k, &v, 0);
    printf("%s.rc=%d\n", label, rc); ok(rc, label);
}
static MDB_dbi open_upper(MDB_txn *t, unsigned flags) {
    MDB_dbi db;
    int rc = mdb_dbi_open(t, "X", flags, &db);
    printf("open.upper.rc=%d\n", rc); ok(rc, "open upper");
    ok(mdb_set_compare(t, db, upper_cmp), "upper comparator before alias data");
    return db;
}
static MDB_dbi open_lower(MDB_txn *t) {
    MDB_dbi db;
    int rc = mdb_dbi_open(t, "x", 0, &db);
    printf("open.lower.rc=%d\n", rc); ok(rc, "open lower");
    ok(mdb_set_compare(t, db, lower_cmp), "lower comparator before alias data");
    return db;
}
static void committed_view(MDB_env *e) {
    MDB_txn *t = begin(e, MDB_RDONLY); main_fold(t);
    MDB_dbi lower = open_lower(t), upper = open_upper(t, 0);
    printf("committed.ids.lower=%u upper=%u distinct=%d\n", lower, upper, lower != upper);
    trace_alias(t, lower, "committed.lower"); trace_alias(t, upper, "committed.upper");
    mdb_txn_abort(t);
}
static void journey(const char *path, int kind) {
    need(!mkdir(path, 0700), "fresh journey");
    MDB_env *e = env(path, 0); MDB_txn *t;
    if (kind != 2) {
        t = begin(e, 0); main_fold(t); MDB_dbi upper = open_upper(t, MDB_CREATE);
        put_text(t, upper, "k", "B", "seed.B"); ok(mdb_txn_commit(t), "seed commit");
    }
    t = begin(e, 0); main_fold(t); MDB_dbi upper = open_upper(t, kind == 2 ? MDB_CREATE : 0), lower = 0;
    if (kind == 0) {
        lower = open_lower(t);
        trace_alias(t, lower, "before.lower"); trace_alias(t, upper, "before.upper");
    } else if (kind == 1) {
        trace_alias(t, upper, "before.upper");
    }
    put_text(t, upper, "k", kind == 2 ? "B" : "NEW", "private.upper.k");
    put_text(t, upper, "n", "ADD", "private.upper.n");
    if (kind != 0) lower = open_lower(t);
    printf("private.ids.lower=%u upper=%u distinct=%d\n", lower, upper, lower != upper);
    trace_alias(t, lower, "private.lower"); trace_alias(t, upper, "private.upper");
    authority(t, lower, 1, "private.lower.cmp"); authority(t, upper, 0, "private.upper.cmp");
    int rc = mdb_txn_commit(t); printf("private.commit.rc=%d\n", rc); ok(rc, "private commit");
    committed_view(e); mdb_env_close(e);
    e = env(path, MDB_RDONLY); committed_view(e); mdb_env_close(e);
}
int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    Dl_info provider; need(dladdr((void *)mdb_env_open, &provider) && provider.dli_fname, "provider");
    fprintf(stderr, "PROVIDER %s\n", provider.dli_fname);
    if (argc != 2) return 2;
    need(!mkdir(argv[1], 0700), "fresh root");
    const char *names[] = {"preload-both-then-write-upper", "open-lower-after-upper-private-write", "new-X-private-write-before-lower-open"};
    for (int kind = 0; kind != 3; ++kind) {
        char path[4096]; int n = snprintf(path, sizeof path, "%s/case%d", argv[1], kind);
        need(n > 0 && (size_t)n < sizeof path, "path");
        printf("scenario=%s\n", names[kind]); journey(path, kind);
    }
    printf("write-views-complete callback-errors=%d\n", errors);
    return errors ? 1 : 0;
}
