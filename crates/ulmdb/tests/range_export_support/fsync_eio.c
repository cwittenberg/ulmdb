#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
static unsigned matched;
int fsync(int fd) {
    int (*real_fsync)(int) = dlsym(RTLD_NEXT, "fsync");
    char proc[64], path[PATH_MAX];
    snprintf(proc, sizeof(proc), "/proc/self/fd/%d", fd);
    ssize_t n = readlink(proc, path, sizeof(path)-1);
    if (n >= 0) path[n] = 0;
    const char *target = getenv("RANGE_EXPORT_EIO_TARGET");
    const char *kind = getenv("RANGE_EXPORT_EIO_KIND");
    int match = 0;
    if (target && kind && n >= 0) {
        size_t t = strlen(target);
        if (!strcmp(kind, "directory")) match = !strcmp(path, target);
        if (!strncmp(path, target, t) && path[t] == '/') {
            if (!strcmp(kind, "data")) match = !strcmp(path+t+1, "RANGE.pack");
            if (!strcmp(kind, "marker")) match = strstr(path+t+1, "-BACKEND.tmp") != NULL;
        }
    }
    if (match) {
        unsigned ordinal = ++matched;
        unsigned selected = !strcmp(kind, "directory") ? 2 : 1;
        if (ordinal == selected) {
            const char *log = getenv("RANGE_EXPORT_EIO_LOG");
            if (log) {
                int out = open(log, O_WRONLY|O_APPEND|O_CREAT, 0600);
                if (out >= 0) { dprintf(out, "fsync fd=%d path=%s matched_ordinal=%u errno=5\n", fd, path, ordinal); close(out); }
            }
            errno = EIO;
            return -1;
        }
    }
    return real_fsync(fd);
}
