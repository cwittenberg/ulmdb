/* V2 adapter-only weak-source export/borrow oracle. All business calls use C.
 * Export data once in the seed commit; overlapping source txns never dbi_open. */
#include <lmdb.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#define REQUIRE(x) do { if (!(x)) { fprintf(stderr, "copy policy oracle: %s\n", #x); exit(1); } } while (0)
#define OK(x) do { int rc = (x); if (rc) { fprintf(stderr, "%s=%d\n", #x, rc); exit(1); } } while (0)
static MDB_val text(const char *s) { MDB_val v = {strlen(s), (void *)s}; return v; }
static MDB_env *open_env(const char *path, unsigned flags) {
    MDB_env *env; OK(mdb_env_create(&env)); OK(mdb_env_set_maxdbs(env, 8));
    OK(mdb_env_open(env, path, flags | MDB_NOTLS, 0600)); return env;
}
static MDB_dbi seed(MDB_env *env, const char *value) {
    MDB_txn *txn; MDB_dbi db; MDB_val key = text("key"), data = text(value);
    OK(mdb_txn_begin(env, NULL, 0, &txn));
    OK(mdb_dbi_open(txn, "data", MDB_CREATE, &db));
    OK(mdb_put(txn, db, &key, &data, 0)); OK(mdb_txn_commit(txn)); return db;
}
static MDB_dbi open_data(MDB_env *env) {
    MDB_txn *txn; MDB_dbi db;
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn));
    OK(mdb_dbi_open(txn, "data", 0, &db)); OK(mdb_txn_commit(txn)); return db;
}
static void put(MDB_env *env, MDB_dbi db, const char *value) {
    MDB_txn *txn; MDB_val key = text("key"), data = text(value);
    OK(mdb_txn_begin(env, NULL, 0, &txn));
    OK(mdb_put(txn, db, &key, &data, 0)); OK(mdb_txn_commit(txn));
}
static void expect(MDB_env *env, MDB_dbi db, const char *value) {
    MDB_txn *txn; MDB_val key = text("key"), data;
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn));
    OK(mdb_get(txn, db, &key, &data));
    REQUIRE(data.mv_size == strlen(value) && !memcmp(data.mv_data, value, data.mv_size));
    mdb_txn_abort(txn);
}
int main(int argc, char **argv) {
    REQUIRE(argc == 5); setvbuf(stdout, NULL, _IONBF, 0);
    MDB_env *source = open_env(argv[1], MDB_NOSYNC); MDB_dbi db = seed(source, "old");
    MDB_txn *old; MDB_val key = text("key"), borrowed;
    OK(mdb_txn_begin(source, NULL, MDB_RDONLY, &old));
    OK(mdb_get(old, db, &key, &borrowed));
    REQUIRE(borrowed.mv_size == 3 && !memcmp(borrowed.mv_data, "old", 3));
    put(source, db, "copied");
    puts("WEAK_SOURCE_READY");
    char line[16]; REQUIRE(fgets(line, sizeof line, stdin) && !strcmp(line, "GO\n"));
    MDB_txn *private_write; MDB_val private_data = text("uncommitted");
    OK(mdb_txn_begin(source, NULL, 0, &private_write));
    OK(mdb_put(private_write, db, &key, &private_data, 0));
    REQUIRE(!mkdir(argv[2], 0700)); OK(mdb_env_copy(source, argv[2]));
    OK(mdb_env_copy2(source, argv[3], MDB_CP_COMPACT));
    int fd = open(argv[4], O_WRONLY | O_CREAT | O_EXCL, 0600); REQUIRE(fd >= 0);
    OK(mdb_env_copyfd2(source, fd, MDB_CP_COMPACT)); REQUIRE(fcntl(fd, F_GETFD) >= 0); REQUIRE(!close(fd));
    mdb_txn_abort(private_write); expect(source, db, "copied"); put(source, db, "latest");
    REQUIRE(borrowed.mv_size == 3 && !memcmp(borrowed.mv_data, "old", 3));
    MDB_val after; OK(mdb_get(old, db, &key, &after));
    REQUIRE(after.mv_size == 3 && !memcmp(after.mv_data, "old", 3)); mdb_txn_abort(old);
    MDB_env *copy = open_env(argv[2], MDB_RDONLY); MDB_dbi copydb = open_data(copy);
    expect(copy, copydb, "copied"); mdb_env_close(copy);
    copy = open_env(argv[3], 0); copydb = open_data(copy);
    expect(copy, copydb, "copied"); put(copy, copydb, "independent"); mdb_env_close(copy);
    expect(source, db, "latest");
    copy = open_env(argv[4], MDB_NOSUBDIR | MDB_RDONLY); copydb = open_data(copy);
    expect(copy, copydb, "copied"); mdb_env_close(copy);
    mdb_env_close(source); puts("weak-copy-and-old-borrow=ok"); return 0;
}
