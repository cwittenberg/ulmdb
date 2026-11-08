/* Identical source and executable exercise the public LMDB cursor contract. */
#include <lmdb.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define OK(call) do { int rc_ = (call); if (rc_) { \
    fprintf(stderr, "%s:%d: %s returned %d\n", __FILE__, __LINE__, #call, rc_); \
    exit(1); } } while (0)

static MDB_val text(const char *s) { MDB_val v = {strlen(s), (void *)s}; return v; }
static void show(MDB_cursor *cursor, const char *label) {
    MDB_val key = {0, NULL}, value = {0, NULL};
    int rc = mdb_cursor_get(cursor, &key, &value, MDB_GET_CURRENT);
    printf("%s rc=%d", label, rc);
    if (!rc) {
        const unsigned char *bytes = value.mv_data;
        printf(" key=%.*s value=", (int)key.mv_size, (char *)key.mv_data);
        for (size_t i = 0; i < value.mv_size; ++i) printf("%02x", bytes[i]);
    }
    putchar('\n');
}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    MDB_env *env;
    MDB_txn *txn;
    MDB_dbi kv, fixed, duplicates;
    MDB_cursor *cursor;
    MDB_val key, value;
    OK(mdb_env_create(&env));
    OK(mdb_env_set_mapsize(env, 64u * 1024u * 1024u));
    OK(mdb_env_set_maxdbs(env, 8));
    OK(mdb_env_open(env, argv[1], MDB_NOTLS, 0600));
    OK(mdb_txn_begin(env, NULL, 0, &txn));
    OK(mdb_dbi_open(txn, "kv", MDB_CREATE, &kv));
    OK(mdb_dbi_open(txn, "fixed", MDB_CREATE | MDB_DUPSORT | MDB_DUPFIXED | MDB_INTEGERDUP, &fixed));
    OK(mdb_dbi_open(txn, "duplicates", MDB_CREATE | MDB_DUPSORT, &duplicates));
    key = text("key"); value = text("original");
    OK(mdb_put(txn, kv, &key, &value, 0));
    OK(mdb_cursor_open(txn, kv, &cursor));
    char input[] = "key";
    key = text(input);
    OK(mdb_cursor_get(cursor, &key, &value, MDB_SET));
    printf("set-preserves-input=%d\n", key.mv_data == input && key.mv_size == 3);
    key = text(input);
    OK(mdb_cursor_get(cursor, &key, &value, MDB_SET_KEY));
    printf("set-key-returns-stored=%d\n", key.mv_data != input && key.mv_size == 3);
    key = text("key"); value.mv_size = 5; value.mv_data = NULL;
    int rc = mdb_cursor_put(cursor, &key, &value, MDB_CURRENT | MDB_RESERVE);
    printf("current-reserve=%d\n", rc);
    if (!rc) memcpy(value.mv_data, "alpha", 5);
    show(cursor, "after-current-reserve");
    mdb_cursor_close(cursor);

    OK(mdb_cursor_open(txn, kv, &cursor));
    key = text("key"); value = text("replacement");
    rc = mdb_cursor_put(cursor, &key, &value, MDB_NOOVERWRITE);
    printf("nooverwrite=%d returned=%.*s\n", rc, (int)value.mv_size, (char *)value.mv_data);
    show(cursor, "after-nooverwrite");
    mdb_cursor_close(cursor);
    key = text("ignored"); value = text("kept");
    OK(mdb_put(txn, kv, &key, &value, 0));
    value.mv_size = 1; value.mv_data = NULL;
    printf("ordinary-delete-ignores-data=%d\n", mdb_del(txn, kv, &key, &value));

    OK(mdb_cursor_open(txn, fixed, &cursor));
    uint32_t initial[] = {9, 4, 7};
    MDB_val batch[2] = {{sizeof(uint32_t), initial}, {3, NULL}};
    key = text("group");
    rc = mdb_cursor_put(cursor, &key, batch, MDB_MULTIPLE);
    printf("multiple=%d count=%zu pointer=%td\n", rc, batch[1].mv_size,
           (unsigned char *)batch[0].mv_data - (unsigned char *)initial);
    show(cursor, "after-multiple");
    batch[0].mv_data = initial; batch[1].mv_size = 3;
    rc = mdb_put(txn, fixed, &key, batch, MDB_MULTIPLE);
    printf("plain-put-multiple=%d count=%zu\n", rc, batch[1].mv_size);

    key = text("group");
    OK(mdb_cursor_get(cursor, &key, &value, MDB_SET));
    uint32_t partial[] = {11, 7, 13};
    batch[0].mv_data = partial; batch[1].mv_size = 3;
    rc = mdb_cursor_put(cursor, &key, batch, MDB_MULTIPLE | MDB_NODUPDATA);
    printf("multiple-partial=%d count=%zu pointer=%td\n", rc, batch[1].mv_size,
           (unsigned char *)batch[0].mv_data - (unsigned char *)partial);
    show(cursor, "after-multiple-partial");
    mdb_cursor_close(cursor);

    key = text("key"); value = text("a"); OK(mdb_put(txn, duplicates, &key, &value, 0));
    value = text("m"); OK(mdb_put(txn, duplicates, &key, &value, 0));
    value = text("z"); OK(mdb_put(txn, duplicates, &key, &value, 0));
    MDB_cursor *observer;
    OK(mdb_cursor_open(txn, duplicates, &cursor));
    OK(mdb_cursor_open(txn, duplicates, &observer));
    key = text("key"); value = text("m"); OK(mdb_cursor_get(cursor, &key, &value, MDB_GET_BOTH));
    key = text("key"); value = text("m"); OK(mdb_cursor_get(observer, &key, &value, MDB_GET_BOTH));
    key = text("key"); value = text("n"); OK(mdb_cursor_put(cursor, &key, &value, MDB_CURRENT));
    show(observer, "sibling-current-after-replacement");
    OK(mdb_cursor_del(observer, 0));
    show(cursor, "sibling-current-after-delete");
    mdb_cursor_close(observer);
    mdb_cursor_close(cursor);
    mdb_txn_abort(txn);
    mdb_env_close(env);
    puts("cursor-contract-ok");
    return 0;
}
