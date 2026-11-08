#include "lmdb.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
static void ok(int rc) { if(rc) {fprintf(stderr,"unexpected rc=%d\n",rc);abort();} }
int main(int argc,char **argv) {
 assert(argc==2); assert(mdb_env_set_pagesize(NULL,4096)==22);
 for(unsigned ps=256;ps<=65536;ps*=2) {
  char path[4096];snprintf(path,sizeof(path),"%s/%u",argv[1],ps);assert(!mkdir(path,0700)||errno==EEXIST);
  MDB_env *e;MDB_txn*t;MDB_dbi main,dup;MDB_stat st;ok(mdb_env_create(&e));ok(mdb_env_set_maxdbs(e,8));ok(mdb_env_set_mapsize(e,64u*1024u*1024u));
  for(int bad=-1;bad<=1;bad++)assert(mdb_env_set_pagesize(e,bad)==22);
  assert(mdb_env_set_pagesize(e,257)==22);assert(mdb_env_set_pagesize(e,131072)==22);
  ok(mdb_env_set_pagesize(e,4096));ok(mdb_env_set_pagesize(e,(int)ps));ok(mdb_env_open(e,path,0,0600));
  assert(mdb_env_set_pagesize(e,(int)ps)==22);ok(mdb_env_stat(e,&st));assert(st.ms_psize==ps);
  int max=mdb_env_get_maxkeysize(e);assert(max==(int)ps/2-70);
  ok(mdb_txn_begin(e,NULL,0,&t));ok(mdb_dbi_open(t,NULL,0,&main));ok(mdb_dbi_open(t,"dups",MDB_CREATE|MDB_DUPSORT|MDB_DUPFIXED,&dup));
  unsigned char *longkey=malloc((size_t)max+1);memset(longkey,0x77,(size_t)max+1);MDB_val k={(size_t)max,longkey},v={5,"large"};ok(mdb_put(t,main,&k,&v,0));k.mv_size++;assert(mdb_put(t,main,&k,&v,0)==MDB_BAD_VALSIZE);k.mv_size--;
  unsigned n; for(n=0;n<180;n++){k.mv_size=sizeof(n);k.mv_data=&n;v.mv_size=sizeof(n);v.mv_data=&n;ok(mdb_put(t,main,&k,&v,0));MDB_val dk={3,"key"};ok(mdb_put(t,dup,&dk,&v,0));}
  ok(mdb_txn_commit(t));ok(mdb_txn_begin(e,NULL,MDB_RDONLY,&t));k.mv_size=(size_t)max;k.mv_data=longkey;ok(mdb_get(t,main,&k,&v));assert(v.mv_size==5&&!memcmp(v.mv_data,"large",5));MDB_cursor*c;ok(mdb_cursor_open(t,dup,&c));MDB_val dk={3,"key"};ok(mdb_cursor_get(c,&dk,&v,MDB_SET));size_t count;ok(mdb_cursor_count(c,&count));assert(count==180);mdb_cursor_close(c);mdb_txn_abort(t);mdb_env_close(e);
  ok(mdb_env_create(&e));ok(mdb_env_set_maxdbs(e,8));ok(mdb_env_set_pagesize(e,ps==4096?8192:4096));ok(mdb_env_open(e,path,MDB_RDONLY,0600));ok(mdb_env_stat(e,&st));assert(st.ms_psize==ps);assert(mdb_env_get_maxkeysize(e)==max);ok(mdb_txn_begin(e,NULL,MDB_RDONLY,&t));ok(mdb_dbi_open(t,NULL,0,&main));ok(mdb_get(t,main,&k,&v));assert(v.mv_size==5);mdb_txn_abort(t);mdb_env_close(e);free(longkey);
  printf("pagesize=%u maxkey=%d data=ok duplicates=180 reopen=ok\n",ps,max);
 }
 return 0;
}
