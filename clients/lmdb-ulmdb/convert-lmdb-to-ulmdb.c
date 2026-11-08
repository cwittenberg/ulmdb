#ifdef _WIN32
#include "launcher_windows.c"
#else
/* Select the native ULMDB implementation for an unchanged dynamic LMDB app. */
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#if defined(__APPLE__)
#include "darwin_tools.h"
#define TOOL_LIBRARY_PATH "DYLD_LIBRARY_PATH"
#else
#define TOOL_LIBRARY_PATH "LD_LIBRARY_PATH"
#endif

static void fail(const char *operation) {
    fprintf(stderr, "convert-lmdb-to-ulmdb: %s: %s\n", operation, strerror(errno));
    exit(1);
}

static char *format_path(const char *directory, const char *tail) {
    size_t length = strlen(directory) + strlen(tail) + 1;
    char *result = malloc(length);
    if (!result) fail("allocate path");
    snprintf(result, length, "%s%s", directory, tail);
    return result;
}

static char *executable_directory(void) {
#if defined(__APPLE__)
    char *path = ulmdb_tool_executable_path();
    if (!path) fail("locate executable");
    char *separator = strrchr(path, '/');
    if (!separator) { errno = EINVAL; fail("resolve executable directory"); }
    if (separator == path) separator[1] = 0; else *separator = 0;
    return path;
#else
    size_t capacity = 256;
    for (;;) {
        char *path = malloc(capacity + 1);
        if (!path) fail("allocate executable path");
        ssize_t length = readlink("/proc/self/exe", path, capacity);
        if (length < 0) fail("locate executable");
        if ((size_t)length < capacity) {
            path[length] = 0;
            char *separator = strrchr(path, '/');
            if (!separator) { errno = EINVAL; fail("resolve executable directory"); }
            *separator = 0;
            return path;
        }
        free(path);
        if (capacity > 1048576) { errno = ENAMETOOLONG; fail("resolve executable path"); }
        capacity *= 2;
    }
#endif
}

int main(int count, char **arguments) {
    int application = 1;
    const char *abi = NULL;
    if (count > 1 && !strcmp(arguments[1], "--abi")) {
        if (count < 4 || (strcmp(arguments[2], "0.9.31") &&
            strcmp(arguments[2], "0.9.36") && strcmp(arguments[2], "1.0.2"))) {
            fprintf(stderr, "convert-lmdb-to-ulmdb: --abi requires 0.9.31, 0.9.36 or 1.0.2 and an application\n");
            return 2;
        }
        abi = arguments[2];
        application = 3;
    }
    if (count <= application) {
        fprintf(stderr, "usage: convert-lmdb-to-ulmdb APPLICATION [ARGUMENT ...]\n");
        fprintf(stderr, "       convert-lmdb-to-ulmdb --abi 0.9.31|0.9.36|1.0.2 APPLICATION [ARGUMENT ...]\n");
        return 2;
    }
    if (abi && setenv("ULMDB_LMDB_ABI", abi, 1)) fail("set ABI profile");
    char *binary = executable_directory();
    char *library = format_path(binary, "/../lib");
    const char *previous = getenv(TOOL_LIBRARY_PATH);
    char *search = library;
    if (previous && previous[0]) {
        size_t length = strlen(library) + strlen(previous) + 2;
        search = malloc(length);
        if (!search) fail("allocate library search path");
        snprintf(search, length, "%s:%s", library, previous);
    }
    if (setenv(TOOL_LIBRARY_PATH, search, 1)) fail("set library search path");
    const char *configured_migrator = getenv("ULMDB_MIGRATOR");
    if (!configured_migrator || !configured_migrator[0]) {
        char *migrator = format_path(binary, "/ulmdb-migrate");
        if (setenv("ULMDB_MIGRATOR", migrator, 1)) fail("set migration helper");
        free(migrator);
    }
    if (search != library) free(search);
    free(library);
    free(binary);
    execvp(arguments[application], arguments + application);
    int number = errno;
    fprintf(stderr, "convert-lmdb-to-ulmdb: %s: %s\n", arguments[application], strerror(number));
    return number == ENOENT ? 127 : 126;
}

#endif
