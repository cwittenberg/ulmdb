/* Copies own a readonly slot: high water, capacity, TLS and error cleanup. */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <lmdb.h>
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#define OK(x) do { int probe_code = (x); if (probe_code) { \
    fprintf(stderr, "line=%d rc=%d %s\n", __LINE__, probe_code, mdb_strerror(probe_code)); \
    return 3; } } while (0)
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "copy-readers line=%d: %s\n", __LINE__, #x); return 3; } } while (0)

/* Same public calls as the focused probe; only case/root selection is factored. */
static int copy_case(const char *root, unsigned index, unsigned flags,
                     unsigned compact, int fdmode, int hold, int expected_copy) {
    MDB_env *env;
    MDB_txn *writer, *held = NULL, *reader;
    MDB_dbi database;
    MDB_envinfo before, after;
    char directory[4096], source[4096], target[4096];
    int n = snprintf(directory, sizeof directory, "%s/case-%u", root, index);
    CHECK(n > 0 && (size_t)n < sizeof directory);
    n = snprintf(source, sizeof source, "%s/source", directory);
    CHECK(n > 0 && (size_t)n < sizeof source);
    n = snprintf(target, sizeof target, "%s/target", directory);
    CHECK(n > 0 && (size_t)n < sizeof target);
    CHECK(!mkdir(directory, 0700) && !mkdir(source, 0700));
    OK(mdb_env_create(&env));
    OK(mdb_env_set_maxreaders(env, 1));
    OK(mdb_env_set_mapsize(env, 16u * 1024u * 1024u));
    OK(mdb_env_open(env, source, flags, 0600));
    OK(mdb_txn_begin(env, NULL, 0, &writer));
    OK(mdb_dbi_open(writer, NULL, 0, &database));
    MDB_val key = {1, "k"}, value = {1, "A"};
    OK(mdb_put(writer, database, &key, &value, 0));
    OK(mdb_txn_commit(writer));
    if (hold) OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &held));
    OK(mdb_env_info(env, &before));
    int result;
    if (fdmode) {
        int fd = fdmode == 2 ? open("/dev/full", O_WRONLY)
                            : open(target, O_CREAT | O_EXCL | O_WRONLY, 0600);
        CHECK(fd >= 0);
        result = mdb_env_copyfd2(env, fd, compact);
        CHECK(!close(fd)); /* Copy must leave the caller's FD owned and open. */
    } else {
        CHECK(!mkdir(target, 0700));
        result = mdb_env_copy2(env, target, compact);
    }
    OK(mdb_env_info(env, &after));
    CHECK(result == expected_copy);
    CHECK(before.me_numreaders == (unsigned)(hold != 0));
    CHECK(after.me_numreaders == (flags & MDB_NOLOCK ? 0u : 1u));
    if (held) mdb_txn_abort(held);
    /* A leaked active slot makes this fail for either TLS or maxreaders=1. */
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &reader));
    MDB_val got;
    OK(mdb_get(reader, database, &key, &got));
    CHECK(got.mv_size == 1 && !memcmp(got.mv_data, "A", 1));
    mdb_txn_abort(reader);
    Dl_info provider;
    CHECK(dladdr((void *)mdb_env_create, &provider));
    CHECK(provider.dli_fname && provider.dli_fname[0]);
    printf("flags=%u compact=%u fd=%d held=%d before=%u after=%u copy=%d progress=A\n",
           flags, compact, fdmode, hold, before.me_numreaders, after.me_numreaders, result);
    mdb_env_close(env);
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    unsigned index = 0;
    const unsigned modes[] = {MDB_NOTLS, 0};
    const unsigned copies[] = {0, MDB_CP_COMPACT};
    for (size_t mode = 0; mode < sizeof modes / sizeof *modes; ++mode)
        for (size_t copy = 0; copy < sizeof copies / sizeof *copies; ++copy)
            for (int fdmode = 0; fdmode < 2; ++fdmode)
                OK(copy_case(argv[1], index++, modes[mode], copies[copy], fdmode, 0, 0));
    for (size_t copy = 0; copy < sizeof copies / sizeof *copies; ++copy)
        OK(copy_case(argv[1], index++, MDB_NOTLS, copies[copy], 0, 1, MDB_READERS_FULL));
    OK(copy_case(argv[1], index++, 0, 0, 0, 1, MDB_BAD_RSLOT));
    for (size_t copy = 0; copy < sizeof copies / sizeof *copies; ++copy)
        OK(copy_case(argv[1], index++, MDB_NOTLS | MDB_NOLOCK, copies[copy], 0, 0, 0));
    for (size_t mode = 0; mode < sizeof modes / sizeof *modes; ++mode)
        for (size_t copy = 0; copy < sizeof copies / sizeof *copies; ++copy)
            OK(copy_case(argv[1], index++, modes[mode], copies[copy], 2, 0, ENOSPC));
    CHECK(index == 17);
    return 0;
}
