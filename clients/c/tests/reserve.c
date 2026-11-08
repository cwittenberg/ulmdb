/* Standard LMDB writable-reserve lifetime scenario. Compile unchanged against
 * upstream or ULMDB; reads cannot retire a still-live writable view. */
#include <lmdb.h>
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define REQUIRE(x) do{if(!(x)){fprintf(stderr,"%s:%d: %s\n",__FILE__,__LINE__,#x);exit(1);}}while(0)
#define OK(x) do{int r_=(x);if(r_){fprintf(stderr,"%s:%d: %s: %d %s\n",__FILE__,__LINE__,#x,r_,mdb_strerror(r_));exit(1);}}while(0)
static MDB_val text(const char*s){MDB_val v={strlen(s),(void*)s};return v;}
static void equals(MDB_val v,const char*s){REQUIRE(v.mv_size==strlen(s));REQUIRE(!memcmp(v.mv_data,s,v.mv_size));}
static MDB_val get(MDB_txn*t,MDB_dbi d,const char*k,const char*expected){MDB_val key=text(k),value;OK(mdb_get(t,d,&key,&value));equals(value,expected);return value;}
static void* reserve(MDB_txn*t,MDB_dbi d,const char*k,size_t size){MDB_val key=text(k),value={size,NULL};OK(mdb_put(t,d,&key,&value,MDB_RESERVE));REQUIRE(value.mv_size==size);return value.mv_data;}
static int casecmp(const MDB_val*a,const MDB_val*b){size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;for(size_t i=0;i<n;i++){int x=tolower(((const unsigned char*)a->mv_data)[i]),y=tolower(((const unsigned char*)b->mv_data)[i]);if(x!=y)return x-y;}return (a->mv_size>b->mv_size)-(a->mv_size<b->mv_size);}
int main(int argc,char**argv){
    REQUIRE(argc==2);MDB_env*env;MDB_txn*t,*child;MDB_dbi db,custom;MDB_cursor*c;MDB_val key,value,retained;void*pending;
    OK(mdb_env_create(&env));OK(mdb_env_set_maxdbs(env,8));OK(mdb_env_open(env,argv[1],MDB_NOTLS,0600));OK(mdb_txn_begin(env,NULL,0,&t));OK(mdb_dbi_open(t,"records",MDB_CREATE,&db));
    pending=reserve(t,db,"key",3);memcpy(pending,"one",3);retained=get(t,db,"key","one");memcpy(pending,"two",3);equals(retained,"two");get(t,db,"key","two");
    OK(mdb_cursor_open(t,db,&c));key=text("key");OK(mdb_cursor_get(c,&key,&value,MDB_SET));equals(value,"two");retained=value;memcpy(pending,"six",3);equals(retained,"six");OK(mdb_cursor_get(c,&key,&value,MDB_GET_CURRENT));equals(value,"six");mdb_cursor_close(c);memcpy(pending,"end",3);get(t,db,"key","end");
    OK(mdb_txn_commit(t));OK(mdb_txn_begin(env,NULL,0,&t));get(t,db,"key","end");
    /* The next explicit mutation retires prior staging and must not let it
       overwrite a replacement or resurrect a deletion during a later read. */
    pending=reserve(t,db,"key",3);memcpy(pending,"old",3);key=text("key");value=text("replacement");OK(mdb_put(t,db,&key,&value,0));get(t,db,"key","replacement");
    pending=reserve(t,db,"deleted",3);memcpy(pending,"old",3);key=text("deleted");OK(mdb_del(t,db,&key,NULL));REQUIRE(mdb_get(t,db,&key,&value)==MDB_NOTFOUND);
    /* Ordinary existing-db opens and cursor reads preserve the writable view. */
    pending=reserve(t,db,"parent",3);memcpy(pending,"aaa",3);get(t,db,"parent","aaa");MDB_dbi reopened;OK(mdb_dbi_open(t,"records",0,&reopened));memcpy(pending,"bbb",3);get(t,db,"parent","bbb");
    OK(mdb_txn_begin(env,t,0,&child));pending=reserve(child,db,"parent",3);memcpy(pending,"ccc",3);get(child,db,"parent","ccc");memcpy(pending,"ddd",3);OK(mdb_txn_commit(child));get(t,db,"parent","ddd");
    OK(mdb_dbi_open(t,"custom",MDB_CREATE,&custom));OK(mdb_set_compare(t,custom,casecmp));pending=reserve(t,custom,"Alpha",3);memcpy(pending,"one",3);retained=get(t,custom,"alpha","one");memcpy(pending,"two",3);equals(retained,"two");get(t,custom,"ALPHA","two");
    OK(mdb_cursor_open(t,custom,&c));key=text("alpha");OK(mdb_cursor_get(c,&key,&value,MDB_SET));equals(value,"two");memcpy(pending,"end",3);OK(mdb_cursor_get(c,&key,&value,MDB_GET_CURRENT));equals(value,"end");mdb_cursor_close(c);
    OK(mdb_txn_commit(t));OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&t));get(t,db,"key","replacement");get(t,db,"parent","ddd");get(t,custom,"alpha","end");mdb_txn_abort(t);mdb_env_close(env);puts("reserve-ok");return 0;
}
