#ifndef ULMDB_POSTGRESQL_SQL_BATCH_H
#define ULMDB_POSTGRESQL_SQL_BATCH_H
#include "ulmdb.h"

/* Format admission for an active ordinary read or top-level writer. Requires a
 * direct read or authenticated writer view of a named KV database, flags0,
 * native bytewise key/duplicate axes and no
 * comparator/relocation callbacks. Does not rewrite ordering or records.
 * INCOMPATIBLE is an unsupported SQL format; other errors remain operational. */
int ulmdb_sql_bytewise_format(MDB_txn *txn, MDB_dbi dbi);

/* Optional ulmdb-c sql-batch feature. Named, direct, bytewise readonly KV only;
 * INCOMPATIBLE leaves cursor position untouched for a generic-API fallback.
 * capacity is 1..1024 (256 recommended); op is MDB_FIRST or MDB_NEXT.
 * count is zero on failure. Success returns at least one row; NOTFOUND is EOF.
 * Rows borrow the real transaction snapshot until reset/commit/abort, just as
 * mdb_get; callers must copy data they retain beyond that lifetime. No callback
 * crosses Rust. A call emits at most 64 MiB of key+value bytes. */
typedef struct ulmdb_sql_row {
    MDB_val key;
    MDB_val value;
} ulmdb_sql_row;
int ulmdb_sql_cursor_batch(MDB_cursor *cursor, ulmdb_sql_row *rows,
                          size_t capacity, size_t *count, unsigned int op);

/* Exact SQL int64 keys use sign-flipped big-endian encoding. Candidates and
 * output capacity are at most 1024. Hits preserve input order/duplicates; misses
 * consume candidates without producing hits. consumed/found are zero on error.
 * Success may return zero hits but must consume input when candidates>0.
 * A byte/capacity stop leaves the next candidate unconsumed. First-hit budget
 * refusal returns BAD_VALSIZE for scalar fallback. Values borrow the txn;
 * input keys need only live through this call. Outputs must be disjoint. */
typedef struct ulmdb_sql_hit {
    int64_t id;
    MDB_val value;
} ulmdb_sql_hit;
int ulmdb_sql_get_batch(MDB_txn *txn, MDB_dbi dbi, const int64_t *ids,
                       size_t candidates, ulmdb_sql_hit *hits, size_t capacity,
                       size_t *consumed, size_t *found);
#endif
