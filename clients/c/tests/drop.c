/* Clear semantics shared by stock LMDB and the native adapter. */
#include <lmdb.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifndef MDB_DBIS_BUSY
#define MDB_DBIS_BUSY (-30773)
#endif
#define REQUIRE(x) do { if(!(x)){fprintf(stderr,"%s:%d: %s\n",__FILE__,__LINE__,#x);exit(1);} } while(0)
#define OK(x) do {int rc_=(x);if(rc_){fprintf(stderr,"%s:%d: %s returned %d (%s)\n",__FILE__,__LINE__,#x,rc_,mdb_strerror(rc_));exit(1);}}while(0)
static MDB_val text(const char*s){MDB_val v={strlen(s),(void*)s};return v;}
static void put(MDB_txn*t,MDB_dbi db,const char*k,const char*v){MDB_val key=text(k),value=text(v);OK(mdb_put(t,db,&key,&value,0));}
static MDB_env* open_env(const char*path){MDB_env*e;OK(mdb_env_create(&e));OK(mdb_env_set_maxdbs(e,16));OK(mdb_env_set_mapsize(e,8u*1024u*1024u));OK(mdb_env_open(e,path,MDB_NOTLS,0600));return e;}
int main(int argc,char**argv){
    REQUIRE(argc==2);MDB_env*env=open_env(argv[1]);MDB_txn*w,*r,*child;MDB_dbi main_db,integer,fixed,named,long_db;MDB_val k,v;MDB_stat stats;MDB_cursor*cursor;
    OK(mdb_txn_begin(env,NULL,0,&w));
    OK(mdb_dbi_open(w,"integer",MDB_CREATE|MDB_INTEGERKEY,&integer));uint32_t oldkey=1;uint64_t newkey=UINT64_C(0x100000002);k=(MDB_val){4,&oldkey};v=text("integer");OK(mdb_put(w,integer,&k,&v,0));OK(mdb_drop(w,integer,0));k=(MDB_val){8,&newkey};OK(mdb_put(w,integer,&k,&v,0));
    OK(mdb_dbi_open(w,"fixed",MDB_CREATE|MDB_DUPSORT|MDB_DUPFIXED,&fixed));put(w,fixed,"key","aa");OK(mdb_cursor_open(w,fixed,&cursor));OK(mdb_cursor_get(cursor,&k,&v,MDB_FIRST));OK(mdb_drop(w,fixed,0));REQUIRE(mdb_cursor_get(cursor,&k,&v,MDB_GET_CURRENT)!=0);put(w,fixed,"key","bbbb");OK(mdb_cursor_get(cursor,&k,&v,MDB_NEXT));REQUIRE(v.mv_size==4&&!memcmp(v.mv_data,"bbbb",4));mdb_cursor_close(cursor);
    OK(mdb_dbi_open(w,"named",MDB_CREATE,&named));put(w,named,"key","original");
    char longname[301];memset(longname,'q',300);longname[300]=0;OK(mdb_dbi_open(w,longname,MDB_CREATE,&long_db));put(w,long_db,"key","long-name");
    OK(mdb_dbi_open(w,NULL,0,&main_db));put(w,main_db,"0-user","main");OK(mdb_txn_commit(w));
    OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&r));k=(MDB_val){8,&newkey};OK(mdb_get(r,integer,&k,&v));k=text("key");OK(mdb_get(r,fixed,&k,&v));REQUIRE(v.mv_size==4);mdb_txn_abort(r);
    /* 0.9 permits clearing with cached named DBIs; modern LMDB reports BUSY.
       Roll the version-specific attempt back, then exercise the portable path. */
    OK(mdb_txn_begin(env,NULL,0,&w)); int cached_result=mdb_drop(w,main_db,0); REQUIRE(cached_result==0||cached_result==MDB_DBIS_BUSY);
    if(cached_result==0){OK(mdb_stat(w,main_db,&stats));REQUIRE(stats.ms_entries==0);} mdb_txn_abort(w);
    OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&r));k=text("key");OK(mdb_get(r,named,&k,&v));REQUIRE(v.mv_size==8&&!memcmp(v.mv_data,"original",8));mdb_txn_abort(r);
    /* Close cached handles before the durable clear, as required by modern LMDB. */
    mdb_dbi_close(env,integer);mdb_dbi_close(env,fixed);mdb_dbi_close(env,named);mdb_dbi_close(env,long_db);
    OK(mdb_txn_begin(env,NULL,0,&w));OK(mdb_txn_begin(env,w,0,&child));OK(mdb_drop(child,main_db,0));OK(mdb_stat(child,main_db,&stats));REQUIRE(stats.ms_entries==0);mdb_txn_abort(child);OK(mdb_stat(w,main_db,&stats));REQUIRE(stats.ms_entries==5);mdb_txn_abort(w);
    OK(mdb_txn_begin(env,NULL,0,&w));OK(mdb_drop(w,main_db,0));OK(mdb_stat(w,main_db,&stats));REQUIRE(stats.ms_entries==0);OK(mdb_txn_commit(w));
    OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&r));REQUIRE(mdb_dbi_open(r,"named",0,&named)==MDB_NOTFOUND);REQUIRE(mdb_dbi_open(r,longname,0,&long_db)==MDB_NOTFOUND);REQUIRE(mdb_dbi_open(r,"integer",0,&integer)==MDB_NOTFOUND);k=text("0-user");REQUIRE(mdb_get(r,main_db,&k,&v)==MDB_NOTFOUND);mdb_txn_abort(r);mdb_env_close(env);
    env=open_env(argv[1]);OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&r));OK(mdb_dbi_open(r,NULL,0,&main_db));OK(mdb_stat(r,main_db,&stats));REQUIRE(stats.ms_entries==0);REQUIRE(mdb_dbi_open(r,"named",0,&named)==MDB_NOTFOUND);REQUIRE(mdb_dbi_open(r,longname,0,&long_db)==MDB_NOTFOUND);mdb_txn_abort(r);
    OK(mdb_txn_begin(env,NULL,0,&w));OK(mdb_dbi_open(w,"named",MDB_CREATE,&named));put(w,named,"new","value");OK(mdb_txn_commit(w));mdb_env_close(env);puts("drop-ok");return 0;
}
