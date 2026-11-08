/* Reversible fixture callbacks exercise the ABI; these are not cryptography. */
#include <lmdb.h>
#include <errno.h>
#include <dirent.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#define CHECK(x) do { if (!(x)) { fprintf(stderr,"line %d: %s\n",__LINE__,#x); exit(1); } } while(0)
#define OK(x) do { int rc=(x); if(rc) { fprintf(stderr,"line %d: %s: %d (%s)\n",__LINE__,#x,rc,mdb_strerror(rc)); exit(1); } } while(0)
static uint64_t digest(const void *p,size_t n,uint64_t h) {
    const unsigned char *s=p; while(n--) h=(h ^ *s++)*UINT64_C(1099511628211); return h;
}
static void sum(const MDB_val *source,MDB_val *destination,const MDB_val *key) {
    uint64_t h=digest(source->mv_data,source->mv_size,UINT64_C(1469598103934665603));
    if(key) h=digest(key->mv_data,key->mv_size,h);
    for(size_t i=0;i<destination->mv_size;i++) ((unsigned char *)destination->mv_data)[i]=(unsigned char)(h>>(8*(i%8)));
}
static int encrypt(const MDB_val *source,MDB_val *destination,const MDB_val *keys,int enc) {
    CHECK(keys[0].mv_size==32&&keys[1].mv_size==16&&keys[2].mv_size==16);
    CHECK(source->mv_size==destination->mv_size);
    const unsigned char *key=keys[0].mv_data,*iv=keys[1].mv_data,*input=source->mv_data;
    unsigned char *out=destination->mv_data;
    if(!enc) {
        unsigned char tag[16];MDB_val data={sizeof(tag),tag};sum(source,&data,&keys[0]);
        if(memcmp(tag,keys[2].mv_data,sizeof(tag))) return 1;
    }
    for(size_t i=0;i<source->mv_size;i++) out[i]=input[i]^key[i%32]^iv[i%16];
    if(enc) {MDB_val mac=keys[2];sum(destination,&mac,&keys[0]);}
    return 0;
}
static int derive(const char *password,MDB_val *key) {
    uint64_t h=digest(password,strlen(password),7);
    for(size_t i=0;i<key->mv_size;i++) ((unsigned char *)key->mv_data)[i]=(unsigned char)(h>>(8*(i%8)));
    return 0;
}
static void verify(MDB_env *env,const unsigned char *expected,size_t n) {
    MDB_txn *txn=NULL;MDB_dbi db;OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&txn));
    OK(mdb_dbi_open(txn,"data",0,&db));char k[]={0,'k',(char)0xff};MDB_val key={sizeof(k),k},value;
    OK(mdb_get(txn,db,&key,&value));CHECK(value.mv_size==n&&!memcmp(value.mv_data,expected,n));
    MDB_stat stat;OK(mdb_stat(txn,db,&stat));CHECK(stat.ms_psize==8192);
    mdb_txn_abort(txn);
}

/* Counters assert real callback use without comparing implementation-specific
 * callback counts. The reversible callbacks above are copied unchanged from
 * clients/c/tests/transforms.c; they are ABI fixtures, not cryptography. */
static unsigned sum_calls, encrypt_calls, derive_calls;
static void counted_sum(const MDB_val *source, MDB_val *destination, const MDB_val *key) {
    ++sum_calls;
    sum(source, destination, key);
}
static int counted_encrypt(const MDB_val *source, MDB_val *destination,
                           const MDB_val *keys, int enc) {
    ++encrypt_calls;
    return encrypt(source, destination, keys, enc);
}
static int counted_derive(const char *password, MDB_val *key) {
    ++derive_calls;
    return derive(password, key);
}
static MDB_env *configured_env(int kind, int setup) {
    MDB_env *env;
    OK(mdb_env_create(&env));
    OK(mdb_env_set_maxdbs(env, 4));
    OK(mdb_env_set_mapsize(env, 64u << 20));
    OK(mdb_env_set_pagesize(env, 8192));
    if (setup) {
        MDB_crypto_funcs hooks = {counted_derive, counted_encrypt, counted_sum, 32, 16, 8};
        mdb_modsetup(env, &hooks, "passphrase");
    } else {
        if (kind & 1) OK(mdb_env_set_checksum(env, counted_sum, 8));
        if (kind & 2) {
            unsigned char bytes[32];
            memset(bytes, 7, sizeof bytes);
            MDB_val key = {sizeof bytes, bytes};
            OK(mdb_env_set_encrypt(env, counted_encrypt, &key, 16));
            memset(bytes, 0, sizeof bytes);
        }
    }
    unsigned selected;
    OK(mdb_env_get_flags(env, &selected));
    CHECK(!!(selected & MDB_REMAP_CHUNKS) == !!(kind & 2));
    return env;
}
static MDB_env *shared_open(const char *path, int kind, int setup, unsigned flags) {
    MDB_env *env = configured_env(kind, setup);
    OK(mdb_env_open(env, path, flags | MDB_NOTLS, 0600));
    CHECK(mdb_env_set_checksum(env, counted_sum, 8) == EINVAL);
    unsigned char bytes[32] = {0};
    MDB_val key = {sizeof bytes, bytes};
    CHECK(mdb_env_set_encrypt(env, counted_encrypt, &key, 16) == EINVAL);
    return env;
}
static void path_under(char *out, size_t size, const char *root,
                       const char *role, unsigned index) {
    int n = snprintf(out, size, "%s/%s-%u", root, role, index);
    CHECK(n > 0 && (size_t)n < size);
}
int main(int argc, char **argv) {
    CHECK(argc == 2);
    CHECK(mdb_env_set_checksum(NULL, counted_sum, 8) == EINVAL);
    unsigned char material[32] = {0};
    MDB_val key_material = {sizeof material, material};
    CHECK(mdb_env_set_encrypt(NULL, counted_encrypt, &key_material, 16) == EINVAL);
    unsigned char payload[20000];
    for (size_t i = 0; i < sizeof payload; ++i) payload[i] = (unsigned char)(i * 31);
    for (unsigned index = 0; index < 4; ++index) {
        int kind = index == 3 ? 3 : (int)index + 1;
        int setup = index == 3;
        char source[4096], full[4096];
        path_under(source, sizeof source, argv[1], "source", index);
        path_under(full, sizeof full, argv[1], "full", index);
        /* LMDB's copy destination must already exist and be empty. */
        CHECK(mkdir(source, 0700) == 0);
        CHECK(mkdir(full, 0700) == 0);
        sum_calls = encrypt_calls = derive_calls = 0;
        MDB_env *env = shared_open(source, kind, setup, 0);
        MDB_txn *txn;
        MDB_dbi db;
        OK(mdb_txn_begin(env, NULL, 0, &txn));
        OK(mdb_dbi_open(txn, "data", MDB_CREATE, &db));
        char bytes[] = {0, 'k', (char)0xff};
        MDB_val key = {sizeof bytes, bytes}, value = {sizeof payload, payload};
        OK(mdb_put(txn, db, &key, &value, 0));
        OK(mdb_txn_commit(txn));
        verify(env, payload, sizeof payload);
        CHECK(!(kind & 1) || sum_calls > 0);
        CHECK(!(kind & 2) || encrypt_calls > 0);
        CHECK(!setup || derive_calls > 0);
        OK(mdb_env_copy2(env, full, 0));
        mdb_env_close(env);
        const char *reopen[] = {source, full};
        for (size_t i = 0; i < sizeof reopen / sizeof *reopen; ++i) {
            env = shared_open(reopen[i], kind, setup, MDB_RDONLY);
            verify(env, payload, sizeof payload);
            mdb_env_close(env);
        }
        printf("transform-kind=%d setup=%d overflow=ok full=ok cold=ok\n", kind, setup);
    }
    /* Compact-copy reference defects are observed in a separate per-case
     * executable, rather than counted as matching positive coverage here.
     * All callbacks live in this executable until every environment closes.
     * No failed-derive or wrong-key phase assumptions from Native tests. */
    puts("transform-callbacks-ok");
    return 0;
}
