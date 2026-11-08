/* Actual C writer retained across selected-file force-sync EIO.
 * Requires a fresh, genuinely initialized Native file directory. The helper
 * performs no business operations. Build once with the pinned modern header.
 */
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define NEED(x) do { if (!(x)) { fprintf(stderr, "retained-sync oracle: %s\n", #x); exit(1); } } while (0)
#define OK(x) do { int rc = (x); if (rc) { fprintf(stderr, "%s=%d\n", #x, rc); exit(1); } } while (0)
static MDB_val text(const char *s) { MDB_val v = {strlen(s), (void *)s}; return v; }
static void phase(const char *s, int begin) { fprintf(stderr, "@@%s %s@@\n", begin ? "BEGIN" : "END", s); fflush(stderr); }
static void put(MDB_txn *t, MDB_dbi db, const char *key, const char *value) {
    MDB_val k = text(key), v = text(value); OK(mdb_put(t, db, &k, &v, 0));
}
static void expect(MDB_txn *t, MDB_dbi db, const char *value) {
    MDB_val k = text("k"), v; OK(mdb_get(t, db, &k, &v));
    NEED(v.mv_size == strlen(value) && !memcmp(v.mv_data, value, v.mv_size));
}
static void large(MDB_txn *t, MDB_dbi db) {
    unsigned char bytes[8192]; memset(bytes, 0x5a, sizeof bytes);
    MDB_val k = text("large"), v = {sizeof bytes, bytes}; OK(mdb_put(t, db, &k, &v, 0));
}
static void expect_large(MDB_txn *t, MDB_dbi db) {
    MDB_val k = text("large"), v; OK(mdb_get(t, db, &k, &v));
    NEED(v.mv_size == 8192);
    for (size_t i = 0; i < v.mv_size; ++i) NEED(((const unsigned char *)v.mv_data)[i] == 0x5a);
}
static MDB_env *open_env(const char *path, unsigned policy) {
    MDB_env *e; OK(mdb_env_create(&e)); OK(mdb_env_set_maxdbs(e, 4));
    OK(mdb_env_set_mapsize(e, 4u * 1024u * 1024u));
    OK(mdb_env_open(e, path, policy | MDB_NOTLS, 0600)); return e;
}
int main(int argc, char **argv) {
    NEED(argc == 4); setvbuf(stdout, NULL, _IONBF, 0);
    const char *state = argv[2]; unsigned policy = 0;
    if (!strcmp(argv[3], "nosync")) policy = MDB_NOSYNC;
    else if (!strcmp(argv[3], "nometasync")) policy = MDB_NOMETASYNC;
    else NEED(!strcmp(argv[3], "full"));
    int clean = !strcmp(state, "clean"), prepared = !strcmp(state, "prepared");
    NEED(clean || prepared || !strcmp(state, "dirty"));
    MDB_env *e = open_env(argv[1], policy); MDB_txn *seed, *old, *held; MDB_dbi db, olddb;
    phase("seed.commit", 1); OK(mdb_txn_begin(e, NULL, 0, &seed)); OK(mdb_dbi_open(seed, NULL, 0, &db));
    put(seed, db, "k", "A"); OK(mdb_txn_commit(seed)); phase("seed.commit", 0);
    olddb = db; OK(mdb_txn_begin(e, NULL, MDB_RDONLY, &old));
    NEED(mdb_txn_id(old) == 1); expect(old, olddb, "A");
    MDB_val k = text("k"), borrowed; OK(mdb_get(old, olddb, &k, &borrowed));
    OK(mdb_txn_begin(e, NULL, 0, &held));
    NEED(mdb_txn_id(held) == 2);
    if (!clean) { put(held, db, "k", "B"); large(held, db); }
    MDB_cursor *cursor = NULL;
    if (prepared) { phase("retained.prepare", 1); OK(mdb_txn_prepare(held)); phase("retained.prepare", 0); }
    else { MDB_val v; OK(mdb_cursor_open(held, db, &cursor)); k = text("k"); OK(mdb_cursor_get(cursor, &k, &v, MDB_SET)); }
    printf("retained.state=%s policy=%s\n", state, argv[3]);
    phase("retained.sync.force1", 1); int rc = mdb_env_sync(e, 1); phase("retained.sync.force1", 0);
    printf("retained.sync.force1=%d\n", rc); NEED(rc == EIO);
    NEED(borrowed.mv_size == 1 && !memcmp(borrowed.mv_data, "A", 1));
    expect(old, olddb, "A"); NEED(mdb_txn_id(old) == 1);
    if (!prepared) {
        MDB_val v; OK(mdb_cursor_get(cursor, &k, &v, MDB_GET_CURRENT));
        const char *want = clean ? "A" : "B";
        NEED(v.mv_size == 1 && !memcmp(v.mv_data, want, 1)); mdb_cursor_close(cursor);
        expect(held, db, want);
        if (clean) { put(held, db, "k", "B"); large(held, db); }
        else expect_large(held, db);
        put(held, db, "after-error", "usable");
    }
    phase("retained.commit", 1); OK(mdb_txn_commit(held)); phase("retained.commit", 0);
    expect(old, olddb, "A"); NEED(borrowed.mv_size == 1 && !memcmp(borrowed.mv_data, "A", 1));
    mdb_txn_abort(old);
    MDB_txn *fresh; OK(mdb_txn_begin(e, NULL, MDB_RDONLY, &fresh));
    NEED(mdb_txn_id(fresh) == 2);
    expect(fresh, db, "B"); expect_large(fresh, db); mdb_txn_abort(fresh);
    phase("retained.after.sync", 1); OK(mdb_env_sync(e, 1)); phase("retained.after.sync", 0); mdb_env_close(e);
    e = open_env(argv[1], 0); OK(mdb_txn_begin(e, NULL, MDB_RDONLY, &fresh));
    OK(mdb_dbi_open(fresh, NULL, 0, &db)); NEED(mdb_txn_id(fresh) == 2);
    expect(fresh, db, "B"); expect_large(fresh, db); mdb_txn_abort(fresh); mdb_env_close(e);
    puts("retained-sync EIO: original writer committed once; old borrow stable; reopen B/8192 PASS");
    return 0;
}
