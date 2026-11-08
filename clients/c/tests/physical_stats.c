#include <lmdb.h>
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define CHECK(x) do {if(!(x)){fprintf(stderr,"line %d: %s\n",__LINE__,#x);exit(1);}} while(0)
#define OK(x) CHECK((x)==0)
static unsigned comparisons;
static int compare(const MDB_val *a,const MDB_val *b) {
    comparisons++;size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;
    const unsigned char *x=a->mv_data,*y=b->mv_data;
    for(size_t i=0;i<n;i++){int d=tolower(x[i])-tolower(y[i]);if(d)return d;}
    return (a->mv_size>b->mv_size)-(a->mv_size<b->mv_size);
}
int main(int argc,char **argv) {
    CHECK(argc==2);MDB_env *env;MDB_txn *txn,*child;MDB_dbi db;MDB_stat before,after;
    OK(mdb_env_create(&env));OK(mdb_env_set_maxdbs(env,2));OK(mdb_env_set_mapsize(env,64u<<20));OK(mdb_env_open(env,argv[1],MDB_NOTLS,0600));
    OK(mdb_txn_begin(env,NULL,0,&txn));OK(mdb_dbi_open(txn,"tree",MDB_CREATE,&db));OK(mdb_set_compare(txn,db,compare));
    for(unsigned i=0;i<1500;i++){char k[16];snprintf(k,sizeof(k),"key%04u",i);MDB_val key={strlen(k),k},value={32,"abcdefghijklmnopqrstuvwxyz012345"};OK(mdb_put(txn,db,&key,&value,0));}
    char large[20000];memset(large,7,sizeof(large));MDB_val key={8,"overflow"},value={sizeof(large),large};OK(mdb_put(txn,db,&key,&value,0));
    comparisons=0;OK(mdb_stat(txn,db,&before));CHECK(comparisons==0);CHECK(before.ms_entries==1501&&before.ms_depth>=2&&before.ms_branch_pages>0&&before.ms_leaf_pages>1&&before.ms_overflow_pages>=5);
    OK(mdb_txn_begin(env,txn,MDB_RDONLY,&child));OK(mdb_stat(child,db,&after));CHECK(after.ms_entries==before.ms_entries&&after.ms_overflow_pages==before.ms_overflow_pages);mdb_txn_abort(child);
    OK(mdb_txn_begin(env,txn,0,&child));key=(MDB_val){3,"new"};value=(MDB_val){1,"x"};OK(mdb_put(child,db,&key,&value,0));OK(mdb_stat(child,db,&after));CHECK(after.ms_entries==1502);mdb_txn_abort(child);
    OK(mdb_stat(txn,db,&after));CHECK(after.ms_entries==1501&&after.ms_leaf_pages==before.ms_leaf_pages);OK(mdb_txn_commit(txn));mdb_env_close(env);
    OK(mdb_env_create(&env));OK(mdb_env_set_maxdbs(env,2));OK(mdb_env_open(env,argv[1],MDB_RDONLY|MDB_NOTLS,0600));OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&txn));OK(mdb_dbi_open(txn,"tree",0,&db));
    OK(mdb_stat(txn,db,&after));CHECK(after.ms_entries==before.ms_entries&&after.ms_depth==before.ms_depth&&after.ms_branch_pages==before.ms_branch_pages&&after.ms_leaf_pages==before.ms_leaf_pages&&after.ms_overflow_pages==before.ms_overflow_pages);
    mdb_txn_abort(txn);mdb_env_close(env);puts("physical-stats-ok: actual split/overflow pages, no custom callbacks, child/abort, cold reopen");return 0;
}
