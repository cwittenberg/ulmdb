/* Absolute IDs from a truly empty environment; no normalization by a seed ID.
 * The driver documents ULMDB-only absent-directory convenience separately.
 * Modern prepare calls use only live dirty owners; consumed pointers are nulled. */
#include <lmdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define MODERN (MDB_VERSION_MAJOR >= 1)
static void need(int yes, const char *s) { if (!yes) { fprintf(stderr,"first-id oracle: %s\n",s);exit(1); } }
static void ok(int rc, const char *s) { if(rc){fprintf(stderr,"%s=%d\n",s,rc);exit(1);} }
static MDB_val value(const char *s) { MDB_val v={strlen(s),(void*)s};return v; }
static MDB_env *open_env(const char *path, unsigned flags) {
 MDB_env *e;ok(mdb_env_create(&e),"create");ok(mdb_env_set_mapsize(e,16u*1024u*1024u),"mapsize");
 ok(mdb_env_set_maxdbs(e,4),"maxdbs");ok(mdb_env_open(e,path,flags,0600),"open");return e;
}
static MDB_txn *begin(MDB_env *e,unsigned flags,size_t id,const char *label) {
 MDB_txn *t;ok(mdb_txn_begin(e,NULL,flags,&t),"begin");size_t actual=(size_t)mdb_txn_id(t);
 printf("%s=%zu\n",label,actual);need(actual==id,label);return t;
}
static void info(MDB_env *e,size_t id,const char *label) { MDB_envinfo i;ok(mdb_env_info(e,&i),"info");printf("%s=%zu\n",label,(size_t)i.me_last_txnid);need((size_t)i.me_last_txnid==id,label); }
static MDB_dbi main_db(MDB_txn*t) { MDB_dbi d;ok(mdb_dbi_open(t,NULL,0,&d),"main");return d; }
static void put(MDB_txn*t,const char*s) { MDB_val k=value("k"),v=value(s);ok(mdb_put(t,main_db(t),&k,&v,0),"put"); }
static void inspect(MDB_env *e,size_t id,const char *expected,const char *label) {
 MDB_txn*t=begin(e,MDB_RDONLY,id,label);MDB_val k=value("k"),v;int rc=mdb_get(t,main_db(t),&k,&v);
 printf("read.value=%d",rc);if(!rc)printf(" value=%.*s",(int)v.mv_size,(char*)v.mv_data);putchar('\n');
 if(expected)need(!rc&&v.mv_size==strlen(expected)&&!memcmp(v.mv_data,expected,v.mv_size),"value");else need(rc==MDB_NOTFOUND,"empty");mdb_txn_abort(t);
}
int main(int argc,char**argv) {
 if(argc!=3)return 2;
 unsigned flags=(unsigned)strtoul(argv[2],NULL,0);MDB_env*e=open_env(argv[1],flags);MDB_txn*t;
 puts("first-ids-version=1");info(e,0,"initial.info");inspect(e,0,NULL,"initial.reader");
 t=begin(e,0,1,"clean.writer");main_db(t);ok(mdb_txn_commit(t),"clean commit");info(e,0,"clean.info");
 t=begin(e,0,1,"aborted.writer");put(t,"aborted");mdb_txn_abort(t);info(e,0,"abort.info");inspect(e,0,NULL,"abort.reader");
#if MODERN
 t=begin(e,0,1,"clean-prepare.writer");ok(mdb_txn_prepare(t),"clean prepare");t=NULL;info(e,0,"clean-prepare.info");
 t=begin(e,MDB_RDONLY,0,"readonly-prepare.reader");ok(mdb_txn_prepare(t),"readonly prepare");t=NULL;info(e,0,"readonly-prepare.info");
 t=begin(e,0,1,"abort-prepare.writer");put(t,"prepared-aborted");ok(mdb_txn_prepare(t),"dirty prepare abort");info(e,0,"abort-prepare.info");inspect(e,0,NULL,"abort-prepare.reader");mdb_txn_abort(t);t=NULL;
#endif
 mdb_env_close(e);e=open_env(argv[1],flags);info(e,0,"reopen-empty.info");inspect(e,0,NULL,"reopen-empty.reader");
 t=begin(e,0,1,"first-dirty.writer");put(t,"first");ok(mdb_txn_commit(t),"first commit");info(e,1,"first-commit.info");inspect(e,1,"first","first-commit.reader");
 mdb_env_close(e);e=open_env(argv[1],flags);info(e,1,"reopen-first.info");inspect(e,1,"first","reopen-first.reader");
 t=begin(e,0,2,"second-dirty.writer");put(t,"second");
#if MODERN
 ok(mdb_txn_prepare(t),"dirty prepare commit");info(e,1,"pending.info");inspect(e,1,"first","pending.reader");
#endif
 ok(mdb_txn_commit(t),"second commit");t=NULL;info(e,2,"second-commit.info");inspect(e,2,"second","second-commit.reader");
 mdb_env_close(e);e=open_env(argv[1],flags);info(e,2,"reopen-second.info");inspect(e,2,"second","reopen-second.reader");
 t=begin(e,0,3,"next.writer");mdb_txn_abort(t);mdb_env_close(e);puts("first-ids-complete");return 0;
}
