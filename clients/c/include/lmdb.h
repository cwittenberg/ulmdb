/* LMDB-compatible declarations implemented by ULMDB. See ../README.md for scope. */
#ifndef ULMDB_LMDB_H
#define ULMDB_LMDB_H
#include <stddef.h>
#ifndef _WIN32
#include <sys/types.h>
#endif
#define MDB_VERSION_MAJOR 0
#define MDB_VERSION_MINOR 9
#define MDB_VERSION_PATCH 31
#define MDB_VERINT(a,b,c) (((a) << 24) | ((b) << 16) | (c))
#define MDB_VERSION_FULL MDB_VERINT(MDB_VERSION_MAJOR,MDB_VERSION_MINOR,MDB_VERSION_PATCH)
#define MDB_VERSION_STRING "LMDB 0.9.31 API / ULMDB adapter"
#ifdef __cplusplus
extern "C" {
#endif
typedef struct MDB_env MDB_env;
typedef struct MDB_txn MDB_txn;
typedef struct MDB_cursor MDB_cursor;
typedef unsigned int MDB_dbi;
#ifdef _WIN32
typedef int mdb_mode_t;
typedef void *mdb_filehandle_t;
#else
typedef mode_t mdb_mode_t;
typedef int mdb_filehandle_t;
#endif
typedef size_t mdb_size_t;
typedef struct MDB_val { size_t mv_size; void *mv_data; } MDB_val;
typedef int MDB_cmp_func(const MDB_val *, const MDB_val *);
typedef void MDB_rel_func(MDB_val *, void *, void *, void *);
typedef void MDB_assert_func(MDB_env *, const char *);
typedef int MDB_msg_func(const char *, void *);
typedef struct MDB_stat {
    unsigned int ms_psize, ms_depth;
    size_t ms_branch_pages, ms_leaf_pages, ms_overflow_pages, ms_entries;
} MDB_stat;
typedef struct MDB_envinfo {
    void *me_mapaddr;
    size_t me_mapsize, me_last_pgno, me_last_txnid;
    unsigned int me_maxreaders, me_numreaders;
} MDB_envinfo;
#define MDB_FIXEDMAP 0x01
#define MDB_ENCRYPT 0x2000U
#define MDB_NOSUBDIR 0x4000
#define MDB_NOSYNC 0x10000
#define MDB_RDONLY 0x20000
#define MDB_NOMETASYNC 0x40000
#define MDB_WRITEMAP 0x80000
#define MDB_MAPASYNC 0x100000
#define MDB_NOTLS 0x200000
#define MDB_NOLOCK 0x400000
#define MDB_NORDAHEAD 0x800000
#define MDB_NOMEMINIT 0x1000000
/* Requires the selected LMDB 1.0 profile. */
#define MDB_PREVSNAPSHOT 0x2000000
#define MDB_REMAP_CHUNKS 0x4000000
#define MDB_REVERSEKEY 0x02
#define MDB_DUPSORT 0x04
#define MDB_INTEGERKEY 0x08
#define MDB_DUPFIXED 0x10
#define MDB_INTEGERDUP 0x20
#define MDB_REVERSEDUP 0x40
#define MDB_CREATE 0x40000
#define MDB_NOOVERWRITE 0x10
#define MDB_NODUPDATA 0x20
#define MDB_CURRENT 0x40
#define MDB_RESERVE 0x10000
#define MDB_APPEND 0x20000
#define MDB_APPENDDUP 0x40000
#define MDB_MULTIPLE 0x80000
#define MDB_CP_COMPACT 0x01
#define MDB_SUCCESS 0
#define MDB_KEYEXIST (-30799)
#define MDB_NOTFOUND (-30798)
#define MDB_PAGE_NOTFOUND (-30797)
#define MDB_CORRUPTED (-30796)
#define MDB_PANIC (-30795)
#define MDB_VERSION_MISMATCH (-30794)
#define MDB_INVALID (-30793)
#define MDB_MAP_FULL (-30792)
#define MDB_DBS_FULL (-30791)
#define MDB_READERS_FULL (-30790)
#define MDB_TLS_FULL (-30789)
#define MDB_TXN_FULL (-30788)
#define MDB_CURSOR_FULL (-30787)
#define MDB_PAGE_FULL (-30786)
#define MDB_MAP_RESIZED (-30785)
#define MDB_INCOMPATIBLE (-30784)
#define MDB_BAD_RSLOT (-30783)
#define MDB_BAD_TXN (-30782)
#define MDB_PROBLEM (-30779)
#define MDB_BAD_CHECKSUM (-30778)
#define MDB_CRYPTO_FAIL (-30777)
#define MDB_ENV_ENCRYPTION (-30776)
#define MDB_TXN_PENDING (-30775)
#define MDB_CANT_ROLLBACK (-30774)
#define MDB_BAD_VALSIZE (-30781)
#define MDB_BAD_DBI (-30780)
#define MDB_LAST_ERRCODE MDB_BAD_DBI
#define MDB_DBIS_BUSY (-30773)
#define MDB_SHORT_WRITE (-30772)
#define MDB_ENV_BUSY (-30771)
#define MDB_IS_READONLY (-30770)
#define MDB_ADDR_BUSY (-30769)
typedef enum MDB_cursor_op {
    MDB_FIRST, MDB_FIRST_DUP, MDB_GET_BOTH, MDB_GET_BOTH_RANGE,
    MDB_GET_CURRENT, MDB_GET_MULTIPLE, MDB_LAST, MDB_LAST_DUP,
    MDB_NEXT, MDB_NEXT_DUP, MDB_NEXT_MULTIPLE, MDB_NEXT_NODUP,
    MDB_PREV, MDB_PREV_DUP, MDB_PREV_NODUP, MDB_SET, MDB_SET_KEY,
    MDB_SET_RANGE, MDB_PREV_MULTIPLE
} MDB_cursor_op;
char *mdb_version(int *, int *, int *);
char *mdb_strerror(int);
int mdb_env_create(MDB_env **);
int mdb_env_open(MDB_env *, const char *, unsigned int, mdb_mode_t);
void mdb_env_close(MDB_env *);
int mdb_env_set_mapsize(MDB_env *, size_t);
int mdb_env_set_maxdbs(MDB_env *, MDB_dbi);
int mdb_env_set_maxreaders(MDB_env *, unsigned int);
int mdb_env_get_maxreaders(MDB_env *, unsigned int *);
int mdb_env_get_maxkeysize(MDB_env *);
int mdb_env_set_flags(MDB_env *, unsigned int, int);
int mdb_env_get_flags(MDB_env *, unsigned int *);
int mdb_env_get_path(MDB_env *, const char **);
int mdb_env_get_fd(MDB_env *, mdb_filehandle_t *);
int mdb_env_set_userctx(MDB_env *, void *);
void *mdb_env_get_userctx(MDB_env *);
int mdb_env_set_assert(MDB_env *, MDB_assert_func *);
int mdb_env_sync(MDB_env *, int);
/* LMDB 1.0 rollback: call immediately after commit, with no intervening
 * environment operations in any process. Never call twice consecutively.
 * All other environment instances must be closed. MDB_NOLOCK requires the
 * caller to provide the same exclusive-use guarantee. */
int mdb_env_rollback(MDB_env *, mdb_size_t);
int mdb_env_stat(MDB_env *, MDB_stat *);
int mdb_env_info(MDB_env *, MDB_envinfo *);
int mdb_env_copy(MDB_env *, const char *);
int mdb_env_copy2(MDB_env *, const char *, unsigned int);
int mdb_env_copyfd(MDB_env *, mdb_filehandle_t);
int mdb_env_copyfd2(MDB_env *, mdb_filehandle_t, unsigned int);
/* LMDB 1.0 incremental backups. Native delta bytes require an exact native
 * base backup; they are distinct from full native archives and LMDB page files. */
int mdb_env_incr_dumpfd(MDB_env *, mdb_filehandle_t, size_t);
int mdb_env_incr_dump(MDB_env *, const char *, size_t);
int mdb_env_incr_loadfd(MDB_env *, mdb_filehandle_t);
typedef int MDB_enc_func(const MDB_val *, MDB_val *, const MDB_val *, int);
typedef void MDB_sum_func(const MDB_val *, MDB_val *, const MDB_val *);
typedef int MDB_str2key_func(const char *, MDB_val *);
typedef struct MDB_crypto_funcs {
    MDB_str2key_func *mcf_str2key;
    MDB_enc_func *mcf_encfunc;
    MDB_sum_func *mcf_sumfunc;
    int mcf_keysize;
    int mcf_esumsize;
    int mcf_sumsize;
} MDB_crypto_funcs;
typedef MDB_crypto_funcs *MDB_crypto_hooks(void);
int mdb_env_set_checksum(MDB_env *, MDB_sum_func *, unsigned int);
int mdb_env_set_encrypt(MDB_env *, MDB_enc_func *, const MDB_val *, unsigned int);
void *mdb_modload(const char *, const char *, MDB_crypto_funcs **, char **);
void mdb_modunload(void *);
void mdb_modsetup(MDB_env *, MDB_crypto_funcs *, const char *);
int mdb_txn_begin(MDB_env *, MDB_txn *, unsigned int, MDB_txn **);
int mdb_txn_commit(MDB_txn *);
/* LMDB 1.0 extension: dirty outer writers remain pending for commit/abort. */
int mdb_txn_prepare(MDB_txn *);
void mdb_txn_abort(MDB_txn *);
void mdb_txn_reset(MDB_txn *);
int mdb_txn_renew(MDB_txn *);
MDB_env *mdb_txn_env(MDB_txn *);
size_t mdb_txn_id(MDB_txn *);
/* Supported LMDB 1.0 extension; returns MDB_RDONLY or zero. */
int mdb_txn_flags(MDB_txn *, unsigned int *);
int mdb_dbi_open(MDB_txn *, const char *, unsigned int, MDB_dbi *);
void mdb_dbi_close(MDB_env *, MDB_dbi);
int mdb_dbi_flags(MDB_txn *, MDB_dbi, unsigned int *);
int mdb_stat(MDB_txn *, MDB_dbi, MDB_stat *);
int mdb_drop(MDB_txn *, MDB_dbi, int);
int mdb_get(MDB_txn *, MDB_dbi, MDB_val *, MDB_val *);
int mdb_put(MDB_txn *, MDB_dbi, MDB_val *, MDB_val *, unsigned int);
int mdb_del(MDB_txn *, MDB_dbi, MDB_val *, MDB_val *);
int mdb_set_compare(MDB_txn *, MDB_dbi, MDB_cmp_func *);
int mdb_set_dupsort(MDB_txn *, MDB_dbi, MDB_cmp_func *);
int mdb_set_relfunc(MDB_txn *, MDB_dbi, MDB_rel_func *);
int mdb_set_relctx(MDB_txn *, MDB_dbi, void *);
int mdb_cmp(MDB_txn *, MDB_dbi, const MDB_val *, const MDB_val *);
int mdb_dcmp(MDB_txn *, MDB_dbi, const MDB_val *, const MDB_val *);
int mdb_cursor_open(MDB_txn *, MDB_dbi, MDB_cursor **);
void mdb_cursor_close(MDB_cursor *);
int mdb_cursor_renew(MDB_txn *, MDB_cursor *);
MDB_txn *mdb_cursor_txn(MDB_cursor *);
MDB_dbi mdb_cursor_dbi(MDB_cursor *);
int mdb_cursor_is_db(MDB_cursor *);
int mdb_cursor_get(MDB_cursor *, MDB_val *, MDB_val *, MDB_cursor_op);
int mdb_cursor_put(MDB_cursor *, MDB_val *, MDB_val *, unsigned int);
int mdb_cursor_del(MDB_cursor *, unsigned int);
int mdb_cursor_count(MDB_cursor *, size_t *);
int mdb_reader_check(MDB_env *, int *);
int mdb_reader_list(MDB_env *, MDB_msg_func *, void *);
/* Configure immutable database pages before opening; existing files retain their persisted size. */
int mdb_env_set_pagesize(MDB_env *env, int size);
/* Linux: opt into a setgid shared namespace before env_open; use group-RW mode
 * and a compatible umask. Existing namespace rights are never widened by open. */
int ulmdb_env_set_shared_group(MDB_env *, unsigned int);
/* Explicit exclusive migration of recognized, caller-owned regular files.
 * Close all environments first. UINT_MAX selects private ownership. */
int ulmdb_file_access_migrate(const char *, unsigned int, unsigned int);
#ifdef __cplusplus
}
#endif
#endif
