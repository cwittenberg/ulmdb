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

MDB_crypto_funcs *MDB_crypto(void) {
    static MDB_crypto_funcs functions = {derive, encrypt, sum, 32, 16, 8};
    return &functions;
}
