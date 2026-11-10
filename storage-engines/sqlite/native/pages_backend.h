#ifndef ULMDB_SQLITE_PAGES_BACKEND_H
#define ULMDB_SQLITE_PAGES_BACKEND_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Private page-store ABI. Result codes, open flags and lock levels use
 * SQLite's constants. Reads zero-fill any short suffix before IOERR_SHORT_READ.
 * Successful commits and syncs are FULL durable engine publications. */
typedef struct UpgFile UpgFile;
int upg_open(const char *path, int flags, UpgFile **out);
int upg_close(UpgFile *file);
int upg_read(UpgFile *file, void *out, int bytes, int64_t offset);
int upg_write(UpgFile *file, const void *in, int bytes, int64_t offset);
int upg_truncate(UpgFile *file, int64_t size);
int upg_size(UpgFile *file, int64_t *out);
int upg_lock(UpgFile *file, int level);
int upg_unlock(UpgFile *file, int level);
int upg_lockstate(UpgFile *file, int *out);
int upg_reserved(UpgFile *file, int *out);
int upg_begin_atomic(UpgFile *file);
int upg_commit_atomic(UpgFile *file);
int upg_rollback_atomic(UpgFile *file);
int upg_sync(UpgFile *file);

#ifdef __cplusplus
}
#endif
#endif
