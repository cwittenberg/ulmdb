/* Optional SQLite VFS: normal SQLite pages in a private ULMDB page store. */
#include <sqlite3ext.h>
SQLITE_EXTENSION_INIT1
#include "native/pages_backend.h"
#include <string.h>

typedef struct {
    sqlite3_file base;
    UpgFile *store;
} PageFile;

static sqlite3_vfs page_vfs;
static sqlite3_vfs *parent_vfs;

static int page_close(sqlite3_file *base) {
    PageFile *file = (PageFile *)base;
    int rc = upg_close(file->store);
    file->store = 0;
    base->pMethods = 0;
    return rc;
}
static int page_read(sqlite3_file *base, void *out, int bytes, sqlite3_int64 offset) {
    return upg_read(((PageFile *)base)->store, out, bytes, (int64_t)offset);
}
static int page_write(sqlite3_file *base, const void *in, int bytes, sqlite3_int64 offset) {
    return upg_write(((PageFile *)base)->store, in, bytes, (int64_t)offset);
}
static int page_truncate(sqlite3_file *base, sqlite3_int64 size) {
    return upg_truncate(((PageFile *)base)->store, (int64_t)size);
}
static int page_sync(sqlite3_file *base, int flags) {
    (void)flags;
    return upg_sync(((PageFile *)base)->store);
}
static int page_size(sqlite3_file *base, sqlite3_int64 *out) {
    int64_t size = 0;
    int rc = upg_size(((PageFile *)base)->store, &size);
    if (rc == SQLITE_OK) *out = (sqlite3_int64)size;
    return rc;
}
static int page_lock(sqlite3_file *base, int level) {
    return upg_lock(((PageFile *)base)->store, level);
}
static int page_unlock(sqlite3_file *base, int level) {
    return upg_unlock(((PageFile *)base)->store, level);
}
static int page_reserved(sqlite3_file *base, int *out) {
    return upg_reserved(((PageFile *)base)->store, out);
}
static int page_pragma(char **args) {
    const char *name = args[1], *value = args[2];
    if (!value) return SQLITE_NOTFOUND;
    if (sqlite3_stricmp(name, "journal_mode") == 0 &&
        sqlite3_stricmp(value, "delete") != 0) {
        args[0] = sqlite3_mprintf("ULMDB VFS requires journal_mode=DELETE");
        return args[0] ? SQLITE_ERROR : SQLITE_NOMEM;
    }
    if (sqlite3_stricmp(name, "synchronous") == 0 &&
        sqlite3_stricmp(value, "full") && sqlite3_stricmp(value, "extra") &&
        strcmp(value, "2") && strcmp(value, "3")) {
        args[0] = sqlite3_mprintf("ULMDB VFS requires synchronous=FULL or EXTRA");
        return args[0] ? SQLITE_ERROR : SQLITE_NOMEM;
    }
    return SQLITE_NOTFOUND;
}
static int page_control(sqlite3_file *base, int operation, void *arg) {
    PageFile *file = (PageFile *)base;
    switch (operation) {
    case SQLITE_FCNTL_BEGIN_ATOMIC_WRITE:
        return upg_begin_atomic(file->store);
    case SQLITE_FCNTL_COMMIT_ATOMIC_WRITE:
        return upg_commit_atomic(file->store);
    case SQLITE_FCNTL_ROLLBACK_ATOMIC_WRITE:
        return upg_rollback_atomic(file->store);
    case SQLITE_FCNTL_COMMIT_PHASETWO:
        /* SQLite can shrink the database after its phase-one sync and after
         * closing the journal. Persist that staged truncation before unlock. */
        return upg_sync(file->store);
    case SQLITE_FCNTL_LOCKSTATE:
        return upg_lockstate(file->store, (int *)arg);
    case SQLITE_FCNTL_VFSNAME:
        *(char **)arg = sqlite3_mprintf("ulmdb/%s", parent_vfs->zName);
        return *(char **)arg ? SQLITE_OK : SQLITE_NOMEM;
    case SQLITE_FCNTL_VFS_POINTER:
        *(sqlite3_vfs **)arg = &page_vfs;
        return SQLITE_OK;
    case SQLITE_FCNTL_FILE_POINTER:
        *(sqlite3_file **)arg = base;
        return SQLITE_OK;
    case SQLITE_FCNTL_SIZE_HINT:
        return SQLITE_OK;
    case SQLITE_FCNTL_MMAP_SIZE:
        *(sqlite3_int64 *)arg = 0;
        return SQLITE_OK;
    case SQLITE_FCNTL_POWERSAFE_OVERWRITE:
        *(int *)arg = 0;
        return SQLITE_OK;
    case SQLITE_FCNTL_PRAGMA:
        return page_pragma((char **)arg);
    default:
        return SQLITE_NOTFOUND;
    }
}
static int page_sector(sqlite3_file *base) { (void)base; return 4096; }
static int page_capabilities(sqlite3_file *base) {
    (void)base;
    return SQLITE_IOCAP_BATCH_ATOMIC;
}
static const sqlite3_io_methods page_methods = {
    .iVersion = 1,
    .xClose = page_close, .xRead = page_read, .xWrite = page_write,
    .xTruncate = page_truncate, .xSync = page_sync, .xFileSize = page_size,
    .xLock = page_lock, .xUnlock = page_unlock,
    .xCheckReservedLock = page_reserved, .xFileControl = page_control,
    .xSectorSize = page_sector, .xDeviceCharacteristics = page_capabilities
};

static int vfs_open(sqlite3_vfs *vfs, const char *name, sqlite3_file *base,
                    int flags, int *out_flags) {
    PageFile *file = (PageFile *)base;
    int rc;
    (void)vfs;
    if (!(flags & SQLITE_OPEN_MAIN_DB))
        return parent_vfs->xOpen(parent_vfs, name, base, flags, out_flags);
    memset(file, 0, sizeof(*file));
    if (!name || (flags & SQLITE_OPEN_DELETEONCLOSE)) return SQLITE_CANTOPEN;
    rc = upg_open(name, flags, &file->store);
    if (rc != SQLITE_OK) return rc;
    base->pMethods = &page_methods;
    if (out_flags) *out_flags = flags;
    return SQLITE_OK;
}
static int vfs_delete(sqlite3_vfs *vfs, const char *name, int sync_dir) {
    (void)vfs;
    return parent_vfs->xDelete(parent_vfs, name, sync_dir);
}
static int vfs_access(sqlite3_vfs *vfs, const char *name, int flags, int *out) {
    (void)vfs;
    return parent_vfs->xAccess(parent_vfs, name, flags, out);
}
static int vfs_fullpath(sqlite3_vfs *vfs, const char *name, int size, char *out) {
    (void)vfs;
    return parent_vfs->xFullPathname(parent_vfs, name, size, out);
}
static void *vfs_dlopen(sqlite3_vfs *vfs, const char *name) {
    (void)vfs;
    return parent_vfs->xDlOpen(parent_vfs, name);
}
static void vfs_dlerror(sqlite3_vfs *vfs, int size, char *out) {
    (void)vfs;
    parent_vfs->xDlError(parent_vfs, size, out);
}
static void (*vfs_dlsym(sqlite3_vfs *vfs, void *handle, const char *name))(void) {
    (void)vfs;
    return parent_vfs->xDlSym(parent_vfs, handle, name);
}
static void vfs_dlclose(sqlite3_vfs *vfs, void *handle) {
    (void)vfs;
    parent_vfs->xDlClose(parent_vfs, handle);
}
static int vfs_random(sqlite3_vfs *vfs, int size, char *out) {
    (void)vfs;
    return parent_vfs->xRandomness(parent_vfs, size, out);
}
static int vfs_sleep(sqlite3_vfs *vfs, int micros) {
    (void)vfs;
    return parent_vfs->xSleep(parent_vfs, micros);
}
static int vfs_time(sqlite3_vfs *vfs, double *out) {
    (void)vfs;
    return parent_vfs->xCurrentTime(parent_vfs, out);
}
static int vfs_last_error(sqlite3_vfs *vfs, int size, char *out) {
    (void)vfs;
    if (parent_vfs->xGetLastError) return parent_vfs->xGetLastError(parent_vfs, size, out);
    if (size) out[0] = 0;
    return 0;
}
static int vfs_time64(sqlite3_vfs *vfs, sqlite3_int64 *out) {
    (void)vfs;
    return parent_vfs->xCurrentTimeInt64(parent_vfs, out);
}
static int vfs_set_call(sqlite3_vfs *vfs, const char *name, sqlite3_syscall_ptr call) {
    (void)vfs;
    return parent_vfs->xSetSystemCall ? parent_vfs->xSetSystemCall(parent_vfs, name, call) : SQLITE_NOTFOUND;
}
static sqlite3_syscall_ptr vfs_get_call(sqlite3_vfs *vfs, const char *name) {
    (void)vfs;
    return parent_vfs->xGetSystemCall ? parent_vfs->xGetSystemCall(parent_vfs, name) : 0;
}
static const char *vfs_next_call(sqlite3_vfs *vfs, const char *name) {
    (void)vfs;
    return parent_vfs->xNextSystemCall ? parent_vfs->xNextSystemCall(parent_vfs, name) : 0;
}

int sqlite3_ulmdbvfs_init(sqlite3 *db, char **err, const sqlite3_api_routines *api) {
    sqlite3_mutex *mutex;
    sqlite3_vfs *existing;
    int rc = SQLITE_OK;
    (void)db;
    SQLITE_EXTENSION_INIT2(api);
#ifndef __linux__
    *err = sqlite3_mprintf("ULMDB VFS currently requires Linux");
    return SQLITE_ERROR;
#endif
    if (!api || !api->compileoption_used ||
        !sqlite3_compileoption_used("ENABLE_BATCH_ATOMIC_WRITE")) {
        *err = sqlite3_mprintf("ULMDB VFS requires SQLite ENABLE_BATCH_ATOMIC_WRITE");
        return SQLITE_ERROR;
    }
    mutex = api->mutex_alloc && api->mutex_enter && api->mutex_leave
        ? sqlite3_mutex_alloc(SQLITE_MUTEX_STATIC_VFS1) : 0;
    if (mutex) sqlite3_mutex_enter(mutex);
    existing = sqlite3_vfs_find("ulmdb");
    if (existing && existing != &page_vfs) {
        *err = sqlite3_mprintf("a different VFS already uses the name ulmdb");
        rc = SQLITE_ERROR;
    } else if (!existing) {
        parent_vfs = sqlite3_vfs_find(0);
        if (!parent_vfs) rc = SQLITE_NOTFOUND;
        else {
            memset(&page_vfs, 0, sizeof(page_vfs));
            page_vfs.iVersion = parent_vfs->iVersion > 3 ? 3 : parent_vfs->iVersion;
            page_vfs.szOsFile = parent_vfs->szOsFile > (int)sizeof(PageFile)
                ? parent_vfs->szOsFile : (int)sizeof(PageFile);
            page_vfs.mxPathname = parent_vfs->mxPathname;
            page_vfs.zName = "ulmdb";
            page_vfs.xOpen = vfs_open; page_vfs.xDelete = vfs_delete;
            page_vfs.xAccess = vfs_access; page_vfs.xFullPathname = vfs_fullpath;
            page_vfs.xDlOpen = vfs_dlopen; page_vfs.xDlError = vfs_dlerror;
            page_vfs.xDlSym = vfs_dlsym; page_vfs.xDlClose = vfs_dlclose;
            page_vfs.xRandomness = vfs_random; page_vfs.xSleep = vfs_sleep;
            page_vfs.xCurrentTime = vfs_time; page_vfs.xGetLastError = vfs_last_error;
            if (page_vfs.iVersion >= 2 && parent_vfs->xCurrentTimeInt64)
                page_vfs.xCurrentTimeInt64 = vfs_time64;
            if (page_vfs.iVersion >= 3) {
                page_vfs.xSetSystemCall = vfs_set_call;
                page_vfs.xGetSystemCall = vfs_get_call;
                page_vfs.xNextSystemCall = vfs_next_call;
            }
            rc = sqlite3_vfs_register(&page_vfs, 0);
        }
    }
    if (mutex) sqlite3_mutex_leave(mutex);
    return rc;
}
