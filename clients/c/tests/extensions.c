#include <ulmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define REQUIRE(condition) do { if (!(condition)) { \
    fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #condition); exit(1); \
} } while (0)
#define OK(expression) do { int result_ = (expression); if (result_ != 0) { \
    fprintf(stderr, "%s:%d: %s: %d (%s)\n", __FILE__, __LINE__, #expression, \
            result_, mdb_strerror(result_)); exit(1); \
} } while (0)

static MDB_val text(const char *bytes) {
    MDB_val result = {strlen(bytes), (void *)bytes};
    return result;
}

static void equals(MDB_val value, const char *expected) {
    REQUIRE(value.mv_size == strlen(expected));
    REQUIRE(value.mv_size == 0 || memcmp(value.mv_data, expected, value.mv_size) == 0);
}

int main(int argc, char **argv) {
    REQUIRE(argc == 2);
    MDB_env *env;
    MDB_txn *write, *old, *read;
    MDB_dbi v4, v6, kv;
    MDB_val data, matched;
    REQUIRE(mdb_env_create(NULL) == EINVAL);
    OK(mdb_env_create(&env));
    OK(mdb_env_set_maxdbs(env, 8));
    OK(mdb_env_open(env, argv[1], MDB_NOTLS, 0600));
    OK(mdb_txn_begin(env, NULL, 0, &write));
    OK(ulmdb_dbi_open(write, "routes4", ULMDB_IPV4, MDB_CREATE, &v4));
    OK(ulmdb_dbi_open(write, "routes6", ULMDB_IPV6, MDB_CREATE, &v6));
    OK(ulmdb_dbi_open(write, "kv", ULMDB_KV, MDB_CREATE, &kv));
    data = text("default");
    OK(ulmdb_prefix_put(write, v4, "0.0.0.0/0", &data));
    data = text("v4");
    OK(ulmdb_prefix_put(write, v4, "192.0.2.0/24", &data));
    data = text("specific");
    OK(ulmdb_prefix_put(write, v4, "192.0.2.128/25", &data));
    data = text("v6");
    OK(ulmdb_prefix_put(write, v6, "2001:db8::/32", &data));
    data = text("");
    OK(ulmdb_prefix_put(write, v6, "2001:db8:1::/48", &data));
    REQUIRE(ulmdb_prefix_put(write, v4, "192.0.2.1/24", &data) != 0);
    REQUIRE(ulmdb_prefix_put(write, v4, "192.0.2.0/33", &data) != 0);
    REQUIRE(ulmdb_prefix_put(write, v6, "::/129", &data) != 0);
    REQUIRE(ulmdb_prefix_put(write, v6, "malformed", &data) != 0);
    REQUIRE(ulmdb_prefix_put(write, v4, "::/0", &data) == MDB_INCOMPATIBLE);
    REQUIRE(ulmdb_prefix_put(write, kv, "0.0.0.0/0", &data) == MDB_INCOMPATIBLE);
    OK(mdb_txn_commit(write));

    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &old));
    OK(ulmdb_longest_match(old, v4, "192.0.2.200", &matched, &data));
    equals(matched, "192.0.2.128/25");
    equals(data, "specific");
    MDB_val borrowed = data;
    OK(ulmdb_longest_match(old, v4, "198.51.100.1", &matched, &data));
    equals(matched, "0.0.0.0/0");
    equals(data, "default");
    OK(ulmdb_longest_match(old, v6, "2001:db8::1", &matched, &data));
    equals(matched, "2001:db8::/32");
    equals(data, "v6");
    OK(ulmdb_prefix_get(old, v6, "2001:db8:1::/48", &data));
    REQUIRE(data.mv_size == 0);
    REQUIRE(ulmdb_longest_match(old, v6, "2001:db9::1", &matched, &data) == MDB_NOTFOUND);
    REQUIRE(ulmdb_longest_match(old, v4, "::1", &matched, &data) == MDB_INCOMPATIBLE);
    REQUIRE(ulmdb_longest_match(old, v4, "invalid", &matched, &data) != 0);
    data = text("forbidden");
    REQUIRE(ulmdb_prefix_put(old, v4, "192.0.2.0/24", &data) == EACCES);

    OK(mdb_txn_begin(env, NULL, 0, &write));
    OK(ulmdb_prefix_delete(write, v4, "192.0.2.128/25"));
    OK(ulmdb_prefix_delete(write, v6, "2001:db8::/32"));
    REQUIRE(ulmdb_prefix_delete(write, v6, "2001:db8::/32") == MDB_NOTFOUND);
    OK(mdb_txn_commit(write));
    equals(borrowed, "specific");
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    OK(ulmdb_longest_match(read, v4, "192.0.2.200", &matched, &data));
    equals(matched, "192.0.2.0/24");
    equals(data, "v4");
    REQUIRE(ulmdb_prefix_get(read, v6, "2001:db8::/32", &data) == MDB_NOTFOUND);
    OK(ulmdb_prefix_get(old, v6, "2001:db8::/32", &data));
    equals(data, "v6");
    mdb_txn_abort(read);
    mdb_txn_abort(old);
    mdb_env_close(env);

    OK(mdb_env_create(&env));
    OK(mdb_env_set_maxdbs(env, 8));
    OK(mdb_env_open(env, argv[1], MDB_NOTLS, 0600));
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    OK(ulmdb_dbi_open(read, "routes4", ULMDB_IPV4, 0, &v4));
    OK(ulmdb_dbi_open(read, "routes6", ULMDB_IPV6, 0, &v6));
    OK(ulmdb_longest_match(read, v4, "192.0.2.200", &matched, &data));
    equals(data, "v4");
    REQUIRE(ulmdb_prefix_get(read, v6, "2001:db8::/32", &data) == MDB_NOTFOUND);
    mdb_txn_abort(read);
    mdb_env_close(env);
    puts("extensions-ok");
    return 0;
}
