#ifndef ULMDB_DARWIN_TOOLS_H
#define ULMDB_DARWIN_TOOLS_H
#if defined(__APPLE__)
#include <errno.h>
#include <fcntl.h>
#include <mach-o/dyld.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static inline char *ulmdb_tool_executable_path(void) {
    uint32_t size = 0;
    (void)_NSGetExecutablePath(NULL, &size);
    if (!size || size > 1048576) { errno = ENAMETOOLONG; return NULL; }
    char *raw = malloc(size);
    if (!raw) return NULL;
    if (_NSGetExecutablePath(raw, &size)) { free(raw); errno = ENAMETOOLONG; return NULL; }
    char *path = realpath(raw, NULL);
    int saved = errno;
    free(raw); errno = saved;
    return path;
}
static inline int ulmdb_tool_fullsync(int fd) {
    int result;
    do { result = fcntl(fd, F_FULLFSYNC); } while (result < 0 && errno == EINTR);
    return result; /* No successful fsync-only fallback. */
}
static inline int ulmdb_tool_dirsync(int fd) {
    int result;
    do { result = fsync(fd); } while (result < 0 && errno == EINTR);
    return result;
}
static inline int ulmdb_tool_pipe_cloexec(int descriptors[2]) {
    if (pipe(descriptors)) return -1;
    for (unsigned i = 0; i < 2; ++i) {
        int flags = fcntl(descriptors[i], F_GETFD);
        if (flags < 0 || fcntl(descriptors[i], F_SETFD, flags | FD_CLOEXEC) < 0) {
            int saved = errno;
            close(descriptors[0]); close(descriptors[1]); errno = saved; return -1;
        }
    }
    return 0;
}
struct ulmdb_tool_parent { char *path, *leaf, *canonical; int fd; };
static inline void ulmdb_tool_close_parent(struct ulmdb_tool_parent *parent) {
    if (parent->fd >= 0) close(parent->fd);
    free(parent->canonical); free(parent->path);
}
static inline int ulmdb_tool_parent_matches(const struct ulmdb_tool_parent *parent) {
    struct stat named, held;
    if (lstat(parent->canonical, &named) || fstat(parent->fd, &held)) return errno;
    return S_ISDIR(named.st_mode) && S_ISDIR(held.st_mode)
        && named.st_dev == held.st_dev && named.st_ino == held.st_ino ? 0 : ESTALE;
}
static inline int ulmdb_tool_open_parent(const char *path, struct ulmdb_tool_parent *parent) {
    memset(parent, 0, sizeof *parent); parent->fd = -1;
    parent->path = strdup(path);
    if (!parent->path) return ENOMEM;
    size_t length = strlen(parent->path);
    while (length > 1 && parent->path[length-1] == '/') parent->path[--length] = 0;
    char *slash = strrchr(parent->path, '/');
    const char *directory = ".";
    parent->leaf = slash ? slash + 1 : parent->path;
    if (!*parent->leaf || !strcmp(parent->leaf, ".") || !strcmp(parent->leaf, "..")) return EINVAL;
    if (slash) {
        if (slash == parent->path) directory = "/";
        else { *slash = 0; directory = parent->path; }
    }
    parent->canonical = realpath(directory, NULL);
    if (!parent->canonical) return errno;
    parent->fd = open(parent->canonical, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (parent->fd < 0) return errno;
    return ulmdb_tool_parent_matches(parent);
}
/* Caller owns stage. Native commits/metadata file writes have already fenced
 * contents. Directory publication never replaces an existing target. Errors
 * after successful rename leave *published true; they never erase the target. */
static inline int ulmdb_tool_publish_exclusive(const char *stage, const char *target, int *published) {
    struct ulmdb_tool_parent source, destination;
    int result = ulmdb_tool_open_parent(stage, &source);
    if (result) { ulmdb_tool_close_parent(&source); return result; }
    result = ulmdb_tool_open_parent(target, &destination);
    if (result) { ulmdb_tool_close_parent(&destination); ulmdb_tool_close_parent(&source); return result; }
    int fd = openat(source.fd, source.leaf, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (fd < 0) { result = errno; goto done; }
    struct stat held, named;
    if (fstat(fd, &held) || fstatat(source.fd, source.leaf, &named, AT_SYMLINK_NOFOLLOW)) {
        result = errno; goto done;
    }
    if (!S_ISDIR(held.st_mode) || !S_ISDIR(named.st_mode)
        || held.st_dev != named.st_dev || held.st_ino != named.st_ino) {
        result = ESTALE; goto done;
    }
    if (ulmdb_tool_dirsync(fd)) { result = errno; goto done; }
    result = ulmdb_tool_parent_matches(&source);
    if (!result) result = ulmdb_tool_parent_matches(&destination);
    if (result) goto done;
    if (renameatx_np(source.fd, source.leaf, destination.fd, destination.leaf, RENAME_EXCL)) {
        result = errno; goto done;
    }
    *published = 1;
    if (fstatat(destination.fd, destination.leaf, &named, AT_SYMLINK_NOFOLLOW)) { result = errno; goto done; }
    if (!S_ISDIR(named.st_mode) || held.st_dev != named.st_dev || held.st_ino != named.st_ino) {
        result = ESTALE; goto done;
    }
    if (lstat(target, &named)) { result = errno; goto done; }
    if (!S_ISDIR(named.st_mode) || held.st_dev != named.st_dev || held.st_ino != named.st_ino) {
        result = ESTALE; goto done;
    }
    if (ulmdb_tool_dirsync(destination.fd) || ulmdb_tool_dirsync(source.fd)) { result = errno; goto done; }
    result = ulmdb_tool_parent_matches(&source);
    if (!result) result = ulmdb_tool_parent_matches(&destination);
done:
    if (fd >= 0) close(fd);
    ulmdb_tool_close_parent(&destination); ulmdb_tool_close_parent(&source);
    return result;
}
#endif
#endif
