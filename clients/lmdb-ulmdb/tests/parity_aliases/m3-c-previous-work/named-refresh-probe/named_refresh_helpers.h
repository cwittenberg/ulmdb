/* Unchanged modern LMDB application. Every named DBI is properly opened;
 * closed branch handles are never reused. Old readonly cursors end explicitly. */
#include <lmdb.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

static void need(int yes, const char *what) { if(!yes){fprintf(stderr,"previous-named oracle: %s\n",what);exit(1);} }
static void ok(int code, const char *what) { if(code){fprintf(stderr,"%s: %d %s\n",what,code,mdb_strerror(code));exit(1);} }
static MDB_val val(const char *s) { MDB_val v={strlen(s),(void*)s};return v; }
static unsigned calls;
static int bytes_compare(const MDB_val *a,const MDB_val *b) { calls++;size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;int c=memcmp(a->mv_data,b->mv_data,n);return c?c:(a->mv_size>b->mv_size)-(a->mv_size<b->mv_size); }
static MDB_env *open_env(const char *path,unsigned flags) { MDB_env *e;ok(mdb_env_create(&e),"create");ok(mdb_env_set_maxdbs(e,12),"maxdbs");ok(mdb_env_set_mapsize(e,64u*1024u*1024u),"mapsize");ok(mdb_env_open(e,path,flags,0600),"open");return e; }
static MDB_txn *begin(MDB_env *e,unsigned flags) { MDB_txn *t;ok(mdb_txn_begin(e,NULL,flags,&t),"begin");return t; }
static MDB_dbi db(MDB_txn *t,const char *name,unsigned flags) { MDB_dbi d;ok(mdb_dbi_open(t,name,flags,&d),"dbi");return d; }
static void put(MDB_txn *t,MDB_dbi d,const char *key,const char *text) { MDB_val k=val(key),v=val(text);ok(mdb_put(t,d,&k,&v,0),"put"); }
static void value(MDB_txn *t,MDB_dbi d,const char *key,const char *expected,const char *label) { MDB_val k=val(key),v;int c=mdb_get(t,d,&k,&v);printf("%s=%d",label,c);if(!c)printf(" value=%.*s",(int)v.mv_size,(char*)v.mv_data);putchar('\n');if(expected)need(!c&&v.mv_size==strlen(expected)&&!memcmp(v.mv_data,expected,v.mv_size),label);else need(c==MDB_NOTFOUND,label); }
static void flags(MDB_txn *t,MDB_dbi d,unsigned expected,const char *label) { unsigned actual=~0u;ok(mdb_dbi_flags(t,d,&actual),"flags");printf("%s=%u\n",label,actual);need(actual==expected,label); }
static void comparator(MDB_txn *t,MDB_dbi d) { MDB_val a=val("a"),z=val("z");calls=0;int c=mdb_cmp(t,d,&a,&z);printf("callback.sign=%d calls=%u\n",(c>0)-(c<0),calls);need(c<0&&calls==1,"callback authority"); }

