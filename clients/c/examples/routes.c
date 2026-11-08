/* Atomic IPv4 and IPv6 prefix updates through ULMDB's native C extensions. */
#include <ulmdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(int result) {
    if (result != MDB_SUCCESS) {
        fprintf(stderr, "database error %d: %s\n", result, mdb_strerror(result));
        exit(1);
    }
}

static MDB_val text(const char *data) {
    MDB_val value = {strlen(data), (void *)data};
    return value;
}

static void lookup(MDB_txn *txn, MDB_dbi db, const char *ip) {
    MDB_val matched, data;
    int result = ulmdb_longest_match(txn, db, ip, &matched, &data);
    if (result == MDB_NOTFOUND) {
        printf("%s: no route\n", ip);
        return;
    }
    check(result);
    /* matched.mv_data has no NUL terminator; always use matched.mv_size. */
    printf("%s -> %.*s: %.*s\n", ip,
           (int)matched.mv_size, (const char *)matched.mv_data,
           (int)data.mv_size, (const char *)data.mv_data);
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s DATABASE_DIRECTORY\n", argv[0]);
        return 2;
    }
    MDB_env *env;
    MDB_txn *write, *read;
    MDB_dbi routes4, routes6;
    check(mdb_env_create(&env));
    check(mdb_env_set_maxdbs(env, 4));
    check(mdb_env_open(env, argv[1], MDB_NOTLS, 0600));
    check(mdb_txn_begin(env, NULL, 0, &write));
    check(ulmdb_dbi_open(write, "routes4", ULMDB_IPV4, MDB_CREATE, &routes4));
    check(ulmdb_dbi_open(write, "routes6", ULMDB_IPV6, MDB_CREATE, &routes6));
    MDB_val data = text("example-v4");
    check(ulmdb_prefix_put(write, routes4, "192.0.2.0/24", &data));
    data = text("specific-v4");
    check(ulmdb_prefix_put(write, routes4, "192.0.2.128/25", &data));
    data = text("example-v6");
    check(ulmdb_prefix_put(write, routes6, "2001:db8::/32", &data));
    check(mdb_txn_commit(write));

    check(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    lookup(read, routes4, "192.0.2.200");
    lookup(read, routes4, "198.51.100.1");
    lookup(read, routes6, "2001:db8::1");
    mdb_txn_abort(read);
    mdb_dbi_close(env, routes4);
    mdb_dbi_close(env, routes6);
    mdb_env_close(env);
    return 0;
}
