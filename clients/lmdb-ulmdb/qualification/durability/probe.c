/* Unchanged public LMDB application, compiled once per pinned public header.
   Phase markers are observer annotations, not replacement durability calls. */
#define _GNU_SOURCE
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#define MODERN (MDB_VERSION_MAJOR >= 1)
static void phase(const char*s,int begin){fprintf(stderr,"@@%s %s@@\n",begin?"BEGIN":"END",s);fflush(stderr);}
static void need(int yes,const char*s){if(!yes){fprintf(stderr,"oracle: %s\n",s);exit(1);}}
static void ok(int rc,const char*s){if(rc){fprintf(stderr,"%s=%d %s\n",s,rc,mdb_strerror(rc));exit(1);}}
static unsigned policy(const char*s){if(!strcmp(s,"full"))return 0;if(!strcmp(s,"nosync"))return MDB_NOSYNC;if(!strcmp(s,"nometasync"))return MDB_NOMETASYNC;if(!strcmp(s,"mapasync"))return MDB_MAPASYNC;if(!strcmp(s,"nosync+nometasync"))return MDB_NOSYNC|MDB_NOMETASYNC;if(!strcmp(s,"nosync+mapasync"))return MDB_NOSYNC|MDB_MAPASYNC;if(!strcmp(s,"nometasync+mapasync"))return MDB_NOMETASYNC|MDB_MAPASYNC;if(!strcmp(s,"nosync+nometasync+mapasync"))return MDB_NOSYNC|MDB_NOMETASYNC|MDB_MAPASYNC;need(0,"policy name");return 0;}
static MDB_val text(const char*s){MDB_val v={strlen(s),(void*)s};return v;}
static void put(MDB_txn*t,MDB_dbi d,int large){MDB_val k=text("k"),v=text(large?"new":"old");ok(mdb_put(t,d,&k,&v,0),"put");if(large){static char bytes[8192];memset(bytes,'L',sizeof bytes);k=text("large");v=(MDB_val){sizeof bytes,bytes};ok(mdb_put(t,d,&k,&v,0),"large put");}}
static void inspect(MDB_env*e,size_t id,const char*want){MDB_txn*t;MDB_dbi d;ok(mdb_txn_begin(e,NULL,MDB_RDONLY,&t),"inspect begin");need((size_t)mdb_txn_id(t)==id,"inspect id");ok(mdb_dbi_open(t,NULL,0,&d),"inspect main");MDB_val k=text("k"),v;ok(mdb_get(t,d,&k,&v),"inspect get");need(v.mv_size==strlen(want)&&!memcmp(v.mv_data,want,v.mv_size),"inspect bytes");mdb_txn_abort(t);}
static void flags(MDB_env*e,const char*label){unsigned f;ok(mdb_env_get_flags(e,&f),"get flags");printf("flags.%s=%u\n",label,f);}
static void change(MDB_env*e,const char*p){phase("flags.change",1);ok(mdb_env_set_flags(e,MDB_NOSYNC|MDB_NOMETASYNC|MDB_MAPASYNC,0),"clear policies");ok(mdb_env_set_flags(e,policy(p),1),"set policy");phase("flags.change",0);flags(e,"changed");}
static void syncs(MDB_env*e,const char*state,int readonly){for(int force=0;force<2;force++){char name[96];snprintf(name,sizeof name,"sync.%s.force%d",state,force);phase(name,1);int rc=mdb_env_sync(e,force);phase(name,0);printf("%s=%d\n",name,rc);
#if MODERN
 int expected=readonly?MDB_IS_READONLY:0;
#else
 int expected=readonly?EACCES:0;
#endif
 need(rc==expected,"sync return");}}
int main(int argc,char**argv){if(argc!=8)return 2;setvbuf(stdout,NULL,_IONBF,0);const char*scenario=argv[4];int wr=atoi(argv[3]),abort_final=!strcmp(argv[7],"abort");unsigned tf=0;
#if MODERN
 tf=policy(argv[5]);need(!(tf&MDB_MAPASYNC),"documented transaction policy only");
#else
 if(strcmp(argv[5],"full")){puts("UNSUPPORTED transaction-local policy absent from this header contract");return 77;}
#endif
 need(!mkdir(argv[1],0700),"fresh directory");MDB_env*e;MDB_txn*t=NULL;
#if MODERN
 MDB_txn*child=NULL;
#endif
 MDB_dbi d;unsigned initial=policy(argv[2])|MDB_NOTLS|(wr?MDB_WRITEMAP:0);ok(mdb_env_create(&e),"create");ok(mdb_env_set_maxdbs(e,4),"maxdbs");ok(mdb_env_set_mapsize(e,4u*1024u*1024u),"mapsize");phase("env.open",1);ok(mdb_env_open(e,argv[1],initial,0600),"open");phase("env.open",0);flags(e,"opened");phase("seed.commit",1);ok(mdb_txn_begin(e,NULL,0,&t),"seed begin");ok(mdb_dbi_open(t,NULL,0,&d),"seed main");put(t,d,0);ok(mdb_txn_commit(t),"seed commit");phase("seed.commit",0);t=NULL;MDB_envinfo info;MDB_stat stat;ok(mdb_env_info(e,&info),"info");ok(mdb_env_stat(e,&stat),"stat");printf("published.id=%zu mapsize=%zu pagesize=%u published.pages=%zu modern=%d\n",(size_t)info.me_last_txnid,(size_t)info.me_mapsize,stat.ms_psize,(size_t)info.me_last_pgno+1,MODERN);
 if(!strcmp(scenario,"readonly")||!strcmp(scenario,"readonly-active")){mdb_env_close(e);ok(mdb_env_create(&e),"readonly create");phase("readonly.open",1);ok(mdb_env_open(e,argv[1],initial|MDB_RDONLY,0600),"readonly open");phase("readonly.open",0);if(!strcmp(scenario,"readonly-active"))ok(mdb_txn_begin(e,NULL,MDB_RDONLY,&t),"readonly begin");flags(e,"readonly");syncs(e,"readonly",1);change(e,"nosync");syncs(e,"readonly.changed",1);if(t)mdb_txn_abort(t);mdb_env_close(e);return 0;}
 if(!strcmp(scenario,"fault-sync")){phase("sync.fault.force1",1);int rc=mdb_env_sync(e,1);phase("sync.fault.force1",0);printf("sync.fault.force1=%d\n",rc);need(rc==EIO,"injected sync EIO");ok(mdb_txn_begin(e,NULL,0,&t),"writer after sync error");mdb_txn_abort(t);puts("sync-error.writer=usable");mdb_env_close(e);return 0;}
 if(!strcmp(scenario,"idle")){syncs(e,"idle",0);phase("flags.invalid-writemap",1);int rc=mdb_env_set_flags(e,MDB_WRITEMAP,!wr);phase("flags.invalid-writemap",0);printf("set.writemap=%d\n",rc);need(rc==EINVAL,"immutable writemap");inspect(e,1,"old");mdb_env_close(e);return 0;}
 phase("B.begin",1);ok(mdb_txn_begin(e,NULL,tf|(!strcmp(scenario,"prepare-readonly")?MDB_RDONLY:0),&t),"B begin");ok(mdb_dbi_open(t,NULL,0,&d),"B main");phase("B.begin",0);
#if MODERN
 unsigned public_flags=~0u;ok(mdb_txn_flags(t,&public_flags),"txn flags");printf("txn.public.flags=%u\n",public_flags);need(public_flags==(!strcmp(scenario,"prepare-readonly")?MDB_RDONLY:0),"txn exposes only RDONLY flag");
#endif

#if MODERN
 if(!strcmp(scenario,"prepare-clean")||!strcmp(scenario,"prepare-readonly")){phase("B.prepare.consumed",1);ok(mdb_txn_prepare(t),"consumed prepare");phase("B.prepare.consumed",0);t=NULL;syncs(e,"consumed-prepare",0);inspect(e,1,"old");ok(mdb_txn_begin(e,NULL,0,&t),"writer after consumed prepare");mdb_txn_abort(t);puts("prepare=consumed writer=usable");mdb_env_close(e);return 0;}
#endif
 if(strcmp(scenario,"sync-clean")){phase("B.put",1);put(t,d,1);phase("B.put",0);}
 if(!strcmp(scenario,"runtime-before")||!strcmp(scenario,"rollback-runtime-before"))change(e,argv[6]);
 if(!strncmp(scenario,"nested-",7)){
#if MODERN
  need(!wr,"nested writer requires non-WRITEMAP");ok(mdb_txn_begin(e,t,policy(argv[6]),&child),"nested begin");if(strcmp(scenario,"nested-clean"))put(child,d,1);phase("nested.finish",1);if(!strcmp(scenario,"nested-abort"))mdb_txn_abort(child);else ok(mdb_txn_commit(child),"nested commit");phase("nested.finish",0);child=NULL;
#else
  need(0,"modern nested policy only");
#endif
 }

#if MODERN
 size_t rollback_id=(size_t)mdb_txn_id(t);
#endif
 int prepared=!strcmp(scenario,"prepare")||!strcmp(scenario,"runtime-after-prepare")||!strcmp(scenario,"rollback-prepared")||!strcmp(scenario,"rollback-runtime-after-prepare");
 if(prepared){
#if MODERN
  phase("B.prepare",1);ok(mdb_txn_prepare(t),"prepare");phase("B.prepare",0);if(!strcmp(scenario,"runtime-after-prepare")||!strcmp(scenario,"rollback-runtime-after-prepare"))change(e,argv[6]);syncs(e,"prepared",0);
#else
  need(0,"prepare absent from this public header");
#endif
 }
 if(!strcmp(scenario,"sync-clean")||!strcmp(scenario,"sync-dirty")){syncs(e,scenario,0);mdb_txn_abort(t);t=NULL;inspect(e,1,"old");puts("unpublished.writer=not-selected");}
 else{phase("B.finish",1);if(abort_final)mdb_txn_abort(t);else ok(mdb_txn_commit(t),"B commit");phase("B.finish",0);t=NULL;
#if MODERN
 if(!strncmp(scenario,"rollback",8)){need(!abort_final,"rollback requires preceding commit");phase("rollback",1);ok(mdb_env_rollback(e,rollback_id),"rollback");phase("rollback",0);inspect(e,1,"old");puts("rollback=success published=1");syncs(e,"after-rollback",0);flags(e,"final");mdb_env_close(e);return 0;}
#endif
 inspect(e,abort_final?1:2,abort_final?"old":"new");puts(abort_final?"finish=abort published=1":"finish=commit published=2");if(!strcmp(scenario,"runtime-after-commit"))change(e,argv[6]);syncs(e,"after",0);}
 flags(e,"final");mdb_env_close(e);return 0;}
