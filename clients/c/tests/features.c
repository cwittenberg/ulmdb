/* Unchanged LMDB feature scenario: compile against upstream or the adapter. */
#include <lmdb.h>
#include <ctype.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define REQUIRE(x) do { if (!(x)) { fprintf(stderr,"%s:%d: %s\n",__FILE__,__LINE__,#x);exit(1); } } while (0)
#define OK(x) do {int rc_=(x);if(rc_){fprintf(stderr,"%s:%d: %s: %d %s\n",__FILE__,__LINE__,#x,rc_,mdb_strerror(rc_));exit(1);}}while(0)
static MDB_val val(const void *p,size_t n){MDB_val v={n,(void*)p};return v;}
static MDB_val text(const char *p){return val(p,strlen(p));}
static void equals(MDB_val v,const char *s){REQUIRE(v.mv_size==strlen(s));REQUIRE(!memcmp(v.mv_data,s,v.mv_size));}
static void put(MDB_txn*t,MDB_dbi d,const char*k,const char*v){MDB_val a=text(k),b=text(v);OK(mdb_put(t,d,&a,&b,0));}
static int casecmp(const MDB_val *a,const MDB_val *b){size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;for(size_t i=0;i<n;i++){int x=tolower(((const unsigned char*)a->mv_data)[i]),y=tolower(((const unsigned char*)b->mv_data)[i]);if(x!=y)return x-y;}return (a->mv_size>b->mv_size)-(a->mv_size<b->mv_size);}
static int descending(const MDB_val*a,const MDB_val*b){size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;int c=memcmp(a->mv_data,b->mv_data,n);return c?-c:(a->mv_size<b->mv_size)-(a->mv_size>b->mv_size);}
static void check_text_cursor(MDB_txn*t,MDB_dbi d,const char **keys,const char **values,size_t n){MDB_cursor*c;MDB_val k,v;OK(mdb_cursor_open(t,d,&c));for(size_t i=0;i<n;i++){OK(mdb_cursor_get(c,&k,&v,i?MDB_NEXT:MDB_FIRST));if(keys)equals(k,keys[i]);if(values)equals(v,values[i]);}REQUIRE(mdb_cursor_get(c,&k,&v,MDB_NEXT)==MDB_NOTFOUND);mdb_cursor_close(c);}
int main(int argc,char **argv){
    REQUIRE(argc==2);MDB_env*env;MDB_txn*w,*r;MDB_dbi reverse,integer,dups,rdups,custom,cdups,reserved,multiple;MDB_val k,v;MDB_cursor*c;unsigned flags;
    OK(mdb_env_create(&env));OK(mdb_env_set_mapsize(env,64u*1024u*1024u));OK(mdb_env_set_maxdbs(env,32));OK(mdb_env_open(env,argv[1],MDB_NOTLS,0600));OK(mdb_txn_begin(env,NULL,0,&w));
    OK(mdb_dbi_open(w,"reverse",MDB_CREATE|MDB_REVERSEKEY,&reverse));put(w,reverse,"ab","AB");put(w,reverse,"ba","BA");
    const char *rk[]={"ba","ab"};check_text_cursor(w,reverse,rk,NULL,2);k=text("ba");v=text("ab");REQUIRE(mdb_cmp(w,reverse,&k,&v)<0);
    OK(mdb_dbi_open(w,"integer",MDB_CREATE|MDB_INTEGERKEY,&integer));uint32_t numbers[]={257,2,100};for(size_t i=0;i<3;i++){k=val(&numbers[i],4);v=text("number");OK(mdb_put(w,integer,&k,&v,0));}
    OK(mdb_cursor_open(w,integer,&c));uint32_t sorted[]={2,100,257};for(size_t i=0;i<3;i++){OK(mdb_cursor_get(c,&k,&v,i?MDB_NEXT:MDB_FIRST));REQUIRE(k.mv_size==4);REQUIRE(!memcmp(k.mv_data,&sorted[i],4));}mdb_cursor_close(c);
    OK(mdb_dbi_open(w,"duplicates",MDB_CREATE|MDB_DUPSORT|MDB_DUPFIXED|MDB_INTEGERDUP,&dups));for(size_t i=0;i<3;i++){k=text("key");v=val(&numbers[i],4);OK(mdb_put(w,dups,&k,&v,0));}k=text("key");OK(mdb_get(w,dups,&k,&v));REQUIRE(!memcmp(v.mv_data,&sorted[0],4));OK(mdb_dbi_flags(w,dups,&flags));REQUIRE(flags==(MDB_DUPSORT|MDB_DUPFIXED|MDB_INTEGERDUP));
    OK(mdb_dbi_open(w,"reverse-duplicates",MDB_CREATE|MDB_DUPSORT|MDB_REVERSEDUP,&rdups));put(w,rdups,"key","ab");put(w,rdups,"key","ba");const char *rv[]={"ba","ab"};check_text_cursor(w,rdups,NULL,rv,2);
    OK(mdb_dbi_open(w,"custom",MDB_CREATE,&custom));OK(mdb_set_compare(w,custom,casecmp));put(w,custom,"Alpha","one");put(w,custom,"ALPHA","two");k=text("alpha");OK(mdb_get(w,custom,&k,&v));equals(v,"two");v=text("three");REQUIRE(mdb_put(w,custom,&k,&v,MDB_NOOVERWRITE)==MDB_KEYEXIST);equals(v,"two");const char *ck[]={"Alpha"};check_text_cursor(w,custom,ck,NULL,1);
    OK(mdb_dbi_open(w,"custom-dups",MDB_CREATE|MDB_DUPSORT,&cdups));OK(mdb_set_dupsort(w,cdups,descending));put(w,cdups,"key","a");put(w,cdups,"key","c");put(w,cdups,"key","b");const char *cv[]={"c","b","a"};check_text_cursor(w,cdups,NULL,cv,3);k=text("key");OK(mdb_get(w,cdups,&k,&v));equals(v,"c");
    OK(mdb_dbi_open(w,"reserved",MDB_CREATE,&reserved));k=text("key");v=val(NULL,5);OK(mdb_put(w,reserved,&k,&v,MDB_RESERVE));memcpy(v.mv_data,"hello",5);OK(mdb_get(w,reserved,&k,&v));equals(v,"hello");v=val(NULL,3);OK(mdb_put(w,reserved,&k,&v,MDB_RESERVE));memcpy(v.mv_data,"bye",3);
    OK(mdb_dbi_open(w,"multiple",MDB_CREATE|MDB_DUPSORT|MDB_DUPFIXED|MDB_INTEGERDUP,&multiple));OK(mdb_cursor_open(w,multiple,&c));uint32_t many[]={9,4,7};MDB_val batch[2]={{4,many},{3,NULL}};k=text("key");OK(mdb_cursor_put(c,&k,batch,MDB_MULTIPLE));REQUIRE(batch[1].mv_size==3);k=text("key");OK(mdb_cursor_get(c,&k,&v,MDB_SET));OK(mdb_cursor_get(c,&k,&v,MDB_GET_MULTIPLE));uint32_t expected[]={4,7,9};REQUIRE(v.mv_size==sizeof expected);REQUIRE(!memcmp(v.mv_data,expected,sizeof expected));REQUIRE(mdb_cursor_get(c,&k,&v,MDB_NEXT_MULTIPLE)==MDB_NOTFOUND);mdb_cursor_close(c);
    char longname[301]; memset(longname,'n',300); longname[300]=0; MDB_dbi longdb, main_db;
    OK(mdb_dbi_open(w,longname,MDB_CREATE,&longdb)); put(w,longdb,"long","kept");
    size_t oversized_length=(size_t)mdb_env_get_maxkeysize(env)+1; char *oversized=malloc(oversized_length+1); REQUIRE(oversized!=NULL);
    memset(oversized,'x',oversized_length); oversized[oversized_length]=0; MDB_dbi invalid_name;
    REQUIRE(mdb_dbi_open(w,oversized,MDB_CREATE,&invalid_name)==MDB_BAD_VALSIZE); free(oversized);
    OK(mdb_dbi_open(w,NULL,0,&main_db)); OK(mdb_cursor_open(w,main_db,&c));
    k=val(longname,300); OK(mdb_cursor_get(c,&k,&v,MDB_SET_RANGE)); REQUIRE(k.mv_size==300||k.mv_size==301); REQUIRE(!memcmp(k.mv_data,longname,300)); mdb_cursor_close(c);
    OK(mdb_txn_commit(w));OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&r));k=text("key");OK(mdb_get(r,reserved,&k,&v));equals(v,"bye");check_text_cursor(r,reverse,rk,NULL,2);OK(mdb_dbi_flags(r,integer,&flags));REQUIRE(flags==MDB_INTEGERKEY);mdb_txn_abort(r);mdb_env_close(env);
    OK(mdb_env_create(&env));OK(mdb_env_set_maxdbs(env,32));OK(mdb_env_open(env,argv[1],MDB_NOTLS,0600));OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&r));OK(mdb_dbi_open(r,"integer",0,&integer));OK(mdb_dbi_flags(r,integer,&flags));REQUIRE(flags==MDB_INTEGERKEY);OK(mdb_cursor_open(r,integer,&c));for(size_t i=0;i<3;i++){OK(mdb_cursor_get(c,&k,&v,i?MDB_NEXT:MDB_FIRST));REQUIRE(!memcmp(k.mv_data,&sorted[i],4));}mdb_cursor_close(c);OK(mdb_dbi_open(r,longname,0,&longdb)); k=text("long"); OK(mdb_get(r,longdb,&k,&v)); equals(v,"kept");
    mdb_txn_abort(r);
    OK(mdb_txn_begin(env,NULL,0,&w)); OK(mdb_dbi_open(w,longname,0,&longdb)); OK(mdb_drop(w,longdb,1)); OK(mdb_txn_commit(w));
    char aborted_name[301]; memset(aborted_name,'a',300); aborted_name[300]=0;
    OK(mdb_txn_begin(env,NULL,0,&w)); OK(mdb_dbi_open(w,aborted_name,MDB_CREATE,&longdb)); mdb_txn_abort(w);
    OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&r)); REQUIRE(mdb_dbi_open(r,longname,0,&longdb)==MDB_NOTFOUND); REQUIRE(mdb_dbi_open(r,aborted_name,0,&longdb)==MDB_NOTFOUND); mdb_txn_abort(r);
    mdb_env_close(env);puts("features-ok");return 0;
}
