/* Pure global comparator metadata remains usable on an old invalid DBI slot. */
#include <lmdb.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

struct mode { const char *name; unsigned flags; int custom_key, custom_dup; };
static const struct mode modes[] = {
    {"kv-default", 0, 0, 0},
    {"kv-reverse", MDB_REVERSEKEY, 0, 0},
    {"kv-integer", MDB_INTEGERKEY, 0, 0},
    {"kv-custom", 0, 1, 0},
    {"dup-default", MDB_DUPSORT, 0, 0},
    {"dup-reverse", MDB_DUPSORT | MDB_REVERSEDUP, 0, 0},
    {"dup-integer", MDB_DUPSORT | MDB_DUPFIXED | MDB_INTEGERDUP, 0, 0},
    {"dup-custom", MDB_DUPSORT, 1, 1}
};
static unsigned key_calls, dup_calls;
static void need(int yes, const char *label) {
    if (!yes) { fprintf(stderr, "catalog comparison oracle: %s\n", label); exit(1); }
}
static void ok(int code, const char *label) {
    if (code) { fprintf(stderr, "%s: %s (%d)\n", label, mdb_strerror(code), code); exit(1); }
}
static int bytes(const MDB_val *a, const MDB_val *b) {
    size_t size = a->mv_size < b->mv_size ? a->mv_size : b->mv_size;
    int order = memcmp(a->mv_data, b->mv_data, size);
    return order ? order : (a->mv_size > b->mv_size) - (a->mv_size < b->mv_size);
}
static int custom_key(const MDB_val *a, const MDB_val *b) { ++key_calls; return bytes(b, a); }
static int custom_dup(const MDB_val *a, const MDB_val *b) { ++dup_calls; return bytes(b, a); }
static MDB_val text(const char *value) { MDB_val v = {strlen(value), (void *)value}; return v; }
static int sign(int value) { return (value > 0) - (value < 0); }
static void comparison(int actual, int expected, const char *label) {
    printf("%s=%d\n", label, sign(actual));
    need(sign(actual) == expected, label);
}
static void callbacks(unsigned actual, unsigned expected, const char *label) {
    printf("%s=%u\n", label, actual);
    need(actual == expected, label);
}
static void register_callbacks(MDB_txn *txn, MDB_dbi db, struct mode mode) {
    if (mode.custom_key) ok(mdb_set_compare(txn, db, custom_key), "set_compare_before_data");
    if (mode.custom_dup) ok(mdb_set_dupsort(txn, db, custom_dup), "set_dupsort_before_data");
}
static void seed_data(MDB_txn *txn, MDB_dbi db, struct mode mode,
                      MDB_val left_key, MDB_val right_key, MDB_val left_dup, MDB_val right_dup) {
    if (mode.flags & MDB_DUPSORT) {
        ok(mdb_put(txn, db, &left_key, &left_dup, 0), "put_left_duplicate");
        ok(mdb_put(txn, db, &left_key, &right_dup, 0), "put_right_duplicate");
    } else {
        MDB_val one = text("one"), two = text("two");
        ok(mdb_put(txn, db, &left_key, &one, 0), "put_left_key");
        ok(mdb_put(txn, db, &right_key, &two, 0), "put_right_key");
    }
}
static void invalid_data(MDB_txn *txn, MDB_dbi db, MDB_val key, const char *stage) {
    unsigned flags = ~0u;
    MDB_val value;
    int flagged = mdb_dbi_flags(txn, db, &flags);
    int got = mdb_get(txn, db, &key, &value);
    printf("%s.flags=%d get=%d\n", stage, flagged, got);
    need(flagged == EINVAL && got == EINVAL, "comparison does not grant transaction validity");
}
static void scenario(const char *path, struct mode mode, int persisted, int notls) {
    need(!mkdir(path, 0700), "fresh scenario");
    unsigned char binary_a[] = {0x01, 0xff}, binary_b[] = {0x02, 0x00};
    uint32_t integer_a = 1, integer_b = 256;
    MDB_val left_key = {sizeof binary_a, binary_a}, right_key = {sizeof binary_b, binary_b};
    MDB_val left_dup = left_key, right_dup = right_key;
    if (mode.flags & MDB_INTEGERKEY) {
        left_key.mv_size = right_key.mv_size = sizeof integer_a;
        left_key.mv_data = &integer_a; right_key.mv_data = &integer_b;
    }
    if (mode.flags & MDB_INTEGERDUP) {
        left_dup.mv_size = right_dup.mv_size = sizeof integer_a;
        left_dup.mv_data = &integer_a; right_dup.mv_data = &integer_b;
    }
    MDB_env *env;
    MDB_txn *txn, *old;
    MDB_dbi seed, exported, alias;
    const char *target = persisted ? "stored" : "future";
    ok(mdb_env_create(&env), "env_create");
    ok(mdb_env_set_maxdbs(env, 1), "maxdbs");
    ok(mdb_env_set_mapsize(env, 16u * 1024u * 1024u), "mapsize");
    ok(mdb_env_open(env, path, notls ? MDB_NOTLS : 0, 0600), "env_open");
    ok(mdb_txn_begin(env, NULL, 0, &txn), "seed_begin");
    ok(mdb_dbi_open(txn, "stored", MDB_CREATE | (persisted ? mode.flags : 0), &seed), "seed_open");
    if (persisted) {
        register_callbacks(txn, seed, mode);
        seed_data(txn, seed, mode, left_key, right_key, left_dup, right_dup);
    } else {
        MDB_val key = text("k"), value = text("seed");
        ok(mdb_put(txn, seed, &key, &value, 0), "seed_slot");
    }
    ok(mdb_txn_commit(txn), "seed_commit");
    mdb_dbi_close(env, seed); /* This closed numeric seed handle is never reused. */
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &old), "old_reader");
    ok(mdb_txn_begin(env, NULL, 0, &txn), "export_begin");
    ok(mdb_dbi_open(txn, target, persisted ? 0 : MDB_CREATE | mode.flags, &exported), "export_open");
    /* Proper reopen resets callbacks. Restore the SAME ordering before access.
       In persisted default/flag modes, exporting open goes straight to commit:
       no flags/get call can initialize global comparator metadata incidentally. */
    register_callbacks(txn, exported, mode);
    if (!persisted) seed_data(txn, exported, mode, left_key, right_key, left_dup, right_dup);
    ok(mdb_txn_commit(txn), "export_commit");
    ok(mdb_dbi_open(old, target, 0, &alias), "old_alias_open");
    need(alias == exported, "same exported alias");
    puts("old.alias.open=0 same=1");
    invalid_data(old, alias, left_key, "before");
    key_calls = dup_calls = 0;
    int key_order = mode.custom_key || (mode.flags & MDB_REVERSEKEY) ? 1 : -1;
    comparison(mdb_cmp(old, alias, &left_key, &right_key), key_order, "cmp.forward");
    comparison(mdb_cmp(old, alias, &right_key, &left_key), -key_order, "cmp.backward");
    if (mode.flags & MDB_DUPSORT) {
        int dup_order = mode.custom_dup || (mode.flags & MDB_REVERSEDUP) ? 1 : -1;
        comparison(mdb_dcmp(old, alias, &left_dup, &right_dup), dup_order, "dcmp.forward");
        comparison(mdb_dcmp(old, alias, &right_dup, &left_dup), -dup_order, "dcmp.backward");
    }
    callbacks(key_calls, mode.custom_key ? 2 : 0, "callbacks.key");
    callbacks(dup_calls, mode.custom_dup ? 2 : 0, "callbacks.dup");
    invalid_data(old, alias, left_key, "after");
    mdb_txn_abort(old);
    mdb_env_close(env);
}
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    if (mkdir(argv[1], 0700) && errno != EEXIST) return 1;
    puts("catalog-comparison-version=1");
    for (size_t mode = 0; mode < sizeof modes / sizeof modes[0]; ++mode)
        for (int persisted = 0; persisted < 2; ++persisted)
            for (int notls = 0; notls < 2; ++notls) {
                char name[128], path[4096];
                int count = snprintf(name, sizeof name, "%s-%s-%s", modes[mode].name,
                    persisted ? "persisted" : "new", notls ? "notls" : "tls");
                need(count > 0 && (size_t)count < sizeof name, "scenario name");
                count = snprintf(path, sizeof path, "%s/%s", argv[1], name);
                need(count > 0 && count < (int)sizeof path, "scenario path");
                printf("scenario=%s\n", name);
                scenario(path, modes[mode], persisted, notls);
            }
    puts("catalog-comparison-complete");
    return 0;
}
