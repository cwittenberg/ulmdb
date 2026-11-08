#include "lmdb.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
static void ok(int rc){if(rc){fprintf(stderr,"rc=%d\n",rc);abort();}}
static void check(MDB_txn*t,MDB_dbi db,const char*key,size_t n,unsigned char byte){MDB_val k={strlen(key),(void*)key},v;ok(mdb_get(t,db,&k,&v));assert(v.mv_size==n);for(size_t i=0;i<n;i++)assert(((unsigned char*)v.mv_data)[i]==byte);}
int main(int argc,char**argv){assert(argc==2);MDB_env*e;MDB_txn*t,*child,*ro;MDB_dbi db;ok(mdb_env_create(&e));ok(mdb_env_set_mapsize(e,64u*1024u*1024u));ok(mdb_env_open(e,argv[1],MDB_NOTLS,0600));ok(mdb_txn_begin(e,0,0,&t));ok(mdb_dbi_open(t,0,0,&db));MDB_val k={3,"old"},v={1,"A"};ok(mdb_put(t,db,&k,&v,0));ok(mdb_txn_commit(t));ok(mdb_txn_begin(e,0,MDB_RDONLY,&ro));ok(mdb_txn_begin(e,0,0,&t));
 k=(MDB_val){7,"reserve"};v=(MDB_val){1024u*1024u,0};ok(mdb_put(t,db,&k,&v,MDB_RESERVE));unsigned char*p=v.mv_data;memset(p,0x41,v.mv_size);MDB_val found;ok(mdb_get(t,db,&k,&found));assert(found.mv_data==p&&found.mv_size==v.mv_size);
 MDB_cursor*c;ok(mdb_cursor_open(t,db,&c));MDB_val ck=k,cv;ok(mdb_cursor_get(c,&ck,&cv,MDB_SET));assert(cv.mv_size==v.mv_size&&!memcmp(cv.mv_data,p,v.mv_size));memset(p,0x42,v.mv_size);check(t,db,"reserve",v.mv_size,0x42);ck=k;ok(mdb_cursor_get(c,&ck,&cv,MDB_SET));assert(((unsigned char*)cv.mv_data)[17]==0x42);mdb_cursor_close(c);
 ok(mdb_txn_begin(e,t,0,&child));MDB_val childval={32,0};ok(mdb_put(child,db,&k,&childval,MDB_RESERVE));memset(childval.mv_data,0x43,32);check(child,db,"reserve",32,0x43);mdb_txn_abort(child);check(t,db,"reserve",1024u*1024u,0x42);
 ok(mdb_txn_begin(e,t,MDB_RDONLY,&child));check(child,db,"reserve",1024u*1024u,0x42);mdb_txn_abort(child);
 MDB_val fail={99,0};assert(mdb_put(t,db,&k,&fail,MDB_RESERVE|MDB_NOOVERWRITE)==MDB_KEYEXIST);assert(fail.mv_size==1024u*1024u);check(t,db,"reserve",fail.mv_size,0x42);
 MDB_val z={0,0};MDB_val zk={4,"zero"};ok(mdb_put(t,db,&zk,&z,MDB_RESERVE));check(t,db,"zero",0,0);ok(mdb_txn_commit(t));assert(mdb_get(ro,db,&k,&found)==MDB_NOTFOUND);mdb_txn_abort(ro);
 ok(mdb_txn_begin(e,0,MDB_RDONLY,&ro));check(ro,db,"reserve",1024u*1024u,0x42);mdb_txn_abort(ro);mdb_env_close(e);
 ok(mdb_env_create(&e));ok(mdb_env_open(e,argv[1],MDB_NOTLS,0600));ok(mdb_txn_begin(e,0,MDB_RDONLY,&ro));ok(mdb_dbi_open(ro,0,0,&db));check(ro,db,"reserve",1024u*1024u,0x42);mdb_txn_abort(ro);mdb_env_close(e);puts("reserve pointer/read/cursor/child/private/zero/reopen=ok");return 0;}
