/* Build/run commands are in ../README.md. Uses only the common LMDB API. */
#include <lmdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(int result) {
    if (result != MDB_SUCCESS) {
        fprintf(stderr, "database error %d: %s\n", result, mdb_strerror(result));
        exit(1);
    }
}

static MDB_val bytes(const void *data, size_t size) {
    MDB_val value = {size, (void *)data};
    return value;
}

static void print_hex(MDB_val value) {
    const unsigned char *data = value.mv_data;
    for (size_t i = 0; i < value.mv_size; ++i) printf("%02x", data[i]);
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s DATABASE_DIRECTORY\n", argv[0]);
        return 2;
    }
    MDB_env *env;
    MDB_txn *write, *read;
    MDB_dbi items;
    MDB_cursor *cursor;
    const unsigned char binary[] = {0, 255, 128};

    check(mdb_env_create(&env));
    check(mdb_env_set_maxdbs(env, 4));
    check(mdb_env_open(env, argv[1], MDB_NOTLS, 0600));
    check(mdb_txn_begin(env, NULL, 0, &write));
    check(mdb_dbi_open(write, "items", MDB_CREATE, &items));
    MDB_val key = bytes("greeting", 8), data = bytes("hello", 5);
    check(mdb_put(write, items, &key, &data, 0));
    key = bytes("binary", 6);
    data = bytes(binary, sizeof binary);
    check(mdb_put(write, items, &key, &data, 0));
    check(mdb_txn_commit(write));

    check(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    key = bytes("greeting", 8);
    check(mdb_get(read, items, &key, &data));
    printf("greeting: %.*s\n", (int)data.mv_size, (const char *)data.mv_data);
    /* Results borrow the transaction. Read them before reset, update or end. */
    check(mdb_cursor_open(read, items, &cursor));
    int result = mdb_cursor_get(cursor, &key, &data, MDB_FIRST);
    while (result == MDB_SUCCESS) {
        print_hex(key);
        printf(" = ");
        print_hex(data);
        putchar('\n');
        result = mdb_cursor_get(cursor, &key, &data, MDB_NEXT);
    }
    if (result != MDB_NOTFOUND) check(result);
    mdb_cursor_close(cursor);
    mdb_txn_abort(read);
    mdb_dbi_close(env, items);
    mdb_env_close(env);
    return 0;
}
