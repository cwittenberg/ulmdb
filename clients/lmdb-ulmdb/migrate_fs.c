#ifdef _WIN32
#include "migrate_fs_windows.c"
#else
/* Private staging, provenance and no-replace publication have one owner. */
#include "migrate_fs.h"
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#if defined(__APPLE__)
#include "darwin_tools.h"
#define MIGRATION_FILE_SYNC(fd) ulmdb_tool_fullsync(fd)
#else
#define MIGRATION_FILE_SYNC(fd) fsync(fd)
#endif

int migration_error(char *output, size_t size, const char *format, ...) {
    va_list arguments;
    va_start(arguments, format);
    vsnprintf(output, size, format, arguments);
    va_end(arguments);
    return -1;
}
char *migration_join(const char *parent, const char *name) {
    size_t left = strlen(parent), right = strlen(name);
    if (left > SIZE_MAX - right - 2) { errno = ENOMEM; return NULL; }
    char *result = malloc(left + right + 2);
    if (result) snprintf(result, left + right + 2, "%s/%s", parent, name);
    return result;
}
static uint32_t word(const unsigned char *bytes, int big_endian) {
    uint32_t value = 0;
    for (unsigned i = 0; i < 4; ++i)
        value |= (uint32_t)bytes[big_endian ? 3 - i : i] << (8 * i);
    return value;
}
int migration_source_abi(const char *source, const char **abi, int *single_file,
                         char *error, size_t size) {
    struct stat state;
    if (stat(source, &state)) return migration_error(error, size, "LMDB source: %s", strerror(errno));
    *single_file = S_ISREG(state.st_mode);
    if (!*single_file && !S_ISDIR(state.st_mode))
        return migration_error(error, size, "LMDB source must be a regular file or directory");
    char *datafile = *single_file ? strdup(source) : migration_join(source, "data.mdb");
    if (!datafile) return migration_error(error, size, "allocate LMDB source path: %s", strerror(errno));
    int descriptor = open(datafile, O_RDONLY | O_CLOEXEC);
    free(datafile);
    if (descriptor < 0) return migration_error(error, size, "open LMDB data file: %s", strerror(errno));
    unsigned char header[64];
    size_t length = 0;
    while (length < sizeof header) {
        ssize_t count = read(descriptor, header + length, sizeof header - length);
        if (count < 0 && errno == EINTR) continue;
        if (count < 0) {
            int saved = errno;
            close(descriptor);
            return migration_error(error, size, "read LMDB data file: %s", strerror(saved));
        }
        if (!count) break;
        length += (size_t)count;
    }
    close(descriptor);
    /* LMDB's native page header is 12/16 bytes in 0.9, 20/24 in 1.0. */
    static const size_t offsets[] = {24, 20, 16, 12};
    for (size_t i = 0; i < sizeof offsets / sizeof offsets[0]; ++i) {
        size_t offset = offsets[i];
        if (length < offset + 8) continue;
        for (int big = 0; big <= 1; ++big) {
            if (word(header + offset, big) != UINT32_C(0xBEEFC0DE)) continue;
            uint32_t version = word(header + offset + 4, big);
            if (version == 1 || version == 3) {
                *abi = version == 1 ? "0.9" : "1.0";
                return 0;
            }
            return migration_error(error, size, "Unsupported LMDB data format version %" PRIu32, version);
        }
    }
    return migration_error(error, size, "LMDB metadata header is invalid");
}
int migration_destination(const char *requested, char **destination, char **stage,
                           char *error, size_t size) {
    struct stat state;
    if (!lstat(requested, &state))
        return migration_error(error, size, "Migration destination already exists: %s", requested);
    if (errno != ENOENT) return migration_error(error, size, "inspect destination: %s", strerror(errno));
    char *copy = strdup(requested);
    if (!copy) return migration_error(error, size, "allocate destination: %s", strerror(errno));
    size_t length = strlen(copy);
    while (length > 1 && copy[length - 1] == '/') copy[--length] = 0;
    char *last = strrchr(copy, '/');
    const char *base = last ? last + 1 : copy;
    char *parent;
    if (!last) parent = realpath(".", NULL);
    else if (last == copy) parent = realpath("/", NULL);
    else { *last = 0; parent = realpath(copy, NULL); }
    if (!parent || !*base || !strcmp(base, ".") || !strcmp(base, "..")) {
        free(parent); free(copy);
        return migration_error(error, size, "Destination parent must exist and destination must be a new directory");
    }
    *destination = migration_join(parent, base);
    size_t capacity = strlen(base) + 24;
    char *template = malloc(capacity);
    if (template) snprintf(template, capacity, ".%s.ulmdb-import-XXXXXX", base);
    *stage = template ? migration_join(parent, template) : NULL;
    free(template); free(parent); free(copy);
    if (!*destination || !*stage)
        return migration_error(error, size, "allocate private staging path: %s", strerror(errno));
    if (!mkdtemp(*stage)) {
        int saved = errno;
        free(*stage); *stage = NULL;
        return migration_error(error, size, "create private staging directory: %s", strerror(saved));
    }
    return 0;
}
/* Preserve valid UTF-8 and encode arbitrary Unix filename bytes with surrogate
   escapes, matching Python's filesystem representation in existing provenance. */
static size_t utf8_width(const unsigned char *p) {
    unsigned first = p[0];
    size_t width = first >= 0xc2 && first <= 0xdf ? 2 :
                   first >= 0xe0 && first <= 0xef ? 3 :
                   first >= 0xf0 && first <= 0xf4 ? 4 : 0;
    if (!width) return 0;
    for (size_t i = 1; i < width; ++i)
        if (!p[i] || p[i] < 0x80 || p[i] > 0xbf) return 0;
    if ((first == 0xe0 && p[1] < 0xa0) || (first == 0xed && p[1] >= 0xa0) ||
        (first == 0xf0 && p[1] < 0x90) || (first == 0xf4 && p[1] >= 0x90)) return 0;
    return width;
}
static void json_path(FILE *output, const char *path) {
    fputc('"', output);
    const unsigned char *cursor = (const unsigned char *)path;
    while (*cursor) {
        unsigned c = *cursor;
        if (c == '"' || c == '\\') { fputc('\\', output); fputc((int)c, output); }
        else if (c < 0x20) fprintf(output, "\\u%04x", c);
        else if (c >= 0x80) {
            size_t width = utf8_width(cursor);
            if (width) { fwrite(cursor, 1, width, output); cursor += width; continue; }
            fprintf(output, "\\udc%02x", c);
        } else fputc((int)c, output);
        ++cursor;
    }
    fputc('"', output);
}
static FILE *metadata_file(const char *stage, const char *name, char *error, size_t size) {
    char *path = migration_join(stage, name);
    if (!path) { migration_error(error, size, "allocate metadata path: %s", strerror(errno)); return NULL; }
    int descriptor = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
    free(path);
    if (descriptor < 0) { migration_error(error, size, "create migration metadata: %s", strerror(errno)); return NULL; }
    FILE *output = fdopen(descriptor, "w");
    if (!output) { int saved = errno; close(descriptor); migration_error(error, size, "open migration metadata: %s", strerror(saved)); }
    return output;
}
static int finish_metadata(FILE *output, char *error, size_t size) {
    int failed = ferror(output) || fflush(output) || MIGRATION_FILE_SYNC(fileno(output));
    int saved = errno;
    if (fclose(output) && !failed) { failed = 1; saved = errno; }
    return failed ? migration_error(error, size, "write migration metadata: %s", strerror(saved)) : 0;
}
int migration_metadata(const char *stage, const char *source, const char *abi,
                        const struct migration_info *info, char *error, size_t size) {
    char *reader_configuration = migration_join(stage, "LMDB-READERS");
    if (!reader_configuration) return migration_error(error, size, "allocate reader configuration path: %s", strerror(errno));
    int removed = unlink(reader_configuration), saved = errno;
    free(reader_configuration);
    if (removed && saved != ENOENT) return migration_error(error, size, "remove private reader configuration: %s", strerror(saved));
    FILE *output = metadata_file(stage, "LMDB_MIGRATION.json", error, size);
    if (!output) return -1;
    fputs("{\n  \"schema\": 1,\n  \"source\": ", output);
    json_path(output, source);
    fprintf(output, ",\n  \"source_sequence\": %" PRIu64 ",\n  \"max_readers\": %" PRIu32
            ",\n  \"map_size\": %" PRIu64 ",\n  \"databases\": %" PRIu64
            ",\n  \"records\": %" PRIu64 ",\n  \"abi\": \"%s\"\n}\n",
            info->source_sequence, info->max_readers, info->map_size,
            info->databases, info->records, abi);
    if (finish_metadata(output, error, size)) return -1;
    output = metadata_file(stage, "LMDB-ABI", error, size);
    if (!output) return -1;
    fprintf(output, "%s\n", abi);
    return finish_metadata(output, error, size);
}
int migration_publish(const char *stage, const char *destination, int *published,
                       char *error, size_t size) {
#if defined(__APPLE__)
    int result = ulmdb_tool_publish_exclusive(stage, destination, published);
    return result ? migration_error(error, size, "publish migration destination: %s", strerror(result)) : 0;
#else
    int descriptor = open(stage, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
    if (descriptor < 0) return migration_error(error, size, "open staging directory: %s", strerror(errno));
    int result = fsync(descriptor), saved = errno;
    close(descriptor);
    if (result) return migration_error(error, size, "sync staging directory: %s", strerror(saved));
    if (renameat2(AT_FDCWD, stage, AT_FDCWD, destination, RENAME_NOREPLACE))
        return migration_error(error, size, "publish migration destination: %s", strerror(errno));
    *published = 1;
    char *parent = strdup(destination);
    if (!parent) return migration_error(error, size, "allocate destination parent: %s", strerror(errno));
    char *last = strrchr(parent, '/');
    if (last == parent) last[1] = 0;
    else if (last) *last = 0;
    descriptor = open(parent, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    free(parent);
    if (descriptor < 0) return migration_error(error, size, "open destination parent: %s", strerror(errno));
    result = fsync(descriptor); saved = errno;
    close(descriptor);
    return result ? migration_error(error, size, "sync destination parent: %s", strerror(saved)) : 0;
#endif
}
static int remove_contents(int descriptor) {
    DIR *directory = fdopendir(descriptor);
    if (!directory) { close(descriptor); return -1; }
    int result = 0, saved = 0;
    struct dirent *entry;
    for (;;) {
        errno = 0;
        entry = readdir(directory);
        if (!entry) { if (errno) { result = -1; saved = errno; } break; }
        if (!strcmp(entry->d_name, ".") || !strcmp(entry->d_name, "..")) continue;
        struct stat state;
        if (fstatat(dirfd(directory), entry->d_name, &state, AT_SYMLINK_NOFOLLOW)) { result = -1; saved = errno; break; }
        if (S_ISDIR(state.st_mode)) {
            int child = openat(dirfd(directory), entry->d_name, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
            if (child < 0 || remove_contents(child) || unlinkat(dirfd(directory), entry->d_name, AT_REMOVEDIR)) {
                result = -1; saved = errno; break;
            }
        } else if (unlinkat(dirfd(directory), entry->d_name, 0)) { result = -1; saved = errno; break; }
    }
    if (closedir(directory) && !result) { result = -1; saved = errno; }
    errno = saved;
    return result;
}
int migration_remove_stage(const char *stage, char *error, size_t size) {
    int descriptor = open(stage, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
    if (descriptor < 0) {
        if (errno == ENOENT) return 0;
        return migration_error(error, size, "open owned staging directory for cleanup: %s", strerror(errno));
    }
    if (remove_contents(descriptor) || rmdir(stage))
        return migration_error(error, size, "remove owned staging directory: %s", strerror(errno));
    return 0;
}

#endif
