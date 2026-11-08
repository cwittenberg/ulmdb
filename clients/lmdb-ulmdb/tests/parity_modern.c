/* Compile against the pinned LMDB 1.0 header; run the executable unchanged. */
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>

typedef int (*txn_flags_function)(MDB_txn *, unsigned int *);
_Static_assert(_Generic(&mdb_txn_flags, txn_flags_function: 1, default: 0),
               "mdb_txn_flags must have the LMDB 1.0 signature");

static void ok(int code, const char *operation) {
    if (code) {
        fprintf(stderr, "%s: %s (%d)\n", operation, mdb_strerror(code), code);
        exit(1);
    }
}
static void flags(MDB_txn *txn, const char *label, unsigned expected) {
    unsigned value = ~0u;
    int code = mdb_txn_flags(txn, &value);
    printf("flags.%s code=%d value=%u\n", label, code, value);
    if (code || value != expected) exit(1);
}
static void put(MDB_txn *txn, MDB_dbi db, char *contents) {
    char key_bytes[] = "key";
    MDB_val key = {sizeof key_bytes - 1, key_bytes};
    MDB_val value = {1, contents};
    ok(mdb_put(txn, db, &key, &value, 0), "put");
}
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    if (mkdir(argv[1], 0700) && errno != EEXIST) return 1;
    puts("modern-transaction-flags-version=1");
    puts("scenario=F01-negative-null-transaction");
    unsigned unchanged = 123;
    int invalid = mdb_txn_flags(NULL, &unchanged);
    printf("flags.null code=%d preserved=%d\n", invalid, unchanged == 123);
    if (invalid != EINVAL || unchanged != 123) return 1;

    MDB_env *env;
    ok(mdb_env_create(&env), "env_create");
    ok(mdb_env_set_mapsize(env, 8u * 1024u * 1024u), "mapsize");
    ok(mdb_env_set_maxdbs(env, 4), "maxdbs");
    ok(mdb_env_open(env, argv[1], MDB_NOTLS, 0600), "env_open");
    MDB_txn *writer, *child;
    MDB_dbi db;
    puts("scenario=F01-positive-transaction-lifecycle");
    ok(mdb_txn_begin(env, NULL, 0, &writer), "writer_begin");
    flags(writer, "writer-clean", 0);
    ok(mdb_dbi_open(writer, "records", MDB_CREATE, &db), "dbi_open");
    put(writer, db, "a");
    flags(writer, "writer-dirty", 0);
    ok(mdb_txn_begin(env, writer, 0, &child), "child_begin");
    flags(child, "child-abort", 0);
    flags(writer, "parent-suspended", 0);
    put(child, db, "b");
    mdb_txn_abort(child);
    flags(writer, "parent-after-abort", 0);
    ok(mdb_txn_begin(env, writer, 0, &child), "child_begin_again");
    put(child, db, "c");
    flags(child, "child-dirty", 0);
    ok(mdb_txn_commit(child), "child_commit");
    flags(writer, "parent-after-commit", 0);
    ok(mdb_txn_commit(writer), "writer_commit");

    MDB_txn *reader;
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &reader), "reader_begin");
    flags(reader, "reader-active", MDB_RDONLY);
    mdb_txn_reset(reader);
    flags(reader, "reader-reset", MDB_RDONLY);
    ok(mdb_txn_renew(reader), "reader_renew");
    flags(reader, "reader-renewed", MDB_RDONLY);
    ok(mdb_txn_commit(reader), "reader_commit");
    mdb_env_close(env);

    puts("scenario=F01-reopen-readonly-environment");
    ok(mdb_env_create(&env), "readonly_env_create");
    ok(mdb_env_open(env, argv[1], MDB_RDONLY | MDB_NOTLS, 0600), "readonly_env_open");
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &reader), "readonly_reader_begin");
    flags(reader, "readonly-env", MDB_RDONLY);
    mdb_txn_reset(reader);
    flags(reader, "readonly-env-reset", MDB_RDONLY);
    mdb_txn_abort(reader);
    mdb_env_close(env);
    return 0;
}
