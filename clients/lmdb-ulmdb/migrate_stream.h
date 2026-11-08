/* The importer loads only ULMDB; the real LMDB reader remains a subprocess. */
#ifndef ULMDB_MIGRATE_STREAM_H
#define ULMDB_MIGRATE_STREAM_H
#include "../c/include/lmdb.h"
#include <stdint.h>
#include <stddef.h>

struct migration_api {
    void *library;
    int (*env_create)(MDB_env **);
    int (*env_set_maxdbs)(MDB_env *, MDB_dbi);
    int (*env_set_mapsize)(MDB_env *, size_t);
    int (*env_set_maxreaders)(MDB_env *, unsigned int);
    int (*env_set_pagesize)(MDB_env *, int);
    int (*env_get_maxkeysize)(MDB_env *);
    int (*env_open)(MDB_env *, const char *, unsigned int, mdb_mode_t);
    void (*env_close)(MDB_env *);
    int (*txn_begin)(MDB_env *, MDB_txn *, unsigned int, MDB_txn **);
    void (*txn_abort)(MDB_txn *);
    int (*txn_commit)(MDB_txn *);
    int (*dbi_open)(MDB_txn *, const char *, unsigned int, MDB_dbi *);
    int (*put)(MDB_txn *, MDB_dbi, MDB_val *, MDB_val *, unsigned int);
    char *(*strerror)(int);
};
struct migration_info {
    uint64_t source_sequence, map_size, databases, records;
    uint32_t max_readers, page_size, max_key_size;
};
int migration_load(struct migration_api *, const char *, char *, size_t);
void migration_unload(struct migration_api *);
int migration_check(const struct migration_api *, int, const char *, char *, size_t);
int migration_read_header(int, struct migration_info *, char *, size_t);
int migration_replay(const struct migration_api *, MDB_env *, int,
                     struct migration_info *, char *, size_t);
#endif
