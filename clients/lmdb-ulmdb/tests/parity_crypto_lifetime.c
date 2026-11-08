/* Linux loader lifetime witness. Each dlopen(RTLD_NOLOAD) probe is balanced.
 * The first lease closes before any environment uses the second lease's hooks;
 * every environment/transaction closes before the final module lease closes. */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <lmdb.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "module-lifetime: %s\n", #x); exit(1); } } while (0)
#define OK(x) do { int rc = (x); if (rc) { fprintf(stderr, "%s: %d\n", #x, rc); exit(1); } } while (0)
static int loaded(const char *path) {
    void *probe = dlopen(path, RTLD_NOW | RTLD_NOLOAD);
    if (!probe) return 0;
    CHECK(dlclose(probe) == 0);
    return 1;
}
static unsigned mappings(const struct stat *identity) {
    FILE *maps = fopen("/proc/self/maps", "r");
    CHECK(maps);
    unsigned count = 0;
    char line[8192];
    while (fgets(line, sizeof line, maps)) {
        unsigned long begin, end, offset;
        unsigned device_major, device_minor;
        unsigned long long inode;
        char permissions[5];
        if (sscanf(line, "%lx-%lx %4s %lx %x:%x %llu", &begin, &end, permissions,
                   &offset, &device_major, &device_minor, &inode) == 7 &&
            device_major == major(identity->st_dev) && device_minor == minor(identity->st_dev) &&
            inode == (unsigned long long)identity->st_ino) ++count;
    }
    CHECK(!ferror(maps));
    CHECK(fclose(maps) == 0);
    return count;
}
static MDB_env *open_with_hooks(const char *path, MDB_crypto_funcs *hooks, unsigned flags) {
    MDB_env *env;
    OK(mdb_env_create(&env));
    OK(mdb_env_set_mapsize(env, 64u << 20));
    OK(mdb_env_set_pagesize(env, 8192));
    mdb_modsetup(env, hooks, "passphrase");
    OK(mdb_env_open(env, path, MDB_NOTLS | flags, 0600));
    return env;
}
static void read_value(MDB_env *env, const unsigned char *expected, size_t length) {
    MDB_txn *txn;
    MDB_dbi db;
    MDB_val key = {3, "key"}, value;
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn));
    OK(mdb_dbi_open(txn, NULL, 0, &db));
    OK(mdb_get(txn, db, &key, &value));
    CHECK(value.mv_size == length && !memcmp(value.mv_data, expected, length));
    mdb_txn_abort(txn);
}
int main(int argc, char **argv) {
    CHECK(argc == 2);
    char plugin[4096], source[4096];
    int n = snprintf(plugin, sizeof plugin, "%s/crypto-module.so", argv[1]);
    CHECK(n > 0 && (size_t)n < sizeof plugin);
    n = snprintf(source, sizeof source, "%s/source", argv[1]);
    CHECK(n > 0 && (size_t)n < sizeof source);
    struct stat identity;
    CHECK(!stat(plugin, &identity) && S_ISREG(identity.st_mode));
    CHECK(!loaded(plugin) && !mappings(&identity));
    MDB_crypto_funcs *first_hooks = NULL, *second_hooks = NULL;
    char *error = NULL;
    void *first = mdb_modload(plugin, NULL, &first_hooks, &error);
    CHECK(first && first_hooks && first_hooks->mcf_encfunc && first_hooks->mcf_sumfunc);
    void *second = mdb_modload(plugin, NULL, &second_hooks, &error);
    CHECK(second && second_hooks && second_hooks->mcf_str2key);
    CHECK(loaded(plugin) && mappings(&identity));
    mdb_modunload(first);
    CHECK(loaded(plugin) && mappings(&identity));
    CHECK(!mkdir(source, 0700));
    unsigned char payload[20000];
    for (size_t i = 0; i < sizeof payload; ++i) payload[i] = (unsigned char)(i * 31);
    MDB_env *env = open_with_hooks(source, second_hooks, 0);
    MDB_txn *txn;
    MDB_dbi db;
    MDB_val key = {3, "key"}, value = {sizeof payload, payload};
    OK(mdb_txn_begin(env, NULL, 0, &txn));
    OK(mdb_dbi_open(txn, NULL, 0, &db));
    OK(mdb_put(txn, db, &key, &value, 0));
    OK(mdb_txn_commit(txn));
    read_value(env, payload, sizeof payload);
    mdb_env_close(env);
    env = open_with_hooks(source, second_hooks, MDB_RDONLY);
    read_value(env, payload, sizeof payload);
    mdb_env_close(env);
    puts("module-callbacks-after-first-unload=ok");
    mdb_modunload(second);
    CHECK(!loaded(plugin) && !mappings(&identity));
    puts("module-lifetime-ok: real hooks, two leases, cold data, final unload");
    return 0;
}
