/* Public comparator contract plus bounded comparison work on a 1024-row tree.
 * Same source and executable for each provider. No provider/source detection. */
#include <lmdb.h>
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <sys/stat.h>
#define N 1024
static unsigned calls;
static void need(int x,const char*s){if(!x){fprintf(stderr,"ordering-index: %s\n",s);exit(1);}}
static void ok(int rc,const char*s){if(rc){fprintf(stderr,"%s: %d %s\n",s,rc,mdb_strerror(rc));exit(1);}}
static MDB_val text(const char*s){MDB_val v={strlen(s),(void*)s};return v;}
static int fold(const MDB_val*a,const MDB_val*b){++calls;size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;const unsigned char*x=a->mv_data,*y=b->mv_data;for(size_t i=0;i<n;i++){int d=tolower(x[i])-tolower(y[i]);if(d)return d;}return (a->mv_size>b->mv_size)-(a->mv_size<b->mv_size);}
static void env_open(const char*path,MDB_env**env){ok(mdb_env_create(env),"create");ok(mdb_env_set_mapsize(*env,64u*1024u*1024u),"mapsize");ok(mdb_env_set_maxdbs(*env,8),"maxdbs");ok(mdb_env_open(*env,path,MDB_NOTLS,0600),"open");}
static void register_order(MDB_txn*t,MDB_dbi kv,MDB_dbi dup){calls=0;ok(mdb_set_compare(t,kv,fold),"key comparator");ok(mdb_set_dupsort(t,dup,fold),"dup comparator");need(calls==0,"registration calls no comparisons");}
static void inspect(MDB_txn*t,MDB_dbi kv,MDB_dbi dup){MDB_val k=text("k1023"),v;calls=0;ok(mdb_get(t,kv,&k,&v),"equivalent get");need(v.mv_size==5&&!memcmp(v.mv_data,"value",5),"equivalent value");need(calls<128,"indexed equivalent get comparison bound");MDB_cursor*c;ok(mdb_cursor_open(t,kv,&c),"cursor");k=text("k0511");calls=0;ok(mdb_cursor_get(c,&k,&v,MDB_SET_RANGE),"equivalent range");need(k.mv_size==5&&!memcmp(k.mv_data,"K0511",5),"stored representative");need(calls<128,"indexed range comparison bound");mdb_cursor_close(c);k=text("key");v=text("V0511");ok(mdb_cursor_open(t,dup,&c),"dup cursor");calls=0;ok(mdb_cursor_get(c,&k,&v,MDB_GET_BOTH_RANGE),"duplicate equivalent range");need(v.mv_size==5&&!memcmp(v.mv_data,"v0511",5),"duplicate stored representative");need(calls<128,"indexed duplicate range comparison bound");mdb_cursor_close(c);puts("indexed_get_range_duplicate=ok");}
int main(int argc,char**argv){need(argc==2,"DATABASE_ROOT");MDB_env*e;MDB_txn*t;MDB_dbi kv,dup,num;MDB_val k,v;env_open(argv[1],&e);ok(mdb_txn_begin(e,NULL,0,&t),"write");ok(mdb_dbi_open(t,"custom",MDB_CREATE,&kv),"custom");ok(mdb_dbi_open(t,"dups",MDB_CREATE|MDB_DUPSORT,&dup),"dups");ok(mdb_dbi_open(t,"integer",MDB_CREATE|MDB_INTEGERKEY,&num),"integer");register_order(t,kv,dup);
 for(unsigned i=N;i--!=0;){char key[16],data[16];snprintf(key,sizeof key,"K%04u",i);snprintf(data,sizeof data,"v%04u",i);k=text(key);v=text("value");ok(mdb_put(t,kv,&k,&v,0),"put key");k=text("key");v=text(data);ok(mdb_put(t,dup,&k,&v,0),"put duplicate");uint32_t n=i;k.mv_size=sizeof n;k.mv_data=&n;v=text("number");ok(mdb_put(t,num,&k,&v,0),"put integer");}
 inspect(t,kv,dup);k=text("k0511");v=text("other");calls=0;need(mdb_put(t,kv,&k,&v,MDB_NOOVERWRITE)==MDB_KEYEXIST,"nooverwrite");need(calls<128,"indexed nooverwrite comparison bound");k=text("K1024");v=text("last");calls=0;ok(mdb_put(t,kv,&k,&v,MDB_APPEND),"append");need(calls<128,"indexed append comparison bound");k=text("K1000");need(mdb_put(t,kv,&k,&v,MDB_APPEND)==MDB_KEYEXIST,"append rejects earlier key");k=text("key");v=text("V0511");calls=0;need(mdb_put(t,dup,&k,&v,MDB_NODUPDATA)==MDB_KEYEXIST,"duplicate equality");need(calls<128,"indexed duplicate equality comparison bound");k=text("key");v=text("V0512");calls=0;ok(mdb_del(t,dup,&k,&v),"delete equivalent duplicate");need(calls<128,"indexed duplicate delete comparison bound");ok(mdb_txn_commit(t),"commit");mdb_env_close(e);
 env_open(argv[1],&e);ok(mdb_txn_begin(e,NULL,MDB_RDONLY,&t),"read");ok(mdb_dbi_open(t,"custom",0,&kv),"reopen custom");ok(mdb_dbi_open(t,"dups",0,&dup),"reopen dups");ok(mdb_dbi_open(t,"integer",0,&num),"reopen integer");register_order(t,kv,dup);inspect(t,kv,dup);MDB_cursor*c;ok(mdb_cursor_open(t,num,&c),"integer cursor");for(uint32_t i=0;i<N;i++){ok(mdb_cursor_get(c,&k,&v,i?MDB_NEXT:MDB_FIRST),"integer next");need(k.mv_size==4&&!memcmp(k.mv_data,&i,4),"unsigned integer order");}mdb_cursor_close(c);mdb_txn_abort(t);mdb_env_close(e);puts("ordering-index-ok");return 0;}
