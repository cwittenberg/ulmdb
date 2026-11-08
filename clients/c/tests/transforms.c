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
static int keyless(const MDB_val *source,MDB_val *destination,const MDB_val *keys,int enc) {
    CHECK(keys[0].mv_size==0);unsigned char supplied[32];memset(supplied,0xa5,sizeof(supplied));
    MDB_val actual[3]={{sizeof(supplied),supplied},keys[1],keys[2]};
    return encrypt(source,destination,actual,enc);
}
static int derive_failure(const char *password,MDB_val *key) {(void)password;(void)key;return 1;}
static int derive_short(const char *password,MDB_val *key) {CHECK(key->mv_size>=32);key->mv_size=32;return derive(password,key);}
static int encryption_failure(const MDB_val *source,MDB_val *destination,const MDB_val *keys,int enc) {(void)source;(void)destination;(void)keys;(void)enc;return 1;}
static MDB_env *open_env(const char *path,int kind,unsigned flags,int wrong) {
    MDB_env *env=NULL;OK(mdb_env_create(&env));OK(mdb_env_set_maxdbs(env,4));
    OK(mdb_env_set_mapsize(env,64u<<20));OK(mdb_env_set_pagesize(env,8192));
    if(kind&1) OK(mdb_env_set_checksum(env,sum,8));
    if(kind&2) {
        unsigned char bytes[32];memset(bytes,wrong?9:7,sizeof(bytes));MDB_val key={sizeof(bytes),bytes};
        OK(mdb_env_set_encrypt(env,encrypt,&key,16));memset(bytes,0,sizeof(bytes));
    }
    OK(mdb_env_open(env,path,flags,0600));
    CHECK(mdb_env_set_checksum(env,sum,8)==EINVAL);
    return env;
}
static void verify(MDB_env *env,const unsigned char *expected,size_t n) {
    MDB_txn *txn=NULL;MDB_dbi db;OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&txn));
    OK(mdb_dbi_open(txn,"data",0,&db));char k[]={0,'k',(char)0xff};MDB_val key={sizeof(k),k},value;
    OK(mdb_get(txn,db,&key,&value));CHECK(value.mv_size==n&&!memcmp(value.mv_data,expected,n));
    MDB_stat stat;OK(mdb_stat(txn,db,&stat));CHECK(stat.ms_psize==8192);
    mdb_txn_abort(txn);
}
int main(int argc,char **argv) {
    CHECK(argc==2);unsigned char payload[20000];for(size_t i=0;i<sizeof(payload);i++) payload[i]=(unsigned char)(i*31);
    for(int kind=1;kind<=3;kind++) for(int unlocked=0;unlocked<=1;unlocked++) {
        char source[1024],copy[1024],compact[1024],archive[1024];
        snprintf(source,sizeof(source),"%s/source-%d-%d",argv[1],kind,unlocked);
        snprintf(copy,sizeof(copy),"%s/copy-%d-%d",argv[1],kind,unlocked);
        snprintf(compact,sizeof(compact),"%s/compact-%d-%d",argv[1],kind,unlocked);
        snprintf(archive,sizeof(archive),"%s/archive-%d-%d",argv[1],kind,unlocked);
        unsigned flags=unlocked?MDB_NOLOCK:0;MDB_env *env=open_env(source,kind,flags,0);
        MDB_txn *txn=NULL;MDB_dbi db;OK(mdb_txn_begin(env,NULL,0,&txn));OK(mdb_dbi_open(txn,"data",MDB_CREATE,&db));
        char k[]={0,'k',(char)0xff};MDB_val key={sizeof(k),k},value={sizeof(payload),payload};OK(mdb_put(txn,db,&key,&value,0));OK(mdb_txn_commit(txn));
        verify(env,payload,sizeof(payload));OK(mdb_env_copy2(env,copy,0));OK(mdb_env_copy2(env,compact,MDB_CP_COMPACT));
        int fd=open(archive,O_CREAT|O_EXCL|O_WRONLY,0600);CHECK(fd>=0);OK(mdb_env_copyfd2(env,fd,0));CHECK(fcntl(fd,F_GETFD)>=0);CHECK(close(fd)==0);
        mdb_env_close(env);
        env=open_env(source,kind,flags|MDB_RDONLY,0);verify(env,payload,sizeof(payload));mdb_env_close(env);
        env=open_env(copy,kind,flags|MDB_RDONLY,0);verify(env,payload,sizeof(payload));mdb_env_close(env);
        env=open_env(compact,kind,flags|MDB_RDONLY,0);verify(env,payload,sizeof(payload));mdb_env_close(env);
        env=open_env(archive,kind,flags|MDB_NOSUBDIR,0);verify(env,payload,sizeof(payload));mdb_env_close(env);
        OK(mdb_env_create(&env));CHECK(mdb_env_open(env,source,MDB_RDONLY,0600)==MDB_ENV_ENCRYPTION);mdb_env_close(env);
        if(kind&2) {
            OK(mdb_env_create(&env));if(kind&1) OK(mdb_env_set_checksum(env,sum,8));
            unsigned char bad[32];memset(bad,9,sizeof(bad));MDB_val badkey={sizeof(bad),bad};OK(mdb_env_set_encrypt(env,encrypt,&badkey,16));
            CHECK(mdb_env_open(env,source,MDB_RDONLY,0600)==MDB_CRYPTO_FAIL);mdb_env_close(env);
        }
    }
    char module_path[1024];snprintf(module_path,sizeof(module_path),"%s/module-setup",argv[1]);
    MDB_env *env=NULL;MDB_crypto_funcs hooks={derive,encrypt,sum,32,16,8};OK(mdb_env_create(&env));
    mdb_modsetup(env,&hooks,"passphrase");OK(mdb_env_open(env,module_path,0,0600));mdb_env_close(env);
    OK(mdb_env_create(&env));mdb_modsetup(env,&hooks,"passphrase");OK(mdb_env_open(env,module_path,MDB_RDONLY,0600));mdb_env_close(env);
    snprintf(module_path,sizeof(module_path),"%s/module-short-key",argv[1]);hooks=(MDB_crypto_funcs){derive_short,encrypt,sum,64,16,8};
    OK(mdb_env_create(&env));mdb_modsetup(env,&hooks,"passphrase");OK(mdb_env_open(env,module_path,0,0600));mdb_env_close(env);
    OK(mdb_env_create(&env));mdb_modsetup(env,&hooks,"passphrase");OK(mdb_env_open(env,module_path,MDB_RDONLY,0600));mdb_env_close(env);
    snprintf(module_path,sizeof(module_path),"%s/keyless",argv[1]);MDB_val empty={0,NULL};
    OK(mdb_env_create(&env));OK(mdb_env_set_encrypt(env,keyless,&empty,16));OK(mdb_env_set_encrypt(env,keyless,&empty,0));OK(mdb_env_open(env,module_path,0,0600));mdb_env_close(env);
    hooks=(MDB_crypto_funcs){NULL,keyless,NULL,0,16,0};OK(mdb_env_create(&env));mdb_modsetup(env,&hooks,"passphrase");OK(mdb_env_open(env,module_path,MDB_RDONLY,0600));mdb_env_close(env);
    snprintf(module_path,sizeof(module_path),"%s/module-fallback",argv[1]);hooks=(MDB_crypto_funcs){NULL,encrypt,sum,32,16,8};
    OK(mdb_env_create(&env));mdb_modsetup(env,&hooks,"passphrase");OK(mdb_env_open(env,module_path,0,0600));mdb_env_close(env);
    OK(mdb_env_create(&env));mdb_modsetup(env,&hooks,"passphrase");OK(mdb_env_open(env,module_path,MDB_RDONLY,0600));mdb_env_close(env);
    snprintf(module_path,sizeof(module_path),"%s/failed-derivation",argv[1]);hooks.mcf_str2key=derive_failure;
    OK(mdb_env_create(&env));mdb_modsetup(env,&hooks,"passphrase");CHECK(mdb_env_open(env,module_path,0,0600)==MDB_CRYPTO_FAIL);CHECK(access(module_path,F_OK)<0&&errno==ENOENT);mdb_env_close(env);
    snprintf(module_path,sizeof(module_path),"%s/failed-empty-directory",argv[1]);CHECK(mkdir(module_path,0700)==0);
    unsigned char material[32]={0};MDB_val material_key={sizeof(material),material};OK(mdb_env_create(&env));OK(mdb_env_set_encrypt(env,encryption_failure,&material_key,16));CHECK(mdb_env_open(env,module_path,0,0600)==MDB_CRYPTO_FAIL);mdb_env_close(env);
    DIR *directory=opendir(argv[1]);CHECK(directory);struct dirent *entry;
    while((entry=readdir(directory))) CHECK(!strstr(entry->d_name,".ulmdb-init-")&&strncmp(entry->d_name,".ulmdb-copy-",12));
    CHECK(closedir(directory)==0);
    puts("transforms-ok: checksum/encryption/both, copied key, overflow, geometry, locked+NOLOCK, full+compact+FD copies, cold reopen, wrong/missing key, module setup");
    return 0;
}
