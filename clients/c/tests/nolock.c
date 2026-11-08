/* Caller-serialized public API operations, including an independent process. */
#include "lmdb.h"
#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include <sys/wait.h>
#define OK(c) do{int rc=(c);if(rc){fprintf(stderr,"line%d %s =>%d\n",__LINE__,#c,rc);exit(1);}}while(0)
static unsigned common(void){return MDB_NOTLS|(getenv("LOCKFUL_CONTROL")?0:MDB_NOLOCK);}
static MDB_env *opened(const char*p,unsigned flags){MDB_env*e;OK(mdb_env_create(&e));OK(mdb_env_set_maxdbs(e,8));OK(mdb_env_set_mapsize(e,32u<<20));OK(mdb_env_open(e,p,common()|flags,0600));return e;}
static void put(MDB_env*e,MDB_dbi db,const char*v,int prepare){MDB_txn*t;MDB_val k={1,"k"},value={1,(void*)v};OK(mdb_txn_begin(e,0,0,&t));OK(mdb_put(t,db,&k,&value,0));if(prepare){OK(mdb_txn_prepare(t));OK(mdb_env_sync(e,1));}OK(mdb_txn_commit(t));}
static void expect(MDB_env*e,const char*v,size_t id){MDB_txn*t;MDB_dbi db;MDB_val k={1,"k"},value;OK(mdb_txn_begin(e,0,MDB_RDONLY,&t));OK(mdb_dbi_open(t,0,0,&db));assert(mdb_txn_id(t)==id);OK(mdb_get(t,db,&k,&value));assert(value.mv_size==1&&!memcmp(value.mv_data,v,1));OK(mdb_txn_commit(t));}
static void nofiles(const char*p){if(!(common()&MDB_NOLOCK))return;const char*names[]={".ulmdb-coordination","LMDB-WRITER","LMDB-ENV","LMDB-ENV-GATE","LMDB-VIEW","LMDB-VIEW-GATE","LMDB-READERS","lmdb-readers"};char name[2048];for(size_t i=0;i<sizeof(names)/sizeof(*names);i++){snprintf(name,sizeof(name),"%s/%s",p,names[i]);assert(access(name,F_OK)<0);}}
int main(int argc,char**argv){assert(argc==3);
 if(!strcmp(argv[1],"--peer")){MDB_env*e=opened(argv[2],0);MDB_txn*t;MDB_dbi db;OK(mdb_txn_begin(e,0,0,&t));OK(mdb_dbi_open(t,0,0,&db));MDB_val k={1,"k"},v={1,"R"};OK(mdb_put(t,db,&k,&v,0));OK(mdb_txn_commit(t));mdb_env_close(e);nofiles(argv[2]);return 0;}
char src[2048],base[2048],compact[2048],archive[2048],delta[2048];snprintf(src,sizeof(src),"%s/src",argv[2]);snprintf(base,sizeof(base),"%s/base",argv[2]);snprintf(compact,sizeof(compact),"%s/compact",argv[2]);snprintf(archive,sizeof(archive),"%s/archive",argv[2]);snprintf(delta,sizeof(delta),"%s/delta",argv[2]);MDB_env*e=opened(src,0);MDB_txn*t;MDB_dbi db;OK(mdb_txn_begin(e,0,0,&t));OK(mdb_dbi_open(t,0,0,&db));MDB_val k={1,"k"},v={1,"A"};OK(mdb_put(t,db,&k,&v,0));OK(mdb_txn_commit(t));
 if(!strcmp(argv[1],"previous")){put(e,db,"B",0);mdb_env_close(e);e=opened(src,MDB_PREVSNAPSHOT);expect(e,"A",1);MDB_txn*r;OK(mdb_txn_begin(e,0,MDB_RDONLY,&r));OK(mdb_dbi_open(r,0,0,&db));OK(mdb_txn_commit(r));put(e,db,"C",1);expect(e,"C",2);put(e,db,"D",0);expect(e,"D",3);mdb_env_close(e);e=opened(src,MDB_PREVSNAPSHOT|MDB_RDONLY);expect(e,"C",2);mdb_env_close(e);nofiles(src);puts("nolock-previous-ok");return 0;}
 OK(mdb_env_copy2(e,base,0));MDB_txn*old;MDB_cursor*cursor;OK(mdb_txn_begin(e,0,MDB_RDONLY,&old));OK(mdb_get(old,db,&k,&v));const void*borrow=v.mv_data;OK(mdb_cursor_open(old,db,&cursor));put(e,db,"B",0);put(e,db,"C",1);assert(*(const char*)borrow=='A');MDB_val ck,cv;OK(mdb_cursor_get(cursor,&ck,&cv,MDB_FIRST));assert(cv.mv_size==1&&*(char*)cv.mv_data=='A');OK(mdb_env_incr_dump(e,delta,1));OK(mdb_env_copy2(e,compact,MDB_CP_COMPACT));int fd=open(archive,O_CREAT|O_EXCL|O_RDWR,0600);assert(fd>=0);OK(mdb_env_copyfd2(e,fd,0));assert(fcntl(fd,F_GETFD)>=0);close(fd);MDB_env*target=opened(base,MDB_WRITEMAP);fd=open(delta,O_RDONLY);assert(fd>=0);OK(mdb_env_incr_loadfd(target,fd));assert(fcntl(fd,F_GETFD)>=0);close(fd);expect(target,"C",3);OK(mdb_env_rollback(target,3));expect(target,"B",2);mdb_env_close(target);mdb_cursor_close(cursor);mdb_txn_abort(old);OK(mdb_env_rollback(e,3));expect(e,"B",2);mdb_env_close(e);e=opened(src,0);expect(e,"B",2);
 /* No library operation overlaps the independently executed peer. The caller's
  * waitpid ordering supplies serialization; the environment stays open. */
 pid_t peer=fork();assert(peer>=0);if(!peer){execl(argv[0],argv[0],"--peer",src,(char*)NULL);_exit(127);}int status;assert(waitpid(peer,&status,0)==peer&&WIFEXITED(status)&&WEXITSTATUS(status)==0);expect(e,"R",3);mdb_env_close(e);e=opened(archive,MDB_NOSUBDIR);expect(e,"C",3);mdb_env_close(e);nofiles(src);nofiles(base);nofiles(compact);char restored[2060];snprintf(restored,sizeof(restored),"%s.ulmdb",archive);nofiles(restored);puts("nolock-current-ok: prepared/sync/borrow/cursor/raw+compact+FD+restore/delta/rollback/reopen");return 0;}
