/* Candidate resource contract: scan an already-mapped database with bounded
 * extra address space, preserving returned mapped bytes across cursor steps. */
#include <lmdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef __linux__
#include <sys/resource.h>
#include <unistd.h>
#endif
#define RECORDS 2048u
#define VALUE_BYTES 8192u
#define REQUIRE(x) do {if(!(x)){fprintf(stderr,"%s:%d: %s\n",__FILE__,__LINE__,#x);exit(1);}}while(0)
#define OK(x) do{int r_=(x);if(r_){fprintf(stderr,"%s:%d: %s: %d %s\n",__FILE__,__LINE__,#x,r_,mdb_strerror(r_));exit(1);}}while(0)
static MDB_val bytes(void *p,size_t n){MDB_val v={n,p};return v;}
static void number(unsigned n,unsigned char out[4]){out[0]=(unsigned char)(n>>24);out[1]=(unsigned char)(n>>16);out[2]=(unsigned char)(n>>8);out[3]=(unsigned char)n;}
static unsigned decoded(const unsigned char *p){return ((unsigned)p[0]<<24)|((unsigned)p[1]<<16)|((unsigned)p[2]<<8)|p[3];}
int main(int argc,char **argv){
    REQUIRE(argc==3);int seed=!strcmp(argv[1],"seed");MDB_env*env;MDB_txn*txn;MDB_dbi records,duplicates;MDB_cursor*cursor;MDB_val key,value;
    OK(mdb_env_create(&env));OK(mdb_env_set_maxdbs(env,8));OK(mdb_env_set_mapsize(env,128u*1024u*1024u));OK(mdb_env_open(env,argv[2],MDB_NOTLS,0600));
    OK(mdb_txn_begin(env,NULL,seed?0:MDB_RDONLY,&txn));
    OK(mdb_dbi_open(txn,"records",seed?MDB_CREATE:0,&records));OK(mdb_dbi_open(txn,"duplicates",seed?(MDB_CREATE|MDB_DUPSORT|MDB_DUPFIXED):0,&duplicates));
    if(seed){
        unsigned char *payload=malloc(VALUE_BYTES);REQUIRE(payload!=NULL);
        for(unsigned i=0;i<RECORDS;i++){char text[16];int n=snprintf(text,sizeof text,"%08u",i);memset(payload,(int)(i&255u),VALUE_BYTES);key=bytes(text,(size_t)n);value=bytes(payload,VALUE_BYTES);OK(mdb_put(txn,records,&key,&value,0));unsigned char packed[4];number(i,packed);key=bytes("group",5);value=bytes(packed,4);OK(mdb_put(txn,duplicates,&key,&value,0));}
        free(payload);OK(mdb_txn_commit(txn));mdb_env_close(env);return 0;
    }
#ifdef __linux__
    /* Seed in a separate process so malloc cannot reuse its large writer arena.
       The snapshot is fully mapped before limiting only additional allocation. */
    FILE *statm=fopen("/proc/self/statm","r");REQUIRE(statm!=NULL);unsigned long long pages;REQUIRE(fscanf(statm,"%llu",&pages)==1);fclose(statm);
    struct rlimit previous,limited;REQUIRE(getrlimit(RLIMIT_AS,&previous)==0);limited=previous;
    unsigned long long bound=pages*(unsigned long long)sysconf(_SC_PAGESIZE)+8u*1024u*1024u;
    if(previous.rlim_cur==RLIM_INFINITY||bound<previous.rlim_cur)limited.rlim_cur=(rlim_t)bound;
    REQUIRE(setrlimit(RLIMIT_AS,&limited)==0);
#endif
    OK(mdb_cursor_open(txn,records,&cursor));MDB_val first={0,NULL};
    for(unsigned i=0;i<RECORDS;i++){OK(mdb_cursor_get(cursor,&key,&value,i?MDB_NEXT:MDB_FIRST));char expected[16];int n=snprintf(expected,sizeof expected,"%08u",i);REQUIRE(key.mv_size==(size_t)n&&!memcmp(key.mv_data,expected,(size_t)n));REQUIRE(value.mv_size==VALUE_BYTES);REQUIRE(((const unsigned char*)value.mv_data)[0]==(i&255u));REQUIRE(((const unsigned char*)value.mv_data)[VALUE_BYTES-1]==(i&255u));if(!i)first=value;}
    REQUIRE(mdb_cursor_get(cursor,&key,&value,MDB_NEXT)==MDB_NOTFOUND);mdb_cursor_close(cursor);
    REQUIRE(first.mv_size==VALUE_BYTES&&((const unsigned char*)first.mv_data)[0]==0&&((const unsigned char*)first.mv_data)[VALUE_BYTES-1]==0);
    OK(mdb_cursor_open(txn,duplicates,&cursor));key=bytes("group",5);OK(mdb_cursor_get(cursor,&key,&value,MDB_SET));
    unsigned total=0;int result=mdb_cursor_get(cursor,&key,&value,MDB_GET_MULTIPLE);MDB_val retained={0,NULL};unsigned batches=0;
    while(result==0){REQUIRE(value.mv_size>0&&value.mv_size<=4096&&value.mv_size%4==0);if(!batches)retained=value;for(size_t offset=0;offset<value.mv_size;offset+=4){REQUIRE(decoded((const unsigned char*)value.mv_data+offset)==total);total++;}batches++;result=mdb_cursor_get(cursor,&key,&value,MDB_NEXT_MULTIPLE);}
    REQUIRE(result==MDB_NOTFOUND&&total==RECORDS&&batches>1);REQUIRE(decoded(retained.mv_data)==0);OK(mdb_cursor_get(cursor,&key,&value,MDB_PREV_MULTIPLE));REQUIRE(value.mv_size>0&&value.mv_size<=4096);mdb_cursor_close(cursor);REQUIRE(decoded(retained.mv_data)==0);
#ifdef __linux__
    REQUIRE(setrlimit(RLIMIT_AS,&previous)==0);
#endif
    mdb_txn_abort(txn);mdb_env_close(env);puts("streaming-ok");return 0;
}
