#ifndef ULMDB_POSTGRESQL_SQL_HASH_H
#define ULMDB_POSTGRESQL_SQL_HASH_H
#include "ulmdb.h"

/* Optional sql-hash C feature. Counts concern this exact live read transaction.
 * Indexes borrow authoritative native bytes and end with its real snapshot.
 * Preparation is lazy and charged to the operation that first uses a database.
 * Unsupported/budget-refused databases retain native ordered reads. */
typedef struct ulmdb_sql_hash_statistics {
    size_t prepared_databases;
    size_t refused_databases;
    size_t preparations;
    size_t entries;
    size_t index_bytes;
    size_t total_value_bytes;
} ulmdb_sql_hash_statistics;
int ulmdb_sql_hash_stats(MDB_txn *txn, ulmdb_sql_hash_statistics *out);
#endif
