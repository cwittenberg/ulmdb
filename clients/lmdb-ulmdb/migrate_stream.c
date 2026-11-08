/* Bounded decoder for the versioned, little-endian snapshot stream. */
#include "migrate_stream.h"
#include "migrate_fs.h"
#ifdef _WIN32
#include "operator_windows.h"
#include "migration_windows.h"
#include <io.h>
#else
#include <dlfcn.h>
#endif
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifndef _WIN32
#include <unistd.h>
#include <fcntl.h>
#else
#define read migration_windows_read
#endif

#define MIGRATION_MAX_KEY 65536u
#define MIGRATION_MAX_VALUE UINT32_MAX
#define MIGRATION_MEMORY (1024u*1024u)
#define MIGRATION_IO 65536u
#define MIGRATION_MAX_DATABASES 65529u

int migration_load(struct migration_api *api, const char *path, char *error, size_t size) {
    memset(api, 0, sizeof *api);
#ifdef _WIN32
    wchar_t *wide=tool_windows_wide(path);if(!wide)return migration_error(error,size,"invalid UTF8 library path");
    api->library=LoadLibraryExW(wide,NULL,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);free(wide);
    if(!api->library)return migration_error(error,size,"load ULMDB library: Windows error %lu",(unsigned long)GetLastError());
#define LOAD(name) do { \
    FARPROC symbol=GetProcAddress((HMODULE)api->library,"mdb_" #name); \
    if(!symbol){migration_error(error,size,"ULMDB library missing mdb_%s",#name);migration_unload(api);return -1;} \
    _Static_assert(sizeof api->name==sizeof symbol,"Windows function pointer size"); \
    memcpy(&api->name,&symbol,sizeof symbol); \
} while(0)
#else
    api->library = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (!api->library) return migration_error(error, size, "load ULMDB library: %s", dlerror());
#define LOAD(name) do { \
    void *symbol = dlsym(api->library, "mdb_" #name); \
    if (!symbol) { \
        migration_error(error, size, "ULMDB library is missing mdb_%s", #name); \
        migration_unload(api); return -1; \
    } \
    _Static_assert(sizeof api->name == sizeof symbol, "POSIX dlsym pointer size"); \
    memcpy(&api->name, &symbol, sizeof symbol); \
} while (0)
#endif
    LOAD(env_create); LOAD(env_set_maxdbs); LOAD(env_set_mapsize); LOAD(env_set_maxreaders);
    LOAD(env_set_pagesize); LOAD(env_get_maxkeysize);
    LOAD(env_open); LOAD(env_close); LOAD(txn_begin); LOAD(txn_abort); LOAD(txn_commit);
    LOAD(dbi_open); LOAD(put); LOAD(strerror);
#undef LOAD
    return 0;
}
void migration_unload(struct migration_api *api) {
#ifdef _WIN32
    if(api->library)FreeLibrary((HMODULE)api->library);
#else
    if (api->library) dlclose(api->library);
#endif
    memset(api, 0, sizeof *api);
}
int migration_check(const struct migration_api *api, int result, const char *operation,
                    char *error, size_t size) {
    if (!result) return 0;
    const char *message = api->strerror(result);
    return migration_error(error, size, "%s: %s (%d)", operation,
                           message ? message : "ULMDB error", result);
}
static int exact(int input, void *output, size_t size, char *error, size_t capacity) {
    unsigned char *bytes = output;
    while (size) {
        ssize_t count = read(input, bytes, size);
        if (count < 0 && errno == EINTR) continue;
        if (count < 0) return migration_error(error, capacity, "read LMDB export: %s", strerror(errno));
        if (!count) return migration_error(error, capacity, "LMDB exporter ended before the complete snapshot was received");
        bytes += count;
        size -= (size_t)count;
    }
    return 0;
}
static uint32_t little32(const unsigned char *data) {
    uint32_t value = 0;
    for (unsigned i = 0; i < 4; ++i) value |= (uint32_t)data[i] << (8 * i);
    return value;
}
static uint64_t little64(const unsigned char *data) {
    uint64_t value = 0;
    for (unsigned i = 0; i < 8; ++i) value |= (uint64_t)data[i] << (8 * i);
    return value;
}
int migration_read_header(int input, struct migration_info *info, char *error, size_t size) {
    static const unsigned char magic[16] = {'U','L','M','D','B','-','M','I','G','R','A','T','E',0,2,0};
    unsigned char header[36];
    memset(info, 0, sizeof *info);
    if (exact(input, header, sizeof header, error, size)) return -1;
    if (memcmp(header, magic, 14) || header[15] || (header[14]!=2 && header[14]!=3))
        return migration_error(error, size, "Unsupported LMDB export stream header");
    info->page_size=4096;info->max_key_size=511;
    if(header[14]==3){
        unsigned char geometry[8];if(exact(input,geometry,sizeof geometry,error,size))return -1;
        info->page_size=little32(geometry);info->max_key_size=little32(geometry+4);
        if(info->page_size<256||info->page_size>65536||(info->page_size&(info->page_size-1))||!info->max_key_size||info->max_key_size>MIGRATION_MAX_KEY)
            return migration_error(error,size,"invalid source geometry/key bounds");
    }
    info->source_sequence = little64(header + 16);
    info->max_readers = little32(header + 24);
    info->map_size = little64(header + 28);
    if (!info->max_readers || !info->map_size || info->map_size > SIZE_MAX)
        return migration_error(error, size, "LMDB export has invalid environment limits");
    return 0;
}
static int database(const struct migration_api *api, MDB_txn *txn, int input,
                     MDB_dbi *current, unsigned *current_flags, struct migration_info *info, char *error, size_t size) {
    unsigned char fields[8];
    char name[MIGRATION_MAX_KEY + 1];
    if (exact(input, fields, sizeof fields, error, size)) return -1;
    uint32_t flags = little32(fields), length = little32(fields + 4);
    if (length > info->max_key_size || info->databases >= MIGRATION_MAX_DATABASES)
        return migration_error(error, size, "Exported database exceeds the LMDB name or database limit");
    if (flags & ~(MDB_REVERSEKEY | MDB_DUPSORT | MDB_INTEGERKEY |
                  MDB_DUPFIXED | MDB_INTEGERDUP | MDB_REVERSEDUP))
        return migration_error(error, size, "Exported database contains unsupported flags");
    /* The first D ordinal is MAIN. Every subsequent D is named, even when
     * its name length is zero (modern LMDB). No stream version change needed. */
    if (length && !info->databases)
        return migration_error(error, size, "LMDB export must start with the main database");
    if (exact(input, name, length, error, size)) return -1;
    if (memchr(name, 0, length))
        return migration_error(error, size, "Exported database name contains NUL");
    name[length] = 0;
    if (migration_check(api, api->dbi_open(txn, info->databases ? name : NULL, flags | MDB_CREATE, current),
                        "open destination database", error, size)) return -1;
    *current_flags=flags;
    ++info->databases;
    return 0;
}
static int spool_open(FILE **out){
#ifdef _WIN32
    return tool_spool_open(out);
#else
    const char *tmp=getenv("TMPDIR");if(!tmp||!*tmp)tmp="/tmp";
    size_t n=strlen(tmp);if(n>SIZE_MAX-40)return ENAMETOOLONG;
    char *path=malloc(n+40);if(!path)return ENOMEM;
    snprintf(path,n+40,"%s/ulmdb-migrate-field-XXXXXX",tmp);
    int fd;
#if defined(__APPLE__)
    fd=mkstemp(path);
    if(fd>=0){int flags=fcntl(fd,F_GETFD);
        if(flags<0||fcntl(fd,F_SETFD,flags|FD_CLOEXEC)<0){
            int rc=errno;close(fd);(void)unlink(path);free(path);return rc;
        }
    }
#else
    fd=mkostemp(path,O_CLOEXEC);
#endif
    if(fd<0){int rc=errno;free(path);return rc;}
    if(unlink(path)){int rc=errno;close(fd);free(path);return rc;}free(path);
    FILE *file=fdopen(fd,"w+b");if(!file){int rc=errno;close(fd);return rc;}*out=file;return 0;
#endif
}
static int record(const struct migration_api *api,MDB_env *env,MDB_txn *txn,MDB_dbi current,
                   unsigned flags,int input,struct migration_info *info,char *error,size_t capacity){
    unsigned char fields[16];
    if(exact(input,fields,sizeof fields,error,capacity))return -1;
    uint64_t key_size=little64(fields),value_size=little64(fields+8);
    int maxkey=api->env_get_maxkeysize(env);
    if(maxkey<=0||!key_size||key_size>(uint64_t)maxkey||key_size>info->max_key_size||value_size>MIGRATION_MAX_VALUE||value_size>SIZE_MAX||
       ((flags&MDB_DUPSORT)&&value_size>(uint64_t)maxkey))
        return migration_error(error,capacity,"Exported record exceeds the environment key/value profile");
    unsigned char *key=malloc((size_t)key_size),*value=NULL;FILE *spool=NULL;int result=-1;
    if(!key){migration_error(error,capacity,"allocate key");goto done;}
    if(exact(input,key,(size_t)key_size,error,capacity))goto done;
    MDB_val k={(size_t)key_size,key},v={(size_t)value_size,NULL};
    if(value_size<=MIGRATION_MEMORY){
        value=malloc(value_size?(size_t)value_size:1);if(!value){migration_error(error,capacity,"allocate bounded value");goto done;}
        if(exact(input,value,(size_t)value_size,error,capacity))goto done;
        v.mv_data=value;
        if(migration_check(api,api->put(txn,current,&k,&v,0),"put destination record",error,capacity))goto done;
    }else{
        int rc=spool_open(&spool);if(rc){migration_error(error,capacity,"create private value spool: %s",strerror(rc));goto done;}
        unsigned char block[MIGRATION_IO];uint64_t remaining=value_size;
        while(remaining){size_t n=remaining>sizeof block?sizeof block:(size_t)remaining;
            if(exact(input,block,n,error,capacity))goto done;
            if(fwrite(block,1,n,spool)!=n){migration_error(error,capacity,"write value spool");goto done;}remaining-=n;
        }
        if(fflush(spool)){migration_error(error,capacity,"flush value spool");goto done;}
        if(migration_check(api,api->put(txn,current,&k,&v,MDB_RESERVE),"reserve destination record",error,capacity))goto done;
#ifdef _WIN32
        rc=tool_spool_copy(spool,v.mv_data,v.mv_size);
        if(rc){migration_error(error,capacity,"map/copy value spool: %s",strerror(rc));goto done;}
#else
        if(fseeko(spool,0,SEEK_SET)){migration_error(error,capacity,"rewind value spool");goto done;}
        size_t offset=0;while(offset<v.mv_size){size_t n=v.mv_size-offset;if(n>MIGRATION_IO)n=MIGRATION_IO;
            if(fread((unsigned char*)v.mv_data+offset,1,n,spool)!=n){migration_error(error,capacity,"read value spool");goto done;}offset+=n;}
#endif
    }
    ++info->records;result=0;
 done:if(spool)fclose(spool);free(value);free(key);return result;
}
int migration_replay(const struct migration_api *api, MDB_env *env, int input,
                     struct migration_info *info, char *error, size_t size) {
    MDB_txn *txn = NULL;
    MDB_dbi current = 0;
    int inside = 0, result = -1;unsigned current_flags=0;
    if (migration_check(api, api->txn_begin(env, NULL, 0, &txn), "begin import", error, size))
        return -1;
    for (;;) {
        unsigned char tag;
        if (exact(input, &tag, 1, error, size)) break;
        if (tag == 'D' && !inside) {
            if (database(api, txn, input, &current, &current_flags, info, error, size)) break;
            inside = 1;
        } else if (tag == 'R' && inside) {
            if (record(api, env, txn, current, current_flags, input, info, error, size)) break;
        } else if (tag == 'E' && inside) {
            inside = 0;
        } else if (tag == 'Z' && !inside && info->databases) {
            unsigned char trailing;
            ssize_t count;
            do { count = read(input, &trailing, 1); } while (count < 0 && errno == EINTR);
            if (count) {
                migration_error(error, size, count < 0 ? "Read failed after LMDB snapshot" :
                                "Trailing data after LMDB snapshot");
                break;
            }
            MDB_txn *committing = txn;
            txn = NULL; /* Commit consumes the handle even when it returns an error. */
            result = migration_check(api, api->txn_commit(committing), "commit import", error, size);
            break;
        } else {
            migration_error(error, size, "Invalid or misplaced LMDB export record 0x%02x", tag);
            break;
        }
    }
    if (txn) api->txn_abort(txn);
    return result;
}
