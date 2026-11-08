/* Process and filesystem effects for the development-only native packager. */
#include "build_support.h"

#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

void build_fail(const char *format, ...) {
    va_list arguments;
    va_start(arguments, format);
    fprintf(stderr, "Adapter build: ");
    vfprintf(stderr, format, arguments);
    fprintf(stderr, "\n");
    va_end(arguments);
    exit(EXIT_FAILURE);
}

void *build_alloc(size_t size) {
    void *result = calloc(1, size ? size : 1);
    if (!result) build_fail("out of memory");
    return result;
}

char *build_format(const char *format, ...) {
    va_list arguments, measure;
    va_start(arguments, format);
    va_copy(measure, arguments);
    int length = vsnprintf(NULL, 0, format, measure);
    va_end(measure);
    if (length < 0) build_fail("cannot format path");
    char *result = build_alloc((size_t)length + 1);
    vsnprintf(result, (size_t)length + 1, format, arguments);
    va_end(arguments);
    return result;
}

char *build_read(const char *path) {
    FILE *file = fopen(path, "rb");
    if (!file) build_fail("cannot read %s: %s", path, strerror(errno));
    if (fseek(file, 0, SEEK_END)) build_fail("cannot seek %s", path);
    long length = ftell(file);
    if (length < 0 || length > 1048576) build_fail("invalid manifest size: %s", path);
    rewind(file);
    char *result = build_alloc((size_t)length + 1);
    if (fread(result, 1, (size_t)length, file) != (size_t)length || fclose(file))
        build_fail("cannot read complete manifest: %s", path);
    if (memchr(result, 0, (size_t)length)) build_fail("NUL in manifest: %s", path);
    return result;
}

char *build_absolute(const char *path) {
    char *resolved = realpath(path, NULL);
    if (resolved) return resolved;
    if (path[0] == '/') return build_format("%s", path);
    char *directory = getcwd(NULL, 0);
    if (!directory) build_fail("cannot find working directory: %s", strerror(errno));
    char *result = build_format("%s/%s", directory, path);
    free(directory);
    return result;
}

void build_mkdirs(const char *path) {
    char *copy = build_format("%s", path);
    for (char *cursor = copy + 1; ; ++cursor) {
        if (*cursor && *cursor != '/') continue;
        char saved = *cursor;
        *cursor = 0;
        if (mkdir(copy, 0755) && errno != EEXIST)
            build_fail("cannot create %s: %s", copy, strerror(errno));
        struct stat metadata;
        if (stat(copy, &metadata) || !S_ISDIR(metadata.st_mode))
            build_fail("expected directory: %s", copy);
        if (!saved) break;
        *cursor = saved;
    }
    free(copy);
}

void build_copy(const char *source, const char *target, unsigned mode) {
    int input = open(source, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (input < 0) build_fail("cannot open %s: %s", source, strerror(errno));
    char *temporary = build_format("%s.build-%ld", target, (long)getpid());
    int output = open(temporary, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, mode);
    if (output < 0) build_fail("cannot create %s: %s", temporary, strerror(errno));
    unsigned char buffer[65536];
    for (;;) {
        ssize_t count = read(input, buffer, sizeof buffer);
        if (count < 0 && errno == EINTR) continue;
        if (count < 0) build_fail("cannot read %s: %s", source, strerror(errno));
        if (!count) break;
        for (ssize_t done = 0; done < count;) {
            ssize_t written = write(output, buffer + done, (size_t)(count - done));
            if (written < 0 && errno == EINTR) continue;
            if (written <= 0) build_fail("cannot write %s: %s", temporary, strerror(errno));
            done += written;
        }
    }
    if (close(input) || close(output) || rename(temporary, target))
        build_fail("cannot install %s: %s", target, strerror(errno));
    free(temporary);
}

void build_run(char *const arguments[]) {
    pid_t child = fork();
    if (child < 0) build_fail("cannot spawn %s: %s", arguments[0], strerror(errno));
    if (!child) {
        execvp(arguments[0], arguments);
        fprintf(stderr, "Cannot execute %s: %s\n", arguments[0], strerror(errno));
        _exit(127);
    }
    int status;
    while (waitpid(child, &status, 0) < 0) {
        if (errno != EINTR) build_fail("cannot wait for %s: %s", arguments[0], strerror(errno));
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status))
        build_fail("%s failed (%s %d)", arguments[0], WIFEXITED(status) ? "exit" : "signal",
                   WIFEXITED(status) ? WEXITSTATUS(status) : WTERMSIG(status));
}

void build_compile(const char *compiler, char *const arguments[]) {
    /* CC supports quoted words without passing any input to a command shell. */
    char *words = build_format("%s", compiler);
    char *command[256];
    size_t count = 0;
    char *read = words, *write = words;
    while (*read) {
        while (isspace((unsigned char)*read)) ++read;
        if (!*read) break;
        if (count == 128) build_fail("too many compiler command arguments");
        command[count++] = write;
        int quote = 0;
        while (*read && (quote || !isspace((unsigned char)*read))) {
            char next = *read++;
            if (!quote && (next == '\'' || next == '"')) quote = next;
            else if (quote && next == quote) quote = 0;
            else if (next == '\\' && quote != '\'') {
                if (!*read) build_fail("trailing escape in CC");
                *write++ = *read++;
            } else *write++ = next;
        }
        if (quote) build_fail("unclosed quote in CC");
        while (isspace((unsigned char)*read)) ++read;
        *write++ = 0;
    }
    if (!count) build_fail("CC is empty");
    for (size_t index = 0; arguments[index]; ++index) {
        if (count == 255) build_fail("too many compiler arguments");
        command[count++] = arguments[index];
    }
    command[count] = NULL;
    build_run(command);
    free(words);
}
