/* Separate public differential error-lifetime fixture, not the 314 success
 * corpus. Compile unchanged against each pinned header. The driver calibrates
 * an actual B.finish data-msync ordinal independently for every provider. */
#define _GNU_SOURCE
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
static void need(int yes,const char *what){if(!yes){fprintf(stderr,"oracle: %s\n",what);exit(1);}}
static void ok(int rc,const char *what){if(rc){fprintf(stderr,"%s=%d\n",what,rc);exit(1);}}
static void marker(const char *name,int begin){fprintf(stderr,"@@%s %s@@\n",begin?"BEGIN":"END",name);fflush(stderr);}
static MDB_val bytes(const char *p){MDB_val v={strlen(p),(void*)p};return v;}
static void put(MDB_txn *t,MDB_dbi db,const char *p){MDB_val k=bytes("k"),v=bytes(p);ok(mdb_put(t,db,&k,&v,0),"put");}
static void expect(MDB_txn *t,MDB_dbi db,size_t id,char value){MDB_val k=bytes("k"),v;ok(mdb_get(t,db,&k,&v),"get");need(v.mv_size==1&&*(char*)v.mv_data==value,"selected bytes");need((size_t)mdb_txn_id(t)==id,"selected id");}
int main(int argc,char **argv){
 need(argc==4,"usage DIRECTORY ASYNC EXPECT_EIO");setvbuf(stdout,NULL,_IONBF,0);
 need(!mkdir(argv[1],0700)||errno==EEXIST,"directory");int async=atoi(argv[2]),eio=atoi(argv[3]);
 unsigned flags=MDB_NOTLS|MDB_WRITEMAP|(async?MDB_MAPASYNC:0);MDB_env *e;MDB_txn *t,*old,*read;MDB_dbi db;MDB_cursor *cursor;
 ok(mdb_env_create(&e),"create");ok(mdb_env_set_mapsize(e,4u<<20),"mapsize");ok(mdb_env_open(e,argv[1],flags,0600),"open");
 marker("seed.commit",1);ok(mdb_txn_begin(e,NULL,0,&t),"seed begin");ok(mdb_dbi_open(t,NULL,0,&db),"seed MAIN");put(t,db,"A");ok(mdb_txn_commit(t),"seed commit");marker("seed.commit",0);
 /* DBI was exported by a committed seed. Neither overlapping reader opens DBIs. */
 ok(mdb_txn_begin(e,NULL,MDB_RDONLY,&old),"old begin");MDB_val k=bytes("k"),borrow;ok(mdb_get(old,db,&k,&borrow),"old get");
 ok(mdb_cursor_open(old,db,&cursor),"old cursor");MDB_val ck,cv;ok(mdb_cursor_get(cursor,&ck,&cv,MDB_FIRST),"old first");
 ok(mdb_txn_begin(e,NULL,0,&t),"B begin");put(t,db,"B");static char large[8192];memset(large,'L',sizeof large);k=bytes("large");MDB_val v={sizeof large,large};ok(mdb_put(t,db,&k,&v,0),"B large");
 marker("B.finish",1);int rc=mdb_txn_commit(t);marker("B.finish",0);printf("B.commit=%d\n",rc);need(rc==(eio?EIO:0),"calibrated data-msync error");
 need(borrow.mv_size==1&&*(char*)borrow.mv_data=='A',"old borrowed bytes");ok(mdb_cursor_get(cursor,&ck,&cv,MDB_GET_CURRENT),"old current");need(cv.mv_size==1&&*(char*)cv.mv_data=='A',"old cursor bytes");
 size_t selected=eio?1:2;ok(mdb_txn_begin(e,NULL,MDB_RDONLY,&read),"post-error reader");expect(read,db,selected,eio?'A':'B');mdb_txn_abort(read);printf("selected=%zu old.borrow=A old.cursor=A\n",selected);
 t=NULL;rc=mdb_txn_begin(e,NULL,0,&t);printf("next.begin=%d\n",rc);
 if(!rc){size_t id=(size_t)mdb_txn_id(t);printf("next.id=%zu\n",id);put(t,db,"C");rc=mdb_txn_commit(t);printf("next.commit=%d\n",rc);if(!rc){ok(mdb_txn_begin(e,NULL,MDB_RDONLY,&read),"next reader");expect(read,db,id,'C');mdb_txn_abort(read);}}
 need(borrow.mv_size==1&&*(char*)borrow.mv_data=='A',"old borrow after retry");mdb_cursor_close(cursor);mdb_txn_abort(old);mdb_env_close(e);
 /* Cold reopen opens its own DBI after all previous transactions are gone. */
 ok(mdb_env_create(&e),"cold create");ok(mdb_env_open(e,argv[1],flags,0600),"cold open");ok(mdb_txn_begin(e,NULL,MDB_RDONLY,&read),"cold reader");ok(mdb_dbi_open(read,NULL,0,&db),"cold MAIN");k=bytes("k");ok(mdb_get(read,db,&k,&v),"cold get");printf("cold.id=%zu cold.value=%c\n",(size_t)mdb_txn_id(read),*(char*)v.mv_data);mdb_txn_abort(read);mdb_env_close(e);puts("lifetime=complete");return 0;
}
