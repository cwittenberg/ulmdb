#ifndef ULMDB_H
#define ULMDB_H
#include "lmdb.h"
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define ULMDB_KV 0
#define ULMDB_DUPLICATES 1
#define ULMDB_IPV4 4
#define ULMDB_IPV6 6
/* Copy the immutable base checkpoint into a caller-owned, writable 64-byte buffer.
 * A top-level read reports its pinned snapshot; a top-level writer reports the
 * immutable base before its uncommitted changes. Compare all 64 bytes, rather
 * than transaction sequence numbers, to detect a changed snapshot. No new read
 * transaction or physical pin is acquired. Only live top-level readers/writers
 * are supported: reset, nested, private-read and prepared transactions fail.
 * The output buffer is unchanged on error. Ordinary thread ownership applies. */
int ulmdb_txn_base_checkpoint(MDB_txn *, unsigned char out[64]);
/* Linux, macOS and Windows coordinated file backends: automatic commit-time maintenance, including
 * progress across C transactions. Call after open with no live transactions.
 * A zero segment_threshold disables it. Other arguments bound rewrite input
 * bytes and file unlinks per commit; marking uses the native default budget.
 * Exact historical cohort metadata is archived before payload retirement.
 * NOLOCK and unsupported backends return ENOTSUP/EINVAL, without hidden locks.
 * Ordinary coordinated Linux, macOS and Windows owners default to threshold 128, rewrite 16 MiB,
 * and 8 unlinks. Configuration is process-local. Optional rewrite preparation
 * failures preserve the already-prepared business write. */
int ulmdb_env_set_maintenance(MDB_env *, size_t segment_threshold,
                              uint64_t rewrite_budget_bytes, size_t reclaim_budget_files);
/* flags: MDB_CREATE. Names are UTF-8 strings; NULL selects the main KV db. */
int ulmdb_dbi_open(MDB_txn *, const char *, unsigned int kind, unsigned int flags, MDB_dbi *);
/* CIDRs must be canonical (no host bits). Values have transaction lifetime. */
int ulmdb_prefix_put(MDB_txn *, MDB_dbi, const char *cidr, const MDB_val *);
int ulmdb_prefix_get(MDB_txn *, MDB_dbi, const char *cidr, MDB_val *);
int ulmdb_prefix_delete(MDB_txn *, MDB_dbi, const char *cidr);
/* matched_cidr is UTF-8 without a trailing NUL, valid until the next longest
 * match call or transaction end. Value follows the ordinary MDB_val lifetime. */
int ulmdb_longest_match(MDB_txn *, MDB_dbi, const char *address, MDB_val *matched_cidr, MDB_val *);
#ifdef __cplusplus
}
#endif
#endif
