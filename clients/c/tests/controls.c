/* Adapter controls and capacity admission through the public LMDB API. */
#include <lmdb.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define REQUIRE(condition) do { if (!(condition)) { \
    fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #condition); exit(1); \
} } while (0)
#define OK(expression) do { int code_ = (expression); if (code_ != 0) { \
    fprintf(stderr, "%s:%d: %s returned %d (%s)\n", __FILE__, __LINE__, \
            #expression, code_, mdb_strerror(code_)); exit(1); \
} } while (0)

static unsigned assertions;
static void assertion_callback(MDB_env *env, const char *message) {
    (void)env;
    (void)message;
    ++assertions;
}
static MDB_val text(const char *bytes) {
    MDB_val result = {strlen(bytes), (void *)bytes};
    return result;
}
static void expect_bytes(MDB_val actual, const void *expected, size_t size) {
    REQUIRE(actual.mv_size == size);
    REQUIRE(size == 0 || memcmp(actual.mv_data, expected, size) == 0);
}
static MDB_env *open_env(const char *path, unsigned flags, size_t mapsize) {
    MDB_env *env;
    OK(mdb_env_create(&env));
    OK(mdb_env_set_maxdbs(env, 8));
    OK(mdb_env_set_mapsize(env, mapsize));
    OK(mdb_env_set_assert(env, assertion_callback));
    OK(mdb_env_open(env, path, flags | MDB_NOTLS, 0600));
    return env;
}
static void mutable_flags(MDB_env *env) {
    const unsigned mutable = MDB_NOSYNC | MDB_NOMETASYNC | MDB_MAPASYNC | MDB_NOMEMINIT;
    unsigned original, reported;
    OK(mdb_env_get_flags(env, &original));
    OK(mdb_env_set_flags(env, mutable, 1));
    OK(mdb_env_get_flags(env, &reported));
    REQUIRE(reported == (original | mutable));
    OK(mdb_env_set_flags(env, MDB_NOSYNC | MDB_MAPASYNC, 0));
    OK(mdb_env_get_flags(env, &reported));
    REQUIRE(reported == ((original | mutable) & ~(MDB_NOSYNC | MDB_MAPASYNC)));
    REQUIRE(mdb_env_set_flags(env, MDB_RDONLY, 1) == EINVAL);
    unsigned unchanged;
    OK(mdb_env_get_flags(env, &unchanged));
    REQUIRE(unchanged == reported);
    OK(mdb_env_set_flags(env, mutable, 0));
    OK(mdb_env_get_flags(env, &reported));
    REQUIRE(reported == (original & ~mutable));
}
static void map_capacity(MDB_env *env) {
    MDB_txn *write, *read;
    MDB_dbi data;
    MDB_val key = text("seed"), value = text("kept");
    MDB_envinfo info;
    OK(mdb_txn_begin(env, NULL, 0, &write));
    OK(mdb_dbi_open(write, "capacity", MDB_CREATE, &data));
    OK(mdb_put(write, data, &key, &value, 0));
    OK(mdb_txn_commit(write));
    OK(mdb_env_info(env, &info));
    REQUIRE(info.me_mapsize == 32768);

    unsigned char payload[4096];
    memset(payload, 0x5a, sizeof payload);
    unsigned failed = 100;
    OK(mdb_txn_begin(env, NULL, 0, &write));
    for (unsigned index = 0; index < 100; ++index) {
        char bytes[32];
        int length = snprintf(bytes, sizeof bytes, "bulk-%03u", index);
        REQUIRE(length > 0 && (size_t)length < sizeof bytes);
        key = (MDB_val){(size_t)length, bytes};
        value = (MDB_val){sizeof payload, payload};
        int result = mdb_put(write, data, &key, &value, 0);
        if (result != 0) {
            REQUIRE(result == MDB_MAP_FULL);
            failed = index;
            break;
        }
    }
    REQUIRE(failed < 100);
    mdb_txn_abort(write);

    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    key = text("seed");
    OK(mdb_get(read, data, &key, &value));
    expect_bytes(value, "kept", 4);
    for (unsigned index = 0; index <= failed; ++index) {
        char bytes[32];
        int length = snprintf(bytes, sizeof bytes, "bulk-%03u", index);
        key = (MDB_val){(size_t)length, bytes};
        REQUIRE(mdb_get(read, data, &key, &value) == MDB_NOTFOUND);
    }
    mdb_txn_abort(read);

    OK(mdb_env_set_mapsize(env, 2u * 1024u * 1024u));
    OK(mdb_env_info(env, &info));
    REQUIRE(info.me_mapsize == 2u * 1024u * 1024u);
    OK(mdb_txn_begin(env, NULL, 0, &write));
    for (unsigned index = 0; index < 20; ++index) {
        char bytes[32];
        int length = snprintf(bytes, sizeof bytes, "bulk-%03u", index);
        key = (MDB_val){(size_t)length, bytes};
        payload[0] = (unsigned char)index;
        value = (MDB_val){sizeof payload, payload};
        OK(mdb_put(write, data, &key, &value, 0));
    }
    OK(mdb_txn_commit(write));
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    for (unsigned index = 0; index < 20; ++index) {
        char bytes[32];
        int length = snprintf(bytes, sizeof bytes, "bulk-%03u", index);
        key = (MDB_val){(size_t)length, bytes};
        OK(mdb_get(read, data, &key, &value));
        payload[0] = (unsigned char)index;
        expect_bytes(value, payload, sizeof payload);
    }
    mdb_txn_abort(read);
}
static size_t marker(const char *path, unsigned char output[64]) {
    int fd = open(path, O_RDONLY);
    REQUIRE(fd >= 0);
    ssize_t size = read(fd, output, 64);
    REQUIRE(size > 0 && size <= 64);
    REQUIRE(close(fd) == 0);
    return (size_t)size;
}
static void nosubdir_reopen(const char *path) {
    MDB_env *env = open_env(path, MDB_NOSUBDIR, 1024u * 1024u);
    MDB_txn *write, *read;
    MDB_dbi data;
    MDB_val key = text("key"), value = text("persisted");
    struct stat metadata;
    REQUIRE(stat(path, &metadata) == 0 && S_ISREG(metadata.st_mode));
    REQUIRE((metadata.st_mode & 0077) == 0);
    unsigned char before[64], after[64];
    size_t before_size = marker(path, before);
    OK(mdb_txn_begin(env, NULL, 0, &write));
    OK(mdb_dbi_open(write, "data", MDB_CREATE, &data));
    OK(mdb_put(write, data, &key, &value, 0));
    OK(mdb_txn_commit(write));
    const char *reported;
    OK(mdb_env_get_path(env, &reported));
    REQUIRE(strcmp(reported, path) == 0);
    unsigned flags;
    OK(mdb_env_get_flags(env, &flags));
    REQUIRE((flags & MDB_NOSUBDIR) != 0);
    mdb_env_close(env);

    env = open_env(path, MDB_NOSUBDIR | MDB_RDONLY, 1024u * 1024u);
    REQUIRE(marker(path, after) == before_size);
    REQUIRE(memcmp(before, after, before_size) == 0);
    REQUIRE(mdb_txn_begin(env, NULL, 0, &write) != 0);
    OK(mdb_txn_begin(env, NULL, MDB_RDONLY, &read));
    OK(mdb_dbi_open(read, "data", 0, &data));
    OK(mdb_get(read, data, &key, &value));
    expect_bytes(value, "persisted", 9);
    mdb_txn_abort(read);
    mdb_env_close(env);
}
static void relative_path_survives_working_directory_change(const char *fixture) {
    char original[4096];
    REQUIRE(getcwd(original,sizeof original)!=NULL);
    char *parent=malloc(strlen(fixture)+1);
    REQUIRE(parent!=NULL);
    strcpy(parent,fixture);
    char *slash=strrchr(parent,'/');
    REQUIRE(slash!=NULL);
    *slash=0;
    REQUIRE(chdir(parent)==0);
    MDB_env *env=open_env("relative-storage",0,1024u*1024u);
    REQUIRE(chdir("/")==0);
    MDB_txn *write,*read,*child,*grandchild;
    MDB_dbi main_db;
    OK(mdb_txn_begin(env,NULL,0,&write));
    size_t id=mdb_txn_id(write);
    OK(mdb_txn_begin(env,write,0,&child));
    OK(mdb_txn_begin(env,child,0,&grandchild));
    REQUIRE(mdb_txn_id(child)==id && mdb_txn_id(grandchild)==id);
    mdb_txn_abort(grandchild);
    mdb_txn_abort(child);
    OK(mdb_dbi_open(write,NULL,0,&main_db));
    MDB_val key=text("key"),value=text("value");
    OK(mdb_put(write,main_db,&key,&value,0));
    OK(mdb_txn_commit(write));
    mdb_dbi_close(env,main_db); /* Closing the main handle is a no-op. */
    OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&read));
    OK(mdb_get(read,main_db,&key,&value));
    expect_bytes(value,"value",5);
    id=mdb_txn_id(read);
    mdb_txn_reset(read);
    REQUIRE(mdb_txn_id(read)==id);
    mdb_txn_abort(read);
    const char *reported;
    OK(mdb_env_get_path(env,&reported));
    REQUIRE(strcmp(reported,"relative-storage")==0);
    mdb_env_close(env);
    REQUIRE(chdir(original)==0);
    free(parent);
}
int main(int argc, char **argv) {
    REQUIRE(argc == 3);
    MDB_env *env = open_env(argv[1], 0, 32768);
    mutable_flags(env);
    mdb_filehandle_t fd;
    struct stat metadata;
    OK(mdb_env_get_fd(env, &fd));
    REQUIRE(fcntl(fd, F_GETFD) >= 0);
    REQUIRE(fstat(fd, &metadata) == 0 && S_ISREG(metadata.st_mode));
    map_capacity(env);
    mdb_env_close(env);
    REQUIRE(fcntl(fd, F_GETFD) == -1 && errno == EBADF);

    env = open_env(argv[1], MDB_WRITEMAP, 2u * 1024u * 1024u);
    MDB_txn *parent, *child = NULL;
    OK(mdb_txn_begin(env, NULL, 0, &parent));
    REQUIRE(mdb_txn_begin(env, parent, 0, &child) == MDB_BAD_TXN);
    REQUIRE(child == NULL);
    mdb_txn_abort(parent);
    mdb_env_close(env);
    nosubdir_reopen(argv[2]);
    relative_path_survives_working_directory_change(argv[1]);
    REQUIRE(assertions == 0);
    puts("controls-ok");
    return 0;
}
