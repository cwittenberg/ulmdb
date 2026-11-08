/* Adapter-specific archive contract. Uses only public LMDB C entry points.
 * The outer runner may precreate source using any supported file publication
 * mode. Every DBI is committed before another transaction uses that DBI. */
#define _GNU_SOURCE
#include <lmdb.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define NEED(x) do { if (!(x)) { fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #x); exit(2); } } while (0)
#define OK(x) do { int rc_ = (x); if (rc_) { fprintf(stderr, "%s:%d: %s=%d\n", __FILE__, __LINE__, #x, rc_); exit(2); } } while (0)
#define CAP (32U * 1024U * 1024U)
typedef struct { unsigned char *data; size_t size; } Blob;
typedef struct { size_t start, name, payload, end; uint32_t length; uint64_t size; } Record;
static const char *root;
static unsigned rejected;

static void path(char out[4096], const char *name) {
    int n = snprintf(out, 4096, "%s/%s", root, name);
    NEED(n > 0 && n < 4096);
}
static uint64_t le(const unsigned char *p, unsigned width) {
    uint64_t n = 0;
    for (unsigned i = 0; i < width; ++i) n |= (uint64_t)p[i] << (8 * i);
    return n;
}
static void set(unsigned char *p, uint64_t n, unsigned width) {
    for (unsigned i = 0; i < width; ++i) p[i] = (unsigned char)(n >> (8 * i));
}
static Blob clone(Blob b) {
    Blob out = {malloc(b.size + 128), b.size};
    NEED(out.data != NULL);
    memcpy(out.data, b.data, b.size);
    return out;
}
static Blob load(const char *name) {
    int fd = open(name, O_RDONLY);
    NEED(fd >= 0);
    struct stat info;
    NEED(!fstat(fd, &info) && info.st_size >= 32 && info.st_size <= CAP);
    Blob out = {malloc((size_t)info.st_size), (size_t)info.st_size};
    NEED(out.data != NULL);
    size_t done = 0;
    while (done != out.size) {
        ssize_t n = read(fd, out.data + done, out.size - done);
        NEED(n > 0);
        done += (size_t)n;
    }
    NEED(close(fd) == 0);
    return out;
}
static void store(const char *name, Blob b) {
    int fd = open(name, O_WRONLY | O_CREAT | O_EXCL, 0600);
    NEED(fd >= 0);
    size_t done = 0;
    while (done != b.size) {
        ssize_t n = write(fd, b.data + done, b.size - done);
        NEED(n > 0);
        done += (size_t)n;
    }
    NEED(!fsync(fd) && !close(fd));
}
static Record record(Blob b, size_t start) {
    NEED(start <= b.size && b.size - start >= 12);
    Record out = {.start = start, .name = start + 12};
    out.length = (uint32_t)le(b.data + start, 4);
    out.size = le(b.data + start + 4, 8);
    NEED(out.length <= 128 && out.length <= b.size - out.name);
    out.payload = out.name + out.length;
    NEED(out.size <= b.size - out.payload);
    out.end = out.payload + (size_t)out.size;
    return out;
}
static Record find_record(Blob b, const char *name) {
    uint64_t count = le(b.data + 16, 8);
    NEED(count <= 1024);
    size_t at = 32;
    for (uint64_t i = 0; i < count; ++i) {
        Record r = record(b, at);
        if (r.length == strlen(name) && !memcmp(b.data + r.name, name, r.length)) return r;
        at = r.end;
    }
    NEED(0);
    return (Record){0};
}
static MDB_env *environment(const char *name, unsigned flags) {
    MDB_env *env;
    OK(mdb_env_create(&env));
    OK(mdb_env_set_maxdbs(env, 8));
    OK(mdb_env_open(env, name, MDB_NOTLS | flags, 0600));
    return env;
}
static MDB_val value(const char *s) {
    return (MDB_val){strlen(s), (void *)s};
}
static void no_staging(void) {
    for (unsigned i = 0; i < 2; ++i) {
        char temporary[4096];
        path(temporary, "tmp");
        DIR *directory = opendir(i ? temporary : root);
        NEED(directory != NULL);
        struct dirent *entry;
        while ((entry = readdir(directory))) NEED(strncmp(entry->d_name, ".ulmdb-copy-", 12));
        NEED(!closedir(directory));
    }
}
static void reject(const char *label, Blob b) {
    char filename[4096], leaf[128], native[4096];
    int n = snprintf(leaf, sizeof leaf, "bad-%u-%s.archive", rejected++, label);
    NEED(n > 0 && (size_t)n < sizeof leaf);
    path(filename, leaf);
    n = snprintf(native, sizeof native, "%s.ulmdb", filename);
    NEED(n > 0 && (size_t)n < sizeof native);
    store(filename, b);
    MDB_env *env;
    OK(mdb_env_create(&env));
    OK(mdb_env_set_maxdbs(env, 8));
    int rc = mdb_env_open(env, filename, MDB_NOTLS | MDB_NOSUBDIR | MDB_RDONLY, 0600);
    mdb_env_close(env);
    NEED(rc == MDB_INVALID || rc == MDB_CORRUPTED);
    struct stat info;
    errno = 0;
    NEED(lstat(native, &info) < 0 && errno == ENOENT);
    Blob after = load(filename);
    NEED(after.size == b.size && !memcmp(after.data, b.data, b.size));
    free(after.data);
    no_staging();
}
static void malformed(Blob original, const char *kind) {
    Record current = find_record(original, "CURRENT");
    NEED(current.size == 64 || current.size == 144);
    for (unsigned i = 0; i < 16; ++i) {
        Blob b = clone(original);
        const char *label = kind;
        switch (i) {
            case 0: set(b.data + 8, 0, 4); break;
            case 1: set(b.data + 8, 3, 4); break;
            case 2: b.data[12] = 1; break;
            case 3: set(b.data + 16, 0, 8); break;
            case 4: set(b.data + 16, UINT64_MAX, 8); break;
            case 5: set(b.data + 24, 0, 8); break;
            case 6: set(b.data + 24, UINT64_MAX, 8); break;
            case 7: set(b.data + current.start, 0, 4); break;
            case 8: set(b.data + current.start, 129, 4); break;
            case 9: set(b.data + current.start + 4, 63, 8); break;
            case 10: b.data[current.name] = '/'; break;
            case 11: b.data[current.name] = '\\'; break;
            case 12: b.data[current.name] = '\0'; break;
            case 13: b.data[current.payload + current.size - 1] ^= 1; break;
            case 14: --b.size; break;
            case 15: b.data[b.size++] = 1; break;
        }
        reject(label, b);
        free(b.data);
    }
    Blob b = clone(original);
    size_t length = current.end - current.start;
    b.data = realloc(b.data, b.size + length);
    NEED(b.data != NULL);
    memcpy(b.data + b.size, original.data + current.start, length);
    b.size += length;
    set(b.data + 16, le(original.data + 16, 8) + 1, 8);
    set(b.data + 24, le(original.data + 24, 8) + current.size, 8);
    reject("duplicate-current", b);
    free(b.data);
}
static void view(const char *archive, unsigned flags, size_t id, const char *expected) {
    MDB_env *env = environment(archive, MDB_NOSUBDIR | MDB_RDONLY | flags);
    MDB_txn *txn;
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn));
    NEED(mdb_txn_id(txn) == id);
    MDB_dbi db;
    int rc = mdb_dbi_open(txn, "data", 0, &db);
    if (expected) {
        NEED(rc == 0);
        MDB_val key = value("key"), out;
        OK(mdb_get(txn, db, &key, &out));
        NEED(out.mv_size == strlen(expected) && !memcmp(out.mv_data, expected, out.mv_size));
    } else {
        NEED(rc == MDB_NOTFOUND);
        MDB_stat stat;
        OK(mdb_stat(txn, 1, &stat));
        NEED(stat.ms_entries == 0);
    }
    mdb_txn_abort(txn);
    mdb_env_close(env);
}
int main(int argc, char **argv) {
    NEED(argc == 2);
    root = argv[1];
    char source_path[4096], temporary[4096], raw[4096], compact[4096];
    path(source_path, "source");
    path(temporary, "tmp");
    NEED(!mkdir(temporary, 0700));
    NEED(!setenv("TMPDIR", temporary, 1));
    MDB_env *source = environment(source_path, 0);
    MDB_txn *writer;
    MDB_dbi db;
    MDB_val key = value("key"), a = value("A"), b = value("B"), private_value = value("PRIVATE");
    OK(mdb_txn_begin(source, NULL, 0, &writer));
    OK(mdb_dbi_open(writer, "data", MDB_CREATE, &db));
    OK(mdb_put(writer, db, &key, &a, 0));
    OK(mdb_txn_commit(writer));
    MDB_txn *old;
    MDB_val old_value;
    OK(mdb_txn_begin(source, NULL, MDB_RDONLY, &old));
    OK(mdb_get(old, db, &key, &old_value));
    NEED(mdb_txn_id(old) == 1 && old_value.mv_size == 1 && *(char *)old_value.mv_data == 'A');
    OK(mdb_txn_begin(source, NULL, 0, &writer));
    OK(mdb_put(writer, db, &key, &b, 0));
    OK(mdb_txn_commit(writer));
    OK(mdb_txn_begin(source, NULL, 0, &writer));
    OK(mdb_put(writer, db, &key, &private_value, 0));
    path(raw, "raw.archive");
    path(compact, "compact.archive");
    const char *archives[] = {raw, compact};
    Blob originals[2];
    for (unsigned i = 0; i < 2; ++i) {
        int fd = open(archives[i], O_RDWR | O_CREAT | O_EXCL, 0600);
        NEED(fd >= 0);
        if (i) OK(mdb_env_copyfd2(source, fd, MDB_CP_COMPACT));
        else OK(mdb_env_copyfd(source, fd));
        NEED(fcntl(fd, F_GETFD) >= 0 && lseek(fd, 0, SEEK_CUR) > 32);
        NEED(!close(fd));
        originals[i] = load(archives[i]);
        NEED(!memcmp(originals[i].data, "ULMDBCP\0", 8));
        NEED(le(originals[i].data + 8, 4) == 2);
        no_staging();
    }
    NEED(mdb_env_copyfd2(source, -1, 0) == EBADF);
    int fd = open("/dev/full", O_WRONLY);
    NEED(fd >= 0);
    NEED(mdb_env_copyfd2(source, fd, 0) == ENOSPC);
    NEED(fcntl(fd, F_GETFD) >= 0 && !close(fd));
    char invalid_flags[4096];
    path(invalid_flags, "invalid-flags");
    fd = open(invalid_flags, O_RDWR | O_CREAT | O_EXCL, 0600);
    NEED(fd >= 0);
    NEED(mdb_env_copyfd2(source, fd, 2) == EINVAL);
    NEED(lseek(fd, 0, SEEK_END) == 0 && !close(fd));
    no_staging();
    mdb_txn_abort(writer);
    MDB_val c = value("C");
    OK(mdb_txn_begin(source, NULL, 0, &writer));
    OK(mdb_put(writer, db, &key, &c, 0));
    OK(mdb_txn_commit(writer));
    view(raw, 0, 2, "B");
    view(raw, MDB_PREVSNAPSHOT, 1, "A");
    view(compact, 0, 1, "B");
    view(compact, MDB_PREVSNAPSHOT, 0, NULL);
    malformed(originals[0], "raw");
    malformed(originals[1], "compact");
    /* Updating the restored copy changes only its native sidecar. */
    MDB_env *independent = environment(raw, MDB_NOSUBDIR);
    MDB_dbi copied_db;
    MDB_val x = value("X");
    OK(mdb_txn_begin(independent, NULL, 0, &writer));
    OK(mdb_dbi_open(writer, "data", 0, &copied_db));
    OK(mdb_put(writer, copied_db, &key, &x, 0));
    OK(mdb_txn_commit(writer));
    mdb_env_close(independent);
    view(raw, 0, 3, "X");
    view(raw, MDB_PREVSNAPSHOT, 2, "B");
    view(compact, 0, 1, "B");
    NEED(old_value.mv_size == 1 && *(char *)old_value.mv_data == 'A');
    MDB_val again;
    OK(mdb_get(old, db, &key, &again));
    NEED(again.mv_size == 1 && *(char *)again.mv_data == 'A');
    mdb_txn_abort(old);
    MDB_txn *latest;
    OK(mdb_txn_begin(source, NULL, MDB_RDONLY, &latest));
    OK(mdb_get(latest, db, &key, &again));
    NEED(mdb_txn_id(latest) == 3 && again.mv_size == 1 && *(char *)again.mv_data == 'C');
    mdb_txn_abort(latest);
    for (unsigned i = 0; i < 2; ++i) {
        Blob after = load(archives[i]);
        NEED(after.size == originals[i].size && !memcmp(after.data, originals[i].data, after.size));
        free(after.data);
        free(originals[i].data);
    }
    mdb_env_close(source);
    no_staging();
    printf("copy-archives-ok rejected=%u\n", rejected);
    return 0;
}
