/* Unchanged application: alias export does not alter an old reader's validity. */
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

static void need(int yes, const char *label) {
    if (!yes) { fprintf(stderr, "catalog visibility oracle: %s\n", label); exit(1); }
}
static void ok(int code, const char *label) {
    if (code) { fprintf(stderr, "%s: %s (%d)\n", label, mdb_strerror(code), code); exit(1); }
}
static MDB_val text(const char *value) {
    MDB_val result = {strlen(value), (void *)value};
    return result;
}
static void put(MDB_txn *txn, MDB_dbi db, const char *contents) {
    MDB_val key = text("k"), value = text(contents);
    ok(mdb_put(txn, db, &key, &value, 0), "seed_put");
}
static void flags(MDB_txn *txn, MDB_dbi db, const char *label, int expected, unsigned contents) {
    unsigned actual = ~0u;
    int code = mdb_dbi_flags(txn, db, &actual);
    printf("%s=%d", label, code);
    if (!code) printf(" flags=%u", actual);
    putchar('\n');
    need(code == expected, label);
    if (!code) need(actual == contents, label);
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
static void scenario(const char *path, int existing, int duplicates, int renew, int notls) {
    need(!mkdir(path, 0700), "fresh scenario");
    MDB_env *env;
    MDB_txn *txn, *old;
    MDB_dbi seed, exported, alias;
    const char *target = existing ? "stored" : "future";
    const char *contents = duplicates ? "aa" : "old";
    unsigned dbflags = duplicates ? MDB_DUPSORT | MDB_DUPFIXED : 0;
    ok(mdb_env_create(&env), "env_create");
    ok(mdb_env_set_maxdbs(env, 1), "maxdbs");
    ok(mdb_env_set_mapsize(env, 16u * 1024u * 1024u), "mapsize");
    ok(mdb_env_open(env, path, notls ? MDB_NOTLS : 0, 0600), "env_open");
    ok(mdb_txn_begin(env, NULL, 0, &txn), "seed_begin");
    ok(mdb_dbi_open(txn, "stored", MDB_CREATE | dbflags, &seed), "seed_open");
    put(txn, seed, contents);
    if (duplicates) put(txn, seed, "bb");
    ok(mdb_txn_commit(txn), "seed_commit");
    /* Keep the slot within env->me_numdbs, but invalid at the old reader's
       start. Never use this closed seed handle again. */
    mdb_dbi_close(env, seed);
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &old), "old_begin");

    /* The old reader has not called dbi_open. This writer finishes before its
       first such call; no later writer overlaps its dbi_open-use interval. */
    ok(mdb_txn_begin(env, NULL, 0, &txn), "export_begin");
    ok(mdb_dbi_open(txn, target, existing ? 0 : MDB_CREATE | dbflags, &exported), "export_open");
    if (!existing) {
        put(txn, exported, contents);
        if (duplicates) put(txn, exported, "bb");
    }
    ok(mdb_txn_commit(txn), "export_commit");
    puts("writer.alias.export=0");
    ok(mdb_dbi_open(old, target, 0, &alias), "old_alias_open");
    need(alias == exported, "same exported alias");
    puts("old.alias.open=0 same=1");
    flags(old, alias, "old.flags.first", EINVAL, 0);
    get(old, alias, "old.get.first", EINVAL, NULL);
    /* These failures perform no write and do not make the slot valid. */
    flags(old, alias, "old.flags.after-failed-get", EINVAL, 0);
    get(old, alias, "old.get.repeat", EINVAL, NULL);
    if (renew) {
        mdb_txn_reset(old);
        ok(mdb_txn_renew(old), "reader_renew");
        puts("reader.reset-renew=0");
        ok(mdb_dbi_open(old, target, 0, &alias), "renewed_alias_open");
        flags(old, alias, "renewed.flags", 0, dbflags);
        get(old, alias, "renewed.get", 0, contents);
    }
    mdb_txn_abort(old);
    old = NULL;
    /* A fresh reader begins after the prior dbi_open user has ended. */
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn), "fresh_begin");
    ok(mdb_dbi_open(txn, target, 0, &alias), "fresh_alias_open");
    flags(txn, alias, "fresh.flags", 0, dbflags);
    get(txn, alias, "fresh.get", 0, contents);
    ok(mdb_txn_commit(txn), "fresh_reader_commit");
    mdb_env_close(env);
}
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    if (mkdir(argv[1], 0700) && errno != EEXIST) return 1;
    puts("catalog-visibility-version=1");
    for (int existing = 0; existing < 2; ++existing)
        for (int duplicates = 0; duplicates < 2; ++duplicates)
            for (int renew = 0; renew < 2; ++renew)
                for (int notls = 0; notls < 2; ++notls) {
                    char name[128], path[4096];
                    int count = snprintf(name, sizeof name, "%s-%s-%s-%s",
                        existing ? "persisted" : "new", duplicates ? "dupfixed" : "kv",
                        renew ? "renew" : "fresh", notls ? "notls" : "tls");
                    need(count > 0 && (size_t)count < sizeof name, "scenario name");
                    count = snprintf(path, sizeof path, "%s/%s", argv[1], name);
                    need(count > 0 && count < (int)sizeof path, "scenario path");
                    printf("scenario=%s\n", name);
                    scenario(path, existing, duplicates, renew, notls);
                }
    puts("catalog-visibility-complete");
    return 0;
}
