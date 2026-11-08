/* One deterministic executable is run unchanged against each implementation. */
#include "parity_workload_data.h"
#include <errno.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>

static uint64_t number(const char *text) {
    char *end;
    errno = 0;
    unsigned long long result = strtoull(text, &end, 10);
    parity_require(*text && !*end && !errno && text[0] != '-', "unsigned decimal argument");
    return (uint64_t)result;
}
static MDB_env *open_environment(const char *path, MDB_dbi *kv, MDB_dbi *duplicates) {
    MDB_env *env;
    MDB_txn *txn;
    parity_check(mdb_env_create(&env), "env_create");
    parity_check(mdb_env_set_mapsize(env, 128 * 1024 * 1024), "env_set_mapsize");
    parity_check(mdb_env_set_maxdbs(env, 16), "env_set_maxdbs");
    parity_check(mdb_env_set_maxreaders(env, 64), "env_set_maxreaders");
    parity_check(mdb_env_open(env, path, MDB_NOTLS, 0600), "env_open");
    parity_check(mdb_txn_begin(env, NULL, 0, &txn), "initial txn_begin");
    parity_check(mdb_dbi_open(txn, "parity-kv", MDB_CREATE, kv), "kv open");
    parity_check(mdb_dbi_open(txn, "parity-dup", MDB_CREATE | MDB_DUPSORT, duplicates), "dup open");
    parity_check(mdb_txn_commit(txn), "initial commit");
    return env;
}
static int same_image(struct parity_image left, struct parity_image right) {
    return left.forward == right.forward && left.reverse == right.reverse && left.entries == right.entries;
}
int main(int argc, char **argv) {
    if (argc != 4) {
        fputs("usage: parity_workload DATABASE_DIRECTORY SEED OPERATIONS\n", stderr);
        return 2;
    }
    uint64_t seed = number(argv[2]), operations = number(argv[3]);
    parity_require(operations > 0 && operations <= 1000000, "operation count between 1 and 1000000");
    struct parity_random random = {seed ? seed : UINT64_C(0x9e3779b97f4a7c15), 0};
    MDB_dbi kv, duplicates;
    MDB_env *env = open_environment(argv[1], &kv, &duplicates);
    printf("seed=%" PRIu64 " operations=%" PRIu64 "\n", seed, operations);
    unsigned batch = 0;
    while (random.step < operations) {
        MDB_txn *snapshot, *writer;
        parity_check(mdb_txn_begin(env, NULL, MDB_RDONLY, &snapshot), "snapshot begin");
        struct parity_image before[2] = {parity_image(snapshot, kv), parity_image(snapshot, duplicates)};
        parity_check(mdb_txn_begin(env, NULL, 0, &writer), "writer begin");
        unsigned count = 16 + parity_random_next(&random) % 33;
        for (unsigned i = 0; i < count && random.step < operations; ++i)
            parity_operation(writer, kv, duplicates, &random);
        if (parity_random_next(&random) % 3 == 0 && random.step < operations) {
            MDB_txn *child;
            parity_check(mdb_txn_begin(env, writer, 0, &child), "nested begin");
            for (unsigned i = 0; i < 4 && random.step < operations; ++i)
                parity_operation(child, kv, duplicates, &random);
            int commit = (int)(parity_random_next(&random) & 1);
            printf("nested batch=%u commit=%d\n", batch, commit);
            if (commit) parity_check(mdb_txn_commit(child), "nested commit");
            else mdb_txn_abort(child);
        }
        int commit = parity_random_next(&random) % 4 != 0;
        printf("writer batch=%u commit=%d\n", batch, commit);
        if (commit) parity_check(mdb_txn_commit(writer), "writer commit");
        else mdb_txn_abort(writer);
        struct parity_image after[2] = {parity_image(snapshot, kv), parity_image(snapshot, duplicates)};
        parity_require(same_image(before[0], after[0]) && same_image(before[1], after[1]),
                       "reader snapshot stable across writer publication");
        mdb_txn_abort(snapshot);
        MDB_txn *fresh;
        parity_check(mdb_txn_begin(env, NULL, MDB_RDONLY, &fresh), "published read");
        parity_print_image("published", 0, parity_image(fresh, kv));
        parity_print_image("published", 1, parity_image(fresh, duplicates));
        mdb_txn_abort(fresh);
        ++batch;
    }
    parity_check(mdb_env_sync(env, 1), "sync");
    MDB_txn *reader;
    parity_check(mdb_txn_begin(env, NULL, MDB_RDONLY, &reader), "final read");
    struct parity_image final[2] = {parity_image(reader, kv), parity_image(reader, duplicates)};
    parity_dump(reader, kv, 0); parity_dump(reader, duplicates, 1);
    mdb_txn_abort(reader);
    mdb_env_close(env);
    env = open_environment(argv[1], &kv, &duplicates);
    parity_check(mdb_txn_begin(env, NULL, MDB_RDONLY, &reader), "reopened read");
    struct parity_image reopened[2] = {parity_image(reader, kv), parity_image(reader, duplicates)};
    parity_require(same_image(final[0], reopened[0]) && same_image(final[1], reopened[1]), "reopen preserves state");
    parity_print_image("reopened", 0, reopened[0]); parity_print_image("reopened", 1, reopened[1]);
    mdb_txn_abort(reader);
    mdb_env_close(env);
    printf("workload-complete operations=%" PRIu64 " batches=%u\n", random.step, batch);
    return 0;
}
