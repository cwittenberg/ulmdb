/* Test-only, opt-in LD_PRELOAD fault boundary. No engine implementation. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/vfs.h>
#include <linux/magic.h>
#include <unistd.h>

/* Each test invokes one worker thread in its own process. The marker is absent
 * for startup/seed/snapshot capture, and consumed by exactly one target capacity query.
 * A separate observation marker stays active until failed commit returns, so
 * transient post-error target writes/syncs are also observable. Logging is
 * intentionally diagnostic and never used for timing. */
static int target_fd(int fd) {
    int saved = errno;
    const char *target = getenv("ULMDB_RESIDENT_CAPACITY_EIO_WAL");
    char proc[64], path[PATH_MAX];
    int matched = 0;
    if (target) {
        snprintf(proc, sizeof(proc), "/proc/self/fd/%d", fd);
        ssize_t n = readlink(proc, path, sizeof(path) - 1);
        if (n >= 0) {
            path[n] = 0;
            matched = !strcmp(path, target);
        }
    }
    errno = saved;
    return matched;
}
static int observing(void) {
    int saved = errno;
    const char *marker = getenv("ULMDB_RESIDENT_CAPACITY_EIO_OBSERVE");
    int active = marker && access(marker, F_OK) == 0;
    errno = saved;
    return active;
}
static void event(const char *name, long long offset, size_t length,
                  long long result, int error) {
    int saved = errno;
    const char *path = getenv("ULMDB_RESIDENT_CAPACITY_EIO_LOG");
    if (path) {
        int out = open(path, O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0600);
        if (out >= 0) {
            dprintf(out, "%s offset=%lld length=%zu result=%lld errno=%d\n",
                    name, offset, length, result, error);
            close(out);
        }
    }
    errno = saved;
}
static int reject_stat(int fd, const char *name) {
    if (!target_fd(fd)) return 0;
    const char *marker = getenv("ULMDB_RESIDENT_CAPACITY_EIO_ARM");
    /* No process-global environment mutation or fake File descriptor. */
    if (marker && unlink(marker) == 0) {
        event(name, 0, 0, -1, EIO);
        errno = EIO;
        return 1;
    }
    return 0;
}
static void *required_symbol(const char *name) {
    void *symbol = dlsym(RTLD_NEXT, name);
    if (!symbol) _exit(125);
    return symbol;
}
/* Independently observe the standard library's unarmed metadata method. This
 * keeps fallback assertions portable when statx is unavailable on a kernel. */
static void metadata_sample(int fd, const char *name) {
    int saved = errno;
    const char *marker = getenv("ULMDB_RESIDENT_CAPACITY_METADATA_SAMPLE");
    const char *path = getenv("ULMDB_RESIDENT_CAPACITY_METADATA_LOG");
    if (target_fd(fd) && marker && path && access(marker, F_OK) == 0) {
        int out = open(path, O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0600);
        if (out >= 0) { dprintf(out, "%s\n", name); close(out); }
    }
    errno = saved;
}
int fstat(int fd, struct stat *status) {
    int (*next)(int, struct stat *) = required_symbol("fstat");
    metadata_sample(fd, "fstat");
    if (reject_stat(fd, "fstat")) return -1;
    return next(fd, status);
}
int fstat64(int fd, struct stat64 *status) {
    int (*next)(int, struct stat64 *) = required_symbol("fstat64");
    metadata_sample(fd, "fstat64");
    if (reject_stat(fd, "fstat64")) return -1;
    return next(fd, status);
}
/* Match the exact target fd plus AT_EMPTY_PATH; do not dereference pathname:
 * Rust availability probes may pass NULL. Keep
 * fstat/fstat64 above for the standard-library fallback on other kernels. */
int statx(int fd, const char *path, int flags, unsigned mask, struct statx *status) {
    int (*next)(int, const char *, int, unsigned, struct statx *) = required_symbol("statx");
    if (flags & AT_EMPTY_PATH) {
        metadata_sample(fd, "statx");
        if (reject_stat(fd, "statx")) return -1;
    }
    return next(fd, path, flags, mask, status);
}
static void detection_event(const char *name, const char *mode, int result,
                            unsigned long long kind, int error) {
    int saved = errno;
    const char *path = getenv("ULMDB_RESIDENT_CAPACITY_DETECT_LOG");
    if (path) {
        int out = open(path, O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0600);
        if (out >= 0) {
            const char *strategy = result == 0 &&
                (kind == TMPFS_MAGIC || kind == RAMFS_MAGIC) ? "ram" : "metadata";
            dprintf(out, "%s mode=%s result=%d kind=%llu errno=%d strategy=%s\n",
                    name, mode, result, kind, error, strategy);
            close(out);
        }
    }
    errno = saved;
}
#define FILESYSTEM_WRAPPER(NAME, TYPE) \
int NAME(int fd, struct TYPE *status) { \
    int (*next)(int, struct TYPE *) = required_symbol(#NAME); \
    int target = target_fd(fd); \
    const char *mode = getenv("ULMDB_RESIDENT_CAPACITY_DETECT_MODE"); \
    if (!mode) mode = "normal"; \
    if (target && !strcmp(mode, "eio")) { \
        detection_event(#NAME, mode, -1, 0, EIO); \
        errno = EIO; \
        return -1; \
    } \
    int result = next(fd, status); \
    int saved = errno; \
    if (target) { \
        if (result == 0 && !strcmp(mode, "unknown")) status->f_type = 0; \
        detection_event(#NAME, mode, result, result == 0 ? \
                        (unsigned long long)status->f_type : 0, result < 0 ? saved : 0); \
    } \
    errno = saved; \
    return result; \
}
FILESYSTEM_WRAPPER(fstatfs, statfs)
FILESYSTEM_WRAPPER(fstatfs64, statfs64)
#define POSITIONAL_WRAPPER(NAME, TYPE, BUFFER) \
ssize_t NAME(int fd, BUFFER buffer, size_t length, TYPE offset) { \
    ssize_t (*next)(int, BUFFER, size_t, TYPE) = required_symbol(#NAME); \
    int trace = target_fd(fd) && observing(); \
    ssize_t result = next(fd, buffer, length, offset); \
    int saved = errno; \
    if (trace) event(#NAME, (long long)offset, length, (long long)result, \
                     result < 0 ? saved : 0); \
    errno = saved; \
    return result; \
}
POSITIONAL_WRAPPER(pread, off_t, void *)
POSITIONAL_WRAPPER(pread64, off64_t, void *)
POSITIONAL_WRAPPER(pwrite, off_t, const void *)
POSITIONAL_WRAPPER(pwrite64, off64_t, const void *)
int fdatasync(int fd) {
    int (*next)(int) = required_symbol("fdatasync");
    int trace = target_fd(fd) && observing();
    int result = next(fd);
    int saved = errno;
    if (trace) event("fdatasync", 0, 0, result, result < 0 ? saved : 0);
    errno = saved;
    return result;
}
