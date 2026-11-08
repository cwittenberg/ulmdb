#ifndef ULMDB_ADAPTER_BUILD_SUPPORT_H
#define ULMDB_ADAPTER_BUILD_SUPPORT_H

#include <stddef.h>
#include <stdint.h>

struct build_pin {
    char name[256];
    uint64_t bytes;
    char sha256[65];
};

struct build_source {
    char directory[4096];
    char commit[41];
    struct build_pin files[64];
    size_t count;
};

void build_fail(const char *format, ...) __attribute__((format(printf, 1, 2), noreturn));
void *build_alloc(size_t size);
char *build_format(const char *format, ...) __attribute__((format(printf, 1, 2)));
char *build_read(const char *path);
char *build_absolute(const char *path);
void build_mkdirs(const char *path);
void build_copy(const char *source, const char *target, unsigned mode);
void build_run(char *const arguments[]);
void build_compile(const char *compiler, char *const arguments[]);
void build_hash(const char *path, uint64_t *bytes, char digest[65]);
void build_verify(const struct build_source *source);
void build_modern_source(const char *root, const char *path, struct build_source *source);
void build_legacy_source(const char *here, const char *path, int fetch, struct build_source *source);
void build_current09_source(const char *here, const char *path, int fetch, struct build_source *source);
void build_notices(const char *here, const char *output,
                   const struct build_source *modern, const struct build_source *legacy);

#endif
