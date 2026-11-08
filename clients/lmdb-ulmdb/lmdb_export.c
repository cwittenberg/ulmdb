/* Read one real LMDB snapshot in a process isolated from the ULMDB adapter. */
#include <lmdb.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_DATABASES 65528
struct named { char *name; MDB_dbi dbi; unsigned flags; };

static void fail(int result, const char *operation) {
    fprintf(stderr, "lmdb-export: %s: %s (%d)\n", operation, mdb_strerror(result), result);
    exit(1);
}
static void check(int result, const char *operation) { if (result) fail(result, operation); }
static void write_bytes(const void *bytes, size_t size) {
    if (size && fwrite(bytes, 1, size, stdout) != size) { perror("lmdb-export: stdout"); exit(1); }
}
static void u32(uint32_t value) {
    unsigned char bytes[4];
    for (unsigned i = 0; i < 4; ++i) bytes[i] = (unsigned char)(value >> (8 * i));
    write_bytes(bytes, sizeof bytes);
}
static void u64(uint64_t value) {
    unsigned char bytes[8];
    for (unsigned i = 0; i < 8; ++i) bytes[i] = (unsigned char)(value >> (8 * i));
    write_bytes(bytes, sizeof bytes);
}
static char *candidate_name(MDB_val key) {
    size_t size = key.mv_size;
    if (!size) return NULL;
    const char *bytes = key.mv_data;
#if MDB_VERSION_MAJOR >= 1
    if (bytes[size - 1] == '\0') --size; /* LMDB 1.0 includes the terminator. */
#endif
#if MDB_VERSION_MAJOR < 1
    if (!size) return NULL;
#endif
    if (memchr(bytes, '\0', size)) return NULL;
    char *name = malloc(size + 1);
    if (!name) { perror("lmdb-export: malloc"); exit(1); }
    memcpy(name, bytes, size);
    name[size] = '\0';
    return name;
}
static int is_database(MDB_txn *txn, MDB_cursor *cursor, MDB_val key) {
#if MDB_VERSION_MAJOR >= 1
    (void)txn;
    (void)key;
    return mdb_cursor_is_db(cursor);
#else
    (void)cursor;
    char *name = candidate_name(key);
    if (!name) return 0;
    MDB_dbi dbi;
    int opened = mdb_dbi_open(txn, name, 0, &dbi);
    free(name);
    if (opened == 0) return 1;
    if (opened == MDB_NOTFOUND || opened == MDB_INCOMPATIBLE) return 0;
    fail(opened, "legacy catalog entry");
    return 0;
#endif
}
static void database(MDB_txn *txn, MDB_dbi dbi, const char *name, unsigned flags) {
    write_bytes("D", 1);
    u32(flags);
    size_t length = name ? strlen(name) : 0;
    u32((uint32_t)length);
    write_bytes(name, length);
    MDB_cursor *cursor;
    check(mdb_cursor_open(txn, dbi, &cursor), "cursor_open");
    MDB_val key, value;
    int result = mdb_cursor_get(cursor, &key, &value, MDB_FIRST);
    while (result == 0) {
        /* The public cursor predicate distinguishes a descriptor from a user
           key such as "plain" beside the LMDB 1.0 catalog key "plain\0". */
        if (name || !is_database(txn, cursor, key)) {
            write_bytes("R", 1);
            u64((uint64_t)key.mv_size);
            u64((uint64_t)value.mv_size);
            write_bytes(key.mv_data, key.mv_size);
            write_bytes(value.mv_data, value.mv_size);
        }
        result = mdb_cursor_get(cursor, &key, &value, MDB_NEXT);
    }
    if (result != MDB_NOTFOUND) fail(result, "cursor_get");
    mdb_cursor_close(cursor);
    write_bytes("E", 1);
}
#ifdef _WIN32
#include <io.h>
#include <fcntl.h>
#include <windows.h>
static int exporter_main(int,char**);
int wmain(int argc,wchar_t **wide){
    char **argv=calloc((size_t)argc+1,sizeof *argv);if(!argv)return 1;int rc=1;
    for(int i=0;i<argc;i++){int n=WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,wide[i],-1,NULL,0,NULL,NULL);if(!n)goto done;argv[i]=malloc((size_t)n);if(!argv[i]||!WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,wide[i],-1,argv[i],n,NULL,NULL))goto done;}
    if(_setmode(_fileno(stdout),_O_BINARY)<0)goto done;
    rc=exporter_main(argc,argv);
 done:for(int i=0;i<argc;i++)free(argv[i]);free(argv);return rc;
}
static int exporter_main(int argc,char **argv){
#else
int main(int argc, char **argv) {
#endif
    if (argc < 2 || argc > 3 || (argc == 3 && strcmp(argv[2], "--nosubdir") != 0)) {
        fputs("usage: lmdb-export SOURCE [--nosubdir]\n", stderr); return 2;
    }
    MDB_env *env;
    MDB_txn *txn;
    MDB_dbi main_db;
    check(mdb_env_create(&env), "env_create");
    check(mdb_env_set_maxdbs(env, MAX_DATABASES), "set_maxdbs");
    /* A read-only export needs one slot. The existing lock file supplies its
       actual capacity; requesting the default could enlarge that source file. */
    check(mdb_env_set_maxreaders(env, 1), "set_maxreaders");
    unsigned envflags = MDB_RDONLY | MDB_NOTLS | (argc == 3 ? MDB_NOSUBDIR : 0);
    check(mdb_env_open(env, argv[1], envflags, 0400), "env_open");
    check(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn), "txn_begin");
    check(mdb_dbi_open(txn, NULL, 0, &main_db), "main_open");
    struct named *names = calloc(MAX_DATABASES, sizeof *names);
    if (!names) { perror("lmdb-export: calloc"); return 1; }
    size_t count = 0;
    MDB_cursor *cursor;
    check(mdb_cursor_open(txn, main_db, &cursor), "main_cursor");
    MDB_val key, value;
    int result = mdb_cursor_get(cursor, &key, &value, MDB_FIRST);
    while (result == 0) {
        if (is_database(txn, cursor, key)) {
            char *name = candidate_name(key);
            if (!name) fail(MDB_CORRUPTED, "invalid catalog name");
            MDB_dbi dbi;
            int opened = mdb_dbi_open(txn, name, 0, &dbi);
            if (opened == 0) {
                if (count >= MAX_DATABASES) fail(MDB_DBS_FULL, "catalog capacity");
                names[count].name = name;
                names[count].dbi = dbi;
                check(mdb_dbi_flags(txn, dbi, &names[count].flags), "dbi_flags");
                ++count;
            } else {
                free(name);
                fail(opened, "named_open");
            }
        }
        result = mdb_cursor_get(cursor, &key, &value, MDB_NEXT);
    }
    if (result != MDB_NOTFOUND) fail(result, "main_cursor_get");
    mdb_cursor_close(cursor);
    const unsigned char magic[16] = {'U','L','M','D','B','-','M','I','G','R','A','T','E',0,3,0};
    write_bytes(magic, sizeof magic);
    u64((uint64_t)mdb_txn_id(txn));
    MDB_envinfo environment_info;
    check(mdb_env_info(env, &environment_info), "env_info");
    u32(environment_info.me_maxreaders);
    u64((uint64_t)environment_info.me_mapsize);
    MDB_stat geometry;check(mdb_env_stat(env,&geometry),"env_stat");
    u32(geometry.ms_psize);u32((uint32_t)mdb_env_get_maxkeysize(env));
    unsigned main_flags;
    check(mdb_dbi_flags(txn, main_db, &main_flags), "main_flags");
    database(txn, main_db, NULL, main_flags);
    for (size_t i = 0; i < count; ++i) database(txn, names[i].dbi, names[i].name, names[i].flags);
    write_bytes("Z", 1);
    if (fflush(stdout) != 0) { perror("lmdb-export: flush"); return 1; }
    for (size_t i = 0; i < count; ++i) free(names[i].name);
    free(names);
    mdb_txn_abort(txn);
    mdb_env_close(env);
    return 0;
}
