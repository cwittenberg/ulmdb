#include <lmdb.h>
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define CHECK(x) do { if(!(x)){fprintf(stderr,"line %d: %s\n",__LINE__,#x);exit(1);} } while(0)
#define OK(x) CHECK((x)==0)
static unsigned comparisons;
static int folded(const MDB_val *a,const MDB_val *b) {
    comparisons++;size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;
    const unsigned char *x=a->mv_data,*y=b->mv_data;
    for(size_t i=0;i<n;i++){int d=tolower(x[i])-tolower(y[i]);if(d)return d;}
    return (a->mv_size>b->mv_size)-(a->mv_size<b->mv_size);
}
int main(int argc,char **argv) {
    CHECK(argc==2);MDB_env *env;MDB_txn *txn;MDB_dbi db;MDB_val key,value;
    OK(mdb_env_create(&env));OK(mdb_env_set_maxdbs(env,2));OK(mdb_env_open(env,argv[1],MDB_NOTLS,0600));
    OK(mdb_txn_begin(env,NULL,0,&txn));OK(mdb_dbi_open(txn,"custom",MDB_CREATE,&db));OK(mdb_set_compare(txn,db,folded));
    for(unsigned i=0;i<1024;i++){char k[16];snprintf(k,sizeof(k),"k%04u",i);key=(MDB_val){strlen(k),k};value=(MDB_val){1,"x"};OK(mdb_put(txn,db,&key,&value,0));}
    key=(MDB_val){5,"K0256"};value=(MDB_val){20000,NULL};comparisons=0;OK(mdb_put(txn,db,&key,&value,MDB_RESERVE));CHECK(comparisons<128);
    void *reserved=value.mv_data;memset(reserved,0x42,value.mv_size);MDB_val got;OK(mdb_get(txn,db,&key,&got));CHECK(got.mv_data==reserved&&got.mv_size==20000);
    memset(reserved,0x43,value.mv_size);MDB_cursor *cursor;OK(mdb_cursor_open(txn,db,&cursor));MDB_val current=key;OK(mdb_cursor_get(cursor,&current,&got,MDB_SET_KEY));
    CHECK(current.mv_size==5&&!memcmp(current.mv_data,"k0256",5));CHECK(((unsigned char *)got.mv_data)[19999]==0x43);mdb_cursor_close(cursor);
    value=(MDB_val){8,NULL};comparisons=0;CHECK(mdb_put(txn,db,&key,&value,MDB_RESERVE|MDB_NOOVERWRITE)==MDB_KEYEXIST);CHECK(value.mv_size==20000&&comparisons<128);
    key=(MDB_val){5,"z1024"};value=(MDB_val){8,NULL};comparisons=0;OK(mdb_put(txn,db,&key,&value,MDB_RESERVE|MDB_APPEND));CHECK(comparisons<128);memcpy(value.mv_data,"reserved",8);
    OK(mdb_txn_commit(txn));mdb_env_close(env);
    OK(mdb_env_create(&env));OK(mdb_env_set_maxdbs(env,2));OK(mdb_env_open(env,argv[1],MDB_RDONLY|MDB_NOTLS,0600));OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&txn));
    OK(mdb_dbi_open(txn,"custom",0,&db));OK(mdb_set_compare(txn,db,folded));key=(MDB_val){5,"K0256"};OK(mdb_get(txn,db,&key,&got));CHECK(got.mv_size==20000&&((unsigned char *)got.mv_data)[19999]==0x43);
    mdb_txn_abort(txn);mdb_env_close(env);
    OK(mdb_env_create(&env));OK(mdb_env_set_maxdbs(env,2));OK(mdb_env_open(env,argv[1],MDB_NOTLS,0600));OK(mdb_txn_begin(env,NULL,0,&txn));
    OK(mdb_dbi_open(txn,"custom",0,&db));OK(mdb_set_compare(txn,db,folded));key=(MDB_val){6,"z-again"};value=(MDB_val){6,"writer"};comparisons=0;
    OK(mdb_put(txn,db,&key,&value,0));CHECK(comparisons<128);OK(mdb_txn_commit(txn));mdb_env_close(env);
    puts("reserve-ordering-ok: actual pointer, custom equality, indexed no-overwrite/append, reopen read+write");return 0;
}
