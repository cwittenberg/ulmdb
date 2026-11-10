/* Simulate a batch byte-budget refusal, preserving the generic bookmark. */
#include "native/sql_batch.h"
#include <dlfcn.h>
#include <stdlib.h>

int ulmdb_sql_cursor_batch(MDB_cursor *cursor, ulmdb_sql_row *rows,
                          size_t capacity, size_t *count, unsigned int op)
{
    if (op == MDB_FIRST && getenv("ULMDB_PG_BATCH_REFUSE_REFILL"))
    {
        typedef int (*batch_function)(MDB_cursor *, ulmdb_sql_row *, size_t,
                                      size_t *, unsigned int);
        batch_function native = (batch_function) dlsym(RTLD_NEXT, "ulmdb_sql_cursor_batch");
        if (!native) return MDB_BAD_TXN;
        return native(cursor, rows, capacity, count, op);
    }
    if (count) *count = 0;
    return MDB_BAD_VALSIZE;
}

int ulmdb_sql_get_batch(MDB_txn *txn, MDB_dbi dbi, const int64_t *ids,
                       size_t candidates, ulmdb_sql_hit *hits, size_t capacity,
                       size_t *consumed, size_t *found)
{
    if (consumed) *consumed = 0;
    if (found) *found = 0;
    if (getenv("ULMDB_PG_BATCH_REFUSE_REFILL") && candidates && ids[0] <= 256)
    {
        typedef int (*lookup_function)(MDB_txn *, MDB_dbi, const int64_t *,
                                       size_t, ulmdb_sql_hit *, size_t,
                                       size_t *, size_t *);
        lookup_function native = (lookup_function) dlsym(RTLD_NEXT, "ulmdb_sql_get_batch");
        if (!native) return MDB_BAD_TXN;
        return native(txn, dbi, ids, candidates, hits, capacity, consumed, found);
    }
    return MDB_BAD_VALSIZE;
}
