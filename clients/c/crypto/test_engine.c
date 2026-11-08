#include "lmdb.h"
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include <stdint.h>
#define OK(c) do {int rc=(c);if(rc){fprintf(stderr,"line%d %s=%d\n",__LINE__,#c,rc);exit(1);}}while(0)
static const char *key="000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
static MDB_crypto_funcs *hooks;
static MDB_env *opened(const char *path,unsigned flags,const char *hex){MDB_env *e;OK(mdb_env_create(&e));OK(mdb_env_set_mapsize(e,64u<<20));OK(mdb_env_set_pagesize(e,8192));mdb_modsetup(e,hooks,hex);OK(mdb_env_open(e,path,flags|MDB_NOTLS,0600));return e;}
static void expect(MDB_env *e,int byte,size_t id){MDB_txn *r;MDB_dbi db;MDB_val k={1,"k"},v;OK(mdb_txn_begin(e,0,MDB_RDONLY,&r));OK(mdb_dbi_open(r,0,0,&db));assert(mdb_txn_id(r)==id);OK(mdb_get(r,db,&k,&v));assert(v.mv_size==20000);for(size_t i=0;i<v.mv_size;i++)assert(((unsigned char*)v.mv_data)[i]==(unsigned char)(i+(size_t)byte));OK(mdb_txn_commit(r));}
static void put(MDB_env *e,MDB_dbi db,int byte){unsigned char payload[20000];for(size_t i=0;i<sizeof(payload);i++)payload[i]=(unsigned char)(i+(size_t)byte);MDB_val k={1,"k"},v={sizeof(payload),payload};MDB_txn *w;OK(mdb_txn_begin(e,0,0,&w));OK(mdb_put(w,db,&k,&v,0));OK(mdb_txn_commit(w));}

static unsigned crc32c_bytes(const unsigned char *bytes,size_t length) {
    unsigned c=~0u;
    for(size_t i=0;i<length;i++) {c^=bytes[i];for(unsigned bit=0;bit<8;bit++)c=(c>>1)^(0x82f63b78u*(c&1));}
    return ~c;
}
static int integrity_error(int rc) {
    return rc==MDB_CRYPTO_FAIL||rc==MDB_BAD_CHECKSUM||rc==MDB_CORRUPTED;
}
static void tamper_selected(const char *source,unsigned flags) {
    char current_path[4096],asset[4096],generation[33];
    int n=snprintf(current_path,sizeof current_path,"%s/CURRENT",source);assert(n>0&&(size_t)n<sizeof current_path);
    unsigned char current[64],after[64],extra;
    int fd=open(current_path,O_RDONLY|O_CLOEXEC|O_NOFOLLOW);assert(fd>=0);assert(read(fd,current,sizeof current)==sizeof current&&read(fd,&extra,1)==0);OK(close(fd));
    assert(!memcmp(current,"ULMDBCUR",8)&&!current[8]&&!current[9]&&current[10]==64&&!current[11]);
    unsigned expected=(unsigned)current[60]|((unsigned)current[61]<<8)|((unsigned)current[62]<<16)|((unsigned)current[63]<<24);assert(crc32c_bytes(current,60)==expected);
    for(size_t i=0;i<16;i++)assert(snprintf(generation+2*i,3,"%02x",(unsigned)current[27-i])==2);
    n=snprintf(asset,sizeof asset,"%s/commits/%s.root",source,generation);assert(n>0&&(size_t)n<sizeof asset);
    fd=open(asset,O_RDONLY|O_CLOEXEC|O_NOFOLLOW);assert(fd>=0);struct stat before,held;assert(!fstat(fd,&before)&&S_ISREG(before.st_mode));
    unsigned char header[128];assert(pread(fd,header,sizeof header,0)==sizeof header&&!memcmp(header,"ULMDBTX1",8)&&header[44]==1);
    assert(!fchmod(fd,(before.st_mode&07777)|S_IWUSR));int writer=open(asset,O_RDWR|O_CLOEXEC|O_NOFOLLOW);assert(writer>=0&&!fstat(writer,&held)&&held.st_dev==before.st_dev&&held.st_ino==before.st_ino&&held.st_size==before.st_size);OK(close(fd));
    const off_t offset=128+41;assert(before.st_size>offset);unsigned char saved,changed;assert(pread(writer,&saved,1,offset)==1);changed=(unsigned char)(saved^1u);
    MDB_env *env=opened(source,flags,key);expect(env,3,3);mdb_env_close(env);
    assert(pwrite(writer,&changed,1,offset)==1&&!fsync(writer)&&!fchmod(writer,before.st_mode&07777));
    OK(mdb_env_create(&env));OK(mdb_env_set_mapsize(env,64u<<20));OK(mdb_env_set_pagesize(env,8192));mdb_modsetup(env,hooks,key);
    int rc=mdb_env_open(env,source,flags|MDB_RDONLY|MDB_NOTLS,0600);printf("openssl-selected-tamper-open=%d\n",rc);
    if(!rc) {
        MDB_txn *read;rc=mdb_txn_begin(env,NULL,MDB_RDONLY,&read);printf("openssl-selected-tamper-begin=%d\n",rc);
        if(!rc) {assert(mdb_txn_id(read)==3);MDB_dbi db;rc=mdb_dbi_open(read,NULL,0,&db);if(!rc){MDB_val k={1,"k"},v;rc=mdb_get(read,db,&k,&v);}mdb_txn_abort(read);printf("openssl-selected-tamper-get=%d\n",rc);}
    }
    assert(integrity_error(rc));mdb_env_close(env);
    fd=open(current_path,O_RDONLY|O_CLOEXEC|O_NOFOLLOW);assert(fd>=0&&read(fd,after,sizeof after)==sizeof after&&!memcmp(after,current,sizeof current));OK(close(fd));
    assert(pwrite(writer,&saved,1,offset)==1&&!fsync(writer)&&!fchmod(writer,before.st_mode&07777));assert(!fstat(writer,&held)&&held.st_dev==before.st_dev&&held.st_ino==before.st_ino&&held.st_size==before.st_size);OK(close(writer));
    env=opened(source,flags,key);expect(env,3,3);mdb_env_close(env);
    puts("openssl-selected-tamper-restored=ok");
}

int main(int argc,char **argv){assert(argc==3||(argc==4&&!strcmp(argv[3],"--expect-load-einval")));int blocked=argc==4;char *error=0;void *module=mdb_modload(argv[1],0,&hooks,&error);assert(module&&hooks&&hooks->mcf_encfunc&&hooks->mcf_str2key&&!hooks->mcf_sumfunc&&hooks->mcf_keysize==32&&hooks->mcf_esumsize==16&&hooks->mcf_sumsize==0);
 for(int unlocked=0;unlocked<=1;unlocked++) {char src[2048],base[2048],compact[2048],archive[2048],delta[2048];snprintf(src,sizeof(src),"%s/src-%d",argv[2],unlocked);snprintf(base,sizeof(base),"%s/base-%d",argv[2],unlocked);snprintf(compact,sizeof(compact),"%s/compact-%d",argv[2],unlocked);snprintf(archive,sizeof(archive),"%s/archive-%d",argv[2],unlocked);snprintf(delta,sizeof(delta),"%s/delta-%d",argv[2],unlocked);unsigned flags=unlocked?MDB_NOLOCK:0;MDB_env *e=opened(src,flags,key);MDB_txn *w;MDB_dbi db;OK(mdb_txn_begin(e,0,0,&w));OK(mdb_dbi_open(w,0,0,&db));OK(mdb_txn_commit(w));put(e,db,1);expect(e,1,1);OK(mdb_env_copy2(e,base,0));put(e,db,2);put(e,db,3);OK(mdb_env_incr_dump(e,delta,1));OK(mdb_env_copy2(e,compact,MDB_CP_COMPACT));int fd=open(archive,O_CREAT|O_EXCL|O_RDWR,0600);assert(fd>=0);OK(mdb_env_copyfd2(e,fd,0));assert(fcntl(fd,F_GETFD)>=0);close(fd);mdb_env_close(e);
 e=opened(src,flags,key);expect(e,3,3);mdb_env_close(e);e=opened(base,flags|MDB_WRITEMAP,key);fd=open(delta,O_RDONLY);assert(fd>=0);int load=mdb_env_incr_loadfd(e,fd);if(blocked)assert(load==EINVAL);else OK(load);assert(fcntl(fd,F_GETFD)>=0);close(fd);expect(e,blocked?1:3,blocked?1:3);mdb_env_close(e);e=opened(base,flags,key);expect(e,blocked?1:3,blocked?1:3);mdb_env_close(e);e=opened(compact,flags,key);expect(e,3,1);mdb_env_close(e);e=opened(archive,flags|MDB_NOSUBDIR,key);expect(e,3,3);mdb_env_close(e);
 if(!unlocked)tamper_selected(src,flags);
 MDB_env *bad;OK(mdb_env_create(&bad));mdb_modsetup(bad,hooks,"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");assert(mdb_env_open(bad,src,flags|MDB_RDONLY|MDB_NOTLS,0600)!=0);mdb_env_close(bad);char failed[2048];snprintf(failed,sizeof(failed),"%s/badhex-%d",argv[2],unlocked);OK(mdb_env_create(&bad));mdb_modsetup(bad,hooks,"not-a-key");assert(mdb_env_open(bad,failed,flags,0600)!=0);assert(access(failed,F_OK)<0);mdb_env_close(bad);
 }
 mdb_modunload(module);puts(blocked?"openssl-module-engine-ok: realmoduleload, AESGCM overflow+8192pages, raw/compact/FDarchive, deltadump+knownloadEINVAL, coldreopen, wrongkey/badhex, lockful+NOLOCK":"openssl-module-engine-ok: realmoduleload, AESGCM overflow+8192pages, raw/compact/FDarchive, delta/load, coldreopen, wrongkey/badhex, lockful+NOLOCK");return 0;}
