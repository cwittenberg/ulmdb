#ifndef ULMDB_SQLITE_BTREE_NATIVE_H
#define ULMDB_SQLITE_BTREE_NATIVE_H
/* Private fixed-format backend, not an LMDB-compatible library. Active handles
 * stay on their creator thread. Read bytes are owned by the actual pinned
 * native transaction; writer bytes last only until its next mutation. */
struct MDB_txn;
int ubt_txn_validate(struct MDB_txn *txn);
unsigned int ubt_process_id_fast(void);
#define mdb_env_create ubt_env_create
#define mdb_env_open ubt_env_open
#define mdb_env_close ubt_env_close
#define mdb_env_set_maxdbs ubt_env_set_maxdbs
#define mdb_env_set_maxreaders ubt_env_set_maxreaders
#define mdb_env_set_mapsize ubt_env_set_mapsize
#define mdb_env_set_flags ubt_env_set_flags
#define mdb_env_get_maxkeysize ubt_env_get_maxkeysize
#define mdb_env_stat ubt_env_stat
#define mdb_env_info ubt_env_info
#define mdb_txn_begin ubt_txn_begin
#define mdb_txn_commit ubt_txn_commit
#define mdb_txn_abort ubt_txn_abort
#define mdb_txn_reset ubt_txn_reset
#define mdb_txn_renew ubt_txn_renew
#define ulmdb_txn_base_checkpoint ubt_txn_base_checkpoint
#define mdb_dbi_open ubt_dbi_open
#define mdb_get ubt_get
#define mdb_put ubt_put
#define mdb_stat ubt_stat
#define mdb_drop ubt_drop
#define mdb_cursor_open ubt_cursor_open
#define mdb_cursor_get ubt_cursor_get
#define mdb_cursor_put ubt_cursor_put
#define mdb_cursor_del ubt_cursor_del
#define mdb_cursor_close ubt_cursor_close
#endif
