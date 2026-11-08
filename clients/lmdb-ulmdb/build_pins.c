/* Read the repository-owned source manifests without a scripting runtime. */
#include "build_support.h"

#include <ctype.h>
#include <errno.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static void copy_text(char *target, size_t capacity, const char *start, size_t length) {
    if (length >= capacity) build_fail("source manifest field exceeds its bound");
    memcpy(target, start, length);
    target[length] = 0;
}

static const char *json_field(const char *text, const char *name) {
    char *pattern = build_format("\"%s\"", name);
    const char *field = strstr(text, pattern);
    free(pattern);
    if (!field) build_fail("missing source manifest field: %s", name);
    field = strchr(field, ':');
    if (!field) build_fail("invalid source manifest field: %s", name);
    do { ++field; } while (isspace((unsigned char)*field));
    return field;
}

static const char *json_text(const char *text, const char *name, char *target, size_t capacity) {
    const char *value = json_field(text, name);
    if (*value != '"') build_fail("expected string in source manifest: %s", name);
    const char *end = strchr(++value, '"');
    if (!end || memchr(value, '\\', (size_t)(end - value)))
        build_fail("unsupported escaped source manifest field: %s", name);
    copy_text(target, capacity, value, (size_t)(end - value));
    return end + 1;
}

static void validate_pin(const struct build_pin *pin) {
    if (!pin->name[0] || strchr(pin->name, '/') || !strcmp(pin->name, ".") ||
        !strcmp(pin->name, "..") || !pin->bytes || strlen(pin->sha256) != 64)
        build_fail("invalid pinned file metadata: %s", pin->name);
    for (size_t index = 0; index < 64; ++index)
        if (!isxdigit((unsigned char)pin->sha256[index]))
            build_fail("invalid SHA-256 in source manifest: %s", pin->name);
}

static void source_directory(const char *path, struct build_source *source) {
    char *nested = build_format("%s/libraries/liblmdb", path);
    struct stat metadata;
    const char *chosen = !stat(nested, &metadata) && S_ISDIR(metadata.st_mode) ? nested : path;
    char *absolute = build_absolute(chosen);
    copy_text(source->directory, sizeof source->directory, absolute, strlen(absolute));
    free(absolute);
    free(nested);
}

void build_verify(const struct build_source *source) {
    if (!source->count || strlen(source->commit) != 40)
        build_fail("incomplete source manifest");
    for (size_t index = 0; index < source->count; ++index) {
        const struct build_pin *pin = source->files + index;
        validate_pin(pin);
        char *path = build_format("%s/%s", source->directory, pin->name);
        uint64_t bytes;
        char digest[65];
        build_hash(path, &bytes, digest);
        if (bytes != pin->bytes || strcmp(digest, pin->sha256))
            build_fail("source does not match its pinned size and SHA-256: %s", path);
        free(path);
    }
}

static void json_files(const char *text, struct build_source *source) {
    while ((text = strstr(text, "\"path\""))) {
        if (source->count == 64) build_fail("too many pinned source files");
        struct build_pin *pin = source->files + source->count++;
        const char *after_name = json_text(text, "path", pin->name, sizeof pin->name);
        const char *number = json_field(after_name, "bytes");
        if (sscanf(number, "%" SCNu64, &pin->bytes) != 1)
            build_fail("invalid JSON source byte count");
        text = json_text(after_name, "sha256", pin->sha256, sizeof pin->sha256);
        validate_pin(pin);
    }
}

void build_modern_source(const char *root, const char *path, struct build_source *source) {
    memset(source, 0, sizeof *source);
    source_directory(path, source);
    char *manifest = build_format("%s/clients/lmdb-ulmdb/modern-source.json", root);
    char *text = build_read(manifest);
    json_text(text, "commit", source->commit, sizeof source->commit);
    json_files(text, source);
    free(text);
    free(manifest);
    build_verify(source);
}

static void build_cached_source(const char *here, const char *manifest_name, const char *path,
                                int fetch, struct build_source *source) {
    memset(source, 0, sizeof *source);
    source_directory(path, source);
    char *manifest = build_format("%s/%s", here, manifest_name);
    char *text = build_read(manifest);
    json_text(text, "commit", source->commit, sizeof source->commit);
    json_files(text, source);
    if (fetch) {
        build_mkdirs(source->directory);
        for (size_t index = 0; index < source->count; ++index) {
            const struct build_pin *pin = source->files + index;
            char *file = build_format("%s/%s", source->directory, pin->name);
            struct stat metadata;
            if (lstat(file, &metadata) && errno == ENOENT) {
                char *temporary = build_format("%s.download-%ld", file, (long)getpid());
                char *url = build_format("https://raw.githubusercontent.com/LMDB/lmdb/%s/libraries/liblmdb/%s",
                                         source->commit, pin->name);
                char *arguments[] = {"curl", "--fail", "--silent", "--show-error", "--location",
                                     "--proto", "=https", "--max-time", "60", "--output", temporary, url, NULL};
                build_run(arguments);
                uint64_t bytes;
                char digest[65];
                build_hash(temporary, &bytes, digest);
                if (bytes != pin->bytes || strcmp(digest, pin->sha256)) {
                    unlink(temporary);
                    build_fail("download does not match its source pin: %s", url);
                }
                /* Never overwrite a concurrently populated cache entry. */
                if (link(temporary, file) && errno != EEXIST)
                    build_fail("cannot publish downloaded source: %s", strerror(errno));
                if (unlink(temporary)) build_fail("cannot remove source download temporary");
                free(url);
                free(temporary);
            }
            free(file);
        }
    }
    free(text);
    free(manifest);
    build_verify(source);
}

void build_legacy_source(const char *here, const char *path, int fetch, struct build_source *source) {
    build_cached_source(here, "legacy-source.json", path, fetch, source);
}

void build_current09_source(const char *here, const char *path, int fetch, struct build_source *source) {
    build_cached_source(here, "current09-source.json", path, fetch, source);
}

static char *json_section(const char *text, const char *name) {
    const char *start = json_field(text, name);
    if (*start != '{') build_fail("expected source notice object");
    unsigned depth = 0;
    int quoted = 0;
    const char *end = start;
    do {
        if (!*end) build_fail("unclosed source notice object");
        if (*end == '"' && (end == start || end[-1] != '\\')) quoted = !quoted;
        if (!quoted && *end == '{') ++depth;
        if (!quoted && *end == '}') --depth;
        ++end;
    } while (depth);
    char *result = build_alloc((size_t)(end - start) + 1);
    memcpy(result, start, (size_t)(end - start));
    return result;
}

void build_notices(const char *here, const char *output,
                   const struct build_source *modern, const struct build_source *legacy) {
    char *manifest = build_format("%s/exporter-notices.json", here);
    char *text = build_read(manifest);
    const struct build_source *sources[] = {modern, legacy};
    const char *versions[] = {"1.0.2", "0.9.31"};
    for (size_t index = 0; index < 2; ++index) {
        const struct build_source *source = sources[index];
        char *section = json_section(text, versions[index]);
        struct build_source notice = {0};
        copy_text(notice.directory, sizeof notice.directory, source->directory, strlen(source->directory));
        json_text(section, "commit", notice.commit, sizeof notice.commit);
        if (strcmp(notice.commit, source->commit)) build_fail("source notice commit mismatch");
        json_files(section, &notice);
        build_verify(&notice);
        char *target = build_format("%s/share/licenses/lmdb-%s", output, versions[index]);
        char *retained = build_format("%s/source", target);
        build_mkdirs(retained);
        const char *names[] = {"LICENSE", "COPYRIGHT", "mdb.c", "midl.c", "lmdb.h", "midl.h"};
        for (size_t file = 0; file < 6; ++file) {
            char *from = build_format("%s/%s", source->directory, names[file]);
            char *to = build_format("%s/%s", file < 2 ? target : retained, names[file]);
            build_copy(from, to, 0644);
            free(from);
            free(to);
        }
        char *provenance = build_format("%s/SOURCE.json", target);
        FILE *record = fopen(provenance, "w");
        if (!record) build_fail("cannot write exporter provenance: %s", strerror(errno));
        if (fprintf(record, "{\n  \"repository\": \"https://github.com/LMDB/lmdb.git\",\n"
                            "  \"commit\": \"%s\",\n  \"license\": \"OpenLDAP Public License 2.8\"\n}\n",
                    source->commit) < 0 || fclose(record)) build_fail("cannot finish exporter provenance");
        free(provenance);
        free(target);
        free(retained);
        free(section);
    }
    free(text);
    free(manifest);
}
