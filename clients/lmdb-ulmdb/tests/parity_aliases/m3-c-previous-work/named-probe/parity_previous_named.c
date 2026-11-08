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
static void missing(MDB_txn *t,const char *name,const char *label) { MDB_dbi d=0;int c=mdb_dbi_open(t,name,0,&d);printf("%s=%d\n",label,c);need(c==MDB_NOTFOUND,label); }
static void comparator(MDB_txn *t,MDB_dbi d) { MDB_val a=val("a"),z=val("z");calls=0;int c=mdb_cmp(t,d,&a,&z);printf("callback.sign=%d calls=%u\n",(c>0)-(c<0),calls);need(c<0&&calls==1,"callback authority"); }
static void duplicates(MDB_txn *t,MDB_dbi d,const char *expected,const char *label) { MDB_cursor *c;ok(mdb_cursor_open(t,d,&c),"dup cursor");MDB_val k=val("k"),v;ok(mdb_cursor_get(c,&k,&v,MDB_SET),"dup set");size_t n;ok(mdb_cursor_count(c,&n),"dup count");printf("%s count=%zu values=",label,n);size_t i=0;int terminal;do{need(v.mv_size==1&&i<strlen(expected)&&*(char*)v.mv_data==expected[i],label);putchar(*(char*)v.mv_data);i++;}while((terminal=mdb_cursor_get(c,&k,&v,MDB_NEXT_DUP))==0);need(terminal==MDB_NOTFOUND,"duplicate terminal result");putchar('\n');need(i==strlen(expected)&&n==i,label);mdb_cursor_close(c); }

static void scenario(const char *path,int held) {
    need(!mkdir(path,0700)||errno==EEXIST,"mkdir");
    unsigned options=held?MDB_NOTLS:0;
    MDB_env *e=open_env(path,options);MDB_txn *t=begin(e,0);
    MDB_dbi main=db(t,NULL,0),stable=db(t,"stable",MDB_CREATE),dups=db(t,"dups",MDB_CREATE|MDB_DUPSORT|MDB_DUPFIXED);
    ok(mdb_set_compare(t,stable,bytes_compare),"seed comparator before data access");
    put(t,main,"k","A");put(t,stable,"k","old");put(t,dups,"k","a");put(t,dups,"k","b");ok(mdb_txn_commit(t),"seed A");
    t=begin(e,0);put(t,main,"k","B");put(t,stable,"k","new");put(t,dups,"k","c");MDB_dbi future=db(t,"future",MDB_CREATE|MDB_REVERSEKEY);put(t,future,"k","future");ok(mdb_txn_commit(t),"seed B");mdb_env_close(e);
    e=open_env(path,options|MDB_PREVSNAPSHOT);
    t=begin(e,MDB_RDONLY);main=db(t,NULL,0);stable=db(t,"stable",0);dups=db(t,"dups",0);
    ok(mdb_set_compare(t,stable,bytes_compare),"set comparator before data access");
    printf("initial.id=%zu\n",(size_t)mdb_txn_id(t));need(mdb_txn_id(t)==1,"A id");
    value(t,main,"k","A","initial.main");value(t,stable,"k","old","initial.named");
    flags(t,stable,0,"initial.kv.flags");flags(t,dups,MDB_DUPSORT|MDB_DUPFIXED,"initial.dup.flags");duplicates(t,dups,"ab","initial.dups");missing(t,"future","initial.future");
    comparator(t,stable);ok(mdb_txn_commit(t),"export readonly DBIs");
    MDB_txn *old=NULL;MDB_cursor *old_cursor=NULL;MDB_val borrowed={0};
    if(held){old=begin(e,MDB_RDONLY);MDB_val k=val("k");ok(mdb_get(old,stable,&k,&borrowed),"borrow A");ok(mdb_cursor_open(old,dups,&old_cursor),"old dup cursor");MDB_val v;ok(mdb_cursor_get(old_cursor,&k,&v,MDB_SET),"old dup position");}
    t=begin(e,0);MDB_dbi alias=db(t,"stable",MDB_DUPSORT);need(alias==stable,"existing alias ignores requested kind");flags(t,alias,0,"alias.original.flags");comparator(t,stable);
    put(t,main,"first","C1");put(t,stable,"k","first");put(t,dups,"k","x");MDB_dbi branch1=db(t,"branch1",MDB_CREATE|MDB_REVERSEKEY);put(t,branch1,"k","branch1");
    printf("first.writer=%zu\n",(size_t)mdb_txn_id(t));need(mdb_txn_id(t)==2,"C1 id");ok(mdb_txn_prepare(t),"first prepare");ok(mdb_txn_commit(t),"first commit");
    /* No transaction ever uses this numeric handle after close. */
    mdb_dbi_close(e,branch1);
    t=begin(e,MDB_RDONLY);printf("offset.id=%zu\n",(size_t)mdb_txn_id(t));need(mdb_txn_id(t)==1,"offset A id");stable=db(t,"stable",0);dups=db(t,"dups",0);value(t,stable,"k","old","offset.named");value(t,main,"first",NULL,"offset.first");duplicates(t,dups,"ab","offset.dups");missing(t,"branch1","offset.branch1");comparator(t,stable);ok(mdb_txn_commit(t),"end offset reader");
    t=begin(e,0);stable=db(t,"stable",0);dups=db(t,"dups",0);value(t,stable,"k","old","second.base");put(t,main,"second","C2");put(t,stable,"k","second");put(t,dups,"k","y");MDB_dbi branch2=db(t,"branch2",MDB_CREATE|MDB_INTEGERKEY);uint64_t number=7;MDB_val key={sizeof number,&number},data=val("branch2");ok(mdb_put(t,branch2,&key,&data,0),"branch2 integer put");printf("second.writer=%zu\n",(size_t)mdb_txn_id(t));need(mdb_txn_id(t)==2,"C2 reused id");ok(mdb_txn_commit(t),"second commit");
    if(held){need(borrowed.mv_size==3&&!memcmp(borrowed.mv_data,"old",3),"borrowed A survived");value(old,stable,"k","old","held.named");MDB_val k,v;ok(mdb_cursor_get(old_cursor,&k,&v,MDB_GET_CURRENT),"held dup current");need(v.mv_size==1&&*(char*)v.mv_data=='a',"held dup bytes");size_t count;ok(mdb_cursor_count(old_cursor,&count),"held dup count");need(count==2,"held dup count");mdb_cursor_close(old_cursor);mdb_txn_abort(old);puts("held.snapshot=preserved");}
    t=begin(e,MDB_RDONLY);printf("canonical.id=%zu\n",(size_t)mdb_txn_id(t));need(mdb_txn_id(t)==2,"canonical C2 id");stable=db(t,"stable",0);dups=db(t,"dups",0);value(t,stable,"k","second","canonical.named");value(t,main,"first",NULL,"canonical.first");value(t,main,"second","C2","canonical.second");duplicates(t,dups,"aby","canonical.dups");flags(t,dups,MDB_DUPSORT|MDB_DUPFIXED,"canonical.dup.flags");branch2=db(t,"branch2",0);flags(t,branch2,MDB_INTEGERKEY,"canonical.branch2.flags");missing(t,"branch1","canonical.branch1");missing(t,"future","canonical.future");comparator(t,stable);mdb_txn_abort(t);mdb_env_close(e);
    e=open_env(path,options|MDB_RDONLY);t=begin(e,MDB_RDONLY);stable=db(t,"stable",0);dups=db(t,"dups",0);ok(mdb_set_compare(t,stable,bytes_compare),"reopen comparator before data access");value(t,stable,"k","second","reopen.named");duplicates(t,dups,"aby","reopen.dups");missing(t,"branch1","reopen.branch1");missing(t,"future","reopen.future");mdb_txn_abort(t);mdb_env_close(e);
}
int main(int argc,char **argv){if(argc!=2)return 2;need(!mkdir(argv[1],0700)||errno==EEXIST,"root");puts("previous-named-version=2");for(int held=0;held<2;held++){char path[4096];int n=snprintf(path,sizeof path,"%s/held%d",argv[1],held);need(n>0&&(size_t)n<sizeof path,"path");printf("scenario=G03-named-held%d\n",held);scenario(path,held);}puts("previous-named-complete journeys=2");return 0;}
