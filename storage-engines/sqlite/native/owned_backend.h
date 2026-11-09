#ifndef ULMDB_SQLITE_OWNED_BACKEND_H
#define ULMDB_SQLITE_OWNED_BACKEND_H
/* Private SQLite backend ABI. This is not an LMDB-compatible library.
 * Uses the existing declarations solely for the small frontend subset.
 * All handles are process-local; transaction/cursor handles are thread-local.
 * Borrowed get bytes live until update/transaction end; cursor bytes until its
 * next step/close or transaction end. Copy before mutating a writer.
 */
#define mdb_env_create usql_env_create
#define mdb_env_set_maxdbs usql_env_set_maxdbs
#define mdb_env_open usql_env_open
#define mdb_env_close usql_env_close
#define mdb_txn_begin usql_txn_begin
#define mdb_txn_commit usql_txn_commit
#define mdb_txn_abort usql_txn_abort
#define mdb_dbi_open usql_dbi_open
#define mdb_dbi_close usql_dbi_close
#define mdb_get usql_get
#define mdb_put usql_put
#define mdb_del usql_del
#define mdb_cursor_open usql_cursor_open
#define mdb_cursor_get usql_cursor_get
#define mdb_cursor_close usql_cursor_close
#define mdb_strerror usql_strerror
#include <stdint.h>
#include "lmdb.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Empty read-carrier reuse. Recycle drops the complete snapshot and owner;
 * renew always captures a fresh head. An Idle carrier has no native capability,
 * may move sequentially between threads, and must be freed with discard.
 * Each function returns zero on success; failures preserve handle ownership. */
int usql_read_recycle(MDB_txn *);
int usql_read_renew(MDB_env *, MDB_txn *);
int usql_read_discard(MDB_txn *);
/* Fresh typed point read. NULL/Idle *slot becomes Active on success or
 * MDB_NOTFOUND. Other failures preserve slot ownership/state and output.
 * Its borrowed result lasts until recycle/abort, as for usql_get. */
int usql_read_point(MDB_env *, MDB_txn **slot, int64_t id, MDB_val *out);
/* Read-only row iterator. FIRST ignores input id; SET_RANGE seeks to input id;
 * NEXT advances. Success replaces id/value; failures preserve both outputs. */
int usql_cursor_row(MDB_cursor *, int64_t *id, MDB_val *out, int operation);
#ifdef __cplusplus
}
#endif
#endif
