/* Public control and transaction contracts, unchanged across both engines. */
#include <lmdb.h>
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

static int traced(const char *label, int code) {
    printf("%s=%d\n", label, code);
    return code;
}
static void ok(int code, const char *operation) {
    if (code) {
        fprintf(stderr, "%s: %s (%d)\n", operation, mdb_strerror(code), code);
        exit(1);
    }
}
static void directory(const char *parent, const char *name, char *output, size_t size) {
    int count = snprintf(output, size, "%s/%s", parent, name);
    if (count < 0 || (size_t)count >= size || (mkdir(output, 0700) && errno != EEXIST)) {
        perror("contract directory"); exit(1);
    }
}
static MDB_env *environment(const char *path, unsigned flags, unsigned readers) {
    MDB_env *env;
    ok(mdb_env_create(&env), "create");
    ok(mdb_env_set_maxdbs(env, 8), "maxdbs");
    ok(mdb_env_set_maxreaders(env, readers), "maxreaders");
    ok(mdb_env_set_mapsize(env, 8u * 1024u * 1024u), "mapsize");
    ok(mdb_env_open(env, path, flags, 0600), "open");
    return env;
}
static void controls(const char *path) {
    MDB_env *env;
    ok(mdb_env_create(&env), "create");
    unsigned flags;
    const char *reported = (const char *)1;
    mdb_filehandle_t fd;
    traced("unopened.path", mdb_env_get_path(env, &reported));
    printf("unopened.path-null=%d\n", reported == NULL);
    traced("unopened.fd", mdb_env_get_fd(env, &fd));
    printf("unopened.fd-invalid=%d\n", fd == (mdb_filehandle_t)-1);
    int context;
    traced("context.set", mdb_env_set_userctx(env, &context));
    printf("context.same=%d\n", mdb_env_get_userctx(env) == &context);
    ok(mdb_env_set_maxdbs(env, 8), "maxdbs");
    ok(mdb_env_set_mapsize(env, 8u * 1024u * 1024u), "mapsize");
    traced("flags.preset", mdb_env_set_flags(env, MDB_NOSYNC | MDB_NOMETASYNC, 1));
    ok(mdb_env_open(env, path, MDB_NOTLS, 0600), "open");
    ok(mdb_env_get_flags(env, &flags), "get flags");
    printf("flags.open=%u\n", flags);
    traced("flags.clear", mdb_env_set_flags(env, MDB_NOSYNC | MDB_NOMETASYNC, 0));
    traced("flags.invalid", mdb_env_set_flags(env, MDB_RDONLY, 1));
    traced("open.again", mdb_env_open(env, path, 0, 0600));
    traced("maxdbs.after-open", mdb_env_set_maxdbs(env, 4));
    traced("maxreaders.after-open", mdb_env_set_maxreaders(env, 4));
    MDB_txn *txn = NULL;
    traced("txn.ignored-flags", mdb_txn_begin(env, NULL, 0x08000000u, &txn));
    if (txn) mdb_txn_abort(txn);
    ok(mdb_txn_begin(env, NULL, 0, &txn), "writer");
    traced("mapsize.active-writer", mdb_env_set_mapsize(env, 16u * 1024u * 1024u));
    traced("renew.writer", mdb_txn_renew(txn));
    MDB_txn *child = NULL;
#if MDB_VERSION_MAJOR < 1
    /* Read-only children were added in 1.0, outside the 0.9 API profile. */
    traced("nested.read", mdb_txn_begin(env, txn, MDB_RDONLY, &child));
    if (child) mdb_txn_abort(child);
#endif
    MDB_dbi db;
    ok(mdb_dbi_open(txn, "committed", MDB_CREATE, &db), "named db");
    ok(mdb_txn_begin(env, txn, 0, &child), "child");
    MDB_txn *sibling = NULL;
    traced("nested.sibling", mdb_txn_begin(env, txn, 0, &sibling));
    if (sibling) mdb_txn_abort(sibling);
    MDB_dbi ephemeral;
    ok(mdb_dbi_open(child, "ephemeral", MDB_CREATE, &ephemeral), "child db");
    mdb_txn_abort(child);
    traced("nested.aborted-name", mdb_dbi_open(txn, "ephemeral", 0, &ephemeral));
    ok(mdb_txn_commit(txn), "commit");
    traced("sync.write-env", mdb_env_sync(env, 1));
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn), "reader");
    traced("renew.active-reader", mdb_txn_renew(txn));
    mdb_txn_reset(txn);
    traced("renew.reset-reader", mdb_txn_renew(txn));
    traced("commit.reader", mdb_txn_commit(txn));
    mdb_env_close(env);

    env = environment(path, MDB_RDONLY | MDB_WRITEMAP | MDB_NOTLS, 126);
    ok(mdb_env_get_flags(env, &flags), "readonly flags");
    printf("flags.readonly=%u\n", flags);
    traced("sync.readonly", mdb_env_sync(env, 0));
    traced("sync.readonly-force", mdb_env_sync(env, 1));
    txn = NULL;
    traced("write.readonly", mdb_txn_begin(env, NULL, 0, &txn));
    if (txn) mdb_txn_abort(txn);
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn), "readonly reader");
    child = NULL;
    traced("nested.read-parent", mdb_txn_begin(env, txn, 0, &child));
    if (child) mdb_txn_abort(child);
    mdb_txn_abort(txn);
    mdb_env_close(env);
}
static void tls_slots(const char *path) {
    MDB_env *env = environment(path, 0, 1);
    MDB_txn *first, *second = NULL;
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &first), "TLS first");
    traced("tls.second-active", mdb_txn_begin(env, NULL, MDB_RDONLY, &second));
    if (second) mdb_txn_abort(second);
    mdb_txn_reset(first);
    second = NULL;
    traced("tls.begin-after-reset", mdb_txn_begin(env, NULL, MDB_RDONLY, &second));
    traced("tls.renew-slot-in-use", mdb_txn_renew(first));
    mdb_txn_abort(first);
    MDB_txn *third = NULL;
    traced("tls.reset-handle-abort", mdb_txn_begin(env, NULL, MDB_RDONLY, &third));
    if (third) mdb_txn_abort(third);
    mdb_txn_abort(second);
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &first), "TLS reuse");
    traced("tls.renew-active", mdb_txn_renew(first));
    mdb_txn_abort(first);
    mdb_env_close(env);
}
static void notls_slots(const char *path) {
    MDB_env *env = environment(path, MDB_NOTLS, 1);
    MDB_txn *first, *second = NULL;
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &first), "NOTLS first");
    mdb_txn_reset(first);
    traced("notls.reset-reserves-slot", mdb_txn_begin(env, NULL, MDB_RDONLY, &second));
    if (second) mdb_txn_abort(second);
    traced("notls.renew-reserved", mdb_txn_renew(first));
    mdb_txn_abort(first);
    traced("notls.reuse-after-abort", mdb_txn_begin(env, NULL, MDB_RDONLY, &second));
    if (second) mdb_txn_abort(second);
    mdb_env_close(env);
}
static void nolock_slots(const char *path) {
    MDB_env *env = environment(path, MDB_NOLOCK, 1);
    MDB_txn *first, *second = NULL;
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &first), "NOLOCK first");
    traced("nolock.independent-readers", mdb_txn_begin(env, NULL, MDB_RDONLY, &second));
    if (second) mdb_txn_abort(second);
    mdb_txn_abort(first);
    mdb_env_close(env);
}
struct thread_read { MDB_env *env; int result; };
static void *read_thread(void *opaque) {
    struct thread_read *data = opaque;
    MDB_txn *txn = NULL;
    data->result = mdb_txn_begin(data->env, NULL, MDB_RDONLY, &txn);
    if (txn) mdb_txn_abort(txn);
    return NULL;
}
static void thread_exit(const char *path) {
    MDB_env *env = environment(path, 0, 1);
    struct thread_read data = {env, -1};
    pthread_t thread;
    if (pthread_create(&thread, NULL, read_thread, &data) || pthread_join(thread, NULL)) {
        fputs("reader thread failure\n", stderr); exit(1);
    }
    traced("tls.thread-read", data.result);
    MDB_txn *txn = NULL;
    traced("tls.thread-exit-releases-slot", mdb_txn_begin(env, NULL, MDB_RDONLY, &txn));
    if (txn) mdb_txn_abort(txn);
    mdb_env_close(env);
}
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    if (mkdir(argv[1], 0700) && errno != EEXIST) return 1;
    char path[4096];
    puts("contract-version=1");
    directory(argv[1], "controls", path, sizeof path); controls(path);
    directory(argv[1], "tls", path, sizeof path); tls_slots(path);
    directory(argv[1], "notls", path, sizeof path); notls_slots(path);
    directory(argv[1], "nolock", path, sizeof path); nolock_slots(path);
    directory(argv[1], "thread-exit", path, sizeof path); thread_exit(path);
    directory(argv[1], "invalid", path, sizeof path);
    MDB_env *env;
    ok(mdb_env_create(&env), "invalid create");
    traced("open.invalid-flags", mdb_env_open(env, path, 0x08000000u, 0600));
    mdb_env_close(env);
    return 0;
}
