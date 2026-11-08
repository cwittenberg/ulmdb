/* Reader-slot high water is shared table state, not active transaction count. */
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

static void check(int result) {
    if (result) {
        fprintf(stderr, "reader fixture: %s (%d)\n", mdb_strerror(result), result);
        exit(1);
    }
}

static MDB_env *open_environment(const char *path, unsigned flags) {
    MDB_env *env;
    check(mdb_env_create(&env));
    check(mdb_env_set_maxreaders(env, 4));
    check(mdb_env_open(env, path, flags, 0600));
    return env;
}

static void readers(MDB_env *env, const char *phase, unsigned expected) {
    MDB_envinfo info;
    check(mdb_env_info(env, &info));
    printf("%s %u\n", phase, info.me_numreaders);
    if (info.me_numreaders != expected) {
        fprintf(stderr, "reader fixture: %s expected %u, got %u\n",
                phase, expected, info.me_numreaders);
        exit(1);
    }
}

int main(int argc, char **argv) {
    if (argc == 3 && !strcmp(argv[1], "--crash-reader")) {
        MDB_env *child_env = open_environment(argv[2], MDB_NOTLS);
        MDB_txn *child_txn;
        check(mdb_txn_begin(child_env, NULL, MDB_RDONLY, &child_txn));
        _exit(0); /* Leave a dead row; run no library destructors. */
    }
    if (argc != 2 || (mkdir(argv[1], 0700) && errno != EEXIST)) return 2;
    MDB_env *env = open_environment(argv[1], MDB_NOTLS);
    MDB_txn *first, *second;
    readers(env, "initial", 0);
    check(mdb_txn_begin(env, NULL, MDB_RDONLY, &first));
    readers(env, "first", 1);
    mdb_txn_reset(first);
    readers(env, "reset", 1);
    check(mdb_txn_begin(env, NULL, MDB_RDONLY, &second));
    readers(env, "second", 2);
    check(mdb_txn_renew(first));
    readers(env, "renew", 2);
    mdb_txn_abort(first);
    mdb_txn_abort(second);
    readers(env, "abort-all", 2);
    check(mdb_txn_begin(env, NULL, MDB_RDONLY, &first));
    readers(env, "reuse", 2);
    mdb_txn_abort(first);
    int dead;
    check(mdb_reader_check(env, &dead));
    readers(env, "clean", 2);
    mdb_env_close(env);

    env = open_environment(argv[1], 0);
    readers(env, "exclusive-reopen", 0);
    check(mdb_txn_begin(env, NULL, MDB_RDONLY, &first));
    mdb_txn_reset(first);
    readers(env, "tls-reset", 1);
    check(mdb_txn_renew(first));
    mdb_txn_abort(first);
    readers(env, "tls-abort", 1);
    check(mdb_txn_begin(env, NULL, MDB_RDONLY, &first));
    readers(env, "tls-reuse", 1);
    mdb_txn_abort(first);
    mdb_env_close(env);

    env = open_environment(argv[1], MDB_NOTLS);
    fflush(stdout);
    pid_t child = fork();
    if (child < 0) return 2;
    if (child == 0) {
        /* Open independently after exec; never use an inherited live handle. */
        execl(argv[0], argv[0], "--crash-reader", argv[1], (char *)NULL);
        _exit(2);
    }
    int status;
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status) || WEXITSTATUS(status)) return 2;
    readers(env, "crashed-reader", 1);
    check(mdb_reader_check(env, &dead));
    if (dead != 1) return 1;
    readers(env, "dead-clean", 1);
    mdb_env_close(env);
    env = open_environment(argv[1], MDB_NOTLS);
    readers(env, "after-crash-reopen", 0);
    mdb_env_close(env);

    env = open_environment(argv[1], MDB_NOLOCK | MDB_NOTLS);
    check(mdb_txn_begin(env, NULL, MDB_RDONLY, &first));
    readers(env, "nolock", 0);
    mdb_txn_abort(first);
    mdb_env_close(env);

    env = open_environment(argv[1], MDB_NOTLS);
    MDB_txn *slots[4], *excess = NULL;
    for (unsigned i = 0; i < 4; ++i)
        check(mdb_txn_begin(env, NULL, MDB_RDONLY, slots + i));
    int full = mdb_txn_begin(env, NULL, MDB_RDONLY, &excess);
    printf("full-table %d\n", full);
    if (full != MDB_READERS_FULL || excess) return 1;
    readers(env, "full-table-high-water", 4);
    for (unsigned i = 0; i < 4; ++i) mdb_txn_abort(slots[i]);
    readers(env, "full-table-released", 4);
    mdb_env_close(env);
    env = open_environment(argv[1], MDB_NOTLS);
    readers(env, "full-table-reopen", 0);
    mdb_env_close(env);
    return 0;
}
